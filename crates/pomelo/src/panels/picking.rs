//! Picking filter presentation; cancellation and selection state belong to the viewport.
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
    cx: &App,
    commands: impl Fn(PickCategory) -> ChangeHandler,
) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .child(text(locale, Key::PickCategories))
        .children(
            [
                (
                    pomelo_core::picking::PickCategory::Segment,
                    Key::SourceSegment,
                ),
                (pomelo_core::picking::PickCategory::Pin, Key::SourcePin),
                (pomelo_core::picking::PickCategory::Via, Key::SourceVia),
                (pomelo_core::picking::PickCategory::Zone, Key::SourceZone),
            ]
            .into_iter()
            .map(|(category, label)| {
                Checkbox::new(("pick-category", category as u32))
                    .label(text(locale, label))
                    .checked(filter.contains(category))
                    .on_change(commands(category))
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
