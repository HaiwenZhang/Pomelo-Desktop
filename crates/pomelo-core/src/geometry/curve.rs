//! View-dependent f64 contour refinement, matching Web curve-tessellator.
//! Clipped contours retain even/odd coverage; they need parity fans rather than earcut.

use crate::{
    geometry::copper::MeshError,
    model::{Arc, Bounds, Point, Segment},
    task::CancellationToken,
};

#[path = "curve/trig.rs"]
mod trig;

#[derive(Debug, Clone, Copy)]
pub struct CurveLimits {
    pub max_points: usize,
    /// Peak owned point buffers, including input/output during a clipping pass.
    pub max_allocation_bytes: usize,
}
impl Default for CurveLimits {
    fn default() -> Self {
        Self {
            max_points: 1_000_000,
            max_allocation_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Only visible monotone arc intervals are refined. Endpoint connectors remain exact.
pub fn tessellate_curve_ring(
    path: &[Segment],
    view: Bounds,
    tolerance: f64,
    limits: CurveLimits,
    cancel: &CancellationToken,
) -> Result<Vec<Point>, MeshError> {
    validate_view(view, cancel)?;
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(invalid("CURVE_TOLERANCE"));
    }
    let mut points = Vec::new();
    for edge in path {
        check_cancelled(cancel)?;
        push_unique(&mut points, edge.a, limits)?;
        if let Some(arc) = edge.arc {
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
                || arc.radius <= 0.0
                || arc.sweep.abs() > std::f64::consts::TAU + 1e-9
            {
                return Err(invalid("CURVE_ARC"));
            }
            let step = std::f64::consts::FRAC_PI_2;
            let end = arc.start + arc.sweep;
            let direction = if arc.sweep > 0.0 { 1.0 } else { -1.0 };
            let mut cardinal = if direction > 0.0 {
                (arc.start / step).floor() + 1.0
            } else {
                (arc.start / step).ceil() - 1.0
            };
            let mut start = arc.start;
            push_unique(&mut points, arc_point(arc, start), limits)?;
            loop {
                check_cancelled(cancel)?;
                let angle = cardinal * step;
                let next = if direction > 0.0 {
                    angle.min(end)
                } else {
                    angle.max(end)
                };
                if next == start && next != end {
                    return Err(invalid("CURVE_ANGLE_PRECISION"));
                }
                Refinement {
                    arc,
                    view,
                    tolerance,
                    limits,
                    cancel,
                    points: &mut points,
                }
                .interval(
                    start,
                    next,
                    arc_point(arc, start),
                    arc_point(arc, next),
                    0,
                )?;
                if next == end {
                    break;
                }
                start = next;
                let previous = cardinal;
                cardinal += direction;
                if cardinal == previous {
                    return Err(invalid("CURVE_ANGLE_PRECISION"));
                }
            }
        }
        push_unique(&mut points, edge.b, limits)?;
    }
    clip_ring(points, view, limits, cancel)
}

struct Refinement<'a> {
    arc: Arc,
    view: Bounds,
    tolerance: f64,
    limits: CurveLimits,
    cancel: &'a CancellationToken,
    points: &'a mut Vec<Point>,
}
impl Refinement<'_> {
    fn interval(
        &mut self,
        start: f64,
        end: f64,
        a: Point,
        b: Point,
        depth: u8,
    ) -> Result<(), MeshError> {
        check_cancelled(self.cancel)?;
        let bounds = Bounds {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        };
        let sine = trig::sin((end - start) / 4.0);
        let error = (2.0 * self.arc.radius) * (sine * sine);
        let mid = (start + end) / 2.0;
        if error > self.tolerance
            && depth < 52
            && mid != start
            && mid != end
            && overlaps(bounds, self.view)
        {
            let m = arc_point(self.arc, mid);
            self.interval(start, mid, a, m, depth + 1)?;
            self.interval(mid, end, m, b, depth + 1)?;
        } else {
            push_unique(self.points, b, self.limits)?;
        }
        Ok(())
    }
}

