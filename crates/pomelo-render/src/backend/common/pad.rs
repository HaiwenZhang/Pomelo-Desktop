//! Portable native pipelines using the common command driver.
use super::super::{TraceFrame, board};
use super::Rect;
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::pad_index::PadIndex;
use super::pad_overlay::OverlayKey;
use super::pad_ranges::PadRanges;
use super::pad_visibility::Accepted;
use super::trace::{CHUNK_INSTANCES, CHUNKS_PER_FRAME};
use super::{Device, NativeGpuContext};
use crate::pads::{PadInstance, PreparedPads};
use crate::split_position;
use anyhow::ensure;
use pomelo_core::selection::{SelectedObject, SelectionTarget};
use pomelo_core::{interaction::Camera, model::LayerId};
use std::{cell::RefCell, sync::Arc};
#[derive(Clone, Copy)]
pub(crate) enum Visibility<'a> {
    All,
    Predicate(&'a dyn Fn(&PadInstance) -> bool),
    Pads { pin: bool, show_backdrills: bool },
    Drills(&'a Arc<pomelo_core::display::BoardDisplay>),
}
impl Visibility<'_> {
    fn accepts(self, source: &PreparedPads, instance: &PadInstance) -> bool {
        match self {
            Self::All => true,
            Self::Predicate(predicate) => predicate(instance),
            Self::Pads {
                pin,
                show_backdrills,
            } => {
                (instance.source[0] == 0) == pin
                    && (instance.source[3] & 4 == 0 || !show_backdrills)
            }
            Self::Drills(display) => {
                let backdrill = instance.source[3] & 2 != 0;
                let scopes = if backdrill {
                    &source.backdrill_scopes
                } else {
                    &source.drill_scopes
                };
                let scope = (instance.source[0] != 0).then(|| {
                    scopes
                        .as_ref()
                        .and_then(|scopes| scopes.get(instance.source[1] as usize))
                        .map_or(&[][..], Vec::as_slice)
                });
                if backdrill {
                    display.backdrill_visible(scope.unwrap_or(&[]))
                } else {
                    display.drill_visible(scope)
                }
            }
        }
    }
}
struct DrillVisibility {
    display: Arc<pomelo_core::display::BoardDisplay>,
    accepted: Accepted,
}
#[derive(Clone, Copy)]
struct SelectedNetSummary {
    net: u32,
    count: usize,
    runs: usize,
    extra_hover: bool,
}
struct Chunk {
    start: usize,
    count: usize,
    buffer: Buffer,
    ranges: PadRanges,
    index: PadIndex,
    pad_visibility: RefCell<[Option<Accepted>; 4]>,
    drill_visibility: RefCell<Option<DrillVisibility>>,
    overlay_visibility: RefCell<[Option<(OverlayKey, Accepted)>; 2]>,
}
impl Chunk {
    fn overlay(
        &self,
        source: &PreparedPads,
        frame: &TraceFrame,
        key: &OverlayKey,
        range: std::ops::Range<usize>,
        emit: impl FnMut(std::ops::Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let slot = usize::from(frame.pass != board::OverlayPass::Selection);
        let mut entries = self.overlay_visibility.borrow_mut();
        if entries[slot]
            .as_ref()
            .is_none_or(|(cached, _)| cached != key)
        {
            let accepted = Accepted::build(
                source.analytic[self.start..self.start + self.count]
                    .iter()
                    .map(|instance| highlight(frame, instance).is_some()),
            )?;
            entries[slot] = Some((key.clone(), accepted));
        }
        entries[slot]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU_PAD_OVERLAY_MISSING"))?
            .1
            .visible(range, emit)
    }
    fn visible(
        &self,
        source: &PreparedPads,
        visibility: Visibility<'_>,
        range: std::ops::Range<usize>,
        legacy: bool,
        mut emit: impl FnMut(std::ops::Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let build = || {
            Accepted::build(
                source.analytic[self.start..self.start + self.count]
                    .iter()
                    .map(|instance| visibility.accepts(source, instance)),
            )
        };
        if !legacy {
            match visibility {
                Visibility::All => return emit(range),
                Visibility::Pads {
                    pin,
                    show_backdrills,
                } => {
                    let slot = usize::from(pin) + usize::from(show_backdrills) * 2;
                    let mut entries = self.pad_visibility.borrow_mut();
                    if entries[slot].is_none() {
                        entries[slot] = Some(build()?);
                    }
                    return entries[slot]
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GPU_PAD_VISIBILITY_MISSING"))?
                        .visible(range, emit);
                }
                Visibility::Drills(display) => {
                    let mut entry = self.drill_visibility.borrow_mut();
                    if entry
                        .as_ref()
                        .is_none_or(|entry| !Arc::ptr_eq(&entry.display, display))
                    {
                        *entry = Some(DrillVisibility {
                            display: Arc::clone(display),
                            accepted: build()?,
                        });
                    }
                    return entry
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GPU_DRILL_VISIBILITY_MISSING"))?
                        .accepted
                        .visible(range, emit);
                }
                Visibility::Predicate(_) => {}
            }
        }
        let mut cursor = range.start;
        while cursor < range.end {
            while cursor < range.end
                && !visibility.accepts(source, &source.analytic[self.start + cursor])
            {
                cursor += 1;
            }
            let start = cursor;
            while cursor < range.end
                && visibility.accepts(source, &source.analytic[self.start + cursor])
            {
                cursor += 1;
            }
            if start < cursor {
                emit(start..cursor)?;
            }
        }
        Ok(())
    }
    fn selected_net_summary(
        &self,
        net: u32,
        hovered: Option<SelectedObject>,
    ) -> SelectedNetSummary {
        let (count, runs) = self.index.summary(net);
        SelectedNetSummary {
            net,
            count,
            runs,
            extra_hover: self.index.extra_hover(net, hovered),
        }
    }
}
pub(crate) struct UploadedPads {
    pub source: Arc<PreparedPads>,
    chunks: Vec<Chunk>,
    uploaded: usize,
    #[cfg(test)]
    cpu_net_selection: bool,
    #[cfg(test)]
    pub legacy_visibility: bool,
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
            #[cfg(test)]
            cpu_net_selection: false,
            #[cfg(test)]
            legacy_visibility: false,
        })
    }
    pub fn uploaded(&self) -> usize {
        self.uploaded
    }
    #[cfg(test)]
    pub(crate) fn use_cpu_net_selection(&mut self) {
        self.cpu_net_selection = true;
    }
    #[cfg(test)]
    pub(crate) fn disable_range_culling(&mut self) -> anyhow::Result<()> {
        for chunk in &mut self.chunks {
            chunk.ranges = PadRanges::build(std::iter::repeat_n(
                [
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::INFINITY,
                ],
                chunk.count,
            ))?;
        }
        Ok(())
    }
    pub fn upload_next(&mut self, device: &Device<'_>, budget: &mut usize) -> anyhow::Result<u64> {
        let mut bytes = 0;
        for _ in 0..CHUNKS_PER_FRAME {
            let start = self.uploaded;
            let count = CHUNK_INSTANCES.min(self.source.analytic.len() - start);
            if count == 0 {
                break;
            }
            let data = bytemuck::cast_slice(&self.source.analytic[start..start + count]);
            if data.len() > *budget {
                break;
            }
            let buffer = device.structured_buffer(data, size_of::<PadInstance>())?;
            let ranges = PadRanges::build(self.source.analytic[start..start + count].iter().map(
                |pad| {
                    let low = pad.bounds_min;
                    let high = pad.bounds_max;
                    [
                        f64::from(low[0]) + f64::from(low[2]),
                        f64::from(low[1]) + f64::from(low[3]),
                        f64::from(high[0]) + f64::from(high[2]),
                        f64::from(high[1]) + f64::from(high[3]),
                    ]
                },
            ))?;
            let index = PadIndex::build(&self.source.analytic[start..start + count])?;
            bytes += data.len() as u64;
            *budget -= data.len();
            self.chunks.push(Chunk {
                start,
                count,
                buffer,
                ranges,
                index,
                pad_visibility: RefCell::new([None, None, None, None]),
                drill_visibility: RefCell::new(None),
                overlay_visibility: RefCell::new([None, None]),
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
    #[cfg(test)]
    pub fn draw(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedPads,
        layer: Option<LayerId>,
        visible: Option<&dyn Fn(&PadInstance) -> bool>,
    ) -> anyhow::Result<u64> {
        self.draw_scoped(
            context,
            frame,
            cache,
            layer,
            visible.map_or(Visibility::All, Visibility::Predicate),
        )
    }
    pub(crate) fn draw_scoped(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedPads,
        layer: Option<LayerId>,
        visibility: Visibility<'_>,
    ) -> anyhow::Result<u64> {
        #[cfg(all(target_os = "windows", not(test)))]
        let _draw_scope = context.device.draw_scope();
        #[cfg(all(target_os = "windows", test))]
        let _draw_scope = (!cache.legacy_visibility).then(|| context.device.draw_scope());
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
        let half_width = f64::from(width) * 0.5 / camera.pixels_per_mm;
        let half_height = f64::from(height) * 0.5 / camera.pixels_per_mm;
        // Fit views benefit from the original contiguous submission; range pruning
        // can fragment source ranges for pads extending beyond the board outline.
        let covers_board = camera.center.x - half_width <= frame.bounds.min.x
            && camera.center.x + half_width >= frame.bounds.max.x
            && camera.center.y - half_height <= frame.bounds.min.y
            && camera.center.y + half_height >= frame.bounds.max.y;
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
        #[cfg(not(test))]
        let legacy = false;
        #[cfg(test)]
        let legacy = cache.legacy_visibility;
        let uniform_base = !legacy
            && frame.pass == board::OverlayPass::Base
            && frame.highlighted_object.is_none()
            && frame
                .highlighted_objects
                .as_ref()
                .is_none_or(|(objects, _)| objects.is_empty())
            && frame
                .highlighted_related_objects
                .as_ref()
                .is_none_or(|(objects, _)| objects.is_empty())
            && frame.hovered_object.is_none();
        let selected_net = overlay_net(frame);
        let overlay_key =
            (!legacy && frame.pass != board::OverlayPass::Base).then(|| OverlayKey::new(frame));
        #[cfg(test)]
        let selected_net = selected_net.filter(|_| !cache.cpu_net_selection);
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
                if !legacy
                    && let Visibility::Pads { pin, .. } = visibility
                    && !chunk.index.contains_category(pin)
                {
                    continue;
                }
                let summary =
                    selected_net.map(|(net, hovered)| chunk.selected_net_summary(net.0, hovered));
                if summary.is_some_and(|summary| summary.count == 0 && !summary.extra_hover) {
                    continue;
                }
                // Avoid streaming sparse nets through the GPU merely to reject them.
                // Dense, fragmented chunks amortize one instanced submission instead.
                let shader_net = summary.filter(|summary| {
                    !summary.extra_hover && summary.count >= 128 && summary.runs >= 32
                });
                let cached_overlay = overlay_key.as_ref().filter(|_| shader_net.is_none());
                // Typed pad visibility can reject whole quads without splitting
                // alternating Pin/Via instances into individual CPU submissions.
                // Keep the exact CPU path when per-object base colors differ.
                let shader_visibility = !legacy
                    && (uniform_base || shader_net.is_some() || cached_overlay.is_some())
                    && matches!(visibility, Visibility::Pads { .. });
                uniforms.batch[2] &= 1;
                if shader_visibility
                    && let Visibility::Pads {
                        pin,
                        show_backdrills,
                    } = visibility
                {
                    // Bit 0 remains base net highlighting; bits 2/3 select
                    // Pin/Via, and bit 4 suppresses replaced backdrill pads.
                    uniforms.batch[2] |= if pin { 4 } else { 8 };
                    uniforms.batch[2] |= u32::from(show_backdrills) * 16;
                }
                let magnitude = chunk
                    .ranges
                    .coordinate_magnitude()
                    .max(camera.center.x.abs())
                    .max(camera.center.y.abs());
                let padding = 2.0 * f64::from(frame.scale_factor) / camera.pixels_per_mm
                    + magnitude * f64::from(f32::EPSILON).powi(2) * 8.0
                    + half_width.max(half_height) * f64::from(f32::EPSILON) * 8.0;
                let view = [
                    camera.center.x - half_width - padding,
                    camera.center.y - half_height - padding,
                    camera.center.x + half_width + padding,
                    camera.center.y + half_height + padding,
                ];
                let mut draw_range = |range: std::ops::Range<usize>| {
                    let start = range.start + chunk.start;
                    let end = range.end + chunk.start;
                    let selected = |index: usize| highlight(frame, &cache.source.analytic[index]);
                    let mut cursor = start;
                    while cursor < end {
                        let start = cursor;
                        let highlighted = if shader_net.is_some() || cached_overlay.is_some() {
                            Some(if frame.pass == board::OverlayPass::Selection {
                                [1.0, 1.0, 1.0, 0.9]
                            } else {
                                [0.63, 1.0, 0.85, 0.9]
                            })
                        } else if uniform_base {
                            None
                        } else {
                            (cursor < end).then(|| selected(cursor)).flatten()
                        };
                        if uniform_base || shader_net.is_some() || cached_overlay.is_some() {
                            cursor = end;
                        } else {
                            while cursor < end && selected(cursor) == highlighted {
                                cursor += 1;
                            }
                        }
                        let end = cursor;
                        if start == end {
                            continue;
                        }
                        if frame.pass != board::OverlayPass::Base && highlighted.is_none() {
                            continue;
                        }
                        // Mode 2 preserves source order in one instanced range and
                        // collapses nonmatching net quads before rasterization.
                        uniforms.batch[3] = if shader_net.is_some() {
                            2
                        } else {
                            u32::from(highlighted.is_some())
                        };
                        if let Some(summary) = shader_net {
                            uniforms.batch[1] = summary.net;
                        }
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
                    Ok(())
                };
                let range = start - chunk.start..end - chunk.start;
                let mut visible_range = |range| {
                    let mut visible = |range: std::ops::Range<usize>| {
                        // Short camera leaves are cheaper to filter on the CPU;
                        // streaming them doubles vertex work for little saving.
                        if shader_visibility && range.len() >= 128 {
                            draw_range(range)
                        } else {
                            chunk.visible(&cache.source, visibility, range, legacy, &mut draw_range)
                        }
                    };
                    if let Some(key) = cached_overlay {
                        chunk.overlay(&cache.source, frame, key, range, visible)
                    } else {
                        visible(range)
                    }
                };
                if covers_board {
                    visible_range(range)?;
                } else {
                    chunk.ranges.visible(view, range, visible_range)?;
                }
            }
        }
        Ok(draws)
    }
}

fn highlight(frame: &TraceFrame, instance: &PadInstance) -> Option<[f32; 4]> {
    frame.object_highlight(
        object(instance),
        pomelo_core::model::NetId(instance.ids[2]),
        None,
    )
}

fn object(instance: &PadInstance) -> SelectedObject {
    let id = pomelo_core::model::ObjectId(instance.ids[0]);
    if instance.source[0] == 0 {
        SelectedObject::Pin(id)
    } else {
        SelectedObject::Via(id)
    }
}

pub(super) fn overlay_net<S: super::trace::InstanceSource>(
    frame: &TraceFrame<S>,
) -> Option<(pomelo_core::model::NetId, Option<SelectedObject>)> {
    if let Some(net) = frame.selected_net_only() {
        return Some((net, None));
    }
    if !matches!(
        frame.pass,
        board::OverlayPass::Hover | board::OverlayPass::GroupHover
    ) {
        return None;
    }
    let (SelectionTarget::Net(net), _) = frame.hover_selection.as_ref()? else {
        return None;
    };
    // Distinct sole selected/hovered nets cannot share an instance. Other selection
    // predicates and same-net exclusion still require exact cached membership.
    if frame.has_overlay(board::OverlayPass::Selection)
        && frame
            .sole_selected_net()
            .is_none_or(|selected| selected == *net)
    {
        return None;
    }
    // A pointer object is usually in the hovered net. Each chunk verifies that
    // implication before using net-only GPU rejection; extra objects retain the
    // exact cached membership path, including objects on unconnected nets.
    (net.0 != 0).then_some((*net, frame.hovered_object.map(|(object, _)| object)))
}
