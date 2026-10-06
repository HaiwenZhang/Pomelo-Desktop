//! Selected-object details and member navigation, independent of viewport ownership.
use crate::{
    panels::{
        diagnostics::NavigationFocus,
        focus_scroll::{FocusScroll, InspectorFocus},
        inspector::selection_identity,
    },
    tooltips::ButtonTooltipExt,
};
use gpui_kit::{
    base::{Disableable, Selectable},
    component::{
        ActiveTheme, Icon,
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
    selection::{SelectionMembers, SelectionSummary, SelectionTarget},
};
use std::rc::Rc;

pub(crate) struct SelectionDetails<'a> {
    pub selected_target: Option<SelectionTarget>,
    pub candidate_position: Option<(usize, usize)>,
    pub selected_source_labels: &'a [Message],
    pub selected_anchor: Option<SelectionAnchor>,
    pub selected_related_net: Option<NetId>,
    pub selected_related_component: Option<ObjectId>,
    pub selected_summary: Option<SelectionSummary>,
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
type PageCommand = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub(crate) struct SelectionCommands {
    pub locate_target: LocateCommand,
    pub previous_page: PageCommand,
    pub next_page: PageCommand,
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
    let inspector_details = div()
            .w_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().id("inspector-content").child(
                    div()
                        .px_4()
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_sm()
                        .when(state.selected_target.is_none(), |body| {
                            body.child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(locale, Key::NothingSelected)),
                            )
                        })
                        .when_some(
                            state.candidate_position,
                            |body, (current, total)| {
                                body.child(
                                    Message::new(Key::PickCandidatePosition)
                                        .arg("current", current)
                                        .arg("total", total)
                                        .display(locale),
                                )
                            },
                        )
                        .when_some(state.selected_target, |body, target| {
                            let (kind, id) = selection_identity(target);
                            let network_color = match target {
                                pomelo_core::selection::SelectionTarget::Net(net) => {
                                    pomelo_render::scene::colors::net_color(net)
                                }
                                _ => None,
                            };
                            let identity_color = network_color
                                .map(|[r, g, b, a]| Hsla::from(Rgba { r, g, b, a }))
                                .unwrap_or(cx.theme().primary);
                            let title = state
                                .selected_source_labels
                                .iter()
                                .find_map(|message| {
                                    message.args.get("name").map(ToString::to_string)
                                })
                                .unwrap_or_else(|| {
                                    Message::new(Key::SelectionIdentity)
                                        .arg("kind", text(locale, kind))
                                        .arg("id", id)
                                        .display(locale)
                                });
                            body.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .min_w_0()
                                    .gap_2()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .size_4()
                                            .flex_shrink_0()
                                            .rounded_full()
                                            .bg(identity_color),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .px_2()
                                            .py_1()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .child(text(locale, kind)),
                                    ),
                            )
                            .children(state.selected_source_labels.iter().map(|message| {
                                if let Some((label, value)) =
                                    crate::panels::inspector::source_property(message, locale)
                                {
                                    crate::panels::inspector::property_row(label, value, cx)
                                } else {
                                    div()
                                        .child(crate::panels::inspector::display_source_field(
                                            message,
                                            locale,
                                            state.display.length_unit,
                                        ))
                                        .into_any_element()
                                }
                            }))
                            .child(crate::panels::inspector::property_row(
                                text(locale, Key::PropertyIdentifier),
                                format!("0x{:X}", state.selected_anchor.map_or(id, |anchor| selection_identity(pomelo_core::selection::SelectionTarget::Object(anchor.object)).1)),
                                cx,
                            ))
                            .when_some(state.selected_anchor, |body, anchor| {
                                body.child(crate::panels::inspector::property_row(
                                    text(locale, Key::PropertyHitLayer),
                                    crate::panels::inspector::hit_layer_name(anchor.layer, state.scene, locale),
                                    cx,
                                ))
                            })
                        })
                        .when_some(state.selected_related_net, |body, net| {
                            body.child(
                                Button::new("inspect-related-net")
                                    .ghost()
                                    .label(
                                        Message::new(Key::InspectRelatedNet)
                                            .arg("id", net.0)
                                            .display(locale),
                                    )
                                    .disabled(state.locate_pending)
                                    .on_click(locate_on_click(&commands.locate_target, pomelo_core::selection::SelectionTarget::Net(net)))
                                    .map(|button| {
                                        FocusScroll::new(
                                            "inspect-related-net",
                                            &state.inspector_focus.scroll,
                                            button.w_full(),
                                        )
                                    }),
                            )
                        })
                        .when_some(state.selected_related_component, |body, component| {
                            body.child(
                                Button::new("inspect-related-component")
                                    .ghost()
                                    .label(
                                        Message::new(Key::InspectRelatedComponent)
                                            .arg("id", component.0)
                                            .display(locale),
                                    )
                                    .disabled(state.locate_pending)
                                    .on_click(locate_on_click(&commands.locate_target, pomelo_core::selection::SelectionTarget::Component(component)))
                                    .map(|button| {
                                        FocusScroll::new(
                                            "inspect-related-component",
                                            &state.inspector_focus.scroll,
                                            button.w_full(),
                                        )
                                    }),
                            )
                        })
                        .when_some(state.selected_summary, |body, summary| {
                            let body = body.children(
                                [
                                    (Key::PropertySegments, summary.segments),
                                    (Key::PropertyPins, summary.pins),
                                    (Key::PropertyVias, summary.vias),
                                    (Key::PropertyZones, summary.zones),
                                    (Key::PropertyDrawings, summary.drawings),
                                ]
                                .into_iter()
                                .map(|(key, count)| {
                                    crate::panels::inspector::property_row(
                                        text(locale, key),
                                        Message::new(Key::UiNumber)
                                            .arg("value", count)
                                            .display(locale),
                                        cx,
                                    )
                                }),
                            );
                            body.when(
                                matches!(
                                    state.selected_target,
                                    Some(
                                        pomelo_core::selection::SelectionTarget::Net(_)
                                            | pomelo_core::selection::SelectionTarget::Track(_)
                                            | pomelo_core::selection::SelectionTarget::Object(
                                                pomelo_core::selection::SelectedObject::Segment(_)
                                            )
                                    )
                                ),
                                |body| {
                                    body.child(crate::panels::inspector::property_row(
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
                                                    pomelo_core::units::LengthUnit::Mils => {
                                                        Key::UnitMils
                                                    }
                                                },
                                            ),
                                        )
                                        .arg(
                                            "length",
                                            format!(
                                                "{:.6}",
                                                state.display
                                                    .length_unit
                                                    .from_millimeters(summary.centerline_length_mm)
                                            ),
                                        )
                                        .display(locale),
                                    cx,
                                ))
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text(locale, Key::SelectionLengthScope)),
                                )
                                },
                            )
                        })
                        .when_some(state.selected_members.as_ref(), |body, members| {
                            body.child(
                                div()
                                    .pt_3()
                                    .pb_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(text(locale, Key::ConnectedObjects)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        Message::new(Key::SourceMembersShown)
                                            .arg("shown", members.objects.len())
                                            .arg("total", members.total)
                                            .display(locale),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_1()
                                    .child(
                                        Message::new(Key::MembersPage)
                                            .arg("page", state.members_offset / 256 + 1)
                                            .arg("pages", members.total.div_ceil(256).max(1))
                                            .display(locale),
                                    )
                                    .child(
                                        crate::panels::diagnostics::page_button(
                                                "members-previous",
                                                text(locale, Key::MembersPrevious),
                                                &state.members_navigation.previous,
                                                state.locate_pending || state.members_offset == 0,
                                                window,
                                                cx,
                                            )
                                            .on_click(commands.previous_page)
                                            .map(|button| {
                                                FocusScroll::new(
                                                    "members-previous",
                                                    &state.inspector_focus.scroll,
                                                    button,
                                                )
                                            }),
                                    )
                                    .child(
                                        crate::panels::diagnostics::page_button(
                                                "members-next",
                                                text(locale, Key::MembersNext),
                                                &state.members_navigation.next,
                                                state.locate_pending
                                                    || state.members_offset.saturating_add(256)
                                                        >= members.total,
                                                window,
                                                cx,
                                            )
                                            .on_click(commands.next_page)
                                            .map(|button| {
                                                FocusScroll::new(
                                                    "members-next",
                                                    &state.inspector_focus.scroll,
                                                    button,
                                                )
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
                                            .h_24()
                                            .flex_shrink_0()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .track_scroll(state.members_scroll)
                                            .vertical_scrollbar(state.members_scroll)
                                            .child(
                                                div().id(("source-members-page", state.members_page_revision)).w_full().min_w_0().flex_none().h_auto().min_h_full()
                                                .flex().flex_col()
                                                .children(members.objects.iter().map(|&object| {
                                                let (kind, id) = selection_identity(
                                                    pomelo_core::selection::SelectionTarget::Object(
                                                        object,
                                                    ),
                                                );
                                                let target =
                                                    pomelo_core::selection::SelectionTarget::Object(
                                                        object,
                                                    );
                                                let category =
                                                    pomelo_core::picking::PickCategory::from(object)
                                                        as u8;
                                                Button::new((
                                            "source-member",
                                            (u64::from(category) << 32) | u64::from(id),
                                        ))
                                        .ghost()
                                        .w_full()
                                        .h_10()
                                        .flex_shrink_0()
                                        .mb_2()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .rounded_md()
                                        .selected(state.selected_target == Some(target))
                                        .disabled(state.locate_pending)
                                        .native_tooltip(
                                            Message::new(Key::LocateMember)
                                                .arg("kind", text(locale, kind))
                                                .arg("id", id)
                                                .display(locale),
                                        )
                                        .accessibility_label(
                                            Message::new(Key::LocateMember)
                                                .arg("kind", text(locale, kind))
                                                .arg("id", id)
                                                .display(locale),
                                        )
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    Icon::new(
                                                        gpui_kit::assets::IconName::CircuitBoard,
                                                    )
                                                    .size_4()
                                                    .flex_shrink_0(),
                                                )
                                                .child(
                                                    div().flex_1().min_w_0().flex().gap_1()
                                                        .child(div().min_w_0().truncate().child(text(locale, kind)))
                                                        .child(div().flex_shrink_0().child(
                                                            Message::new(Key::UiNumber).arg("value", id).display(locale),
                                                        )),
                                                )
                                                .child(
                                                    Icon::new(
                                                        gpui_kit::assets::IconName::ChevronRight,
                                                    )
                                                    .size_4()
                                                    .flex_shrink_0(),
                                                ),
                                        )
                                        .on_click(
                                            locate_on_click(&commands.locate_target, target),
                                        )
                                        .map(|button| FocusScroll::new(
                                            ("source-member", (u64::from(category) << 32) | u64::from(id)),
                                            state.members_scroll,
                                            button,
                                        ).w_full().min_w_0())
                                            })),
                                            )
                                            .map(|list| FocusScroll::new("source-members-region", &state.inspector_focus.scroll, list.w_full().min_w_0()).w_full().min_w_0()),
                                    ),
                            )
                        }),
                ),
            );
    inspector_details.into_any_element()
}
