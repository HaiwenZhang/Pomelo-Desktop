//! Each document owns cancellation and rejects results from earlier requests.

use std::{path::PathBuf, sync::Arc};

use pomelo_core::i18n::MessageKey as Key;
use pomelo_core::model::Diagnostic;
use pomelo_core::task::{
    CancellationToken, DocumentId, ImportProgress, LatestImportProgress, RequestId,
};
use pomelo_import::{ImportError, ImportedBoard};
use pomelo_render::copper::PreparedCopper;
use pomelo_render::tracks::{PrepareError, PreparedTracks};

pub struct PreparedDocument {
    pub label_index: Option<Arc<pomelo_render::text::msdf::LabelIndex>>,
    pub preview: Option<crate::services::preview::PreparedPreview>,
    pub restored_view: Option<Box<pomelo_core::view_state::ViewState>>,
    pub texts: Option<Arc<pomelo_render::text::msdf::PreparedGlyphs>>,
    pub drawings: Arc<PreparedTracks>,
    pub render_diagnostics: Vec<Diagnostic>,
    pub board: ImportedBoard,
    pub search: Arc<pomelo_core::search::SearchIndex>,
    pub picking: Arc<pomelo_core::picking_index::SegmentIndex>,
    pub tracks: Arc<PreparedTracks>,
    pub zone_outlines: Arc<PreparedTracks>,
    pub copper: Arc<PreparedCopper>,
    pub pads: Arc<pomelo_render::pads::PreparedPads>,
    pub drills: Arc<pomelo_render::pads::PreparedPads>,
}

pub const DIAGNOSTICS_PAGE_SIZE: usize = 32;

pub fn diagnostics_range(count: usize, page: usize) -> std::ops::Range<usize> {
    if count == 0 {
        return 0..0;
    }
    let page = page.min((count - 1) / DIAGNOSTICS_PAGE_SIZE);
    let start = page * DIAGNOSTICS_PAGE_SIZE;
    start..start.saturating_add(DIAGNOSTICS_PAGE_SIZE).min(count)
}

pub enum LoadError {
    Import(ImportError),
    Prepare(PrepareError),
    Picking(pomelo_core::picking_index::IndexError),
    Search(pomelo_core::search::CollationError),
}

impl LoadError {
    fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Import(error) => error.diagnostic(),
            Self::Prepare(error) => error.diagnostic(),
            Self::Search(error) => error.diagnostic(),
            Self::Picking(error) => {
                use pomelo_core::{
                    geometry::PathError, i18n::MessageKey, picking_index::IndexError,
                };
                match error {
                    IndexError::Allocation => Diagnostic::error(
                        "PICK_INDEX_ALLOCATION",
                        MessageKey::RenderPrepareAllocation,
                    ),
                    IndexError::Geometry(PathError::Cancelled) => {
                        Diagnostic::error("PICK_INDEX_CANCELLED", MessageKey::Cancelled)
                    }
                    IndexError::Geometry(PathError::Invalid(id)) => {
                        let mut diagnostic = Diagnostic::error(
                            "PICK_INDEX_INVALID",
                            MessageKey::RenderPrepareInvalid,
                        );
                        diagnostic.object = Some(*id);
                        diagnostic.message = diagnostic.message.arg("object", id.0);
                        diagnostic
                    }
                    IndexError::Geometry(PathError::PointLimit { actual, limit })
                    | IndexError::ByteLimit { actual, limit } => {
                        let mut diagnostic =
                            Diagnostic::error("PICK_INDEX_LIMIT", MessageKey::GeometryLimit);
                        diagnostic.message = diagnostic
                            .message
                            .arg("actual", *actual)
                            .arg("limit", *limit);
                        diagnostic
                    }
                }
            }
        }
    }
}

impl From<ImportError> for LoadError {
    fn from(error: ImportError) -> Self {
        Self::Import(error)
    }
}

impl From<PrepareError> for LoadError {
    fn from(error: PrepareError) -> Self {
        Self::Prepare(error)
    }
}

pub enum DocumentStatus {
    Queued,
    Reading,
    Imported(PreparedDocument),
    Failed(Diagnostic),
    Cancelled,
}

