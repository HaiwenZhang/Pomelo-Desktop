//! Independent contour meshes: shade outer coverage minus the union of all holes.
//! Source coordinates and ring offsets are never reordered or simplified.

use crate::{
    geometry::path_bounds,
    model::{Bounds, Point, Segment},
    task::CancellationToken,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct MeshLimits {
    /// Output and conservative triangulator scratch accounting; excludes borrowed input.
    pub max_allocation_bytes: usize,
    pub max_points: usize,
    pub max_rings: usize,
}
impl Default for MeshLimits {
    fn default() -> Self {
        Self {
            max_allocation_bytes: 1024 * 1024 * 1024,
            max_points: 8_000_000,
            max_rings: 65_536,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("COPPER_MESH_CANCELLED")]
    Cancelled,
    #[error("COPPER_MESH_LIMIT resource={resource} actual={actual} limit={limit}")]
    Limit {
        resource: &'static str,
        actual: usize,
        limit: usize,
    },
    #[error("COPPER_MESH_INVALID ring={ring} field={field}")]
    Invalid { ring: usize, field: &'static str },
    #[error("COPPER_MESH_ALLOCATION")]
    Allocation,
}

/// A bounded range of nearby holes, indexed in Morton-sorted `ring_order`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopperChunk {
    pub start: u32,
    pub count: u32,
    pub ring_start: u32,
    pub ring_count: u32,
    pub bounds: Bounds,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CopperMesh {
    pub vertices: Vec<Point>,
    /// First `outer_count` indices fill the exterior; remaining indices subtract holes.
    pub indices: Vec<u32>,
    pub outer_count: u32,
    /// Vertex ranges in source ring order; the final offset equals `vertices.len()`.
    pub ring_offsets: Vec<u32>,
    pub ring_bounds: Vec<Bounds>,
    pub ring_order: Vec<u32>,
    pub hole_chunks: Vec<CopperChunk>,
    pub curved: bool,
}

impl CopperMesh {
    /// Coverage of the stored fill contours: exterior minus the union of holes.
    /// Curved rings use their fill approximation here; exact path picking is separate.
    /// Hole boundaries are excluded, while the exterior boundary is included.
    pub fn covers_fill(&self, point: Point, cancel: &CancellationToken) -> Result<bool, MeshError> {
        check_cancelled(cancel)?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(MeshError::Invalid {
                ring: 0,
                field: "QUERY_POINT",
            });
        }
        if self.ring_offsets.is_empty() && self.vertices.is_empty() {
            return Ok(false);
        }
        if self.ring_offsets.first() != Some(&0)
            || self
                .ring_offsets
                .last()
                .copied()
                .map(|offset| offset as usize)
                != Some(self.vertices.len())
        {
            return Err(MeshError::Invalid {
                ring: 0,
                field: "RING_OFFSETS",
            });
        }
        for (ring, range) in self.ring_offsets.windows(2).enumerate() {
            check_cancelled(cancel)?;
            if self
                .vertices
                .get(range[0] as usize..range[1] as usize)
                .is_none()
            {
                return Err(MeshError::Invalid {
                    ring,
                    field: "RING_OFFSETS",
                });
            }
        }
        contours_cover(
            self.ring_offsets
                .windows(2)
                .map(|range| &self.vertices[range[0] as usize..range[1] as usize]),
            point,
            cancel,
        )
    }

    /// Build independent meshes with f64 predicates and stable spatial ordering of holes.
    /// Cancellation is checked throughout our loops and before/after each earcut call.
    /// The dependency has no cancellation callback inside a single contour triangulation.
    pub fn build(
        rings: &[Vec<Point>],
        paths: &[Vec<Segment>],
        limits: &MeshLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, MeshError> {
        check_cancelled(cancellation)?;
        check_limit(
            "rings",
            rings.len(),
            limits.max_rings.min(u32::MAX as usize),
        )?;
        if !paths.is_empty() && paths.len() != rings.len() {
            return Err(MeshError::Invalid {
                ring: 0,
                field: "PATH_RING_COUNT",
            });
        }
        let mut points = 0_usize;
        let mut index_capacity = 0_usize;
        let mut largest = 0_usize;
        for ring in rings {
            check_cancelled(cancellation)?;
            points = points.saturating_add(ring.len());
            index_capacity =
                index_capacity.saturating_add(ring.len().saturating_sub(2).saturating_mul(3));
            largest = largest.max(ring.len());
        }
        check_limit("points", points, limits.max_points.min(u32::MAX as usize))?;
        check_limit("indices", index_capacity, u32::MAX as usize)?;
        // Earcut's nodes use u32 byte offsets. Account for growth and split nodes
        // conservatively, as well as the per-ring output and the stable-sort scratch.
        let bytes = points
            .saturating_mul(size_of::<Point>())
            .saturating_add(index_capacity.saturating_mul(size_of::<u32>()))
            .saturating_add(rings.len().saturating_mul(128))
            .saturating_add(largest.saturating_mul(512))
            .saturating_add(4);
        check_limit("bytes", bytes, limits.max_allocation_bytes)?;
        check_limit(
            "earcut_nodes",
            largest.saturating_mul(256),
            u32::MAX as usize,
        )?;
        let mut result = Self {
            vertices: reserve(points)?,
            indices: reserve(index_capacity)?,
            ring_offsets: reserve(rings.len().saturating_add(1))?,
            ring_bounds: reserve(rings.len())?,
            ring_order: reserve(rings.len())?,
            hole_chunks: reserve(rings.len().saturating_sub(1).div_ceil(64))?,
            ..Self::default()
        };
        for (ring_index, ring) in rings.iter().enumerate() {
            result.ring_offsets.push(result.vertices.len() as u32);
            let Some(first) = ring.first() else {
                return Err(MeshError::Invalid {
                    ring: ring_index,
                    field: "EMPTY_RING",
                });
            };
            let mut bounds = Bounds {
                min: *first,
                max: *first,
            };
            for (index, point) in ring.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    check_cancelled(cancellation)?;
                }
                if !point.x.is_finite() || !point.y.is_finite() {
                    return Err(MeshError::Invalid {
                        ring: ring_index,
                        field: "COORDINATES",
                    });
                }
                bounds.include(*point);
                result.vertices.push(*point);
            }
            if let Some(path) = paths.get(ring_index).filter(|path| !path.is_empty()) {
                for edges in path.chunks(1024) {
                    check_cancelled(cancellation)?;
                    let exact = path_bounds(edges).ok_or(MeshError::Invalid {
                        ring: ring_index,
                        field: "ANALYTIC_BOUNDS",
                    })?;
                    bounds.include(exact.min);
                    bounds.include(exact.max);
                    result.curved |= edges.iter().any(|edge| edge.arc.is_some());
                }
            }
            result.ring_bounds.push(bounds);
        }
        result.ring_offsets.push(result.vertices.len() as u32);
        let mut holes = reserve(rings.len().saturating_sub(1))?;
        if let Some(exterior) = result.ring_bounds.first().copied() {
            result.ring_order.push(0);
            for (index, bounds) in result.ring_bounds.iter().enumerate().skip(1) {
                check_cancelled(cancellation)?;
                holes.push((morton(*bounds, exterior), index as u32));
            }
            // Source index is an explicit tie-breaker, matching stable JavaScript sort.
            holes.sort_unstable();
            result
                .ring_order
                .extend(holes.iter().map(|(_, index)| *index));
        }
        check_cancelled(cancellation)?;
        let mut triangulator = earcut::Earcut::<f64>::new();
        let mut local = reserve(largest.saturating_sub(2).saturating_mul(3))?;
        for (position, ring_index) in result.ring_order.iter().copied().enumerate() {
            check_cancelled(cancellation)?;
            let ring_index = ring_index as usize;
            let ring = &rings[ring_index];
            local.clear();
            if ring.len() >= 3 && !convex_indices(ring, &mut local, cancellation)? {
                local.clear();
                triangulator.earcut(ring.iter().map(|point| [point.x, point.y]), &[], &mut local);
            }
            check_cancelled(cancellation)?;
            if local.len() > ring.len().saturating_sub(2).saturating_mul(3)
                || !local.len().is_multiple_of(3)
                || local.iter().any(|index| *index as usize >= ring.len())
            {
                return Err(MeshError::Invalid {
                    ring: ring_index,
                    field: "TRIANGULATION_OUTPUT",
                });
            }
            if position > 0 {
                let bounds = result.ring_bounds[ring_index];
                if (position - 1).is_multiple_of(64) {
                    result.hole_chunks.push(CopperChunk {
                        start: result.indices.len() as u32,
                        count: 0,
                        ring_start: position as u32,
                        ring_count: 0,
                        bounds,
                    });
                }
                if let Some(chunk) = result.hole_chunks.last_mut() {
                    chunk.count += local.len() as u32;
                    chunk.ring_count += 1;
                    chunk.bounds.include(bounds.min);
                    chunk.bounds.include(bounds.max);
                }
            }
            let base = result.ring_offsets[ring_index];
            for (index, vertex) in local.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    check_cancelled(cancellation)?;
                }
                result.indices.push(*vertex + base);
            }
            if position == 0 {
                result.outer_count = result.indices.len() as u32;
            }
        }
        check_cancelled(cancellation)?;
        Ok(result)
    }

    /// Retained mesh allocation for scene-wide accounting after scratch is released.
    pub fn allocation_bytes(&self) -> usize {
        self.vertices.capacity() * size_of::<Point>()
            + self.indices.capacity() * size_of::<u32>()
            + self.ring_offsets.capacity() * size_of::<u32>()
            + self.ring_bounds.capacity() * size_of::<Bounds>()
            + self.ring_order.capacity() * size_of::<u32>()
            + self.hole_chunks.capacity() * size_of::<CopperChunk>()
    }
}

