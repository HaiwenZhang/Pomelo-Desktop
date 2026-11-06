use super::{PreparedSolid, SolidPrepareError, outline_rings, ring_bytes};
use pomelo_core::{
    geometry::flatten_path,
    geometry3d::solid::{SolidError, SolidLimits, SolidMesh},
    interaction::selection::{MemberLayers, SelectedObject},
    model::{Point, Segment},
    pad::{PadOutline, PadPlacement},
    task::CancellationToken,
};

impl PreparedSolid {
    pub fn prepare_trace(
        segment: &Segment,
        z: [f64; 2],
        tolerance_mm: f64,
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        Self::prepare_trace_with_cutouts(segment, z, tolerance_mm, &[], limits, cancel)
    }
    pub(crate) fn prepare_trace_with_cutouts(
        segment: &Segment,
        z: [f64; 2],
        tolerance_mm: f64,
        cutouts: &[Vec<Point>],
        mut limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        let object = SelectedObject::Segment(segment.id);
        let error = |source| SolidPrepareError::Geometry { object, source };
        cancel.check().map_err(|e| error(e.into()))?;
        if segment.bond_wire.is_some() {
            return Err(error(SolidError::Unsupported("BOND_WIRE_PROFILE")));
        }
        if segment.centreline_bounds().is_none()
            || segment.width <= 0.0
            || !tolerance_mm.is_finite()
            || tolerance_mm <= 0.0
        {
            return Err(error(SolidError::Invalid("TRACE")));
        }
        let max_points = limits
            .max_points
            .min(limits.max_allocation_bytes / (2 * size_of::<Point>()));
        let path = flatten_path(
            std::slice::from_ref(segment),
            tolerance_mm * 0.5,
            max_points,
            cancel,
        )
        .map_err(|source| SolidPrepareError::Curve { object, source })?;
        let mesh = if path.len() == 1 {
            let rings = outline_rings(
                PadOutline::Circle {
                    center: path[0],
                    radius: segment.width * 0.5,
                },
                tolerance_mm,
                limits,
                cancel,
            )
            .map_err(error)?;
            limits.max_allocation_bytes = limits
                .max_allocation_bytes
                .saturating_sub(ring_bytes(&rings))
                .saturating_sub(path.capacity().saturating_mul(size_of::<Point>()));
            SolidMesh::extrude_coverage_minus(&rings, cutouts, z, limits, cancel).map_err(error)?
        } else {
            limits.max_allocation_bytes = limits
                .max_allocation_bytes
                .saturating_sub(path.capacity().saturating_mul(size_of::<Point>()));
            let closed = segment
                .arc
                .is_some_and(|a| a.sweep.abs() >= std::f64::consts::TAU - 1e-12)
                && segment.a.distance(segment.b) <= tolerance_mm * 0.5;
            SolidMesh::extrude_stroke_minus(
                &path,
                segment.width,
                closed,
                z,
                tolerance_mm * 0.5,
                cutouts,
                limits,
                cancel,
            )
            .map_err(error)?
        };
        Ok(Self {
            object,
            layers: MemberLayers::Single(segment.layer),
            net: segment.net,
            mesh,
            placement: PadPlacement::default(),
        })
    }
}
