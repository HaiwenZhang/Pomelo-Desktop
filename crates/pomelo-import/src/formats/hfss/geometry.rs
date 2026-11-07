use super::super::geometry as shared;
use super::{
    binary::Object,
    error,
    text::{Call, Value},
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{LayerId, Pad, PadKind, Point, Segment};
pub(super) fn quantity(value: &Value, angle: bool) -> Result<f64, ImportError> {
    if let Value::Number(n) = value {
        return Ok(*n);
    }
    let text = value.text()?.trim();
    let end = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && !matches!(c, '+' | '-' | '.' | 'e' | 'E'))
        .map_or(text.len(), |(i, _)| i);
    let (n, unit) = text.split_at(end);
    let n = n
        .trim()
        .parse::<f64>()
        .map_err(|_| error(format!("Unresolved expression {text}")))?;
    let unit = unit.trim().to_ascii_lowercase();
    let scale = if angle {
        match unit.as_str() {
            "" | "rad" => 1.0,
            "deg" => std::f64::consts::PI / 180.0,
            _ => return Err(error("Unknown angle unit")),
        }
    } else {
        match unit.as_str() {
            "" | "m" => 1.0,
            "cm" => 0.01,
            "mm" => 0.001,
            "um" | "µm" | "μm" => 1e-6,
            "nm" => 1e-9,
            "mil" => 0.0000254,
            "in" | "inch" => 0.0254,
            _ => return Err(error("Unknown length unit")),
        }
    };
    let result = n * scale;
    if !result.is_finite() {
        return Err(error("Non-finite quantity"));
    }
    Ok(result)
}

