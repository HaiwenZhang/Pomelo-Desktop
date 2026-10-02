//! CPU geometry shared by rendering and inspection.
pub mod copper;
pub mod pad;
pub mod units;

use crate::{
    model::{Bounds, ObjectId, Point, Segment},
    task::CancellationToken,
};

pub const COPPER_CHORD_TOLERANCE_MM: f64 = 0.00025;

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("GEOMETRY_CANCELLED")]
    Cancelled,
    #[error("GEOMETRY_INVALID object={0:?}")]
    Invalid(ObjectId),
    #[error("GEOMETRY_POINT_LIMIT actual={actual} limit={limit}")]
    PointLimit { actual: usize, limit: usize },
}

/// Analytic circular sweep, including a full circle when the two angles coincide.
pub fn arc_sweep(start: f64, end: f64, clockwise: bool) -> f64 {
    let mut difference = end - start;
    if !difference.is_finite() {
        return f64::NAN;
    }
    if difference.abs() > std::f64::consts::TAU {
        difference %= std::f64::consts::TAU;
    }
    if clockwise {
        while difference >= 0.0 {
            difference -= std::f64::consts::TAU;
        }
    } else {
        while difference <= 0.0 {
            difference += std::f64::consts::TAU;
        }
    }
    difference
}

/// Approximate copper contours while retaining stored endpoints and removing a closing duplicate.
/// Tracks and outlines keep their original analytic arcs; this only constructs fill contours.
pub fn flatten_path(
    path: &[Segment],
    tolerance: f64,
    max_points: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<Point>, PathError> {
    if cancellation.is_cancelled() {
        return Err(PathError::Cancelled);
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(PathError::Invalid(
            path.first().map_or(ObjectId(0), |edge| edge.id),
        ));
    }
    // Reserve at most the caller's point budget, including the initial small-vector allocation.
    let mut points = Vec::with_capacity(max_points.min(4));
    let mut work = 0_usize;
    let mut add = |point: Point, id: ObjectId| -> Result<(), PathError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(PathError::Invalid(id));
        }
        if points
            .last()
            .is_none_or(|last: &Point| last.distance(point) > 1e-9)
        {
            let actual = points.len().saturating_add(1);
            if actual > max_points {
                return Err(PathError::PointLimit {
                    actual,
                    limit: max_points,
                });
            }
            points.push(point);
        }
        Ok(())
    };
    for edge in path {
        if cancellation.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        add(edge.a, edge.id)?;
        if let Some(arc) = edge.arc {
            if ![arc.radius, arc.start, arc.sweep, arc.center.x, arc.center.y]
                .into_iter()
                .all(f64::is_finite)
                || arc.radius < 0.0
                || arc.sweep.abs() > std::f64::consts::TAU + 1e-12
            {
                return Err(PathError::Invalid(edge.id));
            }
            let step = 2.0
                * (1.0 - tolerance / arc.radius.max(tolerance))
                    .clamp(-1.0, 1.0)
                    .acos();
            let count = (arc.sweep.abs() / step.max(1e-5)).ceil().max(2.0) as usize;
            for index in 1..count {
                let angle = arc.start + (arc.sweep * index as f64) / count as f64;
                add(
                    Point::new(
                        arc.center.x + arc.radius * angle.cos(),
                        arc.center.y + arc.radius * angle.sin(),
                    ),
                    edge.id,
                )?;
                work += 1;
                if work.is_multiple_of(1024) && cancellation.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
            }
        }
        add(edge.b, edge.id)?;
    }
    if points.len() > 1 && points[0].distance(points[points.len() - 1]) < 1e-9 {
        points.pop();
    }
    if cancellation.is_cancelled() {
        return Err(PathError::Cancelled);
    }
    Ok(points)
}

impl Segment {
    /// Bounds of the directed sweep, stored endpoints and round stroke caps.
    pub fn bounds(&self) -> Option<Bounds> {
        self.bounds_with_width(self.width.max(0.0))
    }

    /// Bounds of the analytic boundary without its stroke width.
    pub fn centreline_bounds(&self) -> Option<Bounds> {
        self.bounds_with_width(0.0)
    }

    fn bounds_with_width(&self, width: f64) -> Option<Bounds> {
        if ![self.a.x, self.a.y, self.b.x, self.b.y, self.width, width]
            .into_iter()
            .all(f64::is_finite)
        {
            return None;
        }
        let mut bounds = Bounds::from_points([self.a, self.b])?;
        if let Some(arc) = self.arc {
            let tau = std::f64::consts::TAU;
            if ![
                arc.center.x,
                arc.center.y,
                arc.radius,
                arc.start,
                arc.sweep,
                arc.start + arc.sweep,
            ]
            .into_iter()
            .all(f64::is_finite)
                || arc.radius < 0.0
            {
                return None;
            }
            for candidate in -2..4 {
                let angle = match candidate {
                    -2 => arc.start,
                    -1 => arc.start + arc.sweep,
                    _ => f64::from(candidate) * std::f64::consts::FRAC_PI_2,
                };
                if candidate >= 0 {
                    let travel = (((angle - arc.start) * arc.sweep.signum()) % tau + tau) % tau;
                    if arc.sweep.abs() < tau && travel > arc.sweep.abs() + 1e-12 {
                        continue;
                    }
                }
                let point = Point::new(
                    arc.center.x + arc.radius * angle.cos(),
                    arc.center.y + arc.radius * angle.sin(),
                );
                if !point.x.is_finite() || !point.y.is_finite() {
                    return None;
                }
                bounds.include(point);
            }
        }
        let radius = width / 2.0;
        bounds.min.x -= radius;
        bounds.min.y -= radius;
        bounds.max.x += radius;
        bounds.max.y += radius;
        bounds.is_valid().then_some(bounds)
    }

    /// Centreline length; excludes vertical via length.
    pub fn length_mm(&self) -> f64 {
        self.arc.map_or_else(
            || self.a.distance(self.b),
            |arc| arc.radius * arc.sweep.abs(),
        )
    }
}

/// Union of exact path boundaries; excludes stroke width as copper contours do.
pub fn path_bounds(path: &[Segment]) -> Option<Bounds> {
    let mut result: Option<Bounds> = None;
    for edge in path {
        let bounds = edge.centreline_bounds()?;
        if let Some(result) = &mut result {
            result.include(bounds.min);
            result.include(bounds.max);
        } else {
            result = Some(bounds);
        }
    }
    result
}

pub fn distance_to_line(point: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let denominator = dx * dx + dy * dy;
    if denominator == 0.0 {
        return point.distance(a);
    }
    let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / denominator).clamp(0.0, 1.0);
    point.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Arc, LayerId, NetId, ObjectId};

    #[test]
    fn arc_length_uses_signed_sweep_magnitude() {
        let segment = Segment {
            bond_wire: None,
            id: ObjectId(1),
            track_id: ObjectId(1),
            layer: LayerId(1),
            net: NetId(1),
            a: Point::new(1.0, 0.0),
            b: Point::new(0.0, -1.0),
            width: 0.1,
            arc: Some(Arc {
                center: Point::default(),
                radius: 1.0,
                start: 0.0,
                sweep: -std::f64::consts::FRAC_PI_2,
            }),
        };
        assert_eq!(segment.length_mm(), std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn line_distance_clamps_to_endpoint() {
        assert_eq!(
            distance_to_line(Point::new(3.0, 0.0), Point::default(), Point::new(1.0, 0.0)),
            2.0
        );
    }
}
