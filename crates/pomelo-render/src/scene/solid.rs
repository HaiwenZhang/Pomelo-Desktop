//! Source-aware solid preparation. Layer heights are explicit inputs, never UI order.

mod pads;
mod traces;
mod vias;
pub use pads::PadSolidSource;
pub use vias::ViaWallSpan;

use pomelo_core::{
    geometry::{PathError, flatten_path},
    geometry3d::{
        Bounds3,
        solid::{SolidError, SolidLimits, SolidMesh},
    },
    interaction::selection::{MemberLayers, SelectedObject},
    model::{NetId, Point, Zone},
    pad::{PadOutline, PadPlacement, curve_samples},
    task::CancellationToken,
};

#[derive(Debug)]
pub struct PreparedSolid {
    pub object: SelectedObject,
    /// Source layer scope; retained drill segments use their mesh Z bounds for depth.
    pub layers: MemberLayers,
    pub net: NetId,
    pub mesh: SolidMesh,
    /// Pad/Via geometry stays local for sharing; Zone/Trace use the identity placement.
    pub placement: PadPlacement,
}

#[derive(Debug, thiserror::Error)]
pub enum SolidPrepareError {
    #[error("SOLID_PREPARE_SCENE {0}")]
    Scene(#[source] SolidError),
    #[error("SOLID_PREPARE_GEOMETRY object={object:?}: {source}")]
    Geometry {
        object: SelectedObject,
        source: SolidError,
    },
    #[error("SOLID_PREPARE_CURVE object={object:?}: {source}")]
    Curve {
        object: SelectedObject,
        source: PathError,
    },
    #[error("SOLID_PREPARE_DRILL object={object:?}: {source}")]
    Drill {
        object: SelectedObject,
        source: pomelo_core::geometry3d::drill::DrillSpanError,
    },
}

pub(super) fn outline_rings(
    outline: PadOutline,
    tolerance: f64,
    limits: SolidLimits,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<Point>>, SolidError> {
    cancel.check()?;
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(SolidError::Invalid("CHORD_TOLERANCE"));
    }
    let (a, b, radius, closed) = match outline {
        PadOutline::Rings(rings) => {
            check_ring_budget(&rings, limits)?;
            return Ok(rings);
        }
        PadOutline::Circle { center, radius } => (center, center, radius, true),
        PadOutline::Capsule { a, b, radius } => (a, b, radius, a == b),
    };
    if !a.is_finite() || !b.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Err(SolidError::Invalid("OUTLINE"));
    }
    let sweep = if closed {
        std::f64::consts::TAU
    } else {
        std::f64::consts::PI
    };
    let samples = curve_count(radius, sweep, tolerance, if closed { 8 } else { 4 })?;
    let count = if closed {
        samples
    } else {
        samples.saturating_add(1).saturating_mul(2)
    };
    check_points(count, limits)?;
    let mut ring = Vec::new();
    ring.try_reserve_exact(count)
        .map_err(|_| SolidError::Allocation)?;
    if closed {
        for i in 0..samples {
            if i.is_multiple_of(1024) {
                cancel.check()?;
            }
            let angle = std::f64::consts::TAU * i as f64 / samples as f64;
            ring.push(Point::new(
                a.x + radius * angle.cos(),
                a.y + radius * angle.sin(),
            ));
        }
    } else {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        if !dx.is_finite() || !dy.is_finite() {
            return Err(SolidError::Invalid("OUTLINE"));
        }
        let direction = dy.atan2(dx);
        for (center, start) in [
            (b, direction - std::f64::consts::FRAC_PI_2),
            (a, direction + std::f64::consts::FRAC_PI_2),
        ] {
            for i in 0..=samples {
                if i.is_multiple_of(1024) {
                    cancel.check()?;
                }
                let angle = start + std::f64::consts::PI * i as f64 / samples as f64;
                ring.push(Point::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                ));
            }
        }
    }
    Ok(vec![ring])
}

