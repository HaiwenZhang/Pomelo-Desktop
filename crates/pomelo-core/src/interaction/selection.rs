//! Domain selection predicates shared by picking, search and GPU overlays.

use crate::interaction::component_reference::ComponentAnchor;
use crate::model::{BoardScene, NetId, ObjectId, Pin, Segment, Via, Zone};
use crate::{geometry::PathError, task::CancellationToken};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, Copy)]
pub struct SelectionSummary {
    pub segments: usize,
    pub pins: usize,
    pub vias: usize,
    pub zones: usize,
    pub drawings: usize,
    /// Sum of selected 2D centerlines; excludes drill depth and electrical delay.
    pub centerline_length_mm: f64,
}

#[derive(Debug)]
pub struct SelectionMembers {
    /// Source order within segment, pin, via, zone categories; bounded to 256 rows.
    pub objects: Vec<SelectedObject>,
    pub total: usize,
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
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if key(value) == id {
                    return Ok(value);
                }
            }
            if cancel.is_cancelled() {
                Err(PathError::Cancelled)
            } else {
                Err(PathError::Invalid(id))
            }
        }
        let (net, track, pin) = match self {
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
                        if cancel.is_cancelled() {
                            return Err(PathError::Cancelled);
                        }
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
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(result)
        }
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
    if cancel.is_cancelled() {
        return Err(PathError::Cancelled);
    }
    let limit = limit.min(64);
    let mut candidates: Vec<SelectionCandidate> = Vec::new();
    if limit == 0 {
        return Ok(candidates);
    }
    for hit in hits {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if !hit.distance_mm.is_finite() || hit.distance_mm < 0.0 {
            let id = match hit.object {
                SelectedObject::Segment(id)
                | SelectedObject::Pin(id)
                | SelectedObject::Via(id)
                | SelectedObject::Zone(id) => id,
                SelectedObject::Drawing(id) => id,
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
    if cancel.is_cancelled() {
        Err(PathError::Cancelled)
    } else {
        Ok(candidates)
    }
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
pub fn resolve_canvas_candidates(
    scene: &BoardScene,
    hits: &[crate::picking::ObjectHit],
    mode: crate::interaction::SelectionMode,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<SelectionCandidate>, PathError> {
    let mut candidates = Vec::new();
    for hit in hits {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if candidates.len() >= limit.min(64) {
            break;
        }
        let mut target = if mode == crate::interaction::SelectionMode::Component {
            ComponentAnchor::from_object(hit.object, scene, cancel)?
                .map(SelectionTarget::ComponentGroup)
                .unwrap_or(SelectionTarget::Object(hit.object))
        } else {
            hit.object
                .resolve(scene, mode, cancel)?
                .unwrap_or(SelectionTarget::Object(hit.object))
        };
        if let SelectionTarget::Component(id) = target
            && scene
                .components
                .iter()
                .any(|component| component.id == id && component.reference.is_empty())
        {
            target = SelectionTarget::Object(hit.object);
        }
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
    if cancel.is_cancelled() {
        return Err(PathError::Cancelled);
    }
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
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        let anchor = match self {
            Self::ComponentGroup(anchor) => {
                ComponentAnchor::from_object(anchor.object(), scene, cancel)?
            }
            Self::Component(id) => {
                let mut reference = None;
                for component in &scene.components {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
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
        Ok(anchor.map(Self::ComponentGroup).unwrap_or(self))
    }

    /// Full pin selection for GPU overlays; paging never limits highlighted pins.
    pub fn pin_ids(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<ObjectId>, PathError> {
        let mut pins = BTreeSet::new();
        match self {
            Self::ComponentGroup(anchor) => anchor.visit_members(scene, cancel, |object| {
                if let SelectedObject::Pin(id) = object {
                    pins.insert(id);
                }
            })?,
            Self::Component(id) => {
                for component in &scene.components {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    if component.id == id {
                        for &pin in &component.pins {
                            if cancel.is_cancelled() {
                                return Err(PathError::Cancelled);
                            }
                            pins.insert(pin);
                        }
                        break;
                    }
                }
            }
            Self::Object(SelectedObject::Pin(id)) => {
                pins.insert(id);
            }
            _ => {}
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(pins)
        }
    }

    /// Full reference group for hover. Legacy placement links remain available.
    pub fn component_objects(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<SelectedObject>, PathError> {
        let mut objects = BTreeSet::new();
        if let Self::ComponentGroup(anchor) = self {
            anchor.visit_members(scene, cancel, |object| {
                objects.insert(object);
            })?;
        } else if matches!(self, Self::Component(_)) {
            objects.extend(
                self.pin_ids(scene, cancel)?
                    .into_iter()
                    .map(SelectedObject::Pin),
            );
            objects.extend(self.related_bond_objects(scene, cancel)?);
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(objects)
        }
    }

    /// Revalidate persisted typed identity before restoring selection work.
    pub fn exists(self, scene: &BoardScene, cancel: &CancellationToken) -> Result<bool, PathError> {
        fn contains(
            ids: impl Iterator<Item = ObjectId>,
            expected: ObjectId,
            cancel: &CancellationToken,
        ) -> Result<bool, PathError> {
            for id in ids {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if id == expected {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
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
            Self::Component(id) => {
                contains(scene.components.iter().map(|item| item.id), id, cancel)?
            }
            Self::ComponentGroup(anchor) => anchor.reference(scene, cancel)?.is_some(),
            Self::Net(id) => scene.nets.contains_key(&id),
        };
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(exists)
        }
    }

    /// Reference groups return their fingers; source placements use explicit
    /// bond-wire/finger links through their source pins.
    /// This does not infer physical connectivity or add ordinary net members.
    pub fn related_bond_objects(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Vec<SelectedObject>, PathError> {
        if let Self::ComponentGroup(anchor) = self {
            let mut fingers = Vec::new();
            anchor.visit_members(scene, cancel, |object| {
                if matches!(object, SelectedObject::Via(_)) {
                    fingers.push(object);
                }
            })?;
            return Ok(fingers);
        }
        self.members(scene, 0, cancel)?;
        let Self::Component(id) = self else {
            return Ok(Vec::new());
        };
        let mut pins = BTreeSet::new();
        for component in &scene.components {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if component.id == id {
                for &pin in &component.pins {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    pins.insert(pin);
                }
                break;
            }
        }
        let mut objects = Vec::new();
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if segment
                .bond_wire
                .as_ref()
                .is_some_and(|wire| pins.contains(&wire.source_pin))
            {
                objects.push(SelectedObject::Segment(segment.id));
            }
        }
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if via
                .finger
                .as_ref()
                .and_then(|finger| finger.source_pin)
                .is_some_and(|pin| pins.contains(&pin))
            {
                objects.push(SelectedObject::Via(via.id));
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(objects)
        }
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
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if let Self::ComponentGroup(anchor) = self {
            let mut result = SelectionMembers {
                objects: Vec::new(),
                total: 0,
            };
            anchor.visit_members(scene, cancel, |object| {
                let position = result.total;
                result.total += 1;
                if position >= offset && result.objects.len() < limit.min(256) {
                    result.objects.push(object);
                }
            })?;
            return Ok(result);
        }
        let mut component_pins = if let Self::Component(id) = self {
            let mut pins = BTreeSet::new();
            for component in &scene.components {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if component.id != id {
                    continue;
                }
                for &pin in &component.pins {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    if !pins.insert(pin) {
                        return Err(PathError::Invalid(pin));
                    }
                }
                break;
            }
            Some(pins)
        } else {
            None
        };
        let mut result = SelectionMembers {
            objects: Vec::new(),
            total: 0,
        };
        let limit = limit.min(256);
        let mut append = |object| {
            let position = result.total;
            result.total += 1;
            if position >= offset && result.objects.len() < limit {
                result.objects.push(object);
            }
        };
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.matches_segment(segment) {
                append(SelectedObject::Segment(segment.id));
            }
        }
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let selected = match &mut component_pins {
                Some(pins) => pins.remove(&pin.id),
                None => self.matches_pin(pin, scene),
            };
            if selected {
                if let Self::Component(id) = self
                    && pin.owner_id != id
                {
                    return Err(PathError::Invalid(pin.id));
                }
                append(SelectedObject::Pin(pin.id));
            }
        }
        if let Some(id) = component_pins.as_ref().and_then(|pins| pins.first()) {
            return Err(PathError::Invalid(*id));
        }
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.matches_via(via) {
                append(SelectedObject::Via(via.id));
            }
        }
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.matches_zone(zone) {
                append(SelectedObject::Zone(zone.id));
            }
        }
        for drawing in &scene.drawings {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self == Self::Object(SelectedObject::Drawing(drawing.id)) {
                append(SelectedObject::Drawing(drawing.id));
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(result)
        }
    }

    pub fn summarize(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<SelectionSummary, PathError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if let Self::ComponentGroup(anchor) = self {
            let mut summary = SelectionSummary::default();
            anchor.visit_members(scene, cancel, |object| match object {
                SelectedObject::Pin(_) => summary.pins += 1,
                SelectedObject::Via(_) => summary.vias += 1,
                _ => {}
            })?;
            return Ok(summary);
        }
        let mut component_pins = if let Self::Component(id) = self {
            let mut pins = BTreeSet::new();
            for component in &scene.components {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if component.id != id {
                    continue;
                }
                for &pin in &component.pins {
                    if cancel.is_cancelled() {
                        return Err(PathError::Cancelled);
                    }
                    if !pins.insert(pin) {
                        return Err(PathError::Invalid(pin));
                    }
                }
                break;
            }
            Some(pins)
        } else {
            None
        };
        let mut summary = SelectionSummary::default();
        let mut compensation = 0.0;
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !self.matches_segment(segment) {
                continue;
            }
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
        }
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let selected = match &mut component_pins {
                Some(pins) => pins.remove(&pin.id),
                None => self.matches_pin(pin, scene),
            };
            summary.pins += usize::from(selected);
        }
        if let Some(id) = component_pins.as_ref().and_then(|pins| pins.first()) {
            return Err(PathError::Invalid(*id));
        }
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            summary.vias += usize::from(self.matches_via(via));
        }
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            summary.zones += usize::from(self.matches_zone(zone));
        }
        for drawing in &scene.drawings {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            summary.drawings +=
                usize::from(self == Self::Object(SelectedObject::Drawing(drawing.id)));
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(summary)
        }
    }

    pub fn matches_segment(self, segment: &Segment) -> bool {
        match self {
            Self::Object(SelectedObject::Segment(id)) => id == segment.id,
            Self::Track(id) => id == segment.track_id,
            Self::Net(net) => net.0 != 0 && net == segment.net,
            _ => false,
        }
    }

    pub fn matches_pin(self, pin: &Pin, scene: &BoardScene) -> bool {
        match self {
            Self::Object(SelectedObject::Pin(id)) => id == pin.id,
            Self::Net(net) => net.0 != 0 && net == pin.net,
            Self::Component(id) => scene
                .components
                .iter()
                .find(|value| value.id == id)
                .is_some_and(|component| component.pins.contains(&pin.id)),
            Self::ComponentGroup(anchor) => anchor
                .reference(scene, &CancellationToken::default())
                .ok()
                .flatten()
                .is_some_and(|reference| pin.reference == reference),
            _ => false,
        }
    }

    pub fn matches_via(self, via: &Via) -> bool {
        match self {
            Self::Object(SelectedObject::Via(id)) => id == via.id,
            Self::Net(net) => net.0 != 0 && net == via.net,
            _ => false,
        }
    }

    pub fn matches_zone(self, zone: &Zone) -> bool {
        match self {
            Self::Object(SelectedObject::Zone(id)) => id == zone.id,
            Self::Net(net) => net.0 != 0 && net == zone.net,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, Point};

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
        assert!(
            SelectionTarget::Object(SelectedObject::Segment(ObjectId(7))).matches_segment(&segment)
        );
        assert!(
            !SelectionTarget::Object(SelectedObject::Via(ObjectId(7))).matches_segment(&segment)
        );
        assert!(SelectionTarget::Track(ObjectId(8)).matches_segment(&segment));
        assert!(!SelectionTarget::Track(ObjectId(7)).matches_segment(&segment));
        assert!(SelectionTarget::Net(NetId(9)).matches_segment(&segment));
        let unconnected = Segment {
            net: NetId(0),
            ..segment
        };
        assert!(!SelectionTarget::Net(NetId(0)).matches_segment(&unconnected));
    }
}
