//! Grouped layer rows: visibility leads each row, settings disclose beneath it.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::{
    display::{CellMode, LayerPrimitive},
    i18n::{Locale, Message, MessageKey, text},
    model::LayerId,
};

/// Source layer families, listed in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayerGroup {
    Copper,
    Package,
    Drawings,
}

impl LayerGroup {
    pub const ALL: [Self; 3] = [Self::Copper, Self::Package, Self::Drawings];

    fn label(self) -> MessageKey {
        match self {
            Self::Copper => MessageKey::LayerGroupCopper,
            Self::Package => MessageKey::LayerGroupPackage,
            Self::Drawings => MessageKey::LayerGroupDrawings,
        }
    }
}

pub struct GroupHeader {
    pub group: LayerGroup,
    pub count: usize,
    pub visible: usize,
    pub collapsed: bool,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SharedClickHandler = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type VisibilityHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
/// Receives the clicked matrix column and whether Alt was held (fill/outline).
pub type CellHandler = Box<dyn Fn(&(LayerPrimitive, bool), &mut Window, &mut App)>;
type OpacityHandler = Box<dyn Fn(&f32, &mut Window, &mut App)>;
/// Receives the click, for its modifiers, and the requested visibility.
type LayerVisibilityHandler = Box<dyn Fn(&ClickEvent, bool, &mut Window, &mut App)>;

pub struct GroupCommands {
    pub toggle: ClickHandler,
    /// Receives the visibility the whole group should take.
    pub visibility: VisibilityHandler,
}

/// A disclosure row for one layer family with a family-wide visibility toggle.
pub fn group_header(
    locale: Locale,
    header: GroupHeader,
    commands: GroupCommands,
    cx: &App,
) -> AnyElement {
    let label = text(locale, header.group.label());
    let all_hidden = header.visible == 0;
    div()
        .w_full()
        .px_2()
        .pt_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    Button::new(("layer-group", header.group as u32))
                        .ghost()
                        .small()
                        .flex_1()
                        .min_w_0()
                        .justify_start()
                        .px_1()
                        .accessibility_label(label.clone())
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .min_w_0()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    Icon::new(if header.collapsed {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .xsmall(),
                                )
                                .child(div().truncate().child(label.clone()))
                                .child(
                                    div().font_weight(FontWeight::NORMAL).child(
                                        Message::new(MessageKey::UiNumber)
                                            .arg("value", header.count)
                                            .display(locale),
                                    ),
                                ),
                        )
                        .on_click(commands.toggle),
                )
                .child(
                    Button::new(("layer-group-visible", header.group as u32))
                        .ghost()
                        .xsmall()
                        .icon(if all_hidden {
                            IconName::EyeOff
                        } else {
                            IconName::Eye
                        })
                        .text_color(cx.theme().muted_foreground)
                        .accessibility_label(
                            Message::new(if all_hidden {
                                MessageKey::ShowLayerGroup
                            } else {
                                MessageKey::HideLayerGroup
                            })
                            .arg("group", label.clone())
                            .display(locale),
                        )
                        .on_click(move |_, window, cx| {
                            (commands.visibility)(&all_hidden, window, cx)
                        }),
                ),
        )
        .into_any_element()
}

pub struct LayerRow {
    pub id: LayerId,
    pub label: String,
    pub visible: bool,
    pub at_bottom: bool,
    pub at_top: bool,
    pub color: Option<[f32; 4]>,
    pub expanded: bool,
    pub active: bool,
    /// Matrix cells in [`LayerPrimitive::MATRIX`] order; conductor layers only.
    pub cells: Option<[CellMode; 5]>,
    /// Layer opacity for conductor layers.
    pub opacity: Option<f32>,
    /// Material color controls, one per entry in [`COLOR_ROWS`] order.
    pub colors: Vec<AnyElement>,
}

pub struct LayerCommands {
    pub activate: ClickHandler,
    pub expand: ClickHandler,
    /// Show this layer only, from a double click on its name.
    pub solo: ClickHandler,
    pub cell: CellHandler,
    pub opacity: OpacityHandler,
    /// Receives the requested visibility; the click also carries modifiers
    /// so the owner can isolate the layer.
    pub visibility: LayerVisibilityHandler,
    pub to_bottom: ClickHandler,
    pub to_top: ClickHandler,
}

/// Color rows disclosed under a layer; traces share their color with copper.
const COLOR_ROWS: [MessageKey; 3] = [
    MessageKey::LayerTraces,
    MessageKey::LayerPads,
    MessageKey::LayerVias,
];

