use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable;
use gpui_kit::prelude::FluentBuilder;
// Welcome workspace, with real recent files and local file commands.
pub mod recent;
use crate::panels::focus_scroll::FocusScroll;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
};
use gpui_kit::*;
use pomelo_core::i18n::{Locale, MessageKey as Key, text};

const SIDEBAR_REMS: f32 = 16.0;
const CONTENT_INSET_REMS: f32 = 4.0;
const CONTENT_TOP_INSET_REMS: f32 = 3.5;
// The padded file entrance occupies about 55% of the two-column content band.
const DROP_PANEL_GROW: f32 = 1.1;
const FEATURE_GAP_REMS: f32 = 1.5;
const FEATURE_MIN_REMS: f32 = 20.0;

/// Independent scroll regions; the workbench keeps them across renders.
#[derive(Default)]
pub struct ScrollState {
    pub content: ScrollHandle,
    pub sidebar: ScrollHandle,
    pub preview: ScrollHandle,
    pub all: ScrollHandle,
}

pub fn render(
    locale: Locale,
    recent: AnyElement,
    compact_recent: AnyElement,
    recent_only: bool,
    scroll: &ScrollState,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(
            div()
                .w(rems(SIDEBAR_REMS))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .bg(theme.sidebar)
                .border_r_1()
                .border_color(theme.border)
                .p_3()
                .gap_2()
                .child(
                    div()
                        .px_2()
                        .py_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::Workspace)),
                )
                .child(
                    nav_button(
                        "welcome-start",
                        IconName::FolderOpen,
                        text(locale, Key::StartPage),
                        !recent_only,
                    )
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ShowStartPage), cx)
                    }),
                )
                .child(
                    nav_button(
                        "welcome-recent",
                        IconName::Clock,
                        text(locale, Key::RecentFiles),
                        recent_only,
                    )
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ShowRecentFiles), cx)
                    }),
                )
                .child(
                    nav_button(
                        "welcome-settings",
                        IconName::Settings,
                        text(locale, Key::SettingsMenu),
                        false,
                    )
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)
                    }),
                )
                .child(
                    div()
                        .mt_6()
                        .pt_4()
                        .border_t_1()
                        .border_color(theme.border)
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(compact_recent),
                )
                .child(
                    div()
                        .px_2()
                        .py_4()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::ProductNative))
                        .child(div().child(text(locale, Key::ProductViewer))),
                ),
        )
        .child(if recent_only {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .min_h_0()
                .min_w_0()
                .p(rems(CONTENT_INSET_REMS))
                .child(recent)
                .into_any_element()
        } else {
            div()
                .id("welcome-content")
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll.content)
                .vertical_scrollbar(&scroll.content)
                .child(
                    div()
                        .p(rems(CONTENT_INSET_REMS))
                        .pt(rems(CONTENT_TOP_INSET_REMS))
                        .flex()
                        .flex_col()
                        .gap_6()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(text(locale, Key::LocalWorkspace)),
                                )
                                .child(
                                    div()
                                        .text_size(rems(2.25))
                                        .line_height(relative(1.2))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(text(locale, Key::WelcomeTitle)),
                                )
                                .child(
                                    div()
                                        .text_lg()
                                        .text_color(theme.muted_foreground)
                                        .child(text(locale, Key::WelcomeDescription)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .min_w_0()
                                .gap_8()
                                .child(
                                    div()
                                        .flex_1()
                                        .flex_grow(DROP_PANEL_GROW)
                                        .min_w(rems(20.0))
                                        .border_1()
                                        .border_dashed()
                                        .border_color(theme.muted_foreground.opacity(0.45))
                                        .drag_over::<ExternalPaths>(|style, _, _, cx| {
                                            style
                                                .border_color(cx.theme().primary)
                                                .bg(cx.theme().primary.opacity(0.06))
                                        })
                                        .rounded_lg()
                                        .p_8()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_5()
                                        .min_h_80()
                                        .child(
                                            Icon::new(IconName::FilePlus)
                                                .size_16()
                                                .text_color(theme.muted_foreground),
                                        )
                                        .child(
                                            div()
                                                .text_xl()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(text(locale, Key::DropTitle)),
                                        )
                                        .child(
                                            div()
                                                .text_base()
                                                .text_color(theme.muted_foreground)
                                                .child(text(locale, Key::SupportedFormats)),
                                        )
                                        .child(FocusScroll::new(
                                            "welcome-open-focus",
                                            &scroll.content,
                                            Button::new("welcome-open")
                                                .primary()
                                                .large()
                                                .h_12()
                                                .w_64()
                                                .accessibility_label(text(locale, Key::OpenFile))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_4()
                                                        .child(
                                                            Icon::new(IconName::FolderOpen)
                                                                .size_5(),
                                                        )
                                                        .child(text(locale, Key::OpenFile))
                                                        .child(
                                                            div()
                                                                .pl_4()
                                                                .border_l_1()
                                                                .border_color(
                                                                    theme.primary_foreground,
                                                                )
                                                                .child(text(
                                                                    locale,
                                                                    Key::ShortcutOpen,
                                                                )),
                                                        ),
                                                )
                                                .on_click(|_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(crate::actions::OpenFile),
                                                        cx,
                                                    )
                                                }),
                                        ))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(theme.muted_foreground)
                                                .child(text(locale, Key::BrdExtension)),
                                        ),
                                )
                                .child(div().flex_1().min_w(rems(22.5)).child(recent)),
                        )
                        .child(features(locale, window, cx))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_5()
                                .pt_6()
                                .border_t_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(text(locale, Key::QuickStart)),
                                )
                                .children(
                                    [
                                        ("Ctrl O", Key::OpenFile),
                                        ("Ctrl K", Key::SearchBoard),
                                        ("F2", Key::FitBoard),
                                    ]
                                    .into_iter()
                                    .map(|(shortcut, key)| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .border_1()
                                                    .border_color(theme.border)
                                                    .rounded_md()
                                                    .px_3()
                                                    .py_1()
                                                    .text_sm()
                                                    .child(shortcut),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(theme.muted_foreground)
                                                    .child(text(locale, key)),
                                            )
                                    }),
                                ),
                        ),
                )
                .into_any_element()
        })
        .into_any_element()
}

