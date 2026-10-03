//! Navigation bounds use the same geometry as Web BoardIndex entries.
use super::*;
use crate::{pad::PadPlacement, selection::SelectionTarget};

fn include(
    result: &mut Option<Bounds>,
    value: Option<Bounds>,
    id: crate::model::ObjectId,
) -> Result<(), PathError> {
    let bounds = value
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

impl SegmentIndex {
    /// Bounds of all indexed selection geometry, including hidden entries.
    /// No placement origin or approximate text rectangle is added. Run off the
    /// UI thread; cancellation prevents publishing a partial result.
    pub fn selection_bounds(
        &self,
        target: SelectionTarget,
        cancel: &CancellationToken,
    ) -> Result<Option<Bounds>, PathError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        let scene = &self.scene;
        let target = target.canonical_reference(scene, cancel)?;
        let reference = if let SelectionTarget::ComponentGroup(anchor) = target {
            anchor.reference(scene, cancel)?
        } else {
            None
        };
        let mut result = None;
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if target.matches_segment(segment) {
                include(&mut result, segment.bounds(), segment.id)?;
            }
        }
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let selected = if let SelectionTarget::ComponentGroup(_) = target {
                reference.is_some_and(|reference| pin.reference == reference)
            } else {
                target.matches_pin(pin, scene)
            };
            if !selected {
                continue;
            }
            let owner = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            for pad in &pin.pads {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if pad.supported() && !pad.backdrill {
                    include(&mut result, pad.bounds(owner), pin.id)?;
                }
            }
            if let Some(pad) = pin.drill_shape.pad() {
                include(&mut result, pad.bounds(owner), pin.id)?;
            }
        }
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !target.matches_via(via)
                && !reference.is_some_and(|reference| {
                    via.finger
                        .as_ref()
                        .is_some_and(|finger| finger.reference == reference)
                })
            {
                continue;
            }
            let owner = PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            for pad in via.pads.iter() {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if pad.supported() && !pad.backdrill {
                    include(&mut result, pad.bounds(owner), via.id)?;
                }
            }
            if let Some(pad) = via.drill_shape.pad() {
                include(&mut result, pad.bounds(owner), via.id)?;
            }
            if via.backdrill.is_some()
                && let Some(pad) = via.pads.iter().find(|pad| pad.backdrill)
            {
                include(&mut result, pad.bounds(owner), via.id)?;
            }
        }
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !target.matches_zone(zone) {
                continue;
            }
            let outer = zone.paths.first().filter(|path| !path.is_empty());
            if let Some(outer) = outer {
                for segment in outer {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    include(&mut result, segment.centreline_bounds(), zone.id)?;
                }
            } else if let Some(bounds) = zone.mesh.ring_bounds.first() {
                include(&mut result, Some(*bounds), zone.id)?;
            }
        }
        let mut drawings = std::collections::BTreeSet::new();
        for drawing in &scene.drawings {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if target == SelectionTarget::Object(SelectedObject::Drawing(drawing.id))
                || matches!(target, SelectionTarget::Net(net) if net.0 != 0 && net == drawing.net)
            {
                drawings.insert(drawing.id);
            }
        }
        if !drawings.is_empty() {
            for entry in &self.drawings.entries {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if drawings.contains(&entry.owner) {
                    include(&mut result, Some(entry.bounds), entry.owner)?;
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
