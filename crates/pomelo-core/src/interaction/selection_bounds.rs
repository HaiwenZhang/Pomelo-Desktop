//! Source-space bounds for inspector object navigation, independent of display state.
use crate::{
    geometry::PathError,
    model::{BoardScene, Bounds, ObjectId},
    pad::PadPlacement,
    selection::SelectionTarget,
    task::CancellationToken,
};

fn include(
    result: &mut Option<Bounds>,
    bounds: Option<Bounds>,
    id: ObjectId,
) -> Result<(), PathError> {
    let bounds = bounds
        .filter(|bounds| bounds.is_valid())
        .ok_or(PathError::Invalid(id))?;
    if let Some(result) = result {
        result.include(bounds.min);
        result.include(bounds.max);
    } else {
        *result = Some(bounds);
    }
    Ok(())
}

impl SelectionTarget {
    /// Run in a background task. No hidden-layer or camera state changes source bounds.
    pub fn bounds(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Option<Bounds>, PathError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        let (mut result, related) = match self {
            Self::Net(id) => return crate::search::SearchTarget::Net(id).bounds(scene, cancel),
            Self::Component(id) => (
                crate::search::SearchTarget::Component(id).bounds(scene, cancel)?,
                self.related_bond_objects(scene, cancel)?
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>(),
            ),
            _ => (None, std::collections::BTreeSet::new()),
        };
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.matches_segment(segment)
                || related.contains(&crate::selection::SelectedObject::Segment(segment.id))
            {
                crate::picking::segment_distance_mm(segment, segment.a)?;
                include(&mut result, segment.bounds(), segment.id)?;
            }
        }
        let mut pads = |id,
                        owner: PadPlacement,
                        pads: &[crate::model::Pad],
                        drill: crate::model::DrillShape|
         -> Result<(), PathError> {
            include(&mut result, Bounds::from_points([owner.at]), id)?;
            for pad in pads {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                include(&mut result, pad.bounds(owner), id)?;
            }
            if let Some(pad) = drill.pad() {
                include(&mut result, pad.bounds(owner), id)?;
            }
            Ok(())
        };
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !matches!(self, Self::Component(_)) && self.matches_pin(pin, scene) {
                pads(
                    pin.id,
                    PadPlacement {
                        at: pin.at,
                        angle: pin.angle,
                        mirrored: pin.mirrored,
                    },
                    &pin.pads,
                    pin.drill_shape,
                )?;
            }
        }
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.matches_via(via)
                || related.contains(&crate::selection::SelectedObject::Via(via.id))
            {
                pads(
                    via.id,
                    PadPlacement {
                        at: via.at,
                        angle: via.angle,
                        mirrored: via.mirrored,
                    },
                    &via.pads,
                    via.drill_shape,
                )?;
            }
        }
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !self.matches_zone(zone) {
                continue;
            }
            for &point in &zone.mesh.vertices {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                include(&mut result, Bounds::from_points([point]), zone.id)?;
            }
            for path in &zone.paths {
                for segment in path {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    include(&mut result, segment.centreline_bounds(), zone.id)?;
                }
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(result)
        }
    }
}
