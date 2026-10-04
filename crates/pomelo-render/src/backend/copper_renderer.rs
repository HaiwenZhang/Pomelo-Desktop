//! Copper resource lifecycle and telemetry, independent of the generic GPUI patch.

use super::{NativeGpuContext, NativeGpuRenderer};
use super::{
    TraceFrame, TraceRenderer, TraceTelemetry,
    copper_pipeline::{Pipeline, UploadedCopper},
};
use crate::copper::PreparedCopper;
use anyhow::Context as _;
use std::{
    any::Any,
    collections::BTreeMap,
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
    curve_cache_builds: AtomicU64,
    curve_uploaded_bytes: AtomicU64,
    curve_required_bytes: AtomicU64,
    curve_cached_bytes: AtomicU64,
    curve_lifetime_uploaded_bytes: AtomicU64,
    curve_revision: AtomicU64,
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
    pub curve_cache_builds: u64,
    pub curve_uploaded_bytes: u64,
    pub curve_required_bytes: u64,
    pub curve_cached_bytes: u64,
    pub curve_lifetime_uploaded_bytes: u64,
    pub curve_revision: u64,
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
            curve_cache_builds: self.curve_cache_builds.load(Ordering::Relaxed),
            curve_uploaded_bytes: self.curve_uploaded_bytes.load(Ordering::Acquire),
            curve_required_bytes: self.curve_required_bytes.load(Ordering::Relaxed),
            curve_cached_bytes: self.curve_cached_bytes.load(Ordering::Relaxed),
            curve_lifetime_uploaded_bytes: self
                .curve_lifetime_uploaded_bytes
                .load(Ordering::Relaxed),
            curve_revision: self.curve_revision.load(Ordering::Acquire),
        }
    }
}

