//! Recent-file presentation. Commands and persistence are supplied by owners.
use crate::services::prefs::RecentEntry;
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
};
use gpui_kit::*;
use pomelo_core::i18n::{Locale, MessageKey as Key, text};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub struct RecentCommands {
    pub open: ClickHandler,
    pub relocate: ClickHandler,
    pub remove: ClickHandler,
}

pub fn render(
    locale: Locale,
    entries: &[RecentEntry],
    cx: &App,
    commands: impl Fn(&RecentEntry) -> RecentCommands,
) -> AnyElement {
    let list = div()
        .flex()
        .flex_col()
        .min_h_0()
        .gap_2()
        .child(div().text_lg().child(text(locale, Key::RecentFiles)));
    if entries.is_empty() {
        return list
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::RecentFilesEmpty)),
            )
            .into_any_element();
    }
    let mut rows = div()
        .id("recent-files")
        .flex()
        .flex_col()
        .gap_2()
        .max_h_64()
        .overflow_y_scrollbar();
    for entry in entries {
        let callbacks = commands(entry);
        let path_label = entry.path.to_string_lossy().into_owned();
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    Button::new(SharedString::from(format!(
                        "open-recent:{}",
                        entry.path.display()
                    )))
                    .ghost()
                    .flex_1()
                    .min_w_0()
                    .label(path_label.clone())
                    .tooltip(path_label)
                    .on_click(callbacks.open),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "relocate-recent:{}",
                        entry.path.display()
                    )))
                    .ghost()
                    .label(text(locale, Key::RelocateRecent))
                    .on_click(callbacks.relocate),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "remove-recent:{}",
                        entry.path.display()
                    )))
                    .ghost()
                    .label(text(locale, Key::RemoveRecent))
                    .on_click(callbacks.remove),
                ),
        );
    }
    list.child(rows).into_any_element()
}
