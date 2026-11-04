//! Workbench action handlers: document switching, panels, reload and import control.
use super::*;

impl Workbench {
    pub(super) fn activate_document(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get(index) else {
            return;
        };
        self.active = Some(index);
        self.document_scroll.scroll_to_item(index);
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        if let Some(viewport) = &document.viewport {
            let focus = viewport.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn next_document(
        &mut self,
        _: &NextDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.documents.len();
        if count < 2 {
            return;
        }
        let index = self
            .active
            .filter(|index| *index < count)
            .map_or(0, |index| (index + 1) % count);
        self.activate_document(index, window, cx);
    }

    pub(super) fn previous_document(
        &mut self,
        _: &PreviousDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.documents.len();
        if count < 2 {
            return;
        }
        let index = match self.active.filter(|index| *index < count) {
            Some(0) | None => count - 1,
            Some(index) => index - 1,
        };
        self.activate_document(index, window, cx);
    }

    pub(super) fn fit_active_board(
        &mut self,
        _: &FitActiveBoard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        if let Some(viewport) = self
            .active
            .and_then(|index| self.documents.get(index))
            .and_then(|document| document.viewport.as_ref())
        {
            viewport.update(cx, |viewport, cx| {
                viewport.fit_board(&crate::viewport::FitBoard, window, cx);
            });
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let _ = (window, cx);
    }

    pub(super) fn toggle_panel(
        &mut self,
        side: panel_layout::Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        if let Some(viewport) = self
            .active
            .and_then(|index| self.documents.get(index))
            .and_then(|document| document.viewport.as_ref())
        {
            self.panel_layout
                .update(cx, |layout, cx| layout.toggle(side, cx));
            window.focus(&viewport.read(cx).focus_handle(cx), cx);
            self.save_views(None, cx);
            cx.notify();
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let _ = (side, window, cx);
    }

    pub(super) fn close_document(
        &mut self,
        _: &CloseDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.active {
            self.save_views(Some(index), cx);
            self.documents.remove(index);
            if self.documents.is_empty() {
                self.active = None;
                window.focus(&self.focus, cx);
            } else {
                self.activate_document(index.min(self.documents.len() - 1), window, cx);
            }
            cx.notify();
        }
    }

    pub(super) fn reload_document(
        &mut self,
        _: &ReloadDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active else {
            return;
        };
        if matches!(
            self.documents[index].status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            return;
        }
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
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
                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
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

    pub(super) fn relocate_recent(
        &mut self,
        original_path: PathBuf,
        encoding: pomelo_import::TextEncoding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(text(i18n::current(cx), Key::RelocatePrompt).into()),
        });
        cx.spawn_in(window, async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        let mut options = this.import_options.clone();
                        options.text_encoding = encoding;
                        this.open_path_with_options(path, options, Some(original_path), window, cx);
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

    pub(super) fn cancel_import(
        &mut self,
        _: &CancelImport,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(document) = self.active.and_then(|index| self.documents.get_mut(index)) {
            document.cancel();
            cx.notify();
        }
    }

    pub(super) fn toggle_theme(
        &mut self,
        _: &ToggleTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::theme::toggle(window, cx);
        cx.notify();
    }

    pub(super) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::settings::open(window, cx);
    }

    pub(super) fn retry_import(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
}
