//! Right panel bindings: inspector, display settings, review and the file footer.
use super::*;
impl BoardViewport {
    pub(super) fn render_inspector_panel(
        &mut self,
        right_panel_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let locale = i18n::current(cx);
        let measurements = self.review.measurements.len();
        let switcher = crate::panels::tabs::render(
            crate::panels::tabs::SidebarTabs {
                id: "right-panel-tabs",
                tabs: vec![
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::Inspector).into(),
                        icon: IconName::ScanSearch,
                        count: None,
                    },
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::DisplayPanel).into(),
                        icon: IconName::SlidersHorizontal,
                        count: None,
                    },
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::ReviewPanel).into(),
                        icon: IconName::Ruler,
                        count: (measurements > 0).then(|| {
                            Message::new(Key::UiNumber)
                                .arg("value", measurements)
                                .display(locale)
                                .into()
                        }),
                    },
                ],
                selected: self.right_panel.index(),
                width: right_panel_width - window.rem_size() * 1.5,
            },
            cx.listener(|this, index: &usize, _, cx| {
                this.right_panel = super::RightPanelTab::from_index(*index);
                cx.notify();
            }),
            window,
            cx,
        );
        let body = match self.right_panel {
            super::RightPanelTab::Inspector => self.render_inspector_body(locale, window, cx),
            super::RightPanelTab::Display => self.render_display_body(locale, window, cx),
            super::RightPanelTab::Review => self.render_review_panel(window, cx),
        };
        let diagnostic_count = self.scene.diagnostics.len() + self.render_diagnostics.len();
        div()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .child(switcher)
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
                            .children(self.render_selection_notices(locale, cx))
                            .child(body),
                    ),
            )
            .child(crate::panels::file_info::render(
                crate::panels::file_info::FileInformation {
                    locale,
                    expanded: self.file_information_expanded,
                    scene: &self.scene,
                    format_name: &self.format_name,
                    diagnostic_count,
                },
                self.diagnostics_panel(window, cx),
                &self.inspector_focus.file_information,
                &self.file_information_scroll,
                cx.listener(|this, expanded: &bool, _, cx| {
                    this.file_information_expanded = *expanded;
                    cx.notify();
                }),
                window,
                cx,
            ))
            .into_any_element()
    }

    /// Pending locate work and recoverable selection failures, above any tab.
    fn render_selection_notices(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &App,
    ) -> Vec<AnyElement> {
        let notice = |message: String, color: Hsla| {
            div()
                .mx_4()
                .mt_3()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(cx.theme().secondary)
                .text_sm()
                .text_color(color)
                .child(message)
                .into_any_element()
        };
        let mut notices = Vec::new();
        if self.locate_pending {
            notices.push(notice(
                text(locale, self.pending_message),
                cx.theme().muted_foreground,
            ));
        }
        if let Some(message) = &self.search_notice {
            notices.push(notice(message.display(locale), cx.theme().warning));
        }
        if let Some(error) = &self.locate_details {
            let message = match error {
                pomelo_core::geometry::PathError::Cancelled => Message::new(Key::Cancelled),
                pomelo_core::geometry::PathError::Invalid(id) => {
                    Message::new(Key::PickInvalid).arg("object", id.0)
                }
                pomelo_core::geometry::PathError::PointLimit { actual, limit } => {
                    Message::new(Key::GeometryLimit)
                        .arg("actual", *actual)
                        .arg("limit", *limit)
                }
            };
            notices.push(notice(message.display(locale), cx.theme().warning));
        }
        notices
    }

    fn render_inspector_body(
        &mut self,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::panels::selection_details::render(
            crate::panels::selection_details::SelectionDetails {
                selected_target: self.selected_target,
                selected_anchor: self.selected_anchor,
                selected_related_net: self.selected_related_net,
                selected_related_component: self.selected_related_component,
                selected_summary: self.selected_summary.as_ref(),
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
                clear: Box::new(cx.listener(|this, _, window, cx| {
                    this.clear_selection(&ClearSelection, window, cx)
                })),
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
        )
    }

    fn render_display_body(
        &mut self,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut appearance = self.opacity_controls(locale, cx);
        appearance.push(self.grid_spacing_control(locale, cx));
        for (id, target) in [
            ("grid-color", pomelo_core::appearance::ColorTarget::Grid),
            (
                "canvas-background-color",
                pomelo_core::appearance::ColorTarget::Background,
            ),
            (
                "drill-display-color",
                pomelo_core::appearance::ColorTarget::Drill,
            ),
        ] {
            let control = self.color_control(target, locale, window, cx);
            appearance.push(
                FocusScroll::new(id, &self.inspector_focus.scroll, control)
                    .w_full()
                    .min_h_8()
                    .into_any_element(),
            );
        }
        let net_colors = self.net_color_controls(locale, window, cx);
        div()
            .flex()
            .flex_col()
            .children(net_colors)
            .children(crate::panels::display::controls(
                locale,
                &self.display,
                self.display_commands(cx),
                appearance,
                self.object_filter_controls(locale, cx),
                &self.inspector_focus.scroll,
                cx,
            ))
            .into_any_element()
    }

    fn display_commands(&self, cx: &mut Context<Self>) -> crate::panels::display::DisplayCommands {
        crate::panels::display::DisplayCommands {
            net_colors: Box::new(cx.listener(|this, shown: &bool, _, cx| {
                Arc::make_mut(&mut this.display).appearance.hide_net_colors = !*shown;
                cx.notify();
            })),
            grid: Box::new(cx.listener(|this, enabled: &bool, _, cx| {
                Arc::make_mut(&mut this.display).grid.visible = *enabled;
                cx.notify();
            })),
            shadow: Box::new(cx.listener(|this, enabled: &bool, _, cx| {
                Arc::make_mut(&mut this.display).shadow.enabled = *enabled;
                cx.notify();
            })),
            dim_active: Box::new(cx.listener(|this, enabled: &bool, _, cx| {
                Arc::make_mut(&mut this.display).shadow.dim_active = *enabled;
                cx.notify();
            })),
            filled: Box::new(cx.listener(|this, filled: &bool, _, cx| {
                if this.display.filled != *filled {
                    Arc::make_mut(&mut this.display).filled = *filled;
                    this.invalidate_hover();
                    cx.notify();
                }
            })),
            horizontal_pin_names: Box::new(cx.listener(|this, horizontal: &bool, _, cx| {
                Arc::make_mut(&mut this.display).horizontal_pin_names = *horizontal;
                this.invalidate_hover();
                cx.notify();
            })),
            active_layer_labels_only: Box::new(cx.listener(|this, enabled: &bool, _, cx| {
                Arc::make_mut(&mut this.display).active_layer_labels_only = *enabled;
                cx.notify();
            })),
            labels: Box::new(cx.listener(
                |this, (kind, enabled): &(pomelo_core::display::LabelKind, bool), _, cx| {
                    Arc::make_mut(&mut this.display)
                        .label_options
                        .set(*kind, *enabled);
                    cx.notify();
                },
            )),
            drills: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                if this.display.show_drills != *visible {
                    Arc::make_mut(&mut this.display).show_drills = *visible;
                    this.invalidate_hover();
                    cx.notify();
                }
            })),
            backdrills: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                if this.display.show_backdrills != *visible {
                    Arc::make_mut(&mut this.display).show_backdrills = *visible;
                    this.invalidate_hover();
                    cx.notify();
                }
            })),
            copper: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                Arc::make_mut(&mut this.display).show_copper = *visible;
                this.invalidate_hover();
                cx.notify();
            })),
            static_shapes_fill_solid: Box::new(cx.listener(|this, solid: &bool, _, cx| {
                Arc::make_mut(&mut this.display).static_shapes_fill_solid = *solid;
                cx.notify();
            })),
            texts: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                Arc::make_mut(&mut this.display).show_texts = *visible;
                cx.notify();
            })),
            drawings: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                Arc::make_mut(&mut this.display).show_drawings = *visible;
                cx.notify();
            })),
            reset_order: Box::new(cx.listener(|this, _, _, cx| {
                Arc::make_mut(&mut this.display).layer_order.clear();
                this.layer_scroll_target = Some(super::LayerScrollTarget::Start);
                this.invalidate_hover();
                cx.notify();
            })),
        }
    }
}
