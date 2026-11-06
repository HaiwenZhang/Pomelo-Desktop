//! Material coverage extruded directly into display triangles, with no B-rep intermediate.

use super::Bounds3;
use crate::{model::Point, task::CancellationToken};
use clipper2_rust::{
    ClipType, EndType, FillRule, JoinType, PathD, PathsD, PointD, PolyTreeD, boolean_op_tree_d,
    inflate_paths_d, poly_tree_to_paths_d,
};
use std::ops::Range;

/// One nanometer rounding in millimeter coordinates; coordinates are rebased first.
const PRECISION: i32 = 6;
const MAX_LOCAL_COORDINATE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy)]
pub struct SolidLimits {
    pub max_points: usize,
    pub max_rings: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    /// Output and conservative temporary storage; borrowed input is excluded.
    pub max_allocation_bytes: usize,
}

impl Default for SolidLimits {
    fn default() -> Self {
        Self {
            max_points: 4_000_000,
            max_rings: 65_536,
            max_vertices: 24_000_000,
            max_indices: 96_000_000,
            max_allocation_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SolidError {
    #[error("SOLID_CANCELLED")]
    Cancelled,
    #[error("SOLID_INVALID {0}")]
    Invalid(&'static str),
    #[error("SOLID_UNSUPPORTED {0}")]
    Unsupported(&'static str),
    #[error("SOLID_LIMIT resource={resource} actual={actual} limit={limit}")]
    Limit {
        resource: &'static str,
        actual: usize,
        limit: usize,
    },
    #[error("SOLID_ALLOCATION")]
    Allocation,
}

impl From<crate::task::Cancelled> for SolidError {
    fn from(_: crate::task::Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Hard edges have separate cap/wall vertices so their normals are not averaged.
#[derive(Debug, Clone, Copy)]
pub struct SolidVertex {
    pub position: [f64; 3],
    pub normal: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Top,
    Bottom,
    OuterWall,
    HoleWall,
}

#[derive(Debug)]
pub struct SolidSurface {
    pub kind: SurfaceKind,
    pub indices: Range<u32>,
}

#[derive(Debug, Default)]
pub struct SolidMesh {
    pub vertices: Vec<SolidVertex>,
    pub indices: Vec<u32>,
    pub surfaces: Vec<SolidSurface>,
    /// None when holes remove all material.
    pub bounds: Option<Bounds3>,
}

impl SolidMesh {
    pub fn allocation_bytes(&self) -> usize {
        self.vertices
            .capacity()
            .saturating_mul(size_of::<SolidVertex>())
            .saturating_add(self.indices.capacity().saturating_mul(size_of::<u32>()))
            .saturating_add(
                self.surfaces
                    .capacity()
                    .saturating_mul(size_of::<SolidSurface>()),
            )
    }

    /// The first ring is even/odd exterior coverage; the remaining rings subtract
    /// their union, irrespective of winding, overlap, containment or input order.
    pub fn extrude_coverage(
        rings: &[Vec<Point>],
        z: [f64; 2],
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidError> {
        Self::extrude_rings(rings, &[], z, limits, cancel, true)
    }

    /// Custom pad contours use parity across all rings, including nested islands.
    pub fn extrude_even_odd(
        rings: &[Vec<Point>],
        z: [f64; 2],
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidError> {
        Self::extrude_rings(rings, &[], z, limits, cancel, false)
    }

    /// Parity copper with physical drill cutouts subtracted as a union.
    pub fn extrude_even_odd_minus(
        rings: &[Vec<Point>],
        cutouts: &[Vec<Point>],
        z: [f64; 2],
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidError> {
        Self::extrude_rings(rings, cutouts, z, limits, cancel, false)
    }

    /// A round-ended centerline stroke. Closed paths retain their interior opening.
    pub fn extrude_stroke(
        points: &[Point],
        width: f64,
        closed: bool,
        z: [f64; 2],
        tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidError> {
        cancel.check()?;
        validate_z(z)?;
        if points.len() < if closed { 3 } else { 2 }
            || !width.is_finite()
            || width <= 0.0
            || !tolerance_mm.is_finite()
            || tolerance_mm <= 0.0
        {
            return Err(SolidError::Invalid("STROKE"));
        }
        let radius = width * 0.5;
        if radius <= 0.0 {
            return Err(SolidError::Invalid("STROKE_WIDTH"));
        }
        limit("rings", 1, limits.max_rings)?;
        let step = 2.0 * (1.0 - tolerance_mm / radius).clamp(-1.0, 1.0).acos();
        if !step.is_finite() || step <= 0.0 {
            return Err(SolidError::Invalid("STROKE_TOLERANCE"));
        }
        let samples = (std::f64::consts::TAU / step).ceil().max(8.0) as usize;
        let point_budget = points.len().saturating_mul(samples);
        limit("stroke_points", point_budget, limits.max_points)?;
        let scratch = point_budget.saturating_mul(1024).saturating_add(256);
        limit("bytes", scratch, limits.max_allocation_bytes)?;
        let origin = points[0];
        if !origin.is_finite() {
            return Err(SolidError::Invalid("COORDINATES"));
        }
        let mut path = reserve(points.len())?;
        for (i, p) in points.iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let p = PointD::new(p.x - origin.x, p.y - origin.y);
            if ![p.x, p.y]
                .into_iter()
                .all(|v| v.is_finite() && v.abs() + radius <= MAX_LOCAL_COORDINATE)
            {
                return Err(SolidError::Invalid("COORDINATE_RANGE"));
            }
            path.push(p);
        }
        let paths = inflate_paths_d(
            &vec![path],
            radius,
            JoinType::Round,
            if closed {
                EndType::Joined
            } else {
                EndType::Round
            },
            2.0,
            PRECISION,
            tolerance_mm,
        );
        cancel.check()?;
        if paths.is_empty() {
            return Err(SolidError::Invalid("STROKE_PRECISION"));
        }
        let count = paths.iter().fold(0_usize, |n, p| n.saturating_add(p.len()));
        limit("stroke_points", count, limits.max_points)?;
        limit("rings", paths.len(), limits.max_rings)?;
        let mut tree = PolyTreeD::new();
        boolean_op_tree_d(
            ClipType::Union,
            FillRule::NonZero,
            &paths,
            &Vec::new(),
            &mut tree,
            PRECISION,
        );
        cancel.check()?;
        drop(paths);
        Self::from_tree(tree, origin, z, scratch, limits, cancel)
    }

    fn extrude_rings(
        rings: &[Vec<Point>],
        cutouts: &[Vec<Point>],
        z: [f64; 2],
        limits: SolidLimits,
        cancel: &CancellationToken,
        subtract_holes: bool,
    ) -> Result<Self, SolidError> {
        cancel.check()?;
        validate_z(z)?;
        let ring_count = rings.len().saturating_add(cutouts.len());
        limit("rings", ring_count, limits.max_rings)?;
        let count = rings
            .iter()
            .chain(cutouts)
            .fold(0_usize, |n, ring| n.saturating_add(ring.len()));
        limit("points", count, limits.max_points)?;
        // Includes clipping nodes, trees, triangulator nodes and output conversion scratch.
        let scratch = count
            .saturating_mul(1024)
            .saturating_add(ring_count.saturating_mul(256));
        limit("bytes", scratch, limits.max_allocation_bytes)?;
        let origin = rings
            .first()
            .and_then(|ring| ring.first())
            .copied()
            .ok_or(SolidError::Invalid("EMPTY_COVERAGE"))?;
        if !origin.is_finite() {
            return Err(SolidError::Invalid("COORDINATES"));
        }
        let mut subject = Vec::new();
        let mut holes = Vec::new();
        let mut intermediate_points = 0_usize;
        let mut intermediate_rings = 0_usize;
        // shortcut: Clipper/Earcut calls cannot be interrupted, use a killable worker if single-call latency exceeds the budget.
        for (ring_index, ring) in rings.iter().chain(cutouts).enumerate() {
            cancel.check()?;
            if ring.len() < 3 {
                return Err(SolidError::Invalid("RING_POINTS"));
            }
            let mut path = reserve(ring.len())?;
            for (i, point) in ring.iter().enumerate() {
                if i.is_multiple_of(1024) {
                    cancel.check()?;
                }
                let p = PointD::new(point.x - origin.x, point.y - origin.y);
                if ![p.x, p.y]
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= MAX_LOCAL_COORDINATE)
                {
                    return Err(SolidError::Invalid("COORDINATE_RANGE"));
                }
                path.push(p);
            }
            // Normalize each ring with parity first. Applying parity to all holes
            // together would incorrectly restore material where two holes overlap.
            let mut tree = PolyTreeD::new();
            boolean_op_tree_d(
                ClipType::Union,
                FillRule::EvenOdd,
                &vec![path],
                &Vec::new(),
                &mut tree,
                PRECISION,
            );
            cancel.check()?;
            let normalized = poly_tree_to_paths_d(&tree);
            if normalized.is_empty() {
                return Err(SolidError::Invalid("DEGENERATE_RING"));
            }
            intermediate_rings = intermediate_rings.saturating_add(normalized.len());
            limit("normalized_rings", intermediate_rings, limits.max_rings)?;
            let destination = if ring_index < rings.len() && (ring_index == 0 || !subtract_holes) {
                &mut subject
            } else {
                &mut holes
            };
            destination
                .try_reserve(normalized.len())
                .map_err(|_| SolidError::Allocation)?;
            for path in normalized {
                intermediate_points = intermediate_points.saturating_add(path.len());
                limit("normalized_points", intermediate_points, limits.max_points)?;
                limit(
                    "bytes",
                    intermediate_points
                        .saturating_mul(1024)
                        .saturating_add(intermediate_rings.saturating_mul(256)),
                    limits.max_allocation_bytes,
                )?;
                destination.push(path);
            }
        }
        let mut tree = PolyTreeD::new();
        if !subtract_holes {
            boolean_op_tree_d(
                ClipType::Union,
                FillRule::EvenOdd,
                &subject,
                &Vec::new(),
                &mut tree,
                PRECISION,
            );
            cancel.check()?;
            if holes.is_empty() {
                drop(subject);
                return Self::from_tree(tree, origin, z, scratch, limits, cancel);
            }
            subject = poly_tree_to_paths_d(&tree);
            tree = PolyTreeD::new();
        }
        boolean_op_tree_d(
            ClipType::Difference,
            FillRule::NonZero,
            &subject,
            &holes,
            &mut tree,
            PRECISION,
        );
        cancel.check()?;
        drop(subject);
        drop(holes);
        Self::from_tree(tree, origin, z, scratch, limits, cancel)
    }

    fn from_tree(
        tree: PolyTreeD,
        origin: Point,
        z: [f64; 2],
        scratch: usize,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidError> {
        let mut output_points = 0_usize;
        for (i, node) in tree.nodes.iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            output_points = output_points.saturating_add(node.polygon().len());
        }
        let output_rings = tree.nodes.len().saturating_sub(1);
        limit("normalized_points", output_points, limits.max_points)?;
        limit("normalized_rings", output_rings, limits.max_rings)?;
        let scratch = scratch.max(
            output_points
                .saturating_mul(1024)
                .saturating_add(output_rings.saturating_mul(256)),
        );
        limit("bytes", scratch, limits.max_allocation_bytes)?;
        let mut mesh = Self::default();
        let mut pending: Vec<_> = tree.root().children().iter().rev().copied().collect();
        while let Some(index) = pending.pop() {
            cancel.check()?;
            let node = &tree.nodes[index];
            if !tree.is_hole(index) {
                for &child in node.children() {
                    if !tree.is_hole(child) {
                        return Err(SolidError::Invalid("HOLE_TREE"));
                    }
                }
                let mut region = reserve(node.children().len() + 1)?;
                region.push(node.polygon().clone());
                region.extend(
                    node.children()
                        .iter()
                        .map(|&child| tree.nodes[child].polygon().clone()),
                );
                mesh.append_region(region, origin, z, scratch, limits, cancel)?;
            }
            pending.extend(node.children().iter().rev().copied());
        }
        cancel.check()?;
        Ok(mesh)
    }

    fn append_region(
        &mut self,
        mut rings: PathsD,
        origin: Point,
        z: [f64; 2],
        scratch: usize,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<(), SolidError> {
        for ring in &mut rings {
            remove_collinear(ring, cancel)?;
        }
        let count = rings.iter().map(Vec::len).sum::<usize>();
        // A connected polygon with H holes has N+2H-2 cap triangles.
        let cap_count = count
            .saturating_add(rings.len().saturating_sub(1).saturating_mul(2))
            .saturating_sub(2)
            .saturating_mul(3);
        let vertices = self.vertices.len().saturating_add(count.saturating_mul(6));
        let indices = self
            .indices
            .len()
            .saturating_add(cap_count.saturating_mul(2))
            .saturating_add(count.saturating_mul(6));
        let surfaces = self
            .surfaces
            .len()
            .saturating_add(2)
            .saturating_add(rings.len());
        limit(
            "vertices",
            vertices,
            limits.max_vertices.min(u32::MAX as usize),
        )?;
        limit(
            "indices",
            indices,
            limits.max_indices.min(u32::MAX as usize),
        )?;
        let bytes = vertices
            .saturating_mul(size_of::<SolidVertex>())
            .saturating_add(indices.saturating_mul(size_of::<u32>()))
            .saturating_add(surfaces.saturating_mul(size_of::<SolidSurface>()))
            .saturating_add(scratch);
        limit("bytes", bytes, limits.max_allocation_bytes)?;
        limit("earcut_nodes", count.saturating_mul(256), u32::MAX as usize)?;
        self.vertices
            .try_reserve_exact(vertices - self.vertices.len())
            .map_err(|_| SolidError::Allocation)?;
        self.indices
            .try_reserve_exact(indices - self.indices.len())
            .map_err(|_| SolidError::Allocation)?;
        self.surfaces
            .try_reserve_exact(surfaces - self.surfaces.len())
            .map_err(|_| SolidError::Allocation)?;
        limit(
            "bytes",
            self.allocation_bytes().saturating_add(scratch),
            limits.max_allocation_bytes,
        )?;
        let mut points = reserve(count)?;
        let mut holes: Vec<u32> = reserve(rings.len().saturating_sub(1))?;
        let mut expected_area = 0.0;
        for (i, ring) in rings.iter_mut().enumerate() {
            cancel.check()?;
            let area = signed_area(ring);
            if ring.len() < 3 || !area.is_finite() || area == 0.0 {
                return Err(SolidError::Invalid("NORMALIZED_RING"));
            }
            if (area > 0.0) != (i == 0) {
                ring.reverse();
            }
            expected_area += if i == 0 { area.abs() } else { -area.abs() };
            if i > 0 {
                holes.push(
                    points
                        .len()
                        .try_into()
                        .map_err(|_| SolidError::Invalid("HOLE_OFFSET"))?,
                );
            }
            points.extend(ring.iter().map(|p| [p.x, p.y]));
        }
        if expected_area <= 0.0 {
            return Err(SolidError::Invalid("REGION_AREA"));
        }
        let mut triangles: Vec<u32> = reserve(cap_count)?;
        let mut triangulator = earcut::Earcut::<f64>::new();
        cancel.check()?;
        triangulator.earcut(points.iter().copied(), &holes, &mut triangles);
        cancel.check()?;
        if triangles.is_empty()
            || triangles.len() > cap_count
            || !triangles.len().is_multiple_of(3)
            || triangles.iter().any(|&i| i as usize >= points.len())
        {
            return Err(SolidError::Invalid("TRIANGULATION"));
        }
        let base = self.vertices.len() as u32;
        for (i, p) in points.iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let position = [origin.x + p[0], origin.y + p[1], z[1]];
            if !position.into_iter().all(f64::is_finite) {
                return Err(SolidError::Invalid("COORDINATES"));
            }
            self.vertices.push(SolidVertex {
                position,
                normal: [0.0, 0.0, 1.0],
            });
            if let Some(bounds) = &mut self.bounds {
                bounds.min[0] = bounds.min[0].min(position[0]);
                bounds.min[1] = bounds.min[1].min(position[1]);
                bounds.max[0] = bounds.max[0].max(position[0]);
                bounds.max[1] = bounds.max[1].max(position[1]);
            } else {
                self.bounds = Some(Bounds3 {
                    min: [position[0], position[1], z[0]],
                    max: position,
                });
            }
        }
        for i in 0..count {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let mut vertex = self.vertices[base as usize + i];
            vertex.position[2] = z[0];
            vertex.normal = [0.0, 0.0, -1.0];
            self.vertices.push(vertex);
        }
        let mut area_sum = 0.0;
        let top = self.indices.len() as u32;
        for (i, triangle) in triangles.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let a = points[triangle[0] as usize];
            let b = points[triangle[1] as usize];
            let c = points[triangle[2] as usize];
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if !area.is_finite() || area == 0.0 {
                return Err(SolidError::Invalid("TRIANGLE_AREA"));
            }
            area_sum += area.abs() * 0.5;
            if area < 0.0 {
                triangle.swap(1, 2);
            }
            self.indices.extend(triangle.iter().map(|i| base + *i));
        }
        if (area_sum - expected_area).abs() > expected_area.abs() * 1e-8 + 1e-12 {
            return Err(SolidError::Invalid("TRIANGULATION_AREA"));
        }
        self.surfaces.push(SolidSurface {
            kind: SurfaceKind::Top,
            indices: top..self.indices.len() as u32,
        });
        let bottom = self.indices.len() as u32;
        for (i, t) in triangles.as_chunks::<3>().0.iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let b = base + count as u32;
            self.indices.extend([b + t[0], b + t[2], b + t[1]]);
        }
        self.surfaces.push(SolidSurface {
            kind: SurfaceKind::Bottom,
            indices: bottom..self.indices.len() as u32,
        });
        for (i, ring) in rings.iter().enumerate() {
            let start = self.indices.len() as u32;
            for edge in 0..ring.len() {
                if edge.is_multiple_of(1024) {
                    cancel.check()?;
                }
                let a = &ring[edge];
                let b = &ring[(edge + 1) % ring.len()];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let length = dx.hypot(dy);
                if !length.is_finite() || length == 0.0 {
                    return Err(SolidError::Invalid("WALL_EDGE"));
                }
                let normal = [(dy / length) as f32, (-dx / length) as f32, 0.0];
                let base = self.vertices.len() as u32;
                for position in [
                    [origin.x + a.x, origin.y + a.y, z[0]],
                    [origin.x + b.x, origin.y + b.y, z[0]],
                    [origin.x + b.x, origin.y + b.y, z[1]],
                    [origin.x + a.x, origin.y + a.y, z[1]],
                ] {
                    self.vertices.push(SolidVertex { position, normal });
                }
                self.indices
                    .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            self.surfaces.push(SolidSurface {
                kind: if i == 0 {
                    SurfaceKind::OuterWall
                } else {
                    SurfaceKind::HoleWall
                },
                indices: start..self.indices.len() as u32,
            });
        }
        Ok(())
    }
}

fn signed_area(ring: &PathD) -> f64 {
    let Some(first) = ring.first() else {
        return 0.0;
    };
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(a, b)| (a.x - first.x) * (b.y - first.y) - (b.x - first.x) * (a.y - first.y))
        .sum::<f64>()
        * 0.5
}

fn validate_z(z: [f64; 2]) -> Result<(), SolidError> {
    if !z.into_iter().all(f64::is_finite) || z[0] >= z[1] || !(z[1] - z[0]).is_finite() {
        Err(SolidError::Invalid("Z_INTERVAL"))
    } else {
        Ok(())
    }
}

fn remove_collinear(ring: &mut PathD, cancel: &CancellationToken) -> Result<(), SolidError> {
    if ring.len() < 3 {
        return Err(SolidError::Invalid("NORMALIZED_RING"));
    }
    let mut filtered = reserve(ring.len())?;
    let grid = |p: &PointD| [(p.x * 1e6).round() as i128, (p.y * 1e6).round() as i128];
    for i in 0..ring.len() {
        if i.is_multiple_of(1024) {
            cancel.check()?;
        }
        let a = grid(&ring[(i + ring.len() - 1) % ring.len()]);
        let b = grid(&ring[i]);
        let c = grid(&ring[(i + 1) % ring.len()]);
        // Clipper output lies on this integer grid. Remove only exact collinearity,
        // so cap triangulation and walls share the same unsplit boundary edges.
        if (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) != 0 {
            filtered.push(PointD::new(ring[i].x, ring[i].y));
        }
    }
    if filtered.len() < 3 {
        return Err(SolidError::Invalid("NORMALIZED_RING"));
    }
    *ring = filtered;
    Ok(())
}

fn limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), SolidError> {
    if actual > limit {
        Err(SolidError::Limit {
            resource,
            actual,
            limit,
        })
    } else {
        Ok(())
    }
}

fn reserve<T>(count: usize) -> Result<Vec<T>, SolidError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| SolidError::Allocation)?;
    Ok(result)
}
