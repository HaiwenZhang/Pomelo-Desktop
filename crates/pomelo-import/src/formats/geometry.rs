//! Analytic source geometry helpers used by native format adapters.
use crate::{ImportContext, ImportError};
use pomelo_core::{
    geometry::{arc_sweep, flatten_path, path_bounds},
    model::*,
};
use std::sync::Arc as Shared;
pub(super) fn edge(
    a: Point,
    b: Point,
    width: f64,
    center: Option<Point>,
    clockwise: bool,
) -> Segment {
    let arc = center.map(|center| {
        let start = (a.y - center.y).atan2(a.x - center.x);
        let end = (b.y - center.y).atan2(b.x - center.x);
        Arc {
            center,
            radius: a.distance(center),
            start,
            sweep: arc_sweep(start, end, clockwise),
        }
    });
    Segment {
        id: ObjectId(0),
        track_id: ObjectId(0),
        layer: LayerId::UNASSIGNED,
        net: NetId(0),
        a,
        b,
        width,
        arc,
        bond_wire: None,
    }
}
pub(super) fn transform(edge: &Segment, at: Point, angle: f64, mirror: bool) -> Segment {
    let point = |p: Point| {
        let p = Point::new(p.x, if mirror { -p.y } else { p.y }).rotate(angle);
        Point::new(at.x + p.x, at.y + p.y)
    };
    let mut result = edge.clone();
    result.a = point(edge.a);
    result.b = point(edge.b);
    result.arc = edge.arc.map(|a| Arc {
        center: point(a.center),
        radius: a.radius,
        start: if mirror {
            angle - a.start
        } else {
            angle + a.start
        },
        sweep: if mirror { -a.sweep } else { a.sweep },
    });
    result
}
pub(super) fn pad_paths(pad: &Pad) -> Vec<Vec<Segment>> {
    if let Some(custom) = &pad.custom {
        if !custom.paths.is_empty() {
            return custom.paths.clone();
        }
        return custom
            .contours
            .iter()
            .map(|r| {
                r.iter()
                    .enumerate()
                    .map(|(i, &a)| edge(a, r[(i + 1) % r.len()], 0.0, None, false))
                    .collect()
            })
            .collect();
    }
    let w = pad.width * 0.5;
    let h = pad.height * 0.5;
    if pad.kind.0 == 2 || pad.kind.0 == 25 {
        let circle = |r| {
            vec![edge(
                Point::new(r, 0.0),
                Point::new(r, 0.0),
                0.0,
                Some(Point::default()),
                false,
            )]
        };
        return if let Some(inner) = pad.inner_diameter {
            vec![circle(w), circle(inner * 0.5)]
        } else {
            vec![circle(w)]
        };
    }
    let r = pad.corner_radius();
    let points = [
        Point::new(w, h - r),
        Point::new(w - r, h),
        Point::new(-w + r, h),
        Point::new(-w, h - r),
        Point::new(-w, -h + r),
        Point::new(-w + r, -h),
        Point::new(w - r, -h),
        Point::new(w, -h + r),
    ];
    let centers = [
        Point::new(w - r, h - r),
        Point::new(-w + r, h - r),
        Point::new(-w + r, -h + r),
        Point::new(w - r, -h + r),
    ];
    let mut path = Vec::new();
    for i in 0..8 {
        let a = points[i];
        let b = points[(i + 1) % 8];
        if a == b {
            continue;
        }
        let center = if i % 2 == 0 && r > 0.0 && pad.kind.0 != 3 && pad.kind.0 != 28 {
            Some(centers[i / 2])
        } else {
            None
        };
        path.push(edge(a, b, 0.0, center, false));
    }
    vec![path]
}
pub(super) fn rings(
    paths: &[Vec<Segment>],
    context: &ImportContext<'_>,
) -> Result<Vec<Vec<Point>>, ImportError> {
    paths
        .iter()
        .map(|p| {
            flatten_path(p, 0.001, 8_000_000, context.cancellation).map_err(|e| {
                if context.cancellation.is_cancelled() {
                    ImportError::Cancelled
                } else {
                    ImportError::Format {
                        format: "geometry".into(),
                        details: e.to_string(),
                    }
                }
            })
        })
        .collect()
}
pub(super) fn custom(
    paths: Vec<Vec<Segment>>,
    context: &ImportContext<'_>,
) -> Result<Pad, ImportError> {
    let contours = rings(&paths, context)?;
    let bounds = paths
        .iter()
        .filter_map(|p| path_bounds(p))
        .fold(None::<Bounds>, |b, p| {
            Some(if let Some(mut b) = b {
                b.include(p.min);
                b.include(p.max);
                b
            } else {
                p
            })
        });
    let mut pad = Pad::circle(LayerId(0), 0.0);
    pad.kind = PadKind::CUSTOM;
    if let Some(b) = bounds {
        pad.width = b.max.x - b.min.x;
        pad.height = b.max.y - b.min.y;
    }
    pad.custom = Some(Shared::new(CustomPadGeometry { contours, paths }));
    Ok(pad)
}
pub(super) fn area(ring: &[Point]) -> f64 {
    ring.iter()
        .enumerate()
        .map(|(i, a)| {
            let b = ring[(i + 1) % ring.len()];
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        * 0.5
}
pub(super) fn contains(ring: &[Point], p: Point) -> bool {
    let mut inside = false;
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}
