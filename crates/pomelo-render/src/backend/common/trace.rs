//! Portable native pipelines using the common command driver.
pub use super::super::instances::InstanceSource;
use super::super::{TraceFrame, board};
use super::Rect;
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::net_runs::NetRuns;
use super::pad_overlay::OverlayKey;
use super::pad_visibility::Accepted;
use super::{Device, NativeGpuContext};
use crate::split_position;
use crate::tracks::PreparedTracks;
use anyhow::{Context as _, ensure};
use pomelo_core::interaction::Camera;
use std::{cell::RefCell, sync::Arc};
pub(crate) const CHUNK_INSTANCES: usize = 16384;
pub(crate) const CHUNKS_PER_FRAME: usize = 2;
const SPATIAL_CHUNK_INSTANCES: usize = 4096;

struct Chunk {
    start: usize,
    count: usize,
    buffer: Buffer,
    nets: NetRuns,
}
struct ChunkView<'a> {
    start: usize,
    count: usize,
    buffer: &'a Buffer,
    nets: &'a NetRuns,
}
struct IndexedBuffer {
    buffer: Buffer,
    nets: NetRuns,
}
pub(crate) struct UploadedTracks<S: InstanceSource = PreparedTracks> {
    pub source: Arc<S>,
    chunks: Vec<Chunk>,
    uploaded: usize,
    lifetime_bytes: u64,
    spatial: Option<crate::scene::residency::ResidencyCache<IndexedBuffer>>,
    overlays: RefCell<[Option<(OverlayKey, Accepted)>; 2]>,
    #[cfg(test)]
    pub legacy_overlays: bool,
}
impl<S: InstanceSource> UploadedTracks<S> {
    pub fn new(source: Arc<S>) -> anyhow::Result<Self> {
        ensure!(
            source.instances().len() <= u32::MAX as usize,
            "GPU_TRACE_INSTANCE_LIMIT"
        );
        for batch in source.batches() {
            ensure!(
                (batch.start as usize)
                    .checked_add(batch.count as usize)
                    .is_some_and(|end| end <= source.instances().len()),
                "GPU_TRACE_BATCH_RANGE"
            );
        }
        Ok(Self {
            source,
            chunks: Vec::new(),
            uploaded: 0,
            lifetime_bytes: 0,
            spatial: None,
            overlays: RefCell::new([None, None]),
            #[cfg(test)]
            legacy_overlays: false,
        })
    }
    pub fn enable_residency(&mut self, soft_bytes: usize) -> anyhow::Result<()> {
        self.spatial = Some(crate::scene::residency::ResidencyCache::new(
            self.source.instances().len(),
            SPATIAL_CHUNK_INSTANCES,
            size_of::<S::Instance>(),
            soft_bytes,
            |range| {
                let mut bounds: Option<pomelo_core::model::Bounds> = None;
                for index in range {
                    let next = self.source.bounds(index)?;
                    if !next.is_valid() {
                        return None;
                    }
                    if let Some(bounds) = &mut bounds {
                        bounds.include(next.min);
                        bounds.include(next.max);
                    } else {
                        bounds = Some(next);
                    }
                }
                bounds
            },
        )?);
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.spatial
            .as_ref()
            .map_or(self.uploaded == self.source.instances().len(), |cache| {
                cache.ready()
            })
    }
    pub fn lifetime_bytes(&self) -> u64 {
        self.lifetime_bytes
    }
    fn chunks(&self) -> impl Iterator<Item = ChunkView<'_>> {
        self.chunks
            .iter()
            .map(|chunk| ChunkView {
                start: chunk.start,
                count: chunk.count,
                buffer: &chunk.buffer,
                nets: &chunk.nets,
            })
            .chain(self.spatial.iter().flat_map(|cache| {
                cache.slots.iter().filter_map(|slot| {
                    slot.payload.as_ref().map(|payload| ChunkView {
                        start: slot.range.start,
                        count: slot.range.len(),
                        buffer: &payload.buffer,
                        nets: &payload.nets,
                    })
                })
            }))
    }
    pub fn uploaded(&self) -> usize {
        self.spatial.as_ref().map_or(self.uploaded, |cache| {
            cache
                .slots
                .iter()
                .filter(|slot| slot.payload.is_some())
                .map(|slot| slot.range.len())
                .sum()
        })
    }
    #[cfg(all(test, target_os = "windows"))]
    pub fn upload_next(&mut self, device: &Device<'_>) -> anyhow::Result<u64> {
        let mut budget = super::copper::UPLOAD_BYTES_PER_FRAME;
        self.upload_visible(device, None, &mut budget)
    }
    pub fn upload_visible(
        &mut self,
        device: &Device<'_>,
        view: Option<pomelo_core::model::Bounds>,
        budget: &mut usize,
    ) -> anyhow::Result<u64> {
        let mut bytes = 0;
        if let Some(cache) = &mut self.spatial {
            cache.update(view)?;
            let max_chunks = budget
                .checked_div(SPATIAL_CHUNK_INSTANCES * size_of::<S::Instance>())
                .unwrap_or(0)
                .saturating_add(1);
            let mut pending = Vec::new();
            pending.try_reserve_exact(max_chunks.min(cache.slots.len()))?;
            pending.extend(cache.pending(max_chunks));
            for index in pending {
                let range = cache.slots[index].range.clone();
                let data = bytemuck::cast_slice(&self.source.instances()[range.clone()]);
                if data.len() > *budget {
                    break;
                }
                if !cache.can_prefetch(index) {
                    continue;
                }
                let buffer = device.structured_buffer(data, size_of::<S::Instance>())?;
                let nets = NetRuns::build(self.source.as_ref(), range)?;
                cache.slots[index].payload = Some(IndexedBuffer { buffer, nets });
                *budget -= data.len();
                bytes += data.len() as u64;
                self.lifetime_bytes += data.len() as u64;
            }
            return Ok(bytes);
        }
        for _ in 0..CHUNKS_PER_FRAME {
            let start = self.uploaded;
            let count = CHUNK_INSTANCES.min(self.source.instances().len() - start);
            if count == 0 {
                break;
            }
            let data = bytemuck::cast_slice(&self.source.instances()[start..start + count]);
            if data.len() > *budget {
                break;
            }
            let buffer = device.structured_buffer(data, size_of::<S::Instance>())?;
            let nets = NetRuns::build(self.source.as_ref(), start..start + count)?;
            bytes += data.len() as u64;
            self.lifetime_bytes += data.len() as u64;
            *budget -= data.len();
            self.chunks.push(Chunk {
                start,
                count,
                buffer,
                nets,
            });
            self.uploaded += count;
        }
        Ok(bytes)
    }
    fn overlay<T: InstanceSource>(
        &self,
        frame: &TraceFrame<T>,
        key: &OverlayKey,
        range: std::ops::Range<usize>,
        emit: impl FnMut(std::ops::Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let slot = usize::from(frame.pass != board::OverlayPass::Selection);
        let mut entries = self.overlays.borrow_mut();
        if entries[slot]
            .as_ref()
            .is_none_or(|(cached, _)| cached != key)
        {
            let accepted = Accepted::build((0..self.source.instances().len()).map(|index| {
                let ids = self.source.selection_ids(index);
                frame
                    .object_highlight(
                        self.source.selected_object(index),
                        pomelo_core::model::NetId(ids[3]),
                        Some(pomelo_core::model::ObjectId(ids[1])),
                    )
                    .is_some()
            }))?;
            entries[slot] = Some((key.clone(), accepted));
        }
        entries[slot]
            .as_ref()
            .context("GPU_TRACE_OVERLAY_MISSING")?
            .1
            .visible(range, emit)
    }
}
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Uniforms {
    viewport: [f32; 4],
    canvas: [f32; 4],
    clip: [f32; 4],
    camera: [f32; 4],
    view: [f32; 4],
    color: [f32; 4],
    batch: [u32; 4],
    highlight: [f32; 4],
}
const _: () = assert!(std::mem::size_of::<Uniforms>() == 128);
const _: () = assert!(std::mem::offset_of!(Uniforms, batch) == 96);

