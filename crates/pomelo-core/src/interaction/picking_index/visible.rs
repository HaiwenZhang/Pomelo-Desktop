//! Canvas picking uses signed coverage and the same submission rank as rendering.
use super::*;
use crate::{
    display::{DisplayCategory, LayerPrimitive},
    model::{LayerId, ObjectId, Pad, Point, Segment},
    pad::PadPlacement,
    picking::{ObjectHit, PickCategory},
};
use std::cmp::Ordering;

struct RankedHit {
    object: SelectedObject,
    layer: LayerId,
    category: DisplayCategory,
    distance: f64,
    rank: [i64; 5],
    sequence: usize,
}
/// Actual canvas hit entry; a group expansion must preserve this source anchor.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CanvasHit {
    pub anchor: SelectionAnchor,
    pub distance_mm: f64,
}
impl CanvasHit {
    pub fn object_hit(self) -> ObjectHit {
        ObjectHit {
            object: self.anchor.object,
            distance_mm: self.distance_mm,
        }
    }
}
impl RankedHit {
    fn compare(&self, other: &Self) -> Ordering {
        let exact = self.distance <= 0.0;
        let other_exact = other.distance <= 0.0;
        other_exact
            .cmp(&exact)
            .then_with(|| {
                if exact {
                    Ordering::Equal
                } else {
                    self.distance.total_cmp(&other.distance)
                }
            })
            .then_with(|| other.rank.cmp(&self.rank))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl BoardPickingIndex {
    /// Web's five-logical-pixel tolerance, signed pad/stroke coverage, topmost exact
    /// hit before nearest misses. Truncation follows ordering, never precedes it.
    pub fn query_visible_objects(
        &self,
        query: PickQuery,
        pixels_per_mm: f64,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<ObjectHit>, PathError> {
        self.query_visible_hits(query, pixels_per_mm, filter, display, limit, cancel)
            .map(|hits| hits.into_iter().map(CanvasHit::object_hit).collect())
    }

    /// Preserve the winning layer/category while applying the same signed hit ordering.
    pub fn query_visible_hits(
        &self,
        query: PickQuery,
        pixels_per_mm: f64,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<CanvasHit>, PathError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if !pixels_per_mm.is_finite() || pixels_per_mm <= 0.0 {
            return Ok(Vec::new());
        }
        let limit = limit.min(64);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let px = 1.0 / pixels_per_mm;
        let mut hits: Vec<RankedHit> = Vec::new();
        let mut insert = |object, distance: f64, layer, category, sequence| {
            if !distance.is_finite() || distance > query.tolerance_mm {
                return;
            }
            let hit = RankedHit {
                object,
                layer,
                category,
                distance,
                rank: display.display_rank(layer, category),
                sequence,
            };
            if let Some(index) = hits.iter().position(|old| old.object == object) {
                if hit.compare(&hits[index]) != Ordering::Less {
                    return;
                }
                hits.remove(index);
            }
            let position = hits.partition_point(|old| old.compare(&hit) != Ordering::Greater);
            if position < limit {
                hits.insert(position, hit);
                hits.truncate(limit);
            }
        };
        let broad = query;
        let mut pending = vec![1usize];
        while let Some(index) = pending.pop() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if self.bounds[index].is_none_or(|bounds| !broad.intersects(bounds)) {
                continue;
            }
            if index < self.leaf_start {
                pending.push(index * 2 + 1);
                pending.push(index * 2);
                continue;
            }
            let source = self.order[index - self.leaf_start];
            let segment = &self.scene.segments[source];
            if !filter.allows(
                SelectedObject::Segment(segment.id),
                &[segment.layer],
                display,
            ) {
                continue;
            }
            insert(
                SelectedObject::Segment(segment.id),
                centerline_distance(segment, query.point)? - (segment.width * 0.5).max(px * 0.5),
                segment.layer,
                if segment.bond_wire.is_some() {
                    DisplayCategory::BondWire
                } else {
                    DisplayCategory::Trace
                },
                source
                    .saturating_add(usize::from(segment.arc.is_some()) * self.scene.segments.len()),
            );
        }
        let query_bounds = crate::model::Bounds {
            min: Point::new(
                query.point.x - query.tolerance_mm,
                query.point.y - query.tolerance_mm,
            ),
            max: Point::new(
                query.point.x + query.tolerance_mm,
                query.point.y + query.tolerance_mm,
            ),
        };
        if filter.contains(PickCategory::Drawing) {
            for entry_index in self.drawings.bounds.query(query_bounds, cancel)? {
                let entry = &self.drawings.entries[entry_index];
                if !display.layer_visible(entry.layer) {
                    continue;
                }
                let (distance, category) = match entry.geometry {
                    super::drawings::DrawingGeometry::Stroke { drawing, segment } => {
                        if !display.show_drawings {
                            continue;
                        }
                        let stroke = &self.scene.drawings[drawing].segments[segment];
                        (
                            centerline_distance(stroke, query.point)?
                                - (stroke.width * 0.5).max(px * 0.5),
                            DisplayCategory::Drawing,
                        )
                    }
                    super::drawings::DrawingGeometry::Glyphs { start, count } => {
                        if !display.show_texts {
                            continue;
                        }
                        let mut distance = f64::INFINITY;
                        for quad in &self.drawings.glyphs[start..start + count] {
                            if cancel.is_cancelled() {
                                return Err(PathError::Cancelled);
                            }
                            let mut inside = false;
                            let mut edge_distance = f64::INFINITY;
                            for i in 0..4 {
                                let a = quad.corners[i];
                                let b = quad.corners[(i + 1) % 4];
                                if (a.y > query.point.y) != (b.y > query.point.y)
                                    && query.point.x
                                        < (b.x - a.x) * (query.point.y - a.y) / (b.y - a.y) + a.x
                                {
                                    inside = !inside;
                                }
                                edge_distance = edge_distance.min(
                                    crate::geometry::distance_to_line(query.point, a, b) - px * 0.5,
                                );
                            }
                            distance = distance.min(if inside { -px } else { edge_distance });
                        }
                        (distance, DisplayCategory::Text)
                    }
                };
                insert(
                    SelectedObject::Drawing(entry.owner),
                    distance,
                    entry.layer,
                    category,
                    entry.sequence,
                );
            }
        }
        for source in self.zone_bounds.query(query_bounds, cancel)? {
            let zone = &self.scene.zones[source];
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !filter.contains(PickCategory::Zone)
                || !display.show_copper
                || !display.layer_visible(zone.layer)
            {
                continue;
            }
            let paths_bounds = &self.zone_paths[source];
            let bounds = paths_bounds
                .first()
                .copied()
                .flatten()
                .or_else(|| zone.mesh.ring_bounds.first().copied());
            if bounds.is_none_or(|bounds| !broad.intersects(bounds)) {
                continue;
            }
            let contains = if zone.paths.is_empty() {
                zone.mesh
                    .covers_fill(query.point, cancel)
                    .map_err(|_| PathError::Invalid(zone.id))?
            } else {
                let mut covered =
                    crate::geometry::path_contains(&zone.paths[0], query.point, cancel)?;
                for (path, bounds) in zone.paths.iter().zip(paths_bounds).skip(1) {
                    if !covered {
                        break;
                    }
                    if bounds.is_none_or(|bounds| point_in_bounds(query.point, bounds))
                        && crate::geometry::path_contains(path, query.point, cancel)?
                    {
                        covered = false;
                    }
                }
                covered
            };
            let (distance, category) = if display.copper_opacity > 0.0 && contains {
                (-px, DisplayCategory::Zone)
            } else {
                let distance = if zone.paths.is_empty() {
                    ring_distance(
                        &zone.mesh.vertices,
                        &zone.mesh.ring_offsets,
                        query.point,
                        cancel,
                    )?
                } else {
                    let nearby = PickQuery {
                        point: query.point,
                        tolerance_mm: query.tolerance_mm + 0.65 * px,
                    };
                    let mut distance = f64::INFINITY;
                    for (path, bounds) in zone.paths.iter().zip(paths_bounds) {
                        if cancel.is_cancelled() {
                            return Err(PathError::Cancelled);
                        }
                        if bounds.is_none_or(|bounds| nearby.intersects(bounds)) {
                            distance = distance.min(paths_distance(
                                std::slice::from_ref(path),
                                query.point,
                                cancel,
                            )?);
                        }
                    }
                    distance
                };
                (distance - 0.65 * px, DisplayCategory::ZoneOutline)
            };
            insert(
                SelectedObject::Zone(zone.id),
                distance,
                zone.layer,
                category,
                source,
            );
        }
        // PrimitiveBatchBuilder submits pin pads before via pads. Within each
        // category analytic pads precede custom fills; last source entry wins.
        for source in self.pins.query(query_bounds, cancel)? {
            let pin = &self.scene.pins[source];
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !filter.contains(PickCategory::Pin) {
                continue;
            }
            let owner = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            for (pad_index, pad) in pin.pads.iter().enumerate() {
                let category = if pin.die.is_some() {
                    DisplayCategory::Trace
                } else {
                    DisplayCategory::Pin
                };
                if pad.backdrill
                    || !display.primitive_visible(
                        pad.layer,
                        if category == DisplayCategory::Trace {
                            LayerPrimitive::Traces
                        } else {
                            LayerPrimitive::Pads
                        },
                    )
                {
                    continue;
                }
                if pad
                    .bounds(owner)
                    .is_none_or(|bounds| !broad.intersects(bounds))
                {
                    continue;
                }
                let distance = signed_pad_distance(pad, query.point, owner, pin.id, cancel)?;
                let distance = if display.filled {
                    distance
                } else {
                    distance.abs() - 0.65 * px
                };
                let sequence = source.saturating_mul(65536).saturating_add(pad_index);
                insert(
                    SelectedObject::Pin(pin.id),
                    distance,
                    pad.layer,
                    category,
                    sequence.saturating_add(usize::from(pad.custom.is_some()) * (usize::MAX / 2)),
                );
            }
            if display.show_drills
                && let Some(pad) = pin.drill_shape.pad()
            {
                let distance = signed_pad_distance(&pad, query.point, owner, pin.id, cancel)?;
                insert(
                    SelectedObject::Pin(pin.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    self.scene.pins.len() + source,
                );
            }
        }
        for source in self.vias.query(query_bounds, cancel)? {
            let via = &self.scene.vias[source];
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if !filter.contains(PickCategory::Via) {
                continue;
            }
            let owner = PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            for (pad_index, pad) in via.pads.iter().enumerate() {
                if pad.backdrill
                    || (pad.backdrill_base && display.show_backdrills)
                    || !display.primitive_visible(pad.layer, LayerPrimitive::Vias)
                {
                    continue;
                }
                if pad
                    .bounds(owner)
                    .is_none_or(|bounds| !broad.intersects(bounds))
                {
                    continue;
                }
                let distance = signed_pad_distance(pad, query.point, owner, via.id, cancel)?;
                let distance = if display.filled {
                    distance
                } else {
                    distance.abs() - 0.65 * px
                };
                insert(
                    SelectedObject::Via(via.id),
                    distance,
                    pad.layer,
                    DisplayCategory::Via,
                    source
                        .saturating_mul(65536)
                        .saturating_add(pad_index)
                        .saturating_add(usize::from(pad.custom.is_some()) * (usize::MAX / 2)),
                );
            }
            let visible = via
                .pads
                .iter()
                .any(|pad| display.primitive_visible(pad.layer, LayerPrimitive::Vias));
            if display.show_drills
                && visible
                && let Some(pad) = via.drill_shape.pad()
            {
                let distance = signed_pad_distance(&pad, query.point, owner, via.id, cancel)?;
                insert(
                    SelectedObject::Via(via.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    self.scene.pins.len() + source,
                );
            }
            if display.show_backdrills
                && via.backdrill.is_some()
                && via.pads.iter().any(|pad| {
                    pad.backdrill && display.primitive_visible(pad.layer, LayerPrimitive::Vias)
                })
                && let Some(pad) = via.pads.iter().find(|pad| pad.backdrill)
            {
                let distance = signed_pad_distance(pad, query.point, owner, via.id, cancel)?;
                insert(
                    SelectedObject::Via(via.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    usize::MAX / 4 + source,
                );
            }
        }
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        // SelectionCandidate's public metric is nonnegative; signed coverage
        // has already determined the order and must not be sorted again.
        Ok(hits
            .into_iter()
            .map(|hit| CanvasHit {
                anchor: SelectionAnchor {
                    object: hit.object,
                    layer: hit.layer,
                    category: hit.category,
                },
                distance_mm: hit.distance.max(0.0),
            })
            .collect())
    }
}

pub(crate) fn centerline_distance(segment: &Segment, point: Point) -> Result<f64, PathError> {
    crate::picking::segment_centerline_distance_mm(segment, point)
}
fn point_in_bounds(point: Point, bounds: Bounds) -> bool {
    point.x >= bounds.min.x
        && point.x <= bounds.max.x
        && point.y >= bounds.min.y
        && point.y <= bounds.max.y
}

fn paths_distance(
    paths: &[Vec<Segment>],
    point: Point,
    cancel: &CancellationToken,
) -> Result<f64, PathError> {
    let mut distance = f64::INFINITY;
    for edge in paths.iter().flatten() {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        distance = distance.min(centerline_distance(edge, point)?);
    }
    Ok(distance)
}
fn ring_distance(
    points: &[Point],
    offsets: &[u32],
    point: Point,
    cancel: &CancellationToken,
) -> Result<f64, PathError> {
    let mut distance = f64::INFINITY;
    for range in offsets.windows(2) {
        let ring = points
            .get(range[0] as usize..range[1] as usize)
            .ok_or(PathError::Invalid(ObjectId(0)))?;
        for (index, &a) in ring.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            distance = distance.min(crate::geometry::distance_to_line(
                point,
                a,
                ring[(index + 1) % ring.len()],
            ));
        }
    }
    Ok(distance)
}
fn signed_pad_distance(
    pad: &Pad,
    point: Point,
    owner: PadPlacement,
    id: ObjectId,
    cancel: &CancellationToken,
) -> Result<f64, PathError> {
    let Some(custom) = &pad.custom else {
        return Ok(pad.analytic_distance(point, owner).unwrap_or(f64::INFINITY));
    };
    let mut local = Point::new(
        point.x - owner.at.x - pad.offset.x,
        point.y - owner.at.y - pad.offset.y,
    )
    .rotate(-owner.angle);
    if owner.mirrored {
        local.y = -local.y;
    }
    let distance = if custom.paths.is_empty() {
        let mut distance = f64::INFINITY;
        for ring in &custom.contours {
            for (index, &a) in ring.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                distance = distance.min(crate::geometry::distance_to_line(
                    local,
                    a,
                    ring[(index + 1) % ring.len()],
                ));
            }
        }
        distance
    } else {
        paths_distance(&custom.paths, local, cancel)?
    };
    let inside = pad
        .custom_covers_fill(point, owner, cancel)
        .map_err(|_| PathError::Invalid(id))?
        .unwrap_or(false);
    Ok(if inside { -distance } else { distance })
}

#[cfg(test)]
mod contour_tests {
    use super::*;

    #[test]
    fn contour_bounds_preserve_exact_curves_holes_and_outline_hit_order() {
        use crate::model::{Arc as Curve, NetId, Zone, ZoneKind};
        let circle = |id, center: Point, radius| {
            vec![Segment {
                id: ObjectId(id),
                track_id: ObjectId(id),
                layer: LayerId(1),
                net: NetId(7),
                a: Point::new(center.x + radius, center.y),
                b: Point::new(center.x + radius, center.y),
                width: 0.0,
                bond_wire: None,
                arc: Some(Curve {
                    center,
                    radius,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                }),
            }]
        };
        let bounds = Bounds {
            min: Point::new(-20.0, -20.0),
            max: Point::new(20.0, 20.0),
        };
        let scene = Arc::new(BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![Zone {
                id: ObjectId(1),
                layer: LayerId(1),
                net: NetId(7),
                kind: ZoneKind::Unknown,
                paths: vec![
                    circle(1, Point::default(), 10.0),
                    circle(2, Point::new(4.0, 0.0), 2.0),
                    circle(3, Point::new(-4.0, 0.0), 1.0),
                ],
                mesh: Default::default(),
            }],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds,
            diagnostics: vec![],
        });
        let cancel = CancellationToken::default();
        let cached = BoardPickingIndex::build(Arc::clone(&scene), 1, &cancel).unwrap();
        let mut unpruned = BoardPickingIndex::build(scene, 1, &cancel).unwrap();
        unpruned.zone_paths[0].fill(Some(bounds));
        for opacity in [0.0, 0.25] {
            let display = BoardDisplay {
                copper_opacity: opacity,
                ..Default::default()
            };
            for scale in [1.0, 100.0] {
                for x in -24..=24 {
                    for y in -24..=24 {
                        let query =
                            PickQuery::new(Point::new(f64::from(x) * 0.5, f64::from(y) * 0.5), 0.1)
                                .unwrap();
                        let values = |index: &BoardPickingIndex| {
                            index
                                .query_visible_hits(
                                    query,
                                    scale,
                                    PickFilter::all(),
                                    &display,
                                    64,
                                    &cancel,
                                )
                                .unwrap()
                                .into_iter()
                                .map(|hit| (format!("{:?}", hit.anchor), hit.distance_mm.to_bits()))
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(
                            values(&cached),
                            values(&unpruned),
                            "point={:?} opacity={opacity} scale={scale}",
                            query.point
                        );
                    }
                }
            }
        }
    }
}
