//! Real scene trace renderer. The GPUI callback only transports this opaque frame.

use super::trace_d3d11::{InstanceSource, Pipeline, UploadedTracks};
use crate::tracks::PreparedTracks;
use anyhow::Context as _;
use gpui::{NativeGpuContext, NativeGpuRenderer};
use pomelo_core::{
    interaction::Camera,
    model::{Bounds, LayerId},
};
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy)]
pub(super) enum TraceScope {
    All,
    Layer(LayerId),
    Outline,
    /// Custom pad edges filtered by owner category (pin=true / via=false).
    Pads(LayerId, bool),
    Labels(
        LayerId,
        pomelo_core::display::DisplayCategory,
        Option<pomelo_core::model::ObjectId>,
    ),
}

pub struct TraceFrame<S: InstanceSource = PreparedTracks> {
    pub pass: OverlayPass,
    pub filled: bool,
    pub hover_selection: Option<(
        pomelo_core::selection::SelectionTarget,
        Arc<std::collections::BTreeSet<pomelo_core::selection::SelectedObject>>,
    )>,
    pub color_mode: pomelo_core::display::ColorMode,
    pub tracks: Arc<S>,
    pub bounds: Bounds,
    /// Optional camera in viewport-local logical pixels; None fits the complete board.
    pub camera: Option<Camera>,
    pub scale_factor: f32,
    pub colors: Arc<BTreeMap<LayerId, [f32; 4]>>,
    pub fallback_color: [f32; 4],
    pub highlighted_objects: Option<(
        Arc<std::collections::BTreeSet<pomelo_core::model::ObjectId>>,
        [f32; 4],
    )>,
    pub highlighted_net: Option<(pomelo_core::model::NetId, [f32; 4])>,
    pub highlighted_trace: Option<(TraceSelection, [f32; 4])>,
    pub highlighted_object: Option<(pomelo_core::selection::SelectedObject, [f32; 4])>,
    pub hovered_object: Option<(pomelo_core::selection::SelectedObject, [f32; 4])>,
    pub highlighted_related_objects: Option<(
        Arc<std::collections::BTreeSet<pomelo_core::selection::SelectedObject>>,
        [f32; 4],
    )>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OverlayPass {
    #[default]
    Base = 0,
    Selection = 1,
    Hover = 2,
    GroupHover = 3,
}

impl<S: InstanceSource> Clone for TraceFrame<S> {
    fn clone(&self) -> Self {
        Self {
            pass: self.pass,
            filled: self.filled,
            hover_selection: self.hover_selection.clone(),
            color_mode: self.color_mode,
            tracks: Arc::clone(&self.tracks),
            bounds: self.bounds,
            camera: self.camera,
            scale_factor: self.scale_factor,
            colors: Arc::clone(&self.colors),
            fallback_color: self.fallback_color,
            highlighted_objects: self.highlighted_objects.clone(),
            highlighted_net: self.highlighted_net,
            highlighted_trace: self.highlighted_trace,
            highlighted_object: self.highlighted_object,
            hovered_object: self.hovered_object,
            highlighted_related_objects: self.highlighted_related_objects.clone(),
        }
    }
}

impl<S: InstanceSource> TraceFrame<S> {
    pub(super) fn with_source<T: InstanceSource>(&self, tracks: Arc<T>) -> TraceFrame<T> {
        TraceFrame {
            tracks,
            pass: self.pass,
            filled: self.filled,
            hover_selection: self.hover_selection.clone(),
            color_mode: self.color_mode,
            bounds: self.bounds,
            camera: self.camera,
            scale_factor: self.scale_factor,
            colors: Arc::clone(&self.colors),
            fallback_color: self.fallback_color,
            highlighted_objects: self.highlighted_objects.clone(),
            highlighted_net: self.highlighted_net,
            highlighted_trace: self.highlighted_trace,
            highlighted_object: self.highlighted_object,
            hovered_object: self.hovered_object,
            highlighted_related_objects: self.highlighted_related_objects.clone(),
        }
    }
    pub(super) fn selected(
        &self,
        object: pomelo_core::selection::SelectedObject,
        net: pomelo_core::model::NetId,
        track: Option<pomelo_core::model::ObjectId>,
    ) -> bool {
        use pomelo_core::selection::SelectedObject;
        self.highlighted_object.is_some_and(|(id, _)| id == object)
            || self
                .highlighted_related_objects
                .as_ref()
                .is_some_and(|(ids, _)| ids.contains(&object))
            || matches!(object, SelectedObject::Pin(id) if self.highlighted_objects.as_ref().is_some_and(|(ids, _)| ids.contains(&id)))
            || self
                .highlighted_net
                .is_some_and(|(id, _)| id.0 != 0 && id == net)
            || self.highlighted_trace.is_some_and(|(id, _)| match id {
                TraceSelection::Segment(id) => object == SelectedObject::Segment(id),
                TraceSelection::Track(id) => track == Some(id),
            })
    }

