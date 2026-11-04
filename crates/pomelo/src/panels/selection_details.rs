//! Selected-object details and member navigation, independent of viewport ownership.
use crate::{
    panels::{
        diagnostics::NavigationFocus,
        focus_scroll::{FocusScroll, InspectorFocus},
        inspector::{property_row, selection_identity},
    },
    tooltips::ButtonTooltipExt,
};
use gpui_kit::{
    assets::IconName,
    base::{Disableable, Selectable},
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonVariants},
        scroll::ScrollableElement,
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::{
    display::BoardDisplay,
    i18n::{Locale, Message, MessageKey as Key, text},
    model::{BoardScene, NetId, ObjectId},
    picking_index::SelectionAnchor,
    selection::{
        MemberLayers, SelectedObject, SelectionMembers, SelectionSummary, SelectionTarget,
    },
};
use std::rc::Rc;

/// Members are paged by the selection model in blocks of this size.
const MEMBERS_PAGE: usize = 256;
/// Layer and component names listed before the remainder is summarized.
const NAMED_FACTS: usize = 4;

pub(crate) struct SelectionDetails<'a> {
    pub selected_target: Option<SelectionTarget>,
    pub candidate_position: Option<(usize, usize)>,
    pub selected_source_labels: &'a [Message],
    pub selected_anchor: Option<SelectionAnchor>,
    pub selected_related_net: Option<NetId>,
    pub selected_related_component: Option<ObjectId>,
    pub selected_summary: Option<&'a SelectionSummary>,
    pub selected_members: &'a Option<SelectionMembers>,
    pub locate_pending: bool,
    pub members_offset: usize,
    pub members_page_revision: u64,
    pub members_scroll: &'a ScrollHandle,
    pub members_navigation: &'a NavigationFocus,
    pub inspector_focus: &'a InspectorFocus,
    pub display: &'a BoardDisplay,
    pub scene: &'a BoardScene,
}
type LocateCommand = Rc<dyn Fn(&SelectionTarget, &mut Window, &mut App)>;
type ClickCommand = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub(crate) struct SelectionCommands {
    pub locate_target: LocateCommand,
    pub clear: ClickCommand,
    pub previous_page: ClickCommand,
    pub next_page: ClickCommand,
}
fn locate_on_click(
    command: &LocateCommand,
    target: SelectionTarget,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<> {
    let on_locate = Rc::clone(command);
    move |_, window, cx| on_locate(&target, window, cx)
}

pub(crate) fn render(
    state: SelectionDetails<'_>,
    commands: SelectionCommands,
    locale: Locale,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let Some(target) = state.selected_target else {
        return empty_state(locale, cx);
    };
    let (kind, id) = selection_identity(target);
    let title = state
        .selected_source_labels
        .iter()
        .find_map(|message| message.args.get("name").map(ToString::to_string))
        .unwrap_or_else(|| {
            Message::new(Key::SelectionIdentity)
                .arg("kind", text(locale, kind))
                .arg("id", id)
                .display(locale)
        });
    let subtitle = match state.selected_members {
        Some(members) => Message::new(Key::SelectionObjects)
            .arg("kind", text(locale, kind))
            .arg("count", members.total)
            .display(locale),
        None => text(locale, kind),
    };
    let SelectionCommands {
        locate_target,
        clear,
        previous_page,
        next_page,
    } = commands;
    div()
        .id("inspector-content")
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(identity(
            &state,
            target,
            title,
            subtitle,
            &locate_target,
            clear,
            locale,
            cx,
        ))
        .when_some(state.selected_summary, |body, summary| {
            body.child(stats(summary, locale, cx))
        })
        .child(facts(&state, &locate_target, id, locale, cx))
        .when_some(state.selected_members.as_ref(), |body, members| {
            body.child(member_list(
                &state,
                &locate_target,
                (previous_page, next_page),
                members,
                locale,
                window,
                cx,
            ))
        })
        .into_any_element()
}

fn empty_state(locale: Locale, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .px_6()
        .py_8()
        .text_center()
        .child(
            div()
                .size_10()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::MousePointerClick).text_color(cx.theme().muted_foreground),
                ),
        )
        .child(div().text_sm().child(text(locale, Key::NothingSelected)))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    Message::new(Key::NothingSelectedHint)
                        .arg(
                            "shortcut",
                            crate::shortcuts::shortcut_label(crate::shortcuts::keys::FOCUS_SEARCH),
                        )
                        .display(locale),
                ),
        )
        .into_any_element()
}

