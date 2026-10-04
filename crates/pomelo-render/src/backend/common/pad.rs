//! Portable native pipelines using the common command driver.
use super::super::{TraceFrame, board};
use super::Rect;
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::trace::{CHUNK_INSTANCES, CHUNKS_PER_FRAME};
use super::{Device, NativeGpuContext};
use crate::pads::{PadInstance, PreparedPads};
use crate::split_position;
use anyhow::ensure;
use pomelo_core::{interaction::Camera, model::LayerId};
use std::sync::Arc;
struct Chunk {
    start: usize,
    count: usize,
    buffer: Buffer,
}
pub(crate) struct UploadedPads {
    pub source: Arc<PreparedPads>,
    chunks: Vec<Chunk>,
    uploaded: usize,
}
impl UploadedPads {
    pub fn new(source: Arc<PreparedPads>) -> anyhow::Result<Self> {
        ensure!(
            source.analytic.len() <= u32::MAX as usize,
            "GPU_PAD_INSTANCE_LIMIT"
        );
        for batch in &source.batches {
            ensure!(
                (batch.start as usize)
                    .checked_add(batch.count as usize)
                    .is_some_and(|end| end <= source.analytic.len()),
                "GPU_PAD_BATCH_RANGE"
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
            let count = CHUNK_INSTANCES.min(self.source.analytic.len() - start);
            if count == 0 {
                break;
            }
            let data = bytemuck::cast_slice(&self.source.analytic[start..start + count]);
            let buffer = device.structured_buffer(data, size_of::<PadInstance>())?;
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
}
impl Pipeline {
    pub fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pipeline: device.pipeline(super::Shader::Pad)?,
        })
    }
    pub fn draw(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedPads,
        layer: Option<LayerId>,
        visible: Option<&dyn Fn(&PadInstance) -> bool>,
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
                "GPU_PAD_CAMERA_FIT"
            );
            camera
        };
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_PAD_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, dx, y, dy].into_iter().all(f32::is_finite),
            "GPU_PAD_CAMERA_CENTER"
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
                f32::from(u8::from(
                    frame.color_mode == pomelo_core::display::ColorMode::Net,
                )),
                frame.pass as u8 as f32,
            ],
            color: frame.fallback_color,
            batch: [
                0,
                frame.highlighted_net.map_or(0, |(net, _)| net.0),
                u32::from(
                    frame.pass == board::OverlayPass::Base
                        && frame.highlighted_net.is_some_and(|(net, _)| net.0 != 0),
                ),
                0,
            ],
            highlight: frame.highlighted_net.map_or([0.0; 4], |(_, color)| color),
        };
        let mut draws = 0;
        for batch in &cache.source.batches {
            if layer.is_some_and(|layer| batch.layer != layer) {
                continue;
            }
            uniforms.color = frame.layer_color(batch.layer);
            for chunk in &cache.chunks {
                let start = (batch.start as usize).max(chunk.start);
                let end = ((batch.start + batch.count) as usize).min(chunk.start + chunk.count);
                if start >= end {
                    continue;
                }
                let selected = |index: usize| {
                    let instance = &cache.source.analytic[index];
                    let object = if instance.source[0] == 0 {
                        pomelo_core::selection::SelectedObject::Pin(pomelo_core::model::ObjectId(
                            instance.ids[0],
                        ))
                    } else {
                        pomelo_core::selection::SelectedObject::Via(pomelo_core::model::ObjectId(
                            instance.ids[0],
                        ))
                    };
                    frame.object_highlight(object, pomelo_core::model::NetId(instance.ids[2]), None)
                };
                let mut cursor = start;
                while cursor < end {
                    while cursor < end
                        && visible
                            .is_some_and(|predicate| !predicate(&cache.source.analytic[cursor]))
                    {
                        cursor += 1;
                    }
                    let start = cursor;
                    let highlighted = (cursor < end).then(|| selected(cursor)).flatten();
                    while cursor < end
                        && selected(cursor) == highlighted
                        && visible.is_none_or(|predicate| predicate(&cache.source.analytic[cursor]))
                    {
                        cursor += 1;
                    }
                    let end = cursor;
                    if start == end {
                        continue;
                    }
                    if frame.pass != board::OverlayPass::Base && highlighted.is_none() {
                        continue;
                    }
                    uniforms.batch[3] = u32::from(highlighted.is_some());
                    uniforms.highlight = highlighted.unwrap_or_else(|| {
                        frame.highlighted_net.map_or([0.0; 4], |(_, color)| color)
                    });
                    uniforms.batch[0] = (start - chunk.start) as u32;
                    context.device.draw(
                        &self.pipeline,
                        Draw {
                            uniforms: bytemuck::bytes_of(&uniforms),
                            vertices: &chunk.buffer,
                            indices: None,
                            start: 0,
                            count: 4,
                            instances: (end - start) as u32,
                            rect,
                            stencil: StencilMode::Inherited,
                            atlas: None,
                        },
                    )?;
                    draws += 1;
                }
            }
        }
        Ok(draws)
    }
}
