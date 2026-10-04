//! GPU preparation preserves independent exterior and hole coverage.
//! Consumers must subtract the union of holes, never draw all triangles as a fill.

use crate::{split_position, tracks::PrepareError};
use pomelo_core::{
    model::{Bounds, LayerId, NetId, ObjectId, Zone, ZoneKind},
    task::CancellationToken,
};
use std::ops::Range;
mod source;
use source::CopperSource;

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct CopperVertex {
    /// [x_high, y_high, x_low, y_low], matching one shader float4.
    pub position: [f32; 4],
}
const _: () = assert!(size_of::<CopperVertex>() == 16);

#[derive(Debug)]
pub struct CopperBatch {
    pub object: ObjectId,
    pub selected_object: pomelo_core::selection::SelectedObject,
    pub layer: LayerId,
    pub net: NetId,
    /// Source shape behavior; custom pad meshes remain independent of this tag.
    pub kind: ZoneKind,
    pub source_index: u32,
    pub vertex_start: u32,
    pub vertex_count: u32,
    index_start: u32,
    outer_count: u32,
    hole_count: u32,
    pub bounds: Option<Bounds>,
    /// Analytic edges will require a separate curved boundary coverage pass.
    pub curved: bool,
    /// Clipped self-touching contours use independent parity fans.
    /// Static earcut meshes leave this absent.
    pub parity_rings: Option<Vec<ParityRing>>,
}

#[derive(Debug)]
pub struct ParityRing {
    pub indices: Range<u32>,
    pub outer: bool,
}

impl CopperBatch {
    pub(super) fn parity(
        zone: &Zone,
        source_index: usize,
        bounds: Bounds,
        vertex_count: u32,
        index_count: u32,
        ranges: Vec<ParityRing>,
    ) -> Result<Self, PrepareError> {
        Ok(Self {
            object: zone.id,
            selected_object: pomelo_core::selection::SelectedObject::Zone(zone.id),
            layer: zone.layer,
            net: zone.net,
            kind: zone.kind,
            source_index: u32::try_from(source_index)
                .map_err(|_| PrepareError::Invalid(zone.id))?,
            vertex_start: 0,
            vertex_count,
            index_start: 0,
            outer_count: 0,
            hole_count: index_count,
            bounds: Some(bounds),
            curved: true,
            parity_rings: Some(ranges),
        })
    }
    pub fn outer_indices(&self) -> Range<u32> {
        self.index_start..self.index_start + self.outer_count
    }
    /// Each independently triangulated hole contributes to a coverage union.
    pub fn hole_indices(&self) -> Range<u32> {
        let start = self.index_start + self.outer_count;
        start..start + self.hole_count
    }
}

#[derive(Debug)]
pub struct PreparedCopper {
    pub(crate) source: Option<CopperSource>,
    /// Materialized GPU-format vertices. Source-backed preparations leave this
    /// empty; use `vertex_count` for the logical geometry size.
    pub vertices: Vec<CopperVertex>,
    /// Materialized global vertex indices; empty for source-backed preparations.
    /// Use `index_count` and each batch's explicitly named coverage ranges.
    pub indices: Vec<u32>,
    pub batches: Vec<CopperBatch>,
    // Keep last: buffers must drop before the shared budget returns their bytes.
    pub(crate) memory_reservation: Option<pomelo_core::memory::MemoryReservation>,
}

#[derive(Debug, Clone, Copy)]
pub struct CopperLimits {
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_zones: usize,
    /// Prepared output only; excludes borrowed CPU scene and future GPU resources.
    pub max_bytes: usize,
}
impl Default for CopperLimits {
    fn default() -> Self {
        Self {
            // The verified RDIMM source has 19.7M vertices / 57.8M indices.
            // Keep explicit count and byte ceilings before any output allocation.
            max_vertices: 32_000_000,
            max_indices: 96_000_000,
            max_zones: 250_000,
            max_bytes: 1024 * 1024 * 1024,
        }
    }
}

