//! Opt-in `POMELO_BOARD_GPU_TRACE` report of renderer state for local validation.
use super::*;
use crate::services::preferences::PanelPreferences;

/// Values the canvas already derived for this frame.
pub(super) struct FrameState<'a> {
    pub ready: bool,
    pub status: Option<&'a gpui::GpuPainterStatus>,
    pub failure: Option<&'a str>,
    pub curve_bytes: u64,
    pub curve_revision: u64,
    pub layer_order: &'a [pomelo_core::model::LayerId],
    pub panels: PanelPreferences,
}

impl BoardViewport {
    /// Synchronous by design: the report is written only when explicitly requested.
    pub(super) fn write_gpu_trace(
        &self,
        path: &std::path::Path,
        state: FrameState<'_>,
        window: &Window,
        locale: pomelo_core::i18n::Locale,
    ) {
        let FrameState {
            ready,
            status,
            failure,
            curve_bytes,
            curve_revision,
            layer_order,
            panels,
        } = state;
        let stats = self.telemetry.traces.snapshot();
        let copper_stats = self.telemetry.copper.snapshot();
        let pad_stats = self.telemetry.pads.snapshot();
        let custom_pad_stats = self.telemetry.custom_pads.snapshot();
        let custom_outline_stats = self.telemetry.custom_outlines.snapshot();
        let zone_outline_stats = self.telemetry.zone_outlines.snapshot();
        let drill_stats = self.telemetry.drills.snapshot();
        let drawing_stats = self.telemetry.drawings.snapshot();
        let text_stats = self.telemetry.texts.snapshot();
        let source_stroke_stats = self.telemetry.source_strokes.snapshot();
        let label_stats = self.telemetry.labels.snapshot();
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
                        SelectedObject::Placement(id) => ("placement",id),
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
            "trace_render_mode": self.display.trace_render_mode,
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
                "layer_order": layer_order,
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
}
