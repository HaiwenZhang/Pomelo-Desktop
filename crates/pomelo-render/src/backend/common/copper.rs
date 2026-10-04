//! Two-bit stencil coverage, union, holes and clipped labels shared by D3D11, Metal and wgpu.
use super::super::{TraceFrame, board};
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::{Device, NativeGpuContext, Rect};
use crate::{
    copper::{CopperVertex, PreparedCopper},
    split_position,
};
use anyhow::{Context as _, ensure};
use std::{collections::BTreeMap, sync::Arc};
pub(crate) type ZoneAnnotations<'a> =
    dyn FnMut(&crate::copper::CopperBatch) -> anyhow::Result<()> + 'a;
pub(crate) struct CopperDrawOptions<'a> {
    pub opacity: f32,
    pub layer: Option<pomelo_core::model::LayerId>,
    pub visible: Option<&'a dyn Fn(&crate::copper::CopperBatch) -> bool>,
    pub annotations: Option<&'a mut ZoneAnnotations<'a>>,
    pub overrides: Option<&'a BTreeMap<pomelo_core::model::ObjectId, UploadedCopper>>,
    pub static_shapes_fill_solid: bool,
    pub network_selection: Option<pomelo_core::model::NetId>,
}

pub(crate) const UPLOAD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;

pub(crate) struct UploadedCopper {
    pub source: Arc<PreparedCopper>,
    pub active: bool,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_uploaded: usize,
    index_uploaded: usize,
}
impl UploadedCopper {
    pub fn new(source: Arc<PreparedCopper>, device: &Device<'_>) -> anyhow::Result<Self> {
        let vertex_buffer = if source.vertices.is_empty() {
            None
        } else {
            Some(
                device.buffer_empty(
                    source
                        .vertices
                        .len()
                        .checked_mul(size_of::<CopperVertex>())
                        .context("GPU_COPPER_BUFFER_SIZE")?,
                    false,
                )?,
            )
        };
        let index_buffer = if source.indices.is_empty() {
            None
        } else {
            Some(
                device.buffer_empty(
                    source
                        .indices
                        .len()
                        .checked_mul(4)
                        .context("GPU_COPPER_BUFFER_SIZE")?,
                    true,
                )?,
            )
        };
        Ok(Self {
            source,
            active: false,
            vertex_buffer,
            index_buffer,
            vertex_uploaded: 0,
            index_uploaded: 0,
        })
    }
    pub fn uploaded_bytes(&self) -> usize {
        self.vertex_uploaded * size_of::<CopperVertex>() + self.index_uploaded * 4
    }
    pub fn upload_next(&mut self, context: &NativeGpuContext<'_>) -> anyhow::Result<()> {
        let mut budget = UPLOAD_BYTES_PER_FRAME;
        self.upload_with_budget(context, &mut budget)
    }
    pub fn upload_with_budget(
        &mut self,
        context: &NativeGpuContext<'_>,
        budget: &mut usize,
    ) -> anyhow::Result<()> {
        if let Some(buffer) = &self.vertex_buffer {
            upload_slice(
                &context.device,
                buffer,
                &self.source.vertices,
                &mut self.vertex_uploaded,
                budget,
            )?;
        }
        if let Some(buffer) = &self.index_buffer {
            upload_slice(
                &context.device,
                buffer,
                &self.source.indices,
                &mut self.index_uploaded,
                budget,
            )?;
        }
        Ok(())
    }
}
fn upload_slice<T: bytemuck::Pod>(
    device: &Device<'_>,
    buffer: &Buffer,
    source: &[T],
    uploaded: &mut usize,
    budget: &mut usize,
) -> anyhow::Result<()> {
    let count = (source.len() - *uploaded).min(*budget / size_of::<T>());
    if count != 0 {
        let data = bytemuck::cast_slice(&source[*uploaded..*uploaded + count]);
        device.write_buffer(buffer, *uploaded * size_of::<T>(), data)?;
        *uploaded += count;
        *budget -= data.len();
    }
    Ok(())
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
    rectangle: [f32; 4],
    pattern: [u32; 16],
}
const _: () = assert!(size_of::<Uniforms>() == 176);

// Logical-pixel 16x16 stipple observed in the local Allegro static-shape view.
// Rows are bitmap masks, allowing calibration without touching board geometry.
const STATIC_SHAPE_STIPPLE: [u32; 16] = [
    0, 0, 0, 0x0108, 0, 0, 0x0801, 0, 0, 0, 0, 0x0801, 0, 0, 0x0108, 0,
];
// Recovered from all 256 physical bitmap cells in the Allegro net highlight.
// Independent screenshot regions reproduce this 109/256 pattern exactly at DPI 1.5.
const NETWORK_SHAPE_STIPPLE: [u32; 16] = [
    0, 0x01c0, 0x03e0, 0x07f0, 0x0ff8, 0x1ffc, 0x3ffe, 0x3ffe, 0x3ffe, 0x1ffc, 0x0ff8, 0x07f0,
    0x03e0, 0x01c0, 0, 0,
];
// Minimal physical-period-four candidate from controlled NVL temporary selection.
// Canvas phase compensates the reference's physical origin (294,144). Only DPI
// 1.5 has actual Allegro screenshot evidence; other DPI remain policy coverage.
const DYNAMIC_OBJECT_STIPPLE: [u32; 16] = [
    0, 0x1111, 0, 0x4444, 0, 0x1111, 0, 0x4444, 0, 0x1111, 0, 0x4444, 0, 0x1111, 0, 0x4444,
];

