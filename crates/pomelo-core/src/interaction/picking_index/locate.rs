//! Navigation bounds use the same geometry as Web BoardIndex entries.
use super::*;
use crate::{interaction::membership::Member, pad::PadPlacement, selection::SelectionTarget};

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

impl BoardPickingIndex {
    /// Bounds of all indexed selection geometry, including hidden entries.
    /// No placement origin or approximate text rectangle is added. Run off the
    /// UI thread; cancellation prevents publishing a partial result.
    pub fn selection_bounds(
        &self,
        target: SelectionTarget,
        cancel: &CancellationToken,
    ) -> Result<Option<Bounds>, PathError> {
        cancel.check()?;
        let scene = &self.scene;
        let target = target.canonical_reference(scene, cancel)?;
        let mut result = None;
        let mut drawings = std::collections::BTreeSet::new();
        let pads = |result: &mut Option<Bounds>,
                    id,
                    owner: PadPlacement,
                    pads: &[crate::model::Pad],
                    drill: Option<crate::model::Pad>|
         -> Result<(), PathError> {
            for pad in pads {
                cancel.check()?;
                if pad.supported() && !pad.backdrill {
                    include(result, pad.bounds(owner), id)?;
                }
            }
            if let Some(pad) = drill {
                include(result, pad.bounds(owner), id)?;
            }
            Ok(())
        };
        target.visit_members(scene, cancel, |member| match member {
            Member::Segment(segment) => include(&mut result, segment.bounds(), segment.id),
            Member::Pin(pin) => pads(
                &mut result,
                pin.id,
                PadPlacement {
                    at: pin.at,
                    angle: pin.angle,
                    mirrored: pin.mirrored,
                },
                &pin.pads,
                pin.drill_shape.pad(),
            ),
            Member::Via(via) => {
                let owner = PadPlacement {
                    at: via.at,
                    angle: via.angle,
                    mirrored: via.mirrored,
                };
                pads(&mut result, via.id, owner, &via.pads, via.drill_shape.pad())?;
                // The backdrill display pad frames the drilled-out region.
                if via.backdrill.is_some()
                    && let Some(pad) = via.pads.iter().find(|pad| pad.backdrill)
                {
                    include(&mut result, pad.bounds(owner), via.id)?;
                }
                Ok(())
            }
            Member::Zone(zone) => {
                match zone.paths.first().filter(|path| !path.is_empty()) {
                    Some(outer) => {
                        for segment in outer {
                            cancel.check()?;
                            include(&mut result, segment.centreline_bounds(), zone.id)?;
                        }
                    }
                    None => {
                        if let Some(bounds) = zone.mesh.ring_bounds.first() {
                            include(&mut result, Some(*bounds), zone.id)?;
                        }
                    }
                }
                Ok(())
            }
            Member::Drawing(drawing) => {
                drawings.insert(drawing.id);
                Ok(())
            }
        })?;
        // Drawing extents come from indexed strokes and glyph quads, not placements.
        if !drawings.is_empty() {
            for entry in &self.drawings.entries {
                cancel.check()?;
                if drawings.contains(&entry.owner) {
                    include(&mut result, Some(entry.bounds), entry.owner)?;
                }
            }
        }
        let placements = target.placement_ids(scene, cancel)?;
        for component in &scene.components {
            cancel.check()?;
            if placements.contains(&component.id)
                && let Some(body) = &component.body
            {
                include(&mut result, super::placements::bounds(body), component.id)?;
            }
        }
        cancel.check()?;
        Ok(result)
    }
}
