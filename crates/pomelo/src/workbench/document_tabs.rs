//! Document tab appearance; document lifetime and commands belong to Workbench.
use std::rc::Rc;

use gpui_kit::{
    assets::IconName,
    base::{Tab, Tabs},
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        tag::Tag,
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::i18n::{Locale, Message, MessageKey as Key, text};

use crate::{panels::focus_scroll::FocusScroll, tooltips::ButtonTooltipExt};

pub struct DocumentTab {
    pub label: SharedString,
    pub format: Option<SharedString>,
}

pub struct DocumentTabs {
    pub items: Vec<DocumentTab>,
    pub selected: usize,
    pub locale: Locale,
    pub scroll: ScrollHandle,
}

pub fn render(
    tabs: DocumentTabs,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&usize, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let on_select = Rc::new(on_select);
    let on_close = Rc::new(on_close);
    let menu_select = Rc::clone(&on_select);
    let labels = tabs
        .items
        .iter()
        .map(|tab| tab.label.clone())
        .collect::<Vec<_>>();
    let count = tabs.items.len();
    let selected = tabs.selected;
    let locale = tabs.locale;
    let theme = cx.theme();

    div()
        .flex()
        .items_center()
        .flex_1()
        .min_w_0()
        .child(
            Tabs::new("document-tabs").flex_1().min_w_0().child(
                div()
                    .id("document-tabs-scroll")
                    .flex()
                    .items_end()
                    .h_full()
                    .pl_2()
                    .gap_1()
                    .overflow_x_scroll()
                    .track_scroll(&tabs.scroll)
                    .children(tabs.items.into_iter().enumerate().map(|(index, tab)| {
                        let activate = Rc::clone(&on_select);
                        let close = Rc::clone(&on_close);
                        let active = index == selected;
                        let element = Tab::new(("document-tab", index))
                            .accessibility_label(tab.label.clone())
                            .native_tooltip(tab.label.clone())
                            .set_position(index + 1, count)
                            .selected(active)
                            .flex_shrink_0()
                            .min_w_0()
                            .max_w(window.rem_size() * 24.0)
                            .h_8()
                            .pl_3()
                            .pr_1()
                            .gap_2()
                            .rounded_t(theme.radius_lg)
                            // The active document joins the work surface below it.
                            .when(active, |tab| {
                                tab.mb(px(-1.0))
                                    .border_t_1()
                                    .border_l_1()
                                    .border_r_1()
                                    .border_color(theme.border)
                                    .bg(theme.background)
                            })
                            .text_sm()
                            .text_color(if active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .when(!active, |tab| tab.hover(|tab| tab.bg(theme.list_hover)))
                            .on_click(move |_, window, cx| activate(&index, window, cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(tab.label.clone()),
                            )
                            .when_some(tab.format, |tab, format| {
                                tab.child(Tag::secondary().small().flex_shrink_0().child(format))
                            })
                            .child(
                                Button::new(("close-tab", index))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .accessibility_label(
                                        Message::new(Key::CloseNamedDocument)
                                            .arg("name", tab.label.to_string())
                                            .display(locale),
                                    )
                                    .native_tooltip(text(locale, Key::CloseDocument))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        close(&index, window, cx);
                                    }),
                            );
                        FocusScroll::new(("document-tab-focus", index), &tabs.scroll, element)
                            .horizontal()
                    })),
            ),
        )
        .child(
            Button::new("document-tabs-menu")
                .ghost()
                .xsmall()
                .icon(IconName::ChevronDown)
                .accessibility_label(text(locale, Key::DocumentNavigation))
                .native_tooltip(text(locale, Key::DocumentNavigation))
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.scrollable(true);
                    for (index, label) in labels.iter().enumerate() {
                        let activate = Rc::clone(&menu_select);
                        menu = menu.item(
                            PopupMenuItem::new(label.clone())
                                .checked(index == selected)
                                .on_click(move |_, window, cx| activate(&index, window, cx)),
                        );
                    }
                    menu
                }),
        )
        .into_any_element()
}