/// Name, kind and the commands that act on the whole selection.
#[allow(clippy::too_many_arguments)]
fn identity(
    state: &SelectionDetails<'_>,
    target: SelectionTarget,
    title: String,
    subtitle: String,
    locate: &LocateCommand,
    clear: ClickCommand,
    locale: Locale,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let net_color = match target {
        SelectionTarget::Net(net) => pomelo_render::scene::colors::net_color(net)
            .map(|[r, g, b, a]| Hsla::from(Rgba { r, g, b, a })),
        _ => None,
    };
    let icon = match target {
        SelectionTarget::Net(_) => IconName::Waypoints,
        SelectionTarget::Component(_) | SelectionTarget::ComponentGroup(_) => IconName::Cpu,
        SelectionTarget::Track(_) | SelectionTarget::Object(SelectedObject::Segment(_)) => {
            IconName::Spline
        }
        SelectionTarget::Object(SelectedObject::Pin(_)) => IconName::CircleDot,
        SelectionTarget::Object(SelectedObject::Via(_)) => IconName::CircleDashed,
        SelectionTarget::Object(_) => IconName::Layers,
    };
    let copied = title.clone();
    let scroll = &state.inspector_focus.scroll;
    div()
        .flex()
        .items_start()
        .gap_3()
        .px_4()
        .pt_4()
        .pb_3()
        .child(
            div()
                .relative()
                .size_8()
                .flex_shrink_0()
                .rounded_lg()
                .bg(theme.secondary)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).small())
                .when_some(net_color, |badge, color| {
                    badge.child(
                        div()
                            .absolute()
                            .right(px(-2.0))
                            .bottom(px(-2.0))
                            .size_2p5()
                            .rounded_full()
                            .border_2()
                            .border_color(theme.sidebar)
                            .bg(color),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_family(theme.mono_font_family.clone())
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(subtitle),
                )
                .when_some(state.candidate_position, |column, (current, total)| {
                    column.child(
                        div().text_xs().text_color(theme.muted_foreground).child(
                            Message::new(Key::PickCandidatePosition)
                                .arg("current", current)
                                .arg("total", total)
                                .display(locale),
                        ),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .child(FocusScroll::new(
                    "locate-selection",
                    scroll,
                    Button::new("locate-selection")
                        .ghost()
                        .small()
                        .icon(IconName::Crosshair)
                        .disabled(state.locate_pending)
                        .accessibility_label(locate_label(target, locale))
                        .native_tooltip(locate_label(target, locale))
                        .on_click(locate_on_click(locate, target)),
                ))
                .child(FocusScroll::new(
                    "copy-selection-name",
                    scroll,
                    Button::new("copy-selection-name")
                        .ghost()
                        .small()
                        .icon(IconName::Copy)
                        .accessibility_label(text(locale, Key::CopyName))
                        .native_tooltip(text(locale, Key::CopyName))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                        }),
                ))
                .child(FocusScroll::new(
                    "inspector-clear-selection",
                    scroll,
                    Button::new("inspector-clear-selection")
                        .ghost()
                        .small()
                        .icon(IconName::X)
                        .accessibility_label(text(locale, Key::ClearSelection))
                        .native_tooltip(text(locale, Key::ClearSelection))
                        .on_click(clear),
                )),
        )
}

fn locate_label(target: SelectionTarget, locale: Locale) -> String {
    Message::new(Key::LocateSelected)
        .arg("kind", text(locale, selection_identity(target).0))
        .display(locale)
}

/// Member counts by category, scanned left to right.
fn stats(summary: &SelectionSummary, locale: Locale, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let cells = [
        (Key::PropertySegments, summary.segments),
        (Key::PropertyPins, summary.pins),
        (Key::PropertyVias, summary.vias),
        (Key::PropertyZones, summary.zones),
    ];
    div().px_4().pb_3().child(
        div()
            .flex()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .children(cells.into_iter().enumerate().map(|(index, (key, count))| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_1p5()
                    .when(index > 0, |cell| {
                        cell.border_l_1().border_color(theme.border)
                    })
                    .child(
                        div().font_weight(FontWeight::SEMIBOLD).child(
                            Message::new(Key::UiNumber)
                                .arg("value", count)
                                .display(locale),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.2))
                            .text_color(theme.muted_foreground)
                            .child(text(locale, key)),
                    )
            })),
    )
}

