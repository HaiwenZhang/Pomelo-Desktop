//! Application initialization and native event-loop entry.
use crate::actions::{
    CancelImport, CloseDocument, FitActiveBoard, NextDocument, OpenFile, OpenSettings,
    PreviousDocument, Quit, ReloadDocument, ToggleLeftPanel, ToggleRightPanel, ToggleTheme,
};
use crate::services::startup_error;
use crate::services::{recent, startup};
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use crate::viewport;
use crate::workbench::Workbench;
use crate::{i18n, theme};
use gpui_kit::*;
use pomelo_core::{
    i18n::{Locale, MessageKey},
    model::Diagnostic,
};
use std::{cell::Cell, process::ExitCode, rc::Rc};

pub fn run() -> ExitCode {
    let preferred_locale = startup_error::preferred_locale();
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let diagnostic_locale = startup::Startup::diagnostic_locale(&arguments, preferred_locale);
    let startup = match startup::Startup::parse(arguments) {
        Ok(startup) => startup,
        Err(error) => {
            startup_error::report(diagnostic_locale, &error);
            return ExitCode::from(2);
        }
    };
    run_application(diagnostic_locale, move |cx, failed| {
        init(cx);
        i18n::initialize(cx, startup.locale);
        theme::initialize(cx);
        recent::initialize(cx);
        cx.bind_keys([
            KeyBinding::new(crate::shortcuts::keys::OPEN_FILE, OpenFile, Some("Pomelo")),
            KeyBinding::new("secondary-w", CloseDocument, Some("Pomelo")),
            KeyBinding::new("secondary-r", ReloadDocument, Some("Pomelo")),
            KeyBinding::new("escape", CancelImport, Some("Pomelo")),
            KeyBinding::new("secondary-t", ToggleTheme, Some("Pomelo")),
            KeyBinding::new("secondary-,", OpenSettings, Some("Pomelo")),
            KeyBinding::new("secondary-tab", NextDocument, Some("Pomelo")),
            KeyBinding::new("secondary-shift-tab", PreviousDocument, Some("Pomelo")),
            KeyBinding::new(
                crate::shortcuts::keys::FIT_ACTIVE_BOARD,
                FitActiveBoard,
                Some("Pomelo"),
            ),
            KeyBinding::new(
                crate::shortcuts::keys::TOGGLE_LEFT_PANEL,
                ToggleLeftPanel,
                Some("Pomelo"),
            ),
            KeyBinding::new(
                crate::shortcuts::keys::TOGGLE_RIGHT_PANEL,
                ToggleRightPanel,
                Some("Pomelo"),
            ),
            KeyBinding::new("secondary-q", Quit, None),
        ]);
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        cx.bind_keys([
            KeyBinding::new(
                crate::shortcuts::keys::FOCUS_SEARCH,
                viewport::FocusSearch,
                Some("Pomelo"),
            ),
            KeyBinding::new("escape", viewport::LeaveSearch, Some("BoardWorkspace")),
            KeyBinding::new(
                "escape",
                viewport::CancelCanvasAction,
                Some("BoardViewport"),
            ),
            KeyBinding::new("m", viewport::MeasureDistance, Some("BoardViewport")),
            KeyBinding::new("p", viewport::MeasurePolyline, Some("BoardViewport")),
            KeyBinding::new("c", viewport::MeasureClearance, Some("BoardViewport")),
            KeyBinding::new("shift-l", viewport::MeasurePinPath, Some("BoardViewport")),
            KeyBinding::new("s", viewport::BoxSelect, Some("BoardViewport")),
            KeyBinding::new("z", viewport::ZoomToArea, Some("BoardViewport")),
            KeyBinding::new("enter", viewport::FinishMeasurement, Some("BoardViewport")),
            KeyBinding::new(
                "backspace",
                viewport::RemoveLastPoint,
                Some("BoardViewport"),
            ),
            KeyBinding::new(
                crate::shortcuts::keys::NAVIGATE_BACK,
                viewport::NavigateBack,
                Some("BoardViewport"),
            ),
            KeyBinding::new(
                crate::shortcuts::keys::NAVIGATE_FORWARD,
                viewport::NavigateForward,
                Some("BoardViewport"),
            ),
            KeyBinding::new("f", viewport::FitBoard, Some("BoardViewport")),
            KeyBinding::new("f11", viewport::ZoomIn, Some("BoardViewport")),
            KeyBinding::new("f12", viewport::ZoomOut, Some("BoardViewport")),
            KeyBinding::new("b", viewport::FlipBoard, Some("BoardViewport")),
            KeyBinding::new("g", viewport::ToggleGrid, Some("BoardViewport")),
            KeyBinding::new("pagedown", viewport::NextActiveLayer, Some("BoardViewport")),
            KeyBinding::new(
                "pageup",
                viewport::PreviousActiveLayer,
                Some("BoardViewport"),
            ),
            KeyBinding::new("l", viewport::ToggleSingleLayer, Some("BoardViewport")),
            KeyBinding::new("n", viewport::NextCandidate, Some("BoardViewport")),
            KeyBinding::new("=", viewport::ZoomIn, Some("BoardViewport")),
            KeyBinding::new("shift-=", viewport::ZoomIn, Some("BoardViewport")),
            KeyBinding::new("-", viewport::ZoomOut, Some("BoardViewport")),
            KeyBinding::new("left", viewport::PanLeft, Some("BoardViewport")),
            KeyBinding::new("right", viewport::PanRight, Some("BoardViewport")),
            KeyBinding::new("up", viewport::PanUp, Some("BoardViewport")),
            KeyBinding::new("down", viewport::PanDown, Some("BoardViewport")),
        ]);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let result = open_window(crate::main_window::options(cx), cx, |window, cx| {
            window.set_window_title("Pomelo");
            #[cfg(feature = "window-profiling")]
            crate::window_timing::install(window, cx);
            let workbench = cx.new(|cx| {
                Workbench::new(
                    startup.options.clone(),
                    startup.source_font.clone(),
                    window,
                    cx,
                )
            });
            let quit_workbench = workbench.downgrade();
            cx.on_action(move |_: &Quit, cx| {
                let _ = quit_workbench.update(cx, |this, cx| this.request_quit(cx));
            });
            let close_workbench = workbench.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                let _ = close_workbench.update(cx, |this, cx| this.request_quit(cx));
                false
            });
            window.focus(&workbench.read(cx).focus_handle(cx), cx);
            for path in startup.paths.clone() {
                workbench.update(cx, |this, cx| this.open_path(path, window, cx));
            }
            workbench
        });
        if let Err(error) = result {
            failed.set(true);
            startup_error::report(
                i18n::current(cx),
                &Diagnostic::error("APP_WINDOW_FAILED", MessageKey::WindowFailed)
                    .with_details(error.to_string()),
            );
            cx.quit();
            return;
        }
        cx.activate(true);
    })
}

fn run_application(
    locale: Locale,
    on_launch: impl FnOnce(&mut App, Rc<Cell<bool>>) + 'static,
) -> ExitCode {
    let application = match startup_error::try_initialize(application) {
        Ok(application) => application,
        Err(diagnostic) => {
            startup_error::report(locale, &diagnostic);
            return ExitCode::FAILURE;
        }
    };
    let failed = Rc::new(Cell::new(false));
    let callback_failed = failed.clone();
    application
        .with_assets(crate::assets::Assets)
        .run(move |cx| on_launch(cx, callback_failed));
    ExitCode::from(u8::from(failed.get()))
}
