//! Domain selection predicates shared by picking, search and GPU overlays.

use crate::interaction::component_reference::ComponentAnchor;
use crate::interaction::membership::{Member, listed_pins};
use crate::model::{BoardScene, LayerId, NetId, ObjectId};
use crate::{geometry::PathError, task::CancellationToken};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone)]
pub struct SelectionSummary {
    pub segments: usize,
    pub pins: usize,
    pub vias: usize,
    pub zones: usize,
    pub drawings: usize,
    /// Sum of selected 2D centerlines; excludes drill depth and electrical delay.
    pub centerline_length_mm: f64,
    /// Copper layers carrying member traces, zones, pads and via end layers.
    pub layers: BTreeSet<LayerId>,
    /// Footprint placements owning member pins.
    pub components: BTreeSet<ObjectId>,
}

#[derive(Debug)]
pub struct SelectionMembers {
    /// Source order within segment, pin, via, zone categories; bounded to 256 rows.
    pub objects: Vec<SelectedObject>,
    /// Layers occupied by each listed object, in the same order as `objects`.
    pub layers: Vec<MemberLayers>,
    pub total: usize,
}

/// Layers one member occupies: a single layer, or the outer layers of a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberLayers {
    None,
    Single(LayerId),
    Span(LayerId, LayerId),
}

impl MemberLayers {
    fn between(first: Option<LayerId>, last: Option<LayerId>) -> Self {
        match (first, last) {
            (Some(first), Some(last)) if first != last => Self::Span(first, last),
            (Some(layer), _) | (None, Some(layer)) => Self::Single(layer),
            (None, None) => Self::None,
        }
    }

