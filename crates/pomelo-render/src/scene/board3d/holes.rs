//! Pin holes remain physical cutouts. Vias are rendered as solid bodies in the 3D preview.
use super::super::solid::{outline_rings, ring_bytes};
use super::*;
use pomelo_core::model::{Bounds, Pad};
pub(super) struct Hole {
    pad: Pad,
    placement: PadPlacement,
    layers: Option<(LayerId, LayerId)>,
    bounds: Bounds,
}
pub(super) fn count(board: &BoardScene) -> usize {
    board
        .pins
        .iter()
        .filter(|pin| pin.drill_shape.pad().is_some())
        .count()
}
pub(super) fn build(
    board: &BoardScene,
    limits: SolidLimits,
    cancel: &CancellationToken,
) -> Result<Vec<Hole>, SolidError> {
    let count = count(board);
    let bytes = count.saturating_mul(size_of::<Hole>());
    if bytes > limits.max_allocation_bytes {
        return Err(SolidError::Limit {
            resource: "bytes",
            actual: bytes,
            limit: limits.max_allocation_bytes,
        });
    }
    let mut holes = Vec::new();
    holes
        .try_reserve_exact(count)
        .map_err(|_| SolidError::Allocation)?;
    let pins = board.pins.iter().map(|p| {
        (
            &p.drill_shape,
            PadPlacement {
                at: p.at,
                angle: p.angle,
                mirrored: p.mirrored,
            },
            None,
        )
    });
    for (drill, placement, layers) in pins {
        cancel.check()?;
        if let Some(pad) = drill.pad() {
            let bounds = pad
                .bounds(placement)
                .ok_or(SolidError::Invalid("DRILL_BOUNDS"))?;
            holes.push(Hole {
                pad,
                placement,
                layers,
                bounds,
            });
        }
    }
    Ok(holes)
}
pub(super) fn bounds(hole: &Hole) -> Bounds3 {
    Bounds3 {
        min: [hole.bounds.min.x, hole.bounds.min.y, 0.0],
        max: [hole.bounds.max.x, hole.bounds.max.y, 0.0],
    }
}
pub(super) fn cutouts(
    holes: &[Hole],
    index: &spatial::SpatialIndex3d,
    layer: LayerId,
    bounds: Option<Bounds>,
    tolerance: f64,
    limits: SolidLimits,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<Point>>, SolidError> {
    let Some(bounds) = bounds else {
        return Ok(vec![]);
    };
    let mut rings = Vec::new();
    let mut points = 0usize;
    index.visit_xy(bounds, |id| {
        let hole = &holes[id];
        cancel.check()?;
        if hole.bounds.max.x < bounds.min.x
            || hole.bounds.min.x > bounds.max.x
            || hole.bounds.max.y < bounds.min.y
            || hole.bounds.min.y > bounds.max.y
            || hole
                .layers
                .is_some_and(|(a, b)| layer < a.min(b) || layer > a.max(b))
        {
            return Ok(());
        }
        let outline = hole
            .pad
            .outline(hole.placement, tolerance)
            .map_err(|_| SolidError::Invalid("DRILL_OUTLINE"))?
            .ok_or(SolidError::Invalid("DRILL_OUTLINE"))?;
        let remaining = SolidLimits {
            max_points: limits.max_points.saturating_sub(points),
            max_rings: limits.max_rings.saturating_sub(rings.len()),
            max_allocation_bytes: limits
                .max_allocation_bytes
                .saturating_sub(ring_bytes(&rings)),
            ..limits
        };
        let extra = outline_rings(outline, tolerance, remaining, cancel)?;
        points = points.saturating_add(extra.iter().map(Vec::len).sum::<usize>());
        rings
            .try_reserve(extra.len())
            .map_err(|_| SolidError::Allocation)?;
        rings.extend(extra);
        let bytes = ring_bytes(&rings);
        if bytes > limits.max_allocation_bytes {
            return Err(SolidError::Limit {
                resource: "bytes",
                actual: bytes,
                limit: limits.max_allocation_bytes,
            });
        }
        Ok(())
    })?;
    Ok(rings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::model::{DrillShape, ObjectId, Pin};
    #[test]
    fn surface_mount_pins_do_not_consume_drill_index_budget() {
        let pins = (0..2000)
            .map(|id| Pin {
                id: ObjectId(id),
                owner_id: ObjectId(0),
                net: NetId(0),
                name: String::new(),
                reference: String::new(),
                at: Point::default(),
                angle: 0.0,
                mirrored: false,
                drill: 0.0,
                drill_shape: DrillShape {
                    custom: None,
                    width: 0.0,
                    height: 0.0,
                    plated: false,
                    rectangular: false,
                },
                pads: vec![Pad::circle(LayerId(0), 1.0)],
                stackup_region: None,
                die: None,
            })
            .collect();
        let board = BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins,
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::new(1.0, 1.0),
            },
            diagnostics: vec![],
        };
        let holes = build(
            &board,
            SolidLimits {
                max_allocation_bytes: 0,
                ..Default::default()
            },
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(
            holes.capacity(),
            0,
            "SMT pads need no drill metadata allocation"
        );
    }
}
