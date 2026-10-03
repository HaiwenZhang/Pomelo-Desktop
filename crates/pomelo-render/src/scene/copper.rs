//! GPU preparation preserves independent exterior and hole coverage.
//! Consumers must subtract the union of holes, never draw all triangles as a fill.

use crate::{split_position, tracks::PrepareError};
use pomelo_core::{
    model::{Bounds, LayerId, NetId, ObjectId, Zone},
    task::CancellationToken,
};
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
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
    pub vertices: Vec<CopperVertex>,
    /// Global vertex indices. Access via each batch's explicitly named coverage ranges.
    pub indices: Vec<u32>,
    pub batches: Vec<CopperBatch>,
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
            max_vertices: 16_000_000,
            max_indices: 48_000_000,
            max_zones: 250_000,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

impl PreparedCopper {
    pub fn upload_bytes(&self) -> usize {
        self.vertices.len() * size_of::<CopperVertex>() + self.indices.len() * size_of::<u32>()
    }
    pub fn build(
        zones: &[Zone],
        limits: CopperLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        check_cancelled(cancellation)?;
        check_limit(zones.len(), limits.max_zones.min(u32::MAX as usize))?;
        let (mut vertices, mut indices) = (0_usize, 0_usize);
        for zone in zones {
            check_cancelled(cancellation)?;
            vertices = vertices.saturating_add(zone.mesh.vertices.len());
            indices = indices.saturating_add(zone.mesh.indices.len());
        }
        check_limit(vertices, limits.max_vertices.min(u32::MAX as usize))?;
        check_limit(indices, limits.max_indices.min(u32::MAX as usize))?;
        let bytes = vertices
            .saturating_mul(size_of::<CopperVertex>())
            .saturating_add(indices.saturating_mul(size_of::<u32>()))
            .saturating_add(zones.len().saturating_mul(size_of::<CopperBatch>()));
        check_limit(bytes, limits.max_bytes)?;
        // Every limit is checked before allocating an output array.
        let mut result = Self {
            vertices: reserve(vertices)?,
            indices: reserve(indices)?,
            batches: reserve(zones.len())?,
        };
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
            let vertex_start = result.vertices.len() as u32;
            let index_start = result.indices.len() as u32;
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
                result.vertices.push(CopperVertex { position });
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
                result
                    .indices
                    .push(vertex_start.checked_add(index).ok_or_else(invalid)?);
            }
            result.batches.push(CopperBatch {
                object: zone.id,
                selected_object: pomelo_core::selection::SelectedObject::Zone(zone.id),
                layer: zone.layer,
                net: zone.net,
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
fn reserve<T>(count: usize) -> Result<Vec<T>, PrepareError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| PrepareError::Allocation)?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::Point,
    };

    fn square(x: f64, y: f64, side: f64) -> Vec<Point> {
        vec![
            Point::new(x, y),
            Point::new(x + side, y),
            Point::new(x + side, y + side),
            Point::new(x, y + side),
        ]
    }
    fn zone(id: u32, layer: u32) -> Zone {
        Zone {
            id: ObjectId(id),
            layer: LayerId(layer),
            net: NetId(7),
            paths: vec![],
            mesh: CopperMesh::build(
                &[
                    square(100_000.000_123, 0.0, 20.0),
                    square(100_004.000_123, 4.0, 6.0),
                    square(100_007.000_123, 7.0, 6.0),
                ],
                &[],
                &MeshLimits::default(),
                &CancellationToken::default(),
            )
            .unwrap(),
        }
    }

    #[test]
    fn overlapping_holes_remain_separate_coverage_and_indices_rebase_per_zone() {
        let zones = [zone(9, 2), zone(3, 1), zone(8, 2)];
        let prepared = PreparedCopper::build(
            &zones,
            CopperLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(
            prepared
                .batches
                .iter()
                .map(|b| b.object.0)
                .collect::<Vec<_>>(),
            [3, 9, 8]
        );
        for batch in &prepared.batches {
            let source = &zones[batch.source_index as usize];
            assert_eq!(batch.outer_indices().len(), 6);
            assert_eq!(batch.hole_indices().len(), 12);
            let start = batch.outer_indices().start as usize;
            let end = batch.hole_indices().end as usize;
            assert_eq!(
                &prepared.indices[start..end],
                source
                    .mesh
                    .indices
                    .iter()
                    .map(|index| index + batch.vertex_start)
                    .collect::<Vec<_>>()
            );
            let position = prepared.vertices[batch.vertex_start as usize].position;
            assert!(
                (f64::from(position[0]) + f64::from(position[2]) - source.mesh.vertices[0].x).abs()
                    < 1e-10
            );
        }
    }

    #[test]
    fn invalid_triangle_roles_and_indices_are_rejected() {
        for mutation in 0..3 {
            let mut source = zone(42, 1);
            match mutation {
                0 => source.mesh.indices[0] = 4,
                1 => source.mesh.indices[6] = 0,
                _ => source.mesh.indices[6] = u32::MAX,
            }
            assert!(matches!(
                PreparedCopper::build(
                    &[source],
                    CopperLimits::default(),
                    &CancellationToken::default()
                ),
                Err(PrepareError::Invalid(ObjectId(42)))
            ));
        }
    }

    #[test]
    fn limits_and_cancellation_precede_invalid_mesh_processing() {
        let mut source = zone(42, 1);
        source.mesh.vertices[0].x = f64::NAN;
        assert!(matches!(
            PreparedCopper::build(
                std::slice::from_ref(&source),
                CopperLimits {
                    max_bytes: 0,
                    ..CopperLimits::default()
                },
                &CancellationToken::default()
            ),
            Err(PrepareError::Limit { .. })
        ));
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            PreparedCopper::build(&[source], CopperLimits::default(), &cancellation),
            Err(PrepareError::Cancelled)
        ));
    }

    #[test]
    fn malformed_outer_count_and_non_finite_coordinates_are_rejected() {
        let mut source = zone(42, 1);
        source.mesh.outer_count = 5;
        assert!(
            PreparedCopper::build(
                std::slice::from_ref(&source),
                CopperLimits::default(),
                &CancellationToken::default()
            )
            .is_err()
        );
        source.mesh.outer_count = 6;
        source.mesh.vertices[0].x = f64::INFINITY;
        assert!(
            PreparedCopper::build(
                &[source],
                CopperLimits::default(),
                &CancellationToken::default()
            )
            .is_err()
        );
    }
}
