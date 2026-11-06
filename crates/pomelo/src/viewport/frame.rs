//! GPU frame assembly, upload readiness and canvas painting.
use super::*;
use crate::services::preferences::PanelPreferences;
pub(super) struct CanvasPresentation {
    pub viewport: AnyElement,
    pub ready: bool,
    pub geometry_ready: bool,
    pub failure: Option<String>,
    pub uploaded_bytes: u64,
    pub total_bytes: usize,
}
impl BoardViewport {
    pub(super) fn render_canvas(
        &mut self,
        layer_order: Arc<Vec<pomelo_core::model::LayerId>>,
        panels: PanelPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> CanvasPresentation {
        if self.view3d.active {
            return self.render_canvas_3d(window, cx);
        }
        let locale = i18n::current(cx);
        let theme = cx.theme();
        let stats = self.telemetry.traces.snapshot();
        let copper_stats = self.telemetry.copper.snapshot();
        let placement_stats = self.telemetry.placements.snapshot();
        let placement_outline_stats = self.telemetry.placement_outlines.snapshot();
        let curve_frame = self.curve_fill.frame();
        let curve_revision = curve_frame.as_ref().map_or(0, |cache| cache.revision());
        let curve_bytes = curve_frame.as_ref().map_or(0, |cache| {
            cache
                .active
                .iter()
                .filter_map(|id| cache.entries.get(id))
                .map(|entry| entry.source.upload_bytes() as u64)
                .sum::<u64>()
        });
        let pad_stats = self.telemetry.pads.snapshot();
        let custom_pad_stats = self.telemetry.custom_pads.snapshot();
        let custom_outline_stats = self.telemetry.custom_outlines.snapshot();
        let zone_outline_stats = self.telemetry.zone_outlines.snapshot();
        let drill_stats = self.telemetry.drills.snapshot();
        let drawing_stats = self.telemetry.drawings.snapshot();
        let text_stats = self.telemetry.texts.snapshot();
        let source_stroke_stats = self.telemetry.source_strokes.snapshot();
        let label_stats = self.telemetry.labels.snapshot();
        let custom_bytes = self
            .pads
            .custom_mesh
            .as_ref()
            .map_or(0, |mesh| mesh.upload_bytes());
        let renderer = self.renderer.as_ref().ok().cloned();
        let status = renderer.as_ref().map(GpuPainterHandle::status);
        let failure = self
            .renderer
            .as_ref()
            .err()
            .cloned()
            .or_else(|| self.paint_error.clone())
            .or_else(|| status.as_ref().and_then(|status| status.last_error.clone()));
        let geometry_ready = (self.tracks.instances.is_empty() || stats.visible_ready)
            && copper_stats.uploaded_bytes == self.copper.upload_bytes() as u64
            && pad_stats.uploaded_instances == self.pads.analytic.len() as u64
            && custom_pad_stats.uploaded_bytes == custom_bytes as u64
            && self.pads.custom_outlines.as_ref().is_none_or(|source| {
                source.instances.is_empty() || custom_outline_stats.visible_ready
            })
            && drill_stats.uploaded_instances == self.drills.analytic.len() as u64
            && (self.zone_outlines.instances.is_empty() || zone_outline_stats.visible_ready)
            && (self.drawings.instances.is_empty() || drawing_stats.visible_ready)
            && self
                .texts
                .as_ref()
                .is_none_or(|source| source.instances.is_empty() || text_stats.visible_ready)
            && self.source_strokes.as_ref().is_none_or(|source| {
                source.instances.is_empty() || source_stroke_stats.visible_ready
            });
        let gpu_ready = geometry_ready
            && (self.display.quickview.is_none()
                || placement_stats.uploaded_bytes == self.placements.upload_bytes() as u64)
            && (self.display.quickview.is_none()
                || self.placement_outlines.instances.is_empty()
                || placement_outline_stats.visible_ready)
            && !self.curve_fill.failed
            && copper_stats.curve_uploaded_bytes == curve_bytes
            && copper_stats.curve_revision == curve_revision
            && self
                .labels
                .frame()
                .is_none_or(|source| source.instances.is_empty() || label_stats.visible_ready);
        let ready =
            gpu_ready && !self.curve_fill.pending && !self.labels.pending && !self.labels.failed;
        if failure.is_none()
            && !self.curve_fill.failed
            && (!gpu_ready || status.as_ref().is_some_and(|status| status.encoded == 0))
        {
            window.request_animation_frame();
        }
        if let Some(path) = std::env::var_os("POMELO_BOARD_GPU_TRACE") {
            self.write_gpu_trace(
                std::path::Path::new(&path),
                gpu_trace::FrameState {
                    ready,
                    status: status.as_ref(),
                    failure: failure.as_deref(),
                    curve_bytes,
                    curve_revision,
                    layer_order: &layer_order,
                    panels,
                },
                window,
                locale,
            );
        }
        let mut frame = TraceFrame {
            net_colors: None,
            pattern: None,
            object_filter: Default::default(),
            brightness: 1.0,
            pass: pomelo_render::backend::native::OverlayPass::Base,
            filled: self.display.filled,
            hover_selection: self.hover_selection.clone().filter(|_| {
                self.hovered.as_ref().is_some_and(|(_, context)| {
                    context.matches_view(
                        self.navigation.camera(),
                        self.navigation.size(),
                        &self.display,
                        self.selection_mode,
                    )
                })
            }),
            color_mode: self.display.color_mode,
            hovered_object: self
                .hovered
                .as_ref()
                .filter(|(_, context)| {
                    context.matches_view(
                        self.navigation.camera(),
                        self.navigation.size(),
                        &self.display,
                        self.selection_mode,
                    )
                })
                .map(|(object, _)| {
                    let color: Rgba = theme.ring.into();
                    (*object, [color.r, color.g, color.b, color.a])
                }),
            highlighted_related_objects: Some(self.related_highlight())
                .filter(|objects| !objects.is_empty())
                .map(|objects| {
                    let color: Rgba = theme.primary.into();
                    (objects, [color.r, color.g, color.b, color.a])
                }),
            object_colors: self.review.object_colors.clone(),
            highlighted_trace: self.selected_target.and_then(|target| {
                use pomelo_core::selection::{SelectedObject, SelectionTarget};
                let selection = match target {
                    SelectionTarget::Object(SelectedObject::Segment(id)) => {
                        pomelo_render::backend::native::TraceSelection::Segment(id)
                    }
                    SelectionTarget::Track(id) => {
                        pomelo_render::backend::native::TraceSelection::Track(id)
                    }
                    _ => return None,
                };
                let color: Rgba = theme.primary.into();
                Some((selection, [color.r, color.g, color.b, color.a]))
            }),
            tracks: Arc::clone(&self.tracks),
            bounds: self.scene.bounds,
            camera: None,
            scale_factor: window.scale_factor(),
            colors: Arc::clone(&self.colors),
            fallback_color: [0.6, 0.68, 0.73, 1.0],
            material_override: None,
            opacity: 1.0,
            highlighted_object: self.selected_target.and_then(|target| {
                if let pomelo_core::selection::SelectionTarget::Object(object) = target {
                    let color: Rgba = theme.primary.into();
                    Some((object, [color.r, color.g, color.b, color.a]))
                } else {
                    None
                }
            }),
            highlighted_objects: if matches!(
                self.selected_target,
                Some(
                    pomelo_core::selection::SelectionTarget::Component(_)
                        | pomelo_core::selection::SelectionTarget::ComponentGroup(_)
                        | pomelo_core::selection::SelectionTarget::Object(
                            pomelo_core::selection::SelectedObject::Pin(_)
                        )
                )
            ) {
                let color: Rgba = theme.primary.into();
                Some((
                    Arc::clone(&self.selected_pins),
                    [color.r, color.g, color.b, color.a],
                ))
            } else {
                None
            },
            highlighted_net: self.selected_target.and_then(|target| match target {
                pomelo_core::selection::SelectionTarget::Net(net) => {
                    let color: Rgba = theme.primary.into();
                    Some((net, [color.r, color.g, color.b, color.a]))
                }
                _ => None,
            }),
        };
        let weak = cx.entity().downgrade();
        let layout_view = weak.clone();
        let telemetry = self.telemetry.clone();
        let texts = self.texts.clone();
        let source_strokes = self.source_strokes.clone();
        let drawings = Arc::clone(&self.drawings);
        let drills = Arc::clone(&self.drills);
        let display = Arc::clone(&self.display);
        let drill_color = [0.46, 0.49, 0.51, 1.0];
        let pads = Arc::clone(&self.pads);
        let copper = Arc::clone(&self.copper);
        let placements = Arc::clone(&self.placements);
        let placement_outlines = Arc::clone(&self.placement_outlines);
        let zone_outlines = Arc::clone(&self.zone_outlines);
        let copper_opacity = self.display.copper_opacity;
        let viewport = canvas(
            move |bounds, _window, cx| {
                let prepaint_started = pomelo_render::frame_timing::begin();
                let mut label_pending = false;
                let mut resized = false;
                let mut labels = None;
                let mut curves = None;
                frame.camera = layout_view
                    .update(cx, |this, cx| {
                        let previous_size = this.navigation.size();
                        this.bounds = Some(bounds);
                        this.navigation.resize(
                            this.scene.bounds,
                            f64::from(f32::from(bounds.size.width)),
                            f64::from(f32::from(bounds.size.height)),
                        );
                        resized = this.navigation.size() != previous_size;
                        if resized {
                            this.invalidate_hover();
                        }
                        let camera = this.navigation.camera();
                        let viewport_size = this.navigation.size();
                        let (width, height) = (viewport_size.x, viewport_size.y);
                        this.request_curves(
                            pomelo_render::scene::curves::CurveView::new(
                                camera,
                                width,
                                height,
                                f64::from(frame.scale_factor),
                            ),
                            cx,
                        );
                        curves = this.curve_fill.frame();
                        this.request_labels(camera, width, height, cx);
                        labels = this.labels.frame();
                        label_pending = this.labels.pending;
                        this.update_camera_motion(camera, cx);
                        camera
                    })
                    .ok();
                if resized {
                    frame.hovered_object = None;
                }
                pomelo_render::frame_timing::record("prepaint_board", prepaint_started, || serde_json::json!({
                    "camera":frame.camera,"segments":frame.tracks.instances.len(),"zones":copper.batches.len(),
                    "label_layout_us":0,"label_pending":label_pending,
                    "labels":labels.as_ref().map_or(0, |source|source.instances.len()),
                }));
                Arc::new(BoardFrame {
                    placements: Some(placements),
                    placement_outlines:Some(placement_outlines),
                    curves,
                    zone_outlines: Some(zone_outlines),
                    drawings: Some(drawings),
                    texts: source_strokes,
                    glyphs: texts,
                    labels,
                    traces: frame,
                    display,
                    pads: Some(pads),
                    drills: Some(drills),
                    drill_color,
                    copper,
                    copper_opacity,
                    layer_order,
                })
            },
            move |bounds, frame, window, cx| {
                // GPUI registers per-frame input listeners only during paint.
                let move_view = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = move_view.update(cx, |this, cx| {
                            if this.pointer_gesture.is_some() || this.review.drag.is_some() {
                                this.move_pan(event, window, cx);
                                cx.stop_propagation();
                            }
                        });
                    }
                });
                let release_view = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = release_view.update(cx, |this, cx| {
                            let review_drag = event.button == MouseButton::Left && this.review.drag.is_some();
                            if review_drag || this.pointer_gesture.as_ref().is_some_and(|gesture| gesture.button == event.button) {
                                if bounds.contains(&event.position) {
                                    this.stop_pan(event, window, cx);
                                } else {
                                    this.cancel_pan(event, window, cx);
                                    cx.stop_propagation();
                                }
                            }
                        });
                    }
                });
                if pomelo_render::frame_timing::continuous() {
                    window.request_animation_frame();
                    let repaint = weak.clone();
                    window.on_next_frame(move |_, cx| {
                        let _ = repaint.update(cx, |_, cx| cx.notify());
                    });
                }
                if let Some(renderer) = &renderer
                    && let Err(error) = window.paint_gpu(bounds, renderer, frame)
                {
                    let details = format!("{error:#}");
                    let _ = weak.update(cx, |this, cx| {
                        this.paint_error = Some(details);
                        cx.notify();
                    });
                }
                if let Some(renderer) = renderer {
                    let weak = weak.clone();
                    let before = renderer.status();
                    let before_progress = telemetry.progress();
                    window.on_next_frame(move |_, cx| {
                        let after = renderer.status();
                        if before.last_error != after.last_error
                            || before.resets != after.resets
                            || before_progress != telemetry.progress()
                            || (before.encoded == 0 && after.encoded > 0)
                        {
                            let _ = weak.update(cx, |_, cx| cx.notify());
                        }
                    });
                }
            },
        )
        .size_full();
        CanvasPresentation {
            viewport: viewport.into_any_element(),
            ready,
            geometry_ready,
            failure,
            uploaded_bytes: stats.uploaded_instances * 128
                + copper_stats.uploaded_bytes
                + copper_stats.curve_uploaded_bytes
                + pad_stats.uploaded_instances * 128
                + custom_pad_stats.uploaded_bytes
                + drill_stats.uploaded_instances * 128,
            total_bytes: self.tracks.instances.len() * 128
                + self
                    .copper
                    .upload_bytes()
                    .max(copper_stats.uploaded_bytes as usize)
                + (curve_bytes as usize).max(copper_stats.curve_required_bytes as usize)
                + self.pads.analytic.len() * 128
                + custom_bytes.max(custom_pad_stats.uploaded_bytes as usize)
                + self.drills.analytic.len() * 128,
        }
    }
}
