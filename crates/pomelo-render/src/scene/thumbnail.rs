//! Bounded CPU thumbnails from the same immutable batches as the native viewport.
//! This overview deliberately omits text and view-specific selection/filter state.
use super::{copper::PreparedCopper, pads::PreparedPads, tracks::PreparedTracks};
use crate::tracks::PrepareError;
use pomelo_core::{
    model::{Arc, Bounds, LayerId, ObjectId, Pad, PadKind, Point, Segment},
    pad::PadPlacement,
    picking::segment_distance_mm,
    task::CancellationToken,
};

pub const WIDTH: usize = 192;
pub const HEIGHT: usize = 128;

pub struct Source<'a> {
    pub bounds: Bounds,
    pub tracks: &'a PreparedTracks,
    pub drawings: &'a PreparedTracks,
    pub copper: &'a PreparedCopper,
    pub pads: &'a PreparedPads,
    pub drills: &'a PreparedPads,
}

/// Canvas material policy supplied by the application; no UI or platform dependency.
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: [u8; 3],
    pub copper: [u8; 3],
    pub pads: [u8; 3],
    pub outline: [u8; 3],
    pub drawings: [u8; 3],
}

#[derive(Clone, Copy)]
pub struct Limits {
    /// Evenly sample each instance category rather than only the first part of a board.
    pub instances_per_pass: usize,
    pub triangles_per_pass: usize,
    /// At most seven passes, each with independently bounded pixel work.
    pub pixels_per_pass: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            instances_per_pass: 20_000,
            triangles_per_pass: 100_000,
            pixels_per_pass: 2_000_000,
        }
    }
}

#[derive(Default, Debug)]
pub struct Summary {
    pub tracks: usize,
    pub drawings: usize,
    pub copper: usize,
    pub pads: usize,
    pub drills: usize,
    pub outlines: usize,
    /// True when overview LOD omits any sampled primitive or whole coverage batch.
    pub limited: bool,
}

pub struct Thumbnail {
    /// Opaque RGBA pixels, row-major. GPUI conversion and PNG persistence belong to the app.
    pub rgba: Vec<u8>,
    pub summary: Summary,
}