/// Sutherland–Hodgman clipping in doubles. Disjoint pieces may share clip-boundary
/// connectors; parity fans preserve their coverage without repairing topology.
pub fn clip_ring(
    mut points: Vec<Point>,
    view: Bounds,
    limits: CurveLimits,
    cancel: &CancellationToken,
) -> Result<Vec<Point>, MeshError> {
    validate_view(view, cancel)?;
    check_limit("curve_points", points.len(), limits.max_points)?;
    check_limit(
        "curve_bytes",
        points.capacity().saturating_mul(size_of::<Point>()),
        limits.max_allocation_bytes,
    )?;
    for point in &points {
        check_cancelled(cancel)?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(invalid("CURVE_COORDINATES"));
        }
    }
    for (axis, bound, sign) in [
        (0, view.min.x, 1.0),
        (0, view.max.x, -1.0),
        (1, view.min.y, 1.0),
        (1, view.max.y, -1.0),
    ] {
        let Some(&last) = points.last() else {
            break;
        };
        let mut output = Vec::new();
        let mut a = last;
        let coordinate = |p: Point| if axis == 0 { p.x } else { p.y };
        let mut inside_a = (coordinate(a) - bound) * sign >= 0.0;
        for &b in &points {
            check_cancelled(cancel)?;
            let inside_b = (coordinate(b) - bound) * sign >= 0.0;
            if inside_a != inside_b {
                let t = (bound - coordinate(a)) / (coordinate(b) - coordinate(a));
                let mut p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                if axis == 0 {
                    p.x = bound;
                } else {
                    p.y = bound;
                }
                push_point(&mut output, p, points.capacity(), limits)?;
            }
            if inside_b {
                push_point(&mut output, b, points.capacity(), limits)?;
            }
            a = b;
            inside_a = inside_b;
        }
        points = output;
    }
    check_cancelled(cancel)?;
    Ok(points)
}

fn arc_point(arc: Arc, angle: f64) -> Point {
    Point::new(
        arc.center.x + arc.radius * trig::cos(angle),
        arc.center.y + arc.radius * trig::sin(angle),
    )
}
fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}
fn validate_view(view: Bounds, cancel: &CancellationToken) -> Result<(), MeshError> {
    check_cancelled(cancel)?;
    if view.is_valid() {
        Ok(())
    } else {
        Err(invalid("CURVE_VIEW"))
    }
}
fn invalid(field: &'static str) -> MeshError {
    MeshError::Invalid { ring: 0, field }
}
fn check_cancelled(cancel: &CancellationToken) -> Result<(), MeshError> {
    if cancel.is_cancelled() {
        Err(MeshError::Cancelled)
    } else {
        Ok(())
    }
}
fn check_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), MeshError> {
    if actual > limit {
        Err(MeshError::Limit {
            resource,
            actual,
            limit,
        })
    } else {
        Ok(())
    }
}
fn push_unique(points: &mut Vec<Point>, p: Point, limits: CurveLimits) -> Result<(), MeshError> {
    if points.last() != Some(&p) {
        push_point(points, p, 0, limits)?;
    }
    Ok(())
}
fn push_point(
    points: &mut Vec<Point>,
    p: Point,
    other_capacity: usize,
    limits: CurveLimits,
) -> Result<(), MeshError> {
    if !p.x.is_finite() || !p.y.is_finite() {
        return Err(invalid("CURVE_COORDINATES"));
    }
    let required = points.len().saturating_add(1);
    check_limit("curve_points", required, limits.max_points)?;
    if required > points.capacity() {
        let max_capacity =
            (limits.max_allocation_bytes / size_of::<Point>()).saturating_sub(other_capacity);
        check_limit(
            "curve_bytes",
            required
                .saturating_add(other_capacity)
                .saturating_mul(size_of::<Point>()),
            limits.max_allocation_bytes,
        )?;
        let capacity = points
            .capacity()
            .saturating_mul(2)
            .max(16)
            .max(required)
            .min(limits.max_points)
            .min(max_capacity);
        points
            .try_reserve_exact(capacity - points.len())
            .map_err(|_| MeshError::Allocation)?;
    }
    points.push(p);
    Ok(())
}

#[cfg(test)]
#[path = "curve/tests.rs"]
mod tests;
