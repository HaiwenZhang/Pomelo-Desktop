//! Keep localized sidebar destinations reachable when a panel is narrowed.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::{
    assets::IconName,
    component::{
        Sizable,
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        tab::{Tab, TabBar},
    },
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};
use pomelo_core::i18n::{Locale, MessageKey, text};
use std::rc::Rc;

pub struct SidebarTabs {
    pub id: &'static str,
    pub locale: Locale,
    pub labels: Vec<SharedString>,
    pub selected: usize,
    pub width: Pixels,
}

/// Render a tab list and a separate focusable overflow command.
pub fn render(
    tabs: SidebarTabs,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let overflow = needs_menu(&tabs.labels, tabs.width, window, cx);
    let on_select = Rc::new(on_select);
    let tab_select = Rc::clone(&on_select);
    div()
        .w_full()
        .flex()
        .items_center()
        .flex_shrink_0()
        .child(
            TabBar::new(tabs.id)
                .underline()
                .large()
                .px_4()
                .flex_1()
                .min_w_0()
                .selected_index(tabs.selected)
                .children(tabs.labels.iter().enumerate().map(|(index, label)| {
                    Tab::new()
                        .label(label.clone())
                        .when(tabs.selected == index, |tab| {
                            tab.font_weight(FontWeight::SEMIBOLD)
                        })
                }))
                .on_click(move |index, window, cx| tab_select(index, window, cx)),
        )
        .when(overflow, |header| {
            header.child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .self_stretch()
                    .pr_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(overflow_menu(
                        tabs.id,
                        tabs.locale,
                        tabs.labels,
                        tabs.selected,
                        move |index, window, cx| on_select(index, window, cx),
                    )),
            )
        })
        .into_any_element()
}

fn overflow_menu(
    id: &'static str,
    locale: Locale,
    labels: Vec<SharedString>,
    selected: usize,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_select = Rc::new(on_select);
    Button::new(SharedString::from(format!("{id}-menu")))
        .ghost()
        .xsmall()
        .icon(IconName::ChevronDown)
        .accessibility_label(text(locale, MessageKey::PanelNavigation))
        .native_tooltip(text(locale, MessageKey::PanelNavigation))
        .dropdown_menu(move |mut menu, _, _| {
            for (index, label) in labels.iter().enumerate() {
                let on_select = Rc::clone(&on_select);
                menu = menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(selected == index)
                        .on_click(move |_, window, cx| on_select(&index, window, cx)),
                );
            }
            menu
        })
}

fn needs_menu(labels: &[SharedString], width: Pixels, window: &Window, cx: &App) -> bool {
    let mut font = window.text_style().font();
    font.family = cx.theme().font_family.clone();
    // Measure every destination at the selected weight to avoid changing the
    // overflow affordance merely because another tab becomes selected.
    font.weight = FontWeight::SEMIBOLD;
    let labels_width = labels.iter().fold(px(0.0), |width, label| {
        let run = TextRun {
            len: label.len(),
            font: font.clone(),
            ..Default::default()
        };
        width
            + window
                .text_system()
                .shape_line(label.clone(), window.rem_size(), &[run], None)
                .width
    });
    // GPUI Kit 0.7 Large underline tabs use a 20px gap; the bar uses px_4.
    labels_width + px(20.0 * labels.len().saturating_sub(1) as f32) + window.rem_size() * 2.0
        > width
}