fn check_cancelled(token: &CancellationToken) -> Result<(), MeshError> {
    if token.is_cancelled() {
        Err(MeshError::Cancelled)
    } else {
        Ok(())
    }
}
fn check_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), MeshError> {
    if actual > limit {
        Err(MeshError::Limit {
            resource,
            actual,
            limit,
        })
    } else {
        Ok(())
    }
}
fn reserve<T>(count: usize) -> Result<Vec<T>, MeshError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| MeshError::Allocation)?;
    Ok(result)
}
fn spread_bits(mut value: u32) -> u32 {
    value = (value | (value << 8)) & 0x00ff00ff;
    value = (value | (value << 4)) & 0x0f0f0f0f;
    value = (value | (value << 2)) & 0x33333333;
    (value | (value << 1)) & 0x55555555
}
fn morton(bounds: Bounds, exterior: Bounds) -> u32 {
    let axis = |center: f64, min: f64, max: f64| {
        (((center - min) / (max - min).max(1e-20) * 65535.0).floor()).clamp(0.0, 65535.0) as u32
    };
    let x = axis(
        (bounds.min.x + bounds.max.x) / 2.0,
        exterior.min.x,
        exterior.max.x,
    );
    let y = axis(
        (bounds.min.y + bounds.max.y) / 2.0,
        exterior.min.y,
        exterior.max.y,
    );
    spread_bits(x) | (spread_bits(y) << 1)
}

