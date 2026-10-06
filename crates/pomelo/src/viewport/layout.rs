//! Compose viewport panels, canvas, toolbar and status overlays.
use super::*;
impl Render for BoardViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::workbench::panel_layout::Side;
        let locale = i18n::current(cx);
        let panel_budget = window.viewport_size().width.as_f32()
            - crate::workbench::panel_layout::MIN_CANVAS_WIDTH
            - crate::workbench::panel_layout::PANEL_FRAME_WIDTH;
        let rail_width = (window.rem_size() * 2.5).as_f32();
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
        .clamp(200.0, 440.0));
        let right_max_width = px((panel_budget
            - if panels.left_collapsed {
                rail_width
            } else {
                panels.left_width
            })
        .clamp(240.0, 440.0));
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
        let pointer_coordinates =
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
        let scale_bar = self.navigation.camera().scale_bar_in_unit(
            (self.navigation.size().x - 32.0).min(120.0),
            self.display.length_unit,
        );
        let frame::CanvasPresentation {
            viewport,
            ready,
            geometry_ready,
            failure,
            uploaded_bytes,
            total_bytes,
        } = self.render_canvas(layer_order, panels, window, cx);
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
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom(1.2, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom(1.0 / 1.2, cx)))
            .on_action(cx.listener(|this, _: &PanLeft, _, cx| this.pan(1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanRight, _, cx| this.pan(-1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanUp, _, cx| this.pan(0.0, 1.0, cx)))
            .on_action(cx.listener(|this, _: &PanDown, _, cx| this.pan(0.0, -1.0, cx)))
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar::render(
                toolbar::State {
                    locale,
                    pan_tool: self.pan_tool,
                    color_mode: self.display.color_mode,
                    search: &self.search_input,
                },
                toolbar::Commands {
                    select: Box::new(cx.listener(|this, _, window, cx| {
                        this.pan_tool = false;
                        this.pointer_gesture = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
                    pan: Box::new(cx.listener(|this, _, window, cx| {
                        this.pan_tool = true;
                        this.pointer_gesture = None;
                        this.invalidate_hover();
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
                },
                cx,
            ))
            .child(
                div().flex().flex_1().min_h_0().min_w_0()
                .when(panels.left_collapsed, |row| row.child(crate::panels::sidebar::rail(Side::Left,
                    crate::panels::sidebar::toggle_button(Side::Left, true, locale,
                        |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleLeftPanel), cx)), cx)))
                .child(
                    h_resizable("board-panels")
                        .with_state(&self.panel_sizes)
                        .on_resize(cx.listener(|_, _, _, cx| cx.notify()))
                        .when(!panels.left_collapsed, |group| group.child(
                            resizable_panel()
                                .size(left_panel_width)
                                .size_range(px(200.0)..left_max_width)
                                .flex_none()
                                .child(layer_panel),
                        ))
                        .child(
                            resizable_panel().size_range(px(crate::workbench::panel_layout::MIN_CANVAS_WIDTH)..Pixels::MAX).child(
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
                                    .border_color(theme.border)
                                    .bg(self.canvas_background())
                                    .focus_visible(|style| style.border_color(theme.ring))
                                    .cursor(if self.pointer_gesture.as_ref().is_some_and(|gesture| gesture.dragging) {
                                        CursorStyle::ClosedHand
                                    } else if self.pan_tool {
                                        CursorStyle::OpenHand
                                    } else {
                                        CursorStyle::Arrow
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::start_pan),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Middle,
                                        cx.listener(Self::start_pan),
                                    )
                                    .on_mouse_move(cx.listener(Self::move_pan))
                                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                        if !*hovered {
                                            this.pointer_position = None;
                                            this.hover_cancel.cancel();
                                            this.hovered = None;
                                            this.hover_notice = None;
                                            cx.notify();
                                        }
                                    }))
                                    .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_pan))
                                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::cancel_pan))
                                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::stop_pan))
                                    .on_mouse_up_out(
                                        MouseButton::Middle,
                                        cx.listener(Self::cancel_pan),
                                    )
                                    .on_scroll_wheel(cx.listener(Self::scroll))
                                    .child(viewport)
                                    // Initial uploads must not resize the canvas after its first fit.
                                    .when(!geometry_ready && failure.is_none(), |canvas| {
                                        canvas.child(
                                            div()
                                                .absolute()
                                                .top_10()
                                                .left_4()
                                                .max_w(relative(0.85))
                                                .truncate()
                                                .bg(theme.popover)
                                                .text_color(theme.popover_foreground)
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
                                                        .arg(
                                                            "total",
                                                            total_bytes,
                                                        )
                                                        .display(locale),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .absolute()
                                            .top_3()
                                            .left_4()
                                            .max_w(relative(0.85))
                                            .truncate()
                                            .text_sm()
                                            .text_color(crate::theme::canvas_colors().1)
                                            .child(canvas_breadcrumb),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_4()
                                            .left_4()
                                            .w(rems(3.5))
                                            .h_16()
                                            .text_sm()
                                            .text_color(crate::theme::canvas_colors().1)
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .child(text(locale, Key::AxisY)),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left_1()
                                                    .top_6()
                                                    .w(px(2.0))
                                                    .h_8()
                                                    .bg(crate::theme::canvas_axes().1),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left_1()
                                                    .bottom_2()
                                                    .w_8()
                                                    .h(px(2.0))
                                                    .bg(crate::theme::canvas_axes().0),
                                            )
                                            .child(
                                                div().absolute().left(px(-3.0)).top_4().child(
                                                    Icon::new(IconName::ArrowUp)
                                                        .size_4()
                                                        .text_color(crate::theme::canvas_axes().1),
                                                ),
                                            )
                                            .child(
                                                div().absolute().left_6().bottom_0().child(
                                                    Icon::new(IconName::ArrowRight)
                                                        .size_4()
                                                        .text_color(crate::theme::canvas_axes().0),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .right_0()
                                                    .bottom_0()
                                                    .child(text(locale, Key::AxisX)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_4()
                                            .left_0()
                                            .right_0()
                                            .flex()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .p_1()
                                                    .border_1()
                                                    .border_color(crate::theme::canvas_colors().2)
                                                    .rounded_md()
                                                    .bg(crate::theme::canvas_toolbar_surface())
                                                    .text_color(crate::theme::canvas_colors().1)
                                                    // Commands overlay the board. Their pointer
                                                    // events must not also pick/pan the geometry.
                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                        cx.stop_propagation();
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Middle,
                                                        |_, _, cx| cx.stop_propagation(),
                                                    )
                                                    .on_scroll_wheel(|_, _, cx| {
                                                        cx.stop_propagation();
                                                    })
                                                    .child(
                                                        Button::new("canvas-select")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::MousePointer2)
                                                            .selected(!self.pan_tool)
                                                            .toggled(!self.pan_tool)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::SelectTool,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::SelectTool,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.pan_tool = false;
                                                                    this.pointer_gesture = None;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("canvas-pan")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Hand)
                                                            .selected(self.pan_tool)
                                                            .toggled(self.pan_tool)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::PanTool,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::PanTool,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.pan_tool = true;
                                                                    this.pointer_gesture = None;
                                                                    this.invalidate_hover();
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(1.0))
                                                            .h_6()
                                                            .mx_1()
                                                            .bg(crate::theme::canvas_colors().2),
                                                    )
                                                    .child(
                                                        Button::new("zoom-out")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Minus)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::ZoomOut,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::ZoomOut,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.zoom(1.0 / 1.2, cx)
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div().px_2().text_sm().child(
                                                            Message::new(Key::ZoomPercent)
                                                                .arg(
                                                                    "percent",
                                                                    (self.navigation.zoom_percent())
                                                                        .round()
                                                                        as u64,
                                                                )
                                                                .display(locale),
                                                        ),
                                                    )
                                                    .child(
                                                        Button::new("zoom-in")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Plus)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::ZoomIn,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::ZoomIn,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| this.zoom(1.2, cx),
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(1.0))
                                                            .h_6()
                                                            .mx_1()
                                                            .bg(crate::theme::canvas_colors().2),
                                                    )
                                                    .child(
                                                        Button::new("canvas-fit")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Scan)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::FitBoard,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::FitBoard,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.fit_board(
                                                                        &FitBoard, window, cx,
                                                                    )
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("flip-board")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::RotateCcw)
                                                            .selected(
                                                                self.navigation.camera().flipped,
                                                            )
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::FlipBoard,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::FlipBoard,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.flip_board(
                                                                        &FlipBoard, window, cx,
                                                                    )
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    )
                                    .when_some(scale_bar, |canvas, scale| {
                                        canvas.child(
                                            div()
                                                .absolute()
                                                .right_4()
                                                .bottom_4()
                                                .when(self.navigation.size().x < 640.0, |label| {
                                                    label.bottom_16()
                                                })
                                                .text_sm()
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
                                                        .h_2()
                                                        .border_b_1()
                                                        .border_l_1()
                                                        .border_r_1()
                                                        .border_color(
                                                            crate::theme::canvas_colors().1,
                                                        ),
                                                ),
                                        )
                                    }),
                            ),
                        )
                        .when(!panels.right_collapsed, |group| group.child(
                            resizable_panel()
                                .size(right_panel_width)
                                .size_range(px(240.0)..right_max_width)
                                .flex_none()
                                .child(inspector),
                        )),
                )
                .when(panels.right_collapsed, |row| row.child(crate::panels::sidebar::rail(Side::Right,
                    crate::panels::sidebar::toggle_button(Side::Right, true, locale,
                        |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleRightPanel), cx)), cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_shrink_0()
                    .px_4()
                    .h_9()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().size_2().rounded_full().bg(theme.primary))
                            .child(text(
                                locale,
                                if ready { Key::Ready } else { Key::PreparingGpu },
                            ))
                            .child(
                                Message::new(Key::WorkspaceSummary)
                                    .arg("layers", self.scene.layers.len())
                                    .arg("components", self.scene.components.len())
                                    .arg("nets", self.scene.nets.len())
                                    .display(locale),
                            ),
                    )
                    .when_some(pointer_coordinates, |bar, coordinates| {
                        bar.child(div().truncate().child(coordinates))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(text(locale, Key::LocalFile))
                            .children(
                                [
                                    (
                                        pomelo_core::units::LengthUnit::Millimeters,
                                        Key::UnitMillimeters,
                                        0u32,
                                    ),
                                    (pomelo_core::units::LengthUnit::Mils, Key::UnitMils, 1u32),
                                ]
                                .into_iter()
                                .map(|(unit, key, id)| {
                                    Button::new(("length-unit", id))
                                        .ghost()
                                        .label(text(locale, key))
                                        .selected(self.display.length_unit == unit)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            Arc::make_mut(&mut this.display).length_unit = unit;
                                            cx.notify();
                                        }))
                                }),
                            )
                            .child(text(locale, Key::GpuStatus)),
                    ),
            );
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
                    .child(div().child(text(locale, Key::GpuFailed)))
                    .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                    .child(div().text_sm().child(details)),
            );
        }
        view
    }
}
