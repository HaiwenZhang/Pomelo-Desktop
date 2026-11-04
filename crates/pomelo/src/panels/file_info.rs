//! File summary pinned to the bottom of the right panel; expansion and
//! diagnostic state stay in the viewport.
use gpui_kit::{
    assets::IconName,
    base::Button,
    component::{ActiveTheme, Icon, Sizable, collapsible::Collapsible, scroll::ScrollableElement},
    prelude::FluentBuilder,
    *,
};
use pomelo_core::{
    i18n::{Locale, Message, MessageKey as Key, text},
    model::BoardScene,
};

pub struct FileInformation<'a> {
    pub locale: Locale,
    pub expanded: bool,
    pub scene: &'a BoardScene,
    pub format_name: &'a str,
    pub diagnostic_count: usize,
}

pub fn render(
    information: FileInformation<'_>,
    diagnostics: AnyElement,
    focus: &super::focus_scroll::FocusReveal,
    scroll: &ScrollHandle,
    on_toggle: impl Fn(&bool, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let FileInformation {
        locale,
        expanded,
        scene,
        format_name,
        diagnostic_count,
    } = information;
    let theme = cx.theme();
    // The Kit Accordion trigger in the pinned version has no keyboard focus
    // path. Compose its standard disclosure surface from Collapsible and Base
    // Button, which owns pointer/Enter/Space and expanded accessibility state.
    Collapsible::new()
        .open(expanded)
        .w_full()
        .flex_shrink_0()
        .border_t_1()
        .border_color(theme.border)
        .child(
            focus.wrap(
                Button::new("file-information-toggle")
                    .w_full()
                    .h_9()
                    .px_4()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .border_1()
                    .border_color(if focus.contains_focus(window, cx) {
                        theme.ring
                    } else {
                        transparent_black()
                    })
                    .hover(|style| style.bg(theme.button_hover))
                    .active(|style| style.bg(theme.button_active))
                    .focus_visible(|style| style.border_1().border_color(theme.ring))
                    .accessibility_label(text(locale, Key::FileInformation))
                    .aria_expanded(expanded)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(IconName::FileText).xsmall().flex_shrink_0())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_left()
                                    .child(format_name.to_owned()),
                            )
                            .when(diagnostic_count > 0, |row| {
                                row.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .flex_shrink_0()
                                        .text_color(theme.foreground)
                                        .child(
                                            Icon::new(IconName::TriangleAlert)
                                                .xsmall()
                                                .text_color(theme.warning),
                                        )
                                        .child(
                                            Message::new(Key::UiNumber)
                                                .arg("value", diagnostic_count)
                                                .display(locale),
                                        ),
                                )
                            })
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronUp
                                })
                                .xsmall()
                                .flex_shrink_0(),
                            ),
                    )
                    .on_click(move |_, window, cx| on_toggle(&!expanded, window, cx)),
            ),
        )
        .content(
            div()
                .relative()
                .child(
                    div()
                        .id("file-information-content")
                        .max_h_64()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .px_4()
                        .pb_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_sm()
                        .child(
                            Message::new(Key::SceneSummary)
                                .arg("layers", scene.layers.len())
                                .arg("segments", scene.segments.len())
                                .arg("pins", scene.pins.len())
                                .arg("vias", scene.vias.len())
                                .arg("zones", scene.zones.len())
                                .display(locale),
                        )
                        .child(diagnostics),
                )
                .vertical_scrollbar(scroll),
        )
        .into_any_element()
}