/// Same conservative fast-path selection and clipping order as the frozen Web baseline.
fn convex_indices(
    ring: &[Point],
    output: &mut Vec<u32>,
    token: &CancellationToken,
) -> Result<bool, MeshError> {
    let count = ring.len();
    let mut winding = 0_i32;
    let mut signed_area = 0.0;
    let mut first_x = 0_i32;
    let mut last_x = 0_i32;
    let mut changes = 0;
    for index in 0..count {
        if index.is_multiple_of(1024) {
            check_cancelled(token)?;
        }
        let a = ring[if index == 0 { count - 1 } else { index - 1 }];
        let b = ring[index];
        let c = ring[if index + 1 == count { 0 } else { index + 1 }];
        let left = (b.x - a.x) * (c.y - b.y);
        let right = (b.y - a.y) * (c.x - b.x);
        let turn = left - right;
        if turn.abs() <= 1e-12 * (left.abs() + right.abs()) || !turn.is_finite() {
            return Ok(false);
        }
        let direction = if turn > 0.0 { 1 } else { -1 };
        if winding != 0 && direction != winding {
            return Ok(false);
        }
        winding = direction;
        signed_area += (a.x - b.x) * (b.y + a.y);
        let dx = c.x - b.x;
        if dx != 0.0 {
            let sign = if dx > 0.0 { 1 } else { -1 };
            if first_x == 0 {
                first_x = sign;
            } else if sign != last_x {
                changes += 1;
            }
            last_x = sign;
        }
    }
    if last_x != first_x {
        changes += 1;
    }
    if changes != 2 || !signed_area.is_finite() || signed_area * f64::from(winding) <= 0.0 {
        return Ok(false);
    }
    let forward = signed_area > 0.0;
    let anchor = if forward { count - 2 } else { 1 };
    let mut current = if forward { count - 1 } else { 0 };
    for index in 0..count - 2 {
        if index.is_multiple_of(1024) {
            check_cancelled(token)?;
        }
        let next = if forward {
            if current + 1 == count { 0 } else { current + 1 }
        } else {
            if current == 0 { count - 1 } else { current - 1 }
        };
        let a = ring[anchor];
        let b = ring[current];
        let c = ring[next];
        let left = (b.x - a.x) * (c.y - b.y);
        let right = (b.y - a.y) * (c.x - b.x);
        if left - right <= 1e-12 * (left.abs() + right.abs()) || !(left - right).is_finite() {
            return Ok(false);
        }
        output.extend_from_slice(&[anchor as u32, current as u32, next as u32]);
        current = next;
    }
    Ok(true)
}

