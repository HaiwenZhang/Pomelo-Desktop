use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
// Native command surfaces and per-document asynchronous board imports.

use std::path::PathBuf;

use futures::future::Shared;
use gpui_kit::base::{Disableable, Selectable};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, TITLE_BAR_HEIGHT, TitleBar,
    button::{Button, ButtonVariants},
    menu::{AppMenuBar, DropdownMenu, PopupMenuItem},
};
use gpui_kit::*;
use pomelo_core::task::RequestId;
use pomelo_core::{
    i18n::{LanguagePreference, Locale, Message, MessageKey as Key, text},
    model::Diagnostic,
};
use pomelo_import::{ImportOptions, formats::format_display_name};

use crate::document::{DocumentSession, DocumentStatus};
use crate::i18n::{self, LanguageState};
use crate::tooltips::ButtonTooltipExt;
use crate::welcome::recent::Presentation as RecentPresentation;

use crate::actions::{
    CancelImport, CloseDocument, FitActiveBoard, NextDocument, OpenFile, OpenSettings,
    PreviousDocument, ReloadDocument, ToggleLeftPanel, ToggleRightPanel, ToggleTheme,
};

mod commands;
mod document_tabs;
mod import_progress;
mod imports;
pub(crate) mod panel_layout;
mod saving;
mod view;

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
                let pending = this.queue_view_save(None, cx);
                this.view_save_task = None;
                Some(pending)
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
}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
