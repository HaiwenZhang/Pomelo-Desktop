//! Upload and readiness counters shared between the viewport and its renderer.
use pomelo_render::backend::native::{BoardRenderer, CopperTelemetry, TraceTelemetry};
use std::sync::Arc;

/// One handle per renderer source; cloning shares the same atomic counters.
#[derive(Clone, Default)]
pub(super) struct RendererTelemetry {
    pub traces: Arc<TraceTelemetry>,
    pub copper: Arc<CopperTelemetry>,
    pub placements: Arc<CopperTelemetry>,
    pub placement_outlines: Arc<TraceTelemetry>,
    pub pads: Arc<TraceTelemetry>,
    pub custom_pads: Arc<CopperTelemetry>,
    pub custom_outlines: Arc<TraceTelemetry>,
    pub drills: Arc<TraceTelemetry>,
    pub drawings: Arc<TraceTelemetry>,
    pub texts: Arc<TraceTelemetry>,
    pub source_strokes: Arc<TraceTelemetry>,
    pub zone_outlines: Arc<TraceTelemetry>,
    pub labels: Arc<TraceTelemetry>,
}

/// Upload progress and visible readiness; any change requires a repaint.
#[derive(PartialEq, Eq)]
pub(super) struct Progress {
    uploads: [u64; 13],
    ready: [bool; 14],
}

impl RendererTelemetry {
    /// A board renderer reporting into these counters.
    pub fn renderer(&self) -> BoardRenderer {
        BoardRenderer::new(
            Arc::clone(&self.traces),
            Arc::clone(&self.copper),
            Arc::clone(&self.pads),
            Arc::clone(&self.custom_pads),
            Arc::clone(&self.drills),
            Arc::clone(&self.drawings),
            Arc::clone(&self.texts),
        )
        .with_source_stroke_telemetry(Arc::clone(&self.source_strokes))
        .with_custom_outline_telemetry(Arc::clone(&self.custom_outlines))
        .with_zone_outline_telemetry(Arc::clone(&self.zone_outlines))
        .with_label_telemetry(Arc::clone(&self.labels))
        .with_placement_telemetry(Arc::clone(&self.placements))
        .with_placement_outline_telemetry(Arc::clone(&self.placement_outlines))
    }

    pub fn progress(&self) -> Progress {
        let instances = [
            &self.traces,
            &self.pads,
            &self.custom_outlines,
            &self.drills,
            &self.drawings,
            &self.texts,
            &self.source_strokes,
            &self.labels,
            &self.zone_outlines,
        ]
        .map(|telemetry| telemetry.snapshot());
        let (copper, custom_pads) = (self.copper.snapshot(), self.custom_pads.snapshot());
        let mut uploads = [0; 13];
        let mut ready = [false; 14];
        for (index, statistics) in instances.iter().enumerate() {
            uploads[index] = statistics.uploaded_instances;
            ready[index] = statistics.visible_ready;
        }
        uploads[9] = copper.uploaded_bytes;
        uploads[10] = custom_pads.uploaded_bytes;
        ready[9] = copper.uploaded_bytes > 0;
        ready[10] = copper.curve_uploaded_bytes == copper.curve_required_bytes;
        ready[11] = custom_pads.uploaded_bytes > 0;
        let placements = self.placements.snapshot();
        uploads[11] = placements.uploaded_bytes;
        ready[12] = placements.uploaded_bytes > 0;
        let outlines = self.placement_outlines.snapshot();
        uploads[12] = outlines.uploaded_instances;
        ready[13] = outlines.visible_ready;
        Progress { uploads, ready }
    }
}
