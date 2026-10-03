//! Theme restoration and serialized preference writes.

use crate::prefs::{ThemePreference, ThemeStore};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, AppContext, BorrowAppContext, Global, Task, Window};
use pomelo_core::{i18n::MessageKey, model::Diagnostic};

pub struct ThemeState {
    pub preference: ThemePreference,
    pub warning: Option<Diagnostic>,
    store: Option<ThemeStore>,
    save_task: Option<Task<()>>,
    revision: u64,
}

impl Global for ThemeState {}

pub fn initialize(cx: &mut App) {
    let store = ThemeStore::platform_default();
    let (preference, warning) = match &store {
        Some(store) => match store.load() {
            Ok(preference) => (preference, None),
            Err(error) => (ThemePreference::default(), Some(error)),
        },
        None => (ThemePreference::default(), Some(missing_directory())),
    };
    Theme::change(mode(preference), None, cx);
    apply_palette(cx);
    cx.set_global(ThemeState {
        preference,
        warning,
        store,
        save_task: None,
        revision: 0,
    });
    cx.on_app_quit(|cx| {
        let pending = cx.update_global::<ThemeState, _>(|state, _| state.save_task.take());
        async move {
            if let Some(pending) = pending {
                pending.await;
            }
        }
    })
    .detach();
}

pub fn toggle(window: &mut Window, cx: &mut App) {
    let preference = match cx.global::<ThemeState>().preference {
        ThemePreference::Dark => ThemePreference::Light,
        ThemePreference::Light => ThemePreference::Dark,
    };
    select(preference, window, cx);
}

/// Apply an explicit choice; serialize saves with the existing toolbar toggle.
pub fn select(preference: ThemePreference, window: &mut Window, cx: &mut App) {
    let (previous, store, revision) = cx.update_global::<ThemeState, _>(|state, _| {
        state.preference = preference;
        state.warning = None;
        state.revision = state.revision.wrapping_add(1);
        (state.save_task.take(), state.store.clone(), state.revision)
    });
    Theme::change(mode(preference), Some(window), cx);
    apply_palette(cx);
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_spawn(async move {
                match store {
                    Some(store) => store.save(preference),
                    None => Err(missing_directory()),
                }
            })
            .await;
        cx.update_global::<ThemeState, _>(|state, cx| {
            if state.revision == revision {
                state.warning = result.err();
                cx.refresh_windows();
            }
        });
    });
    cx.update_global::<ThemeState, _>(|state, _| state.save_task = Some(task));
}

fn mode(preference: ThemePreference) -> ThemeMode {
    match preference {
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::Light => ThemeMode::Light,
    }
}

/// Product palette sampled from docs/ui-design; feature code uses semantic roles.
fn apply_palette(cx: &mut App) {
    use gpui_kit::rgb;
    Theme::update(cx, |theme| {
        let dark = theme.is_dark();
        let surface = rgb(if dark { 0x1d2522 } else { 0xfafbf6 }).into();
        let foreground = rgb(if dark { 0xecf0ed } else { 0x18201c }).into();
        let muted = rgb(if dark { 0xaab5b3 } else { 0x66716c }).into();
        let border = rgb(if dark { 0x36413c } else { 0xdce2d8 }).into();
        let primary = rgb(if dark { 0xa3cf72 } else { 0x587c30 }).into();
        let primary_fg = rgb(if dark { 0x15200d } else { 0xffffff }).into();
        theme.colors.background = surface;
        theme.colors.foreground = foreground;
        theme.colors.sidebar = surface;
        theme.colors.sidebar_foreground = foreground;
        theme.colors.title_bar = surface;
        theme.colors.title_bar_border = border;
        theme.colors.border = border;
        theme.colors.input = border;
        theme.colors.muted_foreground = muted;
        theme.colors.primary = primary;
        theme.colors.primary_foreground = primary_fg;
        theme.colors.button_primary = primary;
        theme.colors.button_primary_foreground = primary_fg;
        theme.colors.ring = primary;
        theme.colors.accent = rgb(if dark { 0x304332 } else { 0xeaf3dd }).into();
        theme.colors.accent_foreground = primary;
        theme.colors.secondary = rgb(if dark { 0x26312b } else { 0xf0f3e9 }).into();
        theme.colors.secondary_foreground = foreground;
        theme.colors.secondary_hover = theme.colors.accent;
        theme.colors.secondary_active = theme.colors.accent;
        theme.colors.button_active = theme.colors.accent;
        theme.colors.button_hover = theme.colors.accent;
        theme.colors.button_foreground = foreground;
        theme.colors.button_primary_hover = primary;
        theme.colors.button_primary_active = primary;
        theme.colors.tab_bar = surface;
        theme.colors.tab_active = surface;
        theme.colors.tab_active_foreground = primary;
        theme.colors.tab_foreground = muted;
    });
    apply_ui_font(cx);
}