pub struct DocumentSession {
    pub reload_view: Option<pomelo_core::view_state::ViewState>,
    pub path: PathBuf,
    pub relocated_from: Option<PathBuf>,
    pub diagnostics_expanded: bool,
    pub diagnostics_page: usize,
    pub import_options: pomelo_import::ImportOptions,
    pub request: RequestId,
    pub cancellation: CancellationToken,
    pub status: DocumentStatus,
    pub progress_mailbox: LatestImportProgress,
    pub progress: Option<ImportProgress>,
    #[cfg(target_os = "windows")]
    pub viewport: Option<gpui_kit::Entity<crate::viewport::BoardViewport>>,
}

impl DocumentSession {
    pub fn new(id: DocumentId, path: PathBuf) -> Self {
        Self {
            reload_view: None,
            path,
            relocated_from: None,
            diagnostics_expanded: false,
            diagnostics_page: 0,
            import_options: pomelo_import::ImportOptions::default(),
            request: RequestId {
                document: id,
                generation: 1,
            },
            cancellation: CancellationToken::default(),
            status: DocumentStatus::Queued,
            progress_mailbox: LatestImportProgress::default(),
            progress: None,
            #[cfg(target_os = "windows")]
            viewport: None,
        }
    }

    pub fn begin_import(&mut self) -> bool {
        if !matches!(self.status, DocumentStatus::Queued) || self.cancellation.is_cancelled() {
            return false;
        }
        self.status = DocumentStatus::Reading;
        true
    }

    pub fn retry(&mut self) -> Result<bool, Diagnostic> {
        if !matches!(
            self.status,
            DocumentStatus::Cancelled | DocumentStatus::Failed(_)
        ) {
            return Ok(false);
        }
        self.reload()
    }

    /// Reload terminal documents with a new request while keeping path and options.
    pub fn reload(&mut self) -> Result<bool, Diagnostic> {
        if matches!(
            self.status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            return Ok(false);
        }
        let generation = self.request.generation.checked_add(1).ok_or_else(|| {
            Diagnostic::error(
                "REQUEST_GENERATION_EXHAUSTED",
                Key::RequestGenerationExhausted,
            )
        })?;
        self.cancellation.cancel();
        self.request.generation = generation;
        self.cancellation = CancellationToken::default();
        self.progress_mailbox = LatestImportProgress::default();
        self.progress = None;
        self.diagnostics_expanded = false;
        self.diagnostics_page = 0;
        self.status = DocumentStatus::Queued;
        #[cfg(target_os = "windows")]
        {
            self.viewport = None;
        }
        Ok(true)
    }

    pub fn set_retry_encoding(&mut self, encoding: pomelo_import::TextEncoding) -> bool {
        if !matches!(
            self.status,
            DocumentStatus::Cancelled | DocumentStatus::Failed(_)
        ) {
            return false;
        }
        self.import_options.text_encoding = encoding;
        true
    }

    pub fn complete_import(
        &mut self,
        request: RequestId,
        result: Result<PreparedDocument, LoadError>,
    ) -> bool {
        if request != self.request
            || self.cancellation.is_cancelled()
            || !matches!(self.status, DocumentStatus::Reading)
        {
            return false;
        }
        self.status = match result {
            Ok(board) => DocumentStatus::Imported(board),
            Err(
                LoadError::Import(ImportError::Cancelled)
                | LoadError::Prepare(PrepareError::Cancelled)
                | LoadError::Picking(pomelo_core::picking_index::IndexError::Geometry(
                    pomelo_core::geometry::PathError::Cancelled,
                )),
            ) => DocumentStatus::Cancelled,
            Err(error) => DocumentStatus::Failed(error.diagnostic().with_path(&self.path)),
        };
        true
    }

    pub fn cancel(&mut self) {
        self.cancellation.cancel();
        if matches!(
            self.status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            self.status = DocumentStatus::Cancelled;
        }
    }
}

