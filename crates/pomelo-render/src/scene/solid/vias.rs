use super::{PreparedSolid, SolidPrepareError, outline_rings_with_segments, ring_bytes};
use pomelo_core::{
    geometry3d::{
        drill::{DrillCut3d, DrillSpanError, MAX_DRILL_CUTS, retained_drill_spans},
        solid::{SolidError, SolidLimits, SolidMesh},
    },
    interaction::selection::{MemberLayers, SelectedObject},
    model::{DrillShape, LayerId, Via},
    pad::PadPlacement,
    task::CancellationToken,
};

/// A physically resolved plated interval. Preparation can subtract actual machining
/// cuts from it; Z endpoints and thickness are never inferred from display metadata.
#[derive(Debug, Clone, Copy)]
pub struct ViaWallSpan {
    pub z: [f64; 2],
    pub layers: [LayerId; 2],
    pub plating_thickness_mm: f64,
}

impl PreparedSolid {
    /// Prepare every retained barrel segment. Layer membership remains source scope;
    /// each segment's physical extent is recorded by its mesh's Z bounds.
    pub fn prepare_via_walls(
        via: &Via,
        span: ViaWallSpan,
        cuts: &[DrillCut3d],
        tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Vec<Self>, SolidPrepareError> {
        validate_wall_input(via, span, tolerance_mm, cancel)?;
        let object = SelectedObject::Via(via.id);
        let drill_error = |source| SolidPrepareError::Drill { object, source };
        if via.backdrill.is_some() && cuts.is_empty() {
            return Err(SolidPrepareError::Geometry {
                object,
                source: SolidError::Unsupported("BACKDRILL_CUTS_REQUIRED"),
            });
        }
        if cuts.len() > MAX_DRILL_CUTS {
            return Err(drill_error(DrillSpanError::CutLimit));
        }
        let capacity = cuts.len() + 1;
        let metadata = capacity.saturating_mul(2 * size_of::<[f64; 2]>() + size_of::<Self>());
        if metadata > limits.max_allocation_bytes {
            return Err(SolidPrepareError::Geometry {
                object,
                source: SolidError::Limit {
                    resource: "bytes",
                    actual: metadata,
                    limit: limits.max_allocation_bytes,
                },
            });
        }
        let outer =
            via.drill_shape.width.max(via.drill_shape.height) + 2.0 * span.plating_thickness_mm;
        let intervals = retained_drill_spans(span.z, outer, cuts).map_err(drill_error)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(intervals.len())
            .map_err(|_| SolidPrepareError::Geometry {
                object,
                source: SolidError::Allocation,
            })?;
        let mut remaining = limits;
        let used = result
            .capacity()
            .saturating_mul(size_of::<Self>())
            .saturating_add(intervals.capacity().saturating_mul(size_of::<[f64; 2]>()));
        if used > remaining.max_allocation_bytes {
            return Err(SolidPrepareError::Geometry {
                object,
                source: SolidError::Limit {
                    resource: "bytes",
                    actual: used,
                    limit: remaining.max_allocation_bytes,
                },
            });
        }
        remaining.max_allocation_bytes -= used;
        for &z in &intervals {
            let prepared = Self::prepare_via_wall(
                via,
                ViaWallSpan { z, ..span },
                tolerance_mm,
                remaining,
                cancel,
            )?;
            remaining.max_vertices = remaining
                .max_vertices
                .saturating_sub(prepared.mesh.vertices.len());
            remaining.max_indices = remaining
                .max_indices
                .saturating_sub(prepared.mesh.indices.len());
            remaining.max_allocation_bytes = remaining
                .max_allocation_bytes
                .saturating_sub(prepared.mesh.allocation_bytes());
            result.push(prepared);
        }
        Ok(result)
    }

    pub fn prepare_via_wall(
        via: &Via,
        span: ViaWallSpan,
        tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        Self::prepare_via_geometry(via, span, tolerance_mm, limits, cancel, false)
    }

    /// Solid via display body for the 3D preview, including the hole center.
    pub fn prepare_via_body(
        via: &Via,
        span: ViaWallSpan,
        tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        Self::prepare_via_geometry(via, span, tolerance_mm, limits, cancel, true)
    }

    fn prepare_via_geometry(
        via: &Via,
        span: ViaWallSpan,
        tolerance_mm: f64,
        mut limits: SolidLimits,
        cancel: &CancellationToken,
        filled: bool,
    ) -> Result<Self, SolidPrepareError> {
        let object = SelectedObject::Via(via.id);
        let error = |source| SolidPrepareError::Geometry { object, source };
        validate_wall_input(via, span, tolerance_mm, cancel)?;
        let inner = via
            .drill_shape
            .pad()
            .ok_or_else(|| error(SolidError::Invalid("DRILL")))?;
        let outer = DrillShape {
            custom: None,
            rectangular: via.drill_shape.rectangular,
            width: via.drill_shape.width + 2.0 * span.plating_thickness_mm,
            height: via.drill_shape.height + 2.0 * span.plating_thickness_mm,
            plated: true,
        }
        .pad()
        .ok_or_else(|| error(SolidError::Invalid("PLATING_THICKNESS")))?;
        let circle_segments = if filled
            && via.drill_shape.custom.is_none()
            && !via.drill_shape.rectangular
            && via.drill_shape.width == via.drill_shape.height
        {
            128
        } else {
            32
        };
        let outline = |pad: &pomelo_core::model::Pad| {
            pad.outline(
                PadPlacement {
                    mirrored: via.mirrored,
                    ..Default::default()
                },
                tolerance_mm,
            )
            .map_err(|_| error(SolidError::Invalid("DRILL_OUTLINE")))?
            .ok_or_else(|| error(SolidError::Unsupported("DRILL_OUTLINE")))
        };
        let mut rings = if via.drill_shape.custom.is_some() {
            use clipper2_rust::{EndType, FillRule, JoinType, PointD, inflate_paths_d, union_d};
            let contours = outline_rings_with_segments(
                outline(&inner)?,
                tolerance_mm,
                limits,
                cancel,
                circle_segments,
            )
            .map_err(error)?;
            let scratch = contours
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                .saturating_mul(256);
            if scratch > limits.max_allocation_bytes {
                return Err(error(SolidError::Limit {
                    resource: "bytes",
                    actual: scratch,
                    limit: limits.max_allocation_bytes,
                }));
            }
            let paths = contours
                .iter()
                .map(|r| r.iter().map(|p| PointD::new(p.x, p.y)).collect())
                .collect();
            let normalized = union_d(&paths, &vec![], FillRule::EvenOdd, 6);
            cancel.check().map_err(|e| error(e.into()))?;
            let offset = inflate_paths_d(
                &normalized,
                span.plating_thickness_mm,
                JoinType::Round,
                EndType::Polygon,
                2.0,
                6,
                tolerance_mm,
            );
            let rings: Vec<Vec<_>> = offset
                .into_iter()
                .map(|r| {
                    r.into_iter()
                        .map(|p| pomelo_core::model::Point::new(p.x, p.y))
                        .collect()
                })
                .collect();
            super::check_ring_budget(&rings, limits).map_err(error)?;
            rings
        } else {
            outline_rings_with_segments(
                outline(&outer)?,
                tolerance_mm,
                limits,
                cancel,
                circle_segments,
            )
            .map_err(error)?
        };
        if !filled {
            let used = ring_bytes(&rings);
            let mut remaining = limits;
            remaining.max_allocation_bytes = remaining.max_allocation_bytes.saturating_sub(used);
            let holes = outline_rings_with_segments(
                outline(&inner)?,
                tolerance_mm,
                remaining,
                cancel,
                circle_segments,
            )
            .map_err(error)?;
            rings
                .try_reserve_exact(holes.len())
                .map_err(|_| error(SolidError::Allocation))?;
            rings.extend(holes);
        }
        limits.max_allocation_bytes = limits
            .max_allocation_bytes
            .saturating_sub(ring_bytes(&rings));
        let mesh = SolidMesh::extrude_coverage(&rings, span.z, limits, cancel).map_err(error)?;
        Ok(Self {
            object,
            layers: MemberLayers::Span(span.layers[0], span.layers[1]),
            net: via.net,
            mesh,
            placement: PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: false,
            },
        })
    }
}

fn validate_wall_input(
    via: &Via,
    span: ViaWallSpan,
    tolerance_mm: f64,
    cancel: &CancellationToken,
) -> Result<(), SolidPrepareError> {
    let error = |source| SolidPrepareError::Geometry {
        object: SelectedObject::Via(via.id),
        source,
    };
    cancel.check().map_err(|e| error(e.into()))?;
    if !via.drill_shape.plated {
        return Err(error(SolidError::Unsupported("NPTH_WALL")));
    }
    if !span.plating_thickness_mm.is_finite()
        || span.plating_thickness_mm <= 0.0
        || !via.at.is_finite()
        || !via.angle.is_finite()
        || !tolerance_mm.is_finite()
        || tolerance_mm <= 0.0
        || !span.z.into_iter().all(f64::is_finite)
        || span.z[0] >= span.z[1]
        || !(span.z[1] - span.z[0]).is_finite()
    {
        return Err(error(SolidError::Invalid("VIA_WALL")));
    }
    if ![via.drill_shape.width, via.drill_shape.height]
        .into_iter()
        .all(|v| v.is_finite() && v > 0.0)
    {
        return Err(error(SolidError::Invalid("DRILL")));
    }
    Ok(())
}