impl PreparedCopper {
    /// Owned heap capacities, including batch metadata and allocation slack.
    pub fn allocation_bytes(&self) -> usize {
        self.vertices
            .capacity()
            .saturating_mul(size_of::<CopperVertex>())
            .saturating_add(self.indices.capacity().saturating_mul(size_of::<u32>()))
            .saturating_add(
                self.batches
                    .capacity()
                    .saturating_mul(size_of::<CopperBatch>()),
            )
            .saturating_add(self.batches.iter().fold(0usize, |bytes, batch| {
                bytes.saturating_add(batch.parity_rings.as_ref().map_or(0, |rings| {
                    rings.capacity().saturating_mul(size_of::<ParityRing>())
                }))
            }))
            .saturating_add(self.source_allocation_bytes())
    }
    pub fn upload_bytes(&self) -> usize {
        self.vertex_count() * size_of::<CopperVertex>() + self.index_count() * size_of::<u32>()
    }
    pub fn build(
        zones: &[Zone],
        limits: CopperLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        Self::build_impl(zones, limits, cancellation, true)
    }
    fn build_impl(
        zones: &[Zone],
        limits: CopperLimits,
        cancellation: &CancellationToken,
        materialize: bool,
    ) -> Result<Self, PrepareError> {
        check_cancelled(cancellation)?;
        check_count(zones.len(), limits.max_zones.min(u32::MAX as usize))?;
        let (mut vertices, mut indices) = (0_usize, 0_usize);
        for zone in zones {
            check_cancelled(cancellation)?;
            vertices = vertices.saturating_add(zone.mesh.vertices.len());
            indices = indices.saturating_add(zone.mesh.indices.len());
        }
        check_count(vertices, limits.max_vertices.min(u32::MAX as usize))?;
        check_count(indices, limits.max_indices.min(u32::MAX as usize))?;
        let bytes = zones
            .len()
            .saturating_mul(size_of::<CopperBatch>())
            .saturating_add(if materialize {
                vertices
                    .saturating_mul(size_of::<CopperVertex>())
                    .saturating_add(indices.saturating_mul(size_of::<u32>()))
            } else {
                source::metadata_bytes(zones.len())
            });
        check_limit(bytes, limits.max_bytes)?;
        // Every limit is checked before allocating an output array.
        let mut result = Self {
            source: None,
            memory_reservation: None,
            vertices: reserve(if materialize { vertices } else { 0 })?,
            indices: reserve(if materialize { indices } else { 0 })?,
            batches: reserve(zones.len())?,
        };
        let (mut next_vertex, mut next_index) = (0u32, 0u32);
        for (source_index, zone) in zones.iter().enumerate() {
            check_cancelled(cancellation)?;
            let mesh = &zone.mesh;
            let invalid = || PrepareError::Invalid(zone.id);
            let outer = mesh.outer_count as usize;
            if outer > mesh.indices.len()
                || !outer.is_multiple_of(3)
                || !mesh.indices.len().is_multiple_of(3)
            {
                return Err(invalid());
            }
            let bounds = mesh.ring_bounds.first().copied();
            if let Some(bounds) = bounds {
                if !bounds.is_valid() {
                    return Err(invalid());
                }
            } else if !mesh.vertices.is_empty() || !mesh.indices.is_empty() {
                return Err(invalid());
            }
            let exterior_vertices = if mesh.vertices.is_empty() {
                0
            } else {
                let Some(&end) = mesh.ring_offsets.get(1) else {
                    return Err(invalid());
                };
                if mesh.ring_offsets.first() != Some(&0) || end as usize > mesh.vertices.len() {
                    return Err(invalid());
                }
                end as usize
            };
            let vertex_start = next_vertex;
            let index_start = next_index;
            for (index, point) in mesh.vertices.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    check_cancelled(cancellation)?;
                }
                if !point.x.is_finite() || !point.y.is_finite() {
                    return Err(invalid());
                }
                let [x, dx] = split_position(point.x);
                let [y, dy] = split_position(point.y);
                let position = [x, y, dx, dy];
                if !position.into_iter().all(f32::is_finite) {
                    return Err(invalid());
                }
                if materialize {
                    result.vertices.push(CopperVertex { position });
                }
            }
            for (position, &index) in mesh.indices.iter().enumerate() {
                if position.is_multiple_of(1024) {
                    check_cancelled(cancellation)?;
                }
                let index_usize = index as usize;
                if index_usize >= mesh.vertices.len()
                    || (position < outer && index_usize >= exterior_vertices)
                    || (position >= outer && index_usize < exterior_vertices)
                {
                    return Err(invalid());
                }
                let index = vertex_start.checked_add(index).ok_or_else(invalid)?;
                if materialize {
                    result.indices.push(index);
                }
            }
            result.batches.push(CopperBatch {
                object: zone.id,
                selected_object: pomelo_core::selection::SelectedObject::Zone(zone.id),
                layer: zone.layer,
                net: zone.net,
                kind: zone.kind,
                source_index: source_index as u32,
                vertex_start,
                vertex_count: mesh.vertices.len() as u32,
                index_start,
                outer_count: mesh.outer_count,
                hole_count: (mesh.indices.len() - outer) as u32,
                bounds,
                curved: mesh.curved,
                parity_rings: None,
            });
            next_vertex += mesh.vertices.len() as u32;
            next_index += mesh.indices.len() as u32;
        }
        // Preserve source order within a layer without copying vertex/index buffers.
        result
            .batches
            .sort_unstable_by_key(|batch| (batch.layer, batch.source_index));
        check_cancelled(cancellation)?;
        Ok(result)
    }
}

fn check_cancelled(token: &CancellationToken) -> Result<(), PrepareError> {
    if token.is_cancelled() {
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
fn check_count(actual: usize, limit: usize) -> Result<(), PrepareError> {
    if actual > limit {
        Err(PrepareError::CountLimit { actual, limit })
    } else {
        Ok(())
    }
}
fn reserve<T>(count: usize) -> Result<Vec<T>, PrepareError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| PrepareError::Allocation)?;
    Ok(values)
}

#[cfg(test)]
#[path = "../../tests/unit/scene/copper.rs"]
mod tests;
