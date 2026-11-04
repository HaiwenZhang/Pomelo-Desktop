//! Net and component rows; identity and navigation belong to the viewport.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::base::Selectable;
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::{
    i18n::{Locale, Message, MessageKey},
    search::SearchEntry,
};

/// One net or component row and the states it must show.
pub struct EntityRow<'a> {
    pub index: usize,
    pub entry: &'a SearchEntry,
    /// Folded text to emphasize inside the name.
    pub highlight: &'a str,
    pub selected: bool,
    /// Current row of keyboard navigation from the list filter.
    pub keyboard: bool,
    pub color: Option<[f32; 4]>,
}

pub fn row(
    row: EntityRow<'_>,
    locale: Locale,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let EntityRow {
        index,
        entry,
        highlight,
        selected,
        keyboard,
        color,
    } = row;
    // Preserve empty source names in the list, while giving unnamed network
    // commands an accessible identity independent of their result position.
    let accessible_name = match entry.target {
        pomelo_core::search::SearchTarget::Net(id) if entry.name.trim().is_empty() => {
            Message::new(MessageKey::SourceNetName)
                .arg("name", id.0.to_string())
                .display(locale)
        }
        _ => entry.name.clone(),
    };
    let theme = cx.theme();
    div()
        .w_full()
        .px_2()
        .child(
            Button::new(("entity-result", index))
                .ghost()
                .small()
                .w_full()
                .justify_start()
                .selected(selected)
                .when(keyboard && !selected, |button| button.bg(theme.list_hover))
                .accessibility_label(accessible_name.clone())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size_2().flex_shrink_0().rounded_full().map(
                            |dot| match color {
                                Some([r, g, b, a]) => dot.bg(Rgba { r, g, b, a }),
                                None => dot.bg(theme.muted_foreground.opacity(0.4)),
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(theme.mono_font_family.clone())
                                .text_xs()
                                .child(highlighted(&entry.name, highlight, theme.primary)),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(
                                    Message::new(MessageKey::UiNumber)
                                        .arg("value", entry.count)
                                        .display(locale),
                                ),
                        ),
                )
                .native_tooltip(accessible_name)
                .on_click(on_click),
        )
        .into_any_element()
}

/// Emphasize the first case-insensitive occurrence of the query in a source name.
fn highlighted(name: &str, query: &str, color: Hsla) -> StyledText {
    let text = StyledText::new(name.to_owned());
    match match_range(name, query) {
        Some(range) => text.with_highlights([(
            range,
            HighlightStyle {
                color: Some(color),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        )]),
        None => text,
    }
}

/// Byte range of the query in `name`, compared per character so that case
/// folding never splits a UTF-8 sequence.
fn match_range(name: &str, query: &str) -> Option<std::ops::Range<usize>> {
    if query.is_empty() {
        return None;
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    name.char_indices().find_map(|(start, _)| {
        let mut folded = 0;
        for (offset, character) in name[start..].char_indices() {
            for lower in character.to_lowercase() {
                if query.get(folded) != Some(&lower) {
                    return None;
                }
                folded += 1;
            }
            if folded == query.len() {
                return Some(start..start + offset + character.len_utf8());
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::match_range;

    #[test]
    fn match_range_folds_case_and_keeps_utf8_boundaries() {
        assert_eq!(match_range("DDR0_DQ12", "dq1"), Some(5..8));
        assert_eq!(match_range("DDR0_DQ12", ""), None);
        assert_eq!(match_range("DDR0_DQ12", "dq9"), None);
        assert_eq!(match_range("网络_Clk", "clk"), Some(7..10));
        assert_eq!(match_range("网络_Clk", "络"), Some(3..6));
    }
}
