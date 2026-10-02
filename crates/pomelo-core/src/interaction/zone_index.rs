//! Validated immutable fill bounds; source rings remain in the owning scene.

use crate::{
    copper::{MeshError, contours_cover},
    display::BoardDisplay,
    geometry::PathError,
    model::{BoardScene, Bounds, NetId, Point},
    picking::{PickCategory, PickFilter, PickQuery, ZoneHit},
    picking_index::IndexError,
    task::CancellationToken,
};

pub(crate) struct ZoneFillIndex {
    rings: Vec<Vec<Bounds>>,
}

impl ZoneFillIndex {
    pub(crate) fn build(
        scene: &BoardScene,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Self, IndexError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled.into());
        }
        let mut bytes = scene.zones.len().checked_mul(size_of::<Vec<Bounds>>());
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            bytes = bytes.and_then(|bytes| {
                zone.mesh
                    .ring_offsets
                    .len()
                    .saturating_sub(1)
                    .checked_mul(size_of::<Bounds>())
                    .and_then(|additional| bytes.checked_add(additional))
            });
        }
        let bytes = bytes.unwrap_or(usize::MAX);
        if bytes > max_bytes {
            return Err(IndexError::ByteLimit {
                actual: bytes,
                limit: max_bytes,
            });
        }
        let mut rings = Vec::new();
        rings
            .try_reserve_exact(scene.zones.len())
            .map_err(|_| IndexError::Allocation)?;
        for zone in &scene.zones {
            // Validate every ring before any query can skip it by bounds.
            zone.mesh
                .covers_fill(Point::default(), cancel)
                .map_err(|error| match error {
                    MeshError::Cancelled => PathError::Cancelled,
                    _ => PathError::Invalid(zone.id),
                })?;
            let mut extents = Vec::new();
            extents
                .try_reserve_exact(zone.mesh.ring_offsets.len().saturating_sub(1))
                .map_err(|_| IndexError::Allocation)?;
            for range in zone.mesh.ring_offsets.windows(2) {
                let vertices = &zone.mesh.vertices[range[0] as usize..range[1] as usize];
                let mut bounds = Bounds {
                    min: vertices[0],
                    max: vertices[0],
                };
                for &point in vertices {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled.into());
                    }
                    bounds.include(point);
                }
                extents.push(bounds);
            }
            rings.push(extents);
        }
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled.into());
        }
        Ok(Self { rings })
    }

    pub(crate) fn query<'a>(
        &self,
        scene: &'a BoardScene,
        query: PickQuery,
        filter: PickFilter,
        display: &BoardDisplay,
        cancel: &CancellationToken,
        connected_only: bool,
    ) -> Result<Vec<ZoneHit<'a>>, PathError> {
        let mut hits = Vec::new();
        for (zone, bounds) in scene.zones.iter().zip(&self.rings) {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !filter.contains(PickCategory::Zone)
                || !display.layer_visible(zone.layer)
                || (connected_only && zone.net == NetId(0))
            {
                continue;
            }
            let contains = |bounds: &Bounds| {
                query.point.x >= bounds.min.x
                    && query.point.x <= bounds.max.x
                    && query.point.y >= bounds.min.y
                    && query.point.y <= bounds.max.y
            };
            if bounds.first().is_none_or(|extent| !contains(extent)) {
                continue;
            }
            let ranges = zone
                .mesh
                .ring_offsets
                .windows(2)
                .enumerate()
                .filter(|(index, _)| *index == 0 || contains(&bounds[*index]))
                .map(|(_, range)| &zone.mesh.vertices[range[0] as usize..range[1] as usize]);
            let covered =
                contours_cover(ranges, query.point, cancel).map_err(|error| match error {
                    MeshError::Cancelled => PathError::Cancelled,
                    _ => PathError::Invalid(zone.id),
                })?;
            if covered && hits.len() < 64 {
                hits.push(ZoneHit {
                    zone,
                    distance_mm: 0.0,
                });
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }
}
