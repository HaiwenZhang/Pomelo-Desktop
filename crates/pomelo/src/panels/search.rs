//! Search row presentation; identity and navigation belong to the viewport.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::base::Selectable;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::search::SearchEntry;
pub fn row(
    index: usize,
    entry: &SearchEntry,
    selected: bool,
    color: Option<[f32; 4]>,
    locale: pomelo_core::i18n::Locale,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    // Preserve empty source names in the list, while giving unnamed network
    // commands an accessible identity independent of their result position.
    let accessible_name = match entry.target {
        pomelo_core::search::SearchTarget::Net(id) if entry.name.trim().is_empty() => {
            pomelo_core::i18n::Message::new(pomelo_core::i18n::MessageKey::SourceNetName)
                .arg("name", id.0.to_string())
                .display(locale)
        }
        _ => entry.name.clone(),
    };
    div()
        .w_full()
        .px_2()
        .py_1()
        .child(
            Button::new(("entity-result", index))
                .ghost()
                .small()
                .w_full()
                .justify_start()
                .selected(selected)
                .accessibility_label(accessible_name.clone())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when_some(color, |row, [r, g, b, a]| {
                            row.child(div().size_2().flex_shrink_0().rounded_full().bg(Rgba {
                                r,
                                g,
                                b,
                                a,
                            }))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(entry.name.clone()),
                        ),
                )
                .native_tooltip(accessible_name)
                .on_click(on_click),
        )
        .into_any_element()
}
