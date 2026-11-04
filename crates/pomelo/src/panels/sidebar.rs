//! Standard collapse and expand commands for the side panels.
use crate::{tooltips::ButtonTooltipExt, workbench::panel_layout::Side};
use gpui_kit::{
    assets::IconName,
    component::{
        Sizable,
        button::{Button, ButtonVariants},
    },
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
            "left-panel-toggle",
            Key::CollapseLeftPanel,
            IconName::PanelLeft,
            crate::shortcuts::keys::TOGGLE_LEFT_PANEL,
        ),
        (Side::Left, true) => (
            "left-panel-toggle",
            Key::ExpandLeftPanel,
            IconName::PanelLeft,
            crate::shortcuts::keys::TOGGLE_LEFT_PANEL,
        ),
        (Side::Right, false) => (
            "right-panel-toggle",
            Key::CollapseRightPanel,
            IconName::PanelRight,
            crate::shortcuts::keys::TOGGLE_RIGHT_PANEL,
        ),
        (Side::Right, true) => (
            "right-panel-toggle",
            Key::ExpandRightPanel,
            IconName::PanelRight,
            crate::shortcuts::keys::TOGGLE_RIGHT_PANEL,
        ),
    };
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .accessibility_label(text(locale, key))
        .native_tooltip(format!(
            "{} ({})",
            text(locale, key),
            crate::shortcuts::shortcut_label(shortcut)
        ))
        .on_click(on_click)
}
