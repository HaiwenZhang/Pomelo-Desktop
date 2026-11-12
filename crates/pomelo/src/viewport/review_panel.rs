//! Review tab: measurements, clearance, the selection set, highlight groups, named
//! views and export. Every command is bound to the document's own viewport.
use super::{
    review::{RenameTarget, ReviewTool, SET_PAGE, format_angle, format_length},
    review_export::ImageFormat,
    *,
};
use gpui_kit::component::checkbox::Checkbox;
use pomelo_core::{
    i18n::Locale,
    review::{
        export::object_kind,
        measure::{Leg, MeasureKind, Measurement, SnapKind},
    },
    selection::SelectedObject,
};

fn heading(label: String, cx: &App) -> Div {
    div()
        .px_4()
        .pt_4()
        .pb_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .font_weight(FontWeight::SEMIBOLD)
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().truncate().child(label))
}

fn note(content: impl IntoElement, cx: &App) -> Div {
    div()
        .px_4()
        .pb_2()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(content)
}

fn warning(message: &Message, locale: Locale, cx: &App) -> Div {
    div()
        .px_4()
        .pb_2()
        .text_sm()
        .text_color(cx.theme().warning)
        .child(message.display(locale))
}

fn object_label(object: SelectedObject, locale: Locale) -> String {
    let (_, id) = selection_identity(pomelo_core::selection::SelectionTarget::Object(object));
    Message::new(Key::ReviewObjectLabel)
        .arg("kind", text(locale, object_kind(object)))
        .arg("id", id)
        .display(locale)
}

fn leg_text(leg: Leg, unit: pomelo_core::units::LengthUnit, locale: Locale) -> String {
    Message::new(Key::MeasureLeg)
        .arg("dx", format_length(leg.dx, unit, locale))
        .arg("dy", format_length(leg.dy, unit, locale))
        .arg("distance", format_length(leg.distance, unit, locale))
        .arg("angle", format_angle(leg.angle_degrees))
        .display(locale)
}

fn snap_key(snap: SnapKind) -> Key {
    match snap {
        SnapKind::Free => Key::SnapFree,
        SnapKind::SegmentEnd => Key::SnapSegmentEnd,
        SnapKind::PinCenter => Key::SnapPinCenter,
        SnapKind::ViaCenter => Key::SnapViaCenter,
    }
}

fn icon_button(id: impl Into<ElementId>, icon: IconName, label: String) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .accessibility_label(label.clone())
        .native_tooltip(label)
}

impl BoardViewport {
    fn measurement_rows(
        &self,
        index: usize,
        measurement: &Measurement,
        active: bool,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Div {
        let unit = self.display.length_unit;
        let kind = text(
            locale,
            match measurement.kind {
                MeasureKind::Distance => Key::MeasureDistance,
                MeasureKind::Polyline => Key::MeasurePolyline,
            },
        );
        let title = Message::new(Key::MeasurementTitle)
            .arg("index", index + 1)
            .arg("kind", kind)
            .display(locale);
        let mut row = div()
            .px_4()
            .py_1()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .when(active, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(text(locale, Key::MeasurementInProgress)),
                        )
                    })
                    .when(!active, |row| {
                        row.child(
                            icon_button(
                                ("remove-measurement", index),
                                IconName::Trash,
                                text(locale, Key::RemoveMeasurement),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if index < this.review.measurements.len() {
                                        this.review.measurements.remove(index);
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                    }),
            );
        if let Some(span) = measurement.span() {
            let label = match measurement.kind {
                MeasureKind::Distance => leg_text(span, unit, locale),
                MeasureKind::Polyline => Message::new(Key::LabeledValue)
                    .arg("label", text(locale, Key::MeasureSpan))
                    .arg("value", leg_text(span, unit, locale))
                    .display(locale),
            };
            row = row.child(div().child(label)).child(
                div().child(
                    Message::new(Key::MeasureManhattan)
                        .arg(
                            "distance",
                            format_length(span.manhattan_distance(), unit, locale),
                        )
                        .display(locale),
                ),
            );
        }
        if measurement.kind == MeasureKind::Polyline && measurement.points.len() >= 2 {
            row = row.child(
                div().child(
                    Message::new(Key::MeasureTotal)
                        .arg(
                            "length",
                            format_length(measurement.total_length(), unit, locale),
                        )
                        .display(locale),
                ),
            );
            if let Some(last) = measurement
                .legs()
                .last()
                .filter(|_| measurement.points.len() > 2)
            {
                row = row.child(
                    div().child(
                        Message::new(Key::LabeledValue)
                            .arg("label", text(locale, Key::MeasureLastLeg))
                            .arg("value", leg_text(last, unit, locale))
                            .display(locale),
                    ),
                );
            }
        }
        row.children(
            measurement
                .points
                .iter()
                .enumerate()
                .map(|(point_index, point)| {
                    let position = crate::panels::inspector::display_source_field(
                        &Message::new(Key::SourcePosition)
                            .arg("x", point.position.x.to_string())
                            .arg("y", point.position.y.to_string()),
                        locale,
                        unit,
                    );
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            Message::new(Key::MeasurePoint)
                                .arg("index", point_index + 1)
                                .arg("position", position)
                                .arg("snap", text(locale, snap_key(point.snap)))
                                .display(locale),
                        )
                }),
        )
    }

