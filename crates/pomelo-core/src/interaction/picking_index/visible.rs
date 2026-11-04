//! Canvas picking uses signed coverage and the same submission rank as rendering.
use super::*;
use crate::{
    display::{DisplayCategory, LayerPrimitive},
    model::{LayerId, ObjectId, Pad, Point, Segment},
    pad::PadPlacement,
    picking::{ObjectHit, PickCategory},
};
use std::cmp::Ordering;

struct RankedHit<G> {
    group: G,
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
impl<G> RankedHit<G> {
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

/// One canvas query, shared by every per-category collector.
#[derive(Clone, Copy)]
struct VisibleQuery<'a> {
    query: PickQuery,
    /// `query` widened by the half-pixel minimum stroke for bounds tests.
    broad: PickQuery,
    query_bounds: Bounds,
    /// Millimetres per screen pixel.
    px: f64,
    filter: PickFilter,
    display: &'a BoardDisplay,
    cancel: &'a CancellationToken,
}

/// Best hit per selection group, ordered by signed distance, display rank and
/// source sequence, and truncated to `limit` only after grouping.
struct Ranking<'a, G, F> {
    hits: Vec<RankedHit<G>>,
    /// Objects repeat across pads/layers; resolve each object's group only once.
    groups: Vec<(SelectedObject, G)>,
    group: F,
    display: &'a BoardDisplay,
    tolerance_mm: f64,
    limit: usize,
}

