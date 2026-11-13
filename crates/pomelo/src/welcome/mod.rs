use gpui_kit::assets::IconName;
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

const CONTENT_INSET_REMS: f32 = 4.5;
const CONTENT_TOP_INSET_REMS: f32 = 3.5;
const LEAD_MIN_REMS: f32 = 22.0;
const LEAD_MAX_REMS: f32 = 36.0;
const WALL_MIN_REMS: f32 = 24.0;

/// Independent scroll regions; the workbench keeps them across renders.
#[derive(Default)]
pub struct ScrollState {
    pub content: ScrollHandle,
    pub all: ScrollHandle,
}

/// The start page: one sentence and one action on the leading side, the most
/// recent boards as a wall on the trailing side, and three keys worth knowing.
/// The two columns wrap into one when the window is narrow.
pub fn render(
    locale: Locale,
    recent: AnyElement,
    recent_only: bool,
    scroll: &ScrollState,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    if recent_only {
        return div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .min_w_0()
            .px(rems(CONTENT_INSET_REMS))
            .py(rems(CONTENT_TOP_INSET_REMS))
            .gap_4()
            .child(
                div().child(
                    Button::new("welcome-back")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronLeft)
                        .label(text(locale, Key::StartPage))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::ShowStartPage), cx)
                        }),
                ),
            )
            .child(recent)
            .into_any_element();
    }
    let lead = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(rems(LEAD_MIN_REMS))
        .max_w(rems(LEAD_MAX_REMS))
        .child(
            div()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.muted_foreground)
                .child(text(locale, Key::WelcomeEyebrow)),
        )
        .child(
            div()
                .mt_4()
                .text_size(rems(3.25))
                .line_height(relative(1.1))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text(locale, Key::WelcomeTitle)),
        )
        .child(
            div()
                .mt_5()
                .max_w(rems(30.0))
                .text_base()
                .line_height(relative(1.6))
                .text_color(theme.muted_foreground)
                .child(text(locale, Key::WelcomeLede)),
        )
        .child(
            div()
                .mt_10()
                .min_h(rems(14.0))
                .p_6()
                .rounded_xl()
                .border_1()
                .border_dashed()
                .border_color(theme.muted_foreground.opacity(0.45))
                .drag_over::<ExternalPaths>(|style, _, _, cx| {
                    style
                        .border_color(cx.theme().primary)
                        .bg(cx.theme().primary.opacity(0.06))
                })
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    Icon::new(IconName::Upload)
                        .size_7()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::MEDIUM)
                        .text_center()
                        .child(text(locale, Key::WelcomeDropHere)),
                )
                .child(FocusScroll::new(
                    "welcome-open-focus",
                    &scroll.content,
                    Button::new("welcome-open")
                        .primary()
                        .accessibility_label(text(locale, Key::OpenFile))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .whitespace_nowrap()
                                .child(text(locale, Key::OpenFile))
                                .child(
                                    div()
                                        .px_1p5()
                                        .rounded_sm()
                                        .bg(theme.primary_foreground.opacity(0.14))
                                        .text_xs()
                                        .child(crate::shortcuts::shortcut_label(
                                            crate::shortcuts::keys::OPEN_FILE,
                                        )),
                                ),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::OpenFile), cx)
                        }),
                )),
        );
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
                .min_h_full()
                .flex()
                .flex_col()
                .px(rems(CONTENT_INSET_REMS))
                .pt(rems(CONTENT_TOP_INSET_REMS))
                .pb_6()
                .gap_10()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_start()
                        .gap_x(rems(4.5))
                        .gap_y_10()
                        .child(lead)
                        .child(div().flex_1().min_w(rems(WALL_MIN_REMS)).child(recent)),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_8()
                        .gap_y_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .children(
                            [
                                (
                                    crate::shortcuts::keys::FOCUS_SEARCH,
                                    Key::WelcomeShortcutSearch,
                                ),
                                (crate::shortcuts::keys::FIT_ACTIVE_BOARD, Key::FitBoard),
                                (
                                    crate::shortcuts::keys::SINGLE_LAYER,
                                    Key::WelcomeShortcutSingleLayer,
                                ),
                            ]
                            .into_iter()
                            .map(|(shortcut, key)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .whitespace_nowrap()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(theme.border)
                                            .text_xs()
                                            .text_color(theme.foreground)
                                            .font_family(theme.mono_font_family.clone())
                                            .child(crate::shortcuts::shortcut_label(shortcut)),
                                    )
                                    .child(text(locale, key))
                            }),
                        ),
                ),
        )
        .into_any_element()
}
