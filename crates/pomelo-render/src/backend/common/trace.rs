//! Portable native pipelines using the common command driver.
pub use super::super::instances::InstanceSource;
use super::super::{TraceFrame, board};
use super::Rect;
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::{Device, NativeGpuContext};
use crate::split_position;
use crate::tracks::PreparedTracks;
use anyhow::{Context as _, ensure};
use pomelo_core::interaction::Camera;
use std::sync::Arc;
pub(crate) const CHUNK_INSTANCES: usize = 16384;
pub(crate) const CHUNKS_PER_FRAME: usize = 2;

struct Chunk {
    start: usize,
    count: usize,
    buffer: Buffer,
}
pub(crate) struct UploadedTracks<S: InstanceSource = PreparedTracks> {
    pub source: Arc<S>,
    chunks: Vec<Chunk>,
    uploaded: usize,
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
        })
    }
    pub fn uploaded(&self) -> usize {
        self.uploaded
    }
    pub fn upload_next(&mut self, device: &Device<'_>) -> anyhow::Result<u64> {
        let mut bytes = 0;
        for _ in 0..CHUNKS_PER_FRAME {
            let start = self.uploaded;
            let count = CHUNK_INSTANCES.min(self.source.instances().len() - start);
            if count == 0 {
                break;
            }
            let data = bytemuck::cast_slice(&self.source.instances()[start..start + count]);
            let buffer = device.structured_buffer(data, size_of::<S::Instance>())?;
            bytes += data.len() as u64;
            self.chunks.push(Chunk {
                start,
                count,
                buffer,
            });
            self.uploaded += count;
        }
        Ok(bytes)
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
                    board::TraceScope::ZoneOutlines(_, true) => 1,
                    board::TraceScope::ZoneOutlines(_, false) => 2,
                    _ => 0,
                },
            ],
            highlight,
        };
        let mut draws = 0;
        for batch in cache.source.batches() {
            let selected = match scope {
                board::TraceScope::All => true,
                board::TraceScope::Layer(layer) | board::TraceScope::ZoneOutlines(layer, _) => {
                    !batch.outline && batch.layer == layer
                }
                board::TraceScope::Outline => batch.outline,
                board::TraceScope::Labels(layer, _, _) => batch.layer == layer,
                board::TraceScope::Pads(layer, _) => batch.layer == layer,
            };
            if !selected {
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
            for chunk in &cache.chunks {
                let start = (batch.start as usize).max(chunk.start);
                let end = ((batch.start + batch.count) as usize).min(chunk.start + chunk.count);
                if start >= end {
                    continue;
                }
                let mut cursor = start;
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
                                && owner
                                    .is_none_or(|id| cache.source.selection_ids(index)[0] == id.0)
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
                    let color = selected(cursor);
                    cursor += 1;
                    if frame.pass == board::OverlayPass::Base
                        && !S::MSDF
                        && !matches!(scope, board::TraceScope::Pads(_, _))
                        && frame.highlighted_related_objects.is_none()
                        && frame.highlighted_object.is_none()
                        && frame.hovered_object.is_none()
                    {
                        cursor = end;
                    } else {
                        while cursor < end
                            && selected(cursor) == color
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
                            vertices: &chunk.buffer,
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
            }
        }
        Ok(draws)
    }
}