impl<G, F> Ranking<'_, G, F>
where
    G: Copy + PartialEq,
    F: FnMut(SelectedObject) -> Result<G, PathError>,
{
    fn insert(
        &mut self,
        object: SelectedObject,
        distance: f64,
        layer: LayerId,
        category: DisplayCategory,
        sequence: usize,
    ) -> Result<(), PathError> {
        if !distance.is_finite() || distance > self.tolerance_mm {
            return Ok(());
        }
        let key = match self.groups.iter().find(|(known, _)| *known == object) {
            Some(&(_, key)) => key,
            None => {
                let key = (self.group)(object)?;
                self.groups.push((object, key));
                key
            }
        };
        let hit = RankedHit {
            group: key,
            object,
            layer,
            category,
            distance,
            rank: self.display.display_rank(layer, category),
            sequence,
        };
        if let Some(index) = self.hits.iter().position(|old| old.group == key) {
            if hit.compare(&self.hits[index]) != Ordering::Less {
                return Ok(());
            }
            self.hits.remove(index);
        }
        let position = self
            .hits
            .partition_point(|old| old.compare(&hit) != Ordering::Greater);
        if position < self.limit {
            self.hits.insert(position, hit);
            self.hits.truncate(self.limit);
        }
        Ok(())
    }

    /// SelectionCandidate's public metric is nonnegative; signed coverage
    /// has already determined the order and must not be sorted again.
    fn finish(self) -> Vec<CanvasHit> {
        self.hits
            .into_iter()
            .map(|hit| CanvasHit {
                anchor: SelectionAnchor {
                    object: hit.object,
                    layer: hit.layer,
                    category: hit.category,
                },
                distance_mm: hit.distance.max(0.0),
            })
            .collect()
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
        self.query_visible_hits_grouped(query, pixels_per_mm, filter, display, limit, cancel, Ok)
    }

    /// Like `query_visible_hits`, but deduplicates by `group(object)` before applying
    /// `limit`, keeping each group's highest-ranked hit as its anchor. One dense group
    /// therefore cannot exhaust the limit and hide other selectable groups below it.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors query_visible_hits plus a key"
    )]
    pub fn query_visible_hits_grouped<G: Copy + PartialEq>(
        &self,
        query: PickQuery,
        pixels_per_mm: f64,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &CancellationToken,
        group: impl FnMut(SelectedObject) -> Result<G, PathError>,
    ) -> Result<Vec<CanvasHit>, PathError> {
        cancel.check()?;
        if !pixels_per_mm.is_finite() || pixels_per_mm <= 0.0 {
            return Ok(Vec::new());
        }
        let limit = limit.min(64);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let px = 1.0 / pixels_per_mm;
        // Fine checks thicken strokes and glyph edges to at least one screen pixel,
        // so the broad phase must cover that extra half pixel or it drops valid hits.
        let broad = PickQuery {
            tolerance_mm: query.tolerance_mm + px * 0.5,
            ..query
        };
        let query_bounds = crate::model::Bounds {
            min: Point::new(
                broad.point.x - broad.tolerance_mm,
                broad.point.y - broad.tolerance_mm,
            ),
            max: Point::new(
                broad.point.x + broad.tolerance_mm,
                broad.point.y + broad.tolerance_mm,
            ),
        };
        let visible = VisibleQuery {
            query,
            broad,
            query_bounds,
            px,
            filter,
            display,
            cancel,
        };
        let mut ranking = Ranking {
            hits: Vec::new(),
            groups: Vec::new(),
            group,
            display,
            tolerance_mm: query.tolerance_mm,
            limit,
        };
        self.rank_segments(&visible, &mut ranking)?;
        self.rank_drawings(&visible, &mut ranking)?;
        self.rank_zones(&visible, &mut ranking)?;
        self.rank_placements(&visible, &mut ranking)?;
        self.rank_pins(&visible, &mut ranking)?;
        self.rank_vias(&visible, &mut ranking)?;
        cancel.check()?;
        Ok(ranking.finish())
    }

    fn rank_placements<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        // Placement regions follow the existing Pins picking switch; toggling
        // Quickview's temporary Pin visibility does not hide the component body.
        if !visible.filter.contains(PickCategory::Pin) {
            return Ok(());
        }
        for source in self
            .placements
            .query(visible.query_bounds, visible.cancel)?
        {
            let component = &self.scene.components[source];
            let Some(body) = component.body.as_ref().filter(|body| {
                visible
                    .display
                    .category_visible(body.layer, DisplayCategory::Placement)
            }) else {
                continue;
            };
            if let Some(distance) = super::placements::distance(
                body,
                visible.query.point,
                visible.px * 0.5,
                visible.cancel,
            )? {
                ranking.insert(
                    SelectedObject::Placement(component.id),
                    distance,
                    body.layer,
                    DisplayCategory::Placement,
                    source,
                )?;
            }
        }
        Ok(())
    }
    /// Segments, through the bounding-volume hierarchy.
    fn rank_segments<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        let VisibleQuery {
            query,
            broad,
            px,
            filter,
            display,
            cancel,
            ..
        } = *visible;
        let mut pending = vec![1usize];
        while let Some(index) = pending.pop() {
            cancel.check()?;
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
            ranking.insert(
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
            )?;
        }
        Ok(())
    }
    /// Drawing strokes and text glyph quads.
    fn rank_drawings<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        let VisibleQuery {
            query,
            query_bounds,
            px,
            filter,
            display,
            cancel,
            ..
        } = *visible;
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
                        if !display.show_texts
                            || entry.text_height.is_some_and(|height| {
                                !display.object_filter.visible(
                                    crate::display::filter::FilterKind::Text,
                                    height,
                                    1.0 / px,
                                )
                            })
                        {
                            continue;
                        }
                        let mut distance = f64::INFINITY;
                        for quad in &self.drawings.glyphs[start..start + count] {
                            cancel.check()?;
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
                ranking.insert(
                    SelectedObject::Drawing(entry.owner),
                    distance,
                    entry.layer,
                    category,
                    entry.sequence,
                )?;
            }
        }
        Ok(())
    }
    /// Copper fills (signed coverage) or their outlines when unfilled.
    fn rank_zones<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        let VisibleQuery {
            query,
            broad,
            query_bounds,
            px,
            filter,
            display,
            cancel,
            ..
        } = *visible;
        for source in self.zone_bounds.query(query_bounds, cancel)? {
            let zone = &self.scene.zones[source];
            cancel.check()?;
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
            if bounds.is_none_or(|bounds| {
                !broad.intersects(bounds)
                    || !display.object_filter.bounds_visible(
                        crate::display::filter::FilterKind::Copper,
                        bounds,
                        1.0 / px,
                    )
            }) {
                continue;
            }
            let contains = if zone.paths.is_empty() {
                zone.mesh
                    .covers_fill_where(query.point, cancel, |ring| {
                        zone.mesh.ring_bounds.get(ring).is_none_or(|b| {
                            display.object_filter.bounds_visible(
                                crate::display::filter::FilterKind::Void,
                                *b,
                                1.0 / px,
                            )
                        })
                    })
                    .map_err(|_| PathError::Invalid(zone.id))?
            } else {
                let mut covered =
                    crate::geometry::path_contains(&zone.paths[0], query.point, cancel)?;
                for (path, bounds) in zone.paths.iter().zip(paths_bounds).skip(1) {
                    if !covered {
                        break;
                    }
                    if bounds.is_some_and(|b| {
                        !display.object_filter.bounds_visible(
                            crate::display::filter::FilterKind::Void,
                            b,
                            1.0 / px,
                        )
                    }) {
                        continue;
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
                        |ring| {
                            ring == 0
                                || zone.mesh.ring_bounds.get(ring).is_none_or(|b| {
                                    display.object_filter.bounds_visible(
                                        crate::display::filter::FilterKind::Void,
                                        *b,
                                        1.0 / px,
                                    )
                                })
                        },
                    )?
                } else {
                    let nearby = PickQuery {
                        point: query.point,
                        tolerance_mm: query.tolerance_mm + 0.65 * px,
                    };
                    let mut distance = f64::INFINITY;
                    for (ring, (path, bounds)) in zone.paths.iter().zip(paths_bounds).enumerate() {
                        if ring > 0
                            && bounds.is_some_and(|b| {
                                !display.object_filter.bounds_visible(
                                    crate::display::filter::FilterKind::Void,
                                    b,
                                    1.0 / px,
                                )
                            })
                        {
                            continue;
                        }
                        cancel.check()?;
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
            ranking.insert(
                SelectedObject::Zone(zone.id),
                distance,
                zone.layer,
                category,
                source,
            )?;
        }
        Ok(())
    }
    /// Pin pads and pin drills.
    fn rank_pins<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        let VisibleQuery {
            query,
            broad,
            query_bounds,
            px,
            filter,
            display,
            cancel,
            ..
        } = *visible;
        // PrimitiveBatchBuilder submits pin pads before via pads. Within each
        // category analytic pads precede custom fills; last source entry wins.
        for source in self.pins.query(query_bounds, cancel)? {
            let pin = &self.scene.pins[source];
            cancel.check()?;
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
                    || !display.object_filter.pad_visible(
                        crate::display::filter::FilterKind::Pin,
                        pad,
                        owner,
                        1.0 / px,
                    )
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
                ranking.insert(
                    SelectedObject::Pin(pin.id),
                    distance,
                    pad.layer,
                    category,
                    sequence.saturating_add(usize::from(pad.custom.is_some()) * (usize::MAX / 2)),
                )?;
            }
            if display.show_drills
                && let Some(pad) = pin.drill_shape.pad()
            {
                let distance = signed_pad_distance(&pad, query.point, owner, pin.id, cancel)?;
                ranking.insert(
                    SelectedObject::Pin(pin.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    self.scene.pins.len() + source,
                )?;
            }
        }
        Ok(())
    }
    /// Via pads, via drills and backdrills.
    fn rank_vias<G: Copy + PartialEq>(
        &self,
        visible: &VisibleQuery<'_>,
        ranking: &mut Ranking<'_, G, impl FnMut(SelectedObject) -> Result<G, PathError>>,
    ) -> Result<(), PathError> {
        let VisibleQuery {
            query,
            broad,
            query_bounds,
            px,
            filter,
            display,
            cancel,
            ..
        } = *visible;
        for source in self.vias.query(query_bounds, cancel)? {
            let via = &self.scene.vias[source];
            cancel.check()?;
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
                    || !display.object_filter.pad_visible(
                        crate::display::filter::FilterKind::Via,
                        pad,
                        owner,
                        1.0 / px,
                    )
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
                ranking.insert(
                    SelectedObject::Via(via.id),
                    distance,
                    pad.layer,
                    DisplayCategory::Via,
                    source
                        .saturating_mul(65536)
                        .saturating_add(pad_index)
                        .saturating_add(usize::from(pad.custom.is_some()) * (usize::MAX / 2)),
                )?;
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
                ranking.insert(
                    SelectedObject::Via(via.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    self.scene.pins.len() + source,
                )?;
            }
            if display.show_backdrills
                && via.backdrill.is_some()
                && via.pads.iter().any(|pad| {
                    pad.backdrill && display.primitive_visible(pad.layer, LayerPrimitive::Vias)
                })
                && let Some(pad) = via.pads.iter().find(|pad| pad.backdrill)
            {
                let distance = signed_pad_distance(pad, query.point, owner, via.id, cancel)?;
                ranking.insert(
                    SelectedObject::Via(via.id),
                    distance,
                    LayerId::UNASSIGNED,
                    DisplayCategory::Drill,
                    usize::MAX / 4 + source,
                )?;
            }
        }
        Ok(())
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
        cancel.check()?;
        distance = distance.min(centerline_distance(edge, point)?);
    }
    Ok(distance)
}
fn ring_distance(
    points: &[Point],
    offsets: &[u32],
    point: Point,
    cancel: &CancellationToken,
    include: impl Fn(usize) -> bool,
) -> Result<f64, PathError> {
    let mut distance = f64::INFINITY;
    for (ring_index, range) in offsets.windows(2).enumerate() {
        let ring = points
            .get(range[0] as usize..range[1] as usize)
            .ok_or(PathError::Invalid(ObjectId(0)))?;
        if !include(ring_index) {
            continue;
        }
        for (index, &a) in ring.iter().enumerate() {
            cancel.check()?;
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
                cancel.check()?;
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