/// Column name and one-character heading for each matrix column.
pub fn matrix_labels(kind: LayerPrimitive) -> (MessageKey, MessageKey) {
    match kind {
        LayerPrimitive::Copper => (MessageKey::MatrixCopper, MessageKey::MatrixCopperShort),
        LayerPrimitive::Traces => (MessageKey::MatrixTraces, MessageKey::MatrixTracesShort),
        LayerPrimitive::Pads => (MessageKey::MatrixPads, MessageKey::MatrixPadsShort),
        LayerPrimitive::Vias => (MessageKey::MatrixVias, MessageKey::MatrixViasShort),
        LayerPrimitive::Components => (
            MessageKey::MatrixComponents,
            MessageKey::MatrixComponentsShort,
        ),
    }
}

/// Opacity choices offered from a layer's menu.
pub const OPACITY_STEPS: [f32; 5] = [1.0, 0.7, 0.5, 0.3, 0.1];

/// One matrix cell: filled square, outlined square or an empty slot.
fn cell_button(
    id: impl Into<ElementId>,
    kind: LayerPrimitive,
    mode: CellMode,
    locale: Locale,
    handler: std::rc::Rc<CellHandler>,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    let (name, _) = matrix_labels(kind);
    let state = match mode {
        CellMode::Fill => MessageKey::CellFilled,
        CellMode::Outline => MessageKey::CellOutline,
        CellMode::Hidden => MessageKey::CellHidden,
    };
    let label = Message::new(MessageKey::MatrixCellHint)
        .arg("name", text(locale, name))
        .arg("state", text(locale, state))
        .display(locale);
    Button::new(id)
        .ghost()
        .xsmall()
        .size_4()
        .flex_shrink_0()
        .accessibility_label(label.clone())
        .native_tooltip(label)
        .child(
            div()
                .size_2p5()
                .rounded_sm()
                .border_1()
                .border_color(match mode {
                    CellMode::Hidden => theme.border,
                    _ => theme.primary,
                })
                .when(mode == CellMode::Fill, |square| square.bg(theme.primary)),
        )
        .on_click(move |event, window, cx| handler(&(kind, event.modifiers().alt), window, cx))
}

/// Column headings above the conductor layers; a click applies to the whole column.
pub fn matrix_header(
    locale: Locale,
    columns: [CellMode; 5],
    handler: CellHandler,
    cx: &App,
) -> AnyElement {
    let handler = std::rc::Rc::new(handler);
    let theme = cx.theme();
    div()
        .w_full()
        .px_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .pl_1()
                .pr_1()
                .h_6()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .pl_7()
                        .child(text(locale, MessageKey::MatrixHeading)),
                )
                .child(
                    div().flex().flex_shrink_0().children(
                        LayerPrimitive::MATRIX
                            .into_iter()
                            .zip(columns)
                            .map(|(kind, mode)| {
                                let (name, short) = matrix_labels(kind);
                                let handler = handler.clone();
                                let label = Message::new(MessageKey::MatrixColumnHint)
                                    .arg("name", text(locale, name))
                                    .display(locale);
                                Button::new(("matrix-column", kind as u32))
                                    .ghost()
                                    .xsmall()
                                    .size_4()
                                    .flex_shrink_0()
                                    .accessibility_label(label.clone())
                                    .native_tooltip(label)
                                    .child(
                                        div()
                                            .text_xs()
                                            .when(mode == CellMode::Hidden, |label| {
                                                label
                                                    .text_color(theme.muted_foreground.opacity(0.6))
                                            })
                                            .child(text(locale, short)),
                                    )
                                    .on_click(move |event, window, cx| {
                                        handler(&(kind, event.modifiers().alt), window, cx)
                                    })
                            }),
                    ),
                )
                // Lines up with the layer menu button of each row.
                .child(div().w_6().flex_shrink_0()),
        )
        .into_any_element()
}

