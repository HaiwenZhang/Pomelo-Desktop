//! Real file history with bounded, source-derived previews and maintenance menus.
use super::ScrollState;
use crate::panels::focus_scroll::FocusScroll;
use crate::services::prefs::RecentEntry;
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::i18n::{Locale, Message, MessageKey as Key, text};
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SharedClickHandler = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub struct RecentCommands {
    pub open: ClickHandler,
    pub relocate: ClickHandler,
    pub remove: ClickHandler,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Sidebar,
    Continue,
    All,
}

pub fn render(
    locale: Locale,
    entries: &[RecentEntry],
    presentation: Presentation,
    previews: &std::collections::BTreeMap<std::path::PathBuf, std::sync::Arc<RenderImage>>,
    scroll: &ScrollState,
    cx: &App,
    commands: impl Fn(&RecentEntry) -> RecentCommands,
) -> AnyElement {
    let theme = cx.theme();
    let compact = presentation == Presentation::Sidebar;
    let preview = presentation == Presentation::Continue;
    let list_scroll = match presentation {
        Presentation::Sidebar => &scroll.sidebar,
        Presentation::Continue => &scroll.preview,
        Presentation::All => &scroll.all,
    };
    div()
        .flex()
        .flex_col()
        .min_h_0()
        .when(!preview, |list| list.flex_1().h_full())
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .min_w_0()
                .px_2()
                .py_2()
                .when(!compact, |heading| {
                    heading.py_1().border_b_1().border_color(theme.border)
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(!compact, |title| title.text_xl())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(text(
                            locale,
                            if preview {
                                Key::ContinueRecent
                            } else {
                                Key::RecentFiles
                            },
                        )),
                )
                .when(preview, |heading| {
                    heading.child(FocusScroll::new(
                        "view-all-recent-focus",
                        &scroll.content,
                        Button::new("view-all-recent")
                            .ghost()
                            .flex_shrink_0()
                            .text_color(theme.primary)
                            .label(text(locale, Key::ViewAllRecent))
                            .child(Icon::new(IconName::ArrowRight).small())
                            .on_click(|_, window, cx| {
                                window
                                    .dispatch_action(Box::new(crate::actions::ShowRecentFiles), cx)
                            }),
                    ))
                }),
        )
        .child(
            div()
                .id(if compact {
                    "recent-sidebar"
                } else {
                    "recent-files"
                })
                .flex()
                .flex_col()
                .flex_shrink_0()
                .h_80()
                .when(!preview, |list| list.h_auto().flex_1().flex_shrink_1())
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(list_scroll)
                .vertical_scrollbar(list_scroll)
                .id(match presentation {
                    Presentation::Sidebar => "recent-sidebar-scroll",
                    Presentation::Continue => "recent-continue-scroll",
                    Presentation::All => "recent-all-scroll",
                })
                .when(entries.is_empty(), |list| {
                    list.child(
                        div()
                            .p_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(text(locale, Key::RecentFilesEmpty)),
                    )
                })
                .children(
                    entries
                        .iter()
                        .take(if presentation == Presentation::All {
                            crate::prefs::RECENT_LIMIT
                        } else {
                            3
                        })
                        .map(|entry| {
                            let callbacks = commands(entry);
                            let to_relocate: SharedClickHandler = callbacks.relocate.into();
                            let to_remove: SharedClickHandler = callbacks.remove.into();
                            let name = entry
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            let path = entry.path.to_string_lossy().into_owned();
                            let format = text(locale, Key::AllegroFormat);
                            let summary = entry.presentation.as_ref().map_or_else(
                                || format.clone(),
                                |presentation| {
                                    Message::new(Key::RecentBoardSummary)
                                        .arg("format", format.clone())
                                        .arg("layers", presentation.layer_count)
                                        .display(locale)
                                },
                            );
                            let opened = crate::recent::opened_message(
                                entry.opened_unix_seconds,
                                &chrono::Local::now(),
                            )
                            .display(locale);
                            let focus_id = SharedString::from(format!(
                                "recent-row-focus:{}:{compact}:{preview}",
                                entry.path.display()
                            ));
                            let row = div()
                                .flex()
                                .w_full()
                                .items_center()
                                .min_w_0()
                                .gap_1()
                                .py_3()
                                .when(!compact, |row| row.border_b_1().border_color(theme.border))
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "open-recent:{}:{compact}",
                                        entry.path.display()
                                    )))
                                    .ghost()
                                    .flex_1()
                                    .min_w_0()
                                    .h_20()
                                    .when(compact, |button| button.h_16())
                                    .accessibility_label(name.clone())
                                    .native_tooltip(path)
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .min_w_0()
                                            .gap_3()
                                            .when(!compact, |row| {
                                                row.child(
                                                    div()
                                                        .w_24()
                                                        .h_16()
                                                        .flex_shrink_0()
                                                        .rounded_md()
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .overflow_hidden()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .map(|tile| {
                                                            if let Some(preview) =
                                                                previews.get(&entry.path)
                                                            {
                                                                tile.child(
                                                                    img(preview.clone())
                                                                        .size_full()
                                                                        .object_fit(
                                                                            ObjectFit::Contain,
                                                                        ),
                                                                )
                                                            } else {
                                                                tile.child(
                                                                    Icon::new(IconName::Cpu)
                                                                        .size_8()
                                                                        .text_color(
                                                                            theme.muted_foreground,
                                                                        ),
                                                                )
                                                            }
                                                        }),
                                                )
                                            })
                                            .when(compact, |row| {
                                                row.child(
                                                    Icon::new(IconName::File)
                                                        .size_5()
                                                        .text_color(theme.primary),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_base()
                                                            .whitespace_normal()
                                                            .line_clamp(2)
                                                            .text_ellipsis()
                                                            .child(name),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(theme.muted_foreground)
                                                            .truncate()
                                                            .child(summary),
                                                    ),
                                            ),
                                    )
                                    .on_click(callbacks.open),
                                )
                                .when(!compact, |row| {
                                    row.child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .flex_shrink_0()
                                            .child(opened),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "recent-more:{}",
                                            entry.path.display()
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Ellipsis)
                                        .native_tooltip(text(locale, Key::FileInformation))
                                        .accessibility_label(text(locale, Key::FileInformation))
                                        .dropdown_menu(
                                            move |menu, _, _| {
                                                let to_relocate = to_relocate.clone();
                                                let to_remove = to_remove.clone();
                                                menu.item(
                                                    PopupMenuItem::new(text(
                                                        locale,
                                                        Key::RelocateRecent,
                                                    ))
                                                    .on_click(move |event, window, cx| {
                                                        to_relocate(event, window, cx)
                                                    }),
                                                )
                                                .item(
                                                    PopupMenuItem::new(text(
                                                        locale,
                                                        Key::RemoveRecent,
                                                    ))
                                                    .on_click(move |event, window, cx| {
                                                        to_remove(event, window, cx)
                                                    }),
                                                )
                                            },
                                        ),
                                    )
                                });
                            let row = FocusScroll::new(focus_id.clone(), list_scroll, row).w_full();
                            if preview {
                                // A preview row can be clipped by either scroll region.
                                FocusScroll::new(
                                    (ElementId::from(focus_id), "content"),
                                    &scroll.content,
                                    row,
                                )
                                .w_full()
                                .into_any_element()
                            } else {
                                row.into_any_element()
                            }
                        }),
                ),
        )
        .into_any_element()
}
