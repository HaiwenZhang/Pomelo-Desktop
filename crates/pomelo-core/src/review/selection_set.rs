//! Arbitrary multi-selection, independent of network or component grouping.
//!
//! The set holds typed source identities. Adding, removing, box selection and group
//! expansion share the same deduplicating semantics; the inspector statistics and the
//! renderer highlight read the same immutable snapshot.
use crate::{
    display::BoardDisplay,
    geometry::{PathError, path_bounds},
    model::{BoardScene, Bounds, LayerId, Pad, Point, Segment},
    pad::PadPlacement,
    picking::PickFilter,
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};
use std::{collections::BTreeSet, sync::Arc};

/// Upper bound on one set, so a whole-board selection cannot exhaust memory.
pub const MAX_SELECTION_OBJECTS: usize = 1_000_000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectionCounts {
    pub segments: usize,
    pub pins: usize,
    pub vias: usize,
    pub zones: usize,
    pub drawings: usize,
    pub placements: usize,
}

impl SelectionCounts {
    pub fn total(&self) -> usize {
        self.segments + self.pins + self.vias + self.zones + self.drawings + self.placements
    }
}

/// Result of adding many objects at once.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Extended {
    pub added: usize,
    /// Objects were left out because the set reached `MAX_SELECTION_OBJECTS`.
    pub truncated: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SelectionSet {
    objects: Arc<BTreeSet<SelectedObject>>,
}