pub(crate) struct Pipeline {
    pipeline: GpuPipeline,
    atlas: Option<std::collections::BTreeMap<u16, super::driver::Atlas>>,
}
impl Pipeline {
    pub fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pipeline: device.pipeline(super::Shader::Trace)?,
            atlas: None,
        })
    }
    pub fn new_text(device: &Device<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pipeline: device.pipeline(super::Shader::Text)?,
            atlas: None,
        })
    }
    pub fn new_msdf(
        device: &Device<'_>,
        font: &crate::text::msdf::MsdfFont,
    ) -> anyhow::Result<Self> {
        let mut atlas = std::collections::BTreeMap::new();
        for page in &font.pages {
            atlas.insert(
                page.page,
                device.atlas(page.width, page.height, &page.rgba)?,
            );
        }
        Ok(Self {
            pipeline: device.pipeline(super::Shader::Label)?,
            atlas: Some(atlas),
        })
    }
    pub fn draw<S: InstanceSource, T: InstanceSource>(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame<T>,
        cache: &UploadedTracks<S>,
        scope: board::TraceScope,
    ) -> anyhow::Result<u64> {
        let bounds = context.bounds;
        let width = bounds.size.width.0;
        let height = bounds.size.height.0;
        if width <= 0.0 || height <= 0.0 {
            return Ok(0);
        }
        let camera = if let Some(mut camera) = frame.camera {
            camera.pixels_per_mm *= f64::from(frame.scale_factor);
            camera
        } else {
            let mut camera = Camera::default();
            ensure!(
                camera.fit(
                    frame.bounds,
                    f64::from(width),
                    f64::from(height),
                    f64::from(width.min(height)) * 0.04
                ),
                "GPU_TRACE_CAMERA_FIT"
            );
            camera
        };
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_TRACE_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, dx, y, dy].into_iter().all(f32::is_finite),
            "GPU_TRACE_CAMERA_CENTER"
        );
        let clip = bounds.intersect(&context.content_mask.bounds);
        let rect = Rect {
            left: clip.origin.x.0.floor().max(0.0) as i32,
            top: clip.origin.y.0.floor().max(0.0) as i32,
            right: (clip.origin.x.0 + clip.size.width.0)
                .ceil()
                .min(context.viewport[0]) as i32,
            bottom: (clip.origin.y.0 + clip.size.height.0)
                .ceil()
                .min(context.viewport[1]) as i32,
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Ok(0);
        }
        let (selection_id, selection_kind, highlight) = match frame.highlighted_trace {
            Some((board::TraceSelection::Segment(id), color)) => (id.0, 2, color),
            Some((board::TraceSelection::Track(id), color)) => (id.0, 3, color),
            None => frame
                .highlighted_net
                .map_or((0, 0, [0.0; 4]), |(net, color)| {
                    (net.0, u32::from(net.0 != 0), color)
                }),
        };
        let mut uniforms = Uniforms {
            viewport: [
                context.viewport[0],
                context.viewport[1],
                frame.scale_factor,
                f32::from(u8::from(frame.filled)),
            ],
            canvas: [bounds.origin.x.0, bounds.origin.y.0, width, height],
            clip: [
                clip.origin.x.0,
                clip.origin.y.0,
                clip.size.width.0,
                clip.size.height.0,
            ],
            camera: [x, y, dx, dy],
            view: [
                camera.pixels_per_mm as f32,
                if camera.flipped { -1.0 } else { 1.0 },
                frame.opacity,
                frame.pass as u8 as f32,
            ],
            color: frame.fallback_color,
            batch: [
                0,
                selection_id,
                selection_kind,
                match scope {
                    board::TraceScope::ZoneOutlines(_) => 1,
                    _ => 0,
                },
            ],
            highlight,
        };
        let mut draws = 0;
        #[cfg(not(test))]
        let legacy = false;
        #[cfg(test)]
        let legacy = cache.legacy_overlays;
        let overlay_key =
            (!legacy && frame.pass != board::OverlayPass::Base).then(|| OverlayKey::new(frame));
        let net_overlay = (!legacy).then(|| super::pad::overlay_net(frame)).flatten();
        for batch in cache.source.batches() {
            let selected = match scope {
                board::TraceScope::All => true,
                board::TraceScope::Layer(layer) | board::TraceScope::ZoneOutlines(layer) => {
                    !batch.outline && batch.layer == layer
                }
                board::TraceScope::Outline => batch.outline,
                board::TraceScope::Labels(layer, _, _) => batch.layer == layer,
                board::TraceScope::Pads(layer, _) => batch.layer == layer,
            };
            if !selected {
                continue;
            }
            if batch.outline && overlay_key.is_some() {
                continue;
            }
            uniforms.color = if batch.outline {
                let mut color = frame.fallback_color;
                color[3] *= frame.opacity;
                color
            } else {
                frame.layer_color(batch.layer)
            };
            let base_color = uniforms.color;
            for chunk in cache.chunks() {
                let start = (batch.start as usize).max(chunk.start);
                let end = ((batch.start + batch.count) as usize).min(chunk.start + chunk.count);
                if start >= end {
                    continue;
                }
                let mut draw_range = |range: std::ops::Range<usize>| -> anyhow::Result<()> {
                    let mut cursor = range.start;
                    let end = range.end;
                    while cursor < end {
                        let span_start = cursor;
                        let accepted = |index: usize| match scope {
                            board::TraceScope::Pads(_, pin) => {
                                matches!(
                                    cache.source.selected_object(index),
                                    pomelo_core::selection::SelectedObject::Pin(_)
                                ) == pin
                            }
                            board::TraceScope::Labels(_, category, owner) => {
                                cache.source.label_category(index) == category as u32
                                    && owner.is_none_or(|id| {
                                        cache.source.selection_ids(index)[0] == id.0
                                    })
                            }
                            _ => true,
                        };
                        let accept = accepted(cursor);
                        let page = cache.source.atlas_page(cursor);
                        let selected = |index: usize| {
                            if batch.outline {
                                return None;
                            }
                            frame.object_highlight(
                                cache.source.selected_object(index),
                                pomelo_core::model::NetId(cache.source.selection_ids(index)[3]),
                                Some(pomelo_core::model::ObjectId(
                                    cache.source.selection_ids(index)[1],
                                )),
                            )
                        };
                        let color = if overlay_key.is_some() {
                            Some(if frame.pass == board::OverlayPass::Selection {
                                [1.0, 1.0, 1.0, 0.9]
                            } else {
                                [0.63, 1.0, 0.85, 0.9]
                            })
                        } else {
                            selected(cursor)
                        };
                        cursor += 1;
                        if (overlay_key.is_some()
                            && !S::MSDF
                            && !matches!(
                                scope,
                                board::TraceScope::Pads(_, _) | board::TraceScope::Labels(_, _, _)
                            ))
                            || (frame.pass == board::OverlayPass::Base
                                && !S::MSDF
                                && !matches!(scope, board::TraceScope::Pads(_, _))
                                && frame.highlighted_related_objects.is_none()
                                && frame.highlighted_object.is_none()
                                && frame.hovered_object.is_none())
                        {
                            cursor = end;
                        } else {
                            while cursor < end
                                && (overlay_key.is_some() || selected(cursor) == color)
                                && accepted(cursor) == accept
                                && cache.source.atlas_page(cursor) == page
                            {
                                cursor += 1;
                            }
                        }
                        if !accept {
                            continue;
                        }
                        if frame.pass != board::OverlayPass::Base && color.is_none() {
                            continue;
                        }
                        uniforms.color = color.unwrap_or(base_color);
                        // Copper boundaries stay white for selection and hover alike.
                        uniforms.view[3] = if color.is_some()
                            && matches!(
                                cache.source.selected_object(span_start),
                                pomelo_core::selection::SelectedObject::Zone(_)
                            ) {
                            board::OverlayPass::Selection as u8 as f32
                        } else {
                            frame.pass as u8 as f32
                        };
                        // Network colors are computed per instance; object overrides still win.
                        uniforms.view[2] = if S::MSDF {
                            frame.opacity
                        } else {
                            f32::from(u8::from(
                                frame.color_mode == pomelo_core::display::ColorMode::Net
                                    && !batch.outline
                                    && !S::COMPACT_TEXT
                                    && color.is_none(),
                            ))
                        };
                        uniforms.batch[2] = if color.is_some() { 0 } else { selection_kind };
                        uniforms.batch[0] = (span_start - chunk.start) as u32;
                        context.device.draw(
                            &self.pipeline,
                            Draw {
                                uniforms: bytemuck::bytes_of(&uniforms),
                                vertices: chunk.buffer,
                                indices: None,
                                start: 0,
                                count: 4,
                                instances: (cursor - span_start) as u32,
                                rect,
                                stencil: StencilMode::Inherited,
                                atlas: self
                                    .atlas
                                    .as_ref()
                                    .map(|atlas| atlas.get(&page).context("GPU_MSDF_PAGE_MISSING"))
                                    .transpose()?,
                            },
                        )?;
                        draws += 1;
                    }
                    Ok(())
                };
                if let Some((net, pointer)) = net_overlay
                    && pointer.is_none_or(|object| !chunk.nets.may_contain(object))
                {
                    chunk.nets.visible(net.0, start..end, draw_range)?;
                } else if let Some(key) = overlay_key.as_ref() {
                    cache.overlay(frame, key, start..end, draw_range)?;
                } else {
                    draw_range(start..end)?;
                }
            }
        }
        Ok(draws)
    }
}