pub struct CopperRenderer {
    pipeline: Option<Pipeline>,
    uploaded: Option<UploadedCopper>,
    curves: BTreeMap<pomelo_core::model::ObjectId, UploadedCopper>,
    telemetry: Arc<CopperTelemetry>,
    static_shapes_fill_solid: bool,
    network_selection: Option<pomelo_core::model::NetId>,
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
            curves: BTreeMap::new(),
            telemetry,
            // Standalone geometry callers and custom pad meshes retain solid fill.
            static_shapes_fill_solid: true,
            network_selection: None,
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
            self.curves.clear();
            self.telemetry.curve_revision.store(0, Ordering::Release);
            self.telemetry
                .curve_uploaded_bytes
                .store(0, Ordering::Release);
            self.telemetry
                .curve_required_bytes
                .store(0, Ordering::Relaxed);
            self.telemetry
                .curve_cached_bytes
                .store(0, Ordering::Relaxed);
            self.telemetry.uploaded_bytes.store(0, Ordering::Release);
            self.telemetry.visible_zones.store(0, Ordering::Relaxed);
            if !allow_upload {
                return Ok(false);
            }
            self.uploaded = Some(UploadedCopper::new(Arc::clone(source), &context.device)?);
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
            self.pipeline = Some(Pipeline::new(&context.device)?);
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
        self.draw_filtered(context, frame, opacity, layer, None)
    }
    pub(super) fn prepare_curves(
        &mut self,
        context: &NativeGpuContext<'_>,
        source: Option<&crate::scene::curves::CurveFillCache>,
        allow_upload: bool,
    ) -> anyhow::Result<()> {
        for entry in self.curves.values_mut() {
            entry.active = false;
        }
        if let Some(source) = source {
            self.curves.retain(|id, gpu| {
                source
                    .entries
                    .get(id)
                    .is_some_and(|cpu| Arc::ptr_eq(&gpu.source, &cpu.source))
            });
            let mut budget = super::copper_pipeline::UPLOAD_BYTES_PER_FRAME;
            for id in &source.active {
                let cpu = source.entries.get(id).context("GPU_CURVE_ENTRY_MISSING")?;
                if !self.curves.contains_key(id) && allow_upload {
                    self.curves.insert(
                        *id,
                        UploadedCopper::new(Arc::clone(&cpu.source), &context.device)?,
                    );
                    self.telemetry
                        .curve_cache_builds
                        .fetch_add(1, Ordering::Relaxed);
                }
                if let Some(gpu) = self.curves.get_mut(id) {
                    gpu.active = true;
                    if allow_upload {
                        let before = gpu.uploaded_bytes();
                        gpu.upload_with_budget(context, &mut budget)?;
                        self.telemetry
                            .curve_lifetime_uploaded_bytes
                            .fetch_add((gpu.uploaded_bytes() - before) as u64, Ordering::Relaxed);
                    }
                }
            }
        }
        self.telemetry.curve_required_bytes.store(
            source.map_or(0, |s| {
                s.active
                    .iter()
                    .filter_map(|id| s.entries.get(id))
                    .map(|e| e.source.upload_bytes() as u64)
                    .sum()
            }),
            Ordering::Relaxed,
        );
        self.telemetry.curve_uploaded_bytes.store(
            self.curves
                .values()
                .filter(|e| e.active)
                .map(|e| e.uploaded_bytes() as u64)
                .sum(),
            Ordering::Release,
        );
        self.telemetry.curve_cached_bytes.store(
            self.curves
                .values()
                .map(|e| e.source.upload_bytes() as u64)
                .sum(),
            Ordering::Relaxed,
        );
        self.telemetry.curve_revision.store(
            source.map_or(0, crate::scene::curves::CurveFillCache::revision),
            Ordering::Release,
        );
        Ok(())
    }
    fn draw_filtered(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        opacity: f32,
        layer: Option<pomelo_core::model::LayerId>,
        visible: Option<&dyn Fn(&crate::copper::CopperBatch) -> bool>,
    ) -> anyhow::Result<()> {
        let cache = self.uploaded.as_ref().context("GPU_COPPER_CACHE_MISSING")?;
        let (draws, zones) = self
            .pipeline
            .as_mut()
            .context("GPU_COPPER_PIPELINE_MISSING")?
            .draw(
                context,
                frame,
                cache,
                super::copper_pipeline::CopperDrawOptions {
                    opacity,
                    layer,
                    visible,
                    annotations: None,
                    overrides: Some(&self.curves),
                    static_shapes_fill_solid: self.static_shapes_fill_solid,
                    network_selection: self.network_selection,
                },
            )?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        self.telemetry.visible_zones.store(zones, Ordering::Relaxed);
        Ok(())
    }
    pub fn reset(&mut self) {
        self.pipeline = None;
        self.uploaded = None;
        self.curves.clear();
        self.telemetry.curve_revision.store(0, Ordering::Release);
        self.telemetry
            .curve_uploaded_bytes
            .store(0, Ordering::Release);
        self.telemetry
            .curve_required_bytes
            .store(0, Ordering::Relaxed);
        self.telemetry
            .curve_cached_bytes
            .store(0, Ordering::Relaxed);
        self.telemetry.uploaded_bytes.store(0, Ordering::Release);
        self.telemetry.visible_zones.store(0, Ordering::Relaxed);
        self.telemetry.resets.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn draw_annotated(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        opacity: f32,
        layer: pomelo_core::model::LayerId,
        annotations: &mut dyn FnMut(&crate::copper::CopperBatch) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let cache = self.uploaded.as_ref().context("GPU_COPPER_CACHE_MISSING")?;
        let (draws, zones) = self
            .pipeline
            .as_mut()
            .context("GPU_COPPER_PIPELINE_MISSING")?
            .draw(
                context,
                frame,
                cache,
                super::copper_pipeline::CopperDrawOptions {
                    opacity,
                    layer: Some(layer),
                    visible: None,
                    annotations: Some(annotations),
                    overrides: Some(&self.curves),
                    static_shapes_fill_solid: self.static_shapes_fill_solid,
                    network_selection: self.network_selection,
                },
            )?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        self.telemetry.visible_zones.store(zones, Ordering::Relaxed);
        Ok(())
    }
}

