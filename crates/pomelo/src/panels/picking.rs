//! Picking filter presentation; cancellation and selection state belong to the viewport.
use super::focus_scroll::FocusScroll;
use gpui_kit::*;
use gpui_kit::{
    component::{ActiveTheme, checkbox::Checkbox},
    prelude::FluentBuilder,
};
use pomelo_core::{
    i18n::{Locale, MessageKey as Key, text},
    interaction::SelectionMode,
    picking::{PickCategory, PickFilter},
};
type ChangeHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
pub fn controls(
    locale: Locale,
    filter: PickFilter,
    mode: SelectionMode,
    scroll: &ScrollHandle,
    cx: &App,
    commands: impl Fn(PickCategory) -> ChangeHandler,
) -> AnyElement {
    div()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_wrap()
        .gap_1()
        .child(
            div()
                .w_full()
                .font_weight(FontWeight::SEMIBOLD)
                .child(text(locale, Key::PickCategories)),
        )
        .children(
            [
                (
                    pomelo_core::picking::PickCategory::Segment,
                    Key::LayerTraces,
                ),
                (pomelo_core::picking::PickCategory::Via, Key::LayerVias),
                (pomelo_core::picking::PickCategory::Pin, Key::LayerPads),
                (pomelo_core::picking::PickCategory::Zone, Key::PickCopper),
                (
                    pomelo_core::picking::PickCategory::Drawing,
                    Key::PickDrawings,
                ),
            ]
            .into_iter()
            .map(|(category, label)| {
                FocusScroll::new(
                    ("pick-category", category as u32),
                    scroll,
                    Checkbox::new(("pick-category", category as u32))
                        .whitespace_nowrap()
                        .label(text(locale, label))
                        .checked(filter.contains(category))
                        .on_change(commands(category)),
                )
                .flex_grow(1.0)
                .flex_shrink_0()
            }),
        )
        .when_some(
            if filter == pomelo_core::picking::PickFilter::none() {
                Some(Key::PickNoneEnabled)
            } else if mode == pomelo_core::interaction::SelectionMode::Track
                && !filter.contains(pomelo_core::picking::PickCategory::Segment)
            {
                Some(Key::PickTrackDisabled)
            } else if mode == pomelo_core::interaction::SelectionMode::Component
                && !filter.contains(pomelo_core::picking::PickCategory::Pin)
                && !filter.contains(pomelo_core::picking::PickCategory::Segment)
                && !filter.contains(pomelo_core::picking::PickCategory::Via)
            {
                Some(Key::PickComponentDisabled)
            } else {
                None
            },
            |section, key| {
                section.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, key)),
                )
            },
        )
        .into_any_element()
}
