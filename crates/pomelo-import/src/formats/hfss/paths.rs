//! Nonround path coverage and saved clipped boundaries.
use super::super::geometry as shared;
use super::{binary::Object, error, geometry};
use crate::{ImportContext, ImportError};
use clipper2_rust::{EndType, FillRule, JoinType, PointD, area, inflate_paths_d, union_d};
use pomelo_core::{
    geometry::flatten_path,
    model::{Point, Segment},
};

pub(super) fn outlines(
    p: &Object,
    context: &ImportContext<'_>,
) -> Result<Vec<Vec<Segment>>, ImportError> {
    if p.schema == 51 {
        let template = geometry::polygon_object(p.field(1)?.object(Some(36))?)?;
        let positions = p.field(2)?.object(Some(52))?.field(0)?.array()?;
        if !positions.len().is_multiple_of(2) {
            return Err(error("Odd polygon instance coordinate count"));
        }
        return positions
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let at = Point::new(pair[0].numeric()? * 1000.0, pair[1].numeric()? * 1000.0);
                Ok(template
                    .iter()
                    .map(|s| shared::transform(s, at, 0.0, false))
                    .collect())
            })
            .collect();
    }
    if p.schema != 14 {
        return Ok(vec![geometry::primitive(p)?]);
    }
    if let Some(super::binary::Value::Object(saved)) = p.fields.get(7)
        && saved.schema == 36
        && saved.field(0)?.integer()? == 1
        && !saved.field(2)?.array()?.is_empty()
    {
        return Ok(vec![geometry::polygon_object(saved)?]);
    }
    let corner = p.field(1)?.integer()?;
    let caps = [p.field(2)?.integer()?, p.field(3)?.integer()?];
    if !(0..=2).contains(&corner) || caps.iter().any(|c| !(0..=2).contains(c)) {
        return Err(error("Unsupported path style without a saved boundary"));
    }
    let radius = p.field(4)?.quantity()? * 500.0;
    if radius <= 0.0 {
        return Err(error("Nonpositive filled path width"));
    }
    let center = p.field(6)?.object(Some(36))?;
    let path = geometry::polygon_object(center)?;
    let points = flatten_path(&path, 0.001, 8_000_000, context.cancellation)
        .map_err(|e| error(e.to_string()))?;
    if points.len() < 2 {
        return Err(error("Empty path centerline"));
    }
    let closed = center.field(0)?.integer()? == 1;
    let mut shapes = inflate_paths_d(
        &vec![points.iter().map(|p| PointD::new(p.x, p.y)).collect()],
        radius,
        if corner == 0 {
            JoinType::Round
        } else {
            JoinType::Miter
        },
        if closed {
            EndType::Joined
        } else {
            EndType::Butt
        },
        if corner == 1 { 1_000_000.0 } else { 2.0 },
        6,
        0.001,
    );
    if !closed {
        for (cap, at, neighbor) in [
            (caps[0], points[0], points[1]),
            (caps[1], points[points.len() - 1], points[points.len() - 2]),
        ] {
            let dx = at.x - neighbor.x;
            let dy = at.y - neighbor.y;
            let length = dx.hypot(dy);
            if length == 0.0 || cap == 1 {
                continue;
            }
            let mut shape = if cap == 0 {
                let mut pad =
                    pomelo_core::model::Pad::circle(pomelo_core::model::LayerId(0), radius * 2.0);
                pad.offset = Point::default();
                let disk = shared::pad_paths(&pad).remove(0);
                flatten_path(&disk, 0.001, 8_000_000, context.cancellation)
                    .map_err(|e| error(e.to_string()))?
                    .into_iter()
                    .map(|v| PointD::new(v.x + at.x, v.y + at.y))
                    .collect()
            } else {
                let along = Point::new(dx / length * radius, dy / length * radius);
                let normal = Point::new(-along.y, along.x);
                vec![
                    PointD::new(at.x + normal.x, at.y + normal.y),
                    PointD::new(at.x - normal.x, at.y - normal.y),
                    PointD::new(at.x + along.x - normal.x, at.y + along.y - normal.y),
                    PointD::new(at.x + along.x + normal.x, at.y + along.y + normal.y),
                ]
            };
            if area(&shape) < 0.0 {
                shape.reverse();
            }
            shapes.push(shape);
        }
    }
    context.check_cancelled()?;
    Ok(union_d(&shapes, &vec![], FillRule::NonZero, 6)
        .into_iter()
        .map(|ring| {
            ring.iter()
                .enumerate()
                .map(|(i, a)| {
                    let b = ring[(i + 1) % ring.len()];
                    shared::edge(Point::new(a.x, a.y), Point::new(b.x, b.y), 0.0, None, false)
                })
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::binary::Value as B;
    use super::*;
    use pomelo_core::{geometry::path_bounds, task::CancellationToken};

    fn coverage(
        rings: &[Vec<Point>],
        point: Point,
        cancel: &CancellationToken,
    ) -> Result<bool, pomelo_core::copper::MeshError> {
        pomelo_core::copper::CopperMesh::build(rings, &[], &Default::default(), cancel)?
            .covers_fill(point, cancel)
    }
    fn polygon(closed: bool, values: &[f64]) -> B {
        B::Object(Box::new(Object {
            schema: 36,
            fields: vec![
                B::Number(if closed { 1.0 } else { 0.0 }),
                B::Null,
                B::Array(values.iter().copied().map(B::Number).collect()),
            ],
        }))
    }
    fn trace(caps: [i32; 2], center: B, saved: B) -> Object {
        Object {
            schema: 14,
            fields: vec![
                B::Null,
                B::Number(1.0),
                B::Number(caps[0] as f64),
                B::Number(caps[1] as f64),
                B::Quantity(0.002),
                B::Quantity(0.6),
                center,
                saved,
            ],
        }
    }
    fn context(cancel: &CancellationToken) -> ImportContext<'_> {
        ImportContext {
            cancellation: cancel,
            progress: &|_| {},
        }
    }
    #[test]
    fn flat_round_extended_and_mixed_caps_keep_their_coverage() {
        let cancel = CancellationToken::default();
        let context = context(&cancel);
        for (caps, min, max) in [
            ([0, 0], -1.0, 11.0),
            ([1, 1], 0.0, 10.0),
            ([2, 2], -1.0, 11.0),
            ([0, 1], -1.0, 10.0),
            ([1, 2], 0.0, 11.0),
        ] {
            let p = trace(caps, polygon(false, &[0.0, 0.0, 0.01, 0.0]), B::Null);
            let paths = outlines(&p, &context).unwrap();
            assert_eq!(paths.len(), 1);
            let bounds = path_bounds(&paths[0]).unwrap();
            assert!(
                (bounds.min.x - min).abs() < 0.001 && (bounds.max.x - max).abs() < 0.001,
                "{caps:?}: {bounds:?}"
            );
            let rings = shared::rings(&paths, &context).unwrap();
            assert!(coverage(&rings, Point::new(5.0, 0.0), &cancel).unwrap());
            let at_corner = Point::new(-0.9, 0.9);
            assert_eq!(coverage(&rings, at_corner, &cancel).unwrap(), caps[0] == 2);
        }
    }
    #[test]
    fn closed_path_keeps_its_empty_center() {
        let cancel = CancellationToken::default();
        let context = context(&cancel);
        let p = trace(
            [1, 1],
            polygon(true, &[0.0, 0.0, 0.01, 0.0, 0.01, 0.01, 0.0, 0.01]),
            B::Null,
        );
        let paths = outlines(&p, &context).unwrap();
        let rings = shared::rings(&paths, &context).unwrap();
        assert!(coverage(&rings, Point::new(0.0, 5.0), &cancel).unwrap());
        assert!(!coverage(&rings, Point::new(5.0, 5.0), &cancel).unwrap());
    }
    #[test]
    fn clipped_paths_use_the_saved_boundary_instead_of_extending_the_centerline() {
        let cancel = CancellationToken::default();
        let p = trace(
            [3, 0],
            polygon(false, &[0.0, 0.0, 0.01, 0.0]),
            polygon(
                true,
                &[0.002, -0.001, 0.01, -0.001, 0.01, 0.001, 0.002, 0.001],
            ),
        );
        let paths = outlines(&p, &context(&cancel)).unwrap();
        let b = path_bounds(&paths[0]).unwrap();
        assert_eq!(b.min, Point::new(2.0, -1.0));
        assert_eq!(b.max, Point::new(10.0, 1.0));
        let unsupported = trace([3, 0], polygon(false, &[0.0, 0.0, 0.01, 0.0]), B::Null);
        assert!(outlines(&unsupported, &context(&cancel)).is_err());
    }
    #[test]
    fn packed_polygon_instances_keep_each_translation() {
        let cancel = CancellationToken::default();
        let p = Object {
            schema: 51,
            fields: vec![
                B::Null,
                polygon(true, &[0.0, 0.0, 0.001, 0.0, 0.001, 0.001, 0.0, 0.001]),
                B::Object(Box::new(Object {
                    schema: 52,
                    fields: vec![B::Array(vec![
                        B::Number(0.002),
                        B::Number(0.003),
                        B::Number(-0.002),
                        B::Number(-0.003),
                    ])],
                })),
            ],
        };
        let paths = outlines(&p, &context(&cancel)).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(path_bounds(&paths[0]).unwrap().min, Point::new(2.0, 3.0));
        assert_eq!(path_bounds(&paths[1]).unwrap().min, Point::new(-2.0, -3.0));
    }
}