impl SelectionSet {
    /// Shared snapshot for rendering and background work; cloning it is cheap.
    pub fn objects(&self) -> &Arc<BTreeSet<SelectedObject>> {
        &self.objects
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn contains(&self, object: SelectedObject) -> bool {
        self.objects.contains(&object)
    }

    /// Returns true when the object was added.
    pub fn insert(&mut self, object: SelectedObject) -> bool {
        if self.objects.len() >= MAX_SELECTION_OBJECTS || self.objects.contains(&object) {
            return false;
        }
        Arc::make_mut(&mut self.objects).insert(object)
    }

    pub fn remove(&mut self, object: SelectedObject) -> bool {
        self.objects.contains(&object) && Arc::make_mut(&mut self.objects).remove(&object)
    }

    /// Add an absent object or remove a present one; returns whether it is now selected.
    pub fn toggle(&mut self, object: SelectedObject) -> bool {
        if self.remove(object) {
            false
        } else {
            self.insert(object)
        }
    }

    pub fn extend(&mut self, objects: impl IntoIterator<Item = SelectedObject>) -> Extended {
        let mut result = Extended::default();
        let mut objects = objects.into_iter().peekable();
        if objects.peek().is_none() {
            return result;
        }
        let set = Arc::make_mut(&mut self.objects);
        for object in objects {
            if set.contains(&object) {
                continue;
            }
            if set.len() >= MAX_SELECTION_OBJECTS {
                result.truncated = true;
                break;
            }
            set.insert(object);
            result.added += 1;
        }
        result
    }

    /// Remove every listed object; returns how many were present.
    pub fn remove_all(&mut self, objects: impl IntoIterator<Item = SelectedObject>) -> usize {
        let mut removed = 0;
        for object in objects {
            if self.remove(object) {
                removed += 1;
            }
        }
        removed
    }

    pub fn clear(&mut self) {
        if !self.objects.is_empty() {
            self.objects = Arc::default();
        }
    }

    pub fn counts(&self) -> SelectionCounts {
        count_objects(self.objects.iter().copied())
    }

    /// One page in stable type-then-identity order.
    pub fn page(&self, offset: usize, limit: usize) -> Vec<SelectedObject> {
        self.objects
            .iter()
            .skip(offset)
            .take(limit)
            .copied()
            .collect()
    }
}

pub fn count_objects(objects: impl IntoIterator<Item = SelectedObject>) -> SelectionCounts {
    let mut counts = SelectionCounts::default();
    for object in objects {
        match object {
            SelectedObject::Segment(_) => counts.segments += 1,
            SelectedObject::Pin(_) => counts.pins += 1,
            SelectedObject::Via(_) => counts.vias += 1,
            SelectedObject::Zone(_) => counts.zones += 1,
            SelectedObject::Drawing(_) => counts.drawings += 1,
            SelectedObject::Placement(_) => counts.placements += 1,
        }
    }
    counts
}

/// Expand a network, component or track target into its member objects.
pub fn target_objects(
    scene: &BoardScene,
    target: SelectionTarget,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<(Vec<SelectedObject>, bool), PathError> {
    let mut objects = Vec::new();
    let mut truncated = false;
    target.visit_members(scene, cancel, |member| {
        if objects.len() < limit {
            objects.push(member.object());
        } else {
            truncated = true;
        }
        Ok(())
    })?;
    Ok((objects, truncated))
}

/// Window selection keeps objects entirely inside the rectangle; crossing selection
/// also keeps objects that touch it. Tracks use their round-cap stroke; pads, zones
/// and drawings use their bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxMode {
    Window,
    Crossing,
}

impl BoxMode {
    /// CAD convention: dragging left-to-right is a window, right-to-left a crossing.
    pub fn from_drag(start: Point, end: Point) -> Self {
        if end.x >= start.x {
            Self::Window
        } else {
            Self::Crossing
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BoxSelection {
    pub objects: Vec<SelectedObject>,
    pub truncated: bool,
}

/// Visible, pickable objects in a board-space rectangle, in source order.
pub fn objects_in_box(
    scene: &BoardScene,
    area: Bounds,
    mode: BoxMode,
    filter: PickFilter,
    display: &BoardDisplay,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<BoxSelection, PathError> {
    let mut selection = BoxSelection::default();
    if !area.is_valid() {
        return Ok(selection);
    }
    let mut accept = |object: SelectedObject| {
        if selection.objects.len() < limit {
            selection.objects.push(object);
        } else {
            selection.truncated = true;
        }
    };
    for segment in &scene.segments {
        cancel.check()?;
        let object = SelectedObject::Segment(segment.id);
        if filter.allows(object, &[segment.layer], display) && segment_in_box(segment, area, mode) {
            accept(object);
        }
    }
    for pin in &scene.pins {
        cancel.check()?;
        let object = SelectedObject::Pin(pin.id);
        let owner = PadPlacement {
            at: pin.at,
            angle: pin.angle,
            mirrored: pin.mirrored,
        };
        if pads_in_box(object, &pin.pads, owner, area, mode, filter, display) {
            accept(object);
        }
    }
    for via in &scene.vias {
        cancel.check()?;
        let object = SelectedObject::Via(via.id);
        let owner = PadPlacement {
            at: via.at,
            angle: via.angle,
            mirrored: via.mirrored,
        };
        if pads_in_box(object, &via.pads, owner, area, mode, filter, display) {
            accept(object);
        }
    }
    for zone in &scene.zones {
        cancel.check()?;
        let object = SelectedObject::Zone(zone.id);
        if !filter.allows(object, &[zone.layer], display) {
            continue;
        }
        let bounds = zone.paths.iter().filter_map(|path| path_bounds(path));
        if union(bounds).is_some_and(|bounds| bounds_in_box(bounds, area, mode)) {
            accept(object);
        }
    }
    for drawing in &scene.drawings {
        cancel.check()?;
        let object = SelectedObject::Drawing(drawing.id);
        if !filter.allows(object, &[drawing.layer], display) {
            continue;
        }
        let bounds = drawing.segments.iter().filter_map(Segment::bounds);
        if union(bounds).is_some_and(|bounds| bounds_in_box(bounds, area, mode)) {
            accept(object);
        }
    }
    cancel.check()?;
    Ok(selection)
}

fn union(bounds: impl Iterator<Item = Bounds>) -> Option<Bounds> {
    let mut result: Option<Bounds> = None;
    for bounds in bounds {
        match &mut result {
            Some(result) => {
                result.include(bounds.min);
                result.include(bounds.max);
            }
            None => result = Some(bounds),
        }
    }
    result
}

fn bounds_in_box(bounds: Bounds, area: Bounds, mode: BoxMode) -> bool {
    match mode {
        BoxMode::Window => {
            bounds.min.x >= area.min.x
                && bounds.max.x <= area.max.x
                && bounds.min.y >= area.min.y
                && bounds.max.y <= area.max.y
        }
        BoxMode::Crossing => {
            bounds.min.x <= area.max.x
                && bounds.max.x >= area.min.x
                && bounds.min.y <= area.max.y
                && bounds.max.y >= area.min.y
        }
    }
}

fn pads_in_box(
    object: SelectedObject,
    pads: &[Pad],
    owner: PadPlacement,
    area: Bounds,
    mode: BoxMode,
    filter: PickFilter,
    display: &BoardDisplay,
) -> bool {
    let copper: Vec<_> = pads
        .iter()
        .filter(|pad| pad.supported() && !pad.backdrill)
        .collect();
    let layers: Vec<LayerId> = copper.iter().map(|pad| pad.layer).collect();
    if !filter.allows(object, &layers, display) {
        return false;
    }
    let visible = copper
        .iter()
        .filter(|pad| display.object_visible(pad.layer, object))
        .filter_map(|pad| pad.bounds(owner));
    union(visible).is_some_and(|bounds| bounds_in_box(bounds, area, mode))
}

fn segment_in_box(segment: &Segment, area: Bounds, mode: BoxMode) -> bool {
    let Some(bounds) = segment.bounds() else {
        return false;
    };
    match mode {
        BoxMode::Window => bounds_in_box(bounds, area, mode),
        // Arcs fall back to their bounds; straight strokes use the exact capsule.
        BoxMode::Crossing if segment.arc.is_some() => bounds_in_box(bounds, area, mode),
        BoxMode::Crossing => {
            bounds_in_box(bounds, area, mode)
                && segment_rectangle_distance(segment.a, segment.b, area)
                    <= segment.width.max(0.0) * 0.5
        }
    }
}

fn segment_rectangle_distance(a: Point, b: Point, area: Bounds) -> f64 {
    let inside = |point: Point| {
        point.x >= area.min.x
            && point.x <= area.max.x
            && point.y >= area.min.y
            && point.y <= area.max.y
    };
    if inside(a) || inside(b) {
        return 0.0;
    }
    let corners = [
        area.min,
        Point::new(area.max.x, area.min.y),
        area.max,
        Point::new(area.min.x, area.max.y),
    ];
    (0..4)
        .map(|index| segment_distance(a, b, corners[index], corners[(index + 1) % 4]))
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance(p1: Point, q1: Point, p2: Point, q2: Point) -> f64 {
    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let (d1, d2) = (cross(p2, q2, p1), cross(p2, q2, q1));
    let (d3, d4) = (cross(p1, q1, p2), cross(p1, q1, q2));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    let to_line = crate::geometry::distance_to_line;
    to_line(p1, p2, q2)
        .min(to_line(q1, p2, q2))
        .min(to_line(p2, p1, q1))
        .min(to_line(q2, p1, q1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, NetId, ObjectId};

    fn segment(id: u32, a: (f64, f64), b: (f64, f64)) -> Segment {
        Segment {
            id: ObjectId(id),
            track_id: ObjectId(id),
            layer: LayerId(0),
            net: NetId(1),
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
            width: 0.2,
            arc: None,
            bond_wire: None,
        }
    }

    fn area(min: (f64, f64), max: (f64, f64)) -> Bounds {
        Bounds {
            min: Point::new(min.0, min.1),
            max: Point::new(max.0, max.1),
        }
    }

    #[test]
    fn set_operations_deduplicate_and_count_by_type() {
        let mut set = SelectionSet::default();
        let pin = SelectedObject::Pin(ObjectId(1));
        let via = SelectedObject::Via(ObjectId(1));
        assert!(set.insert(pin));
        assert!(!set.insert(pin));
        assert_eq!(
            set.extend([pin, via, SelectedObject::Segment(ObjectId(4))]),
            Extended {
                added: 2,
                truncated: false
            }
        );
        assert_eq!(set.counts().total(), 3);
        assert_eq!(set.counts().pins, 1);
        assert!(!set.toggle(pin));
        assert!(set.toggle(pin));
        assert_eq!(set.remove_all([pin, via, pin]), 2);
        assert_eq!(set.len(), 1);
        set.clear();
        assert!(set.is_empty());
    }

    #[test]
    fn snapshots_are_not_affected_by_later_edits() {
        let mut set = SelectionSet::default();
        set.insert(SelectedObject::Pin(ObjectId(1)));
        let snapshot = Arc::clone(set.objects());
        set.insert(SelectedObject::Pin(ObjectId(2)));
        assert_eq!(snapshot.len(), 1);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn pages_are_stable() {
        let mut set = SelectionSet::default();
        set.extend(
            (0..10)
                .rev()
                .map(|id| SelectedObject::Segment(ObjectId(id))),
        );
        assert_eq!(
            set.page(3, 2),
            vec![
                SelectedObject::Segment(ObjectId(3)),
                SelectedObject::Segment(ObjectId(4))
            ]
        );
        assert!(set.page(20, 2).is_empty());
    }

    #[test]
    fn drag_direction_selects_window_or_crossing() {
        assert_eq!(
            BoxMode::from_drag(Point::new(0.0, 0.0), Point::new(1.0, -1.0)),
            BoxMode::Window
        );
        assert_eq!(
            BoxMode::from_drag(Point::new(1.0, 0.0), Point::new(0.0, 1.0)),
            BoxMode::Crossing
        );
    }

    #[test]
    fn window_requires_containment_and_crossing_uses_the_stroke() {
        let scene = BoardScene {
            segments: vec![
                segment(1, (1.0, 1.0), (2.0, 2.0)),
                segment(2, (1.0, 1.0), (20.0, 1.0)),
                // Its bounds overlap the box, but the diagonal stroke passes outside it.
                segment(3, (4.0, 8.0), (8.0, 4.0)),
                segment(4, (30.0, 30.0), (31.0, 31.0)),
            ],
            ..crate::review::tests::empty_scene()
        };
        let display = BoardDisplay::default();
        let cancel = CancellationToken::default();
        let select = |mode| {
            objects_in_box(
                &scene,
                area((0.0, 0.0), (5.0, 5.0)),
                mode,
                PickFilter::all(),
                &display,
                100,
                &cancel,
            )
            .unwrap()
            .objects
        };
        assert_eq!(
            select(BoxMode::Window),
            vec![SelectedObject::Segment(ObjectId(1))]
        );
        assert_eq!(
            select(BoxMode::Crossing),
            vec![
                SelectedObject::Segment(ObjectId(1)),
                SelectedObject::Segment(ObjectId(2))
            ]
        );
    }

    #[test]
    fn hidden_layers_filters_and_limits_apply() {
        let scene = BoardScene {
            segments: vec![
                segment(1, (1.0, 1.0), (2.0, 2.0)),
                segment(2, (1.0, 2.0), (2.0, 3.0)),
            ],
            ..crate::review::tests::empty_scene()
        };
        let cancel = CancellationToken::default();
        let mut display = BoardDisplay::default();
        let all = area((0.0, 0.0), (5.0, 5.0));
        let limited = objects_in_box(
            &scene,
            all,
            BoxMode::Window,
            PickFilter::all(),
            &display,
            1,
            &cancel,
        )
        .unwrap();
        assert_eq!(limited.objects.len(), 1);
        assert!(limited.truncated);
        let mut filter = PickFilter::all();
        filter.set(crate::picking::PickCategory::Segment, false);
        assert!(
            objects_in_box(&scene, all, BoxMode::Window, filter, &display, 9, &cancel)
                .unwrap()
                .objects
                .is_empty()
        );
        display.hidden_layers.insert(LayerId(0));
        assert!(
            objects_in_box(
                &scene,
                all,
                BoxMode::Window,
                PickFilter::all(),
                &display,
                9,
                &cancel
            )
            .unwrap()
            .objects
            .is_empty()
        );
    }

    #[test]
    fn network_expansion_lists_members() {
        let scene = BoardScene {
            segments: vec![segment(1, (0.0, 0.0), (1.0, 0.0)), {
                let mut other = segment(2, (0.0, 0.0), (1.0, 0.0));
                other.net = NetId(2);
                other
            }],
            ..crate::review::tests::empty_scene()
        };
        let (objects, truncated) = target_objects(
            &scene,
            SelectionTarget::Net(NetId(1)),
            10,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(objects, vec![SelectedObject::Segment(ObjectId(1))]);
        assert!(!truncated);
    }
}
