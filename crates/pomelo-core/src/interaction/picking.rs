//! Picking eligibility; category filters do not change display visibility.

use crate::{display::BoardDisplay, model::LayerId, selection::SelectedObject};
use crate::{
    geometry::{PathError, distance_to_line},
    model::{Point, Segment},
};

/// Distance to the filled round-cap stroke, in board millimetres.
/// Zero means inside the copper; callers convert pixel tolerance with their camera.
pub fn segment_distance_mm(segment: &Segment, point: Point) -> Result<f64, PathError> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || segment.width < 0.0
        || segment.bounds().is_none()
    {
        return Err(PathError::Invalid(segment.id));
    }
    let centerline = if let Some(arc) = segment.arc {
        let tau = std::f64::consts::TAU;
        if arc.sweep.abs() > tau + 1e-12 {
            return Err(PathError::Invalid(segment.id));
        }
        let dx = point.x - arc.center.x;
        let dy = point.y - arc.center.y;
        let angle = dy.atan2(dx);
        let travel = ((angle - arc.start.rem_euclid(tau)) * arc.sweep.signum()).rem_euclid(tau);
        if arc.sweep.abs() >= tau - 1e-12 || (arc.sweep != 0.0 && travel <= arc.sweep.abs()) {
            (dx.hypot(dy) - arc.radius).abs()
        } else {
            point.distance(segment.a).min(point.distance(segment.b))
        }
    } else {
        distance_to_line(point, segment.a, segment.b)
    };
    if !centerline.is_finite() {
        return Err(PathError::Invalid(segment.id));
    }
    Ok((centerline - segment.width / 2.0).max(0.0))
}

#[derive(Debug, Clone, Copy)]
pub struct PickQuery {
    pub(crate) point: Point,
    pub(crate) tolerance_mm: f64,
}

struct PinQueryOptions {
    filter: PickFilter,
    limit: usize,
    include_custom: bool,
}

fn pad_distance(
    pad: &crate::model::Pad,
    point: Point,
    owner: crate::pad::PadPlacement,
    id: crate::model::ObjectId,
    cancel: &crate::task::CancellationToken,
) -> Result<Option<f64>, PathError> {
    if pad.custom.is_some() {
        return pad
            .custom_covers_fill(point, owner, cancel)
            .map(|coverage| (coverage == Some(true)).then_some(0.0))
            .map_err(|error| match error {
                crate::copper::MeshError::Cancelled => PathError::Cancelled,
                _ => PathError::Invalid(id),
            });
    }
    if !pad.kind.is_analytic() {
        return Ok(None);
    }
    let value = pad
        .analytic_distance(point, owner)
        .ok_or(PathError::Invalid(id))?;
    if !value.is_finite() {
        return Err(PathError::Invalid(id));
    }
    Ok(Some(value.max(0.0)))
}

