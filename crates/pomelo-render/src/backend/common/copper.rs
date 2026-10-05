//! Two-bit stencil coverage, union, holes and clipped labels shared by D3D11, Metal and wgpu.
use super::super::{TraceFrame, board};
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::{Device, NativeGpuContext, Rect};
use crate::{
    copper::{CopperVertex, PreparedCopper},
    split_position,
};
use anyhow::{Context as _, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub(crate) type ZoneAnnotations<'a> =
    dyn FnMut(&crate::copper::CopperBatch) -> anyhow::Result<()> + 'a;
pub(crate) struct CopperDrawOptions<'a> {
    pub opacity: f32,
    pub layer: Option<pomelo_core::model::LayerId>,
    pub visible: Option<&'a dyn Fn(&crate::copper::CopperBatch) -> bool>,
    pub annotations: Option<&'a mut ZoneAnnotations<'a>>,
    /// None conservatively means that every batch may have annotations.
    pub annotation_owners: Option<&'a BTreeSet<pomelo_core::model::ObjectId>>,
    pub overrides: Option<&'a BTreeMap<pomelo_core::model::ObjectId, UploadedCopper>>,
    pub static_shapes_fill_solid: bool,
}

pub(crate) const UPLOAD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;

#[derive(Debug)]
struct PageRanges {
    vertices: std::ops::Range<usize>,
    indices: std::ops::Range<usize>,
}
struct CopperPage {
    ranges: PageRanges,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_uploaded: usize,
    index_uploaded: usize,
}

// Pack in source geometry order, which can differ from layer draw order. Each
// batch stays together so exterior, holes and parity rings share one stencil.
fn page_ranges(
    source: &PreparedCopper,
    vertex_limit: usize,
    index_limit: usize,
) -> anyhow::Result<(Vec<PageRanges>, Vec<usize>)> {
    let mut order = Vec::new();
    order.try_reserve_exact(source.batches.len())?;
    order.extend(0..source.batches.len());
    order.sort_unstable_by_key(|&index| {
        let batch = &source.batches[index];
        (
            batch.vertex_start,
            batch.outer_indices().start,
            batch.source_index,
        )
    });
    let mut pages: Vec<PageRanges> = Vec::new();
    let mut batch_pages = Vec::new();
    batch_pages.try_reserve_exact(source.batches.len())?;
    batch_pages.resize(source.batches.len(), 0);
    for index in order {
        let batch = &source.batches[index];
        let vertices =
            batch.vertex_start as usize..batch.vertex_start as usize + batch.vertex_count as usize;
        let indices = batch.outer_indices().start as usize..batch.hole_indices().end as usize;
        ensure!(
            vertices.len() <= vertex_limit && indices.len() <= index_limit,
            "GPU_COPPER_BATCH_SIZE: object {:?}, vertex bytes {}, index bytes {}, limits {} / {} bytes",
            batch.object,
            vertices.len() * size_of::<CopperVertex>(),
            indices.len() * 4,
            vertex_limit * size_of::<CopperVertex>(),
            index_limit * 4
        );
        if let Some(page) = pages.last_mut().filter(|page| {
            page.vertices.end == vertices.start
                && page.indices.end == indices.start
                && vertices.end - page.vertices.start <= vertex_limit
                && indices.end - page.indices.start <= index_limit
        }) {
            page.vertices.end = vertices.end;
            page.indices.end = indices.end;
        } else {
            pages.try_reserve(1)?;
            pages.push(PageRanges { vertices, indices });
        }
        batch_pages[index] = pages.len() - 1;
    }
    Ok((pages, batch_pages))
}