fn check_points(count: usize, limits: SolidLimits) -> Result<(), SolidError> {
    if limits.max_rings == 0 {
        return Err(SolidError::Limit {
            resource: "rings",
            actual: 1,
            limit: 0,
        });
    }
    if count > limits.max_points {
        return Err(SolidError::Limit {
            resource: "points",
            actual: count,
            limit: limits.max_points,
        });
    }
    let bytes = count
        .saturating_mul(size_of::<Point>())
        .saturating_add(size_of::<Vec<Point>>());
    if bytes > limits.max_allocation_bytes {
        return Err(SolidError::Limit {
            resource: "bytes",
            actual: bytes,
            limit: limits.max_allocation_bytes,
        });
    }
    Ok(())
}

fn check_ring_budget(rings: &Vec<Vec<Point>>, limits: SolidLimits) -> Result<(), SolidError> {
    if rings.len() > limits.max_rings {
        return Err(SolidError::Limit {
            resource: "rings",
            actual: rings.len(),
            limit: limits.max_rings,
        });
    }
    check_points(
        rings.iter().fold(0_usize, |n, r| n.saturating_add(r.len())),
        limits,
    )?;
    let bytes = ring_bytes(rings);
    if bytes > limits.max_allocation_bytes {
        return Err(SolidError::Limit {
            resource: "bytes",
            actual: bytes,
            limit: limits.max_allocation_bytes,
        });
    }
    Ok(())
}

pub(super) fn ring_bytes(rings: &Vec<Vec<Point>>) -> usize {
    rings.iter().fold(
        rings.capacity().saturating_mul(size_of::<Vec<Point>>()),
        |n, r| n.saturating_add(r.capacity().saturating_mul(size_of::<Point>())),
    )
}

fn curve_count(
    radius: f64,
    sweep: f64,
    tolerance: f64,
    minimum: usize,
) -> Result<usize, SolidError> {
    let count = curve_samples(radius, sweep, tolerance).max(minimum);
    if !radius.is_finite()
        || radius < 0.0
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || radius * (1.0 - (sweep / (2.0 * count as f64)).cos()) > tolerance * (1.0 + 1e-9)
    {
        Err(SolidError::Invalid("CURVE_TOLERANCE_LIMIT"))
    } else {
        Ok(count)
    }
}

impl PreparedSolid {
    /// Conservative source-space bounds after the local mesh's placement.
    pub fn world_bounds(&self) -> Option<Bounds3> {
        let bounds = self.mesh.bounds?;
        if !bounds.is_valid() || !self.placement.at.is_finite() || !self.placement.angle.is_finite()
        {
            return None;
        }
        let mut result = Bounds3 {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        };
        for p in bounds.corners() {
            let xy = Point::new(p[0], if self.placement.mirrored { -p[1] } else { p[1] })
                .rotate(self.placement.angle);
            let point = [xy.x + self.placement.at.x, xy.y + self.placement.at.y, p[2]];
            if !point.into_iter().all(f64::is_finite) {
                return None;
            }
            for (i, value) in point.into_iter().enumerate() {
                result.min[i] = result.min[i].min(value);
                result.max[i] = result.max[i].max(value);
            }
        }
        result.is_valid().then_some(result)
    }

