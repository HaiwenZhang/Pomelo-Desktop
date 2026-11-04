//! File opening and asynchronous document import scheduling.

use std::path::PathBuf;

use gpui_kit::{component::WindowExt, *};
use pomelo_core::{
    i18n::{MessageKey as Key, text},
    model::Diagnostic,
    task::DocumentId,
};
use pomelo_import::ImportOptions;

use super::Workbench;
use crate::{
    actions::OpenFile,
    document::{DocumentSession, DocumentStatus, PreparedDocument},
    i18n,
};

impl Workbench {
    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_path_with_options(path, self.import_options.clone(), None, window, cx);
    }

    pub(super) fn open_path_with_options(
        &mut self,
        path: PathBuf,
        options: ImportOptions,
        relocated_from: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = match document_path(&path) {
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
            self.activate_document(index, window, cx);
            if let Some(origin) = relocated_from {
                let document = &mut self.documents[index];
                if let DocumentStatus::Imported(prepared) = &document.status {
                    remember_recent(document, prepared, Some(&origin), cx);
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
        self.documents.push(session);
        self.activate_document(self.documents.len() - 1, window, cx);
        self.error = None;
        self.start_next_import(window, cx);
        cx.notify();
    }

    pub(super) fn start_next_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let search_locale = self.search_locale;
        let source_font = self.source_font.clone();
        let preparation_budget = self.preparation_budget.clone();
        let input = crate::document::preparation::DocumentPreparation {
            path,
            options,
            cancellation,
            mailbox,
            reload_view,
            search_locale,
            source_font,
            preparation_budget,
        };
        let import = cx
            .background_spawn(async move { crate::document::preparation::prepare_document(input) });
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
                            if let Some(last) = document.progress_log.last_mut()
                                && last.stage == progress.stage
                            {
                                *last = progress.clone();
                            } else {
                                document.progress_log.push(progress.clone());
                                if document.progress_log.len() > 6 {
                                    document.progress_log.remove(0);
                                }
                            }
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
                        remember_recent(document, prepared, replaced_path.as_deref(), cx);
                    }
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    if let DocumentStatus::Imported(prepared) = &document.status {
                        let source_name = document
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        document.viewport = Some(cx.new(|cx| {
                            crate::viewport::BoardViewport::new(
                                prepared,
                                source_name,
                                this.panel_layout.clone(),
                                window,
                                cx,
                            )
                        }));
                    }
                }
                if let Some(index) = this
                    .documents
                    .iter()
                    .position(|document| document.request == request)
                    && this.active == Some(index)
                    && !window.has_active_dialog(cx)
                {
                    this.activate_document(index, window, cx);
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

    pub(super) fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: !cfg!(target_os = "windows"),
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
}

/// Identity used to detect an already-open document. On Unix, resolving symbolic
/// links and `..` stops one file from opening twice under different spellings;
/// Windows keeps the absolute form because `canonicalize` yields `\\?\` paths.
fn document_path(path: &std::path::Path) -> std::io::Result<PathBuf> {
    #[cfg(unix)]
    if let Ok(path) = std::fs::canonicalize(path) {
        return Ok(path);
    }
    std::path::absolute(path)
}

fn remember_recent(
    document: &DocumentSession,
    prepared: &PreparedDocument,
    replaced_path: Option<&std::path::Path>,
    cx: &mut App,
) {
    crate::services::recent::remember(
        crate::services::preferences::RecentEntry {
            path: document.path.clone(),
            format: prepared.board.source.format.clone(),
            opened_unix_seconds: opened_time(),
            encoding: document.import_options.text_encoding,
            presentation: Some(crate::services::preferences::RecentPresentation {
                layer_count: prepared.board.scene.layers.len(),
                thumbnail_png: prepared.preview.as_ref().map(|preview| preview.png.clone()),
            }),
        },
        replaced_path,
        prepared
            .preview
            .as_ref()
            .map(|preview| preview.image.clone()),
        cx,
    );
}

fn opened_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
