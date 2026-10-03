//! Main native window configuration.
use gpui_kit::component::TitleBar;
use gpui_kit::*;
pub fn options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1440.0), px(920.0)),
            cx,
        ))),
        window_min_size: Some(size(px(1000.0), px(650.0))),
        ..TitleBar::window_options()
    }
}
