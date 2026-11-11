//! Compose viewport panels, canvas, toolbar and status overlays.
use super::*;
use crate::services::preferences::{LEFT_PANEL_WIDTH, RIGHT_PANEL_WIDTH};
use gpui_kit::component::{
    status_bar::StatusBar,
    tab::{Tab, TabBar},
};
impl Render for BoardViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = i18n::current(cx);
        self.ensure_net_report(cx);
        self.ensure_length_table(cx);
        let length_table = self.render_length_table(locale, cx);
        let panel_budget = window.viewport_size().width.as_f32()
            - crate::workbench::panel_layout::MIN_CANVAS_WIDTH
            - crate::workbench::panel_layout::PANEL_FRAME_WIDTH;
        // Collapsed panels leave no rail; the toolbar keeps their toggles reachable.
        let rail_width = 0.0;
        let panels = crate::workbench::panel_layout::fit_to_window(
            self.panel_layout.read(cx).snapshot(cx),
            window.viewport_size().width.as_f32(),
            rail_width,
        );
        let left_panel_width = px(panels.left_width);
        let right_panel_width = px(panels.right_width);
        let left_max_width = px((panel_budget
            - if panels.right_collapsed {
                rail_width
            } else {
                panels.right_width
            })
        .clamp(*LEFT_PANEL_WIDTH.start(), *LEFT_PANEL_WIDTH.end()));
        let right_max_width = px((panel_budget
            - if panels.left_collapsed {
                rail_width
            } else {
                panels.left_width
            })
        .clamp(*RIGHT_PANEL_WIDTH.start(), *RIGHT_PANEL_WIDTH.end()));
        let canvas_breadcrumb = self
            .selected_source_labels
            .iter()
            .find_map(|message| message.args.get("name"))
            .map_or_else(
                || text(locale, Key::FullBoard),
                |name| {
                    Message::new(Key::BoardSelection)
                        .arg("name", name.to_string())
                        .display(locale)
                },
            );
        let (layer_panel, layer_order) = self.render_layer_panel(left_panel_width, window, cx);
        let inspector = self.render_inspector_panel(right_panel_width, window, cx);
        let mut pointer_coordinates =
            self.pointer_position
                .zip(self.bounds)
                .and_then(|(position, bounds)| {
                    let local = position - bounds.origin;
                    let size = self.navigation.size();
                    let point =
                        pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y));
                    if point.x < 0.0
                        || point.y < 0.0
                        || point.x > size.x
                        || point.y > size.y
                        || !self.navigation.camera().is_renderable()
                    {
                        return None;
                    }
                    let board = self
                        .navigation
                        .camera()
                        .view_to_board(point, size.x, size.y);
                    (board.x.is_finite() && board.y.is_finite()).then(|| {
                        crate::panels::inspector::display_source_field(
                            &Message::new(Key::SourcePosition)
                                .arg("x", board.x.to_string())
                                .arg("y", board.y.to_string()),
                            locale,
                            self.display.length_unit,
                        )
                    })
                });
        if self.view3d.active {
            pointer_coordinates = None;
        }
        let scale_bar = self
            .navigation
            .camera()
            .scale_bar_in_unit(
                (self.navigation.size().x - 32.0).min(120.0),
                self.display.length_unit,
            )
            .filter(|_| !self.view3d.active);
        let frame::CanvasPresentation {
            viewport,
            ready,
            geometry_ready,
            failure,
            uploaded_bytes,
            total_bytes,
        } = self.render_canvas(layer_order, panels, window, cx);
        let grid = self.render_grid();
        let review_overlay = self.render_review_overlay(locale, cx);
        let canvas_view_controls = self.render_canvas_toolbar(locale, cx);
        let view_cube = self
            .view3d
            .active
            .then(|| self.render_view_cube(locale, cx));
        let status_bar = self.render_status_bar(ready, pointer_coordinates, locale, cx);
        let theme = cx.theme();
        let mut view = div()
            .id("board-viewport")
            .key_context("BoardWorkspace")
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::leave_search))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::next_candidate))
            .on_action(cx.listener(Self::fit_board))
            .on_action(cx.listener(Self::flip_board))
            .on_action(cx.listener(|this, _: &ToggleGrid, _, cx| {
                let display = Arc::make_mut(&mut this.display);
                display.grid.visible = !display.grid.visible;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom(1.2, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom(1.0 / 1.2, cx)))
            .on_action(cx.listener(|this, _: &PanLeft, _, cx| this.pan(1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanRight, _, cx| this.pan(-1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanUp, _, cx| this.pan(0.0, 1.0, cx)))
            .on_action(cx.listener(|this, _: &PanDown, _, cx| this.pan(0.0, -1.0, cx)))
            .on_action(cx.listener(Self::cancel_canvas_action))
            .on_action(cx.listener(Self::finish_measurement))
            .on_action(cx.listener(Self::remove_last_point))
            .on_action(cx.listener(Self::navigate_back))
            .on_action(cx.listener(Self::navigate_forward))
            .on_action(
                cx.listener(|this, _: &NextActiveLayer, _, cx| this.step_active_layer(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousActiveLayer, _, cx| {
                    this.step_active_layer(false, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSingleLayer, _, cx| {
                if !this.view3d.active {
                    this.toggle_single_layer(cx)
                }
            }))
            .on_action(cx.listener(|this, _: &MeasureDistance, window, cx| {
                this.set_review_tool(
                    Some(review::ReviewTool::Measure(
                        pomelo_core::review::measure::MeasureKind::Distance,
                    )),
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &MeasurePolyline, window, cx| {
                this.set_review_tool(
                    Some(review::ReviewTool::Measure(
                        pomelo_core::review::measure::MeasureKind::Polyline,
                    )),
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &MeasureClearance, window, cx| {
                this.set_review_tool(Some(review::ReviewTool::Clearance), window, cx)
            }))
            .on_action(cx.listener(|this, _: &MeasurePinPath, window, cx| {
                this.set_review_tool(Some(review::ReviewTool::PinPath), window, cx)
            }))
            .on_action(cx.listener(|this, _: &BoxSelect, window, cx| {
                this.set_review_tool(Some(review::ReviewTool::BoxSelect), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomToArea, window, cx| {
                this.set_review_tool(Some(review::ReviewTool::ZoomBox), window, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar::render(
                toolbar::State {
                    view3d: self.view3d.active,
                    locale,
                    pan_tool: self.pan_tool,
                    color_mode: self.display.color_mode,
                    selection_mode: self.selection_mode,
                    pick_filter: self.pick_filter,
                    left_panel_open: !panels.left_collapsed,
                    right_panel_open: !panels.right_collapsed,
                    search: &self.search_input,
                },
                toolbar::Commands {
                    view3d: Box::new(cx.listener(|this, active: &bool, window, cx| {
                        if this.view3d.active != *active {
                            this.toggle_3d(window, cx);
                        }
                    })),
                    tool: Box::new(cx.listener(|this, pan: &bool, window, cx| {
                        this.pan_tool = *pan;
                        this.review.tool = None;
                        this.review.active = None;
                        this.review.drag = None;
                        this.pointer_gesture = None;
                        if *pan {
                            this.invalidate_hover();
                        }
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
                    colors: Box::new(cx.listener(
                        |this, mode: &pomelo_core::display::ColorMode, _, cx| {
                            Arc::make_mut(&mut this.display).color_mode = *mode;
                            this.invalidate_hover();
                            cx.notify();
                        },
                    )),
                    fit: Box::new(cx.listener(|this, _, window, cx| {
                        this.fit_board(&FitBoard, window, cx);
                    })),
                    pick_mode: Box::new(cx.listener(
                        |this, mode: &pomelo_core::interaction::SelectionMode, window, cx| {
                            this.set_selection_mode(*mode, window, cx)
                        },
                    )),
                    pick_category: Box::new(cx.listener(
                        |this,
                         (category, enabled): &(pomelo_core::picking::PickCategory, bool),
                         _,
                         cx| {
                            this.set_pick_category(*category, *enabled, cx)
                        },
                    )),
                },
                cx,
            ))
            .child(
                div().flex().flex_1().min_h_0().min_w_0().child(
                    h_resizable("board-panels")
                        .with_state(&self.panel_sizes)
                        .on_resize(cx.listener(|_, _, _, cx| cx.notify()))
                        .when(!panels.left_collapsed, |group| {
                            group.child(
                                resizable_panel()
                                    .size(left_panel_width)
                                    .size_range(px(*LEFT_PANEL_WIDTH.start())..left_max_width)
                                    .flex_none()
                                    .child(layer_panel),
                            )
                        })
                        .child(
                            resizable_panel()
                                .size_range(
                                    px(crate::workbench::panel_layout::MIN_CANVAS_WIDTH)
                                        ..Pixels::MAX,
                                )
                                .child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .flex_col()
                                        .min_h_0()
                                        .child(
                                            div()
                                                .id("board-canvas")
                                                .relative()
                                                .key_context("BoardViewport")
                                                .track_focus(&self.focus)
                                                .flex_1()
                                                .min_h_0()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .border_1()
                                                .border_color(self.canvas_background())
                                                .bg(self.canvas_background())
                                                .focus_visible(|style| {
                                                    style.border_color(theme.ring)
                                                })
                                                .cursor(
                                                    if self
                                                        .pointer_gesture
                                                        .as_ref()
                                                        .is_some_and(|gesture| gesture.dragging)
                                                    {
                                                        CursorStyle::ClosedHand
                                                    } else if self.pan_tool {
                                                        CursorStyle::OpenHand
                                                    } else if self.review.tool.is_some() {
                                                        CursorStyle::Crosshair
                                                    } else {
                                                        CursorStyle::Arrow
                                                    },
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(Self::start_pan),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Middle,
                                                    cx.listener(Self::start_pan),
                                                )
                                                // Right-drag pans the 3D view only.
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    cx.listener(|this, event, window, cx| {
                                                        if this.view3d.active {
                                                            this.start_3d_drag(event, window, cx);
                                                        }
                                                    }),
                                                )
                                                .on_mouse_up(
                                                    MouseButton::Right,
                                                    cx.listener(|this, event, window, cx| {
                                                        if this.view3d.active {
                                                            this.stop_3d_drag(event, window, cx);
                                                        }
                                                    }),
                                                )
                                                .on_mouse_move(cx.listener(Self::move_pan))
                                                .on_hover(cx.listener(
                                                    |this, hovered: &bool, _, cx| {
                                                        if !*hovered {
                                                            this.pointer_position = None;
                                                            this.hover_cancel.cancel();
                                                            this.hovered = None;
                                                            this.hover_notice = None;
                                                            cx.notify();
                                                        }
                                                    },
                                                ))
                                                .on_mouse_up(
                                                    MouseButton::Left,
                                                    cx.listener(Self::stop_pan),
                                                )
                                                .on_mouse_up_out(
                                                    MouseButton::Left,
                                                    cx.listener(Self::cancel_pan),
                                                )
                                                .on_mouse_up(
                                                    MouseButton::Middle,
                                                    cx.listener(Self::stop_pan),
                                                )
                                                .on_mouse_up_out(
                                                    MouseButton::Middle,
                                                    cx.listener(Self::cancel_pan),
                                                )
                                                .on_scroll_wheel(cx.listener(Self::scroll))
                                                .children(grid)
                                                .child(viewport)
                                                .children(review_overlay)
                                                // Initial uploads must not resize the canvas after its first fit.
                                                .when(
                                                    !geometry_ready && failure.is_none(),
                                                    |canvas| {
                                                        canvas.child(
                                                            div()
                                                                .absolute()
                                                                .top_10()
                                                                .left_4()
                                                                .max_w(relative(0.85))
                                                                .truncate()
                                                                .bg(theme.popover)
                                                                .text_color(
                                                                    theme.popover_foreground,
                                                                )
                                                                .rounded_sm()
                                                                .px_4()
                                                                .py_1()
                                                                .text_xs()
                                                                .child(
                                                                    Message::new(Key::TraceUpload)
                                                                        .arg(
                                                                            "completed",
                                                                            uploaded_bytes,
                                                                        )
                                                                        .arg("total", total_bytes)
                                                                        .display(locale),
                                                                ),
                                                        )
                                                    },
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .top_3()
                                                        .left_4()
                                                        .max_w(relative(0.6))
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .text_sm()
                                                        .text_color(crate::theme::canvas_colors().1)
                                                        .child(
                                                            Icon::new(IconName::Layers)
                                                                .small()
                                                                .flex_shrink_0(),
                                                        )
                                                        .child(
                                                            div()
                                                                .min_w_0()
                                                                .truncate()
                                                                .child(canvas_breadcrumb),
                                                        ),
                                                )
                                                .when(!self.view3d.active, |canvas| {
                                                    canvas.child(axes_indicator(locale))
                                                })
                                                .when_some(view_cube, |canvas, cube| {
                                                    canvas.child(
                                                        div()
                                                            .absolute()
                                                            .top_3()
                                                            .right_4()
                                                            .child(cube),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .bottom_4()
                                                        .left_0()
                                                        .right_0()
                                                        .flex()
                                                        .justify_center()
                                                        .child(
                                                            canvas_bar(canvas_view_controls)
                                                                .flex_wrap()
                                                                .max_w_full(),
                                                        ),
                                                )
                                                .when_some(scale_bar, |canvas, scale| {
                                                    canvas.child(
                                                div()
                                                    .absolute()
                                                    .right_4()
                                                    .bottom_4()
                                                    .when(
                                                        self.navigation.size().x < 640.0,
                                                        |label| label.bottom_16(),
                                                    )
                                                    .flex()
                                                    .flex_col()
                                                    .items_end()
                                                    .text_xs()
                                                    .text_color(crate::theme::canvas_colors().1)
                                                    .child(
                                                        scale_label::scale_message(
                                                            scale,
                                                            self.display.length_unit,
                                                        )
                                                        .display(locale),
                                                    )
                                                    .child(
                                                        div()
                                                            // This width represents board geometry projected
                                                            // into logical screen pixels, not UI spacing.
                                                            .w(px(scale.pixels as f32))
                                                            .h_1p5()
                                                            .mt_1()
                                                            .border_b_1()
                                                            .border_l_1()
                                                            .border_r_1()
                                                            .border_color(
                                                                crate::theme::canvas_colors().1,
                                                            ),
                                                    ),
                                            )
                                                }),
                                        )
                                        .children(length_table),
                                ),
                        )
                        .when(!panels.right_collapsed, |group| {
                            group.child(
                                resizable_panel()
                                    .size(right_panel_width)
                                    .size_range(px(*RIGHT_PANEL_WIDTH.start())..right_max_width)
                                    .flex_none()
                                    .child(inspector),
                            )
                        }),
                ),
            )
            .child(status_bar);
        let hover_text: SharedString = if let Some((object, context)) = &self.hovered
            && self.hover_tooltip_ready
            && context.matches_view(
                self.navigation.camera(),
                self.navigation.size(),
                &self.display,
                self.selection_mode,
            ) {
            let (kind, id) =
                selection_identity(pomelo_core::selection::SelectionTarget::Object(*object));
            Message::new(Key::HoverObject)
                .arg("kind", text(locale, kind))
                .arg("id", id)
                .display(locale)
                .into()
        } else if let Some(message) = &self.hover_notice
            && self.hover_tooltip_ready
        {
            message.display(locale).into()
        } else {
            SharedString::default()
        };
        if !hover_text.is_empty() {
            view = view.child(
                div()
                    .absolute()
                    .bottom_24()
                    .left_72()
                    .rounded_md()
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .px_3()
                    .py_2()
                    .text_sm()
                    .child(hover_text),
            );
        }
        if let Some(details) = failure {
            // Error details must not resize the canvas and invalidate its curve view.
            view = view.child(
                div()
                    .absolute()
                    .bottom_12()
                    .left_4()
                    .max_w(relative(0.85))
                    .rounded_sm()
                    .bg(theme.popover)
                    .px_3()
                    .py_2()
                    .text_color(theme.danger)
                    .child(div().child(text(
                        locale,
                        if self.view3d.active {
                            Key::Preview3dFailed
                        } else {
                            Key::GpuFailed
                        },
                    )))
                    .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                    .child(div().text_sm().child(details)),
            );
        }
        view
    }
}

impl BoardViewport {
    fn render_status_bar(
        &self,
        ready: bool,
        pointer_coordinates: Option<String>,
        locale: pomelo_core::i18n::Locale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let units = [
            (
                pomelo_core::units::LengthUnit::Millimeters,
                Key::UnitMillimeters,
            ),
            (pomelo_core::units::LengthUnit::Mils, Key::UnitMils),
        ];
        let selected_unit = units
            .iter()
            .position(|(unit, _)| *unit == self.display.length_unit)
            .unwrap_or(0);
        StatusBar::new()
            .h_7()
            .px_3()
            .gap_4()
            .left(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(theme.foreground)
                    .child(div().size_1p5().rounded_full().bg(if ready {
                        theme.success
                    } else {
                        theme.warning
                    }))
                    .child(text(
                        locale,
                        if ready { Key::Ready } else { Key::PreparingGpu },
                    )),
            )
            .left(
                Message::new(Key::WorkspaceSummary)
                    .arg("layers", self.scene.layers.len())
                    .arg("components", self.scene.components.len())
                    .arg("nets", self.scene.nets.len())
                    .display(locale),
            )
            .when_some(pointer_coordinates, |bar, coordinates| {
                bar.child(
                    div()
                        .truncate()
                        .font_family(theme.mono_font_family.clone())
                        .child(coordinates),
                )
            })
            .when(self.view3d.active, |bar| {
                let mode = super::view3d::controls::RENDER_MODES
                    .iter()
                    .find(|(mode, _, _)| *mode == self.view3d.settings.mode)
                    .map_or(Key::RenderModeDefault, |(_, key, _)| *key);
                bar.child(
                    Message::new(Key::StatusView3d)
                        .arg("mode", text(locale, mode))
                        .arg(
                            "scale",
                            super::view3d::controls::format_scale(self.view3d.settings.z_scale),
                        )
                        .display(locale),
                )
            })
            .when_some(
                self.display
                    .active_layer
                    .filter(|_| !self.view3d.active)
                    .and_then(|id| self.scene.layers.iter().find(|layer| layer.id == id)),
                |bar, layer| {
                    bar.child(
                        Message::new(Key::StatusActiveLayer)
                            .arg("layer", layer.display_name(locale))
                            .display(locale),
                    )
                },
            )
            .right(
                TabBar::new("length-unit")
                    .segmented()
                    .xsmall()
                    .selected_index(selected_unit)
                    .children(
                        units
                            .iter()
                            .map(|(_, key)| Tab::new().label(text(locale, *key))),
                    )
                    .on_click(cx.listener(move |this, index: &usize, _, cx| {
                        if let Some((unit, _)) = units.get(*index) {
                            Arc::make_mut(&mut this.display).length_unit = *unit;
                            cx.notify();
                        }
                    })),
            )
            .right(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(Icon::new(IconName::Cpu).xsmall())
                    .child(text(locale, Key::GpuStatus)),
            )
            .into_any_element()
    }
}

/// Command surface floating over the board. Its pointer events must not also
/// pick, pan or zoom the geometry underneath.
pub(super) fn canvas_bar(children: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_1()
        .border_1()
        .border_color(crate::theme::canvas_colors().2)
        .rounded_lg()
        .bg(crate::theme::canvas_toolbar_surface())
        .shadow_md()
        .text_color(crate::theme::canvas_colors().1)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .children(children)
}

pub(super) fn canvas_separator() -> AnyElement {
    div()
        // A one-pixel divider is a hairline, not spacing.
        .w(px(1.0))
        .h_5()
        .mx_1()
        .bg(crate::theme::canvas_colors().2)
        .into_any_element()
}

/// Board axis orientation in the lower-left corner of the canvas.
fn axes_indicator(locale: pomelo_core::i18n::Locale) -> impl IntoElement {
    let (x_axis, y_axis) = crate::theme::canvas_axes();
    div()
        .absolute()
        .bottom_4()
        .left_4()
        .size_12()
        .text_xs()
        .text_color(crate::theme::canvas_colors().1)
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(text(locale, Key::AxisY)),
        )
        // Axis strokes are two-pixel hairlines drawn over board content.
        .child(
            div()
                .absolute()
                .left_1()
                .top_4()
                .bottom_2()
                .w(px(2.0))
                .bg(y_axis),
        )
        .child(
            div()
                .absolute()
                .left_1()
                .right_3()
                .bottom_2()
                .h(px(2.0))
                .bg(x_axis),
        )
        .child(
            div()
                .absolute()
                .right_0()
                .bottom_0()
                .child(text(locale, Key::AxisX)),
        )
}
