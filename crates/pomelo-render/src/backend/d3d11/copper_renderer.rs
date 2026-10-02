//! Copper resource lifecycle and telemetry, independent of the generic GPUI patch.

use super::{
    TraceFrame, TraceRenderer, TraceTelemetry,
    copper_d3d11::{Pipeline, UploadedCopper},
};
use crate::copper::PreparedCopper;
use anyhow::Context as _;
use gpui::{NativeGpuContext, NativeGpuRenderer};
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Default)]
pub struct CopperTelemetry {
    pipeline_builds: AtomicU64,
    cache_builds: AtomicU64,
    uploaded_bytes: AtomicU64,
    lifetime_uploaded_bytes: AtomicU64,
    draw_calls: AtomicU64,
    visible_zones: AtomicU64,
    resets: AtomicU64,
}
#[derive(Debug, serde::Serialize)]
pub struct CopperStatistics {
    pub pipeline_builds: u64,
    pub cache_builds: u64,
    pub uploaded_bytes: u64,
    pub lifetime_uploaded_bytes: u64,
    pub draw_calls: u64,
    pub visible_zones: u64,
    pub resets: u64,
}
impl CopperTelemetry {
    pub fn snapshot(&self) -> CopperStatistics {
        CopperStatistics {
            pipeline_builds: self.pipeline_builds.load(Ordering::Relaxed),
            cache_builds: self.cache_builds.load(Ordering::Relaxed),
            uploaded_bytes: self.uploaded_bytes.load(Ordering::Acquire),
            lifetime_uploaded_bytes: self.lifetime_uploaded_bytes.load(Ordering::Relaxed),
            draw_calls: self.draw_calls.load(Ordering::Relaxed),
            visible_zones: self.visible_zones.load(Ordering::Relaxed),
            resets: self.resets.load(Ordering::Relaxed),
        }
    }
}

pub struct CopperRenderer {
    pipeline: Option<Pipeline>,
    uploaded: Option<UploadedCopper>,
    telemetry: Arc<CopperTelemetry>,
}
impl CopperRenderer {
    fn is_uploaded(&self, source: &Arc<PreparedCopper>) -> bool {
        self.uploaded.as_ref().is_some_and(|cache| {
            Arc::ptr_eq(&cache.source, source) && cache.uploaded_bytes() == source.upload_bytes()
        })
    }
    pub fn new(telemetry: Arc<CopperTelemetry>) -> Self {
        Self {
            pipeline: None,
            uploaded: None,
            telemetry,
        }
    }

