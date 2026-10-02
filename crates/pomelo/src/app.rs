//! Application initialization and native event-loop entry.
use crate::actions::{
    CancelImport, CloseDocument, OpenFile, OpenSettings, Quit, ReloadDocument, ToggleTheme,
};
use crate::workbench::Workbench;
use crate::{gpu_demo, i18n, recent, startup, theme};
#[cfg(target_os = "windows")]
use crate::{native_gpu_demo, viewport};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use pomelo_core::i18n::{MessageKey, text};

pub fn run() {
    if std::env::args_os().any(|argument| argument == "--native-gpu-demo") {
        #[cfg(target_os = "windows")]
        run_native_gpu_demo();
        #[cfg(not(target_os = "windows"))]
        eprintln!(
            "{}",
            text(pomelo_core::i18n::Locale::English, MessageKey::GpuFailed)
        );
        return;
    }
    let gpu_demo = std::env::args_os().any(|argument| argument == "--gpu-demo");
    if gpu_demo {
        run_gpu_demo();
        return;
    }
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let diagnostic_locale = startup::Startup::diagnostic_locale(&arguments);
    let startup = match startup::Startup::parse(arguments) {
        Ok(startup) => startup,
        Err(error) => {
            eprintln!("{}", error.message.display(diagnostic_locale));
            std::process::exit(2);
        }
    };
    application().with_assets(assets::Assets).run(move |cx| {
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
            KeyBinding::new("secondary-q", Quit, None),
        ]);
        #[cfg(target_os = "windows")]
        cx.bind_keys([
            KeyBinding::new("secondary-k", viewport::FocusSearch, Some("BoardWorkspace")),
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
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let result = open_window(crate::main_window::options(cx), cx, |window, cx| {
            let workbench = cx.new(|cx| Workbench::new(startup.options.clone(), cx));
            window.on_window_should_close(cx, |_, cx| {
                cx.quit();
                false
            });
            window.focus(&workbench.read(cx).focus_handle(cx), cx);
            for path in startup.paths.clone() {
                workbench.update(cx, |this, cx| this.open_path(path, window, cx));
            }
            workbench
        });
        if let Err(error) = result {
            eprintln!(
                "{}\n{error}",
                text(i18n::current(cx), MessageKey::WindowFailed)
            );
            cx.quit();
        }
        cx.activate(true);
    });
}

fn run_gpu_demo() {
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);
        i18n::initialize(cx, None);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-r", gpu_demo::RenderTriangle, None),
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
                    title: Some(text(i18n::current(cx), MessageKey::GpuDemoTitle).into()),
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
                let demo = cx.new(|cx| gpu_demo::GpuDemo::new(window, cx));
                window.focus(&demo.read(cx).focus_handle(cx), cx);
                demo
            },
        );
        if let Err(error) = result {
            eprintln!(
                "{}\n{error}",
                text(i18n::current(cx), MessageKey::WindowFailed)
            );
            cx.quit();
        }
        cx.activate(true);
    });
}

#[cfg(target_os = "windows")]
fn run_native_gpu_demo() {
    application().with_assets(assets::Assets).run(|cx| {
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
            eprintln!(
                "{}\n{error}",
                text(i18n::current(cx), MessageKey::WindowFailed)
            );
            cx.quit();
        }
        cx.activate(true);
    });
}
