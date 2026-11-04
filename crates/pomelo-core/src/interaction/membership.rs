//! The single definition of which scene objects belong to a selection target.
//! Member pages, summaries, pin overlays, locate bounds and navigation anchors all
//! use it, so a membership rule changes in exactly one place.

use std::collections::BTreeSet;

use crate::{
    geometry::PathError,
    model::{BoardDrawing, BoardScene, ObjectId, Pin, Segment, Via, Zone},
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};

/// Membership tests for one target, resolved once: a component group's reference
/// and a placement's pin list are looked up up front, so each test is cheap.
/// Hidden layers never change membership; NetId(0) never forms a network.
pub struct Membership<'a> {
    target: SelectionTarget,
    group_reference: Option<&'a str>,
    component_pins: Option<BTreeSet<ObjectId>>,
}

impl<'a> Membership<'a> {
    /// A placement listing the same pin twice is reported as `Invalid(pin)`.
    pub fn new(
        target: SelectionTarget,
        scene: &'a BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Self, PathError> {
        cancel.check()?;
        let group_reference = match target {
            SelectionTarget::ComponentGroup(anchor) => anchor.reference(scene, cancel)?,
            _ => None,
        };
        let component_pins = match target {
            SelectionTarget::Component(id)
            | SelectionTarget::Object(SelectedObject::Placement(id)) => {
                Some(component_pins(scene, id, cancel)?)
            }
            _ => None,
        };
        Ok(Self {
            target,
            group_reference,
            component_pins,
        })
    }

    pub fn segment(&self, segment: &Segment) -> bool {
        match self.target {
            SelectionTarget::Object(SelectedObject::Segment(id)) => id == segment.id,
            SelectionTarget::Track(id) => id == segment.track_id,
            SelectionTarget::Net(net) => net.0 != 0 && net == segment.net,
            _ => false,
        }
    }

    pub fn pin(&self, pin: &Pin) -> bool {
        match self.target {
            SelectionTarget::Object(SelectedObject::Pin(id)) => id == pin.id,
            SelectionTarget::Net(net) => net.0 != 0 && net == pin.net,
            SelectionTarget::Component(_)
            | SelectionTarget::Object(SelectedObject::Placement(_)) => self
                .component_pins
                .as_ref()
                .is_some_and(|pins| pins.contains(&pin.id)),
            SelectionTarget::ComponentGroup(_) => self.group_reference == Some(&pin.reference),
            _ => false,
        }
    }

    /// Component groups include bond fingers that carry the group's reference.
    pub fn via(&self, via: &Via) -> bool {
        match self.target {
            SelectionTarget::Object(SelectedObject::Via(id)) => id == via.id,
            SelectionTarget::Net(net) => net.0 != 0 && net == via.net,
            SelectionTarget::ComponentGroup(_) => self.group_reference.is_some_and(|reference| {
                via.finger
                    .as_ref()
                    .is_some_and(|finger| finger.reference == reference)
            }),
            _ => false,
        }
    }

    pub fn zone(&self, zone: &Zone) -> bool {
        match self.target {
            SelectionTarget::Object(SelectedObject::Zone(id)) => id == zone.id,
            SelectionTarget::Net(net) => net.0 != 0 && net == zone.net,
            _ => false,
        }
    }

    /// Net drawings are members of their net, as they are located and highlighted with it.
    pub fn drawing(&self, drawing: &BoardDrawing) -> bool {
        match self.target {
            SelectionTarget::Object(SelectedObject::Drawing(id)) => id == drawing.id,
            SelectionTarget::Net(net) => net.0 != 0 && net == drawing.net,
            _ => false,
        }
    }
}