pub(super) fn polygon(values: &[f64], closed: bool) -> Result<Vec<Segment>, ImportError> {
    if !values.len().is_multiple_of(2) {
        return Err(error("Odd polygon coordinate count"));
    }
    if values.is_empty() {
        return Ok(Vec::new());
    }
    if values[1] == f64::MAX {
        return Err(error("Arc marker at polygon start"));
    }
    if values.iter().any(|n| !n.is_finite()) {
        return Err(error("Non-finite polygon coordinate"));
    }
    let first = Point::new(values[0] * 1000.0, values[1] * 1000.0);
    let mut previous = first;
    let mut height = None;
    let mut path = Vec::new();
    let mut push = |next: Point, height: &mut Option<f64>| -> Result<(), ImportError> {
        let center = if let Some(h) = height.take().filter(|h| *h != 0.0) {
            let dx = next.x - previous.x;
            let dy = next.y - previous.y;
            let chord = dx.hypot(dy);
            if chord == 0.0 {
                return Err(error("Coincident arc endpoints"));
            }
            let offset = chord * chord / (8.0 * h) - h * 0.5;
            Some((
                Point::new(
                    (previous.x + next.x) * 0.5 + dy / chord * offset,
                    (previous.y + next.y) * 0.5 - dx / chord * offset,
                ),
                h > 0.0,
            ))
        } else {
            None
        };
        if center.is_some() || previous != next {
            path.push(shared::edge(
                previous,
                next,
                0.0,
                center.map(|c| c.0),
                center.is_some_and(|c| c.1),
            ));
        }
        previous = next;
        Ok(())
    };
    for p in values[2..].as_chunks::<2>().0 {
        if p[1] == f64::MAX {
            if height.is_some() {
                return Err(error("Consecutive arc markers"));
            }
            height = Some(p[0] * 1000.0);
        } else {
            push(Point::new(p[0] * 1000.0, p[1] * 1000.0), &mut height)?;
        }
    }
    if closed {
        push(first, &mut height)?;
    } else if height.is_some() {
        return Err(error("Open arc without endpoint"));
    }
    Ok(path)
}
pub(super) fn polygon_object(p: &Object) -> Result<Vec<Segment>, ImportError> {
    if p.schema != 36 {
        return Err(error("Invalid polygon schema"));
    }
    let values = p
        .field(2)?
        .array()?
        .iter()
        .map(|v| v.numeric())
        .collect::<Result<Vec<_>, _>>()?;
    polygon(&values, p.field(0)?.integer()? == 1)
}
pub(super) fn primitive(p: &Object) -> Result<Vec<Segment>, ImportError> {
    match p.schema {
        15 => polygon_object(p.field(1)?.object(Some(36))?),
        13 => {
            let x = p.field(1)?.quantity()? * 1000.0;
            let y = p.field(2)?.quantity()? * 1000.0;
            let r = p.field(3)?.quantity()? * 1000.0;
            if r <= 0.0 {
                return Err(error("Invalid circle radius"));
            }
            let a = Point::new(x + r, y);
            Ok(vec![shared::edge(a, a, 0.0, Some(Point::new(x, y)), false)])
        }
        12 => {
            let x0 = p.field(2)?.quantity()? * 1000.0;
            let y0 = p.field(3)?.quantity()? * 1000.0;
            let x1 = p.field(4)?.quantity()? * 1000.0;
            let y1 = p.field(5)?.quantity()? * 1000.0;
            let corner = p.field(6)?.quantity()? * 1000.0;
            let angle = p.field(7)?.quantity()?;
            let (center, width, height) = match p.field(1)?.integer()? {
                1 => (Point::new(x0, y0), x1, y1),
                2 => (
                    Point::new((x0 + x1) * 0.5, (y0 + y1) * 0.5),
                    (x1 - x0).abs(),
                    (y1 - y0).abs(),
                ),
                _ => return Err(error("Unknown rectangle representation")),
            };
            if width <= 0.0 || height <= 0.0 || corner < 0.0 || corner > width.min(height) * 0.5 {
                return Err(error("Invalid rectangle dimensions"));
            }
            let mut pad = Pad::circle(LayerId(0), width);
            pad.height = height;
            pad.kind = PadKind(if corner > 0.0 { 27 } else { 5 });
            pad.corner = corner;
            Ok(shared::pad_paths(&pad)
                .remove(0)
                .iter()
                .map(|s| shared::transform(s, center, angle, false))
                .collect())
        }
        _ => Err(error("Primitive has no supported outline")),
    }
}
fn nested<'a>(call: &'a Call, name: &str) -> Result<&'a Call, ImportError> {
    call.args
        .iter()
        .find_map(|(_, v)| {
            if let Value::Call(c) = v {
                if c.name == name { Some(c) } else { None }
            } else {
                None
            }
        })
        .ok_or_else(|| error(format!("{} missing {name}", call.name)))
}
fn text_polygon(call: &Call) -> Result<Vec<Segment>, ImportError> {
    let pt = nested(call, "pt")?;
    let unit = pt.arg("U")?.text()?;
    let scale = quantity(&Value::Text(format!("1{unit}")), false)?;
    let args: Vec<_> = pt
        .args
        .iter()
        .filter(|a| a.0.as_deref() != Some("U"))
        .collect();
    if args.len() % 2 != 0 {
        return Err(error("Odd text polygon coordinate count"));
    }
    let mut values = Vec::new();
    for pair in args.as_chunks::<2>().0 {
        if pair[0].0.as_deref() != Some("x") || pair[1].0.as_deref() != Some("y") {
            return Err(error("Invalid text polygon coordinate order"));
        }
        let x = pair[0].1.number()?;
        let y = pair[1].1.number()?;
        values.push(x * scale);
        values.push(if y == 1e200 { f64::MAX } else { y * scale });
    }
    let closed = match call.arg("cl")? {
        Value::Bool(b) => *b,
        _ => return Err(error("Missing polygon closed flag")),
    };
    if closed && values.len() >= 4 && values[values.len() - 2..] == values[..2] {
        values.truncate(values.len() - 2);
    }
    polygon(&values, closed)
}
pub(super) struct Standard {
    pub shape: String,
    pub sizes: Vec<f64>,
    pub offset: Point,
    pub angle: f64,
}
pub(super) fn standard(call: &Call) -> Result<Standard, ImportError> {
    let shape = call.arg("shp")?.text()?;
    let count = match shape {
        "No" => 0,
        "Cir" | "Sq" => 1,
        "Rct" => 2,
        "Ov" | "Blt" => 3,
        _ => return Err(error("Nonstandard pad shape")),
    };
    let sizes = nested(call, "Szs")?
        .args
        .iter()
        .map(|a| quantity(&a.1, false).map(|n| n * 1000.0))
        .collect::<Result<Vec<_>, _>>()?;
    if sizes.len() != count || sizes.iter().any(|&s| s < 0.0) {
        return Err(error("Invalid standard shape dimensions"));
    }
    Ok(Standard {
        shape: shape.into(),
        sizes,
        offset: Point::new(
            quantity(call.arg("X")?, false)? * 1000.0,
            quantity(call.arg("Y")?, false)? * 1000.0,
        ),
        angle: quantity(call.arg("R")?, true)?,
    })
}
pub(super) fn pad(
    call: &Call,
    layer: LayerId,
    context: &ImportContext<'_>,
) -> Result<Option<Pad>, ImportError> {
    if call.arg("shp")?.text()? == "NPl" {
        let sizes = nested(call, "Szs")?;
        if sizes.args.len() != 2 {
            return Err(error("Invalid regular polygon pad dimensions"));
        }
        let diameter = quantity(&sizes.args[0].1, false)? * 1000.0;
        let count = quantity(&sizes.args[1].1, false)?;
        if diameter <= 0.0 || count.fract() != 0.0 || !(3.0..=4096.0).contains(&count) {
            return Err(error("Invalid regular polygon pad dimensions"));
        }
        let angle = quantity(call.arg("R")?, true)?;
        let points: Vec<_> = (0..count as usize)
            .map(|i| {
                let a = angle + (i as f64 + 0.5) * std::f64::consts::TAU / count;
                Point::new(a.cos() * diameter * 0.5, a.sin() * diameter * 0.5)
            })
            .collect();
        let path = points
            .iter()
            .enumerate()
            .map(|(i, &a)| shared::edge(a, points[(i + 1) % points.len()], 0.0, None, false))
            .collect();
        let mut pad = shared::custom(vec![path], context)?;
        pad.layer = layer;
        pad.offset = Point::new(
            quantity(call.arg("X")?, false)? * 1000.0,
            quantity(call.arg("Y")?, false)? * 1000.0,
        );
        return Ok(Some(pad));
    }
    if call.arg("shp")?.text()? == "Ply" {
        let poly = nested(call, "ply")?;
        if !matches!(poly.arg("cl")?, Value::Bool(true)) {
            return Err(error("Open polygon pad"));
        }
        let angle = quantity(call.arg("R")?, true)?;
        let mut paths = vec![text_polygon(poly)?];
        if let Ok(holes) = nested(poly, "hls") {
            for (_, value) in &holes.args {
                let hole = value.call()?;
                if hole.name != "hl" {
                    return Err(error("Invalid pad polygon hole"));
                }
                paths.push(text_polygon(hole)?);
            }
        }
        if paths[0].is_empty() {
            return Ok(None);
        }
        let paths = paths
            .into_iter()
            .map(|p| {
                p.iter()
                    .map(|s| shared::transform(s, Point::default(), angle, false))
                    .collect()
            })
            .collect();
        let mut pad = shared::custom(paths, context)?;
        pad.layer = layer;
        pad.offset = Point::new(
            quantity(call.arg("X")?, false)? * 1000.0,
            quantity(call.arg("Y")?, false)? * 1000.0,
        );
        return Ok(Some(pad));
    }
    let value = standard(call)?;
    let width = value.sizes.first().copied().unwrap_or(0.0);
    let height = value.sizes.get(1).copied().unwrap_or(width);
    let radius = value.sizes.get(2).copied().unwrap_or(0.0);
    if value.shape == "No" || width == 0.0 || height == 0.0 {
        return Ok(None);
    }
    if radius > width.min(height) * 0.5 + 1e-12 {
        return Err(error("Oval corner exceeds dimensions"));
    }
    let mut pad = Pad::circle(layer, width);
    pad.height = height;
    pad.kind = PadKind(if value.shape == "Cir" {
        2
    } else if value.shape == "Ov" && radius > 0.0 {
        27
    } else {
        5
    });
    pad.corner = radius;
    pad.offset = value.offset;
    if value.shape == "Blt" || (value.angle != 0.0 && pad.kind != PadKind::CIRCLE) {
        let mut outlines = shared::pad_paths(&pad);
        if value.shape == "Blt" {
            // Bullet pads have two rounded corners at +X and square corners at -X.
            let w = width * 0.5;
            let h = height * 0.5;
            let r = radius;
            if r > w.min(h) {
                return Err(error("Bullet corner exceeds dimensions"));
            }
            let v = [
                Point::new(w, -h + r),
                Point::new(w, h - r),
                Point::new(w - r, h),
                Point::new(-w, h),
                Point::new(-w, -h),
                Point::new(w - r, -h),
            ];
            outlines = vec![
                v.iter()
                    .enumerate()
                    .map(|(i, &a)| {
                        let center = match i {
                            1 if r > 0.0 => Some(Point::new(w - r, h - r)),
                            5 if r > 0.0 => Some(Point::new(w - r, -h + r)),
                            _ => None,
                        };
                        shared::edge(a, v[(i + 1) % v.len()], 0.0, center, false)
                    })
                    .filter(|s| s.a != s.b)
                    .collect(),
            ];
        }
        let paths = outlines
            .into_iter()
            .map(|p| {
                p.iter()
                    .map(|s| shared::transform(s, Point::default(), value.angle, false))
                    .collect()
            })
            .collect();
        let mut custom = shared::custom(paths, context)?;
        custom.layer = layer;
        custom.offset = value.offset;
        return Ok(Some(custom));
    }
    Ok(Some(pad))
}
pub(super) struct Drill {
    pub rectangular: bool,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    pub offset: Point,
    pub custom: Option<std::sync::Arc<pomelo_core::model::CustomPadGeometry>>,
}
pub(super) fn drill(call: &Call, context: &ImportContext<'_>) -> Result<Drill, ImportError> {
    if call.arg("shp")?.text()? != "Ply" {
        let s = standard(call)?;
        let (width, height, angle) = if s.shape == "Ov" {
            let w = s.sizes[0];
            let h = s.sizes[1];
            if w <= 0.0 || h <= 0.0 || (s.sizes[2] - w.min(h) * 0.5).abs() > 1e-9 {
                return Err(error("Invalid oval drill radius"));
            }
            (w, h, s.angle)
        } else if s.shape == "Rct" || s.shape == "Sq" {
            let w = s.sizes[0];
            let h = s.sizes.get(1).copied().unwrap_or(w);
            if w <= 0.0 || h <= 0.0 {
                return Err(error("Invalid rectangular drill dimensions"));
            }
            (w, h, s.angle)
        } else if s.shape == "No" || s.shape == "Cir" {
            let d = s.sizes.first().copied().unwrap_or(0.0);
            (d, d, 0.0)
        } else {
            return Err(error("Unsupported drill shape"));
        };
        return Ok(Drill {
            rectangular: matches!(s.shape.as_str(), "Rct" | "Sq"),
            width,
            height,
            angle,
            offset: s.offset,
            custom: None,
        });
    }
    let shape =
        pad(call, LayerId::UNASSIGNED, context)?.ok_or_else(|| error("Empty polygon drill"))?;
    let bounds = shape
        .bounds(pomelo_core::pad::PadPlacement::default())
        .ok_or_else(|| error("Invalid polygon drill bounds"))?;
    if bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
        return Err(error("Degenerate polygon drill"));
    }
    let center = bounds.center();
    let geometry = shape
        .custom
        .as_ref()
        .ok_or_else(|| error("Missing polygon drill geometry"))?;
    let paths = geometry
        .paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|edge| {
                    shared::transform(
                        edge,
                        Point::new(shape.offset.x - center.x, shape.offset.y - center.y),
                        0.0,
                        false,
                    )
                })
                .collect()
        })
        .collect();
    let centered = shared::custom(paths, context)?;
    Ok(Drill {
        rectangular: false,
        width: bounds.max.x - bounds.min.x,
        height: bounds.max.y - bounds.min.y,
        angle: 0.0,
        offset: center,
        custom: centered.custom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::task::CancellationToken;

    #[test]
    fn rectangular_drills_keep_sharp_corners_dimensions_and_rotation() {
        let value = super::super::text::statement(
            "hle(shp='Rct', Szs('2mm','1mm'), X='0.1mm', Y='0.2mm', R='90deg')",
        )
        .unwrap()
        .1;
        let cancel = CancellationToken::default();
        let hole = drill(
            value.call().unwrap(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
        assert!(hole.rectangular);
        assert_eq!([hole.width, hole.height], [2.0, 1.0]);
        assert_eq!(hole.offset, Point::new(0.1, 0.2));
        assert!((hole.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn rectangle_corner_order_does_not_change_coverage() {
        use super::super::binary::Value as B;
        for (x0, y0, x1, y1) in [(0.021, 0.0, 0.027, -0.003), (0.027, -0.003, 0.021, 0.0)] {
            let rectangle = Object {
                schema: 12,
                fields: vec![
                    B::Null,
                    B::Number(2.0),
                    B::Quantity(x0),
                    B::Quantity(y0),
                    B::Quantity(x1),
                    B::Quantity(y1),
                    B::Quantity(0.0),
                    B::Quantity(0.0),
                ],
            };
            let path = primitive(&rectangle).unwrap();
            let bounds = pomelo_core::geometry::path_bounds(&path).unwrap();
            assert_eq!(bounds.min, Point::new(21.0, -3.0));
            assert_eq!(bounds.max, Point::new(27.0, 0.0));
        }
    }

    #[test]
    fn center_width_height_rectangles_keep_their_center_and_size() {
        use super::super::binary::Value as B;
        let rectangle = Object {
            schema: 12,
            fields: vec![
                B::Null,
                B::Number(1.0),
                B::Quantity(0.024),
                B::Quantity(-0.0015),
                B::Quantity(0.006),
                B::Quantity(0.003),
                B::Quantity(0.0),
                B::Quantity(0.0),
            ],
        };
        let bounds = pomelo_core::geometry::path_bounds(&primitive(&rectangle).unwrap()).unwrap();
        assert_eq!(bounds.min, Point::new(21.0, -3.0));
        assert_eq!(bounds.max, Point::new(27.0, 0.0));
    }

    #[test]
    fn explicit_quantities_convert_units_and_reject_expressions() {
        assert!((quantity(&Value::Text("10mil".into()), false).unwrap() - 0.000254).abs() < 1e-12);
        assert!(
            (quantity(&Value::Text("90deg".into()), true).unwrap() - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
        for text in ["width/2", "1mm + 2mm", "10deg"] {
            assert!(quantity(&Value::Text(text.into()), false).is_err());
        }
    }

    #[test]
    fn sagitta_marker_retains_arc_direction_and_circle_center() {
        let path = polygon(&[0.0, 0.0, 0.001, f64::MAX, 0.002, 0.0], false).unwrap();
        assert_eq!(path.len(), 1);
        let arc = path[0].arc.unwrap();
        assert_eq!(arc.center, Point::new(1.0, 0.0));
        assert!((arc.radius - 1.0).abs() < 1e-12);
        assert!((arc.sweep + std::f64::consts::PI).abs() < 1e-12);
        assert!(polygon(&[0.0, 0.0, 0.001, f64::MAX], false).is_err());
    }

    #[test]
    fn rotated_per_layer_pad_keeps_analytic_geometry_and_local_offset() {
        let cancel = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        };
        let call = super::super::text::statement(
            "pad(shp='Rct', X='1mm', Y='2mm', R='90deg', Szs('4mm','2mm'))",
        )
        .unwrap()
        .1;
        let pad = pad(call.call().unwrap(), LayerId(2), &context)
            .unwrap()
            .unwrap();
        assert_eq!(pad.offset, Point::new(1.0, 2.0));
        let bounds = pomelo_core::geometry::path_bounds(&pad.custom.unwrap().paths[0]).unwrap();
        assert!((bounds.max.x - bounds.min.x - 2.0).abs() < 1e-9);
        assert!((bounds.max.y - bounds.min.y - 4.0).abs() < 1e-9);
    }

    #[test]
    fn arbitrary_polygon_drills_preserve_edges_offsets_rotation_and_coverage() {
        let cancel = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        };
        let call = super::super::text::statement(
            "hle(shp='Ply', Szs(), ply(cw=false, cl=true, pt(U='mm', x=0, y=0, x=0.4, y=0, x=0, y=0.2)), X='0.1mm', Y='0.2mm', R='90deg')"
        ).unwrap().1;
        let hole = drill(call.call().unwrap(), &context).unwrap();
        let custom = hole.custom.unwrap();
        assert_eq!(custom.paths[0].len(), 3);
        assert!((hole.width - 0.2).abs() < 1e-12 && (hole.height - 0.4).abs() < 1e-12);
        let mut shape = Pad::circle(LayerId(0), hole.width);
        shape.height = hole.height;
        shape.custom = Some(custom);
        let owner = pomelo_core::pad::PadPlacement {
            at: hole.offset,
            ..Default::default()
        };
        assert_eq!(
            shape
                .custom_covers_fill(Point::new(0.05, 0.25), owner, &cancel)
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            shape
                .custom_covers_fill(Point::new(0.05, 0.55), owner, &cancel)
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            shape
                .custom_covers_fill(Point::new(-0.15, 0.25), owner, &cancel)
                .unwrap(),
            Some(false)
        );
    }
}