pub(crate) struct UploadedCopper {
    pub source: Arc<PreparedCopper>,
    pub active: bool,
    pages: Vec<CopperPage>,
    batch_pages: Vec<usize>,
    /// Original batch order within each layer; no geometry is duplicated.
    layer_batches: BTreeMap<pomelo_core::model::LayerId, Vec<usize>>,
    #[cfg(test)]
    pub force_stencil: bool,
}
impl UploadedCopper {
    pub fn new(source: Arc<PreparedCopper>, device: &Device<'_>) -> anyhow::Result<Self> {
        Self::new_with_limits(
            source,
            device,
            device.buffer_limit(false),
            device.buffer_limit(true),
        )
    }
    fn new_with_limits(
        source: Arc<PreparedCopper>,
        device: &Device<'_>,
        vertex_bytes: usize,
        index_bytes: usize,
    ) -> anyhow::Result<Self> {
        let mut counts = BTreeMap::new();
        for batch in &source.batches {
            *counts.entry(batch.layer).or_insert(0usize) += 1;
        }
        let mut layer_batches = BTreeMap::new();
        for (layer, count) in counts {
            let mut indices = Vec::new();
            indices.try_reserve_exact(count)?;
            layer_batches.insert(layer, indices);
        }
        for (index, batch) in source.batches.iter().enumerate() {
            layer_batches
                .get_mut(&batch.layer)
                .context("GPU_COPPER_LAYER_MISSING")?
                .push(index);
        }
        let (ranges, batch_pages) = page_ranges(
            &source,
            vertex_bytes / size_of::<CopperVertex>(),
            index_bytes / 4,
        )?;
        let mut pages = Vec::new();
        pages.try_reserve_exact(ranges.len())?;
        for ranges in ranges {
            let vertex_buffer = if ranges.vertices.is_empty() {
                None
            } else {
                Some(
                    device
                        .buffer_empty(ranges.vertices.len() * size_of::<CopperVertex>(), false)?,
                )
            };
            let index_buffer = if ranges.indices.is_empty() {
                None
            } else {
                Some(device.buffer_empty(ranges.indices.len() * 4, true)?)
            };
            pages.push(CopperPage {
                ranges,
                vertex_buffer,
                index_buffer,
                vertex_uploaded: 0,
                index_uploaded: 0,
            });
        }
        Ok(Self {
            source,
            active: false,
            pages,
            batch_pages,
            layer_batches,
            #[cfg(test)]
            force_stencil: false,
        })
    }
    pub fn uploaded_bytes(&self) -> usize {
        self.pages
            .iter()
            .map(|page| page.vertex_uploaded * size_of::<CopperVertex>() + page.index_uploaded * 4)
            .sum()
    }
    #[cfg(all(test, target_os = "macos"))]
    pub fn upload_next(&mut self, context: &NativeGpuContext<'_>) -> anyhow::Result<()> {
        let mut budget = UPLOAD_BYTES_PER_FRAME;
        self.upload_with_budget(context, &mut budget)
    }
    pub fn upload_with_budget(
        &mut self,
        context: &NativeGpuContext<'_>,
        budget: &mut usize,
    ) -> anyhow::Result<()> {
        for page in &mut self.pages {
            if *budget == 0 {
                break;
            }
            if let Some(buffer) = &page.vertex_buffer {
                upload_slice(
                    &context.device,
                    buffer,
                    page.ranges.vertices.len(),
                    &mut page.vertex_uploaded,
                    budget,
                    |range| {
                        self.source.vertex_block(
                            range.start + page.ranges.vertices.start
                                ..range.end + page.ranges.vertices.start,
                        )
                    },
                )?;
            }
            if let Some(buffer) = &page.index_buffer {
                upload_slice(
                    &context.device,
                    buffer,
                    page.ranges.indices.len(),
                    &mut page.index_uploaded,
                    budget,
                    |range| {
                        let mut data = self.source.index_block(
                            range.start + page.ranges.indices.start
                                ..range.end + page.ranges.indices.start,
                        )?;
                        if page.ranges.vertices.start != 0 {
                            for index in data.to_mut() {
                                *index -= page.ranges.vertices.start as u32;
                            }
                        }
                        Ok(data)
                    },
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../../tests/unit/backend/wgpu/copper_pages.rs"]
mod tests;
fn upload_slice<'a, T: bytemuck::Pod + 'a>(
    device: &Device<'_>,
    buffer: &Buffer,
    total: usize,
    uploaded: &mut usize,
    budget: &mut usize,
    block: impl FnOnce(
        std::ops::Range<usize>,
    ) -> Result<std::borrow::Cow<'a, [T]>, crate::tracks::PrepareError>,
) -> anyhow::Result<()> {
    let count = (total - *uploaded).min(*budget / size_of::<T>());
    if count != 0 {
        let source = block(*uploaded..*uploaded + count)?;
        let data = bytemuck::cast_slice(source.as_ref());
        device.write_buffer(buffer, *uploaded * size_of::<T>(), data)?;
        *uploaded += count;
        *budget -= data.len();
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
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
pub(crate) struct Pipeline {
    pipeline: GpuPipeline,
}
struct DirectRun<'a> {
    page: &'a CopperPage,
    indices: std::ops::Range<u32>,
    uniforms: Uniforms,
    rect: Rect,
}
impl DirectRun<'_> {
    fn draw(self, context: &NativeGpuContext<'_>, pipeline: &GpuPipeline) -> anyhow::Result<()> {
        context.device.draw(
            pipeline,
            Draw {
                uniforms: bytemuck::bytes_of(&self.uniforms),
                vertices: self
                    .page
                    .vertex_buffer
                    .as_ref()
                    .context("GPU_COPPER_VERTEX_MISSING")?,
                indices: Some(
                    self.page
                        .index_buffer
                        .as_ref()
                        .context("GPU_COPPER_INDEX_MISSING")?,
                ),
                start: self.indices.start - self.page.ranges.indices.start as u32,
                count: self.indices.end - self.indices.start,
                instances: 1,
                rect: self.rect,
                stencil: StencilMode::Inherited,
                atlas: None,
            },
        )
    }
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
            annotation_owners,
            overrides,
            static_shapes_fill_solid,
        } = options;
        if cache.pages.iter().all(|page| page.index_uploaded == 0) {
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
        let mut pending: Option<DirectRun<'_>> = None;
        let indexed_layer = layer;
        // The pixel reference keeps the original whole-source scan.
        #[cfg(test)]
        let indexed_layer = indexed_layer.filter(|_| !cache.force_stencil);
        let indices = indexed_layer.and_then(|layer| cache.layer_batches.get(&layer));
        let count = if indexed_layer.is_some() {
            indices.map_or(0, Vec::len)
        } else {
            cache.source.batches.len()
        };
        for index in 0..count {
            let batch_index = indices.map_or(index, |indices| indices[index]);
            let batch = &cache.source.batches[batch_index];
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
                entry.source.vertex_count() == 0 || entry.source.index_count() == 0
            }) {
                continue;
            }
            let cache = replacement.unwrap_or(cache);
            let batch = replacement
                .and_then(|entry| entry.source.batches.first())
                .unwrap_or(batch);
            let page_index = cache.batch_pages[if replacement.is_some() {
                0
            } else {
                batch_index
            }];
            let page = &cache.pages[page_index];
            let overlay = frame.object_highlight(batch.selected_object, batch.net, None);
            if frame.pass != board::OverlayPass::Base && overlay.is_none() {
                continue;
            }
            let zone = matches!(
                batch.selected_object,
                pomelo_core::selection::SelectedObject::Zone(_)
            );
            let zone_overlay = zone && frame.pass != board::OverlayPass::Base;
            if zone && opacity <= 0.0 && !zone_overlay {
                continue;
            }
            // Zone overlays add sparse white dots over the unchanged base material.
            // Custom pads keep their own existing overlay treatment.
            uniforms.view[2] = if zone_overlay {
                6.0
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
            } else {
                STATIC_SHAPE_STIPPLE
            };
            let outer = batch.outer_indices();
            let holes = batch.hole_indices();
            if (batch.parity_rings.is_none() && outer.is_empty())
                || holes.end as usize > page.ranges.indices.start + page.index_uploaded
                || (batch.vertex_start as usize + batch.vertex_count as usize)
                    > page.ranges.vertices.start + page.vertex_uploaded
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
            uniforms.color = frame.material_color(batch.layer, batch.net);
            if let Some(color) = overlay {
                uniforms.color = color;
            }
            if frame.pass == board::OverlayPass::Base {
                uniforms.color[3] *= opacity;
            }
            let vertices = page
                .vertex_buffer
                .as_ref()
                .context("GPU_COPPER_VERTEX_MISSING")?;
            let indices = page
                .index_buffer
                .as_ref()
                .context("GPU_COPPER_INDEX_MISSING")?;
            let encode = |mode, range: Option<std::ops::Range<u32>>| -> anyhow::Result<()> {
                let (start, count, index_buffer) = range.map_or((0, 4, None), |range| {
                    (
                        range.start - page.ranges.indices.start as u32,
                        range.end - range.start,
                        Some(indices),
                    )
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
            let annotated = annotations.is_some()
                && annotation_owners.is_none_or(|owners| owners.contains(&batch.object));
            let direct = batch.parity_rings.is_none() && holes.is_empty() && !annotated;
            #[cfg(test)]
            let direct = direct && !cache.force_stencil;
            if direct {
                // The original earcut exterior is also the shade mesh. Without
                // holes or clipped labels its coverage needs no private stencil.
                // Keep inherited coverage if the caller already has a mask.
                // Indexed copper vertices do not read the clear-quad rectangle.
                // Merge only contiguous original indices with identical material
                // and view uniforms, preserving primitive and blending order.
                let mut direct_uniforms = uniforms;
                direct_uniforms.rectangle = [0.0; 4];
                if let Some(run) = pending.as_mut()
                    && std::ptr::eq(run.page, page)
                    && run.indices.end == outer.start
                    && run.uniforms == direct_uniforms
                {
                    run.indices.end = outer.end;
                    run.rect.left = run.rect.left.min(rect.left);
                    run.rect.top = run.rect.top.min(rect.top);
                    run.rect.right = run.rect.right.max(rect.right);
                    run.rect.bottom = run.rect.bottom.max(rect.bottom);
                } else {
                    if let Some(run) = pending.take() {
                        run.draw(context, &self.pipeline)?;
                        draws += 1;
                    }
                    pending = Some(DirectRun {
                        page,
                        indices: outer,
                        uniforms: direct_uniforms,
                        rect,
                    });
                }
                zones += 1;
                continue;
            }
            if let Some(run) = pending.take() {
                run.draw(context, &self.pipeline)?;
                draws += 1;
            }
            // End this private coverage scope before a subsequent direct batch,
            // otherwise it would inherit this zone's stale stencil contents.
            let _mask = context.device.stencil_scope(context.viewport)?;
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
            if annotated && let Some(labels) = annotations.as_mut() {
                labels(batch)?;
            }
            zones += 1;
        }
        if let Some(run) = pending {
            run.draw(context, &self.pipeline)?;
            draws += 1;
        }
        Ok((draws, zones))
    }
}
