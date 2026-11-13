//! Theme restoration and serialized preference writes.

use crate::services::preferences::{ThemePreference, ThemeStore};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, AppContext, BorrowAppContext, Global, Hsla, Task, Window};
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

/// "Ink & Stage" palette from the UI redesign: neutral chrome so the board keeps
/// every hue, and one accent reserved for the default commit, the current
/// canvas tool and keyboard focus. Feature code uses semantic roles only.
fn apply_palette(cx: &mut App) {
    use gpui_kit::rgb;
    Theme::update(cx, |theme| {
        let dark = theme.is_dark();
        let pick = |dark_value: u32, light_value: u32| -> Hsla {
            rgb(if dark { dark_value } else { light_value }).into()
        };
        // Panels, chrome and the window share one flat surface; hairlines and
        // the framed canvas stage separate them instead of tinted slabs.
        let surface = pick(0x0b0b0c, 0xffffff);
        let foreground = pick(0xf2f2f0, 0x0f0f10);
        let muted = pick(0xa8a8a6, 0x5c5c5a);
        let border = pick(0x2a2a2d, 0xddddda);
        let primary = pick(0xc5e86c, 0x4f7a14);
        let primary_fg = pick(0x10160a, 0xffffff);
        // Neutral fills for selected rows, segments and pressed controls.
        let selected = pick(0x252527, 0xededeb);
        let hover = pick(0x1a1a1c, 0xf4f4f2);
        let raised = pick(0x161618, 0xf6f6f4);
        theme.colors.background = surface;
        theme.colors.foreground = foreground;
        theme.colors.muted = raised;
        theme.colors.muted_foreground = muted;
        theme.colors.border = border;
        theme.colors.input = border;
        theme.colors.title_bar = surface;
        theme.colors.title_bar_border = surface;
        theme.colors.status_bar = surface;
        theme.colors.status_bar_border = surface;
        theme.colors.sidebar = surface;
        theme.colors.sidebar_foreground = foreground;
        theme.colors.sidebar_border = border;
        theme.colors.popover = pick(0x111113, 0xffffff);
        theme.colors.popover_foreground = foreground;
        theme.colors.primary = primary;
        theme.colors.primary_hover = primary;
        theme.colors.primary_active = primary;
        theme.colors.primary_foreground = primary_fg;
        theme.colors.button_primary = primary;
        theme.colors.button_primary_hover = primary;
        theme.colors.button_primary_active = primary;
        theme.colors.button_primary_foreground = primary_fg;
        theme.colors.ring = primary;
        // Selection is neutral; the brand color stays reserved for commits and focus.
        theme.colors.accent = selected;
        theme.colors.accent_foreground = foreground;
        theme.colors.secondary = raised;
        theme.colors.secondary_foreground = foreground;
        theme.colors.secondary_hover = hover;
        theme.colors.secondary_active = selected;
        theme.colors.button = surface;
        theme.colors.button_hover = hover;
        theme.colors.button_active = selected;
        theme.colors.button_foreground = foreground;
        theme.colors.tab_bar = surface;
        theme.colors.tab_active = surface;
        theme.colors.tab_active_foreground = foreground;
        theme.colors.tab_foreground = muted;
        // The track sits one step above the surface; the selected segment
        // takes the strongest neutral so it reads without the accent.
        theme.colors.tab_bar_segmented = raised;
        theme.colors.list_active = selected;
        theme.colors.list_active_border = selected;
        theme.colors.list_hover = hover;
        theme.colors.switch = pick(0x2e2e32, 0xcfcfcb);
        theme.colors.slider_bar = foreground;
        theme.colors.slider_thumb = foreground;
        theme.colors.scrollbar_thumb = pick(0x34343a, 0xcfcfcb);
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

/// The canvas stage stays dark in both UI themes: background, ink and hairline.
pub fn canvas_colors() -> (gpui_kit::Hsla, gpui_kit::Hsla, gpui_kit::Hsla) {
    use gpui_kit::rgb;
    (
        rgb(0x050505).into(),
        rgb(0xd6d6d4).into(),
        rgb(0x2a2a2d).into(),
    )
}

/// Secondary ink for labels floating over the canvas.
pub fn canvas_muted() -> gpui_kit::Hsla {
    gpui_kit::rgb(0xa8a8a6).into()
}

/// Frame around the canvas stage: the dark hairline in the dark theme, the
/// ink color in the light theme so the stage reads as one object.
pub fn stage_border(dark: bool) -> gpui_kit::Hsla {
    gpui_kit::rgb(if dark { 0x2a2a2d } else { 0x0f0f10 }).into()
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

/// Measurement and review marks: a warm accent that stays legible on the dark canvas.
pub fn canvas_measure() -> gpui_kit::Hsla {
    gpui_kit::rgb(0xffd54f).into()
}

/// The canvas command surface stays dark with the board in either UI theme.
pub fn canvas_toolbar_surface() -> gpui_kit::Hsla {
    gpui_kit::rgb(0x0b0b0c).into()
}

/// Neutral fill for an open canvas popup trigger or a pressed canvas control.
pub fn canvas_toolbar_open() -> gpui_kit::Hsla {
    gpui_kit::rgb(0x2a2a2d).into()
}

pub fn canvas_toolbar_button(cx: &App) -> gpui_kit::component::button::ButtonCustomVariant {
    gpui_kit::component::button::ButtonCustomVariant::new(cx)
        .foreground(canvas_colors().1)
        .hover(gpui_kit::rgb(0x1c1c1f).into())
        .active(canvas_toolbar_open())
}

/// The current canvas tool is one of the three places the accent appears.
pub fn canvas_toolbar_current(cx: &App) -> gpui_kit::component::button::ButtonCustomVariant {
    let accent: gpui_kit::Hsla = gpui_kit::rgb(0xc5e86c).into();
    gpui_kit::component::button::ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(gpui_kit::rgb(0x10160a).into())
        .hover(accent)
        .active(accent)
}

fn missing_directory() -> Diagnostic {
    Diagnostic::error(
        "CONFIG_DIRECTORY_MISSING",
        MessageKey::ConfigDirectoryMissing,
    )
}