impl Drop for DocumentSession {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared_empty_document() -> PreparedDocument {
        use pomelo_core::{
            model::{BoardScene, Bounds, Point},
            view_state::SourceIdentity,
        };
        let cancel = CancellationToken::default();
        let scene = Arc::new(BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            diagnostics: vec![],
            bounds: Bounds {
                min: Point::new(0.0, 0.0),
                max: Point::new(1.0, 1.0),
            },
        });
        let tracks = Arc::new(
            PreparedTracks::build_with_outline(
                &[],
                &[],
                pomelo_render::tracks::TraceLimits::default(),
                &cancel,
            )
            .unwrap(),
        );
        let pads = Arc::new(
            pomelo_render::pads::PreparedPads::build(
                &[],
                &[],
                pomelo_render::pads::PadLimits::default(),
                &cancel,
            )
            .unwrap(),
        );
        PreparedDocument {
            preview: None,
            restored_view: None,
            texts: None,
            label_index: None,
            drawings: Arc::clone(&tracks),
            zone_outlines: Arc::clone(&tracks),
            render_diagnostics: vec![],
            board: ImportedBoard {
                scene: Arc::clone(&scene),
                source: pomelo_import::FormatProbe {
                    format: "allegro".into(),
                    layout_version: 0,
                    writer_version: String::new(),
                    scene_supported: true,
                },
                identity: SourceIdentity {
                    sha256: [7; 32],
                    format: "allegro".into(),
                    encoding: "utf-8".into(),
                },
            },
            search: Arc::new(
                pomelo_core::search::SearchIndex::build(&scene, &cancel)
                    .unwrap()
                    .unwrap(),
            ),
            picking: Arc::new(
                pomelo_core::picking_index::SegmentIndex::build(scene, 10, &cancel).unwrap(),
            ),
            tracks,
            copper: Arc::new(
                PreparedCopper::build(&[], pomelo_render::copper::CopperLimits::default(), &cancel)
                    .unwrap(),
            ),
            pads: Arc::clone(&pads),
            drills: pads,
        }
    }

    #[test]
    fn imported_reload_releases_previous_scene_and_accepts_only_new_preparation() {
        let mut document = DocumentSession::new(DocumentId(7), PathBuf::from("board.brd"));
        assert!(document.begin_import());
        let previous = document.request;
        let board = prepared_empty_document();
        let previous_scene = Arc::downgrade(&board.board.scene);
        let previous_tracks = Arc::downgrade(&board.tracks);
        let previous_pads = Arc::downgrade(&board.pads);
        let previous_copper = Arc::downgrade(&board.copper);
        let previous_search = Arc::downgrade(&board.search);
        let previous_picking = Arc::downgrade(&board.picking);
        assert!(document.complete_import(previous, Ok(board)));
        assert!(document.reload().unwrap());
        assert!(previous_scene.upgrade().is_none());
        assert!(previous_tracks.upgrade().is_none());
        assert!(previous_pads.upgrade().is_none());
        assert!(previous_copper.upgrade().is_none());
        assert!(previous_search.upgrade().is_none());
        assert!(previous_picking.upgrade().is_none());
        assert!(document.begin_import());
        assert!(!document.complete_import(previous, Ok(prepared_empty_document())));
        assert!(document.complete_import(document.request, Ok(prepared_empty_document())));
        assert!(matches!(document.status, DocumentStatus::Imported(_)));
    }

    #[test]
    fn rejected_import_result_releases_prepared_scene_and_batches() {
        let mut document = DocumentSession::new(DocumentId(7), PathBuf::from("board.brd"));
        assert!(document.begin_import());
        let previous = document.request;
        document.cancel();
        assert!(document.reload().unwrap());
        assert!(document.begin_import());
        let stale = prepared_empty_document();
        let scene = Arc::downgrade(&stale.board.scene);
        let tracks = Arc::downgrade(&stale.tracks);
        let pads = Arc::downgrade(&stale.pads);
        let copper = Arc::downgrade(&stale.copper);
        let search = Arc::downgrade(&stale.search);
        let picking = Arc::downgrade(&stale.picking);

        assert!(!document.complete_import(previous, Ok(stale)));

        assert!(scene.upgrade().is_none());
        assert!(tracks.upgrade().is_none());
        assert!(pads.upgrade().is_none());
        assert!(copper.upgrade().is_none());
        assert!(search.upgrade().is_none());
        assert!(picking.upgrade().is_none());
        assert!(matches!(document.status, DocumentStatus::Reading));
    }

    #[test]
    fn reload_retains_identity_encoding_and_rejects_previous_request() {
        let mut document = DocumentSession::new(DocumentId(7), PathBuf::from("board.brd"));
        document.import_options.text_encoding = pomelo_import::TextEncoding::Windows1252;
        assert!(document.begin_import());
        let previous = document.request;
        let old_cancel = document.cancellation.clone();
        document.cancel();
        assert!(document.reload().unwrap());
        assert!(old_cancel.is_cancelled());
        assert_eq!(document.request.document, previous.document);
        assert_eq!(document.request.generation, previous.generation + 1);
        assert_eq!(document.path, PathBuf::from("board.brd"));
        assert!(matches!(
            document.import_options.text_encoding,
            pomelo_import::TextEncoding::Windows1252
        ));
        assert!(!document.reload().unwrap());
        assert!(document.begin_import());
        assert!(!document.complete_import(previous, Err(ImportError::Cancelled.into())));
        assert!(matches!(document.status, DocumentStatus::Reading));
    }

    #[test]
    fn diagnostic_pages_cover_every_source_row_and_clamp_stale_page_numbers() {
        let rows = (0..4)
            .flat_map(|page| diagnostics_range(97, page))
            .collect::<Vec<_>>();
        assert_eq!(rows, (0..97).collect::<Vec<_>>());
        assert_eq!(diagnostics_range(97, usize::MAX), 96..97);
        assert_eq!(diagnostics_range(0, usize::MAX), 0..0);
        let range = diagnostics_range(usize::MAX, usize::MAX);
        assert!(range.start <= range.end);
        assert_eq!(range.end, usize::MAX);
        assert!(range.len() <= DIAGNOSTICS_PAGE_SIZE);
    }

    #[test]
    fn encoding_change_requires_terminal_import_and_rejects_old_completion() {
        let mut session = DocumentSession::new(DocumentId(11), "board.brd".into());
        assert!(!session.set_retry_encoding(pomelo_import::TextEncoding::Windows1252));
        session.begin_import();
        let previous = session.request;
        assert!(!session.set_retry_encoding(pomelo_import::TextEncoding::Windows1252));
        session.complete_import(previous, Err(ImportError::InvalidEncoding(42).into()));
        assert!(session.set_retry_encoding(pomelo_import::TextEncoding::Windows1252));
        assert_eq!(session.request, previous);
        assert!(matches!(session.status, DocumentStatus::Failed(_)));
        session.retry().unwrap();
        assert_eq!(session.import_options.text_encoding.tag(), "windows-1252");
        session.begin_import();
        assert!(!session.complete_import(previous, Err(ImportError::InvalidEncoding(42).into())));
        assert_eq!(session.import_options.text_encoding.tag(), "windows-1252");
    }

    #[test]
    fn cancelled_document_encoding_change_does_not_change_other_documents() {
        let mut first = DocumentSession::new(DocumentId(12), "first.brd".into());
        let second = DocumentSession::new(DocumentId(13), "second.brd".into());
        first.cancel();
        assert!(first.set_retry_encoding(pomelo_import::TextEncoding::Big5));
        first.retry().unwrap();
        assert_eq!(first.import_options.text_encoding.tag(), "big5");
        assert_eq!(second.import_options.text_encoding.tag(), "utf-8");
        assert_eq!(second.request.generation, 1);
        assert!(matches!(second.status, DocumentStatus::Queued));
    }

    #[test]
    fn failed_relocation_retains_origin_across_retry_and_late_results() {
        let mut session = DocumentSession::new(DocumentId(14), "new.brd".into());
        let origin = PathBuf::from("old.brd");
        session.relocated_from = Some(origin.clone());
        session.begin_import();
        let previous = session.request;
        session.complete_import(previous, Err(ImportError::InvalidEncoding(42).into()));
        assert_eq!(session.relocated_from.as_ref(), Some(&origin));
        session.retry().unwrap();
        assert_eq!(session.relocated_from.as_ref(), Some(&origin));
        session.begin_import();
        assert!(!session.complete_import(previous, Err(ImportError::NoGeometry.into())));
        session.cancel();
        assert_eq!(session.relocated_from.as_ref(), Some(&origin));
    }

    #[test]
    fn retry_cancelled_import_isolates_late_progress_and_completion() {
        let mut session = DocumentSession::new(DocumentId(7), "board.brd".into());
        assert!(session.begin_import());
        let previous = session.request;
        let old_token = session.cancellation.clone();
        let old_progress = session.progress_mailbox.clone();
        session.cancel();
        assert!(session.retry().unwrap());
        assert_eq!(session.request.document, previous.document);
        assert_eq!(session.request.generation, previous.generation + 1);
        assert!(old_token.is_cancelled());
        assert!(!session.cancellation.is_cancelled());
        old_progress.publish(ImportProgress {
            stage: pomelo_core::task::ImportStage::PreparingGpu,
            completed: 99,
            total: Some(100),
        });
        assert!(session.progress_mailbox.take().is_none());
        assert!(session.progress.is_none());
        assert!(session.begin_import());
        assert!(!session.complete_import(previous, Err(ImportError::InvalidDivisor.into())));
        assert!(matches!(session.status, DocumentStatus::Reading));
        assert!(session.complete_import(session.request, Err(ImportError::NoGeometry.into())));
        assert!(matches!(session.status, DocumentStatus::Failed(_)));
    }

    #[test]
    fn retry_failed_import_retains_path_and_document_identity() {
        let mut session = DocumentSession::new(DocumentId(8), "测试 board.brd".into());
        session.import_options.text_encoding = pomelo_import::TextEncoding::Windows1252;
        session.begin_import();
        session.complete_import(session.request, Err(ImportError::InvalidDivisor.into()));
        assert!(session.retry().unwrap());
        assert_eq!(session.path, PathBuf::from("测试 board.brd"));
        assert_eq!(session.request.document, DocumentId(8));
        assert_eq!(session.import_options.text_encoding.tag(), "windows-1252");
        assert!(matches!(session.status, DocumentStatus::Queued));
    }

    #[test]
    fn retry_does_not_replace_queued_or_running_request() {
        let mut session = DocumentSession::new(DocumentId(9), "board.brd".into());
        let request = session.request;
        assert!(!session.retry().unwrap());
        assert!(session.begin_import());
        assert!(!session.retry().unwrap());
        assert_eq!(session.request, request);
        assert!(!session.cancellation.is_cancelled());
        assert!(matches!(session.status, DocumentStatus::Reading));
    }

    #[test]
    fn retry_generation_exhaustion_preserves_terminal_state() {
        let mut session = DocumentSession::new(DocumentId(10), "board.brd".into());
        session.request.generation = u64::MAX;
        session.cancel();
        let error = session.retry().unwrap_err();
        assert_eq!(error.code.as_ref(), "REQUEST_GENERATION_EXHAUSTED");
        assert_eq!(session.request.generation, u64::MAX);
        assert!(session.cancellation.is_cancelled());
        assert!(matches!(session.status, DocumentStatus::Cancelled));
    }

    #[test]
    fn cancelled_document_discards_late_failure() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        let request = session.request;
        session.cancel();
        assert!(!session.complete_import(request, Err(ImportError::InvalidDivisor.into())));
        assert!(matches!(session.status, DocumentStatus::Cancelled));
    }

    #[test]
    fn result_from_another_document_is_rejected() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        assert!(!session.complete_import(
            RequestId {
                document: DocumentId(2),
                generation: 1
            },
            Err(ImportError::InvalidDivisor.into())
        ));
    }

    #[test]
    fn cancelled_queued_document_never_starts_import() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.cancel();
        assert!(!session.begin_import());
    }

    #[test]
    fn cancellation_during_render_preparation_is_not_reported_as_failure() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        let request = session.request;
        assert!(session.complete_import(request, Err(PrepareError::Cancelled.into())));
        assert!(matches!(session.status, DocumentStatus::Cancelled));
    }

    #[test]
    fn terminal_result_cannot_be_overwritten_by_duplicate_completion() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        let request = session.request;
        assert!(session.complete_import(request, Err(ImportError::InvalidDivisor.into())));
        assert!(!session.complete_import(request, Err(ImportError::NoGeometry.into())));
        let DocumentStatus::Failed(error) = &session.status else {
            panic!("expected retained failure");
        };
        assert_eq!(error.code.as_ref(), "BRD_INVALID_DIVISOR");
    }

    #[test]
    fn result_from_previous_generation_is_rejected() {
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        let previous = session.request;
        session.request.generation += 1;
        assert!(!session.complete_import(previous, Err(ImportError::InvalidDivisor.into())));
        assert!(matches!(session.status, DocumentStatus::Reading));
    }

    #[test]
    fn retained_failure_changes_language_without_reimporting_or_losing_source_location() {
        use pomelo_core::i18n::Locale;
        let mut session = DocumentSession::new(DocumentId(1), "board.brd".into());
        session.begin_import();
        let request = session.request;
        session.complete_import(
            request,
            Err(ImportError::OutOfBounds {
                offset: 128,
                requested: 8,
                length: 130,
            }
            .into()),
        );
        let DocumentStatus::Failed(error) = &session.status else {
            panic!("expected failure");
        };
        assert_ne!(
            error.message.display(Locale::English),
            error.message.display(Locale::Japanese)
        );
        assert_eq!(error.offset, Some(128));
        assert_eq!(error.path.as_deref(), Some(session.path.as_path()));
        assert_eq!(session.request, request);
    }
}
