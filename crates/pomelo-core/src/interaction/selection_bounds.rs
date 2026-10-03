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
        let group_reference = if let Self::ComponentGroup(anchor) = self {
            anchor.reference(scene, cancel)?
        } else {
            None
        };
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
            let selected = match self {
                Self::ComponentGroup(_) => {
                    group_reference.is_some_and(|reference| pin.reference == reference)
                }
                Self::Component(_) => false,
                _ => self.matches_pin(pin, scene),
            };
            if selected {
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
                || group_reference.is_some_and(|reference| {
                    via.finger
                        .as_ref()
                        .is_some_and(|finger| finger.reference == reference)
                })
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
        if let Self::Object(crate::selection::SelectedObject::Drawing(id)) = self
            && let Some(drawing) = scene.drawings.iter().find(|d| d.id == id)
        {
            for segment in &drawing.segments {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                include(&mut result, segment.bounds(), id)?;
            }
            for text in scene
                .texts
                .iter()
                .filter(|t| drawing.text_ids.contains(&t.id))
            {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                let rows: Vec<_> = text
                    .text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\t', "    ")
                    .split('\n')
                    .map(str::to_owned)
                    .collect();
                for (row, line) in rows.iter().enumerate() {
                    let count = line.chars().count();
                    if count == 0 {
                        continue;
                    }
                    let width =
                        count as f64 * text.width + count.saturating_sub(1) as f64 * text.spacing;
                    let left = match text.align {
                        crate::model::TextAlignment::Left => 0.0,
                        crate::model::TextAlignment::Center => -width * 0.5,
                        crate::model::TextAlignment::Right => -width,
                    };
                    for (x, y) in [
                        (left, 0.0),
                        (left + width, 0.0),
                        (left + width, text.height),
                        (left, text.height),
                    ] {
                        let p = crate::model::Point::new(
                            if text.mirrored { -x } else { x },
                            y - row as f64 * text.line_spacing,
                        )
                        .rotate(text.angle);
                        include(
                            &mut result,
                            Bounds::from_points([crate::model::Point::new(
                                text.at.x + p.x,
                                text.at.y + p.y,
                            )]),
                            id,
                        )?;
                    }
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