impl PickQuery {
    /// Reference scan of stored zone fill, excluding the union of holes.
    /// Curved boundaries use the mesh approximation; edge tolerance is not applied.
    /// Eligibility precedes truncation and equal-distance ties retain source order.
    pub fn zones_where<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
        eligible: impl Fn(&crate::model::Zone) -> bool,
    ) -> Result<Vec<ZoneHit<'a>>, PathError> {
        let limit = limit.min(64);
        let mut hits = Vec::new();
        for zone in &scene.zones {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if limit == 0
                || !filter.contains(PickCategory::Zone)
                || !display.show_copper
                || !display.layer_visible(zone.layer)
                || !eligible(zone)
            {
                continue;
            }
            let covered =
                zone.mesh
                    .covers_fill(self.point, cancel)
                    .map_err(|error| match error {
                        crate::copper::MeshError::Cancelled => PathError::Cancelled,
                        _ => PathError::Invalid(zone.id),
                    })?;
            if covered && hits.len() < limit {
                hits.push(ZoneHit {
                    zone,
                    distance_mm: 0.0,
                });
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }

    /// Reference via scan; pad layer visibility is independent of drill display.
    pub fn vias_where<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
        eligible: impl Fn(&crate::model::Via) -> bool,
    ) -> Result<Vec<ViaHit<'a>>, PathError> {
        let limit = limit.min(64);
        let mut hits: Vec<ViaHit<'a>> = Vec::new();
        for via in &scene.vias {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if limit == 0 || !filter.contains(PickCategory::Via) || !eligible(via) {
                continue;
            }
            let owner = crate::pad::PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            let mut distance = f64::INFINITY;
            for pad in via.pads.iter() {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if !display.layer_visible(pad.layer) {
                    continue;
                }
                if let Some(value) = pad_distance(pad, self.point, owner, via.id, cancel)? {
                    distance = distance.min(value);
                }
            }
            if distance > self.tolerance_mm {
                continue;
            }
            let position = hits.partition_point(|hit| hit.distance_mm <= distance);
            if position >= limit {
                continue;
            }
            if hits.len() == limit {
                hits.pop();
            }
            hits.insert(
                position,
                ViaHit {
                    via,
                    distance_mm: distance,
                },
            );
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }

    /// Query analytic pin pads only. Custom contours require a separate path query.
    /// One owner appears once even when multiple visible pads overlap the pointer.
    pub fn analytic_pins<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
    ) -> Result<Vec<PinHit<'a>>, PathError> {
        self.pins_impl(
            scene,
            display,
            cancel,
            PinQueryOptions {
                filter,
                limit,
                include_custom: false,
            },
            |_| true,
        )
    }

    /// Analytic pads use distance tolerance; custom pads currently require fill coverage.
    pub fn pins<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
    ) -> Result<Vec<PinHit<'a>>, PathError> {
        self.pins_where(scene, filter, display, limit, cancel, |_| true)
    }

    /// Mode eligibility is evaluated before candidate truncation.
    pub fn pins_where<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
        eligible: impl Fn(&crate::model::Pin) -> bool,
    ) -> Result<Vec<PinHit<'a>>, PathError> {
        self.pins_impl(
            scene,
            display,
            cancel,
            PinQueryOptions {
                filter,
                limit,
                include_custom: true,
            },
            eligible,
        )
    }

    fn pins_impl<'a>(
        self,
        scene: &'a crate::model::BoardScene,
        display: &BoardDisplay,
        cancel: &crate::task::CancellationToken,
        options: PinQueryOptions,
        eligible: impl Fn(&crate::model::Pin) -> bool,
    ) -> Result<Vec<PinHit<'a>>, PathError> {
        let limit = options.limit.min(64);
        let mut hits: Vec<PinHit<'a>> = Vec::new();
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if limit == 0 || !options.filter.contains(PickCategory::Pin) || !eligible(pin) {
                continue;
            }
            let owner = crate::pad::PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            let mut distance = f64::INFINITY;
            for pad in &pin.pads {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if !display.layer_visible(pad.layer) {
                    continue;
                }
                if pad.custom.is_some() && !options.include_custom {
                    continue;
                }
                if let Some(value) = pad_distance(pad, self.point, owner, pin.id, cancel)? {
                    distance = distance.min(value);
                }
            }
            if distance > self.tolerance_mm {
                continue;
            }
            let position = hits.partition_point(|hit| hit.distance_mm <= distance);
            if position >= limit {
                continue;
            }
            if hits.len() == limit {
                hits.pop();
            }
            hits.insert(
                position,
                PinHit {
                    pin,
                    distance_mm: distance,
                },
            );
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }

    pub(crate) fn intersects(self, bounds: crate::model::Bounds) -> bool {
        self.point.x >= bounds.min.x - self.tolerance_mm
            && self.point.x <= bounds.max.x + self.tolerance_mm
            && self.point.y >= bounds.min.y - self.tolerance_mm
            && self.point.y <= bounds.max.y + self.tolerance_mm
    }
    /// Input is local to the canvas in logical pixels; the host removes its origin.
    /// DPI must not be applied again because Camera already uses logical pixels.
    pub fn from_viewport(
        camera: crate::interaction::Camera,
        point: Point,
        size: Point,
        tolerance_pixels: f64,
    ) -> Option<Self> {
        if !size.x.is_finite()
            || !size.y.is_finite()
            || size.x <= 0.0
            || size.y <= 0.0
            || point.x < 0.0
            || point.y < 0.0
            || point.x > size.x
            || point.y > size.y
            || !camera.pixels_per_mm.is_finite()
            || camera.pixels_per_mm <= 0.0
            || !tolerance_pixels.is_finite()
            || tolerance_pixels < 0.0
        {
            return None;
        }
        Self::new(
            camera.view_to_board(point, size.x, size.y),
            tolerance_pixels / camera.pixels_per_mm,
        )
    }

    pub fn new(point: Point, tolerance_mm: f64) -> Option<Self> {
        (point.x.is_finite()
            && point.y.is_finite()
            && tolerance_mm.is_finite()
            && tolerance_mm >= 0.0)
            .then_some(Self {
                point,
                tolerance_mm,
            })
    }

    /// Reference scan for indexed-query parity; keeps at most 64 nearest candidates.
    /// Source order breaks equal-distance ties and cancellation discards partial results.
    pub fn segments<'a>(
        self,
        segments: &'a [Segment],
        filter: PickFilter,
        display: &BoardDisplay,
        limit: usize,
        cancel: &crate::task::CancellationToken,
    ) -> Result<Vec<SegmentHit<'a>>, PathError> {
        let limit = limit.min(64);
        let mut hits: Vec<SegmentHit<'a>> = Vec::new();
        for segment in segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if limit == 0
                || !filter.allows(
                    SelectedObject::Segment(segment.id),
                    &[segment.layer],
                    display,
                )
            {
                continue;
            }
            let distance_mm = segment_distance_mm(segment, self.point)?;
            if distance_mm > self.tolerance_mm {
                continue;
            }
            let position = hits.partition_point(|hit| hit.distance_mm <= distance_mm);
            if position >= limit {
                continue;
            }
            if hits.len() == limit {
                hits.pop();
            }
            hits.insert(
                position,
                SegmentHit {
                    segment,
                    distance_mm,
                },
            );
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(hits)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SegmentHit<'a> {
    pub segment: &'a Segment,
    pub distance_mm: f64,
}