pub(crate) struct Pipeline {
    pipeline: GpuPipeline,
}
impl Pipeline {
    pub fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pipeline: device.pipeline(super::Shader::Copper)?,
        })
    }
    pub fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedCopper,
        options: CopperDrawOptions<'_>,
    ) -> anyhow::Result<(u64, u64)> {
        let CopperDrawOptions {
            opacity,
            layer,
            visible,
            mut annotations,
            overrides,
            static_shapes_fill_solid,
            network_selection,
        } = options;
        if cache.index_uploaded == 0 {
            return Ok((0, 0));
        }
        let bounds = context.bounds;
        let (width, height) = (bounds.size.width.0, bounds.size.height.0);
        if width <= 0.0 || height <= 0.0 {
            return Ok((0, 0));
        }
        let mut camera = frame.camera.unwrap_or_default();
        if frame.camera.is_some() {
            camera.pixels_per_mm *= f64::from(frame.scale_factor);
        } else {
            ensure!(
                camera.fit(
                    frame.bounds,
                    f64::from(width),
                    f64::from(height),
                    f64::from(width.min(height)) * 0.04
                ),
                "GPU_COPPER_CAMERA_FIT"
            );
        }
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_COPPER_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, y, dx, dy].into_iter().all(f32::is_finite)
                && opacity.is_finite()
                && (0.0..=1.0).contains(&opacity),
            "GPU_COPPER_FRAME_INVALID"
        );
        let _mask = context.device.stencil_scope(context.viewport)?;
        let clip = bounds.intersect(&context.content_mask.bounds);
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
                0.0,
                frame.pass as u8 as f32,
            ],
            color: frame.fallback_color,
            rectangle: [0.0; 4],
            pattern: STATIC_SHAPE_STIPPLE,
        };
        let (mut draws, mut zones) = (0, 0);
        for batch in &cache.source.batches {
            if layer.is_some_and(|layer| batch.layer != layer)
                || visible.is_some_and(|test| !test(batch))
            {
                continue;
            }
            let replacement = overrides
                .and_then(|entries| entries.get(&batch.object))
                .filter(|entry| {
                    entry.active && entry.uploaded_bytes() == entry.source.upload_bytes()
                });
            // A clipped curve can have no filled geometry. Its empty override
            // intentionally suppresses the original mesh; it needs no GPU buffers.
            if replacement.is_some_and(|entry| {
                entry.source.vertices.is_empty() || entry.source.indices.is_empty()
            }) {
                continue;
            }
            let cache = replacement.unwrap_or(cache);
            let batch = replacement
                .and_then(|entry| entry.source.batches.first())
                .unwrap_or(batch);
            let overlay = frame.object_highlight(batch.selected_object, batch.net, None);
            if frame.pass != board::OverlayPass::Base && overlay.is_none() {
                continue;
            }
            let zone = matches!(
                batch.selected_object,
                pomelo_core::selection::SelectedObject::Zone(_)
            );
            let net_selected =
                zone && matches!(
                    batch.kind,
                    pomelo_core::model::ZoneKind::Static | pomelo_core::model::ZoneKind::Dynamic
                ) && network_selection
                    .or_else(|| frame.highlighted_net.map(|(net, _)| net))
                    .is_some_and(|net| net.0 != 0 && net == batch.net);
            let object_selected = frame.pass == board::OverlayPass::Selection
                && !net_selected
                && zone
                && frame
                    .highlighted_object
                    .is_some_and(|(object, _)| object == batch.selected_object);
            // Solid static shapes retain their fill; unsupported kinds do
            // not inherit a rule inferred from known static/dynamic shapes.
            if object_selected
                && (batch.kind == pomelo_core::model::ZoneKind::Unknown
                    || (batch.kind == pomelo_core::model::ZoneKind::Static
                        && static_shapes_fill_solid))
            {
                continue;
            }
            // Custom pads share this pipeline but never inherit static-shape styling.
            uniforms.view[2] = if net_selected && frame.pass == board::OverlayPass::Base {
                3.0 // Replace ordinary fill, retaining the full stencil for labels.
            } else if net_selected && frame.pass == board::OverlayPass::Selection {
                2.0 // Dedicated physical-pixel persistent network bitmap.
            } else if object_selected {
                if batch.kind == pomelo_core::model::ZoneKind::Static {
                    4.0 // Recolor the existing logical sparse bitmap.
                } else {
                    5.0 // Original material in gaps, white physical dense points.
                }
            } else {
                f32::from(u8::from(
                    !static_shapes_fill_solid
                        && frame.pass == board::OverlayPass::Base
                        && batch.kind == pomelo_core::model::ZoneKind::Static
                        && zone,
                ))
            };
            uniforms.pattern = if uniforms.view[2] == 2.0 {
                NETWORK_SHAPE_STIPPLE
            } else if uniforms.view[2] == 5.0 {
                DYNAMIC_OBJECT_STIPPLE
            } else {
                STATIC_SHAPE_STIPPLE
            };
            let outer = batch.outer_indices();
            let holes = batch.hole_indices();
            if (batch.parity_rings.is_none() && outer.is_empty())
                || holes.end as usize > cache.index_uploaded
                || (batch.vertex_start as usize + batch.vertex_count as usize)
                    > cache.vertex_uploaded
            {
                continue;
            }
            let Some(zone_bounds) = batch.bounds else {
                continue;
            };
            let a = camera.board_to_view(zone_bounds.min, f64::from(width), f64::from(height));
            let b = camera.board_to_view(zone_bounds.max, f64::from(width), f64::from(height));
            let left = (a.x.min(b.x) + f64::from(bounds.origin.x.0))
                .floor()
                .max(f64::from(clip.origin.x.0))
                .max(0.0);
            let top = (a.y.min(b.y) + f64::from(bounds.origin.y.0))
                .floor()
                .max(f64::from(clip.origin.y.0))
                .max(0.0);
            let right = (a.x.max(b.x) + f64::from(bounds.origin.x.0))
                .ceil()
                .min(f64::from(clip.origin.x.0 + clip.size.width.0))
                .min(f64::from(context.viewport[0]));
            let bottom = (a.y.max(b.y) + f64::from(bounds.origin.y.0))
                .ceil()
                .min(f64::from(clip.origin.y.0 + clip.size.height.0))
                .min(f64::from(context.viewport[1]));
            if right <= left || bottom <= top {
                continue;
            }
            let rect = Rect {
                left: left.floor() as i32,
                top: top.floor() as i32,
                right: right.ceil() as i32,
                bottom: bottom.ceil() as i32,
            };
            uniforms.rectangle = [
                rect.left as f32,
                rect.top as f32,
                rect.right as f32,
                rect.bottom as f32,
            ];
            uniforms.color = frame
                .highlighted_net
                .filter(|(net, _)| net.0 != 0 && *net == batch.net)
                .map(|(_, color)| color)
                .unwrap_or_else(|| frame.material_color(batch.layer, batch.net));
            if let Some(color) = overlay {
                uniforms.color = color;
            }
            if object_selected && batch.kind == pomelo_core::model::ZoneKind::Dynamic {
                // Controlled alpha 99/128/255/0 evidence supports a second
                // source-over material draw in gaps, not a transparent gap.
                uniforms.color = frame.material_color(batch.layer, batch.net);
            }
            if frame.pass == board::OverlayPass::Base {
                uniforms.color[3] *= opacity;
            } else if (net_selected || object_selected)
                && frame.pass == board::OverlayPass::Selection
            {
                // The observed highlighted shape retains the shapes slider,
                // independently of global alpha and ordinary UI selection alpha.
                uniforms.color[3] = opacity;
            }
            let vertices = cache
                .vertex_buffer
                .as_ref()
                .context("GPU_COPPER_VERTEX_MISSING")?;
            let indices = cache
                .index_buffer
                .as_ref()
                .context("GPU_COPPER_INDEX_MISSING")?;
            let encode = |mode, range: Option<std::ops::Range<u32>>| -> anyhow::Result<()> {
                let (start, count, index_buffer) = range.map_or((0, 4, None), |range| {
                    (range.start, range.end - range.start, Some(indices))
                });
                context.device.draw(
                    &self.pipeline,
                    Draw {
                        uniforms: bytemuck::bytes_of(&uniforms),
                        vertices,
                        indices: index_buffer,
                        start,
                        count,
                        instances: 1,
                        rect,
                        stencil: mode,
                        atlas: None,
                    },
                )
            };
            encode(StencilMode::Clear, None)?;
            draws += 1;
            if let Some(rings) = &batch.parity_rings {
                for ring in rings {
                    encode(StencilMode::Toggle, Some(ring.indices.clone()))?;
                    encode(
                        if ring.outer {
                            StencilMode::ApplyOuter
                        } else {
                            StencilMode::ApplyHole
                        },
                        None,
                    )?;
                    encode(StencilMode::ClearScratch, None)?;
                    draws += 3;
                }
            } else {
                encode(StencilMode::Exterior, Some(outer.clone()))?;
                draws += 1;
                if !holes.is_empty() {
                    encode(StencilMode::Holes, Some(holes.clone()))?;
                    draws += 1;
                }
            }
            encode(
                StencilMode::Shade,
                if batch.parity_rings.is_some() {
                    None
                } else {
                    Some(outer.clone())
                },
            )?;
            draws += 1;
            if let Some(labels) = annotations.as_mut() {
                labels(batch)?;
            }
            zones += 1;
        }
        Ok((draws, zones))
    }
}
