//! Standard collapse commands in sidebar headers and compact expansion rails.
use crate::{tooltips::ButtonTooltipExt, workbench::panel_layout::Side};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::i18n::{Locale, MessageKey as Key, text};

pub(crate) fn toggle_button(
    side: Side,
    collapsed: bool,
    locale: Locale,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let (id, key, icon, shortcut) = match (side, collapsed) {
        (Side::Left, false) => (
            "collapse-left-panel",
            Key::CollapseLeftPanel,
            IconName::PanelLeftClose,
            "Ctrl+B",
        ),
        (Side::Left, true) => (
            "expand-left-panel",
            Key::ExpandLeftPanel,
            IconName::PanelLeftOpen,
            "Ctrl+B",
        ),
        (Side::Right, false) => (
            "collapse-right-panel",
            Key::CollapseRightPanel,
            IconName::PanelRightClose,
            "Ctrl+Shift+B",
        ),
        (Side::Right, true) => (
            "expand-right-panel",
            Key::ExpandRightPanel,
            IconName::PanelRightOpen,
            "Ctrl+Shift+B",
        ),
    };
    Button::new(id)
        .ghost()
        .small()
        .size_8()
        .icon(icon)
        .accessibility_label(text(locale, key))
        .native_tooltip(format!("{} ({shortcut})", text(locale, key)))
        .on_click(on_click)
}
pub(crate) fn rail(side: Side, button: Button, cx: &App) -> impl IntoElement {
    div()
        .w_10()
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .pt_2()
        .bg(cx.theme().sidebar)
        .border_color(cx.theme().border)
        .map(|rail| match side {
            Side::Left => rail.border_r_1(),
            Side::Right => rail.border_l_1(),
        })
        .child(button)
}
pub(crate) fn header(tabs: AnyElement, side: Side, button: Button) -> impl IntoElement {
    let row = div().flex().items_center().flex_shrink_0();
    let tabs = div().flex_1().min_w_0().child(tabs);
    let command = div().flex_none().px_1().child(button);
    match side {
        Side::Left => row.child(tabs).child(command),
        Side::Right => row.child(command).child(tabs),
    }
}