pub fn row(locale: Locale, state: LayerRow, commands: LayerCommands, cx: &App) -> AnyElement {
    let visible = state.visible;
    let cell = std::rc::Rc::new(commands.cell);
    let opacity_command = std::rc::Rc::new(commands.opacity);
    let solo = commands.solo;
    let expand = commands.expand;
    let to_bottom: SharedClickHandler = commands.to_bottom.into();
    let to_top: SharedClickHandler = commands.to_top.into();
    let activate: SharedClickHandler = commands.activate.into();
    let theme = cx.theme();
    let color = state
        .color
        .map(|[r, g, b, a]| Rgba { r, g, b, a })
        .map(Hsla::from)
        .unwrap_or(theme.muted_foreground);
    let muted = !visible;
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .pl_1()
        .pr_1()
        .rounded_md()
        .when(state.expanded, |row| row.bg(theme.list_active))
        .child(
            Button::new(("layer-visible", state.id.0))
                .ghost()
                .xsmall()
                .flex_shrink_0()
                .icon(if visible {
                    IconName::Eye
                } else {
                    IconName::EyeOff
                })
                .text_color(if visible {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .accessibility_label(
                    Message::new(if visible {
                        MessageKey::HideLayer
                    } else {
                        MessageKey::ShowLayer
                    })
                    .display(locale),
                )
                .on_click(move |event, window, cx| {
                    (commands.visibility)(event, !visible, window, cx)
                }),
        )
        .child(
            div()
                .size_3()
                .rounded_sm()
                .flex_shrink_0()
                .bg(color)
                .when(muted, |swatch| swatch.opacity(0.4)),
        )
        .child(
            Button::new(("layer-expand", state.id.0))
                .ghost()
                .small()
                .flex_1()
                .min_w_0()
                .justify_start()
                .px_1()
                .accessibility_label(state.label.clone())
                .native_tooltip(state.label.clone())
                .selected(state.expanded)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(muted, |label| label.text_color(theme.muted_foreground))
                                .child(state.label),
                        )
                        // Matrix rows need the width for the name; a click still discloses colors.
                        .when(state.cells.is_none(), |label| {
                            label.child(
                                Icon::new(if state.expanded {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                })
                                .xsmall()
                                .text_color(theme.muted_foreground),
                            )
                        }),
                )
                .on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        solo(event, window, cx)
                    } else {
                        expand(event, window, cx)
                    }
                }),
        )
        .when_some(state.opacity.filter(|alpha| *alpha < 1.0), |row, alpha| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        Message::new(MessageKey::UiPercent)
                            .arg("value", (alpha * 100.0).round() as u32)
                            .display(locale),
                    ),
            )
        })
        .when_some(state.cells, |row, cells| {
            row.child(
                div().flex().flex_shrink_0().children(
                    LayerPrimitive::MATRIX
                        .into_iter()
                        .zip(cells)
                        .map(|(kind, mode)| {
                            cell_button(
                                ("layer-cell", state.id.0 as u64 * 8 + kind as u64),
                                kind,
                                mode,
                                locale,
                                cell.clone(),
                                cx,
                            )
                        }),
                ),
            )
        })
        .child(
            Button::new(("layer-order", state.id.0))
                .ghost()
                .xsmall()
                .flex_shrink_0()
                .icon(IconName::Ellipsis)
                .text_color(theme.muted_foreground)
                .accessibility_label(text(locale, MessageKey::LayerSettings))
                .native_tooltip(text(locale, MessageKey::LayerSettings))
                .dropdown_menu(move |menu, _, _| {
                    let to_bottom = to_bottom.clone();
                    let to_top = to_top.clone();
                    let activate = activate.clone();
                    menu.item(
                        PopupMenuItem::new(text(locale, MessageKey::ActiveLayer))
                            .checked(state.active)
                            .on_click(move |event, window, cx| activate(event, window, cx)),
                    )
                    .item(
                        PopupMenuItem::new(text(locale, MessageKey::LayerToTop))
                            .disabled(state.at_top)
                            .on_click(move |event, window, cx| to_top(event, window, cx)),
                    )
                    .item(
                        PopupMenuItem::new(text(locale, MessageKey::LayerToBottom))
                            .disabled(state.at_bottom)
                            .on_click(move |event, window, cx| to_bottom(event, window, cx)),
                    )
                    .when_some(state.opacity, |mut menu, current| {
                        menu = menu
                            .separator()
                            .label(text(locale, MessageKey::LayerOpacity));
                        for step in OPACITY_STEPS {
                            let command = opacity_command.clone();
                            menu = menu.item(
                                PopupMenuItem::new(
                                    Message::new(MessageKey::UiPercent)
                                        .arg("value", (step * 100.0).round() as u32)
                                        .display(locale),
                                )
                                .checked((current - step).abs() < 0.005)
                                .on_click(move |_, window, cx| command(&step, window, cx)),
                            );
                        }
                        menu
                    })
                }),
        );
    let mut colors = state.colors.into_iter();
    div()
        .w_full()
        .px_2()
        .child(header)
        .when(state.expanded, |row| {
            row.child(
                div()
                    // Indent to the swatch lane so settings read as this layer's own.
                    .pl_8()
                    .pr_1()
                    .pt_1()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(COLOR_ROWS.into_iter().map(|label| {
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .min_h_7()
                            .child(div().text_sm().child(text(locale, label)))
                            .children(colors.next())
                    })),
            )
        })
        .into_any_element()
}