/// Source properties and derived facts as one aligned label/value list.
fn facts(
    state: &SelectionDetails<'_>,
    locate: &LocateCommand,
    id: u32,
    locale: Locale,
    cx: &App,
) -> impl IntoElement {
    let target = state.selected_target;
    let object_target = matches!(target, Some(SelectionTarget::Object(_)));
    let layer_names: Vec<String> = state
        .selected_source_labels
        .iter()
        .filter(|message| is_layer_label(message))
        .filter_map(|message| {
            message
                .args
                .get("name")
                .or_else(|| message.args.get("id"))
                .map(ToString::to_string)
        })
        .collect();
    let measures_length = matches!(
        target,
        Some(
            SelectionTarget::Net(_)
                | SelectionTarget::Track(_)
                | SelectionTarget::Object(SelectedObject::Segment(_))
        )
    );
    div()
        .px_4()
        .pb_3()
        .flex()
        .flex_col()
        .gap_1()
        .text_sm()
        .children(
            state
                .selected_source_labels
                .iter()
                .filter(|message| !is_layer_label(message))
                .map(|message| {
                    if let Some((label, value)) =
                        crate::panels::inspector::source_property(message, locale)
                    {
                        property_row(label, value, cx)
                    } else {
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(crate::panels::inspector::display_source_field(
                                message,
                                locale,
                                state.display.length_unit,
                            ))
                            .into_any_element()
                    }
                }),
        )
        // Source objects list one label per occupied layer; fold them into
        // one row. Group selections show their member layers further down.
        .when(object_target && !layer_names.is_empty(), |body| {
            body.child(property_row(
                text(locale, Key::Layers),
                named_list(layer_names.iter().cloned(), layer_names.len(), locale),
                cx,
            ))
        })
        .child(property_row(
            text(locale, Key::PropertyIdentifier),
            format!(
                "0x{:X}",
                state.selected_anchor.map_or(id, |anchor| {
                    selection_identity(SelectionTarget::Object(anchor.object)).1
                })
            ),
            cx,
        ))
        .when_some(state.selected_anchor, |body, anchor| {
            body.child(property_row(
                text(locale, Key::PropertyHitLayer),
                crate::panels::inspector::hit_layer_name(anchor.layer, state.scene, locale),
                cx,
            ))
        })
        .when_some(
            state.selected_summary.filter(|_| measures_length),
            |body, summary| {
                body.child(
                    div()
                        .id("selection-length")
                        .child(property_row(
                            text(locale, Key::PropertyLength),
                            Message::new(Key::PropertyLengthValue)
                                .arg(
                                    "unit",
                                    text(
                                        locale,
                                        match state.display.length_unit {
                                            pomelo_core::units::LengthUnit::Millimeters => {
                                                Key::UnitMillimeters
                                            }
                                            pomelo_core::units::LengthUnit::Mils => Key::UnitMils,
                                        },
                                    ),
                                )
                                .arg(
                                    "length",
                                    format!(
                                        "{:.3}",
                                        state
                                            .display
                                            .length_unit
                                            .from_millimeters(summary.centerline_length_mm)
                                    ),
                                )
                                .display(locale),
                            cx,
                        ))
                        .tooltip({
                            let scope = text(locale, Key::SelectionLengthScope);
                            move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(scope.clone())
                                    .build(window, cx)
                            }
                        }),
                )
            },
        )
        // A single object already names its hit layer above.
        .when_some(
            state
                .selected_summary
                .filter(|_| !matches!(target, Some(SelectionTarget::Object(_)))),
            |body, summary| {
                body.when(!summary.layers.is_empty(), |body| {
                    body.child(property_row(
                        text(locale, Key::Layers),
                        named_list(
                            summary.layers.iter().map(|layer| {
                                crate::panels::inspector::hit_layer_name(
                                    *layer,
                                    state.scene,
                                    locale,
                                )
                            }),
                            summary.layers.len(),
                            locale,
                        ),
                        cx,
                    ))
                })
                .when(!summary.components.is_empty(), |body| {
                    body.child(property_row(
                        text(locale, Key::SearchComponents),
                        named_list(
                            summary.components.iter().filter_map(|component| {
                                state
                                    .scene
                                    .components
                                    .iter()
                                    .find(|placement| placement.id == *component)
                                    .map(|placement| placement.reference.clone())
                            }),
                            summary.components.len(),
                            locale,
                        ),
                        cx,
                    ))
                })
            },
        )
        .when_some(state.selected_related_net, |body, net| {
            body.child(related_button(
                "inspect-related-net",
                Message::new(Key::InspectRelatedNet)
                    .arg("id", net.0)
                    .display(locale),
                IconName::Waypoints,
                SelectionTarget::Net(net),
                state,
                locate,
            ))
        })
        .when_some(state.selected_related_component, |body, component| {
            body.child(related_button(
                "inspect-related-component",
                Message::new(Key::InspectRelatedComponent)
                    .arg("id", component.0)
                    .display(locale),
                IconName::Cpu,
                SelectionTarget::Component(component),
                state,
                locate,
            ))
        })
}