fn features(locale: Locale, window: &Window, cx: &App) -> AnyElement {
    let theme = cx.theme();
    // Preserve three readable groups after the sidebar and both content insets.
    let row_min_rems =
        SIDEBAR_REMS + 2.0 * CONTENT_INSET_REMS + 3.0 * FEATURE_MIN_REMS + 2.0 * FEATURE_GAP_REMS;
    let horizontal = window.viewport_size().width >= window.rem_size() * row_min_rems;
    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .when(horizontal, |band| band.flex_row())
        .gap(rems(FEATURE_GAP_REMS))
        .py_4()
        .children(
            [
                (
                    IconName::LockKeyhole,
                    Key::PrivacyTitle,
                    Key::PrivacyDescription,
                ),
                (IconName::Layers, Key::ExploreTitle, Key::ExploreDescription),
                (
                    IconName::Keyboard,
                    Key::DesktopTitle,
                    Key::DesktopDescription,
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (icon, title, description))| {
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap_4()
                    .when(horizontal, |feature| feature.flex_1().justify_center())
                    .when(!horizontal, |feature| feature.w_full())
                    .when(index < 2, |feature| {
                        feature.border_color(theme.border).map(|feature| {
                            if horizontal {
                                feature.pr_4().border_r_1()
                            } else {
                                feature.pb_6().border_b_1()
                            }
                        })
                    })
                    .child(
                        Icon::new(icon)
                            .size_12()
                            .flex_shrink_0()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .when(!horizontal, |copy| copy.flex_1())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(text(locale, title)),
                            )
                            .child(
                                div()
                                    .text_base()
                                    .text_color(theme.muted_foreground)
                                    .child(text(locale, description)),
                            ),
                    )
            }),
        )
        .into_any_element()
}

fn nav_button(id: &'static str, icon: IconName, label: String, selected: bool) -> Button {
    Button::new(id)
        .ghost()
        .large()
        .h_12()
        .w_full()
        .selected(selected)
        .accessibility_label(label.clone())
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_4()
                .child(Icon::new(icon).size_5())
                .child(label),
        )
}
