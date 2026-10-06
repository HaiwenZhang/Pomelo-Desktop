//! Inspector and display panel bindings for the viewport.
use super::*;
use crate::workbench::panel_layout::Side;
impl BoardViewport {
    pub(super) fn render_inspector_panel(
        &mut self,
        right_panel_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let locale = i18n::current(cx);
        let opacity_controls: Vec<_> = if self.show_display {
            vec![self.opacity_control(locale, cx)]
        } else {
            Vec::new()
        };
        let appearance_controls = if self.show_display {
            Some(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(
                        [
                            (
                                "canvas-background-color",
                                pomelo_core::appearance::ColorTarget::Background,
                            ),
                            (
                                "drill-display-color",
                                pomelo_core::appearance::ColorTarget::Drill,
                            ),
                        ]
                        .map(|(id, target)| {
                            let control = self.color_control(target, locale, window, cx);
                            FocusScroll::new(id, &self.inspector_focus.scroll, control)
                        }),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let right_tab_labels: Vec<SharedString> = [Key::Inspector, Key::DisplayPanel]
            .map(|key| text(locale, key).into())
            .into();
        let inspector_details = crate::panels::selection_details::render(
            crate::panels::selection_details::SelectionDetails {
                selected_target: self.selected_target,
                selected_anchor: self.selected_anchor,
                selected_related_net: self.selected_related_net,
                selected_related_component: self.selected_related_component,
                selected_summary: self.selected_summary,
                locate_pending: self.locate_pending,
                members_offset: self.members_offset,
                members_page_revision: self.members_page_revision,
                selected_source_labels: &self.selected_source_labels,
                selected_members: &self.selected_members,
                members_scroll: &self.members_scroll,
                members_navigation: &self.members_navigation,
                inspector_focus: &self.inspector_focus,
                display: &self.display,
                scene: &self.scene,
                candidate_position: self.candidate_position.filter(|_| {
                    self.last_pick.as_ref().is_some_and(|last| {
                        last.matches_view(
                            self.navigation.camera(),
                            self.navigation.size(),
                            &self.display,
                            self.selection_mode,
                        )
                    })
                }),
            },
            crate::panels::selection_details::SelectionCommands {
                locate_target: std::rc::Rc::new(cx.listener(
                    |this, target: &pomelo_core::selection::SelectionTarget, window, cx| {
                        this.locate_target(*target, window, cx)
                    },
                )),
                previous_page: Box::new(cx.listener(|this, _, window, cx| {
                    this.page_members(this.members_offset.saturating_sub(256), window, cx)
                })),
                next_page: Box::new(cx.listener(|this, _, window, cx| {
                    this.page_members(this.members_offset.saturating_add(256), window, cx)
                })),
            },
            locale,
            window,
            cx,
        );
        let inspector = div()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(crate::panels::sidebar::header(crate::panels::tabs::render(
                crate::panels::tabs::SidebarTabs {
                    id: "right-panel-tabs",
                    locale,
                    labels: right_tab_labels,
                    selected: usize::from(self.show_display),
                    width: right_panel_width - window.rem_size() * 2.5,
                },
                cx.listener(|this, index: &usize, _, cx| {
                    this.show_display = *index == 1;
                    cx.notify();
                }),
                window,
                cx,
            ), Side::Right, crate::panels::sidebar::toggle_button(Side::Right, false, locale,
                |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleRightPanel), cx))))
            .child(
                div()
                    .id("right-panel-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.inspector_focus.scroll)
                    .vertical_scrollbar(&self.inspector_focus.scroll)
                    .child(
                        div()
                            .flex_none()
                            .h_auto()
                            .min_h_full()
                            .when(self.locate_pending, |body| {
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .child(text(locale, self.pending_message)),
                                )
                            })
                            .when_some(self.search_notice.as_ref(), |body, message| {
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child(message.display(locale)),
                                )
                            })
                            .when_some(self.locate_details.as_ref(), |body, error| {
                                let message = match error {
                                    pomelo_core::geometry::PathError::Cancelled => {
                                        Message::new(Key::Cancelled)
                                    }
                                    pomelo_core::geometry::PathError::Invalid(id) => {
                                        Message::new(Key::PickInvalid).arg("object", id.0)
                                    }
                                    pomelo_core::geometry::PathError::PointLimit {
                                        actual,
                                        limit,
                                    } => Message::new(Key::GeometryLimit)
                                        .arg("actual", *actual)
                                        .arg("limit", *limit),
                                };
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child(message.display(locale)),
                                )
                            })
                            .when(!self.show_display, |body| {
                                body.child(
                            div()
                                .px_4()
                                .pt_4()
                                .pb_2()
                                .flex()
                                .items_center()
                                .justify_between()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(text(locale, Key::SelectedObject))
                                .when(
                                    self.selected_target.is_some() || self.locate_pending,
                                    |heading| {
                                        heading.child(
                                            Button::new("inspector-clear-selection")
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::X)
                                                .accessibility_label(text(
                                                    locale,
                                                    Key::ClearSelection,
                                                ))
                                                .native_tooltip(text(locale, Key::ClearSelection))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.clear_selection(
                                                        &ClearSelection,
                                                        window,
                                                        cx,
                                                    )
                                                }))
                                                .map(|button| FocusScroll::new("inspector-clear-selection", &self.inspector_focus.scroll, button)),
                                        )
                                    },
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .mx_4()
                                .p_1()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .children(
                                    [
                                        (
                                            pomelo_core::interaction::SelectionMode::Object,
                                            Key::ModeObject,
                                            0u32,
                                        ),
                                        (
                                            pomelo_core::interaction::SelectionMode::Track,
                                            Key::ModeTrack,
                                            1u32,
                                        ),
                                        (
                                            pomelo_core::interaction::SelectionMode::Net,
                                            Key::ModeNet,
                                            2u32,
                                        ),
                                        (
                                            pomelo_core::interaction::SelectionMode::Component,
                                            Key::ModeComponent,
                                            3u32,
                                        ),
                                    ]
                                    .into_iter()
                                    .map(|(mode, key, id)| {
                                        Button::new(("selection-mode", id))
                                            .ghost()
                                            .small()
                                            .when(self.selection_mode == mode, |button| button.primary())
                                            .px_1()
                                            .flex_grow(1.0)
                                            .flex_shrink_0()
                                            .native_tooltip(text(locale, key))
                                            .label(text(locale, key))
                                            .selected(self.selection_mode == mode)
                                            .toggled(self.selection_mode == mode)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_selection_mode(mode, window, cx);
                                            }))
                                            .map(|button| FocusScroll::new(("selection-mode", id), &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0())
                                    }),
                                ),
                        )
                        .child(inspector_details)
                        .child(
                            div()
                                .px_4()
                                .py_2()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Button::new("locate-selection")
                                        .primary()
                                        .flex_grow(1.0)
                                        .flex_shrink_0()
                                        .label(
                                            Message::new(Key::LocateSelected)
                                                .arg(
                                                    "kind",
                                                    text(
                                                        locale,
                                                        self.selected_target
                                                            .map(|target| {
                                                                selection_identity(target).0
                                                            })
                                                            .unwrap_or(Key::ModeObject),
                                                    ),
                                                )
                                                .display(locale),
                                        )
                                        .disabled(
                                            self.selected_target.is_none() || self.locate_pending,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if let Some(target) = this.selected_target {
                                                this.locate_target(target, window, cx);
                                            }
                                        }))
                                        .map(|button| FocusScroll::new("locate-selection", &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0()),
                                )
                                .child(
                                    Button::new("clear-selection")
                                        .outline()
                                        .flex_grow(1.0)
                                        .flex_shrink_0()
                                        .label(text(locale, Key::ClearSelection))
                                        .disabled(
                                            self.selected_target.is_none() && !self.locate_pending,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.clear_selection(&ClearSelection, window, cx)
                                        }))
                                        .map(|button| FocusScroll::new("clear-selection", &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0()),
                                ),
                        )
                        .child(crate::panels::picking::controls(
                            locale,
                            self.pick_filter,
                            self.selection_mode,
                            &self.inspector_focus.scroll,
                            cx,
                            |category| {
                                Box::new(cx.listener(move |this, enabled: &bool, _, cx| {
                                    if this.pick_filter.contains(category) == *enabled {
                                        return;
                                    }
                                    this.pick_filter.set(category, *enabled);
                                    this.hover_cancel.cancel();
                                    this.hovered = None;
                                    this.hover_notice = None;
                                    this.last_pick = None;
                                    this.candidate_position = None;
                                    this.search_cancel.cancel();
                                    this.search_pending = false;
                                    this.locate_pending = false;
                                    this.search_notice = None;
                                    this.locate_details = None;
                                    cx.notify();
                                }))
                            },
                        ))
                            })
                            .when(self.show_display, |body| {
                                body.when_some(appearance_controls, |body, controls| body.child(controls)).children(opacity_controls).children(crate::panels::display::controls(
                                    locale,
                                    &self.display,
                                    crate::panels::display::DisplayCommands {
                                        filled: Box::new(cx.listener(
                                            |this, filled: &bool, _, cx| {
                                                if this.display.filled != *filled {
                                                    Arc::make_mut(&mut this.display).filled = *filled;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        horizontal_pin_names: Box::new(cx.listener(
                                            |this, horizontal: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).horizontal_pin_names = *horizontal;
                                                this.invalidate_hover();
                                                cx.notify();
                                            },
                                        )),
                                        labels: Box::new(cx.listener(
                                            |this, (kind, enabled): &(pomelo_core::display::LabelKind, bool), _, cx| {
                                                Arc::make_mut(&mut this.display).label_options.set(*kind, *enabled);
                                                cx.notify();
                                            },
                                        )),
                                        drills: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                if this.display.show_drills != *visible {
                                                    Arc::make_mut(&mut this.display).show_drills =
                                                        *visible;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        backdrills: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                if this.display.show_backdrills != *visible {
                                                    Arc::make_mut(&mut this.display).show_backdrills = *visible;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        copper: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_copper =
                                                    *visible;
                                                this.invalidate_hover();
                                                cx.notify();
                                            },
                                        )),
                                        static_shapes_fill_solid: Box::new(cx.listener(
                                            |this, solid: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).static_shapes_fill_solid = *solid;
                                                cx.notify();
                                            },
                                        )),
                                        texts: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_texts =
                                                    *visible;
                                                cx.notify();
                                            },
                                        )),
                                        drawings: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_drawings =
                                                    *visible;
                                                cx.notify();
                                            },
                                        )),
                                        reset_order: Box::new(cx.listener(|this, _, _, cx| {
                                            Arc::make_mut(&mut this.display).layer_order.clear();
                                            this.layer_scroll_target = Some(0);
                                            this.invalidate_hover();
                                            cx.notify();
                                        })),
                                    },
                                    &self.inspector_focus.scroll,
                                ))
                            })
                            .child(crate::panels::file_info::render(
                                crate::panels::file_info::FileInformation {
                                    locale,
                                    expanded: self.file_information_expanded,
                                    scene: &self.scene,
                                    format_name: &self.format_name,
                                },
                                self.diagnostics_panel(window, cx),
                                &self.inspector_focus.file_information,
                                cx.listener(|this, expanded: &bool, _, cx| {
                                    this.file_information_expanded = *expanded;
                                    cx.notify();
                                }),
                                window,
                                cx,
                            )),
                    ),
            );

        inspector.into_any_element()
    }
}
