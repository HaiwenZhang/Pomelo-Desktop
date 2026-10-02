//! Native command surfaces and per-document asynchronous board imports.

use std::path::PathBuf;

use gpui_kit::base::{Disableable, Selectable};
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    menu::{AppMenuBar, DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
};
use gpui_kit::*;
use pomelo_core::task::{DocumentId, RequestId};
use pomelo_core::{
    i18n::{LanguagePreference, Locale, Message, MessageKey as Key, text},
    model::Diagnostic,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};

use crate::document::{DocumentSession, DocumentStatus, LoadError, PreparedDocument};
use crate::i18n::{self, LanguageState};
use pomelo_render::copper::{CopperLimits, PreparedCopper};
use pomelo_render::tracks::{PreparedTracks, TraceLimits};

use crate::actions::{
    CancelImport, CloseDocument, OpenFile, OpenSettings, ReloadDocument, ToggleTheme,
};

pub struct Workbench {
    documents: Vec<DocumentSession>,
    active: Option<usize>,
    next_id: u64,
    running_import: Option<RequestId>,
    focus: FocusHandle,
    tasks: Vec<Task<()>>,
    error: Option<Diagnostic>,
    menu_bar: Entity<AppMenuBar>,
    menu_revision: u64,
    import_options: ImportOptions,
    view_save_task: Option<Task<()>>,
}

impl Workbench {
    pub fn new(import_options: ImportOptions, cx: &mut Context<Self>) -> Self {
        cx.on_app_quit(|this, cx| {
            this.save_views(None, cx);
            let pending = this.view_save_task.take();
            async move {
                if let Some(pending) = pending {
                    pending.await;
                }
            }
        })
        .detach();
        Self {
            documents: Vec::new(),
            active: None,
            next_id: 1,
            running_import: None,
            focus: cx.focus_handle(),
            tasks: Vec::new(),
            error: None,
            menu_bar: AppMenuBar::new(cx),
            menu_revision: cx.global::<LanguageState>().revision,
            import_options,
            view_save_task: None,
        }
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_path_with_options(path, self.import_options.clone(), None, window, cx);
    }

