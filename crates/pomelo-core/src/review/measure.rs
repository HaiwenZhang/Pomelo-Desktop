//! Two-point and polyline measurement in source board millimetres.
//!
//! Points are stored in board space; screen projection is only for display, so panning,
//! zooming and resizing never change a measured value. Unit conversion is presentation.
use crate::{
    display::BoardDisplay,
    geometry::PathError,
    interaction::picking_index::BoardPickingIndex,
    model::{BoardScene, Point},
    picking::{PickFilter, PickQuery},
    selection::SelectedObject,
    task::CancellationToken,
};

/// Polylines are bounded so a stuck input device cannot grow a measurement forever.
pub const MAX_POLYLINE_POINTS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    /// Exactly two points; the second click completes the measurement.
    Distance,
    /// Any number of points; an explicit finish command completes it.
    Polyline,
}

/// What a measured point was attached to. `Free` points are plain board coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    Free,
    SegmentEnd,
    PinCenter,
    ViaCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeasurePoint {
    pub position: Point,
    pub snap: SnapKind,
    /// Source object the point snapped to; `None` for free points.
    pub object: Option<SelectedObject>,
}

impl MeasurePoint {
    pub fn free(position: Point) -> Self {
        Self {
            position,
            snap: SnapKind::Free,
            object: None,
        }
    }
}

/// Differences and direction of one leg. The angle is counter-clockwise from +X in
/// board space, in degrees within (-180, 180]; a zero-length leg reports 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leg {
    pub dx: f64,
    pub dy: f64,
    pub distance: f64,
    pub angle_degrees: f64,
}

impl Leg {
    /// Axis-aligned travel between the endpoints, independent of the drawn path.
    pub fn manhattan_distance(self) -> f64 {
        self.dx.abs() + self.dy.abs()
    }