/// Use the installed Windows UI family for the active interface language.
pub fn apply_ui_font(cx: &mut App) {
    #[cfg(target_os = "windows")]
    {
        use pomelo_core::i18n::Locale;
        let preferred: &[&str] = match crate::i18n::current(cx) {
            Locale::English => &["Segoe UI"],
            Locale::SimplifiedChinese => &["Microsoft YaHei UI", "微软雅黑 UI"],
            Locale::TraditionalChinese => &["Microsoft JhengHei UI", "微軟正黑體 UI"],
            Locale::Japanese => &["Yu Gothic UI"],
            Locale::Korean => &["Malgun Gothic", "맑은 고딕"],
        };
        // Segoe UI's CJK fallback can select a serif face in this environment.
        // Missing language fonts retain GPUI's normal system fallback.
        let installed = cx.text_system().all_font_names();
        // DirectWrite enumerates localized family names using the OS locale.
        let family = preferred
            .iter()
            .find_map(|preferred| {
                installed
                    .iter()
                    .find(|family| family.eq_ignore_ascii_case(preferred))
            })
            .map_or_else(|| ".SystemUIFont".into(), |family| family.clone().into());
        Theme::update(cx, |theme| theme.font_family = family);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = cx;
}

/// Canvas colors remain dark in both UI themes, as in the supplied designs.
pub fn canvas_colors() -> (gpui_kit::Hsla, gpui_kit::Hsla, gpui_kit::Hsla) {
    use gpui_kit::rgb;
    (
        rgb(0x1c201c).into(),
        rgb(0xd9e1dc).into(),
        rgb(0x34473a).into(),
    )
}

/// Board overview materials stay recognizable against the dark canvas in both UI themes.
pub fn thumbnail_palette() -> pomelo_render::scene::thumbnail::Palette {
    let background: gpui_kit::Rgba = canvas_colors().0.into();
    pomelo_render::scene::thumbnail::Palette {
        background: [
            (background.r * 255.0).round() as u8,
            (background.g * 255.0).round() as u8,
            (background.b * 255.0).round() as u8,
        ],
        copper: [104, 180, 96],
        pads: [179, 211, 115],
        outline: [227, 199, 86],
        drawings: [127, 169, 132],
    }
}

pub fn canvas_axes() -> (gpui_kit::Hsla, gpui_kit::Hsla) {
    (
        gpui_kit::rgb(0xfb6370).into(),
        gpui_kit::rgb(0x9ede68).into(),
    )
}

/// The canvas command surface stays dark with the board in either UI theme.
pub fn canvas_toolbar_surface() -> gpui_kit::Hsla {
    gpui_kit::rgb(0x1d2522).into()
}

pub fn canvas_toolbar_button(cx: &App) -> gpui_kit::component::button::ButtonCustomVariant {
    gpui_kit::component::button::ButtonCustomVariant::new(cx)
        .foreground(canvas_colors().1)
        .hover(gpui_kit::rgb(0x304332).into())
        .active(gpui_kit::rgb(0x3b542f).into())
}

fn missing_directory() -> Diagnostic {
    Diagnostic::error(
        "CONFIG_DIRECTORY_MISSING",
        MessageKey::ConfigDirectoryMissing,
    )
}
