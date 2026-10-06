use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
// Native command surfaces and per-document asynchronous board imports.

use std::path::PathBuf;

use futures::future::{FutureExt as _, Shared};
use gpui_kit::base::{Disableable, Selectable};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, TITLE_BAR_HEIGHT, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    menu::{AppMenuBar, DropdownMenu, PopupMenuItem},
};
use gpui_kit::*;
use pomelo_core::task::{DocumentId, RequestId};
use pomelo_core::{
    i18n::{LanguagePreference, Locale, Message, MessageKey as Key, text},
    model::Diagnostic,
};
use pomelo_import::{ImportOptions, formats::format_display_name};

use crate::document::{DocumentSession, DocumentStatus, PreparedDocument};
use crate::i18n::{self, LanguageState};
use crate::welcome::recent::Presentation as RecentPresentation;

use crate::actions::{
    CancelImport, CloseDocument, FitActiveBoard, NextDocument, OpenFile, OpenSettings,
    PreviousDocument, ReloadDocument, ToggleLeftPanel, ToggleRightPanel, ToggleTheme,
};

mod document_tabs;
mod import_progress;
pub(crate) mod panel_layout;
mod saving;

pub struct Workbench {
    preparation_budget: pomelo_core::memory::MemoryBudget,
    documents: Vec<DocumentSession>,
    document_scroll: ScrollHandle,
    active: Option<usize>,
    next_id: u64,
    running_import: Option<RequestId>,
    focus: FocusHandle,
    diagnostics_navigation: crate::panels::diagnostics::NavigationFocus,
    recent_only: bool,
    welcome_scroll: crate::welcome::ScrollState,
    logo: Option<std::sync::Arc<RenderImage>>,
    tasks: Vec<Task<()>>,
    error: Option<Diagnostic>,
    menu_bar: Entity<AppMenuBar>,
    menu_revision: u64,
    import_options: ImportOptions,
    source_font: Option<crate::services::source_font::SourceFontConfig>,
    view_save_task: Option<Shared<Task<Result<(), Diagnostic>>>>,
    view_save_generation: u64,
    quit_task: Option<Task<()>>,
    quit_snapshot: Option<(Vec<u8>, u64)>,
    quit_flushed: bool,
    panel_layout: Entity<panel_layout::PanelLayout>,
    search_locale: Locale,
}

