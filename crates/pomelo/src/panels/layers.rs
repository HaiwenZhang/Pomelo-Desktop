//! Compact layer rows, with source colors and a native ordering menu.
use crate::tooltips::ButtonTooltipExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
    slider::{Slider, SliderState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::{
    display::{LayerPrimitive, LayerPrimitives},
    i18n::{Locale, Message, MessageKey, text},
    model::{LayerFunction, LayerId},
};
pub struct LayerRow {
    pub id: LayerId,
    pub label: String,
    pub visible: bool,
    pub at_bottom: bool,
    pub at_top: bool,
    pub color: Option<[f32; 4]>,
    pub function: Option<LayerFunction>,
    pub expanded: bool,
    pub primitives: LayerPrimitives,
}
type SharedClickHandler = std::rc::Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type VisibilityHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
type PrimitiveHandler = Box<dyn Fn(&(LayerPrimitive, bool), &mut Window, &mut App)>;

/// Present the shared opacity slider with a separate, aligned current value.
pub fn settings(
    locale: Locale,
    opacity: f32,
    slider: &Entity<SliderState>,
    cx: &App,
) -> AnyElement {
    div()
        .p_4()
        .flex_shrink_0()
        .border_t_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(text(locale, MessageKey::LayerSettings)),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text(locale, MessageKey::CopperOpacityLabel)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div().flex_1().min_w_0().child(
                        Slider::new(slider)
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary),
                    ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_sm()
                        .min_w_8()
                        .text_right()
                        .child(
                            Message::new(MessageKey::UiPercent)
                                .arg("value", (opacity * 100.0).round() as u32)
                                .display(locale),
                        ),
                ),
        )
        .into_any_element()
}

pub struct LayerCommands {
    pub expand: ClickHandler,
    pub primitive: PrimitiveHandler,
    pub visibility: VisibilityHandler,
    pub to_bottom: ClickHandler,
    pub to_top: ClickHandler,
}
pub fn row(locale: Locale, state: LayerRow, commands: LayerCommands, cx: &App) -> AnyElement {
    let visible = state.visible;
    let primitive = std::rc::Rc::new(commands.primitive);
    let to_bottom: SharedClickHandler = commands.to_bottom.into();
    let to_top: SharedClickHandler = commands.to_top.into();
    let color = state
        .color
        .map(|[r, g, b, a]| Rgba { r, g, b, a })
        .map(Hsla::from)
        .unwrap_or(cx.theme().primary);
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .child(div().size_4().rounded_sm().flex_shrink_0().bg(color))
        .child(
            Button::new(("layer-expand", state.id.0))
                .ghost()
                .small()
                .px_1()
                .flex_1()
                .min_w_0()
                .justify_start()
                .accessibility_label(state.label.clone())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(48.0))
                                .truncate()
                                .child(state.label.clone()),
                        )
                        .when_some(state.function, |row, function| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .min_w_0()
                                    .truncate()
                                    .child(text(
                                        locale,
                                        match function {
                                            LayerFunction::Conductor => MessageKey::LayerConductor,
                                            LayerFunction::Plane => MessageKey::LayerPlane,
                                            LayerFunction::Dielectric => {
                                                MessageKey::LayerDielectric
                                            }
                                            LayerFunction::Unknown => {
                                                MessageKey::LayerFunctionUnknown
                                            }
                                        },
                                    )),
                            )
                        }),
                )
                .native_tooltip(state.label)
                .when(!visible, |button| {
                    button.text_color(cx.theme().muted_foreground)
                })
                .on_click(commands.expand),
        )
        .child(
            Button::new(("layer-visible", state.id.0))
                .ghost()
                .xsmall()
                .icon(if visible {
                    IconName::Eye
                } else {
                    IconName::EyeOff
                })
                .accessibility_label(text(
                    locale,
                    if visible {
                        MessageKey::HideLayer
                    } else {
                        MessageKey::ShowLayer
                    },
                ))
                .on_click(move |_, window, cx| (commands.visibility)(&!visible, window, cx)),
        )
        .child(
            Button::new(("layer-order", state.id.0))
                .ghost()
                .xsmall()
                .icon(IconName::Ellipsis)
                .accessibility_label(text(locale, MessageKey::LayerSettings))
                .native_tooltip(text(locale, MessageKey::LayerSettings))
                .dropdown_menu(move |menu, _, _| {
                    let to_bottom = to_bottom.clone();
                    let to_top = to_top.clone();
                    menu.item(
                        PopupMenuItem::new(text(locale, MessageKey::LayerToBottom))
                            .disabled(state.at_bottom)
                            .on_click(move |event, window, cx| to_bottom(event, window, cx)),
                    )
                    .item(
                        PopupMenuItem::new(text(locale, MessageKey::LayerToTop))
                            .disabled(state.at_top)
                            .on_click(move |event, window, cx| to_top(event, window, cx)),
                    )
                }),
        );
    let content = div()
        .w_full()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(cx.theme().border)
        .when(state.expanded, |row| {
            row.rounded_sm()
                .border_1()
                .border_color(cx.theme().primary.opacity(0.3))
                .bg(cx.theme().accent)
        })
        .child(header)
        .when(state.expanded, |row| {
            row.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .pl_8()
                    .pr_2()
                    .pb_2()
                    .children(
                        [
                            (LayerPrimitive::Traces, MessageKey::LayerTraces),
                            (LayerPrimitive::Vias, MessageKey::LayerVias),
                            (LayerPrimitive::Pads, MessageKey::LayerPads),
                        ]
                        .into_iter()
                        .map(|(kind, label)| {
                            let command = primitive.clone();
                            Checkbox::new(("layer-primitive", state.id.0 as u64 * 3 + kind as u64))
                                .xsmall()
                                .flex_grow(1.0)
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .label(text(locale, label))
                                .checked(state.primitives.visible(kind))
                                .on_change(move |enabled, window, cx| {
                                    command(&(kind, *enabled), window, cx)
                                })
                        }),
                    ),
            )
        });
    div().w_full().px_3().child(content).into_any_element()
}