pub struct BoardFrame {
    pub curves: Option<Arc<crate::scene::curves::CurveFillCache>>,
    pub glyphs: Option<Arc<crate::text::msdf::PreparedGlyphs>>,
    pub labels: Option<Arc<crate::text::msdf::PreparedGlyphs>>,
    pub zone_outlines: Option<Arc<crate::tracks::PreparedTracks>>,
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
    glyphs: TraceRenderer<crate::text::msdf::PreparedGlyphs>,
    labels: TraceRenderer<crate::text::msdf::PreparedGlyphs>,
    zone_outlines: TraceRenderer,
    traces: TraceRenderer,
    drawings: TraceRenderer,
    texts: TraceRenderer<crate::text_instances::PreparedTextInstances>,
    copper: CopperRenderer,
    pads: super::PadRenderer,
    custom_pads: CopperRenderer,
    custom_outlines: TraceRenderer,
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
            glyphs: TraceRenderer::new(Arc::clone(&texts)),
            labels: TraceRenderer::new(Arc::new(TraceTelemetry::default())),
            zone_outlines: TraceRenderer::new(Arc::new(TraceTelemetry::default())),
            traces: TraceRenderer::new(traces),
            drawings: TraceRenderer::new(drawings),
            texts: TraceRenderer::new(texts),
            copper: CopperRenderer::new(copper),
            pads: super::PadRenderer::new(pads),
            custom_pads: CopperRenderer::new(custom_pads),
            custom_outlines: TraceRenderer::new(Arc::new(TraceTelemetry::default())),
            drills: super::PadRenderer::new(drills),
        }
    }
    pub fn with_source_stroke_telemetry(mut self, telemetry: Arc<TraceTelemetry>) -> Self {
        self.texts = TraceRenderer::new(telemetry);
        self
    }
    pub fn with_label_telemetry(mut self, telemetry: Arc<TraceTelemetry>) -> Self {
        self.labels = TraceRenderer::new(telemetry);
        self
    }
    pub fn with_custom_outline_telemetry(mut self, telemetry: Arc<TraceTelemetry>) -> Self {
        self.custom_outlines = TraceRenderer::new(telemetry);
        self
    }
    pub fn with_zone_outline_telemetry(mut self, telemetry: Arc<TraceTelemetry>) -> Self {
        self.zone_outlines = TraceRenderer::new(telemetry);
        self
    }

    /// Test-only aggregate of production readiness checks for geometric slots.
    /// Text, labels and optional curve-LOD caches are intentionally outside this proof.
    #[cfg(all(test, target_os = "windows"))]
    pub(super) fn geometry_is_uploaded(&self, frame: &BoardFrame) -> bool {
        self.traces.is_uploaded(&frame.traces.tracks)
            && self.copper.is_uploaded(&frame.copper)
            && frame.pads.as_ref().is_none_or(|source| {
                self.pads.is_uploaded(source)
                    && source
                        .custom_mesh
                        .as_ref()
                        .is_none_or(|mesh| self.custom_pads.is_uploaded(mesh))
                    && source
                        .custom_outlines
                        .as_ref()
                        .is_none_or(|edges| self.custom_outlines.is_uploaded(edges))
            })
            && frame
                .drills
                .as_ref()
                .is_none_or(|source| self.drills.is_uploaded(source))
            && frame
                .zone_outlines
                .as_ref()
                .is_none_or(|source| self.zone_outlines.is_uploaded(source))
            && frame
                .drawings
                .as_ref()
                .filter(|source| !source.instances.is_empty())
                .is_none_or(|source| self.drawings.is_uploaded(source))
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
        self.copper.static_shapes_fill_solid = frame.display.static_shapes_fill_solid;
        // Keep selection identity separate from base material colors. Standalone
        // copper tinting and unknown zone styling retain their existing contract.
        self.copper.network_selection = frame
            .traces
            .highlighted_net
            .filter(|(net, _)| net.0 != 0)
            .map(|(net, _)| net);
        // Defer copper uploads until this exact trace source is uploaded. Combined application
        // upload traffic remains <= 4 MiB/frame, including scene revisions and device recovery.
        let allow_copper_upload = self.traces.is_uploaded(&frame.traces.tracks);
        let allow_pad_upload = allow_copper_upload && self.copper.is_uploaded(&frame.copper);
        let allow_custom_upload = allow_pad_upload
            && frame
                .pads
                .as_ref()
                .is_none_or(|source| self.pads.is_uploaded(source));
        let allow_custom_outline_upload = allow_custom_upload
            && frame
                .pads
                .as_ref()
                .and_then(|pads| pads.custom_mesh.as_ref())
                .is_none_or(|source| self.custom_pads.is_uploaded(source));
        let custom_outline_frame = frame
            .pads
            .as_ref()
            .and_then(|pads| pads.custom_outlines.as_ref())
            .map(|tracks| frame.traces.with_source(Arc::clone(tracks)));
        let allow_drill_upload = allow_custom_outline_upload
            && custom_outline_frame
                .as_ref()
                .is_none_or(|source| self.custom_outlines.is_uploaded(&source.tracks));
        let allow_outline_upload = allow_drill_upload
            && frame
                .drills
                .as_ref()
                .is_none_or(|source| self.drills.is_uploaded(source));
        let outline_frame = frame.zone_outlines.as_ref().map(|tracks| {
            let mut traces = frame.traces.clone();
            traces.tracks = Arc::clone(tracks);
            traces
        });
        let allow_drawing_upload = allow_outline_upload
            && outline_frame
                .as_ref()
                .is_none_or(|outline| self.zone_outlines.is_uploaded(&outline.tracks));
        let drawing_frame = frame
            .drawings
            .as_ref()
            .filter(|tracks| !tracks.instances.is_empty())
            .map(|tracks| {
                let mut drawing = frame.traces.with_source(Arc::clone(tracks));
                drawing.color_mode = pomelo_core::display::ColorMode::Layer;
                drawing
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
            .map(|tracks| {
                let mut text = frame.traces.with_source(Arc::clone(tracks));
                text.filled = true;
                text.color_mode = pomelo_core::display::ColorMode::Layer;
                text
            });
        if text_frame.is_none() {
            self.texts.clear_if_cached();
        }
        let glyph_frame = frame
            .glyphs
            .as_ref()
            .map(|source| frame.traces.with_source(Arc::clone(source)));
        let label_frame = frame.labels.as_ref().map(|source| {
            let mut labels = frame.traces.base().with_source(Arc::clone(source));
            labels.opacity = frame.display.global_opacity;
            labels
        });
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
        if allow_custom_outline_upload && let Some(source) = &custom_outline_frame {
            self.custom_outlines.prepare(context, source)?;
        }
        if custom_outline_frame.is_none() {
            self.custom_outlines.clear_if_cached();
        }
        let mut seen = std::collections::BTreeSet::new();
        if allow_drill_upload && let Some(drills) = &frame.drills {
            self.drills.prepare(context, drills)?;
        }
        if allow_outline_upload && let Some(outline_frame) = &outline_frame {
            self.zone_outlines.prepare(context, outline_frame)?;
        }
        if outline_frame.is_none() {
            self.zone_outlines.clear_if_cached();
        }
        if allow_drawing_upload && let Some(drawing_frame) = &drawing_frame {
            self.drawings.prepare(context, drawing_frame)?;
        }
        if allow_text_upload && let Some(text_frame) = &text_frame {
            self.texts.prepare(context, text_frame)?;
        }
        let allow_glyph_upload = allow_text_upload
            && text_frame
                .as_ref()
                .is_none_or(|text| self.texts.is_uploaded(&text.tracks));
        if allow_glyph_upload && let Some(glyph) = &glyph_frame {
            self.glyphs.prepare(context, glyph)?;
        }
        let allow_label_upload = allow_glyph_upload
            && glyph_frame
                .as_ref()
                .is_none_or(|glyph| self.glyphs.is_uploaded(&glyph.tracks));
        if allow_label_upload && let Some(label) = &label_frame {
            self.labels.prepare(context, label)?;
        }
        if glyph_frame.is_none() {
            self.glyphs.clear_if_cached();
        }
        if label_frame.is_none() {
            self.labels.clear_if_cached();
        }
        let allow_curve_upload = allow_label_upload
            && label_frame
                .as_ref()
                .is_none_or(|label| self.labels.is_uploaded(&label.tracks));
        self.copper
            .prepare_curves(context, frame.curves.as_deref(), allow_curve_upload)?;
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
                    .glyphs
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
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
            .chain(
                custom_outline_frame
                    .iter()
                    .flat_map(|frame| frame.tracks.batches.iter().map(|batch| batch.layer)),
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
        use super::board::{OverlayPass, TraceScope};
        use pomelo_core::display::{DisplayCategory as Category, LayerPrimitive};
        let mut base = frame.traces.base();
        base.opacity = frame.display.global_opacity;
        self.traces
            .draw_prepared(context, &base, TraceScope::Outline)?;
        let mut commands: Vec<_> = layers
            .iter()
            .copied()
            .flat_map(|layer| {
                [
                    Category::Drawing,
                    Category::Zone,
                    Category::ZoneOutline,
                    Category::Trace,
                    Category::Text,
                    Category::Pin,
                    Category::Via,
                ]
                .map(|category| (layer, category))
            })
            .collect();
        commands.push((pomelo_core::model::LayerId::UNASSIGNED, Category::Drill));
        commands.sort_by_key(|&(layer, category)| frame.display.display_rank(layer, category));
        let mut zones = 0;
        let mut custom_zones = 0;
        for pass in [
            OverlayPass::Base,
            OverlayPass::Selection,
            OverlayPass::Hover,
        ] {
            let mut traces = if pass == OverlayPass::Base {
                base.clone()
            } else {
                frame.traces.clone()
            };
            traces.filled = frame.display.filled;
            traces.pass = if pass == OverlayPass::Hover
                && traces.hover_selection.as_ref().is_some_and(|(target, _)| {
                    !matches!(target, pomelo_core::selection::SelectionTarget::Object(_))
                }) {
                OverlayPass::GroupHover
            } else {
                pass
            };
            for &(layer, category) in &commands {
                if category != Category::Drill && !frame.display.layer_visible(layer) {
                    continue;
                }
                traces.material_override = frame.display.appearance.material(layer, category);
                // Allegro's shapes slider overrides global alpha for shape fill;
                // ordinary geometry and labels use the independent global value.
                traces.opacity = if pass != OverlayPass::Base
                    || matches!(category, Category::Zone | Category::ZoneOutline)
                {
                    1.0
                } else {
                    frame.display.global_opacity
                };
                match category {
                    Category::Zone
                        if copper_ready
                            && frame.display.show_copper
                            && frame.copper_opacity > 0.0
                            && pass != OverlayPass::Hover =>
                    {
                        if pass == OverlayPass::Base
                            && let Some(label) = &label_frame
                            && self.labels.is_uploaded(&label.tracks)
                        {
                            self.copper.draw_annotated(
                                context,
                                &traces,
                                frame.copper_opacity,
                                layer,
                                &mut |batch| {
                                    self.labels.draw_prepared(
                                        context,
                                        label,
                                        TraceScope::Labels(
                                            layer,
                                            Category::Zone,
                                            Some(batch.object),
                                        ),
                                    )
                                },
                            )?;
                        } else {
                            self.copper.draw_prepared(
                                context,
                                &traces,
                                frame.copper_opacity,
                                Some(layer),
                            )?;
                        }
                        if pass == OverlayPass::Base {
                            zones += self.copper.telemetry.visible_zones.load(Ordering::Relaxed);
                        }
                    }
                    Category::ZoneOutline if frame.display.show_copper => {
                        if let Some(source) = &outline_frame
                            && self.zone_outlines.is_uploaded(&source.tracks)
                        {
                            let mut outline = traces.clone();
                            outline.tracks = Arc::clone(&source.tracks);
                            let scope = if pass == OverlayPass::Base {
                                // Controlled Allegro captures retain Dynamic hairlines at25/255,
                                // and suppress independent bright boundaries starting at26/255.
                                TraceScope::ZoneOutlines(
                                    layer,
                                    frame.copper_opacity <= 25.0 / 255.0,
                                )
                            } else {
                                TraceScope::Layer(layer)
                            };
                            self.zone_outlines.draw_prepared(context, &outline, scope)?;
                        }
                    }
                    Category::Trace
                        if layer != pomelo_core::model::LayerId::BOND_TOP
                            && frame
                                .display
                                .primitive_visible(layer, LayerPrimitive::Traces) =>
                    {
                        self.traces
                            .draw_prepared(context, &traces, TraceScope::Layer(layer))?;
                    }
                    Category::Pin | Category::Via | Category::Trace => {
                        // The importer maps die pads to BOND_TOP. Web submits their pin
                        // geometry with etch, including custom fills and unfilled outlines.
                        let die_layer = layer == pomelo_core::model::LayerId::BOND_TOP;
                        if (category == Category::Trace && !die_layer)
                            || (category == Category::Pin && die_layer)
                        {
                            continue;
                        }
                        let kind = match category {
                            Category::Trace => LayerPrimitive::Traces,
                            Category::Via => LayerPrimitive::Vias,
                            _ => LayerPrimitive::Pads,
                        };
                        if !frame.display.primitive_visible(layer, kind) {
                            continue;
                        }
                        if category == Category::Trace {
                            self.traces.draw_prepared(
                                context,
                                &traces,
                                TraceScope::Layer(layer),
                            )?;
                        }
                        let pin = category != Category::Via;
                        if pads_ready {
                            let visible = |instance: &crate::pads::PadInstance| {
                                (instance.source[0] == 0) == pin
                                    && (instance.source[3] & 4 == 0
                                        || !frame.display.show_backdrills)
                            };
                            self.pads.draw_filtered(
                                context,
                                &traces,
                                Some(layer),
                                Some(&visible),
                            )?;
                        }
                        if custom_ready && frame.display.filled {
                            let visible = |batch: &crate::copper::CopperBatch| {
                                matches!(
                                    batch.selected_object,
                                    pomelo_core::selection::SelectedObject::Pin(_)
                                ) == pin
                            };
                            self.custom_pads.draw_filtered(
                                context,
                                &traces,
                                1.0,
                                Some(layer),
                                Some(&visible),
                            )?;
                            if pass == OverlayPass::Base {
                                custom_zones += self
                                    .custom_pads
                                    .telemetry
                                    .visible_zones
                                    .load(Ordering::Relaxed);
                            }
                        }
                        if (!frame.display.filled || pass != OverlayPass::Base)
                            && let Some(source) = &custom_outline_frame
                            && self.custom_outlines.is_uploaded(&source.tracks)
                        {
                            self.custom_outlines.draw_prepared(
                                context,
                                &traces.with_source(Arc::clone(&source.tracks)),
                                TraceScope::Pads(layer, pin),
                            )?;
                        }
                    }
                    Category::Drawing if frame.display.show_drawings => {
                        if let Some(drawing) = &drawing_frame
                            && self.drawings.is_uploaded(&drawing.tracks)
                        {
                            self.drawings.draw_prepared(
                                context,
                                &{
                                    let mut drawing = drawing.clone();
                                    drawing.pass = pass;
                                    drawing.material_override = traces.material_override;
                                    drawing.opacity = traces.opacity;
                                    drawing
                                },
                                TraceScope::Layer(layer),
                            )?;
                        }
                    }
                    Category::Text if frame.display.show_texts => {
                        if let Some(glyph) = &glyph_frame
                            && self.glyphs.is_uploaded(&glyph.tracks)
                        {
                            self.glyphs.draw_prepared(
                                context,
                                &{
                                    let mut glyph = glyph.clone();
                                    glyph.pass = pass;
                                    glyph.material_override = traces.material_override;
                                    glyph.opacity = traces.opacity;
                                    glyph
                                },
                                TraceScope::Layer(layer),
                            )?;
                        }
                        if let Some(text) = &text_frame
                            && self.texts.is_uploaded(&text.tracks)
                        {
                            self.texts.draw_prepared(
                                context,
                                &{
                                    let mut text = text.clone();
                                    text.pass = traces.pass;
                                    text.material_override = traces.material_override;
                                    text.opacity = traces.opacity;
                                    text
                                },
                                TraceScope::Layer(layer),
                            )?;
                        }
                    }
                    Category::Drill => {
                        if let Some(source) = &frame.drills
                            && (frame.display.show_drills || frame.display.show_backdrills)
                            && self.drills.has_source(source)
                        {
                            let mut drill_frame = traces.clone();
                            drill_frame.color_mode = pomelo_core::display::ColorMode::Layer;
                            drill_frame.material_override =
                                Some(traces.material_override.unwrap_or(frame.drill_color));
                            drill_frame.filled = true;
                            let visible = |instance: &crate::pads::PadInstance| {
                                let backdrill = instance.source[3] & 2 != 0;
                                let scopes = if backdrill {
                                    &source.backdrill_scopes
                                } else {
                                    &source.drill_scopes
                                };
                                let scope = if instance.source[0] == 0 {
                                    None
                                } else {
                                    Some(
                                        scopes
                                            .as_ref()
                                            .and_then(|scopes| {
                                                scopes.get(instance.source[1] as usize)
                                            })
                                            .map_or(&[][..], Vec::as_slice),
                                    )
                                };
                                if backdrill {
                                    frame.display.backdrill_visible(scope.unwrap_or(&[]))
                                } else {
                                    frame.display.drill_visible(scope)
                                }
                            };
                            self.drills.draw_filtered(
                                context,
                                &drill_frame,
                                None,
                                Some(&visible),
                            )?;
                        }
                        if pass == OverlayPass::Base
                            && let Some(label) = &label_frame
                            && self.labels.is_uploaded(&label.tracks)
                        {
                            self.labels.draw_prepared(
                                context,
                                label,
                                TraceScope::Labels(layer, Category::Drill, None),
                            )?;
                        }
                    }
                    _ => {}
                }
                if pass == OverlayPass::Base
                    && matches!(category, Category::Trace | Category::Pin)
                    && let Some(label) = &label_frame
                    && self.labels.is_uploaded(&label.tracks)
                {
                    let primitive = if category == Category::Trace {
                        LayerPrimitive::Traces
                    } else {
                        LayerPrimitive::Pads
                    };
                    if frame.display.primitive_visible(layer, primitive) {
                        self.labels.draw_prepared(
                            context,
                            label,
                            TraceScope::Labels(layer, category, None),
                        )?;
                    }
                }
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
        Ok(())
    }
    fn reset(&mut self) {
        self.glyphs.clear_if_cached();
        self.labels.reset();
        self.zone_outlines.reset();
        self.traces.reset();
        self.drawings.reset();
        self.texts.clear_if_cached();
        self.copper.reset();
        self.pads.reset();
        self.custom_pads.reset();
        self.custom_outlines.clear_if_cached();
        self.drills.reset();
    }
}
