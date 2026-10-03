//! File information presentation; expansion and diagnostic state stay in the viewport.
use gpui_kit::{
    base::Button,
    component::{ActiveTheme, Icon, collapsible::Collapsible},
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
}

pub fn render(
    information: FileInformation<'_>,
    diagnostics: AnyElement,
    focus: &super::focus_scroll::FocusReveal,
    on_toggle: impl Fn(&bool, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let FileInformation {
        locale,
        expanded,
        scene,
    } = information;
    // The Kit Accordion trigger in the pinned version has no keyboard focus
    // path. Compose its standard disclosure surface from Collapsible and Base
    // Button, which owns pointer/Enter/Space and expanded accessibility state.
    Collapsible::new()
        .open(expanded)
        .w_full()
        .flex_shrink_0()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            focus.wrap(
                Button::new("file-information-toggle")
                    .w_full()
                    .h_10()
                    .px_4()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .border_1()
                    .border_color(if focus.contains_focus(window, cx) {
                        cx.theme().ring
                    } else {
                        transparent_black()
                    })
                    .hover(|style| style.bg(cx.theme().button_hover))
                    .active(|style| style.bg(cx.theme().button_active))
                    .focus_visible(|style| style.border_1().border_color(cx.theme().ring))
                    .accessibility_label(text(locale, Key::FileInformation))
                    .aria_expanded(expanded)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(text(locale, Key::FileInformation))
                            .child(
                                Icon::new(if expanded {
                                    gpui_kit::assets::IconName::ChevronDown
                                } else {
                                    gpui_kit::assets::IconName::ChevronRight
                                })
                                .size_4(),
                            ),
                    )
                    .on_click(move |_, window, cx| on_toggle(&!expanded, window, cx)),
            ),
        )
        .content(
            div()
                .px_4()
                .pb_3()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, Key::BoardFormat)),
                )
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
        .into_any_element()
}