    pub fn between(a: Point, b: Point) -> Self {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let distance = crate::geometry::web_hypot(dx, dy);
        let angle_degrees = if distance == 0.0 {
            0.0
        } else {
            let angle = dy.atan2(dx).to_degrees();
            // atan2 returns -180 for (-x, -0.0); report the equivalent +180.
            if angle <= -180.0 {
                angle + 360.0
            } else {
                angle
            }
        };
        Self {
            dx,
            dy,
            distance,
            angle_degrees,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Measurement {
    pub kind: MeasureKind,
    pub points: Vec<MeasurePoint>,
}

impl Measurement {
    pub fn new(kind: MeasureKind) -> Self {
        Self {
            kind,
            points: Vec::new(),
        }
    }

    /// Append a point; returns false when the measurement cannot take more points.
    pub fn push(&mut self, point: MeasurePoint) -> bool {
        let limit = match self.kind {
            MeasureKind::Distance => 2,
            MeasureKind::Polyline => MAX_POLYLINE_POINTS,
        };
        if self.points.len() >= limit || !point.position.is_finite() {
            return false;
        }
        self.points.push(point);
        true
    }

    /// Remove the most recent point, for stepwise cancellation.
    pub fn pop(&mut self) -> Option<MeasurePoint> {
        self.points.pop()
    }

    /// A distance completes with its second point; a polyline needs two or more.
    pub fn is_complete_shape(&self) -> bool {
        self.points.len() >= 2
    }

    pub fn legs(&self) -> impl Iterator<Item = Leg> + '_ {
        self.points
            .windows(2)
            .map(|pair| Leg::between(pair[0].position, pair[1].position))
    }

    /// Accumulated centreline length of every leg.
    pub fn total_length(&self) -> f64 {
        self.legs().map(|leg| leg.distance).sum()
    }

    /// The span between the first and last point, independent of the path taken.
    pub fn span(&self) -> Option<Leg> {
        let first = self.points.first()?;
        let last = self.points.last()?;
        (self.points.len() >= 2).then(|| Leg::between(first.position, last.position))
    }

    /// The measurement as it would be with a provisional point appended, for previews.
    pub fn with_preview(&self, preview: MeasurePoint) -> Self {
        let mut preview_measurement = self.clone();
        preview_measurement.push(preview);
        preview_measurement
    }
}

/// Pointer position and the camera-derived reach of one snap query.
#[derive(Debug, Clone, Copy)]
pub struct SnapRequest {
    pub point: Point,
    pub radius_mm: f64,
    pub pixels_per_mm: f64,
    pub filter: PickFilter,
}

/// Snap a board point to the nearest exact feature under the pointer: track
/// endpoints within the radius, then pin and via centres of hit pads. Hidden and
/// filtered objects are not snap targets. Without a target the point stays free.
pub fn snap_point(
    scene: &BoardScene,
    index: &BoardPickingIndex,
    request: SnapRequest,
    display: &BoardDisplay,
    cancel: &CancellationToken,
) -> Result<MeasurePoint, PathError> {
    let SnapRequest {
        point,
        radius_mm,
        pixels_per_mm,
        filter,
    } = request;
    let Some(query) = PickQuery::new(point, radius_mm) else {
        return Ok(MeasurePoint::free(point));
    };
    let hits = index.query_visible_hits(query, pixels_per_mm, filter, display, 16, cancel)?;
    let mut best: Option<(f64, MeasurePoint)> = None;
    let mut consider = |position: Point, snap, object| {
        let distance = point.distance(position);
        if distance.is_finite() && best.is_none_or(|(current, _)| distance < current) {
            best = Some((
                distance,
                MeasurePoint {
                    position,
                    snap,
                    object: Some(object),
                },
            ));
        }
    };
    for hit in hits {
        cancel.check()?;
        let object = hit.anchor.object;
        match object {
            SelectedObject::Segment(id) => {
                if let Some(segment) = scene.segments.iter().find(|segment| segment.id == id) {
                    // Endpoints count only near the pointer, not anywhere along the stroke.
                    let reach = radius_mm + segment.width.max(0.0) * 0.5;
                    for end in [segment.a, segment.b] {
                        if point.distance(end) <= reach {
                            consider(end, SnapKind::SegmentEnd, object);
                        }
                    }
                }
            }
            SelectedObject::Pin(id) => {
                if let Some(pin) = scene.pins.iter().find(|pin| pin.id == id) {
                    consider(pin.at, SnapKind::PinCenter, object);
                }
            }
            SelectedObject::Via(id) => {
                if let Some(via) = scene.vias.iter().find(|via| via.id == id) {
                    consider(via.at, SnapKind::ViaCenter, object);
                }
            }
            SelectedObject::Zone(_) | SelectedObject::Drawing(_) | SelectedObject::Placement(_) => {
            }
        }
    }
    cancel.check()?;
    Ok(best.map_or_else(|| MeasurePoint::free(point), |(_, point)| point))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> MeasurePoint {
        MeasurePoint::free(Point::new(x, y))
    }

    #[test]
    fn leg_reports_differences_distance_and_counter_clockwise_angle() {
        let leg = Leg::between(Point::new(1.0, 1.0), Point::new(4.0, 5.0));
        assert_eq!((leg.dx, leg.dy, leg.distance), (3.0, 4.0, 5.0));
        assert_eq!(leg.manhattan_distance(), 7.0);
        assert!((leg.angle_degrees - 53.130_102_354_155_98).abs() < 1e-9);
        assert_eq!(
            Leg::between(Point::new(0.0, 0.0), Point::new(-2.0, -0.0)).angle_degrees,
            180.0
        );
        assert_eq!(
            Leg::between(Point::new(0.0, 0.0), Point::new(0.0, -3.0)).angle_degrees,
            -90.0
        );
        assert_eq!(
            Leg::between(Point::default(), Point::default()).angle_degrees,
            0.0
        );
    }

    #[test]
    fn distance_takes_exactly_two_points() {
        let mut measurement = Measurement::new(MeasureKind::Distance);
        assert!(measurement.push(point(0.0, 0.0)));
        assert!(!measurement.is_complete_shape());
        assert!(measurement.push(point(3.0, 4.0)));
        assert!(!measurement.push(point(9.0, 9.0)));
        assert_eq!(measurement.total_length(), 5.0);
        assert_eq!(measurement.span().unwrap().distance, 5.0);
    }

    #[test]
    fn polyline_accumulates_legs_and_keeps_span_separate() {
        let mut measurement = Measurement::new(MeasureKind::Polyline);
        for (x, y) in [(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)] {
            assert!(measurement.push(point(x, y)));
        }
        assert_eq!(measurement.total_length(), 7.0);
        assert_eq!(measurement.span().unwrap().distance, 5.0);
        assert_eq!(measurement.legs().count(), 2);
        assert_eq!(measurement.pop(), Some(point(3.0, 4.0)));
        assert_eq!(measurement.total_length(), 3.0);
    }

    #[test]
    fn preview_does_not_modify_the_committed_points() {
        let mut measurement = Measurement::new(MeasureKind::Distance);
        measurement.push(point(0.0, 0.0));
        let preview = measurement.with_preview(point(0.0, 2.0));
        assert_eq!(preview.total_length(), 2.0);
        assert_eq!(measurement.points.len(), 1);
    }

    #[test]
    fn non_finite_points_are_rejected() {
        let mut measurement = Measurement::new(MeasureKind::Polyline);
        assert!(!measurement.push(point(f64::NAN, 0.0)));
        assert!(measurement.points.is_empty());
    }

    #[test]
    fn polyline_point_count_is_bounded() {
        let mut measurement = Measurement::new(MeasureKind::Polyline);
        for index in 0..MAX_POLYLINE_POINTS {
            assert!(measurement.push(point(index as f64, 0.0)));
        }
        assert!(!measurement.push(point(0.0, 1.0)));
    }
}