fn member_layers(layers: MemberLayers, scene: &BoardScene, locale: Locale) -> String {
    let name = |layer| crate::panels::inspector::hit_layer_name(layer, scene, locale);
    match layers {
        MemberLayers::None => String::new(),
        MemberLayers::Single(layer) => name(layer),
        MemberLayers::Span(first, last) => format!("{} → {}", name(first), name(last)),
    }
}

fn is_layer_label(message: &Message) -> bool {
    matches!(message.key, Key::SourceLayerName | Key::SourceLayerId)
}

/// The first few names, then a count of the rest.
fn named_list(names: impl Iterator<Item = String>, total: usize, locale: Locale) -> String {
    let mut shown: Vec<String> = names.take(NAMED_FACTS).collect();
    if total > shown.len() {
        shown.push(
            Message::new(Key::UiNumber)
                .arg("value", format!("+{}", total - shown.len()))
                .display(locale),
        );
    }
    shown.join(", ")
}

fn related_button(
    id: &'static str,
    label: String,
    icon: IconName,
    target: SelectionTarget,
    state: &SelectionDetails<'_>,
    locate: &LocateCommand,
) -> impl IntoElement {
    FocusScroll::new(
        id,
        &state.inspector_focus.scroll,
        Button::new(id)
            .outline()
            .small()
            .w_full()
            .icon(icon)
            .label(label)
            .disabled(state.locate_pending)
            .on_click(locate_on_click(locate, target)),
    )
    .w_full()
    .mt_1()
}