pub fn rasterize(
    source: Source<'_>,
    palette: Palette,
    limits: Limits,
    cancel: &CancellationToken,
) -> Result<Option<Thumbnail>, PrepareError> {
    check_cancelled(cancel)?;
    let Some(projection) = Projection::new(source.bounds) else {
        return Ok(None);
    };
    let mut raster = Raster {
        projection,
        pixels: vec![palette.background; WIDTH * HEIGHT],
        remaining: limits.pixels_per_pass,
        limited: false,
        cancel,
    };
    let copper = raster.meshes(source.copper, palette.copper, 0.28, limits)?;
    raster.reset_budget(limits);
    let tracks = raster.traces(source.tracks, false, palette.copper, limits)?;
    raster.reset_budget(limits);
    let drawings = raster.traces(source.drawings, false, palette.drawings, limits)?;
    raster.reset_budget(limits);
    let mut pads = raster.analytic_pads(source.pads, palette.pads, limits)?;
    raster.reset_budget(limits);
    if let Some(mesh) = &source.pads.custom_mesh {
        pads += raster.meshes(mesh, palette.pads, 1.0, limits)?;
    }
    raster.reset_budget(limits);
    let drills = raster.analytic_pads(source.drills, palette.background, limits)?;
    raster.reset_budget(limits);
    let outlines = raster.traces(source.tracks, true, palette.outline, limits)?;
    check_cancelled(cancel)?;
    let mut rgba = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for pixel in raster.pixels {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    Ok(Some(Thumbnail {
        rgba,
        summary: Summary {
            tracks,
            drawings,
            copper,
            pads,
            drills,
            outlines,
            limited: raster.limited,
        },
    }))
}

struct Projection {
    center: Point,
    scale: f64,
}
impl Projection {
    fn new(bounds: Bounds) -> Option<Self> {
        if !bounds.is_valid() {
            return None;
        }
        let width = bounds.max.x - bounds.min.x;
        let height = bounds.max.y - bounds.min.y;
        let scale = (176.0 / width).min(112.0 / height);
        (scale.is_finite() && scale > 0.0).then_some(Self {
            center: bounds.center(),
            scale,
        })
    }
    fn screen(&self, point: Point) -> Point {
        Point::new(
            96.0 + (point.x - self.center.x) * self.scale,
            64.0 - (point.y - self.center.y) * self.scale,
        )
    }
    fn board(&self, x: usize, y: usize) -> Point {
        Point::new(
            self.center.x + (x as f64 + 0.5 - 96.0) / self.scale,
            self.center.y - (y as f64 + 0.5 - 64.0) / self.scale,
        )
    }
    fn rect(&self, bounds: Bounds, padding: f64) -> PixelRect {
        let a = self.screen(bounds.min);
        let b = self.screen(bounds.max);
        PixelRect {
            x0: (a.x.min(b.x) - padding).floor().clamp(0.0, WIDTH as f64) as usize,
            y0: (a.y.min(b.y) - padding).floor().clamp(0.0, HEIGHT as f64) as usize,
            x1: (a.x.max(b.x) + padding).ceil().clamp(0.0, WIDTH as f64) as usize,
            y1: (a.y.max(b.y) + padding).ceil().clamp(0.0, HEIGHT as f64) as usize,
        }
    }
}
#[derive(Clone, Copy)]
struct PixelRect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}
impl PixelRect {
    fn area(self) -> usize {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

struct Raster<'a> {
    projection: Projection,
    pixels: Vec<[u8; 3]>,
    remaining: usize,
    limited: bool,
    cancel: &'a CancellationToken,
}
impl Raster<'_> {
    fn reset_budget(&mut self, limits: Limits) {
        self.remaining = limits.pixels_per_pass;
    }
    fn reserve(&mut self, pixels: usize) -> bool {
        if pixels > self.remaining {
            self.limited = true;
            false
        } else {
            self.remaining -= pixels;
            true
        }
    }
    fn blend(&mut self, index: usize, color: [u8; 3], opacity: f64) {
        for (destination, source) in self.pixels[index].iter_mut().zip(color) {
            *destination = (f64::from(*destination) * (1.0 - opacity) + f64::from(source) * opacity)
                .round() as u8;
        }
    }
    fn distance_shape(
        &mut self,
        bounds: Bounds,
        color: [u8; 3],
        distance: impl Fn(Point) -> Result<f64, PrepareError>,
    ) -> Result<bool, PrepareError> {
        let rect = self.projection.rect(bounds, 1.0);
        if !self.reserve(rect.area()) {
            return Ok(false);
        }
        for y in rect.y0..rect.y1 {
            check_cancelled(self.cancel)?;
            for x in rect.x0..rect.x1 {
                let signed = distance(self.projection.board(x, y))?;
                let opacity = (0.5 - signed * self.projection.scale).clamp(0.0, 1.0);
                if opacity > 0.0 {
                    self.blend(y * WIDTH + x, color, opacity);
                }
            }
        }
        Ok(true)
    }
    fn traces(
        &mut self,
        tracks: &PreparedTracks,
        outline: bool,
        color: [u8; 3],
        limits: Limits,
    ) -> Result<usize, PrepareError> {
        let stride = stride(tracks.instances.len(), limits.instances_per_pass);
        let mut count = 0;
        for (index, instance) in tracks.instances.iter().enumerate() {
            if index.is_multiple_of(1024) {
                check_cancelled(self.cancel)?;
            }
            if (instance.flags[3] & 4 != 0) != outline {
                continue;
            }
            // Preserve every outline edge until its independent pixel budget is exhausted.
            if !outline && !index.is_multiple_of(stride) {
                self.limited = true;
                continue;
            }
            let id = ObjectId(instance.ids[0]);
            let sweep = f64::from(instance.arc[3]);
            // Shader angles are float32, so a full turn can exceed f64 TAU after
            // conversion. Retain the explicit full-circle contract before CPU queries.
            let sweep = if instance.flags[3] & 1 != 0 {
                sweep.signum() * std::f64::consts::TAU
            } else {
                sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU)
            };
            let segment = Segment {
                id,
                track_id: ObjectId(instance.ids[1]),
                layer: LayerId(instance.ids[2]),
                net: pomelo_core::model::NetId(instance.ids[3]),
                a: point(instance.a),
                b: point(instance.b),
                width: 0.0,
                arc: (instance.flags[0] != 0).then_some(Arc {
                    center: point(instance.center),
                    radius: f64::from(instance.arc[0]) + f64::from(instance.arc[1]),
                    start: f64::from(instance.arc[2]),
                    sweep,
                }),
                bond_wire: None,
            };
            let radius = (f64::from(f32::from_bits(instance.flags[2])) * 0.5)
                .max(0.35 / self.projection.scale);
            let bounds = Bounds {
                min: point(instance.bounds_min),
                max: point(instance.bounds_max),
            };
            if self.distance_shape(bounds, color, |at| {
                segment_distance_mm(&segment, at)
                    .map(|distance| distance - radius)
                    .map_err(|_| PrepareError::Invalid(id))
            })? {
                count += 1;
            }
        }
        Ok(count)
    }
    fn analytic_pads(
        &mut self,
        pads: &PreparedPads,
        color: [u8; 3],
        limits: Limits,
    ) -> Result<usize, PrepareError> {
        let stride = stride(pads.analytic.len(), limits.instances_per_pass);
        self.limited |= stride > 1;
        let mut count = 0;
        for instance in pads.analytic.iter().step_by(stride) {
            check_cancelled(self.cancel)?;
            let id = ObjectId(instance.ids[0]);
            let pad = Pad {
                layer: LayerId(instance.ids[1]),
                width: f64::from(instance.shape[0]),
                height: f64::from(instance.shape[1]),
                offset: Point::default(),
                kind: PadKind(instance.ids[3] as u16),
                corner: f64::from(instance.shape[2]),
                inner_diameter: (instance.ids[3] == u32::from(PadKind::DONUT.0))
                    .then_some(f64::from(instance.shape[3])),
                custom: None,
                backdrill: false,
                backdrill_base: false,
            };
            let placement = PadPlacement {
                at: point(instance.center),
                angle: f64::from(instance.rotation[1]).atan2(f64::from(instance.rotation[0])),
                mirrored: false,
            };
            if self.distance_shape(
                Bounds {
                    min: point(instance.bounds_min),
                    max: point(instance.bounds_max),
                },
                color,
                |at| {
                    pad.analytic_distance(at, placement)
                        .ok_or(PrepareError::Invalid(id))
                },
            )? {
                count += 1;
            }
        }
        Ok(count)
    }
    fn meshes(
        &mut self,
        copper: &PreparedCopper,
        color: [u8; 3],
        opacity: f64,
        limits: Limits,
    ) -> Result<usize, PrepareError> {
        let mut mask = vec![0_u8; WIDTH * HEIGHT];
        let stride = stride(copper.batches.len(), limits.instances_per_pass);
        self.limited |= stride > 1;
        let mut triangles_remaining = limits.triangles_per_pass;
        let mut count = 0;
        for batch in copper.batches.iter().step_by(stride) {
            check_cancelled(self.cancel)?;
            let outer = batch.outer_indices();
            let holes = batch.hole_indices();
            let triangles = (outer.len() + holes.len()) / 3;
            if triangles > triangles_remaining {
                self.limited = true;
                continue;
            }
            let Some(bounds) = batch.bounds else { continue };
            let rect = self.projection.rect(bounds, 0.0);
            // The triangle quota bounds this conversion even for source-backed copper.
            let batch_indices = copper.index_block(outer.start as usize..holes.end as usize)?;
            let mut cost = rect.area().saturating_mul(2);
            // Reserve a complete batch, including all holes, before changing the image.
            // A limited thumbnail never fills a hole just because its budget ran out.
            for indices in batch_indices.as_chunks::<3>().0 {
                check_cancelled(self.cancel)?;
                cost = cost.saturating_add(self.triangle(copper, indices, batch.object)?.1.area());
                if cost > self.remaining {
                    break;
                }
            }
            if !self.reserve(cost) {
                continue;
            }
            triangles_remaining -= triangles;
            for y in rect.y0..rect.y1 {
                mask[y * WIDTH + rect.x0..y * WIDTH + rect.x1].fill(0);
            }
            for (indices, subtract) in [(outer, false), (holes, true)] {
                for triangle in batch_indices[(indices.start - batch.outer_indices().start) as usize
                    ..(indices.end - batch.outer_indices().start) as usize]
                    .as_chunks::<3>()
                    .0
                {
                    check_cancelled(self.cancel)?;
                    let (points, triangle_rect) = self.triangle(copper, triangle, batch.object)?;
                    for y in triangle_rect.y0..triangle_rect.y1 {
                        for x in triangle_rect.x0..triangle_rect.x1 {
                            let mut coverage = 0;
                            for (sample, (dx, dy)) in
                                [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                                    .into_iter()
                                    .enumerate()
                            {
                                if inside_triangle(points, Point::new(x as f64 + dx, y as f64 + dy))
                                {
                                    coverage |= 1 << sample;
                                }
                            }
                            let value = &mut mask[y * WIDTH + x];
                            if subtract {
                                *value &= !coverage;
                            } else {
                                *value |= coverage;
                            }
                        }
                    }
                }
            }
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let index = y * WIDTH + x;
                    self.blend(
                        index,
                        color,
                        opacity * f64::from(mask[index].count_ones()) / 4.0,
                    );
                }
            }
            count += 1;
        }
        Ok(count)
    }
    fn triangle(
        &self,
        copper: &PreparedCopper,
        indices: &[u32],
        object: ObjectId,
    ) -> Result<([Point; 3], PixelRect), PrepareError> {
        let mut points = [Point::default(); 3];
        for (destination, index) in points.iter_mut().zip(indices) {
            let vertex = copper
                .vertex_at(*index as usize)
                .ok_or(PrepareError::Invalid(object))?;
            *destination = point(vertex.position);
        }
        let bounds = Bounds::from_points(points)
            .filter(|bounds| bounds.is_valid())
            .ok_or(PrepareError::Invalid(object))?;
        Ok((
            points.map(|point| self.projection.screen(point)),
            self.projection.rect(bounds, 0.0),
        ))
    }
}
fn point(value: [f32; 4]) -> Point {
    Point::new(
        f64::from(value[0]) + f64::from(value[2]),
        f64::from(value[1]) + f64::from(value[3]),
    )
}
fn stride(count: usize, limit: usize) -> usize {
    count.div_ceil(limit.max(1)).max(1)
}
fn check_cancelled(cancel: &CancellationToken) -> Result<(), PrepareError> {
    if cancel.is_cancelled() {
        Err(PrepareError::Cancelled)
    } else {
        Ok(())
    }
}
fn inside_triangle([a, b, c]: [Point; 3], at: Point) -> bool {
    let cross =
        |a: Point, b: Point, p: Point| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let area = cross(a, b, c);
    if area.abs() < 1e-12 {
        return false;
    }
    let signs = [cross(a, b, at), cross(b, c, at), cross(c, a, at)];
    signs.iter().all(|value| *value >= 0.0) || signs.iter().all(|value| *value <= 0.0)
}

#[cfg(test)]
#[path = "../../tests/unit/scene/thumbnail.rs"]
mod tests;