    fn of(member: Member<'_>) -> Self {
        match member {
            Member::Segment(segment) => Self::Single(segment.layer),
            Member::Zone(zone) => Self::Single(zone.layer),
            Member::Drawing(drawing) => Self::Single(drawing.layer),
            Member::Via(via) => Self::between(via.start_layer, via.end_layer),
            Member::Pin(pin) => Self::between(
                pin.pads.first().map(|pad| pad.layer),
                pin.pads.last().map(|pad| pad.layer),
            ),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SelectedObject {
    Segment(ObjectId),
    Pin(ObjectId),
    Via(ObjectId),
    Zone(ObjectId),
    Drawing(ObjectId),
    /// Visible placement area; its semantic selection is the owning component.
    Placement(ObjectId),
}

impl SelectedObject {
    /// Resolve source identity before applying the user's selection mode.
    /// Unsupported relationships and unconnected nets return no target.
    pub fn resolve(
        self,
        scene: &BoardScene,
        mode: crate::interaction::SelectionMode,
        cancel: &CancellationToken,
    ) -> Result<Option<SelectionTarget>, PathError> {
        use crate::interaction::SelectionMode;
        fn find<'a, T>(
            values: &'a [T],
            id: ObjectId,
            key: impl Fn(&T) -> ObjectId,
            cancel: &CancellationToken,
        ) -> Result<&'a T, PathError> {
            for value in values {
                cancel.check()?;
                if key(value) == id {
                    return Ok(value);
                }
            }
            cancel.check()?;
            Err(PathError::Invalid(id))
        }
        let (net, track, pin) = match self {
            Self::Placement(id) => {
                find(&scene.components, id, |component| component.id, cancel)?;
                return Ok(Some(SelectionTarget::Component(id)));
            }
            Self::Drawing(id) => {
                find(&scene.drawings, id, |value| value.id, cancel)?;
                (NetId(0), None, None)
            }
            Self::Segment(id) => {
                let value = find(&scene.segments, id, |value| value.id, cancel)?;
                (
                    value.net,
                    Some(value.track_id),
                    value.bond_wire.as_ref().map(|wire| wire.source_pin),
                )
            }
            Self::Pin(id) => {
                let value = find(&scene.pins, id, |value| value.id, cancel)?;
                (value.net, None, Some(value.id))
            }
            Self::Via(id) => {
                let value = find(&scene.vias, id, |value| value.id, cancel)?;
                (
                    value.net,
                    None,
                    value.finger.as_ref().and_then(|finger| finger.source_pin),
                )
            }
            Self::Zone(id) => {
                let value = find(&scene.zones, id, |value| value.id, cancel)?;
                (value.net, None, None)
            }
        };
        let result = match mode {
            SelectionMode::Object => Some(SelectionTarget::Object(self)),
            SelectionMode::Track => track.map(SelectionTarget::Track),
            SelectionMode::Net => (net.0 != 0).then_some(SelectionTarget::Net(net)),
            SelectionMode::Component => {
                if let Some(id) = pin {
                    let pin = find(&scene.pins, id, |value| value.id, cancel)?;
                    let component =
                        find(&scene.components, pin.owner_id, |value| value.id, cancel)?;
                    let mut member = false;
                    for &id in &component.pins {
                        cancel.check()?;
                        if id == pin.id {
                            member = true;
                            break;
                        }
                    }
                    if !member {
                        return Err(PathError::Invalid(pin.id));
                    }
                    Some(SelectionTarget::Component(component.id))
                } else {
                    None
                }
            }
        };
        cancel.check()?;
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionTarget {
    Object(SelectedObject),
    Track(ObjectId),
    Net(NetId),
    Component(ObjectId),
    /// UI reference grouping; original footprint placement IDs remain independent.
    ComponentGroup(ComponentAnchor),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionCandidate {
    pub target: SelectionTarget,
    /// First source hit explaining why this target entered the candidate list.
    pub object: SelectedObject,
    pub distance_mm: f64,
}

/// Resolve already ordered hits into distinct mode targets, preserving first-hit order.
/// Net zero and unsupported relationships are omitted before the target limit.
/// Source errors and cancellation discard the entire candidate list.
pub fn resolve_candidates(
    scene: &BoardScene,
    hits: &[crate::picking::ObjectHit],
    mode: crate::interaction::SelectionMode,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<SelectionCandidate>, PathError> {
    cancel.check()?;
    let limit = limit.min(64);
    let mut candidates: Vec<SelectionCandidate> = Vec::new();
    if limit == 0 {
        return Ok(candidates);
    }
    for hit in hits {
        cancel.check()?;
        if !hit.distance_mm.is_finite() || hit.distance_mm < 0.0 {
            let id = match hit.object {
                SelectedObject::Segment(id)
                | SelectedObject::Pin(id)
                | SelectedObject::Via(id)
                | SelectedObject::Zone(id) => id,
                SelectedObject::Drawing(id) | SelectedObject::Placement(id) => id,
            };
            return Err(PathError::Invalid(id));
        }
        let Some(target) = hit.object.resolve(scene, mode, cancel)? else {
            continue;
        };
        if candidates.len() < limit
            && !candidates
                .iter()
                .any(|candidate| candidate.target == target)
        {
            candidates.push(SelectionCandidate {
                target,
                object: hit.object,
                distance_mm: hit.distance_mm,
            });
        }
    }
    cancel.check()?;
    Ok(candidates)
}

impl From<crate::search::SearchTarget> for SelectionTarget {
    fn from(value: crate::search::SearchTarget) -> Self {
        match value {
            crate::search::SearchTarget::Net(id) => Self::Net(id),
            crate::search::SearchTarget::Component(id) => Self::Component(id),
            crate::search::SearchTarget::ComponentGroup(anchor) => Self::ComponentGroup(anchor),
        }
    }
}

/// Canvas mode expansion follows the Web viewer: unsupported track/component
/// relationships and unconnected nets retain the picked object as the anchor.
/// The selection target a canvas hit expands to in `mode`; also the grouping key that
/// `BoardPickingIndex::query_visible_hits_grouped` uses before applying its hit limit.
pub fn canvas_target(
    scene: &BoardScene,
    object: SelectedObject,
    mode: crate::interaction::SelectionMode,
    cancel: &CancellationToken,
) -> Result<SelectionTarget, PathError> {
    let target = if mode == crate::interaction::SelectionMode::Component {
        ComponentAnchor::from_object(object, scene, cancel)?
            .map(SelectionTarget::ComponentGroup)
            .unwrap_or(match object {
                SelectedObject::Placement(id) => SelectionTarget::Component(id),
                _ => SelectionTarget::Object(object),
            })
    } else {
        object
            .resolve(scene, mode, cancel)?
            .unwrap_or(SelectionTarget::Object(object))
    };
    if let SelectionTarget::Component(id) = target
        && !matches!(object, SelectedObject::Placement(_))
        && scene
            .components
            .iter()
            .any(|component| component.id == id && component.reference.is_empty())
    {
        return Ok(SelectionTarget::Object(object));
    }
    Ok(target)
}

pub fn resolve_canvas_candidates(
    scene: &BoardScene,
    hits: &[crate::picking::ObjectHit],
    mode: crate::interaction::SelectionMode,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<SelectionCandidate>, PathError> {
    let mut candidates = Vec::new();
    for hit in hits {
        cancel.check()?;
        if candidates.len() >= limit.min(64) {
            break;
        }
        let target = canvas_target(scene, hit.object, mode, cancel)?;
        if !candidates
            .iter()
            .any(|candidate: &SelectionCandidate| candidate.target == target)
        {
            candidates.push(SelectionCandidate {
                target,
                object: hit.object,
                distance_mm: hit.distance_mm,
            });
        }
    }
    cancel.check()?;
    Ok(candidates)
}

impl SelectionTarget {
    /// Upgrade a retained placement selection to the current interactive reference
    /// group without changing source placement IDs or the persisted file format.
    pub fn canonical_reference(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Self, PathError> {
        cancel.check()?;
        let anchor = match self {
            Self::ComponentGroup(anchor) => {
                ComponentAnchor::from_object(anchor.object(), scene, cancel)?
            }
            Self::Component(id) | Self::Object(SelectedObject::Placement(id)) => {
                let mut reference = None;
                for component in &scene.components {
                    cancel.check()?;
                    if component.id == id {
                        reference = Some(component.reference.as_str());
                        break;
                    }
                }
                match reference {
                    Some(reference) => ComponentAnchor::for_reference(reference, scene, cancel)?,
                    None => None,
                }
            }
            _ => None,
        };
        Ok(anchor.map(Self::ComponentGroup).unwrap_or(match self {
            Self::Object(SelectedObject::Placement(id)) => Self::Component(id),
            _ => self,
        }))
    }

    /// Full pin selection for GPU overlays; paging never limits highlighted pins.
    /// A placement's listed pins are returned as listed, without membership checks.
    pub fn pin_ids(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<ObjectId>, PathError> {
        let mut pins = BTreeSet::new();
        match self {
            Self::Component(id) | Self::Object(SelectedObject::Placement(id)) => {
                pins.extend(listed_pins(scene, id))
            }
            Self::Object(SelectedObject::Pin(id)) => {
                pins.insert(id);
            }
            Self::ComponentGroup(_) => self.visit_members(scene, cancel, |member| {
                if let Member::Pin(pin) = member {
                    pins.insert(pin.id);
                }
                Ok(())
            })?,
            _ => {}
        }
        cancel.check()?;
        Ok(pins)
    }

    /// Full reference group for hover. Legacy placement links remain available.
    pub fn component_objects(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<SelectedObject>, PathError> {
        let mut objects = BTreeSet::new();
        match self {
            Self::ComponentGroup(_) => self.visit_members(scene, cancel, |member| {
                objects.insert(member.object());
                Ok(())
            })?,
            Self::Component(_) | Self::Object(SelectedObject::Placement(_)) => {
                objects.extend(
                    self.pin_ids(scene, cancel)?
                        .into_iter()
                        .map(SelectedObject::Pin),
                );
                objects.extend(self.related_bond_objects(scene, cancel)?);
            }
            _ => {}
        }
        objects.extend(
            self.placement_ids(scene, cancel)?
                .into_iter()
                .map(SelectedObject::Placement),
        );
        cancel.check()?;
        Ok(objects)
    }

    /// Actual placement identities belonging to a component/reference selection.
    pub fn placement_ids(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<ObjectId>, PathError> {
        let reference = match self {
            Self::ComponentGroup(anchor) => anchor.reference(scene, cancel)?,
            _ => None,
        };
        let mut result = BTreeSet::new();
        for component in &scene.components {
            cancel.check()?;
            let selected = match self {
                Self::Component(id) | Self::Object(SelectedObject::Placement(id)) => {
                    id == component.id
                }
                Self::ComponentGroup(_) => reference == Some(component.reference.as_str()),
                _ => false,
            };
            if selected && component.body.is_some() {
                result.insert(component.id);
            }
        }
        Ok(result)
    }

    /// Revalidate persisted typed identity before restoring selection work.
    pub fn exists(self, scene: &BoardScene, cancel: &CancellationToken) -> Result<bool, PathError> {
        fn contains(
            ids: impl Iterator<Item = ObjectId>,
            expected: ObjectId,
            cancel: &CancellationToken,
        ) -> Result<bool, PathError> {
            for id in ids {
                cancel.check()?;
                if id == expected {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        cancel.check()?;
        let exists = match self {
            Self::Object(SelectedObject::Drawing(id)) => {
                contains(scene.drawings.iter().map(|item| item.id), id, cancel)?
            }
            Self::Object(SelectedObject::Segment(id)) => {
                contains(scene.segments.iter().map(|item| item.id), id, cancel)?
            }
            Self::Object(SelectedObject::Pin(id)) => {
                contains(scene.pins.iter().map(|item| item.id), id, cancel)?
            }
            Self::Object(SelectedObject::Via(id)) => {
                contains(scene.vias.iter().map(|item| item.id), id, cancel)?
            }
            Self::Object(SelectedObject::Zone(id)) => {
                contains(scene.zones.iter().map(|item| item.id), id, cancel)?
            }
            Self::Track(id) => {
                contains(scene.segments.iter().map(|item| item.track_id), id, cancel)?
            }
            Self::Component(id) | Self::Object(SelectedObject::Placement(id)) => {
                contains(scene.components.iter().map(|item| item.id), id, cancel)?
            }
            Self::ComponentGroup(anchor) => anchor.reference(scene, cancel)?.is_some(),
            Self::Net(id) => scene.nets.contains_key(&id),
        };
        cancel.check()?;
        Ok(exists)
    }

    /// Reference groups return their fingers; source placements use explicit
    /// bond-wire/finger links through their source pins.
    /// This does not infer physical connectivity or add ordinary net members.
    pub fn related_bond_objects(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Vec<SelectedObject>, PathError> {
        let mut objects = Vec::new();
        match self {
            Self::ComponentGroup(_) => self.visit_members(scene, cancel, |member| {
                if let Member::Via(via) = member {
                    objects.push(SelectedObject::Via(via.id));
                }
                Ok(())
            })?,
            Self::Component(id) => {
                // Validate the placement's pin list before following its links.
                self.visit_members(scene, cancel, |_| Ok(()))?;
                let pins: BTreeSet<_> = listed_pins(scene, id).iter().copied().collect();
                for segment in &scene.segments {
                    cancel.check()?;
                    if segment
                        .bond_wire
                        .as_ref()
                        .is_some_and(|wire| pins.contains(&wire.source_pin))
                    {
                        objects.push(SelectedObject::Segment(segment.id));
                    }
                }
                for via in &scene.vias {
                    cancel.check()?;
                    if via
                        .finger
                        .as_ref()
                        .and_then(|finger| finger.source_pin)
                        .is_some_and(|pin| pins.contains(&pin))
                    {
                        objects.push(SelectedObject::Via(via.id));
                    }
                }
            }
            _ => self.visit_members(scene, cancel, |_| Ok(()))?,
        }
        cancel.check()?;
        Ok(objects)
    }

    /// Source membership, not proof of electrical continuity or physical contact.
    /// Hidden layers do not alter membership. Cancellation discards partial rows.
    pub fn members(
        self,
        scene: &BoardScene,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<SelectionMembers, PathError> {
        self.members_page(scene, 0, limit, cancel)
    }

    /// Returns a source-order page, retaining at most 256 objects.
    /// `offset` counts matching members, not scene objects. Total and relation
    /// validation cover the complete selection, including rows outside the page.
    pub fn members_page(
        self,
        scene: &BoardScene,
        offset: usize,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<SelectionMembers, PathError> {
        let limit = limit.min(256);
        let mut result = SelectionMembers {
            objects: Vec::new(),
            layers: Vec::new(),
            total: 0,
        };
        self.visit_members(scene, cancel, |member| {
            if result.total >= offset && result.objects.len() < limit {
                result.objects.push(member.object());
                result.layers.push(MemberLayers::of(member));
            }
            result.total += 1;
            Ok(())
        })?;
        Ok(result)
    }

    pub fn summarize(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<SelectionSummary, PathError> {
        let mut summary = SelectionSummary::default();
        // Kahan compensation keeps long networks' total length stable.
        let mut compensation = 0.0;
        self.visit_members(scene, cancel, |member| {
            match member {
                Member::Segment(segment) => {
                    let length = segment.length_mm();
                    if !length.is_finite() || length < 0.0 {
                        return Err(PathError::Invalid(segment.id));
                    }
                    let adjusted = length - compensation;
                    let next = summary.centerline_length_mm + adjusted;
                    if !next.is_finite() {
                        return Err(PathError::Invalid(segment.id));
                    }
                    compensation = (next - summary.centerline_length_mm) - adjusted;
                    summary.centerline_length_mm = next;
                    summary.segments += 1;
                    summary.layers.insert(segment.layer);
                }
                Member::Pin(pin) => {
                    summary.pins += 1;
                    summary.components.insert(pin.owner_id);
                    summary.layers.extend(pin.pads.iter().map(|pad| pad.layer));
                }
                Member::Via(via) => {
                    summary.vias += 1;
                    summary.layers.extend(via.start_layer);
                    summary.layers.extend(via.end_layer);
                }
                Member::Zone(zone) => {
                    summary.zones += 1;
                    summary.layers.insert(zone.layer);
                }
                Member::Drawing(_) => summary.drawings += 1,
            }
            Ok(())
        })?;
        for id in self.placement_ids(scene, cancel)? {
            summary.components.insert(id);
            if let Some(body) = scene
                .components
                .iter()
                .find(|component| component.id == id)
                .and_then(|component| component.body.as_ref())
            {
                summary.layers.insert(body.layer);
            }
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, Point, Segment};

    #[test]
    fn persisted_selection_checks_category_identity_and_cancellation() {
        let scene = BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: std::collections::BTreeMap::from([(NetId(9), "source net".into())]),
            segments: vec![Segment {
                id: ObjectId(7),
                track_id: ObjectId(8),
                layer: LayerId(1),
                net: NetId(9),
                a: Point::default(),
                b: Point::new(1.0, 0.0),
                width: 0.1,
                arc: None,
                bond_wire: None,
            }],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            diagnostics: vec![],
            bounds: crate::model::Bounds {
                min: Point::default(),
                max: Point::new(1.0, 1.0),
            },
        };
        let cancel = CancellationToken::default();
        assert!(
            SelectionTarget::Object(SelectedObject::Segment(ObjectId(7)))
                .exists(&scene, &cancel)
                .unwrap()
        );
        assert!(
            !SelectionTarget::Object(SelectedObject::Pin(ObjectId(7)))
                .exists(&scene, &cancel)
                .unwrap()
        );
        assert!(
            SelectionTarget::Track(ObjectId(8))
                .exists(&scene, &cancel)
                .unwrap()
        );
        assert!(
            !SelectionTarget::Track(ObjectId(7))
                .exists(&scene, &cancel)
                .unwrap()
        );
        assert!(
            SelectionTarget::Net(NetId(9))
                .exists(&scene, &cancel)
                .unwrap()
        );
        cancel.cancel();
        assert!(matches!(
            SelectionTarget::Net(NetId(9)).exists(&scene, &cancel),
            Err(PathError::Cancelled)
        ));
    }
}