    fn open_path_with_options(
        &mut self,
        path: PathBuf,
        options: ImportOptions,
        relocated_from: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = match std::path::absolute(&path) {
            Ok(path) => path,
            Err(error) => {
                self.error = Some(
                    pomelo_import::ImportError::Io(error)
                        .diagnostic()
                        .with_path(&path),
                );
                cx.notify();
                return;
            }
        };
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path == path)
        {
            self.active = Some(index);
            if let Some(origin) = relocated_from {
                let document = &mut self.documents[index];
                if let DocumentStatus::Imported(prepared) = &document.status {
                    crate::recent::remember(
                        crate::prefs::RecentEntry {
                            path: document.path.clone(),
                            format: prepared.board.source.format.clone(),
                            opened_unix_seconds: opened_time(),
                            encoding: document.import_options.text_encoding,
                        },
                        Some(&origin),
                        cx,
                    );
                } else {
                    document.relocated_from = Some(origin);
                }
            }
            cx.notify();
            return;
        }
        let Some(next_id) = self.next_id.checked_add(1) else {
            self.error = Some(Diagnostic::error(
                "DOCUMENT_ID_EXHAUSTED",
                Key::DocumentIdExhausted,
            ));
            cx.notify();
            return;
        };
        let mut session = DocumentSession::new(DocumentId(self.next_id), path.clone());
        session.import_options = options;
        session.relocated_from = relocated_from;
        self.next_id = next_id;
        self.active = Some(self.documents.len());
        self.documents.push(session);
        self.error = None;
        self.start_next_import(window, cx);
        cx.notify();
    }

    fn start_next_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running_import.is_some() {
            return;
        }
        let Some(document) = self
            .documents
            .iter_mut()
            .find(|document| matches!(document.status, DocumentStatus::Queued))
        else {
            return;
        };
        if !document.begin_import() {
            return;
        }
        let request = document.request;
        let cancellation = document.cancellation.clone();
        let path = document.path.clone();
        let mailbox = document.progress_mailbox.clone();
        self.running_import = Some(request);
        let options = document.import_options.clone();
        let reload_view = document.reload_view.clone();
        let import = cx.background_spawn(async move {
            let board = AllegroImporter.import(
                &path,
                &options,
                &ImportContext {
                    cancellation: &cancellation,
                    progress: &|progress| mailbox.publish(progress),
                },
            )?;
            use pomelo_core::task::{ImportProgress, ImportStage};
            let begin_stage = |stage| {
                if cancellation.is_cancelled() {
                    return Err(pomelo_render::tracks::PrepareError::Cancelled);
                }
                mailbox.publish(ImportProgress {
                    stage,
                    completed: 0,
                    total: None,
                });
                Ok(())
            };
            begin_stage(ImportStage::PreparingTracks)?;
            let tracks = PreparedTracks::build_with_outline(
                &board.scene.segments,
                &board.scene.outline,
                TraceLimits::default(),
                &cancellation,
            )?;
            let drawings = PreparedTracks::build_drawings(
                &board.scene.drawings,
                TraceLimits::default(),
                &cancellation,
            )?;
            begin_stage(ImportStage::PreparingTexts)?;
            let mut text_diagnostics = Vec::new();
            let mut prepared_text_ids = std::collections::BTreeSet::new();
            let texts = if board.scene.texts.is_empty() {
                None
            } else {
                let prepare_texts = || -> Result<_, pomelo_core::model::Diagnostic> {
                    let font = pomelo_render::text::StrokeFont::bundled_for_recovering_texts(
                        &board.scene.texts,
                        2_000_000,
                        4 * 1024 * 1024,
                        pomelo_render::text::FontLimits {
                            glyphs: 65_536,
                            encoded_bytes: 4 * 1024 * 1024,
                            points_per_glyph: 1024,
                            strokes: 1_000_000,
                        },
                        &cancellation,
                    )?;
                    let mut source = pomelo_render::text_instances::PreparedTextInstances::build(
                        &board.scene.texts,
                        &font,
                        1_000_000,
                        2_000_000,
                        512 * 1024 * 1024,
                        &cancellation,
                    )?;
                    let ids = source.objects.iter().copied().collect();
                    let diagnostics = std::mem::take(&mut source.summary.diagnostics);
                    Ok((source, diagnostics, ids))
                };
                match prepare_texts() {
                    Ok((source, diagnostics, ids)) => {
                        text_diagnostics.extend(diagnostics);
                        prepared_text_ids = ids;
                        Some(std::sync::Arc::new(source))
                    }
                    Err(mut diagnostic) => {
                        if cancellation.is_cancelled() {
                            return Err(LoadError::Prepare(
                                pomelo_render::tracks::PrepareError::Cancelled,
                            ));
                        }
                        diagnostic.severity = pomelo_core::model::Severity::Warning;
                        text_diagnostics.push(diagnostic);
                        None
                    }
                }
            };
            begin_stage(ImportStage::PreparingCopper)?;
            let copper =
                PreparedCopper::build(&board.scene.zones, CopperLimits::default(), &cancellation)?;
            begin_stage(ImportStage::PreparingPads)?;
            let mut pads = pomelo_render::pads::PreparedPads::build(
                &board.scene.pins,
                &board.scene.vias,
                pomelo_render::pads::PadLimits::default(),
                &cancellation,
            )?;
            pads.custom_mesh = Some(std::sync::Arc::new(pads.build_custom_meshes(
                CopperLimits::default(),
                &pomelo_core::copper::MeshLimits::default(),
                &cancellation,
            )?));
            begin_stage(ImportStage::PreparingDrills)?;
            let drills = pomelo_render::drills::PreparedDrills::build(
                &board.scene.pins,
                &board.scene.vias,
                pomelo_render::pads::PadLimits::default(),
                &cancellation,
            )?;
            begin_stage(ImportStage::BuildingSearch)?;
            let search = pomelo_core::search::SearchIndex::build(&board.scene, &cancellation)
                .ok_or(pomelo_render::tracks::PrepareError::Cancelled)?;
            begin_stage(ImportStage::BuildingPicking)?;
            let picking = pomelo_core::picking_index::SegmentIndex::build(
                board.scene.clone(),
                pomelo_render::tracks::TraceLimits::default().max_instances,
                &cancellation,
            )
            .map_err(LoadError::Picking)?;
            let restored_view_result = if let Some(state) = reload_view {
                state
                    .matching(&board.identity)
                    .map(|matched| matched.cloned())
            } else {
                crate::prefs::ViewStore::platform_default()
                    .map(|store| store.load_matching(&path, &board.identity))
                    .transpose()
                    .map(Option::flatten)
            };
            let restored_view = match restored_view_result {
                Ok(state) => state,
                Err(mut diagnostic) => {
                    diagnostic.severity = pomelo_core::model::Severity::Warning;
                    text_diagnostics.push(diagnostic);
                    None
                }
            };
            if cancellation.is_cancelled() {
                return Err(LoadError::Prepare(
                    pomelo_render::tracks::PrepareError::Cancelled,
                ));
            }
            Ok::<_, LoadError>(PreparedDocument {
                restored_view: restored_view.map(Box::new),
                drawings: std::sync::Arc::new(drawings),
                render_diagnostics: {
                    text_diagnostics.extend(pomelo_render::coverage::diagnostics_with_text_ids(
                        &board.scene,
                        &prepared_text_ids,
                    ));
                    text_diagnostics
                },
                texts,
                board,
                search: std::sync::Arc::new(search),
                picking: std::sync::Arc::new(picking),
                tracks: std::sync::Arc::new(tracks),
                copper: std::sync::Arc::new(copper),
                pads: std::sync::Arc::new(pads),
                drills: std::sync::Arc::new(drills.geometry),
            })
        });
        let executor = cx.background_executor().clone();
        let progress_task = cx.spawn_in(window, async move |this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(100)).await;
                let keep_polling = this
                    .update_in(cx, |this, _, cx| {
                        let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                        else {
                            return false;
                        };
                        if !matches!(document.status, DocumentStatus::Reading) {
                            return false;
                        }
                        if let Some(progress) = document.progress_mailbox.take() {
                            document.progress = Some(progress);
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = import.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(document) = this
                    .documents
                    .iter_mut()
                    .find(|document| document.request == request)
                    && document.complete_import(request, result)
                {
                    if let DocumentStatus::Imported(prepared) = &document.status {
                        let replaced_path = document.relocated_from.take();
                        crate::recent::remember(
                            crate::prefs::RecentEntry {
                                path: document.path.clone(),
                                format: prepared.board.source.format.clone(),
                                opened_unix_seconds: opened_time(),
                                encoding: document.import_options.text_encoding,
                            },
                            replaced_path.as_deref(),
                            cx,
                        );
                    }
                    #[cfg(target_os = "windows")]
                    if let DocumentStatus::Imported(prepared) = &document.status {
                        document.viewport =
                            Some(cx.new(|cx| {
                                crate::viewport::BoardViewport::new(prepared, window, cx)
                            }));
                    }
                }
                if this.running_import == Some(request) {
                    this.running_import = None;
                    this.start_next_import(window, cx);
                }
                cx.notify();
            });
        });
        self.tasks.retain(|task| !task.is_ready());
        self.tasks.push(progress_task);
        self.tasks.push(task);
    }

    fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(text(i18n::current(cx), Key::OpenPrompt).into()),
        });
        cx.spawn_in(window, async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update_in(cx, |this, window, cx| {
                    for path in paths {
                        this.open_path(path, window, cx);
                    }
                });
            }
            Ok(Ok(None)) => {}
            error => {
                let _ = this.update_in(cx, |this, _, cx| {
                    this.error = Some(
                        Diagnostic::error("FILE_DIALOG_FAILED", Key::FileDialogFailed)
                            .with_details(format!("{error:?}")),
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn save_views(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        #[cfg(target_os = "windows")]
        let entries: Vec<_> = self
            .documents
            .iter()
            .enumerate()
            .filter(|(position, _)| index.is_none_or(|index| index == *position))
            .filter_map(|(_, document)| {
                let DocumentStatus::Imported(prepared) = &document.status else {
                    return None;
                };
                let viewport = document.viewport.as_ref()?;
                Some(crate::prefs::ViewEntry {
                    path: document.path.clone(),
                    state: viewport.read(cx).snapshot(&prepared.board.identity)?,
                })
            })
            .collect();
        #[cfg(not(target_os = "windows"))]
        let entries: Vec<crate::prefs::ViewEntry> = {
            let _ = index;
            Vec::new()
        };
        if entries.is_empty() {
            return;
        }
        let previous = self.view_save_task.take();
        let store = crate::prefs::ViewStore::platform_default();
        self.view_save_task = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx
                .background_spawn(async move {
                    let store = store.ok_or_else(|| {
                        Diagnostic::error("CONFIG_DIRECTORY_MISSING", Key::ConfigDirectoryMissing)
                    })?;
                    store.save_updates(entries)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(error);
                    cx.notify();
                }
            });
        }));
    }

    fn close_document(&mut self, _: &CloseDocument, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.active {
            self.save_views(Some(index), cx);
            self.documents.remove(index);
            self.active = if self.documents.is_empty() {
                None
            } else {
                Some(index.min(self.documents.len() - 1))
            };
            cx.notify();
        }
    }

    fn reload_document(&mut self, _: &ReloadDocument, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        if matches!(
            self.documents[index].status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            return;
        }
        #[cfg(target_os = "windows")]
        let snapshot = match &self.documents[index].status {
            DocumentStatus::Imported(prepared) => self.documents[index]
                .viewport
                .as_ref()
                .and_then(|viewport| viewport.read(cx).snapshot(&prepared.board.identity)),
            _ => self.documents[index].reload_view.clone(),
        };
        self.save_views(Some(index), cx);
        match self.documents[index].reload() {
            Ok(true) => {
                #[cfg(target_os = "windows")]
                {
                    self.documents[index].reload_view = snapshot;
                }
                self.error = None;
                self.start_next_import(window, cx);
            }
            Ok(false) => return,
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn relocate_recent(
        &mut self,
        entry: crate::prefs::RecentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(text(i18n::current(cx), Key::RelocatePrompt).into()),
        });
        cx.spawn_in(window, async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        let mut options = this.import_options.clone();
                        options.text_encoding = entry.encoding;
                        this.open_path_with_options(path, options, Some(entry.path), window, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            error => {
                let _ = this.update_in(cx, |this, _, cx| {
                    this.error = Some(
                        Diagnostic::error("FILE_DIALOG_FAILED", Key::FileDialogFailed)
                            .with_details(format!("{error:?}")),
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn cancel_import(&mut self, _: &CancelImport, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(document) = self.active.and_then(|index| self.documents.get_mut(index)) {
            document.cancel();
            cx.notify();
        }
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        crate::theme::toggle(window, cx);
        cx.notify();
    }

    fn retry_import(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.active.and_then(|index| self.documents.get_mut(index)) else {
            return;
        };
        match document.retry() {
            Ok(true) => {
                self.error = None;
                self.start_next_import(window, cx);
            }
            Ok(false) => return,
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn content(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let locale = i18n::current(cx);
        let Some(document) = self.active.and_then(|index| self.documents.get(index)) else {
            return crate::welcome::render(
                locale,
                self.recent_list(cx),
                cx,
                cx.listener(|this, _, window, cx| this.open_file(&OpenFile, window, cx)),
            );
        };
        let mut content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .p_6()
            .gap_4()
            .child(
                div().text_lg().child(
                    document
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ),
            );
        match &document.status {
            DocumentStatus::Queued => {
                content = content.child(text(locale, Key::ImportQueued));
            }
            DocumentStatus::Reading => {
                let key = document
                    .progress
                    .as_ref()
                    .map_or(Key::Reading, |progress| progress.stage.message_key());
                content = content.child(text(locale, key));
                if let Some(progress) = &document.progress
                    && (progress.completed != 0 || progress.total.is_some())
                {
                    use pomelo_core::task::ImportStage;
                    let bytes =
                        matches!(progress.stage, ImportStage::Reading | ImportStage::Indexing);
                    let key = match (bytes, progress.total) {
                        (true, Some(_)) => Key::ImportBytesKnown,
                        (true, None) => Key::ImportBytesUnknown,
                        (false, Some(_)) => Key::ImportItemsKnown,
                        (false, None) => Key::ImportItemsUnknown,
                    };
                    let mut message = Message::new(key).arg("completed", progress.completed);
                    if let Some(total) = progress.total {
                        message = message.arg("total", total);
                    }
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(message.display(locale)),
                    );
                }
            }
            DocumentStatus::Cancelled => {
                content = content
                    .child(text(locale, Key::Cancelled))
                    .child(self.retry_encoding(document, cx))
                    .child(
                        Button::new("retry-cancelled-import")
                            .label(text(locale, Key::RetryImport))
                            .on_click(cx.listener(Self::retry_import)),
                    );
            }
            DocumentStatus::Failed(error) => {
                content = content.child(
                    div()
                        .text_color(theme.danger)
                        .child(error.message.display(locale)),
                );
                content = content.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(document.path.to_string_lossy().into_owned()),
                );
                content = content.child(self.retry_encoding(document, cx)).child(
                    Button::new("retry-failed-import")
                        .label(text(locale, Key::RetryImport))
                        .on_click(cx.listener(Self::retry_import)),
                );
                if let Some(details) = &error.technical_details {
                    content = content
                        .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                        .child(
                            div()
                                .text_sm()
                                .child(SharedString::from(details.to_string())),
                        );
                }
            }
            DocumentStatus::Imported(document) => {
                let board = &document.board;
                for diagnostic in &document.render_diagnostics {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(theme.warning)
                            .child(diagnostic.message.display(locale)),
                    );
                }
                content = content
                    .child(text(locale, Key::SceneImported))
                    .child(
                        Message::new(Key::HeaderFormat)
                            .arg("format", "Allegro")
                            .arg("version", board.source.layout_version)
                            .arg("writer", board.source.writer_version.as_str())
                            .display(locale),
                    )
                    .child(
                        Message::new(Key::SceneSummary)
                            .arg("layers", board.scene.layers.len())
                            .arg("segments", board.scene.segments.len())
                            .arg("pins", board.scene.pins.len())
                            .arg("vias", board.scene.vias.len())
                            .arg("zones", board.scene.zones.len())
                            .display(locale),
                    );
                if let Some(session) = self.active.and_then(|index| self.documents.get(index)) {
                    content = content.child(self.import_diagnostics(
                        session,
                        &board.scene.diagnostics,
                        cx,
                    ));
                }
                #[cfg(target_os = "windows")]
                if let Some(viewport) = self
                    .active
                    .and_then(|index| self.documents.get(index))
                    .and_then(|session| session.viewport.as_ref())
                {
                    content =
                        content.child(div().flex_1().min_h_0().min_w_0().child(viewport.clone()));
                }
                #[cfg(not(target_os = "windows"))]
                {
                    content = content.child(text(locale, Key::ViewportPending));
                }
            }
        }
        content.into_any_element()
    }

    fn import_diagnostics(
        &self,
        session: &DocumentSession,
        diagnostics: &[Diagnostic],
        cx: &Context<Self>,
    ) -> AnyElement {
        use pomelo_core::model::Severity;
        let locale = i18n::current(cx);
        let request = session.request;
        let count = diagnostics.len();
        let mut panel = div().flex().flex_col().min_h_0().gap_2().child(
            Button::new("import-diagnostics")
                .ghost()
                .selected(session.diagnostics_expanded)
                .disabled(count == 0)
                .label(
                    Message::new(Key::ImportDiagnostics)
                        .arg("count", count)
                        .display(locale),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(document) = this
                        .documents
                        .iter_mut()
                        .find(|document| document.request == request)
                    {
                        document.diagnostics_expanded = !document.diagnostics_expanded;
                        cx.notify();
                    }
                })),
        );
        if !session.diagnostics_expanded || count == 0 {
            return panel.into_any_element();
        }
        let last_page = (count - 1) / crate::document::DIAGNOSTICS_PAGE_SIZE;
        let page = session.diagnostics_page.min(last_page);
        let range = crate::document::diagnostics_range(count, page);
        let mut rows = div()
            .id(("diagnostic-page", page))
            .max_h_48()
            .overflow_y_scrollbar()
            .flex()
            .flex_col()
            .gap_2();
        for diagnostic in &diagnostics[range] {
            let (severity, color) = match diagnostic.severity {
                Severity::Info => (Key::DiagnosticInfo, cx.theme().info),
                Severity::Warning => (Key::DiagnosticWarning, cx.theme().warning),
                Severity::Error => (Key::DiagnosticError, cx.theme().danger),
            };
            let mut row = div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child(div().text_color(color).child(text(locale, severity)))
                .child(diagnostic.message.display(locale))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(diagnostic.code.as_ref())),
                );
            if let Some(offset) = diagnostic.offset {
                row = row.child(
                    Message::new(Key::DiagnosticOffset)
                        .arg("offset", offset)
                        .display(locale),
                );
            }
            if let Some(object) = diagnostic.object {
                row = row.child(
                    Message::new(Key::DiagnosticObject)
                        .arg("id", object.0)
                        .display(locale),
                );
            }
            if let Some(path) = &diagnostic.path {
                row = row.child(path.to_string_lossy().into_owned());
            }
            if let Some(details) = &diagnostic.technical_details {
                row = row
                    .child(text(locale, Key::TechnicalDetails))
                    .child(SharedString::from(details.as_ref()));
            }
            rows = rows.child(row);
        }
        panel = panel.child(rows).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("diagnostics-previous")
                        .ghost()
                        .label(text(locale, Key::MembersPrevious))
                        .disabled(page == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(document) = this
                                .documents
                                .iter_mut()
                                .find(|document| document.request == request)
                            {
                                document.diagnostics_page = page.saturating_sub(1);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Message::new(Key::DiagnosticPage)
                        .arg("current", page + 1)
                        .arg("total", last_page + 1)
                        .display(locale),
                )
                .child(
                    Button::new("diagnostics-next")
                        .ghost()
                        .label(text(locale, Key::MembersNext))
                        .disabled(page == last_page)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(document) = this
                                .documents
                                .iter_mut()
                                .find(|document| document.request == request)
                            {
                                document.diagnostics_page = (page + 1).min(last_page);
                                cx.notify();
                            }
                        })),
                ),
        );
        panel.into_any_element()
    }

    fn retry_encoding(&self, document: &DocumentSession, cx: &Context<Self>) -> AnyElement {
        use pomelo_import::TextEncoding;
        let locale = i18n::current(cx);
        let request = document.request;
        let current = document.import_options.text_encoding;
        let mut choices = div().flex().flex_wrap().gap_2();
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Gbk,
            TextEncoding::Big5,
            TextEncoding::ShiftJis,
            TextEncoding::Windows1252,
        ] {
            choices = choices.child(
                Button::new(encoding.tag())
                    .ghost()
                    .label(encoding.tag())
                    .selected(current.tag() == encoding.tag())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                            && document.set_retry_encoding(encoding)
                        {
                            cx.notify();
                        }
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(text(locale, Key::RetryEncoding)))
            .child(choices)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::RetryEncodingHint)),
            )
            .into_any_element()
    }

    fn recent_list(&self, cx: &Context<Self>) -> AnyElement {
        let locale = i18n::current(cx);
        let entries = &cx.global::<crate::recent::RecentState>().entries;
        crate::welcome::recent::render(locale, entries, cx, |entry| {
            let open_entry = entry.clone();
            let relocate_entry = entry.clone();
            let remove_path = entry.path.clone();
            crate::welcome::recent::RecentCommands {
                open: Box::new(cx.listener(move |this, _, window, cx| {
                    let mut options = this.import_options.clone();
                    options.text_encoding = open_entry.encoding;
                    this.open_path_with_options(open_entry.path.clone(), options, None, window, cx);
                })),
                relocate: Box::new(cx.listener(move |this, _, window, cx| {
                    this.relocate_recent(relocate_entry.clone(), window, cx)
                })),
                remove: Box::new(
                    cx.listener(move |_, _, _, cx| crate::recent::remove(&remove_path, cx)),
                ),
            }
        })
    }
}

impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = i18n::current(cx);
        let preference = cx.global::<LanguageState>().preference;
        let revision = cx.global::<LanguageState>().revision;
        let reload_available = self
            .active
            .and_then(|index| self.documents.get(index))
            .is_some_and(|document| {
                !matches!(
                    document.status,
                    DocumentStatus::Queued | DocumentStatus::Reading
                )
            });
        let menu_changed = i18n::set_reload_available(reload_available, cx);
        if revision != self.menu_revision || menu_changed {
            self.menu_bar.update(cx, |bar, cx| bar.reload(cx));
            self.menu_revision = revision;
        }
        let theme = cx.theme();
        let tabs = self
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                Button::new(("document", index))
                    .ghost()
                    .selected(self.active == Some(index))
                    .label(
                        document
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active = Some(index);
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .key_context("Pomelo")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::close_document))
            .on_action(cx.listener(Self::reload_document))
            .on_action(cx.listener(Self::cancel_import))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(|_, _: &OpenSettings, window, cx| {
                crate::settings::open(window, cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::SystemLanguage, _, cx| {
                i18n::select(LanguagePreference::System, cx)
            }))
            .on_action(cx.listener(|_, _: &i18n::EnglishLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::English), cx)
            }))
            .on_action(cx.listener(|_, _: &i18n::SimplifiedLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::SimplifiedChinese), cx)
            }))
            .on_action(cx.listener(|_, _: &i18n::TraditionalLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::TraditionalChinese), cx)
            }))
            .on_action(cx.listener(|_, _: &i18n::JapaneseLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Japanese), cx)
            }))
            .on_action(cx.listener(|_, _: &i18n::KoreanLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Korean), cx)
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in &paths.0 {
                    this.open_path(path.clone(), window, cx);
                }
            }))
            .children((!cfg!(target_os = "macos")).then(|| {
                div()
                    .h_8()
                    .px_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(self.menu_bar.clone())
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_lg().child("Pomelo"))
                    .child(
                        Button::new("open")
                            .label(text(locale, Key::OpenFile))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_file(&OpenFile, window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-import")
                            .ghost()
                            .label(text(locale, Key::CancelImport))
                            .disabled(
                                !self
                                    .active
                                    .and_then(|index| self.documents.get(index))
                                    .is_some_and(|document| {
                                        matches!(
                                            document.status,
                                            DocumentStatus::Queued | DocumentStatus::Reading
                                        )
                                    }),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_import(&CancelImport, window, cx)
                            })),
                    )
                    .child(
                        Button::new("reload")
                            .ghost()
                            .label(text(locale, Key::ReloadDocument))
                            .disabled(!reload_available)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reload_document(&ReloadDocument, window, cx)
                            })),
                    )
                    .child(
                        Button::new("close")
                            .ghost()
                            .label(text(locale, Key::CloseDocument))
                            .disabled(self.active.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_document(&CloseDocument, window, cx)
                            })),
                    )
                    .child(
                        Button::new("settings")
                            .ghost()
                            .label(text(locale, Key::SettingsMenu))
                            .on_click(cx.listener(|_, _, window, cx| {
                                crate::settings::open(window, cx);
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .label(text(
                                locale,
                                if cx.global::<crate::theme::ThemeState>().preference
                                    == crate::prefs::ThemePreference::Dark
                                {
                                    Key::LightTheme
                                } else {
                                    Key::DarkTheme
                                },
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_theme(&ToggleTheme, window, cx)
                            })),
                    )
                    .child(
                        Button::new("language")
                            .ghost()
                            .label(
                                Message::new(Key::LanguageCurrent)
                                    .arg("language", locale.native_name())
                                    .display(locale),
                            )
                            .dropdown_menu(move |mut menu, _, _| {
                                menu = menu
                                    .item(
                                        PopupMenuItem::new(text(locale, Key::FollowSystem))
                                            .checked(preference == LanguagePreference::System)
                                            .action(i18n::action(LanguagePreference::System)),
                                    )
                                    .separator();
                                for language in Locale::ALL {
                                    let candidate = LanguagePreference::Explicit(language);
                                    menu = menu.item(
                                        PopupMenuItem::new(language.native_name())
                                            .checked(preference == candidate)
                                            .action(i18n::action(candidate)),
                                    );
                                }
                                menu
                            }),
                    ),
            )
            .child(div().flex().gap_1().p_2().children(tabs))
            .child(self.content(cx))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(
                        self.error
                            .as_ref()
                            .or(cx.global::<LanguageState>().warning.as_ref())
                            .or(cx.global::<crate::theme::ThemeState>().warning.as_ref())
                            .or(cx.global::<crate::recent::RecentState>().warning.as_ref())
                            .map(|error| error.message.display(locale))
                            .unwrap_or_else(|| text(locale, Key::LocalReadOnly)),
                    ),
            )
    }
}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn opened_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