    /// Curves are resampled at the requested model-space LOD before subtraction.
    pub fn prepare_zone(
        zone: &Zone,
        z: [f64; 2],
        chord_tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        Self::prepare_zone_with_cutouts(zone, z, chord_tolerance_mm, &[], limits, cancel)
    }
    pub(crate) fn prepare_zone_with_cutouts(
        zone: &Zone,
        z: [f64; 2],
        chord_tolerance_mm: f64,
        cutouts: &[Vec<Point>],
        mut limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        let geometry = |source| SolidPrepareError::Geometry {
            object: SelectedObject::Zone(zone.id),
            source,
        };
        cancel.check().map_err(|e| geometry(e.into()))?;
        if !chord_tolerance_mm.is_finite() || chord_tolerance_mm <= 0.0 {
            return Err(geometry(SolidError::Invalid("CHORD_TOLERANCE")));
        }
        let count = if zone.paths.is_empty() {
            zone.mesh.ring_offsets.len().saturating_sub(1)
        } else {
            zone.paths.len()
        };
        if count > limits.max_rings {
            return Err(geometry(SolidError::Limit {
                resource: "rings",
                actual: count,
                limit: limits.max_rings,
            }));
        }
        let bytes = count.saturating_mul(size_of::<Vec<Point>>());
        if bytes > limits.max_allocation_bytes {
            return Err(geometry(SolidError::Limit {
                resource: "bytes",
                actual: bytes,
                limit: limits.max_allocation_bytes,
            }));
        }
        let mut rings = Vec::new();
        rings
            .try_reserve_exact(count)
            .map_err(|_| geometry(SolidError::Allocation))?;
        let mut points = 0_usize;
        let mut allocation = rings.capacity().saturating_mul(size_of::<Vec<Point>>());
        if zone.paths.is_empty() {
            let mesh = &zone.mesh;
            if mesh.ring_offsets.first() != Some(&0)
                || mesh.ring_offsets.last().copied().map(|i| i as usize)
                    != Some(mesh.vertices.len())
            {
                return Err(geometry(SolidError::Invalid("RING_OFFSETS")));
            }
            for range in mesh.ring_offsets.windows(2) {
                cancel.check().map_err(|e| geometry(e.into()))?;
                let slice = mesh
                    .vertices
                    .get(range[0] as usize..range[1] as usize)
                    .ok_or_else(|| geometry(SolidError::Invalid("RING_OFFSETS")))?;
                points = points.saturating_add(slice.len());
                if points > limits.max_points {
                    return Err(geometry(SolidError::Limit {
                        resource: "points",
                        actual: points,
                        limit: limits.max_points,
                    }));
                }
                let bytes =
                    allocation.saturating_add(slice.len().saturating_mul(size_of::<Point>()));
                if bytes > limits.max_allocation_bytes {
                    return Err(geometry(SolidError::Limit {
                        resource: "bytes",
                        actual: bytes,
                        limit: limits.max_allocation_bytes,
                    }));
                }
                let mut ring = Vec::new();
                ring.try_reserve_exact(slice.len())
                    .map_err(|_| geometry(SolidError::Allocation))?;
                for chunk in slice.chunks(1024) {
                    cancel.check().map_err(|e| geometry(e.into()))?;
                    ring.extend_from_slice(chunk);
                }
                allocation =
                    allocation.saturating_add(ring.capacity().saturating_mul(size_of::<Point>()));
                rings.push(ring);
            }
        } else {
            for path in &zone.paths {
                cancel.check().map_err(|e| geometry(e.into()))?;
                let remaining_bytes = limits.max_allocation_bytes.saturating_sub(allocation);
                let max_points = limits
                    .max_points
                    .saturating_sub(points)
                    .min(remaining_bytes / (2 * size_of::<Point>()));
                let ring = flatten_path(path, chord_tolerance_mm, max_points, cancel).map_err(
                    |source| SolidPrepareError::Curve {
                        object: SelectedObject::Zone(zone.id),
                        source,
                    },
                )?;
                points = points.saturating_add(ring.len());
                allocation =
                    allocation.saturating_add(ring.capacity().saturating_mul(size_of::<Point>()));
                rings.push(ring);
            }
        }
        if allocation > limits.max_allocation_bytes {
            return Err(geometry(SolidError::Limit {
                resource: "bytes",
                actual: allocation,
                limit: limits.max_allocation_bytes,
            }));
        }
        limits.max_allocation_bytes -= allocation;
        let mesh = SolidMesh::extrude_coverage_minus(&rings, cutouts, z, limits, cancel)
            .map_err(geometry)?;
        Ok(Self {
            object: SelectedObject::Zone(zone.id),
            layers: MemberLayers::Single(zone.layer),
            net: zone.net,
            mesh,
            placement: PadPlacement::default(),
        })
    }
}
