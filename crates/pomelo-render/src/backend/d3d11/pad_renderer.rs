//! Analytic pad resources, separated from GPUI's generic callback.
use super::{
    TraceFrame, TraceTelemetry,
    pad_d3d11::{Pipeline, UploadedPads},
};
use crate::pads::PreparedPads;
use anyhow::Context as _;
use gpui::NativeGpuContext;
use pomelo_core::model::LayerId;
use std::sync::{Arc, atomic::Ordering};

pub struct PadRenderer {
    pipeline: Option<Pipeline>,
    uploaded: Option<UploadedPads>,
    telemetry: Arc<TraceTelemetry>,
}
impl PadRenderer {
    pub(super) fn is_uploaded(&self, source: &Arc<PreparedPads>) -> bool {
        self.uploaded.as_ref().is_some_and(|cache| {
            Arc::ptr_eq(&cache.source, source) && cache.uploaded() == source.analytic.len()
        })
    }
    pub(super) fn has_source(&self, source: &Arc<PreparedPads>) -> bool {
        self.pipeline.is_some()
            && self
                .uploaded
                .as_ref()
                .is_some_and(|cache| Arc::ptr_eq(&cache.source, source))
    }
    pub fn new(telemetry: Arc<TraceTelemetry>) -> Self {
        Self {
            pipeline: None,
            uploaded: None,
            telemetry,
        }
    }
    /// Called once per frame; at most 4 MiB of analytic instances is uploaded.
    pub fn prepare(
        &mut self,
        context: &NativeGpuContext<'_>,
        source: &Arc<PreparedPads>,
    ) -> anyhow::Result<()> {
        if self
            .uploaded
            .as_ref()
            .is_none_or(|cache| !Arc::ptr_eq(&cache.source, source))
        {
            self.uploaded = Some(UploadedPads::new(Arc::clone(source))?);
            self.telemetry.cache_builds.fetch_add(1, Ordering::Relaxed);
            self.telemetry
                .uploaded_instances
                .store(0, Ordering::Release);
        }
        let cache = self.uploaded.as_mut().context("GPU_PAD_CACHE_MISSING")?;
        let before = cache.uploaded();
        let result = cache.upload_next(context.device);
        self.telemetry.uploaded_bytes.fetch_add(
            ((cache.uploaded() - before) * std::mem::size_of::<crate::pads::PadInstance>()) as u64,
            Ordering::Relaxed,
        );
        self.telemetry
            .uploaded_instances
            .store(cache.uploaded() as u64, Ordering::Release);
        result?;
        if self.pipeline.is_none() {
            self.pipeline = Some(Pipeline::new(context.device)?);
            self.telemetry
                .pipeline_builds
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    /// Draw a selected layer, or all layers, without uploading geometry again.
    pub fn draw_prepared(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        layer: Option<LayerId>,
    ) -> anyhow::Result<()> {
        self.draw_filtered(context, frame, layer, None)
    }
    pub(super) fn draw_filtered(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        layer: Option<LayerId>,
        visible: Option<&dyn Fn(&crate::pads::PadInstance) -> bool>,
    ) -> anyhow::Result<()> {
        let cache = self.uploaded.as_ref().context("GPU_PAD_CACHE_MISSING")?;
        let draws = self
            .pipeline
            .as_ref()
            .context("GPU_PAD_PIPELINE_MISSING")?
            .draw(context, frame, cache, layer, visible)?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        Ok(())
    }
    pub fn reset(&mut self) {
        self.pipeline = None;
        self.uploaded = None;
        self.telemetry
            .uploaded_instances
            .store(0, Ordering::Release);
        self.telemetry.resets.fetch_add(1, Ordering::Relaxed);
    }
}
