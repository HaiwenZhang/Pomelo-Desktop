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
        let locale = i18n::current(cx);
        let theme = cx.theme();
        let stats = self.telemetry.snapshot();
        let copper_stats = self.copper_telemetry.snapshot();
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
        let pad_stats = self.pad_telemetry.snapshot();
        let custom_pad_stats = self.custom_pad_telemetry.snapshot();
        let custom_outline_stats = self.custom_outline_telemetry.snapshot();
        let zone_outline_stats = self.zone_outline_telemetry.snapshot();
        let drill_stats = self.drill_telemetry.snapshot();
        let drawing_stats = self.drawing_telemetry.snapshot();
        let text_stats = self.text_telemetry.snapshot();
        let source_stroke_stats = self.source_stroke_telemetry.snapshot();
        let label_stats = self.label_telemetry.snapshot();
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
        let ready = geometry_ready
            && !self.curve_fill.pending
            && !self.curve_fill.failed
            && copper_stats.curve_uploaded_bytes == curve_bytes
            && copper_stats.curve_revision == curve_revision
            && self
                .label_cache
                .as_ref()
                .is_none_or(|cache| cache.source.instances.is_empty() || label_stats.visible_ready);
        if failure.is_none()
            && !self.curve_fill.failed
            && (!ready || status.as_ref().is_some_and(|status| status.encoded == 0))
        {
            window.request_animation_frame();
        }
        if let Some(path) = std::env::var_os("POMELO_BOARD_GPU_TRACE") {
            let specs = window.gpu_specs();
            let pick_filter = serde_json::json!({
                "segment": self.pick_filter.contains(pomelo_core::picking::PickCategory::Segment),
                "pin": self.pick_filter.contains(pomelo_core::picking::PickCategory::Pin),
                "via": self.pick_filter.contains(pomelo_core::picking::PickCategory::Via),
                "zone": self.pick_filter.contains(pomelo_core::picking::PickCategory::Zone),
                "drawing": self.pick_filter.contains(pomelo_core::picking::PickCategory::Drawing),
            });
            let members_report = self.selected_members.as_ref().map(|members| {
                serde_json::json!({
                    "offset": self.members_offset,
                    "page_size": 256,
                    "total": members.total,
                    "objects": members.objects.iter().map(|object| {
                        use pomelo_core::selection::SelectedObject;
                        let (category, id) = match *object {
                            SelectedObject::Segment(id) => ("segment", id),
                            SelectedObject::Pin(id) => ("pin", id),
                            SelectedObject::Via(id) => ("via", id),
                            SelectedObject::Zone(id) => ("zone", id),
                            SelectedObject::Drawing(id) => ("drawing", id),
                        };
                        serde_json::json!({"category":category,"source_id":id.0})
                    }).collect::<Vec<_>>(),
                })
            });
            let mut report = serde_json::json!({
                "backend": pomelo_render::backend::native::NAME, "shader_owner": "pomelo-render", "scope": "traces_outline_copper_pads_drills_drawings_msdf_text_labels",
                "cpu_pixel_readback": false, "expected_instances": self.tracks.instances.len(),
                "ready": ready, "status": {
                    "encoded": status.as_ref().map(|status| status.encoded),
                    "last_error": failure,
                },
                "statistics": stats,
                "copper_statistics": copper_stats,
                "curve_statistics": self.curve_fill.statistics(),
                "curve_pending": self.curve_fill.pending,
                "curve_failed": self.curve_fill.failed,
                "expected_curve_bytes": curve_bytes,
                "expected_curve_revision": curve_revision,
                "pad_statistics": pad_stats,
                "custom_pad_statistics": custom_pad_stats,
                "custom_outline_statistics": custom_outline_stats,
                "zone_outline_statistics": zone_outline_stats,
                "expected_zone_outline_instances": self.zone_outlines.instances.len(),
                "drill_statistics": drill_stats,
                "drawing_statistics": drawing_stats,
                "expected_drawing_instances": self.drawings.instances.len(),
                "text_statistics": text_stats,
                "source_stroke_statistics": source_stroke_stats,
                "label_statistics": label_stats,
            });
            let details = serde_json::json!({
                "expected_source_stroke_instances": self.source_strokes.as_ref().map_or(0, |source| source.instances.len()),
                "source_stroke_objects": self.source_strokes.as_ref().map_or(&[][..], |source| source.objects.as_slice()),
                "expected_text_instances": self.texts.as_ref().map_or(0, |source| source.instances.len()),
                "expected_drills": self.drills.analytic.len(),
                "adapter": specs.as_ref().map(|specs| &specs.device_name),
                "software_emulated": specs.as_ref().map(|specs| specs.is_software_emulated),
                "board_bounds": self.scene.bounds,
                "source_trace_count": self.scene.segments.len(),
                "color_mode": self.display.color_mode,
                "source_outline_count": self.scene.outline.len(),
                "copper_prepared": {
                    "zones": self.copper.batches.len(),
                    "vertices": self.copper.vertex_count(),
                    "indices": self.copper.index_count(),
                    "gpu_drawn": copper_stats.draw_calls > 0,
                },
                "pads_prepared": {
                    "analytic": self.pads.analytic.len(),
                    "custom": self.pads.custom.len(),
                    "gpu_drawn": pad_stats.draw_calls > 0,
                    "custom_gpu_drawn": custom_pad_stats.draw_calls > 0,
                    "custom_vertices": self.pads.custom_mesh.as_ref().map(|mesh| mesh.vertices.len()),
                    "custom_indices": self.pads.custom_mesh.as_ref().map(|mesh| mesh.indices.len()),
                },
                "selected_target": self.selected_target.map(|target| format!("{target:?}")),
                "selected_anchor": self.selected_anchor,
                "panel_layout": panels,
                "canvas_size": self.navigation.size(),
                "device_scale_factor": window.scale_factor(),
                "selection_mode": format!("{:?}", self.selection_mode),
                "hovered_object": self.hovered.as_ref().filter(|(_, context)| context.matches_view(
                    self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                )).map(|(object, _)| format!("{object:?}")),
                "pick_filter": pick_filter,
                "selection_query": {
                    "search_pending": self.search_pending,
                    "selection_pending": self.locate_pending,
                    "candidate_position": self.last_pick.as_ref().filter(|context| context.matches_view(
                        self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                    )).and(self.candidate_position),
                },
                "members": members_report,
                "search_entries": self.search.entries().len(),
                "pick_index_segments": self.picking.segment_count(),
                "display": {
                    "hidden_layers": self.display.hidden_layers.iter().map(|layer| layer.0).collect::<Vec<_>>(),
                    "show_drills": self.display.show_drills,
                    "show_backdrills": self.display.show_backdrills,
                    "show_copper": self.display.show_copper,
                    "static_shapes_fill_solid": self.display.static_shapes_fill_solid,
                    "filled": self.display.filled,
                    "horizontal_pin_names": self.display.horizontal_pin_names,
                    "label_options": self.display.label_options,
                    "appearance": self.display.appearance,
                    "show_texts": self.display.show_texts,
                    "show_drawings": self.display.show_drawings,
                    "layer_order": &*layer_order,
                    "copper_opacity": self.display.copper_opacity,
                    "global_opacity": self.display.global_opacity,
                },
                "camera": {
                    "center": self.navigation.camera().center,
                    "logical_pixels_per_mm": self.navigation.camera().pixels_per_mm,
                    "flipped": self.navigation.camera().flipped,
                    "logical_viewport_size": self.navigation.size(),
                },
            });
            if let (serde_json::Value::Object(report), serde_json::Value::Object(details)) =
                (&mut report, details)
            {
                report.extend(details);
            }
            if let Err(error) = std::fs::write(path, report.to_string()) {
                eprintln!("{}: {error}", text(locale, Key::FileIoFailed));
            }
        }
        let mut frame = TraceFrame {
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
            highlighted_related_objects: (!self.selected_bonds.is_empty()).then(|| {
                let color: Rgba = theme.primary.into();
                (
                    Arc::clone(&self.selected_bonds),
                    [color.r, color.g, color.b, color.a],
                )
            }),
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
        let telemetry = Arc::clone(&self.telemetry);
        let copper_telemetry = Arc::clone(&self.copper_telemetry);
        let pad_telemetry = Arc::clone(&self.pad_telemetry);
        let custom_pad_telemetry = Arc::clone(&self.custom_pad_telemetry);
        let custom_outline_telemetry = Arc::clone(&self.custom_outline_telemetry);
        let drill_telemetry = Arc::clone(&self.drill_telemetry);
        let drawing_telemetry = Arc::clone(&self.drawing_telemetry);
        let text_telemetry = Arc::clone(&self.text_telemetry);
        let source_stroke_telemetry = Arc::clone(&self.source_stroke_telemetry);
        let zone_outline_telemetry = Arc::clone(&self.zone_outline_telemetry);
        let label_telemetry = Arc::clone(&self.label_telemetry);
        let texts = self.texts.clone();
        let source_strokes = self.source_strokes.clone();
        let drawings = Arc::clone(&self.drawings);
        let drills = Arc::clone(&self.drills);
        let display = Arc::clone(&self.display);
        let drill_color = [0.46, 0.49, 0.51, 1.0];
        let pads = Arc::clone(&self.pads);
        let copper = Arc::clone(&self.copper);
        let zone_outlines = Arc::clone(&self.zone_outlines);
        let copper_opacity = self.display.copper_opacity;
        let viewport = canvas(
            move |bounds, _window, cx| {
                let prepaint_started = pomelo_render::frame_timing::begin();
                let mut label_layout_us = 0u64;
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
                        if let Some(index) = &this.label_index {
                            let key = [
                                camera.center.x.to_bits(),
                                camera.center.y.to_bits(),
                                camera.pixels_per_mm.to_bits(),
                                u64::from(camera.flipped),
                                width.to_bits(),
                                height.to_bits(),
                            ];
                            if this.label_cache.as_ref().is_none_or(|cache| {
                                cache.key != key || !Arc::ptr_eq(&cache.display, &this.display)
                            }) {
                                let layout_started = prepaint_started.map(|_|std::time::Instant::now());
                                match index.layout(
                                    camera,
                                    width,
                                    height,
                                    &this.display,
                                    this.display.label_options,
                                    &pomelo_core::task::CancellationToken::default(),
                                ) {
                                    Ok(source) => {
                                        this.label_cache = Some(LabelCache {
                                            key,
                                            display: Arc::clone(&this.display),
                                            source: Arc::new(source),
                                        })
                                    }
                                    Err(diagnostic) => {
                                        this.label_cache = None;
                                        if !this
                                            .render_diagnostics
                                            .iter()
                                            .any(|item| item.code == diagnostic.code)
                                        {
                                            this.render_diagnostics.push(diagnostic);
                                        }
                                    }
                                }
                                if let Some(started) = layout_started {
                                    label_layout_us = started.elapsed().as_micros() as u64;
                                }
                            }
                            labels = this
                                .label_cache
                                .as_ref()
                                .map(|cache| Arc::clone(&cache.source));
                        }
                        camera
                    })
                    .ok();
                if resized {
                    frame.hovered_object = None;
                }
                pomelo_render::frame_timing::record("prepaint_board", prepaint_started, || serde_json::json!({
                    "camera":frame.camera,"segments":frame.tracks.instances.len(),"zones":copper.batches.len(),
                    "label_layout_us":label_layout_us,
                    "labels":labels.as_ref().map_or(0, |source|source.instances.len()),
                }));
                Arc::new(BoardFrame {
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
                            if this.pointer_gesture.is_some() {
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
                            if this.pointer_gesture.as_ref().is_some_and(|gesture| gesture.button == event.button) {
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
                    let before_uploaded = telemetry.snapshot().uploaded_instances;
                    let before_trace_ready = telemetry.snapshot().visible_ready;
                    let before_residency_ready = [
                        custom_outline_telemetry.snapshot().visible_ready,
                        drawing_telemetry.snapshot().visible_ready,
                        text_telemetry.snapshot().visible_ready,
                        source_stroke_telemetry.snapshot().visible_ready,
                        label_telemetry.snapshot().visible_ready,
                        zone_outline_telemetry.snapshot().visible_ready,
                    ];
                    let before_copper = copper_telemetry.snapshot().uploaded_bytes;
                    let before_pads = pad_telemetry.snapshot().uploaded_instances;
                    let before_custom = custom_pad_telemetry.snapshot().uploaded_bytes;
                    let before_custom_outlines =
                        custom_outline_telemetry.snapshot().uploaded_instances;
                    let before_drills = drill_telemetry.snapshot().uploaded_instances;
                    let before_drawings = drawing_telemetry.snapshot().uploaded_instances;
                    let before_texts = text_telemetry.snapshot().uploaded_instances;
                    let before_source_strokes =
                        source_stroke_telemetry.snapshot().uploaded_instances;
                    let before_labels = label_telemetry.snapshot().uploaded_instances;
                    let before_zone_outlines = zone_outline_telemetry.snapshot().uploaded_instances;
                    window.on_next_frame(move |_, cx| {
                        let after = renderer.status();
                        let uploaded = telemetry.snapshot().uploaded_instances;
                        if before.last_error != after.last_error
                            || before.resets != after.resets
                            || before_uploaded != uploaded
                            || before_trace_ready != telemetry.snapshot().visible_ready
                            || before_residency_ready
                                != [
                                    custom_outline_telemetry.snapshot().visible_ready,
                                    drawing_telemetry.snapshot().visible_ready,
                                    text_telemetry.snapshot().visible_ready,
                                    source_stroke_telemetry.snapshot().visible_ready,
                                    label_telemetry.snapshot().visible_ready,
                                    zone_outline_telemetry.snapshot().visible_ready,
                                ]
                            || before_copper != copper_telemetry.snapshot().uploaded_bytes
                            || before_pads != pad_telemetry.snapshot().uploaded_instances
                            || before_custom != custom_pad_telemetry.snapshot().uploaded_bytes
                            || before_custom_outlines
                                != custom_outline_telemetry.snapshot().uploaded_instances
                            || before_drills != drill_telemetry.snapshot().uploaded_instances
                            || before_drawings != drawing_telemetry.snapshot().uploaded_instances
                            || before_texts != text_telemetry.snapshot().uploaded_instances
                            || before_source_strokes
                                != source_stroke_telemetry.snapshot().uploaded_instances
                            || before_labels != label_telemetry.snapshot().uploaded_instances
                            || before_zone_outlines
                                != zone_outline_telemetry.snapshot().uploaded_instances
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
                + self.copper.upload_bytes()
                + curve_bytes as usize
                + self.pads.analytic.len() * 128
                + custom_bytes
                + self.drills.analytic.len() * 128,
        }
    }
}