/// Typed source identity and distance shared by all copper candidate categories.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectHit {
    pub object: SelectedObject,
    pub distance_mm: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PinHit<'a> {
    pub pin: &'a crate::model::Pin,
    pub distance_mm: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ViaHit<'a> {
    pub via: &'a crate::model::Via,
    pub distance_mm: f64,
}

#[derive(Debug)]
pub struct ZoneHit<'a> {
    pub zone: &'a crate::model::Zone,
    pub distance_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PickCategory {
    Segment = 0,
    Pin = 1,
    Via = 2,
    Zone = 3,
}

impl From<SelectedObject> for PickCategory {
    fn from(value: SelectedObject) -> Self {
        match value {
            SelectedObject::Segment(_) => Self::Segment,
            SelectedObject::Pin(_) => Self::Pin,
            SelectedObject::Via(_) => Self::Via,
            SelectedObject::Zone(_) => Self::Zone,
        }
    }
}

/// Independent category switches, including an explicit empty selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct PickFilter(u8);

impl TryFrom<u8> for PickFilter {
    type Error = &'static str;
    fn try_from(bits: u8) -> Result<Self, Self::Error> {
        if bits & !0b1111 != 0 {
            return Err("PICK_FILTER_UNKNOWN_BITS");
        }
        Ok(Self(bits))
    }
}

impl From<PickFilter> for u8 {
    fn from(filter: PickFilter) -> Self {
        filter.0
    }
}

impl Default for PickFilter {
    fn default() -> Self {
        Self::all()
    }
}

impl PickFilter {
    pub const fn all() -> Self {
        Self(0b1111)
    }

    pub const fn none() -> Self {
        Self(0)
    }

    pub const fn contains(self, category: PickCategory) -> bool {
        self.0 & (1 << category as u8) != 0
    }

