//! Sidebar destinations as one segmented control that never hides a destination.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, Sizable,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder,
    *,
};
use std::rc::Rc;

pub struct SidebarTab {
    pub label: SharedString,
    pub icon: IconName,
    /// Localized count of the destination's open work, such as measurements.
    pub count: Option<SharedString>,
}

pub struct SidebarTabs {
    pub id: &'static str,
    pub tabs: Vec<SidebarTab>,
    pub selected: usize,
    /// Width available to the control, used to fall back to icon-only segments.
    pub width: Pixels,
}

/// Render the switcher row. Narrow panels keep every destination visible as
/// an icon segment whose name remains available to tooltips and assistive tools.
pub fn render(
    tabs: SidebarTabs,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let icons_only = !labels_fit(&tabs.tabs, tabs.width, window, cx);
    let on_select = Rc::new(on_select);
    div()
        .w_full()
        .flex_shrink_0()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            TabBar::new(tabs.id)
                .segmented()
                .small()
                .w_full()
                .selected_index(tabs.selected)
                .children(tabs.tabs.into_iter().map(|tab| {
                    // A Kit Tab with an icon renders only that icon, so a labeled
                    // segment composes icon and label as one centered child.
                    Tab::new()
                        .flex_1()
                        .aria_label(tab.label.clone())
                        .map(|segment| {
                            if icons_only {
                                segment.icon(tab.icon).native_tooltip(tab.label)
                            } else {
                                segment.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap_1p5()
                                        .min_w_0()
                                        .child(Icon::new(tab.icon).small().flex_shrink_0())
                                        .child(div().truncate().child(tab.label))
                                        .when_some(tab.count, |content, count| {
                                            content.child(
                                                div()
                                                    .flex_shrink_0()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(count),
                                            )
                                        }),
                                )
                            }
                        })
                }))
                .on_click(move |index, window, cx| on_select(index, window, cx)),
        )
        .into_any_element()
}

fn labels_fit(tabs: &[SidebarTab], width: Pixels, window: &Window, cx: &App) -> bool {
    let mut font = window.text_style().font();
    font.family = cx.theme().font_family.clone();
    font.weight = FontWeight::MEDIUM;
    let font_size = window.rem_size() * 0.875;
    let rem = window.rem_size();
    // Each small segment holds an icon, a gap, its label and horizontal padding.
    let chrome = rem * 2.25;
    let total = tabs.iter().fold(px(0.0), |total, tab| {
        let run = TextRun {
            len: tab.label.len(),
            font: font.clone(),
            ..Default::default()
        };
        total
            + chrome
            + window
                .text_system()
                .shape_line(tab.label.clone(), font_size, &[run], None)
                .width
    });
    total <= width
}