/// One member object, borrowed from the scene.
#[derive(Debug, Clone, Copy)]
pub enum Member<'a> {
    Segment(&'a Segment),
    Pin(&'a Pin),
    Via(&'a Via),
    Zone(&'a Zone),
    Drawing(&'a BoardDrawing),
}

impl Member<'_> {
    pub fn object(self) -> SelectedObject {
        match self {
            Self::Segment(segment) => SelectedObject::Segment(segment.id),
            Self::Pin(pin) => SelectedObject::Pin(pin.id),
            Self::Via(via) => SelectedObject::Via(via.id),
            Self::Zone(zone) => SelectedObject::Zone(zone.id),
            Self::Drawing(drawing) => SelectedObject::Drawing(drawing.id),
        }
    }
}

impl SelectionTarget {
    /// Visit every member in source order: segments, pins, vias, zones, drawings.
    /// A source placement must own each listed pin and every listed pin must exist;
    /// either inconsistency is `Invalid(pin)`. Cancellation discards the visit.
    pub fn visit_members<'a>(
        self,
        scene: &'a BoardScene,
        cancel: &CancellationToken,
        mut visit: impl FnMut(Member<'a>) -> Result<(), PathError>,
    ) -> Result<(), PathError> {
        let membership = Membership::new(self, scene, cancel)?;
        for segment in &scene.segments {
            cancel.check()?;
            if membership.segment(segment) {
                visit(Member::Segment(segment))?;
            }
        }
        // A placement's listed pins are each claimed once, by their first occurrence.
        let mut unseen_pins = membership.component_pins.clone();
        for pin in &scene.pins {
            cancel.check()?;
            let selected = match unseen_pins.as_mut() {
                Some(unseen) => unseen.remove(&pin.id),
                None => membership.pin(pin),
            };
            if !selected {
                continue;
            }
            if let Self::Component(owner) | Self::Object(SelectedObject::Placement(owner)) = self
                && pin.owner_id != owner
            {
                return Err(PathError::Invalid(pin.id));
            }
            visit(Member::Pin(pin))?;
        }
        if let Some(&missing) = unseen_pins.as_ref().and_then(BTreeSet::first) {
            return Err(PathError::Invalid(missing));
        }
        for via in &scene.vias {
            cancel.check()?;
            if membership.via(via) {
                visit(Member::Via(via))?;
            }
        }
        for zone in &scene.zones {
            cancel.check()?;
            if membership.zone(zone) {
                visit(Member::Zone(zone))?;
            }
        }
        for drawing in &scene.drawings {
            cancel.check()?;
            if membership.drawing(drawing) {
                visit(Member::Drawing(drawing))?;
            }
        }
        cancel.check()?;
        Ok(())
    }
}

/// Pins listed by source placement `id`, unvalidated; empty when it does not exist.
pub(crate) fn listed_pins(scene: &BoardScene, id: ObjectId) -> &[ObjectId] {
    scene
        .components
        .iter()
        .find(|component| component.id == id)
        .map_or(&[], |component| component.pins.as_slice())
}

fn component_pins(
    scene: &BoardScene,
    id: ObjectId,
    cancel: &CancellationToken,
) -> Result<BTreeSet<ObjectId>, PathError> {
    let mut pins = BTreeSet::new();
    for &pin in listed_pins(scene, id) {
        cancel.check()?;
        if !pins.insert(pin) {
            return Err(PathError::Invalid(pin));
        }
    }
    Ok(pins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, LayerId, NetId, Point};

    fn empty_scene() -> BoardScene {
        BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::default(),
            },
            diagnostics: vec![],
        }
    }

    fn includes_segment(target: SelectionTarget, segment: &Segment) -> bool {
        let scene = empty_scene();
        Membership::new(target, &scene, &CancellationToken::default())
            .unwrap()
            .segment(segment)
    }

    #[test]
    fn object_category_and_track_identity_do_not_alias_net_or_segment_identity() {
        let segment = Segment {
            id: ObjectId(7),
            track_id: ObjectId(8),
            layer: LayerId(1),
            net: NetId(9),
            a: Point::default(),
            b: Point::new(1.0, 0.0),
            width: 0.1,
            arc: None,
            bond_wire: None,
        };
        let object = |object| SelectionTarget::Object(object);
        assert!(includes_segment(
            object(SelectedObject::Segment(ObjectId(7))),
            &segment
        ));
        assert!(!includes_segment(
            object(SelectedObject::Via(ObjectId(7))),
            &segment
        ));
        assert!(includes_segment(
            SelectionTarget::Track(ObjectId(8)),
            &segment
        ));
        assert!(!includes_segment(
            SelectionTarget::Track(ObjectId(7)),
            &segment
        ));
        assert!(includes_segment(SelectionTarget::Net(NetId(9)), &segment));
        let unconnected = Segment {
            net: NetId(0),
            ..segment
        };
        assert!(!includes_segment(
            SelectionTarget::Net(NetId(0)),
            &unconnected
        ));
    }
}