fn member_list(
    state: &SelectionDetails<'_>,
    locate: &LocateCommand,
    (previous_page, next_page): (ClickCommand, ClickCommand),
    members: &SelectionMembers,
    locale: Locale,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let paged = members.total > MEMBERS_PAGE;
    let first = state.members_offset + usize::from(!members.objects.is_empty());
    let last = state.members_offset + members.objects.len();
    div()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_4()
                .pt_3()
                .pb_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(text(locale, Key::ConnectedObjects)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if paged {
                            Message::new(Key::SourceMembersShown)
                                .arg("shown", format!("{first}–{last}"))
                                .arg("total", members.total)
                                .display(locale)
                        } else {
                            Message::new(Key::UiNumber)
                                .arg("value", members.total)
                                .display(locale)
                        }),
                ),
        )
        .child(
            div()
                // The member viewport owns its wheel input. Prevent it
                // from also scrolling the surrounding inspector.
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("selection-source-members")
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .h_64()
                        .flex_shrink_0()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(state.members_scroll)
                        .vertical_scrollbar(state.members_scroll)
                        .child(
                            div()
                                .id(("source-members-page", state.members_page_revision))
                                .w_full()
                                .min_w_0()
                                .flex_none()
                                .h_auto()
                                .min_h_full()
                                .px_2()
                                .flex()
                                .flex_col()
                                .children(members.objects.iter().zip(&members.layers).map(
                                    |(&object, &layers)| {
                                        member_row(object, layers, state, locate, locale, cx)
                                    },
                                )),
                        )
                        .map(|list| {
                            FocusScroll::new(
                                "source-members-region",
                                &state.inspector_focus.scroll,
                                list.w_full().min_w_0(),
                            )
                            .w_full()
                            .min_w_0()
                        }),
                ),
        )
        .when(paged, |list| {
            list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        crate::panels::diagnostics::page_button(
                            "members-previous",
                            text(locale, Key::MembersPrevious),
                            &state.members_navigation.previous,
                            state.locate_pending || state.members_offset == 0,
                            window,
                            cx,
                        )
                        .on_click(previous_page)
                        .map(|button| {
                            FocusScroll::new(
                                "members-previous",
                                &state.inspector_focus.scroll,
                                button,
                            )
                        }),
                    )
                    .child(
                        Message::new(Key::MembersPage)
                            .arg("page", state.members_offset / MEMBERS_PAGE + 1)
                            .arg("pages", members.total.div_ceil(MEMBERS_PAGE).max(1))
                            .display(locale),
                    )
                    .child(
                        crate::panels::diagnostics::page_button(
                            "members-next",
                            text(locale, Key::MembersNext),
                            &state.members_navigation.next,
                            state.locate_pending
                                || state.members_offset.saturating_add(MEMBERS_PAGE)
                                    >= members.total,
                            window,
                            cx,
                        )
                        .on_click(next_page)
                        .map(|button| {
                            FocusScroll::new("members-next", &state.inspector_focus.scroll, button)
                        }),
                    ),
            )
        })
}

fn member_row(
    object: SelectedObject,
    layers: MemberLayers,
    state: &SelectionDetails<'_>,
    locate: &LocateCommand,
    locale: Locale,
    cx: &App,
) -> impl IntoElement {
    let target = SelectionTarget::Object(object);
    let (kind, id) = selection_identity(target);
    let category = pomelo_core::picking::PickCategory::from(object) as u8;
    let key = (u64::from(category) << 32) | u64::from(id);
    let icon = match object {
        SelectedObject::Segment(_) => IconName::Spline,
        SelectedObject::Pin(_) => IconName::CircleDot,
        SelectedObject::Via(_) => IconName::CircleDashed,
        SelectedObject::Zone(_) => IconName::Layers,
        SelectedObject::Drawing(_) => IconName::Ruler,
        SelectedObject::Placement(_) => IconName::Cpu,
    };
    let name = Message::new(Key::LocateMember)
        .arg("kind", text(locale, kind))
        .arg("id", id)
        .display(locale);
    FocusScroll::new(
        ("source-member", key),
        state.members_scroll,
        Button::new(("source-member", key))
            .ghost()
            .small()
            .w_full()
            .justify_start()
            .selected(state.selected_target == Some(target))
            .disabled(state.locate_pending)
            .accessibility_label(name.clone())
            .native_tooltip(name)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(icon)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_shrink_0().child(text(locale, kind)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(Message::new(Key::UiNumber).arg("value", id).display(locale)),
                    )
                    // Layer lane: a member's layer, or the outer layers it spans.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(member_layers(layers, state.scene, locale)),
                    ),
            )
            .on_click(locate_on_click(locate, target)),
    )
    .w_full()
    .min_w_0()
}
