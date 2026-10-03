//! View-dependent zone contours and immutable cache snapshots, without GPU types.

use super::copper::{CopperBatch, CopperVertex, ParityRing, PreparedCopper};
use crate::{split_position, tracks::PrepareError};
use pomelo_core::{
    copper::MeshError,
    display::BoardDisplay,
    geometry::curve::{CurveLimits, tessellate_curve_ring},
    interaction::Camera,
    model::{BoardScene, Bounds, ObjectId, Point, Zone},
    task::CancellationToken,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveView {
    pub bounds: Bounds,
    pub tolerance: f64,
}
impl CurveView {
    pub fn new(camera: Camera, width: f64, height: f64, dpi: f64) -> Option<Self> {
        let scale = camera.pixels_per_mm * dpi;
        if ![width, height, dpi, scale]
            .into_iter()
            .all(|n| n.is_finite() && n > 0.0)
            || scale <= 1000.0
        {
            return None;
        }
        let a = camera.view_to_board(Point::default(), width, height);
        let b = camera.view_to_board(Point::new(width, height), width, height);
        let bounds = Bounds {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        };
        let tolerance = 0.25 / 2_f64.powf(scale.log2().ceil());
        (bounds.is_valid() && tolerance.is_finite() && tolerance > 0.0)
            .then_some(Self { bounds, tolerance })
    }
    fn overscan(self) -> Bounds {
        let dx = (self.bounds.max.x - self.bounds.min.x) / 2.0;
        let dy = (self.bounds.max.y - self.bounds.min.y) / 2.0;
        Bounds {
            min: Point::new(self.bounds.min.x - dx, self.bounds.min.y - dy),
            max: Point::new(self.bounds.max.x + dx, self.bounds.max.y + dy),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurveEntry {
    pub source: Arc<PreparedCopper>,
    pub view: Bounds,
    pub tolerance: f64,
    last_used: u64,
}
#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct CurveStatistics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub bytes: usize,
    pub over_budget_bytes: usize,
}
#[derive(Debug, Default)]
pub struct CurveFillCache {
    pub entries: BTreeMap<ObjectId, CurveEntry>,
    pub active: BTreeSet<ObjectId>,
    pub statistics: CurveStatistics,
    revision: u64,
    scene: Option<Arc<BoardScene>>,
}
#[derive(Debug, Clone, Copy)]
pub struct CurveCacheLimits {
    pub contours: CurveLimits,
    pub soft_bytes: usize,
    /// Prepared cache output only; excludes borrowed snapshots and worker scratch.
    pub max_bytes: usize,
}
impl Default for CurveCacheLimits {
    fn default() -> Self {
        Self {
            contours: CurveLimits::default(),
            soft_bytes: 64 * 1024 * 1024,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}
impl CurveFillCache {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn inactive(&self, soft_bytes: usize) -> Self {
        let mut result = Self {
            entries: self.entries.clone(),
            statistics: self.statistics,
            revision: self.revision,
            scene: self.scene.clone(),
            ..Self::default()
        };
        let mut bytes = result
            .entries
            .values()
            .map(|e| entry_bytes(&e.source))
            .sum();
        result.evict(&mut bytes, soft_bytes);
        result.statistics.bytes = bytes;
        result.statistics.over_budget_bytes = bytes.saturating_sub(soft_bytes);
        result
    }
    /// Run on a worker. Cancellation never mutates/publishes a partial snapshot;
    /// reused entries retain Arc identity and higher detail on zoom-out.
    pub fn prepare(
        scene: &Arc<BoardScene>,
        view: CurveView,
        display: &BoardDisplay,
        previous: &Self,
        limits: CurveCacheLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        check_cancelled(cancel)?;
        if !view.bounds.is_valid() || !view.tolerance.is_finite() || view.tolerance <= 0.0 {
            return Err(PrepareError::Invalid(ObjectId(0)));
        }
        let mut result = Self {
            entries: if previous
                .scene
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, scene))
            {
                previous.entries.clone()
            } else {
                BTreeMap::new()
            },
            statistics: previous.statistics,
            revision: previous.revision.saturating_add(1),
            scene: Some(Arc::clone(scene)),
            ..Self::default()
        };
        let cached_view = view.overscan();
        if !cached_view.is_valid() {
            return Err(PrepareError::Invalid(ObjectId(0)));
        }
        let mut bytes = result
            .entries
            .values()
            .map(|e| entry_bytes(&e.source))
            .sum::<usize>();
        for zone in &scene.zones {
            check_cancelled(cancel)?;
            if !display.filled
                || !display.show_copper
                || !display.layer_visible(zone.layer)
                || zone.paths.is_empty()
                || !zone.mesh.curved
                || zone
                    .mesh
                    .ring_bounds
                    .first()
                    .is_none_or(|b| !overlaps(*b, view.bounds))
            {
                continue;
            }
            result.active.insert(zone.id);
        }
        // Pin the entire visible working set before any eviction, including
        // zones which occur later in source order.
        for (source_index, zone) in scene.zones.iter().enumerate() {
            check_cancelled(cancel)?;
            if !result.active.contains(&zone.id) {
                continue;
            }
            if let Some(entry) = result.entries.get_mut(&zone.id)
                && entry.tolerance <= view.tolerance
                && contains(entry.view, view.bounds)
            {
                entry.last_used = result.revision;
                result.statistics.hits += 1;
                continue;
            }
            if let Some(old) = result.entries.remove(&zone.id) {
                bytes -= entry_bytes(&old.source);
            }
            result.evict(&mut bytes, limits.soft_bytes);
            let source = prepare_zone(
                zone,
                source_index,
                cached_view,
                view.tolerance,
                limits.contours,
                limits.max_bytes.saturating_sub(bytes),
                cancel,
            )?;
            bytes = bytes.saturating_add(entry_bytes(&source));
            check_limit(bytes, limits.max_bytes)?;
            result.entries.insert(
                zone.id,
                CurveEntry {
                    source: Arc::new(source),
                    view: cached_view,
                    tolerance: view.tolerance,
                    last_used: result.revision,
                },
            );
            result.statistics.misses += 1;
        }
        result.evict(&mut bytes, limits.soft_bytes);
        check_limit(bytes, limits.max_bytes)?;
        result.statistics.bytes = bytes;
        result.statistics.over_budget_bytes = bytes.saturating_sub(limits.soft_bytes);
        check_cancelled(cancel)?;
        Ok(result)
    }
    fn evict(&mut self, bytes: &mut usize, target: usize) {
        while *bytes > target {
            let oldest = self
                .entries
                .iter()
                .filter(|(id, _)| !self.active.contains(id))
                .min_by_key(|(_, e)| e.last_used)
                .map(|(id, _)| *id);
            let Some(id) = oldest else {
                break;
            };
            if let Some(entry) = self.entries.remove(&id) {
                *bytes -= entry_bytes(&entry.source);
                self.statistics.evictions += 1;
            }
        }
    }
}
fn entry_bytes(source: &PreparedCopper) -> usize {
    source.vertices.capacity() * size_of::<CopperVertex>()
        + source.indices.capacity() * size_of::<u32>()
        + source.batches.capacity() * size_of::<CopperBatch>()
        + source
            .batches
            .iter()
            .filter_map(|b| b.parity_rings.as_ref())
            .map(|r| r.capacity() * size_of::<ParityRing>())
            .sum::<usize>()
}
fn prepare_zone(
    zone: &Zone,
    source_index: usize,
    view: Bounds,
    tolerance: f64,
    limits: CurveLimits,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<PreparedCopper, PrepareError> {
    let mut source = PreparedCopper {
        vertices: Vec::new(),
        indices: Vec::new(),
        batches: Vec::new(),
    };
    let mut ranges = Vec::new();
    let metadata = size_of::<CopperBatch>()
        .saturating_add(zone.paths.len().saturating_mul(size_of::<ParityRing>()));
    check_limit(metadata, max_bytes)?;
    ranges
        .try_reserve_exact(zone.paths.len())
        .map_err(|_| PrepareError::Allocation)?;
    for (ring_index, path) in zone.paths.iter().enumerate() {
        check_cancelled(cancel)?;
        if ring_index > 0
            && zone
                .mesh
                .ring_bounds
                .get(ring_index)
                .is_some_and(|b| !overlaps(*b, view))
        {
            continue;
        }
        let live_bytes = metadata
            + source.vertices.capacity() * size_of::<CopperVertex>()
            + source.indices.capacity() * size_of::<u32>();
        let contour_limits = CurveLimits {
            max_allocation_bytes: limits
                .max_allocation_bytes
                .min(max_bytes.saturating_sub(live_bytes)),
            ..limits
        };
        let points = tessellate_curve_ring(path, view, tolerance, contour_limits, cancel)
            .map_err(|e| map_mesh_error(e, zone.id))?;
        if points.len() < 3 {
            continue;
        }
        let vertices = source.vertices.len().saturating_add(points.len());
        let indices = source
            .indices
            .len()
            .saturating_add(points.len().saturating_sub(2).saturating_mul(3));
        check_limit(vertices, u32::MAX as usize)?;
        check_limit(indices, u32::MAX as usize)?;
        check_limit(
            metadata
                .saturating_add(vertices.saturating_mul(size_of::<CopperVertex>()))
                .saturating_add(indices.saturating_mul(size_of::<u32>()))
                .saturating_add(points.capacity().saturating_mul(size_of::<Point>())),
            max_bytes,
        )?;
        source
            .vertices
            .try_reserve_exact(points.len())
            .map_err(|_| PrepareError::Allocation)?;
        source
            .indices
            .try_reserve_exact((points.len() - 2) * 3)
            .map_err(|_| PrepareError::Allocation)?;
        let base = source.vertices.len() as u32;
        let start = source.indices.len() as u32;
        for point in &points {
            check_cancelled(cancel)?;
            let [x, dx] = split_position(point.x);
            let [y, dy] = split_position(point.y);
            let position = [x, y, dx, dy];
            if !position.into_iter().all(f32::is_finite) {
                return Err(PrepareError::Invalid(zone.id));
            }
            source.vertices.push(CopperVertex { position });
        }
        for j in 1..points.len() - 1 {
            check_cancelled(cancel)?;
            source
                .indices
                .extend_from_slice(&[base, base + j as u32, base + j as u32 + 1]);
        }
        ranges.push(ParityRing {
            indices: start..source.indices.len() as u32,
            outer: ring_index == 0,
        });
    }
    source
        .batches
        .try_reserve_exact(1)
        .map_err(|_| PrepareError::Allocation)?;
    source.batches.push(CopperBatch::parity(
        zone,
        source_index,
        view,
        source.vertices.len() as u32,
        source.indices.len() as u32,
        ranges,
    )?);
    Ok(source)
}
fn map_mesh_error(error: MeshError, object: ObjectId) -> PrepareError {
    match error {
        MeshError::Cancelled => PrepareError::Cancelled,
        MeshError::Limit { actual, limit, .. } => PrepareError::Limit { actual, limit },
        MeshError::Invalid { .. } => PrepareError::Invalid(object),
        MeshError::Allocation => PrepareError::Allocation,
    }
}
fn check_cancelled(cancel: &CancellationToken) -> Result<(), PrepareError> {
    if cancel.is_cancelled() {
        Err(PrepareError::Cancelled)
    } else {
        Ok(())
    }
}
fn check_limit(actual: usize, limit: usize) -> Result<(), PrepareError> {
    if actual > limit {
        Err(PrepareError::Limit { actual, limit })
    } else {
        Ok(())
    }
}
fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}
fn contains(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.min.x && a.max.x >= b.max.x && a.min.y <= b.min.y && a.max.y >= b.max.y
}

#[cfg(test)]
#[path = "curves/tests.rs"]
pub(crate) mod tests;