    pub(super) fn base(&self) -> Self {
        let mut base = self.clone();
        base.pass = OverlayPass::Base;
        base.highlighted_objects = None;
        base.highlighted_related_objects = None;
        base.highlighted_net = None;
        base.highlighted_trace = None;
        base.highlighted_object = None;
        base.hovered_object = None;
        base.hover_selection = None;
        base
    }
    pub(super) fn material_color(
        &self,
        layer: LayerId,
        net: pomelo_core::model::NetId,
    ) -> [f32; 4] {
        crate::scene::colors::copper_color(
            self.color_mode,
            self.colors
                .get(&layer)
                .copied()
                .unwrap_or(self.fallback_color),
            net,
        )
    }

    pub(super) fn object_highlight(
        &self,
        object: pomelo_core::selection::SelectedObject,
        net: pomelo_core::model::NetId,
        track: Option<pomelo_core::model::ObjectId>,
    ) -> Option<[f32; 4]> {
        use pomelo_core::selection::SelectedObject;
        if self.pass == OverlayPass::Selection {
            return self
                .selected(object, net, track)
                .then_some([1.0, 1.0, 1.0, 0.9]);
        }
        if matches!(self.pass, OverlayPass::Hover | OverlayPass::GroupHover) {
            use pomelo_core::selection::SelectionTarget;
            let hovered = self
                .hover_selection
                .as_ref()
                .is_some_and(|(target, members)| match *target {
                    SelectionTarget::Net(id) => id.0 != 0 && id == net,
                    SelectionTarget::Track(id) => track == Some(id),
                    SelectionTarget::Component(_) | SelectionTarget::ComponentGroup(_) => {
                        members.contains(&object)
                    }
                    SelectionTarget::Object(id) => id == object,
                })
                || self.hovered_object.is_some_and(|(id, _)| id == object);
            return (hovered && !self.selected(object, net, track))
                .then_some([0.63, 1.0, 0.85, 0.9]);
        }
        if let Some((selected, color)) = self.highlighted_object
            && selected == object
        {
            return Some(color);
        }
        if let Some((objects, color)) = &self.highlighted_related_objects
            && objects.contains(&object)
        {
            return Some(*color);
        }
        if let SelectedObject::Pin(id) = object
            && let Some(color) = self
                .highlighted_objects
                .as_ref()
                .and_then(|(objects, color)| objects.contains(&id).then_some(*color))
        {
            return Some(color);
        }
        let selected_trace = self
            .highlighted_trace
            .is_some_and(|(selected, _)| match selected {
                TraceSelection::Segment(id) => object == SelectedObject::Segment(id),
                TraceSelection::Track(id) => track == Some(id),
            });
        if selected_trace
            || self
                .highlighted_net
                .is_some_and(|(selected, _)| selected.0 != 0 && selected == net)
        {
            return None;
        }
        self.hovered_object
            .and_then(|(hovered, color)| (hovered == object).then_some(color))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TraceSelection {
    Segment(pomelo_core::model::ObjectId),
    Track(pomelo_core::model::ObjectId),
}

#[derive(Default)]
pub struct TraceTelemetry {
    pub(super) pipeline_builds: AtomicU64,
    pub(super) cache_builds: AtomicU64,
    pub(super) uploaded_instances: AtomicU64,
    pub(super) uploaded_bytes: AtomicU64,
    pub(super) draw_calls: AtomicU64,
    pub(super) resets: AtomicU64,
}

#[derive(Debug, serde::Serialize)]
pub struct TraceStatistics {
    pub pipeline_builds: u64,
    pub cache_builds: u64,
    pub uploaded_instances: u64,
    /// Lifetime upload traffic, including any device recovery.
    pub uploaded_bytes: u64,
    pub draw_calls: u64,
    pub resets: u64,
}

impl TraceTelemetry {
    pub fn snapshot(&self) -> TraceStatistics {
        TraceStatistics {
            pipeline_builds: self.pipeline_builds.load(Ordering::Relaxed),
            cache_builds: self.cache_builds.load(Ordering::Relaxed),
            uploaded_instances: self.uploaded_instances.load(Ordering::Acquire),
            uploaded_bytes: self.uploaded_bytes.load(Ordering::Relaxed),
            draw_calls: self.draw_calls.load(Ordering::Relaxed),
            resets: self.resets.load(Ordering::Relaxed),
        }
    }
}

pub struct TraceRenderer<S: InstanceSource = PreparedTracks> {
    pipeline: Option<Pipeline>,
    uploaded: Option<UploadedTracks<S>>,
    telemetry: Arc<TraceTelemetry>,
}

impl<S: InstanceSource> TraceRenderer<S> {
    pub(super) fn clear_if_cached(&mut self) {
        if self.pipeline.is_some() || self.uploaded.is_some() {
            self.reset();
        }
    }

    pub(super) fn is_uploaded(&self, source: &Arc<S>) -> bool {
        self.uploaded.as_ref().is_some_and(|cache| {
            Arc::ptr_eq(&cache.source, source) && cache.uploaded() == source.instances().len()
        })
    }
    pub fn new(telemetry: Arc<TraceTelemetry>) -> Self {
        Self {
            pipeline: None,
            uploaded: None,
            telemetry,
        }
    }
}

impl<S: InstanceSource> TraceRenderer<S> {
    /// Prepare resources once per application frame, independently of draw ordering.
    pub(super) fn prepare(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame<S>,
    ) -> anyhow::Result<()> {
        if self.pipeline.is_none() {
            self.pipeline = Some(if S::MSDF {
                Pipeline::new_msdf(
                    context.device,
                    frame.tracks.font().context("GPU_MSDF_FONT_MISSING")?,
                )?
            } else if S::COMPACT_TEXT {
                Pipeline::new_text(context.device)?
            } else {
                Pipeline::new(context.device)?
            });
            self.telemetry
                .pipeline_builds
                .fetch_add(1, Ordering::Relaxed);
        }
        if self
            .uploaded
            .as_ref()
            .is_none_or(|cache| !Arc::ptr_eq(&cache.source, &frame.tracks))
        {
            self.uploaded = Some(UploadedTracks::new(Arc::clone(&frame.tracks))?);
            self.telemetry.cache_builds.fetch_add(1, Ordering::Relaxed);
            self.telemetry
                .uploaded_instances
                .store(0, Ordering::Release);
        }
        let cache = self.uploaded.as_mut().context("GPU_TRACE_CACHE_MISSING")?;
        let before = cache.uploaded();
        let upload = cache.upload_next(context.device);
        // Record successful chunks even if a later chunk fails in the same frame.
        let bytes = (cache.uploaded() - before) * std::mem::size_of::<S::Instance>();
        self.telemetry
            .uploaded_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.telemetry
            .uploaded_instances
            .store(cache.uploaded() as u64, Ordering::Release);
        upload?;
        Ok(())
    }

    pub(super) fn draw_prepared(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame<S>,
        scope: TraceScope,
    ) -> anyhow::Result<()> {
        let cache = self.uploaded.as_ref().context("GPU_TRACE_CACHE_MISSING")?;
        let draws = self
            .pipeline
            .as_ref()
            .context("GPU_TRACE_PIPELINE_MISSING")?
            .draw(context, frame, cache, scope)?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        Ok(())
    }
}

impl<S: InstanceSource> NativeGpuRenderer for TraceRenderer<S> {
    fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()> {
        let frame = data
            .downcast_ref::<TraceFrame<S>>()
            .context("GPU_TRACE_PAYLOAD_INVALID")?;
        self.prepare(context, frame)?;
        self.draw_prepared(context, frame, TraceScope::All)
    }

    fn reset(&mut self) {
        self.pipeline = None;
        self.uploaded = None;
        self.telemetry
            .uploaded_instances
            .store(0, Ordering::Release);
        self.telemetry.resets.fetch_add(1, Ordering::Relaxed);
    }
}
