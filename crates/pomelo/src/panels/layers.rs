//! Grouped layer rows: visibility leads each row, settings disclose beneath it.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::{
    display::{LayerPrimitive, LayerPrimitives},
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
type PrimitiveHandler = Box<dyn Fn(&(LayerPrimitive, bool), &mut Window, &mut App)>;
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
    pub primitives: LayerPrimitives,
    /// Material color controls, one per primitive in [`PRIMITIVES`] order.
    pub colors: Vec<AnyElement>,
}

pub struct LayerCommands {
    pub activate: ClickHandler,
    pub expand: ClickHandler,
    pub primitive: PrimitiveHandler,
    /// Receives the requested visibility; the click also carries modifiers
    /// so the owner can isolate the layer.
    pub visibility: LayerVisibilityHandler,
    pub to_bottom: ClickHandler,
    pub to_top: ClickHandler,
}

const PRIMITIVES: [(LayerPrimitive, MessageKey); 3] = [
    (LayerPrimitive::Traces, MessageKey::LayerTraces),
    (LayerPrimitive::Pads, MessageKey::LayerPads),
    (LayerPrimitive::Vias, MessageKey::LayerVias),
];

pub fn row(locale: Locale, state: LayerRow, commands: LayerCommands, cx: &App) -> AnyElement {
    let visible = state.visible;
    let primitive = std::rc::Rc::new(commands.primitive);
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
                        .child(
                            Icon::new(if state.expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .xsmall()
                            .text_color(theme.muted_foreground),
                        ),
                )
                .on_click(commands.expand),
        )
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
                    .children(PRIMITIVES.into_iter().map(|(kind, label)| {
                        let command = primitive.clone();
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .min_h_7()
                            .child(
                                Checkbox::new((
                                    "layer-primitive",
                                    state.id.0 as u64 * 3 + kind as u64,
                                ))
                                .small()
                                .label(text(locale, label))
                                .checked(state.primitives.visible(kind))
                                .on_change(
                                    move |enabled, window, cx| {
                                        command(&(kind, *enabled), window, cx)
                                    },
                                ),
                            )
                            .children(colors.next())
                    })),
            )
        })
        .into_any_element()
}
