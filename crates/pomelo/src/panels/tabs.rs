//! Sidebar destinations as a row of text tabs. The selected tab is told apart
//! by weight and ink rather than a filled segment, and tabs that do not fit
//! wrap as whole items instead of shrinking into icons or truncating, so
//! longer translations keep their full names.
use gpui_kit::{
    base::{Tab, Tabs},
    component::ActiveTheme,
    prelude::FluentBuilder,
    *,
};
use std::rc::Rc;

pub struct SidebarTab {
    pub label: SharedString,
    /// Localized count of the destination's open work, such as measurements.
    pub count: Option<SharedString>,
}

pub struct SidebarTabs {
    pub id: &'static str,
    pub tabs: Vec<SidebarTab>,
    /// `None` while the panel shows a page that is not one of its tabs.
    pub selected: Option<usize>,
}

pub fn render(
    tabs: SidebarTabs,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let on_select = Rc::new(on_select);
    let theme = cx.theme();
    let size = tabs.tabs.len();
    let id = tabs.id;
    div()
        .w_full()
        .flex_shrink_0()
        .px_3()
        .pt_2()
        .pb_1()
        .child(
            Tabs::new(id).w_full().child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    .gap_x_4()
                    .gap_y_1()
                    .children(tabs.tabs.into_iter().enumerate().map(|(index, tab)| {
                        let selected = tabs.selected == Some(index);
                        let on_select = Rc::clone(&on_select);
                        Tab::new((id, index))
                            .accessibility_label(tab.label.clone())
                            .set_position(index + 1, size)
                            .selected(selected)
                            .flex()
                            .items_baseline()
                            .gap_1()
                            .py_1()
                            .whitespace_nowrap()
                            .text_base()
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(if selected {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .when(!selected, |tab| {
                                tab.hover(|style| style.text_color(theme.foreground))
                            })
                            .on_click(move |_, window, cx| on_select(&index, window, cx))
                            .child(tab.label)
                            .when_some(tab.count, |tab, count| {
                                tab.child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::NORMAL)
                                        .font_family(theme.mono_font_family.clone())
                                        .text_color(theme.muted_foreground)
                                        .child(count),
                                )
                            })
                    })),
            ),
        )
        .into_any_element()
}
