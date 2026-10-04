//! Application initialization and native event-loop entry.
use crate::actions::{
    CancelImport, CloseDocument, FitActiveBoard, NextDocument, OpenFile, OpenSettings,
    PreviousDocument, Quit, ReloadDocument, ToggleLeftPanel, ToggleRightPanel, ToggleTheme,
};
use crate::services::startup_error;
use crate::workbench::Workbench;
use crate::{i18n, recent, startup, theme};
#[cfg(target_os = "windows")]
use crate::{native_gpu_demo, viewport};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use pomelo_core::{
    i18n::{Locale, MessageKey, text},
    model::Diagnostic,
};
use std::{cell::Cell, process::ExitCode, rc::Rc};

pub fn run() -> ExitCode {
    let preferred_locale = startup_error::preferred_locale();
    if std::env::args_os().any(|argument| argument == "--native-gpu-demo") {
        #[cfg(target_os = "windows")]
        return run_native_gpu_demo(preferred_locale);
        #[cfg(not(target_os = "windows"))]
        {
            startup_error::report(
                preferred_locale,
                &Diagnostic::error("APP_GPU_UNSUPPORTED", MessageKey::GpuFailed),
            );
            return ExitCode::FAILURE;
        }
    }
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
            KeyBinding::new("secondary-o", OpenFile, Some("Pomelo")),
            KeyBinding::new("secondary-w", CloseDocument, Some("Pomelo")),
            KeyBinding::new("secondary-r", ReloadDocument, Some("Pomelo")),
            KeyBinding::new("escape", CancelImport, Some("Pomelo")),
            KeyBinding::new("secondary-t", ToggleTheme, Some("Pomelo")),
            KeyBinding::new("secondary-,", OpenSettings, Some("Pomelo")),
            KeyBinding::new("secondary-tab", NextDocument, Some("Pomelo")),
            KeyBinding::new("secondary-shift-tab", PreviousDocument, Some("Pomelo")),
            KeyBinding::new("f2", FitActiveBoard, Some("Pomelo")),
            KeyBinding::new("secondary-b", ToggleLeftPanel, Some("Pomelo")),
            KeyBinding::new("secondary-shift-b", ToggleRightPanel, Some("Pomelo")),
            KeyBinding::new("secondary-q", Quit, None),
        ]);
        #[cfg(target_os = "windows")]
        cx.bind_keys([
            KeyBinding::new("secondary-k", viewport::FocusSearch, Some("Pomelo")),
            KeyBinding::new("escape", viewport::LeaveSearch, Some("BoardWorkspace")),
            KeyBinding::new("escape", viewport::ClearSelection, Some("BoardViewport")),
            KeyBinding::new("f", viewport::FitBoard, Some("BoardViewport")),
            KeyBinding::new("b", viewport::FlipBoard, Some("BoardViewport")),
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

#[cfg(target_os = "windows")]
fn run_native_gpu_demo(locale: Locale) -> ExitCode {
    run_application(locale, |cx, failed| {
        init(cx);
        i18n::initialize(cx, None);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-r", native_gpu_demo::RotateColors, None),
            KeyBinding::new("secondary-c", native_gpu_demo::ToggleClip, None),
            KeyBinding::new("secondary-o", native_gpu_demo::ShowOverlay, None),
            KeyBinding::new("secondary-t", ToggleTheme, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let result = open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(text(i18n::current(cx), MessageKey::NativeGpuTitle).into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.0), px(780.0)),
                    cx,
                ))),
                window_min_size: Some(size(px(720.0), px(560.0))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let demo = cx.new(|cx| native_gpu_demo::NativeGpuDemo::new(window, cx));
                window.focus(&demo.read(cx).focus_handle(cx), cx);
                demo
            },
        );
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
