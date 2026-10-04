//! Immutable, balanced bounds hierarchy for precise source-space track queries.

use crate::{
    display::BoardDisplay,
    geometry::PathError,
    model::{BoardScene, Bounds},
    picking::{PickFilter, PickQuery, SegmentHit, segment_distance_mm},
    selection::SelectedObject,
    task::CancellationToken,
};
use std::sync::Arc;
mod drawings;
mod locate;
mod navigation;
mod visible;
pub use drawings::TextPickQuad;
pub use navigation::SelectionAnchor;
pub use visible::CanvasHit;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("PICK_INDEX_GEOMETRY {0}")]
    Geometry(#[from] PathError),
    #[error("PICK_INDEX_BYTE_LIMIT actual={actual} limit={limit}")]
    ByteLimit { actual: usize, limit: usize },
    #[error("PICK_INDEX_ALLOCATION")]
    Allocation,
}

pub struct SegmentIndex {
    scene: Arc<BoardScene>,
    order: Vec<usize>,
    bounds: Vec<Option<Bounds>>,
    leaf_start: usize,
    zones: crate::zone_index::ZoneFillIndex,
    drawings: drawings::DrawingIndex,
    pins: super::spatial::BoundsIndex,
    vias: super::spatial::BoundsIndex,
    zone_bounds: super::spatial::BoundsIndex,
    zone_paths: Vec<Vec<Option<Bounds>>>,
    drawing_budget: usize,
}