impl Workbench {
    pub fn new(
        import_options: ImportOptions,
        source_font: Option<crate::services::source_font::SourceFontConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (panels, panel_error) =
            match crate::services::preferences::PanelStore::platform_default() {
                Some(store) => match store.load() {
                    Ok(panels) => (panels, None),
                    Err(error) => (
                        crate::services::preferences::PanelPreferences::default(),
                        Some(error),
                    ),
                },
                None => (
                    crate::services::preferences::PanelPreferences::default(),
                    Some(Diagnostic::error(
                        "CONFIG_DIRECTORY_MISSING",
                        Key::ConfigDirectoryMissing,
                    )),
                ),
            };
        let panel_layout = cx.new(|cx| panel_layout::PanelLayout::new(panels, cx));
        // Locale changes and resizing can remove a focused overflow command.
        // Restore a live ancestor so shortcuts still have a dispatch path.
        cx.on_focus_lost(window, |this, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.focus.clone());
            window.focus(&target, cx);
        })
        .detach();
        cx.on_app_quit(|this, cx| {
            // Normal exits have already flushed while the event loop was alive.
            // System shutdown still gets GPUI's bounded best-effort fallback.
            let pending = if this.quit_flushed {
                None
            } else {
                this.queue_view_save(None, cx);
                this.view_save_task.take()
            };
            let locale = i18n::current(cx);
            async move {
                if let Some(pending) = pending
                    && let Err(error) = pending.await
                {
                    saving::report_save_error(&error, locale);
                }
            }
        })
        .detach();
        Self {
            preparation_budget: pomelo_core::memory::MemoryBudget::new(0),
            search_locale: sys_locale::get_locale()
                .as_deref()
                .and_then(Locale::from_system_tag)
                .unwrap_or_default(),
            documents: Vec::new(),
            document_scroll: ScrollHandle::new(),
            active: None,
            next_id: 1,
            running_import: None,
            focus: cx.focus_handle(),
            diagnostics_navigation: crate::panels::diagnostics::NavigationFocus::new(cx),
            recent_only: false,
            welcome_scroll: crate::welcome::ScrollState::default(),
            logo: cx
                .svg_renderer()
                .render_single_frame(include_bytes!("../../assets/pomelo.svg"), 2.0)
                .ok(),
            tasks: Vec::new(),
            error: panel_error,
            menu_bar: AppMenuBar::new(cx),
            menu_revision: cx.global::<LanguageState>().revision,
            import_options,
            source_font,
            view_save_task: None,
            view_save_generation: 0,
            quit_task: None,
            quit_snapshot: None,
            quit_flushed: false,
            panel_layout,
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
            self.activate_document(index, window, cx);
            if let Some(origin) = relocated_from {
                let document = &mut self.documents[index];
                if let DocumentStatus::Imported(prepared) = &document.status {
                    crate::services::recent::remember(
                        crate::services::preferences::RecentEntry {
                            path: document.path.clone(),
                            format: prepared.board.source.format.clone(),
                            opened_unix_seconds: opened_time(),
                            encoding: document.import_options.text_encoding,
                            presentation: Some(recent_presentation(prepared)),
                        },
                        Some(&origin),
                        prepared
                            .preview
                            .as_ref()
                            .map(|preview| preview.image.clone()),
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
        self.documents.push(session);
        self.activate_document(self.documents.len() - 1, window, cx);
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
                        crate::services::recent::remember(
                            crate::services::preferences::RecentEntry {
                                path: document.path.clone(),
                                format: prepared.board.source.format.clone(),
                                opened_unix_seconds: opened_time(),
                                encoding: document.import_options.text_encoding,
                                presentation: Some(recent_presentation(prepared)),
                            },
                            replaced_path.as_deref(),
                            prepared
                                .preview
                                .as_ref()
                                .map(|preview| preview.image.clone()),
                            cx,
                        );
                    }
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    if let DocumentStatus::Imported(prepared) = &document.status {
                        document.viewport = Some(cx.new(|cx| {
                            crate::viewport::BoardViewport::new(
                                prepared,
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
        self.queue_view_save(index, cx);
        let Some(pending) = self.view_save_task.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Err(error) = pending.await {
                let _ = this.update(cx, |this, cx| {
                    saving::report_save_error(&error, i18n::current(cx));
                    this.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn queue_view_save(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let snapshot = self.view_save_snapshot(index, cx);
        self.enqueue_view_save(snapshot, cx);
    }

    fn view_save_snapshot(
        &self,
        index: Option<usize>,
        cx: &Context<Self>,
    ) -> saving::ViewSaveSnapshot {
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
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
                Some(crate::services::preferences::ViewEntry {
                    path: document.path.clone(),
                    state: viewport.read(cx).snapshot(&prepared.board.identity)?,
                })
            })
            .collect();
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let entries: Vec<crate::services::preferences::ViewEntry> = {
            let _ = index;
            Vec::new()
        };
        let panels = self.panel_layout.read(cx).snapshot(cx);
        saving::ViewSaveSnapshot { entries, panels }
    }

    fn enqueue_view_save(&mut self, snapshot: saving::ViewSaveSnapshot, cx: &mut Context<Self>) {
        let saving::ViewSaveSnapshot { entries, panels } = snapshot;
        self.view_save_generation = self.view_save_generation.wrapping_add(1);
        let panel_store = crate::services::preferences::PanelStore::platform_default();
        let previous = self.view_save_task.take();
        let store = crate::services::preferences::ViewStore::platform_default();
        // GPUI blocks the main thread during shutdown; disk writes must not depend on it.
        self.view_save_task = Some(
            cx.background_executor()
                .spawn(async move {
                    if let Some(previous) = previous {
                        let _ = previous.await;
                    }
                    let panel_store = panel_store.ok_or_else(|| {
                        Diagnostic::error("CONFIG_DIRECTORY_MISSING", Key::ConfigDirectoryMissing)
                    })?;
                    panel_store.save(panels)?;
                    if !entries.is_empty() {
                        let store = store.ok_or_else(|| {
                            Diagnostic::error(
                                "CONFIG_DIRECTORY_MISSING",
                                Key::ConfigDirectoryMissing,
                            )
                        })?;
                        store.save_updates(entries)?;
                    }
                    Ok(())
                })
                .shared(),
        );
    }

    fn activate_document(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn next_document(&mut self, _: &NextDocument, window: &mut Window, cx: &mut Context<Self>) {
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

    fn previous_document(
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

    fn fit_active_board(
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

    fn toggle_panel(
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

    fn close_document(&mut self, _: &CloseDocument, window: &mut Window, cx: &mut Context<Self>) {
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

    fn relocate_recent(
        &mut self,
        original_path: PathBuf,
        encoding: pomelo_import::TextEncoding,
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

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        crate::settings::open(window, cx);
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

    fn content(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let locale = i18n::current(cx);
        let Some(document) = self.active.and_then(|index| self.documents.get(index)) else {
            return crate::welcome::render(
                locale,
                self.recent_list(
                    if self.recent_only {
                        RecentPresentation::All
                    } else {
                        RecentPresentation::Continue
                    },
                    cx,
                ),
                self.recent_list(RecentPresentation::Sidebar, cx),
                self.recent_only,
                &self.welcome_scroll,
                window,
                cx,
            );
        };
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        if let DocumentStatus::Imported(_) = &document.status
            && let Some(viewport) = &document.viewport
        {
            return div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(viewport.clone())
                .into_any_element();
        }
        if matches!(
            document.status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            return import_progress::render(document, locale, cx);
        }
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
            DocumentStatus::Queued | DocumentStatus::Reading => unreachable!(),
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
                    .child(if board.source.format == "allegro" {
                        Message::new(Key::HeaderFormat)
                            .arg("format", format_display_name(&board.source.format))
                            .arg("version", board.source.layout_version)
                            .arg("writer", board.source.writer_version.as_str())
                            .display(locale)
                    } else {
                        format_display_name(&board.source.format).to_owned()
                    })
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
                        window,
                        cx,
                    ));
                }
                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                if let Some(viewport) = self
                    .active
                    .and_then(|index| self.documents.get(index))
                    .and_then(|session| session.viewport.as_ref())
                {
                    content =
                        content.child(div().flex_1().min_h_0().min_w_0().child(viewport.clone()));
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
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
        let rows = crate::panels::diagnostics::page_rows(locale, page, &diagnostics[range], cx);
        panel = panel.child(rows).child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    crate::panels::diagnostics::page_button(
                        "diagnostics-previous",
                        text(locale, Key::MembersPrevious),
                        &self.diagnostics_navigation.previous,
                        page == 0,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                        {
                            document.diagnostics_page = page.saturating_sub(1);
                            this.diagnostics_navigation.focus_boundary(
                                document.diagnostics_page,
                                last_page,
                                window,
                                cx,
                            );
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
                    crate::panels::diagnostics::page_button(
                        "diagnostics-next",
                        text(locale, Key::MembersNext),
                        &self.diagnostics_navigation.next,
                        page == last_page,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                        {
                            document.diagnostics_page = (page + 1).min(last_page);
                            this.diagnostics_navigation.focus_boundary(
                                document.diagnostics_page,
                                last_page,
                                window,
                                cx,
                            );
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
            TextEncoding::Auto,
            TextEncoding::Utf8,
            TextEncoding::Gbk,
            TextEncoding::Big5,
            TextEncoding::ShiftJis,
            TextEncoding::Windows1252,
        ] {
            choices = choices.child(
                Button::new(encoding.tag())
                    .ghost()
                    .label(if matches!(encoding, TextEncoding::Auto) {
                        text(locale, Key::EncodingAuto)
                    } else {
                        encoding.tag().to_owned()
                    })
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

    fn recent_list(&self, presentation: RecentPresentation, cx: &Context<Self>) -> AnyElement {
        let locale = i18n::current(cx);
        let entries = &cx.global::<crate::services::recent::RecentState>().entries;
        crate::welcome::recent::render(
            locale,
            entries,
            presentation,
            &cx.global::<crate::services::recent::RecentState>().previews,
            &self.welcome_scroll,
            cx,
            |entry| {
                let open_path = entry.path.clone();
                let relocate_path = entry.path.clone();
                let encoding = entry.encoding;
                let remove_path = entry.path.clone();
                crate::welcome::recent::RecentCommands {
                    open: Box::new(cx.listener(move |this, _, window, cx| {
                        let mut options = this.import_options.clone();
                        options.text_encoding = encoding;
                        this.open_path_with_options(open_path.clone(), options, None, window, cx);
                    })),
                    relocate: Box::new(cx.listener(move |this, _, window, cx| {
                        this.relocate_recent(relocate_path.clone(), encoding, window, cx)
                    })),
                    remove: Box::new(cx.listener(move |_, _, _, cx| {
                        crate::services::recent::remove(&remove_path, cx)
                    })),
                }
            },
        )
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let fit_available = self
            .active
            .and_then(|index| self.documents.get(index))
            .is_some_and(|document| document.viewport.is_some());
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let fit_available = false;
        let menu_changed = i18n::set_document_commands(
            i18n::DocumentMenuState {
                reload_available,
                fit_available,
                has_document: !self.documents.is_empty(),
                can_cycle: self.documents.len() > 1,
            },
            cx,
        );
        if revision != self.menu_revision || menu_changed {
            self.menu_bar.update(cx, |bar, cx| bar.reload(cx));
            self.menu_revision = revision;
        }
        let theme = cx.theme();
        // Reserve the native macOS traffic lights on the left. Bound the
        // content to the remaining space so long titles cannot displace controls.
        let title_left_padding = if cfg!(target_os = "macos") {
            px(80.0)
        } else {
            px(0.0)
        };
        let controls = window.window_controls();
        let control_count = 1 + u8::from(controls.minimize) + u8::from(controls.maximize);
        let controls_width = if cfg!(target_os = "macos") {
            px(0.0)
        } else {
            TITLE_BAR_HEIGHT * f32::from(control_count)
        };
        // TitleBar adds another 12 pixels inside its content in fullscreen.
        let fullscreen_padding = if window.is_fullscreen() {
            px(12.0)
        } else {
            px(0.0)
        };
        let title_content_width = (window.viewport_size().width
            - title_left_padding
            - controls_width
            - fullscreen_padding)
            .max(px(0.0));
        let title = self
            .active
            .and_then(|index| self.documents.get(index))
            .map(|document| {
                document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "Pomelo · PCB Viewer".into());
        let tabs = self
            .documents
            .iter()
            .map(|document| {
                let file_name = document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let format = match &document.status {
                    DocumentStatus::Imported(prepared) => {
                        let format = prepared.board.source.format.as_str();
                        Some(SharedString::from(format_display_name(format).to_owned()))
                    }
                    _ => None,
                };
                document_tabs::DocumentTab {
                    label: file_name.into(),
                    format,
                }
            })
            .collect::<Vec<_>>();
        div()
            .key_context("Pomelo")
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in paths.paths() {
                    this.open_path(path.clone(), window, cx);
                }
            }))
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
            .on_action(cx.listener(Self::open_settings))
            .on_action(
                cx.listener(|_, _: &crate::actions::AboutPomelo, window, cx| {
                    crate::settings::open_about(window, cx)
                }),
            )
            .on_action(cx.listener(Self::next_document))
            .on_action(cx.listener(Self::previous_document))
            .on_action(cx.listener(Self::fit_active_board))
            .on_action(cx.listener(|this, _: &ToggleLeftPanel, window, cx| {
                this.toggle_panel(panel_layout::Side::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightPanel, window, cx| {
                this.toggle_panel(panel_layout::Side::Right, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::actions::FocusSearch, window, cx| {
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    if let Some(viewport) = this
                        .active
                        .and_then(|index| this.documents.get(index))
                        .and_then(|document| document.viewport.as_ref())
                    {
                        viewport.update(cx, |viewport, cx| {
                            viewport.focus_search(&crate::viewport::FocusSearch, window, cx)
                        });
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ShowRecentFiles, _, cx| {
                    this.recent_only = true;
                    cx.notify();
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ShowStartPage, _, cx| {
                    this.recent_only = false;
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|_, _: &i18n::SystemLanguage, _, cx| {
                i18n::select(LanguagePreference::System, cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::EnglishLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::English), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::SimplifiedLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::SimplifiedChinese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::TraditionalLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::TraditionalChinese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::JapaneseLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Japanese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::KoreanLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Korean), cx);
            }))
            .child(
                TitleBar::new().h_10().pl(title_left_padding).child(
                    div()
                        .w(title_content_width)
                        .min_w_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .pl_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .h_full()
                                .flex_shrink_0()
                                .child(self.logo.as_ref().map_or_else(
                                    || {
                                        Icon::new(IconName::CircuitBoard)
                                            .size_5()
                                            .text_color(theme.primary)
                                            .into_any_element()
                                    },
                                    |logo| {
                                        img(logo.clone())
                                            .w_6()
                                            .h_7()
                                            .object_fit(ObjectFit::Contain)
                                            .into_any_element()
                                    },
                                ))
                                .child(div().font_weight(FontWeight::SEMIBOLD).child("Pomelo"))
                                .child(self.menu_bar.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_center()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(title),
                        ),
                ),
            )
            .when(!self.documents.is_empty(), |shell| {
                shell.child(
                    div()
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(document_tabs::render(
                            document_tabs::DocumentTabs {
                                items: tabs,
                                selected: self.active.unwrap_or(0),
                                locale,
                                scroll: self.document_scroll.clone(),
                            },
                            cx.listener(|this, index: &usize, window, cx| {
                                this.activate_document(*index, window, cx);
                            }),
                            cx.listener(|this, index: &usize, window, cx| {
                                this.activate_document(*index, window, cx);
                                this.close_document(&CloseDocument, window, cx);
                            }),
                            window,
                            cx,
                        ))
                        .child(
                            div()
                                .px_4()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(text(locale, Key::ReadOnly)),
                        ),
                )
            })
            .when(self.active.is_some() && !fit_available, |shell| {
                shell.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            Button::new("open")
                                .ghost()
                                .icon(IconName::FolderOpen)
                                .label(text(locale, Key::OpenFile))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_file(&OpenFile, window, cx)
                                })),
                        )
                        .child(
                            Button::new("cancel-import")
                                .ghost()
                                .label(text(locale, Key::CancelImport))
                                .disabled(reload_available)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_import(&CancelImport, window, cx)
                                })),
                        )
                        .child(
                            Button::new("reload")
                                .ghost()
                                .icon(IconName::RotateCcw)
                                .label(text(locale, Key::ReloadDocument))
                                .disabled(!reload_available)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reload_document(&ReloadDocument, window, cx)
                                })),
                        ),
                )
            })
            .child(self.content(window, cx))
            .when(fit_available, |shell| {
                let warning = self
                    .error
                    .as_ref()
                    .or(cx
                        .global::<crate::services::recent::RecentState>()
                        .warning
                        .as_ref())
                    .or(cx.global::<LanguageState>().warning.as_ref())
                    .or(cx.global::<crate::theme::ThemeState>().warning.as_ref());
                shell.when_some(warning, |shell, warning| {
                    shell.child(
                        div()
                            .px_4()
                            .py_2()
                            .text_sm()
                            .flex_shrink_0()
                            .text_color(theme.danger)
                            .child(warning.message.display(locale)),
                    )
                })
            })
            .when(!fit_available, |shell| {
                shell.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .justify_between()
                        .px_4()
                        .h_9()
                        .border_t_1()
                        .border_color(theme.border)
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(
                            div().child(
                                self.error
                                    .as_ref()
                                    .or(cx
                                        .global::<crate::services::recent::RecentState>()
                                        .warning
                                        .as_ref())
                                    .or(cx.global::<LanguageState>().warning.as_ref())
                                    .or(cx.global::<crate::theme::ThemeState>().warning.as_ref())
                                    .map(|error| error.message.display(locale))
                                    .unwrap_or_else(|| {
                                        text(
                                            locale,
                                            if self.active.is_none() {
                                                Key::WaitingForFile
                                            } else {
                                                Key::LocalReadOnly
                                            },
                                        )
                                    }),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("language")
                                        .ghost()
                                        .small()
                                        .label(locale.native_name())
                                        .dropdown_menu(move |mut menu, _, _| {
                                            menu = menu
                                                .item(
                                                    PopupMenuItem::new(text(
                                                        locale,
                                                        Key::FollowSystem,
                                                    ))
                                                    .checked(
                                                        preference == LanguagePreference::System,
                                                    )
                                                    .action(i18n::action(
                                                        LanguagePreference::System,
                                                    )),
                                                )
                                                .separator();
                                            for language in Locale::ALL {
                                                let candidate =
                                                    LanguagePreference::Explicit(language);
                                                menu = menu.item(
                                                    PopupMenuItem::new(language.native_name())
                                                        .checked(preference == candidate)
                                                        .action(i18n::action(candidate)),
                                                );
                                            }
                                            menu
                                        }),
                                )
                                .child(
                                    Button::new("theme")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Moon)
                                        .label(text(locale, Key::SettingsTheme))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_theme(&ToggleTheme, window, cx)
                                        })),
                                )
                                .child(div().child(text(locale, Key::LocalFile))),
                        ),
                )
            })
    }
}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn recent_presentation(
    prepared: &PreparedDocument,
) -> crate::services::preferences::RecentPresentation {
    crate::services::preferences::RecentPresentation {
        layer_count: prepared.board.scene.layers.len(),
        thumbnail_png: prepared.preview.as_ref().map(|preview| preview.png.clone()),
    }
}

fn opened_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
