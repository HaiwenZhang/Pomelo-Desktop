use super::{
    PreparedSolid, SolidPrepareError, check_points, check_ring_budget, curve_count, outline_rings,
    ring_bytes,
};
use pomelo_core::{
    geometry::flatten_path,
    geometry3d::solid::{SolidError, SolidLimits, SolidMesh},
    interaction::selection::{MemberLayers, SelectedObject},
    model::{DrillShape, NetId, Pad, Point},
    pad::PadPlacement,
    task::CancellationToken,
};

pub struct PadSolidSource<'a> {
    pub object: SelectedObject,
    pub net: NetId,
    pub pad: &'a Pad,
    pub placement: PadPlacement,
    /// Supply only a hole that crosses the whole supplied Z interval. Partial-depth
    /// cuts must be split into separate intervals by physical stackup preparation.
    pub drill: Option<DrillShape>,
}

impl PreparedSolid {
    pub fn prepare_pad(
        source: PadSolidSource<'_>,
        z: [f64; 2],
        tolerance_mm: f64,
        mut limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        let error = |source_error| SolidPrepareError::Geometry {
            object: source.object,
            source: source_error,
        };
        cancel.check().map_err(|e| error(e.into()))?;
        let pad = source.pad;
        if !matches!(
            source.object,
            SelectedObject::Pin(_) | SelectedObject::Via(_)
        ) || !tolerance_mm.is_finite()
            || tolerance_mm <= 0.0
            || ![
                source.placement.at.x,
                source.placement.at.y,
                source.placement.angle,
                pad.offset.x,
                pad.offset.y,
                pad.width,
                pad.height,
                pad.corner,
            ]
            .into_iter()
            .all(f64::is_finite)
            || pad.width < 0.0
            || pad.height < 0.0
        {
            return Err(error(SolidError::Invalid("PAD")));
        }
        let mut local_pad = pad.clone();
        local_pad.offset = Point::default();
        let local_owner = PadPlacement {
            mirrored: source.placement.mirrored,
            ..Default::default()
        };
        let rings = if let Some(custom) = &local_pad.custom {
            let count = if custom.paths.is_empty() {
                custom.contours.len()
            } else {
                custom.paths.len()
            };
            if count > limits.max_rings {
                return Err(error(SolidError::Limit {
                    resource: "rings",
                    actual: count,
                    limit: limits.max_rings,
                }));
            }
            let bytes = count.saturating_mul(size_of::<Vec<Point>>());
            if bytes > limits.max_allocation_bytes {
                return Err(error(SolidError::Limit {
                    resource: "bytes",
                    actual: bytes,
                    limit: limits.max_allocation_bytes,
                }));
            }
            let mut result = Vec::new();
            result
                .try_reserve_exact(count)
                .map_err(|_| error(SolidError::Allocation))?;
            let mut points = 0_usize;
            let mut allocation = result.capacity().saturating_mul(size_of::<Vec<Point>>());
            for i in 0..count {
                cancel.check().map_err(|e| error(e.into()))?;
                let remaining = limits.max_points.saturating_sub(points).min(
                    limits.max_allocation_bytes.saturating_sub(allocation)
                        / (2 * size_of::<Point>()),
                );
                let mut ring = if custom.paths.is_empty() {
                    let input = &custom.contours[i];
                    if input.len() > remaining {
                        return Err(error(SolidError::Limit {
                            resource: "points",
                            actual: input.len(),
                            limit: remaining,
                        }));
                    }
                    let mut ring = Vec::new();
                    ring.try_reserve_exact(input.len())
                        .map_err(|_| error(SolidError::Allocation))?;
                    for chunk in input.chunks(1024) {
                        cancel.check().map_err(|e| error(e.into()))?;
                        ring.extend_from_slice(chunk);
                    }
                    ring
                } else {
                    flatten_path(&custom.paths[i], tolerance_mm, remaining, cancel).map_err(
                        |source_error| SolidPrepareError::Curve {
                            object: source.object,
                            source: source_error,
                        },
                    )?
                };
                for chunk in ring.chunks_mut(1024) {
                    cancel.check().map_err(|e| error(e.into()))?;
                    for p in chunk {
                        *p = local_pad.to_world(*p, local_owner);
                    }
                }
                points = points.saturating_add(ring.len());
                allocation =
                    allocation.saturating_add(ring.capacity().saturating_mul(size_of::<Point>()));
                result.push(ring);
            }
            result
        } else {
            let n = match local_pad.kind.0 {
                2 => 0,
                11 | 12 => 0,
                25 => curve_count(pad.width * 0.5, std::f64::consts::TAU, tolerance_mm, 8)
                    .map_err(error)?
                    .saturating_mul(2),
                3 => 8,
                5 | 6 | 27 | 28 => curve_count(
                    pad.corner_radius(),
                    std::f64::consts::FRAC_PI_2,
                    tolerance_mm,
                    1,
                )
                .map_err(error)?
                .saturating_add(1)
                .saturating_mul(4),
                _ => return Err(error(SolidError::Unsupported("PAD_FAMILY"))),
            };
            check_points(n, limits).map_err(error)?;
            let outline = local_pad
                .outline(local_owner, tolerance_mm)
                .map_err(|_| error(SolidError::Invalid("PAD_OUTLINE")))?
                .ok_or_else(|| error(SolidError::Unsupported("PAD_FAMILY")))?;
            outline_rings(outline, tolerance_mm, limits, cancel).map_err(error)?
        };
        check_ring_budget(&rings, limits).map_err(error)?;
        let mut cutouts = Vec::new();
        if let Some(drill) = source.drill {
            limits.max_allocation_bytes = limits
                .max_allocation_bytes
                .saturating_sub(ring_bytes(&rings));
            let hole = drill
                .pad()
                .ok_or_else(|| error(SolidError::Invalid("DRILL")))?;
            let center = Point::new(-pad.offset.x, -pad.offset.y).rotate(-source.placement.angle);
            let outline = hole
                .outline(
                    PadPlacement {
                        at: center,
                        mirrored: source.placement.mirrored,
                        ..Default::default()
                    },
                    tolerance_mm,
                )
                .map_err(|_| error(SolidError::Invalid("DRILL_OUTLINE")))?
                .ok_or_else(|| error(SolidError::Unsupported("DRILL_OUTLINE")))?;
            cutouts = outline_rings(outline, tolerance_mm, limits, cancel).map_err(error)?;
        } else {
            limits.max_allocation_bytes = limits
                .max_allocation_bytes
                .saturating_sub(ring_bytes(&rings));
        }
        limits.max_allocation_bytes = limits
            .max_allocation_bytes
            .saturating_sub(ring_bytes(&cutouts));
        let mesh = SolidMesh::extrude_even_odd_minus(&rings, &cutouts, z, limits, cancel)
            .map_err(error)?;
        let at = Point::new(
            source.placement.at.x + pad.offset.x,
            source.placement.at.y + pad.offset.y,
        );
        if !at.is_finite() {
            return Err(error(SolidError::Invalid("PLACEMENT")));
        }
        Ok(Self {
            object: source.object,
            layers: MemberLayers::Single(pad.layer),
            net: source.net,
            mesh,
            placement: PadPlacement {
                at,
                angle: source.placement.angle,
                mirrored: false,
            },
        })
    }
}