    pub fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        source: &Arc<PreparedCopper>,
        opacity: f32,
        allow_upload: bool,
    ) -> anyhow::Result<()> {
        if !self.prepare(context, source, allow_upload)? {
            return Ok(());
        }
        self.draw_prepared(context, frame, opacity, None)
    }

    pub(super) fn prepare(
        &mut self,
        context: &NativeGpuContext<'_>,
        source: &Arc<PreparedCopper>,
        allow_upload: bool,
    ) -> anyhow::Result<bool> {
        let changed = self
            .uploaded
            .as_ref()
            .is_none_or(|cache| !Arc::ptr_eq(&cache.source, source));
        if changed {
            self.telemetry.uploaded_bytes.store(0, Ordering::Release);
            self.telemetry.visible_zones.store(0, Ordering::Relaxed);
            if !allow_upload {
                return Ok(false);
            }
            self.uploaded = Some(UploadedCopper::new(Arc::clone(source), context.device)?);
            self.telemetry.cache_builds.fetch_add(1, Ordering::Relaxed);
        }
        let cache = self.uploaded.as_mut().context("GPU_COPPER_CACHE_MISSING")?;
        if allow_upload {
            let before = cache.uploaded_bytes();
            let result = cache.upload_next(context);
            self.telemetry
                .lifetime_uploaded_bytes
                .fetch_add((cache.uploaded_bytes() - before) as u64, Ordering::Relaxed);
            self.telemetry
                .uploaded_bytes
                .store(cache.uploaded_bytes() as u64, Ordering::Release);
            result?;
        }
        if source.indices.is_empty() {
            return Ok(false);
        }
        if self.pipeline.is_none() {
            self.pipeline = Some(Pipeline::new(context.device)?);
            self.telemetry
                .pipeline_builds
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(true)
    }

    pub(super) fn draw_prepared(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        opacity: f32,
        layer: Option<pomelo_core::model::LayerId>,
    ) -> anyhow::Result<()> {
        let cache = self.uploaded.as_ref().context("GPU_COPPER_CACHE_MISSING")?;
        let (draws, zones) = self
            .pipeline
            .as_mut()
            .context("GPU_COPPER_PIPELINE_MISSING")?
            .draw(context, frame, cache, opacity, layer)?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        self.telemetry.visible_zones.store(zones, Ordering::Relaxed);
        Ok(())
    }
    pub fn reset(&mut self) {
        self.pipeline = None;
        self.uploaded = None;
        self.telemetry.uploaded_bytes.store(0, Ordering::Release);
        self.telemetry.visible_zones.store(0, Ordering::Relaxed);
        self.telemetry.resets.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct BoardFrame {
    pub traces: TraceFrame,
    pub drawings: Option<Arc<crate::tracks::PreparedTracks>>,
    pub texts: Option<Arc<crate::text_instances::PreparedTextInstances>>,
    pub display: Arc<pomelo_core::display::BoardDisplay>,
    pub pads: Option<Arc<crate::pads::PreparedPads>>,
    pub drills: Option<Arc<crate::pads::PreparedPads>>,
    pub drill_color: [f32; 4],
    pub copper: Arc<PreparedCopper>,
    pub copper_opacity: f32,
    /// Back-to-front ordering. Unknown source layers are appended deterministically.
    pub layer_order: Arc<Vec<pomelo_core::model::LayerId>>,
}
pub struct BoardRenderer {
    traces: TraceRenderer,
    drawings: TraceRenderer,
    texts: TraceRenderer<crate::text_instances::PreparedTextInstances>,
    copper: CopperRenderer,
    pads: super::PadRenderer,
    custom_pads: CopperRenderer,
    drills: super::PadRenderer,
}
impl BoardRenderer {
    pub fn new(
        traces: Arc<TraceTelemetry>,
        copper: Arc<CopperTelemetry>,
        pads: Arc<TraceTelemetry>,
        custom_pads: Arc<CopperTelemetry>,
        drills: Arc<TraceTelemetry>,
        drawings: Arc<TraceTelemetry>,
        texts: Arc<TraceTelemetry>,
    ) -> Self {
        Self {
            traces: TraceRenderer::new(traces),
            drawings: TraceRenderer::new(drawings),
            texts: TraceRenderer::new(texts),
            copper: CopperRenderer::new(copper),
            pads: super::PadRenderer::new(pads),
            custom_pads: CopperRenderer::new(custom_pads),
            drills: super::PadRenderer::new(drills),
        }
    }
}
impl NativeGpuRenderer for BoardRenderer {
    fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()> {
        let frame = data
            .downcast_ref::<BoardFrame>()
            .context("GPU_BOARD_PAYLOAD_INVALID")?;
        // Defer copper uploads until this exact trace source is uploaded. Combined application
        // upload traffic remains <= 4 MiB/frame, including scene revisions and device recovery.
        let allow_copper_upload = self.traces.is_uploaded(&frame.traces.tracks);
        let allow_pad_upload = allow_copper_upload && self.copper.is_uploaded(&frame.copper);
        let allow_custom_upload = allow_pad_upload
            && frame
                .pads
                .as_ref()
                .is_none_or(|source| self.pads.is_uploaded(source));
        let allow_drill_upload = allow_custom_upload
            && frame
                .pads
                .as_ref()
                .and_then(|pads| pads.custom_mesh.as_ref())
                .is_none_or(|source| self.custom_pads.is_uploaded(source));
        let allow_drawing_upload = allow_drill_upload
            && frame
                .drills
                .as_ref()
                .is_none_or(|source| self.drills.is_uploaded(source));
        let drawing_frame = frame
            .drawings
            .as_ref()
            .filter(|tracks| !tracks.instances.is_empty())
            .map(|tracks| TraceFrame {
                tracks: Arc::clone(tracks),
                bounds: frame.traces.bounds,
                camera: frame.traces.camera,
                scale_factor: frame.traces.scale_factor,
                colors: Arc::clone(&frame.traces.colors),
                fallback_color: frame.traces.fallback_color,
                highlighted_objects: None,
                highlighted_net: None,
                highlighted_trace: None,
                highlighted_object: None,
                hovered_object: None,
                highlighted_related_objects: None,
            });
        if drawing_frame.is_none() {
            self.drawings.clear_if_cached();
        }
        let allow_text_upload = allow_drawing_upload
            && drawing_frame
                .as_ref()
                .is_none_or(|source| self.drawings.is_uploaded(&source.tracks));
        let text_frame = frame
            .texts
            .as_ref()
            .filter(|tracks| !tracks.instances.is_empty())
            .map(|tracks| TraceFrame {
                tracks: Arc::clone(tracks),
                bounds: frame.traces.bounds,
                camera: frame.traces.camera,
                scale_factor: frame.traces.scale_factor,
                colors: Arc::clone(&frame.traces.colors),
                fallback_color: frame.traces.fallback_color,
                highlighted_objects: None,
                highlighted_net: None,
                highlighted_trace: None,
                highlighted_object: None,
                hovered_object: None,
                highlighted_related_objects: None,
            });
        if text_frame.is_none() {
            self.texts.clear_if_cached();
        }
        self.traces.prepare(context, &frame.traces)?;
        let copper_ready = self
            .copper
            .prepare(context, &frame.copper, allow_copper_upload)?;
        if allow_pad_upload && let Some(pads) = &frame.pads {
            self.pads.prepare(context, pads)?;
        }
        let pads_ready = frame
            .pads
            .as_ref()
            .is_some_and(|source| self.pads.has_source(source));
        let custom_source = frame
            .pads
            .as_ref()
            .and_then(|pads| pads.custom_mesh.as_ref());
        let custom_ready = if let Some(source) = custom_source {
            self.custom_pads
                .prepare(context, source, allow_custom_upload)?
        } else {
            false
        };
        let mut seen = std::collections::BTreeSet::new();
        if allow_drill_upload && let Some(drills) = &frame.drills {
            self.drills.prepare(context, drills)?;
        }
        if allow_drawing_upload && let Some(drawing_frame) = &drawing_frame {
            self.drawings.prepare(context, drawing_frame)?;
        }
        if allow_text_upload && let Some(text_frame) = &text_frame {
            self.texts.prepare(context, text_frame)?;
        }
        let mut layers = Vec::new();
        for layer in frame
            .layer_order
            .iter()
            .copied()
            .chain(
                frame
                    .traces
                    .tracks
                    .batches
                    .iter()
                    .filter(|batch| !batch.outline)
                    .map(|batch| batch.layer),
            )
            .chain(frame.copper.batches.iter().map(|batch| batch.layer))
            .chain(
                frame
                    .texts
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
            .chain(
                frame
                    .drawings
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
            .chain(
                frame
                    .pads
                    .iter()
                    .flat_map(|pads| pads.batches.iter().map(|batch| batch.layer)),
            )
            .chain(
                custom_source
                    .into_iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
        {
            if seen.insert(layer) {
                layers.push(layer);
            }
        }
        self.copper
            .telemetry
            .visible_zones
            .store(0, Ordering::Relaxed);
        let mut zones = 0;
        let mut custom_zones = 0;
        for layer in layers {
            if !frame.display.layer_visible(layer) {
                continue;
            }
            if copper_ready && frame.display.show_copper {
                self.copper.draw_prepared(
                    context,
                    &frame.traces,
                    frame.copper_opacity,
                    Some(layer),
                )?;
                zones += self.copper.telemetry.visible_zones.load(Ordering::Relaxed);
            }
            self.traces.draw_prepared(
                context,
                &frame.traces,
                super::board::TraceScope::Layer(layer),
            )?;
            if pads_ready {
                self.pads
                    .draw_prepared(context, &frame.traces, Some(layer))?;
            }
            if custom_ready {
                self.custom_pads
                    .draw_prepared(context, &frame.traces, 1.0, Some(layer))?;
                custom_zones += self
                    .custom_pads
                    .telemetry
                    .visible_zones
                    .load(Ordering::Relaxed);
            }
            if let Some(drawing_frame) = &drawing_frame
                && frame.display.show_drawings
                && self.drawings.is_uploaded(&drawing_frame.tracks)
            {
                self.drawings.draw_prepared(
                    context,
                    drawing_frame,
                    super::board::TraceScope::Layer(layer),
                )?;
            }
            if let Some(text_frame) = &text_frame
                && frame.display.show_texts
                && self.texts.is_uploaded(&text_frame.tracks)
            {
                self.texts.draw_prepared(
                    context,
                    text_frame,
                    super::board::TraceScope::Layer(layer),
                )?;
            }
        }
        self.copper
            .telemetry
            .visible_zones
            .store(zones, Ordering::Relaxed);
        self.custom_pads
            .telemetry
            .visible_zones
            .store(custom_zones, Ordering::Relaxed);
        self.traces
            .draw_prepared(context, &frame.traces, super::board::TraceScope::Outline)?;
        if let Some(source) = &frame.drills
            && frame.display.show_drills
            && self.drills.has_source(source)
        {
            let drill_frame = TraceFrame {
                tracks: Arc::clone(&frame.traces.tracks),
                bounds: frame.traces.bounds,
                camera: frame.traces.camera,
                scale_factor: frame.traces.scale_factor,
                colors: Arc::new(std::collections::BTreeMap::new()),
                fallback_color: frame.drill_color,
                highlighted_net: None,
                highlighted_objects: None,
                highlighted_related_objects: None,
                hovered_object: None,
                highlighted_trace: None,
                highlighted_object: None,
            };
            let visible = |instance: &crate::pads::PadInstance| {
                let scope = if instance.source[0] == 0 {
                    None
                } else {
                    Some(
                        source
                            .drill_scopes
                            .as_ref()
                            .and_then(|scopes| scopes.get(instance.source[1] as usize))
                            .map_or(&[][..], Vec::as_slice),
                    )
                };
                frame.display.drill_visible(scope)
            };
            self.drills
                .draw_filtered(context, &drill_frame, None, Some(&visible))?;
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.traces.reset();
        self.drawings.reset();
        self.texts.reset();
        self.copper.reset();
        self.pads.reset();
        self.custom_pads.reset();
        self.drills.reset();
    }
}
