//! Network/component result presentation; tasks and navigation stay in the viewport.
use gpui_kit::*;
use gpui_kit::{
    base::Selectable,
    component::{
        button::{Button, ButtonVariants},
        scroll::ScrollableElement,
    },
};
use pomelo_core::{
    i18n::{Locale, Message, MessageKey as Key, text},
    search::{SearchEntry, SearchTarget},
    selection::SelectionTarget,
};
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub fn results(
    locale: Locale,
    entries: &[SearchEntry],
    selected: Option<SelectionTarget>,
    commands: impl Fn(SearchTarget) -> ClickHandler,
) -> AnyElement {
    div()
        .id("search-results")
        .max_h_48()
        .min_h_0()
        .flex_shrink_0()
        .overflow_y_scrollbar()
        .children([false, true].into_iter().flat_map(|components| {
            let mut rows = Vec::new();
            for entry in entries.iter().filter(|entry| {
                matches!(
                    entry.target,
                    pomelo_core::search::SearchTarget::Component(_)
                ) == components
            }) {
                if rows.is_empty() {
                    rows.push(
                        div()
                            .px_3()
                            .py_1()
                            .text_sm()
                            .child(text(
                                locale,
                                if components {
                                    Key::SearchComponents
                                } else {
                                    Key::SearchNets
                                },
                            ))
                            .into_any_element(),
                    );
                }
                let target = entry.target;
                let (kind, id) = match target {
                    pomelo_core::search::SearchTarget::Net(id) => ("search-net", id.0),
                    pomelo_core::search::SearchTarget::Component(id) => ("search-component", id.0),
                };
                let label = Message::new(if components {
                    Key::SearchComponentResult
                } else {
                    Key::SearchNetResult
                })
                .arg("name", entry.name.as_str())
                .arg("count", entry.count)
                .display(locale);
                rows.push(
                    div()
                        .px_3()
                        .child(
                            Button::new((kind, id))
                                .selected(selected == Some(target.into()))
                                .ghost()
                                .label(label)
                                .on_click(commands(target)),
                        )
                        .into_any_element(),
                );
            }
            rows
        }))
        .into_any_element()
}