    pub fn set(&mut self, category: PickCategory, enabled: bool) {
        let bit = 1 << category as u8;
        if enabled {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }

    /// Multi-layer pads are eligible if at least one occupied layer is visible.
    /// Empty layer scopes have no displayed geometry and cannot be picked.
    pub fn allows(
        self,
        object: SelectedObject,
        layers: &[LayerId],
        display: &BoardDisplay,
    ) -> bool {
        self.contains(object.into()) && layers.iter().any(|layer| display.layer_visible(*layer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ObjectId;

    #[test]
    fn viewport_query_preserves_physical_target_across_flip_and_zoom() {
        let size = Point::new(800.0, 600.0);
        let target = Point::new(12.0, -3.0);
        for flipped in [false, true] {
            for scale in [2.0, 10.0, 100.0] {
                let camera = crate::interaction::Camera {
                    center: Point::new(10.0, -2.0),
                    pixels_per_mm: scale,
                    flipped,
                };
                let pixel = camera.board_to_view(target, size.x, size.y);
                let query = PickQuery::from_viewport(camera, pixel, size, 5.0).unwrap();
                assert!(query.point.distance(target) < 1e-12);
                assert!((query.tolerance_mm - 5.0 / scale).abs() < 1e-12);
            }
        }
        let camera = crate::interaction::Camera::default();
        assert!(PickQuery::from_viewport(camera, Point::new(-1.0, 0.0), size, 5.0).is_none());
        assert!(
            PickQuery::from_viewport(camera, Point::default(), Point::default(), 5.0).is_none()
        );
        assert!(PickQuery::from_viewport(camera, Point::new(f64::NAN, 0.0), size, 5.0).is_none());
    }

    fn segment() -> Segment {
        Segment {
            id: ObjectId(7),
            track_id: ObjectId(8),
            layer: LayerId(1),
            net: crate::model::NetId(1),
            a: Point::new(0.0, 0.0),
            b: Point::new(2.0, 0.0),
            width: 0.2,
            arc: None,
            bond_wire: None,
        }
    }

    #[test]
    fn line_stroke_includes_round_caps_and_degenerate_disk() {
        let mut line = segment();
        assert_eq!(
            segment_distance_mm(&line, Point::new(1.0, 0.05)).unwrap(),
            0.0
        );
        assert!((segment_distance_mm(&line, Point::new(2.3, 0.0)).unwrap() - 0.2).abs() < 1e-12);
        line.b = line.a;
        assert!((segment_distance_mm(&line, Point::new(0.0, 0.3)).unwrap() - 0.2).abs() < 1e-12);
    }

    #[test]
    fn candidate_scan_keeps_nearest_stable_visible_results_and_cancels() {
        let mut lines = vec![segment(); 100];
        for (index, line) in lines.iter_mut().enumerate() {
            line.id = ObjectId(index as u32);
        }
        lines[0].a.y = 0.4;
        lines[0].b.y = 0.4;
        lines[1].layer = LayerId(2);
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(LayerId(2));
        let query = PickQuery::new(Point::new(1.0, 0.0), 0.5).unwrap();
        let cancel = crate::task::CancellationToken::default();
        let hits = query
            .segments(&lines, PickFilter::all(), &display, 2, &cancel)
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.segment.id).collect::<Vec<_>>(),
            vec![ObjectId(2), ObjectId(3)]
        );
        assert_eq!(
            query
                .segments(&lines, PickFilter::all(), &display, usize::MAX, &cancel)
                .unwrap()
                .len(),
            64
        );
        assert!(
            query
                .segments(&lines, PickFilter::none(), &display, 2, &cancel)
                .unwrap()
                .is_empty()
        );
        assert!(PickQuery::new(Point::default(), -1.0).is_none());
        cancel.cancel();
        assert!(matches!(
            query.segments(&[], PickFilter::all(), &display, 0, &cancel),
            Err(PathError::Cancelled)
        ));
    }

    #[test]
    fn directed_arc_wraps_angles_and_does_not_pick_the_missing_sector() {
        let mut curve = segment();
        curve.arc = Some(crate::model::Arc {
            center: Point::default(),
            radius: 2.0,
            start: std::f64::consts::FRAC_PI_2,
            sweep: -std::f64::consts::PI,
        });
        curve.a = Point::new(0.0, 2.0);
        curve.b = Point::new(0.0, -2.0);
        assert_eq!(
            segment_distance_mm(&curve, Point::new(2.0, 0.0)).unwrap(),
            0.0
        );
        assert!(segment_distance_mm(&curve, Point::new(-2.0, 0.0)).unwrap() > 2.7);
        curve.arc.as_mut().unwrap().sweep = std::f64::consts::TAU;
        assert_eq!(
            segment_distance_mm(&curve, Point::new(-2.0, 0.0)).unwrap(),
            0.0
        );
        assert!((segment_distance_mm(&curve, Point::default()).unwrap() - 1.9).abs() < 1e-12);
    }

    #[test]
    fn invalid_geometry_retains_source_identity() {
        let mut line = segment();
        line.width = -1.0;
        assert!(matches!(
            segment_distance_mm(&line, Point::default()),
            Err(PathError::Invalid(ObjectId(7)))
        ));
        line.width = 0.2;
        assert!(matches!(
            segment_distance_mm(&line, Point::new(f64::NAN, 0.0)),
            Err(PathError::Invalid(ObjectId(7)))
        ));
    }

    #[test]
    fn every_category_combination_preserves_independent_switches() {
        let categories = [
            PickCategory::Segment,
            PickCategory::Pin,
            PickCategory::Via,
            PickCategory::Zone,
        ];
        for mask in 0u8..16 {
            let mut filter = PickFilter::none();
            for (index, category) in categories.into_iter().enumerate() {
                filter.set(category, mask & (1 << index) != 0);
            }
            for (index, category) in categories.into_iter().enumerate() {
                assert_eq!(filter.contains(category), mask & (1 << index) != 0);
            }
            filter.set(PickCategory::Pin, true);
            filter.set(PickCategory::Pin, false);
            assert!(!filter.contains(PickCategory::Pin));
            assert_eq!(filter.contains(PickCategory::Via), mask & 4 != 0);
        }
    }

    #[test]
    fn hidden_layers_override_categories_without_mutating_display() {
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(LayerId(1));
        let object = SelectedObject::Pin(ObjectId(7));
        assert!(!PickFilter::all().allows(object, &[LayerId(1)], &display));
        assert!(PickFilter::all().allows(object, &[LayerId(1), LayerId(2)], &display));
        assert!(!PickFilter::all().allows(object, &[], &display));
        assert!(!PickFilter::none().allows(object, &[LayerId(2)], &display));
        assert!(display.layer_visible(LayerId(2)));
    }
}