impl SegmentIndex {
    /// Merge copper candidates by distance, then pin/via/segment/zone priority.
    /// Within a category equal distances preserve source order. Network queries
    /// discard net zero before each category's limit. Pads still scan; zone
    /// fill checks use validated exterior and hole bounds.
    pub fn query_objects(
        &self,
        query: PickQuery,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
        connected_only: bool,
    ) -> Result<Vec<crate::picking::ObjectHit>, PathError> {
        use crate::{model::NetId, picking::ObjectHit};
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        let limit = limit.min(64);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut hits: Vec<ObjectHit> = Vec::new();
        let mut insert = |hit: ObjectHit| {
            let position = hits.partition_point(|other| other.distance_mm <= hit.distance_mm);
            if position < limit {
                if hits.len() == limit {
                    hits.pop();
                }
                hits.insert(position, hit);
            }
        };
        for hit in query.pins_where(&self.scene, filter, display, limit, cancel, |pin| {
            !connected_only || pin.net != NetId(0)
        })? {
            insert(ObjectHit {
                object: SelectedObject::Pin(hit.pin.id),
                distance_mm: hit.distance_mm,
            });
        }
        for hit in query.vias_where(&self.scene, filter, display, limit, cancel, |via| {
            !connected_only || via.net != NetId(0)
        })? {
            insert(ObjectHit {
                object: SelectedObject::Via(hit.via.id),
                distance_mm: hit.distance_mm,
            });
        }
        for hit in self.query_where(query, filter, display, limit, cancel, |segment| {
            !connected_only || segment.net != NetId(0)
        })? {
            insert(ObjectHit {
                object: SelectedObject::Segment(hit.segment.id),
                distance_mm: hit.distance_mm,
            });
        }
        for hit in self
            .zones
            .query(&self.scene, query, filter, display, cancel, connected_only)?
            .into_iter()
            .take(limit)
        {
            insert(ObjectHit {
                object: SelectedObject::Zone(hit.zone.id),
                distance_mm: hit.distance_mm,
            });
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }

    pub fn segment_count(&self) -> usize {
        self.order.len()
    }

    pub fn build(
        scene: Arc<BoardScene>,
        max_segments: usize,
        cancel: &CancellationToken,
    ) -> Result<Self, IndexError> {
        Self::build_with_budget(scene, max_segments, 512 * 1024 * 1024, cancel)
    }

    /// Budget covers all simultaneously live index and sorting vectors, excluding source scene.
    pub fn build_with_budget(
        scene: Arc<BoardScene>,
        max_segments: usize,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Self, IndexError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled.into());
        }
        let count = scene.segments.len();
        if count > max_segments {
            return Err(PathError::PointLimit {
                actual: count,
                limit: max_segments,
            }
            .into());
        }
        let leaf_start = count
            .max(1)
            .checked_next_power_of_two()
            .ok_or(PathError::PointLimit {
                actual: count,
                limit: max_segments,
            })?;
        let node_count = leaf_start.checked_mul(2).ok_or(PathError::PointLimit {
            actual: count,
            limit: max_segments,
        })?;
        let peak_bytes = node_count
            .checked_mul(std::mem::size_of::<Option<Bounds>>())
            .and_then(|bytes| {
                count
                    .checked_mul(
                        std::mem::size_of::<(usize, Bounds)>() + std::mem::size_of::<usize>(),
                    )
                    .and_then(|temporary| bytes.checked_add(temporary))
            })
            .ok_or(IndexError::ByteLimit {
                actual: usize::MAX,
                limit: max_bytes,
            })?;
        if peak_bytes > max_bytes {
            return Err(IndexError::ByteLimit {
                actual: peak_bytes,
                limit: max_bytes,
            });
        }
        let mut leaves = Vec::new();
        leaves
            .try_reserve_exact(count)
            .map_err(|_| IndexError::Allocation)?;
        for (index, segment) in scene.segments.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            // Validate analytic inputs before publishing the hierarchy.
            segment_distance_mm(segment, segment.a)?;
            let bounds = segment.bounds().ok_or(PathError::Invalid(segment.id))?;
            leaves.push((index, bounds));
        }
        leaves.sort_unstable_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x).then(a.0.cmp(&b.0)));
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled.into());
        }
        let mut bounds = Vec::new();
        bounds
            .try_reserve_exact(node_count)
            .map_err(|_| IndexError::Allocation)?;
        bounds.resize(node_count, None);
        let mut order = Vec::new();
        order
            .try_reserve_exact(count)
            .map_err(|_| IndexError::Allocation)?;
        for (position, (index, extent)) in leaves.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            bounds[leaf_start + position] = Some(extent);
            order.push(index);
        }
        for index in (1..leaf_start).rev() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            bounds[index] = match (bounds[index * 2], bounds[index * 2 + 1]) {
                (Some(mut a), Some(b)) => {
                    a.include(b.min);
                    a.include(b.max);
                    Some(a)
                }
                (a, b) => a.or(b),
            };
        }
        let contour_bytes = scene.zones.iter().fold(0usize, |bytes, zone| {
            bytes
                .saturating_add(size_of::<Vec<Option<Bounds>>>())
                .saturating_add(zone.paths.len().saturating_mul(size_of::<Option<Bounds>>()))
        });
        let peak_bytes = peak_bytes.saturating_add(contour_bytes);
        if peak_bytes > max_bytes {
            return Err(IndexError::ByteLimit {
                actual: peak_bytes,
                limit: max_bytes,
            });
        }
        let zones =
            crate::zone_index::ZoneFillIndex::build(&scene, max_bytes - peak_bytes, cancel)?;
        // Bound each hierarchy's actual entry/node layout. BoundsIndex reserves
        // these buffers once instead of retaining geometric growth slack.
        let owner_bytes = [scene.pins.len(), scene.vias.len(), scene.zones.len()]
            .into_iter()
            .fold(0usize, |bytes, count| {
                bytes.saturating_add(super::spatial::BoundsIndex::max_allocation_bytes(count))
            });
        let used = peak_bytes
            .saturating_add(zones.bytes)
            .saturating_add(owner_bytes);
        if used > max_bytes {
            return Err(IndexError::ByteLimit {
                actual: used,
                limit: max_bytes,
            });
        }
        let pin_bounds =
            |at, angle, mirrored, pads: &[crate::model::Pad], drill: crate::model::DrillShape| {
                let owner = crate::pad::PadPlacement {
                    at,
                    angle,
                    mirrored,
                };
                let mut result: Option<Bounds> = None;
                for bounds in pads
                    .iter()
                    .filter_map(|pad| pad.bounds(owner))
                    .chain(drill.pad().and_then(|pad| pad.bounds(owner)))
                {
                    if let Some(old) = &mut result {
                        old.include(bounds.min);
                        old.include(bounds.max);
                    } else {
                        result = Some(bounds);
                    }
                }
                result
            };
        let pins = super::spatial::BoundsIndex::build(
            scene
                .pins
                .iter()
                .map(|p| pin_bounds(p.at, p.angle, p.mirrored, &p.pads, p.drill_shape)),
            cancel,
        )?;
        let vias = super::spatial::BoundsIndex::build(
            scene
                .vias
                .iter()
                .map(|p| pin_bounds(p.at, p.angle, p.mirrored, &p.pads, p.drill_shape)),
            cancel,
        )?;
        let mut zone_paths = Vec::new();
        zone_paths
            .try_reserve_exact(scene.zones.len())
            .map_err(|_| IndexError::Allocation)?;
        for zone in &scene.zones {
            let mut paths = Vec::new();
            paths
                .try_reserve_exact(zone.paths.len())
                .map_err(|_| IndexError::Allocation)?;
            for path in &zone.paths {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled.into());
                }
                paths.push(crate::geometry::path_bounds(path));
            }
            zone_paths.push(paths);
        }
        let zone_bounds = super::spatial::BoundsIndex::build(
            scene.zones.iter().zip(&zone_paths).map(|(z, paths)| {
                paths
                    .first()
                    .copied()
                    .flatten()
                    .or_else(|| z.mesh.ring_bounds.first().copied())
            }),
            cancel,
        )?;
        let drawing_budget = max_bytes - used;
        let drawings = drawings::DrawingIndex::build(&scene, &[], drawing_budget, cancel)?;
        Ok(Self {
            drawings,
            pins,
            vias,
            zone_bounds,
            zone_paths,
            drawing_budget,
            scene,
            order,
            bounds,
            leaf_start,
            zones,
        })
    }

    pub fn query(
        &self,
        query: PickQuery,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<SegmentHit<'_>>, PathError> {
        self.query_where(query, filter, display, limit, cancel, |_| true)
    }

    /// Apply mode eligibility before the candidate cap, so ineligible geometry cannot hide a target.
    pub fn query_where(
        &self,
        query: PickQuery,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
        eligible: impl Fn(&crate::model::Segment) -> bool,
    ) -> Result<Vec<SegmentHit<'_>>, PathError> {
        let limit = limit.min(64);
        let mut hits: Vec<(usize, SegmentHit<'_>)> = Vec::new();
        let mut pending = vec![1usize];
        while let Some(index) = pending.pop() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if limit == 0 {
                break;
            }
            let Some(bounds) = self.bounds[index] else {
                continue;
            };
            if !query.intersects(bounds) {
                continue;
            }
            if index < self.leaf_start {
                pending.push(index * 2 + 1);
                pending.push(index * 2);
                continue;
            }
            let source_index = self.order[index - self.leaf_start];
            let segment = &self.scene.segments[source_index];
            if !eligible(segment)
                || !filter.allows(
                    SelectedObject::Segment(segment.id),
                    &[segment.layer],
                    display,
                )
            {
                continue;
            }
            let distance_mm = segment_distance_mm(segment, query.point)?;
            if distance_mm > query.tolerance_mm {
                continue;
            }
            let position = hits.partition_point(|(source, hit)| {
                hit.distance_mm < distance_mm
                    || (hit.distance_mm == distance_mm && *source < source_index)
            });
            if position >= limit {
                continue;
            }
            if hits.len() == limit {
                hits.pop();
            }
            hits.insert(
                position,
                (
                    source_index,
                    SegmentHit {
                        segment,
                        distance_mm,
                    },
                ),
            );
        }
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        Ok(hits.into_iter().map(|(_, hit)| hit).collect())
    }
}