pub(crate) fn contours_cover<'a>(
    rings: impl IntoIterator<Item = &'a [Point]>,
    point: Point,
    cancel: &CancellationToken,
) -> Result<bool, MeshError> {
    check_cancelled(cancel)?;
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(MeshError::Invalid {
            ring: 0,
            field: "QUERY_POINT",
        });
    }
    let mut exterior = false;
    let mut hole = false;
    for (ring, vertices) in rings.into_iter().enumerate() {
        check_cancelled(cancel)?;
        if vertices.len() < 3 {
            return Err(MeshError::Invalid {
                ring,
                field: "RING_POINTS",
            });
        }
        let mut inside = false;
        let mut boundary = false;
        let mut a = vertices[vertices.len() - 1];
        for &b in vertices {
            check_cancelled(cancel)?;
            if ![a.x, a.y, b.x, b.y].into_iter().all(f64::is_finite) {
                return Err(MeshError::Invalid {
                    ring,
                    field: "NONFINITE_POINT",
                });
            }
            let distance = crate::geometry::distance_to_line(point, a, b);
            if !distance.is_finite() {
                return Err(MeshError::Invalid {
                    ring,
                    field: "QUERY_ARITHMETIC",
                });
            }
            boundary |= distance == 0.0;
            if (a.y > point.y) != (b.y > point.y) {
                let intersection = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if !intersection.is_finite() {
                    return Err(MeshError::Invalid {
                        ring,
                        field: "QUERY_ARITHMETIC",
                    });
                }
                if point.x < intersection {
                    inside = !inside;
                }
            }
            a = b;
        }
        if ring == 0 {
            exterior = inside || boundary;
        } else {
            hole |= inside || boundary;
        }
    }
    check_cancelled(cancel)?;
    Ok(exterior && !hole)
}