    fn render_measurements_section(&self, locale: Locale, cx: &mut Context<Self>) -> Div {
        let review = &self.review;
        let unit = self.display.length_unit;
        let has_any = !review.measurements.is_empty() || review.clearance.is_some();
        let active = review
            .active
            .as_ref()
            .filter(|measurement| !measurement.points.is_empty());
        let mut section = div().flex().flex_col().child(
            heading(text(locale, Key::MeasurementsHeading), cx)
                .when(
                    matches!(
                        review.tool,
                        Some(ReviewTool::Measure(_) | ReviewTool::Clearance)
                    ),
                    |heading| {
                        heading.child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new("finish-measurement")
                                        .ghost()
                                        .xsmall()
                                        .label(text(locale, Key::FinishMeasurement))
                                        .disabled(active.is_none())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.finish_measurement(&FinishMeasurement, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("cancel-measurement")
                                        .ghost()
                                        .xsmall()
                                        .label(text(locale, Key::CancelMeasurement))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel_canvas_action(
                                                &CancelCanvasAction,
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                    },
                )
                .when(has_any, |heading| {
                    heading.child(
                        icon_button(
                            "clear-measurements",
                            IconName::Trash,
                            text(locale, Key::ClearMeasurements),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.clear_measurements(cx))),
                    )
                }),
        );
        if !has_any && active.is_none() {
            section = section.child(note(text(locale, Key::MeasurementsEmpty), cx));
        }
        for (index, measurement) in review.measurements.iter().enumerate() {
            section = section.child(self.measurement_rows(index, measurement, false, locale, cx));
        }
        if let Some(active) = active {
            let shown = review
                .preview
                .map_or_else(|| active.clone(), |point| active.with_preview(point));
            section = section.child(self.measurement_rows(
                review.measurements.len(),
                &shown,
                true,
                locale,
                cx,
            ));
        }
        if review.clearance_pending {
            section = section.child(note(text(locale, Key::ClearancePending), cx));
        }
        if let Some(notice) = &review.clearance_notice {
            section = section.child(warning(notice, locale, cx));
        }
        if let Some(record) = review.clearance {
            let result = record.result;
            section = section
                .child(
                    div()
                        .px_4()
                        .pt_2()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(text(locale, Key::ReviewClearanceSection)),
                )
                .child(
                    div().px_4().text_sm().child(
                        Message::new(Key::ClearancePair)
                            .arg("first", object_label(record.first, locale))
                            .arg("second", object_label(record.second, locale))
                            .display(locale),
                    ),
                )
                .child(div().px_4().text_sm().child(if result.overlapping() {
                    text(locale, Key::ClearanceOverlap)
                } else {
                    Message::new(Key::ClearanceResult)
                        .arg("distance", format_length(result.distance_mm, unit, locale))
                        .arg(
                            "tolerance",
                            format_length(result.tolerance_mm, unit, locale),
                        )
                        .display(locale)
                }))
                .child(note(text(locale, Key::ClearanceScope), cx));
        }
        section
    }

    fn render_selection_set_section(&self, locale: Locale, cx: &mut Context<Self>) -> Div {
        let review = &self.review;
        let counts = review.set.counts();
        let total = review.set.len();
        let offset = review.set_offset;
        let mut section = div().flex().flex_col().child(
            heading(text(locale, Key::SelectionSetHeading), cx).child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        icon_button(
                            "set-add-current",
                            IconName::ListPlus,
                            text(locale, Key::SelectionSetAddCurrent),
                        )
                        .disabled(self.selected_target.is_none())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_current_selection_to_set(window, cx)
                        })),
                    )
                    .child(
                        icon_button(
                            "set-clear",
                            IconName::ListX,
                            text(locale, Key::SelectionSetClear),
                        )
                        .disabled(total == 0 && !review.set_pending)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_selection_set(cx))),
                    ),
            ),
        );
        if review.set_pending {
            section = section.child(note(text(locale, Key::SelectionSetPending), cx));
        }
        if let Some(notice) = &review.set_notice {
            section = section.child(warning(notice, locale, cx));
        }
        if total == 0 {
            return section.child(note(text(locale, Key::SelectionSetEmpty), cx));
        }
        section = section
            .child(
                div().px_4().text_sm().child(
                    Message::new(Key::SelectionSetCount)
                        .arg("count", total)
                        .display(locale),
                ),
            )
            .child(note(
                Message::new(Key::SelectionSetCounts)
                    .arg("segments", counts.segments)
                    .arg("pins", counts.pins)
                    .arg("vias", counts.vias)
                    .arg("zones", counts.zones)
                    .arg("drawings", counts.drawings)
                    .display(locale),
                cx,
            ));
        let page = review.set.page(offset, SET_PAGE);
        let end = offset + page.len();
        section = section.children(page.into_iter().map(|object| {
            div()
                .px_4()
                .flex()
                .items_center()
                .justify_between()
                .text_sm()
                .child(
                    Button::new(SharedString::from(format!("set-member-{object:?}")))
                        .ghost()
                        .xsmall()
                        .label(object_label(object, locale))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.locate_target(
                                pomelo_core::selection::SelectionTarget::Object(object),
                                window,
                                cx,
                            )
                        })),
                )
                .child(
                    icon_button(
                        SharedString::from(format!("set-remove-{object:?}")),
                        IconName::X,
                        text(locale, Key::SelectionSetRemove),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_from_set(object, cx))),
                )
        }));
        if total > SET_PAGE {
            section = section.child(
                div()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .child(
                        icon_button(
                            "set-previous-page",
                            IconName::ChevronLeft,
                            text(locale, Key::PreviousPage),
                        )
                        .disabled(offset == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.page_set(offset.saturating_sub(SET_PAGE), cx)
                        })),
                    )
                    .child(
                        Message::new(Key::ReviewPage)
                            .arg("start", offset + 1)
                            .arg("end", end)
                            .arg("total", total)
                            .display(locale),
                    )
                    .child(
                        icon_button(
                            "set-next-page",
                            IconName::ChevronRight,
                            text(locale, Key::NextPage),
                        )
                        .disabled(end >= total)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.page_set(offset + SET_PAGE, cx)),
                        ),
                    ),
            );
        }
        section
    }

    fn name_cell(&self, target: RenameTarget, name: &str) -> AnyElement {
        if self.review.rename == Some(target) {
            Input::new(&self.rename_input).xsmall().into_any_element()
        } else {
            div().truncate().child(name.to_owned()).into_any_element()
        }
    }

    pub(super) fn start_rename(
        &mut self,
        target: RenameTarget,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review.rename = Some(target);
        self.rename_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn render_groups_section(&self, locale: Locale, cx: &mut Context<Self>) -> Div {
        let review = &self.review;
        let mut section = div().flex().flex_col().child(
            heading(text(locale, Key::HighlightGroupsHeading), cx).child(
                icon_button(
                    "create-highlight-group",
                    IconName::Highlighter,
                    text(locale, Key::HighlightCreate),
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.create_highlight_group(window, cx)),
                ),
            ),
        );
        if let Some(notice) = &review.notice {
            section = section.child(warning(notice, locale, cx));
        }
        if review.state.highlight_groups.is_empty() {
            return section.child(note(text(locale, Key::HighlightGroupsEmpty), cx));
        }
        for group in &review.state.highlight_groups {
            let id = group.id;
            let visible = group.visible;
            let name = group.name.clone();
            let [r, g, b] = group.color.0;
            let missing = review.missing.get(&id).copied().unwrap_or(0);
            let mut details = vec![
                Message::new(Key::HighlightMembers)
                    .arg("count", group.members.len())
                    .display(locale),
            ];
            if missing > 0 {
                details.push(
                    Message::new(Key::HighlightMissing)
                        .arg("count", missing)
                        .display(locale),
                );
            }
            if !visible {
                details.push(text(locale, Key::HighlightHidden));
            }
            section = section.child(
                div()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(
                        Button::new(("group-color", id))
                            .ghost()
                            .xsmall()
                            .accessibility_label(text(locale, Key::HighlightNextColor))
                            .native_tooltip(text(locale, Key::HighlightNextColor))
                            .child(
                                div()
                                    .size_3()
                                    .rounded_sm()
                                    .bg(gpui_kit::rgb(
                                        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
                                    ))
                                    .when(!visible, |swatch| swatch.opacity(0.35)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                use pomelo_core::review::highlight::PALETTE;
                                this.edit_group(
                                    id,
                                    |group| {
                                        let next = PALETTE
                                            .iter()
                                            .position(|color| *color == group.color)
                                            .map_or(0, |index| (index + 1) % PALETTE.len());
                                        group.color = PALETTE[next];
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(self.name_cell(RenameTarget::Group(id), &name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if missing > 0 {
                                        cx.theme().warning
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(details.join(" · ")),
                            ),
                    )
                    .child(
                        icon_button(
                            ("group-visibility", id),
                            if visible {
                                IconName::Eye
                            } else {
                                IconName::EyeOff
                            },
                            text(
                                locale,
                                if visible {
                                    Key::HighlightHide
                                } else {
                                    Key::HighlightShow
                                },
                            ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_group(id, |group| group.visible = !group.visible, cx)
                        })),
                    )
                    .child(
                        icon_button(
                            ("group-select", id),
                            IconName::ListPlus,
                            text(locale, Key::HighlightSelect),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.select_group_members(id, cx)),
                        ),
                    )
                    .child(
                        icon_button(
                            ("group-rename", id),
                            IconName::Pencil,
                            text(locale, Key::RenameLabel),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.start_rename(RenameTarget::Group(id), name.clone(), window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(
                            ("group-delete", id),
                            IconName::Trash,
                            text(locale, Key::HighlightDelete),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_group(id, cx))),
                    ),
            );
        }
        section
    }

    fn render_views_section(&self, locale: Locale, cx: &mut Context<Self>) -> Div {
        let review = &self.review;
        let mut section = div().flex().flex_col().child(
            heading(text(locale, Key::NamedViewsHeading), cx).child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        icon_button(
                            "navigate-back",
                            IconName::Undo2,
                            Message::new(Key::WithShortcut)
                                .arg("label", text(locale, Key::NavigateBack))
                                .arg(
                                    "shortcut",
                                    crate::shortcuts::shortcut_label(
                                        crate::shortcuts::keys::NAVIGATE_BACK,
                                    ),
                                )
                                .display(locale),
                        )
                        .disabled(!review.history.can_go_back())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.navigate_back(&NavigateBack, window, cx)
                        })),
                    )
                    .child(
                        icon_button(
                            "navigate-forward",
                            IconName::Redo2,
                            Message::new(Key::WithShortcut)
                                .arg("label", text(locale, Key::NavigateForward))
                                .arg(
                                    "shortcut",
                                    crate::shortcuts::shortcut_label(
                                        crate::shortcuts::keys::NAVIGATE_FORWARD,
                                    ),
                                )
                                .display(locale),
                        )
                        .disabled(!review.history.can_go_forward())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.navigate_forward(&NavigateForward, window, cx)
                        })),
                    )
                    .child(
                        icon_button(
                            "save-named-view",
                            IconName::BookmarkPlus,
                            text(locale, Key::NamedViewSave),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.save_named_view(cx))),
                    ),
            ),
        );
        if review.state.named_views.is_empty() {
            return section.child(note(text(locale, Key::NamedViewsEmpty), cx));
        }
        section = section.child(note(text(locale, Key::NamedViewScope), cx));
        for view in &review.state.named_views {
            let id = view.id;
            let name = view.name.clone();
            section = section.child(
                div()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.name_cell(RenameTarget::View(id), &name)),
                    )
                    .child(
                        icon_button(
                            ("view-restore", id),
                            IconName::Locate,
                            text(locale, Key::NamedViewRestore),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.restore_named_view(id, window, cx),
                        )),
                    )
                    .child(
                        icon_button(
                            ("view-update", id),
                            IconName::RotateCcw,
                            text(locale, Key::NamedViewUpdate),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.update_named_view(id, cx)),
                        ),
                    )
                    .child(
                        icon_button(
                            ("view-rename", id),
                            IconName::Pencil,
                            text(locale, Key::RenameLabel),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.start_rename(RenameTarget::View(id), name.clone(), window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(
                            ("view-delete", id),
                            IconName::Trash,
                            text(locale, Key::NamedViewDelete),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.delete_named_view(id, cx)),
                        ),
                    ),
            );
        }
        section
    }

    fn render_export_section(
        &self,
        locale: Locale,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let export = &self.review.export;
        let frame = self.image_frame(window.scale_factor());
        let mut section = div()
            .flex()
            .flex_col()
            .child(heading(text(locale, Key::ExportHeading), cx))
            .child(
                div().px_4().pb_2().flex().gap_1().children(
                    [
                        (false, Key::ExportScopeView, 0u32),
                        (true, Key::ExportScopeBoard, 1),
                    ]
                    .map(|(board, key, id)| {
                        Button::new(("export-scope", id))
                            .ghost()
                            .xsmall()
                            .label(text(locale, key))
                            .selected(export.scope_board == board)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.review.export.scope_board = board;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div().px_4().pb_1().child(
                    Checkbox::new("export-markers")
                        .label(text(locale, Key::ExportMarkers))
                        .checked(export.markers)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.review.export.markers = *checked;
                            cx.notify();
                        })),
                ),
            )
            .child(
                div().px_4().pb_2().child(
                    Checkbox::new("export-anonymize")
                        .label(text(locale, Key::ExportAnonymize))
                        .checked(export.anonymize)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.review.export.anonymize = *checked;
                            cx.notify();
                        })),
                ),
            );
        if let Some(frame) = frame {
            section = section.child(note(
                Message::new(Key::ExportImageContents)
                    .arg("width", frame.width)
                    .arg("height", frame.height)
                    .display(locale),
                cx,
            ));
        }
        section = section
            .child(
                div()
                    .px_4()
                    .pb_2()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("export-png")
                            .outline()
                            .xsmall()
                            .icon(IconName::Camera)
                            .label(text(locale, Key::ExportPng))
                            .disabled(export.pending || frame.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.export_image(ImageFormat::Png, window, cx)
                            })),
                    )
                    .child(
                        Button::new("export-svg")
                            .outline()
                            .xsmall()
                            .icon(IconName::Image)
                            .label(text(locale, Key::ExportSvg))
                            // The 3D view exports as a raster image only.
                            .disabled(export.pending || frame.is_none() || self.view3d.active)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.export_image(ImageFormat::Svg, window, cx)
                            })),
                    ),
            )
            .child(note(
                Message::new(Key::ExportCsvContents)
                    .arg("measurements", self.review.measurements.len())
                    .arg("objects", self.review.set.len())
                    .display(locale),
                cx,
            ))
            .child(
                div().px_4().pb_2().child(
                    Button::new("export-csv")
                        .outline()
                        .xsmall()
                        .icon(IconName::FileDown)
                        .label(text(locale, Key::ExportCsv))
                        .disabled(export.pending)
                        .on_click(cx.listener(|this, _, window, cx| this.export_csv(window, cx))),
                ),
            );
        if export.pending {
            section = section.child(note(text(locale, Key::ExportPending), cx));
        }
        if let Some((message, failed)) = &export.notice {
            section = if *failed {
                section.child(warning(message, locale, cx))
            } else {
                section.child(note(message.display(locale), cx))
            };
        }
        section
    }

    pub(super) fn render_review_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let locale = i18n::current(cx);
        div()
            .flex()
            .flex_col()
            .pb_4()
            .child(self.render_measurements_section(locale, cx))
            .child(self.render_selection_set_section(locale, cx))
            .child(self.render_groups_section(locale, cx))
            .child(self.render_views_section(locale, cx))
            .child(self.render_export_section(locale, window, cx))
            .into_any_element()
    }
}
