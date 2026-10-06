use super::super::geometry;
use super::{
    archive::Archive,
    error,
    features::{self, Contour, Kind, Symbol},
    num,
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{LayerId, Pad, PadKind, Point, Segment};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub(super) struct Island {
    pub paths: Vec<Vec<Segment>>,
    pub rings: Vec<Vec<Point>>,
}
pub(super) fn islands(
    contours: Vec<Contour>,
    context: &ImportContext<'_>,
) -> Result<Vec<Island>, ImportError> {
    let mut islands = Vec::new();
    let mut holes = Vec::new();
    for c in contours {
        let ring = geometry::rings(std::slice::from_ref(&c.path), context)?
            .pop()
            .unwrap_or_default();
        if c.hole {
            holes.push((c.path, ring));
        } else {
            islands.push(Island {
                paths: vec![c.path],
                rings: vec![ring],
            });
        }
    }
    for (path, ring) in holes {
        let first = *ring.first().ok_or_else(|| error("Empty hole contour"))?;
        let i = islands
            .iter()
            .enumerate()
            .filter(|(_, i)| geometry::contains(&i.rings[0], first))
            .min_by(|(_, a), (_, b)| {
                geometry::area(&a.rings[0])
                    .abs()
                    .total_cmp(&geometry::area(&b.rings[0]).abs())
            })
            .map(|(i, _)| i)
            .ok_or_else(|| error("Hole without containing island"))?;
        islands[i].paths.push(path);
        islands[i].rings.push(ring);
    }
    islands.retain(|i| i.rings[0].len() >= 3);
    Ok(islands)
}
#[derive(Default)]
pub(super) struct Geometry {
    pub pads: Vec<Pad>,
    pub strokes: Vec<Segment>,
}
pub(super) struct Symbols {
    cache: HashMap<(String, u64), Arc<Geometry>>,
    pending: HashSet<String>,
}
impl Symbols {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            pending: HashSet::new(),
        }
    }
    pub fn read(
        &mut self,
        symbol: &Symbol,
        archive: &mut Archive<'_>,
        units: f64,
        context: &ImportContext<'_>,
    ) -> Result<Arc<Geometry>, ImportError> {
        context.check_cancelled()?;
        let key = (symbol.name.clone(), symbol.scale.to_bits());
        if let Some(cached) = self.cache.get(&key) {
            return Ok(Arc::clone(cached));
        }
        let result = if let Some(standard) = standard(symbol, context)? {
            standard
        } else {
            if self.pending.len() >= 64 || !self.pending.insert(symbol.name.clone()) {
                return Err(error("Cyclic/excessive symbol nesting"));
            }
            let text = archive.text(&format!("symbols/{}/features", symbol.name), true)?;
            let mut result = Geometry::default();
            features::parse(&text, units, context, |feature| {
                match feature.kind {
                    Kind::Surface(contours) => {
                        for island in islands(contours, context)? {
                            result.pads.push(geometry::custom(island.paths, context)?);
                        }
                    }
                    Kind::Line(mut stroke, symbol) => {
                        let brush = self.read(&symbol, archive, units, context)?;
                        if brush.pads.len() != 1
                            || brush.pads[0].kind != PadKind::CIRCLE
                            || !brush.strokes.is_empty()
                        {
                            return Err(error("Nonround line brush"));
                        }
                        stroke.width = brush.pads[0].width;
                        result.strokes.push(stroke);
                    }
                    Kind::Pad {
                        at,
                        angle,
                        mirror,
                        symbol,
                    } => {
                        let nested = self.read(&symbol, archive, units, context)?;
                        for pad in &nested.pads {
                            if pad.width <= 0.0 || pad.height <= 0.0 {
                                continue;
                            }
                            let paths = geometry::pad_paths(pad)
                                .into_iter()
                                .map(|p| {
                                    p.iter()
                                        .map(|s| geometry::transform(s, at, angle, mirror))
                                        .collect()
                                })
                                .collect();
                            result.pads.push(geometry::custom(paths, context)?);
                        }
                        result.strokes.extend(
                            nested
                                .strokes
                                .iter()
                                .map(|s| geometry::transform(s, at, angle, mirror)),
                        );
                    }
                }
                Ok(())
            })?;
            self.pending.remove(&symbol.name);
            result
        };
        let result = Arc::new(result);
        self.cache.insert(key, Arc::clone(&result));
        Ok(result)
    }
}
fn standard(symbol: &Symbol, context: &ImportContext<'_>) -> Result<Option<Geometry>, ImportError> {
    let name = symbol.name.as_str();
    let scale = symbol.scale;
    let pad = |kind, w: f64, h: f64, corner, inner| {
        let mut p = Pad::circle(LayerId(0), w * scale);
        p.kind = PadKind(kind);
        p.height = h * scale;
        p.corner = corner;
        p.inner_diameter = inner;
        Geometry {
            pads: vec![p],
            strokes: Vec::new(),
        }
    };
    if let Some(value) = name
        .strip_prefix('r')
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
    {
        let d = num(value)?;
        return Ok(Some(pad(2, d, d, 0.0, None)));
    }
    if let Some(value) = name
        .strip_prefix('s')
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
    {
        let d = num(value)?;
        return Ok(Some(pad(6, d, d, 0.0, None)));
    }
    if let Some(dim) = name.strip_prefix("donut_r") {
        let (a, b) = dim
            .split_once('x')
            .ok_or_else(|| error("Invalid donut symbol"))?;
        let a = num(a)?;
        let b = num(b)?;
        if a <= 0.0 || b > a {
            return Err(error("Invalid donut diameters"));
        }
        return Ok(Some(if a == b {
            let r = a * scale * 0.5;
            Geometry {
                pads: Vec::new(),
                strokes: vec![geometry::edge(
                    Point::new(r, 0.0),
                    Point::new(r, 0.0),
                    0.0,
                    Some(Point::default()),
                    false,
                )],
            }
        } else if b == 0.0 {
            pad(2, a, a, 0.0, None)
        } else {
            pad(25, a, a, 0.0, Some(b * scale))
        }));
    }
    if let Some(dim) = name.strip_prefix("oval") {
        let (a, b) = dim
            .split_once('x')
            .ok_or_else(|| error("Invalid oval symbol"))?;
        return Ok(Some(pad(11, num(a)?, num(b)?, 0.0, None)));
    }
    if let Some(dim) = name.strip_prefix("rect") {
        let values: Vec<_> = dim.split('x').collect();
        if values.len() < 2 {
            return Err(error("Invalid rectangle symbol"));
        }
        let w = num(values[0])?;
        let h = num(values[1])?;
        let (kind, r) = if let Some(corner) = values.get(2) {
            let rounded = corner.starts_with('r');
            if !rounded && !corner.starts_with('c') {
                return Err(error("Invalid rectangle corner"));
            }
            (if rounded { 27 } else { 28 }, num(&corner[1..])? * scale)
        } else {
            (6, 0.0)
        };
        if values.len() < 4 {
            return Ok(Some(pad(kind, w, h, r, None)));
        }
        let w = w * scale * 0.5;
        let h = h * scale * 0.5;
        let vertices = [
            Point::new(w, h),
            Point::new(-w, h),
            Point::new(-w, -h),
            Point::new(w, -h),
        ];
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        let mut centers = Vec::new();
        for i in 0..4 {
            let r = if values[3].contains(char::from(b'1' + i as u8)) {
                r
            } else {
                0.0
            };
            let v = vertices[i];
            let prev = vertices[(i + 3) % 4];
            let next = vertices[(i + 1) % 4];
            let a = Point::new(
                v.x + (prev.x - v.x).signum() * r,
                v.y + (prev.y - v.y).signum() * r,
            );
            let b = Point::new(
                v.x + (next.x - v.x).signum() * r,
                v.y + (next.y - v.y).signum() * r,
            );
            starts.push(a);
            ends.push(b);
            centers.push(Point::new(a.x + b.x - v.x, a.y + b.y - v.y));
        }
        let mut path = Vec::new();
        for i in 0..4 {
            if starts[i] != ends[i] {
                path.push(geometry::edge(
                    starts[i],
                    ends[i],
                    0.0,
                    if kind == 27 { Some(centers[i]) } else { None },
                    false,
                ));
            }
            path.push(geometry::edge(
                ends[i],
                starts[(i + 1) % 4],
                0.0,
                None,
                false,
            ));
        }
        return Ok(Some(Geometry {
            pads: vec![geometry::custom(vec![path], context)?],
            strokes: Vec::new(),
        }));
    }
    if let Some(dim) = name.strip_prefix("di") {
        let (w, h) = dim
            .split_once('x')
            .ok_or_else(|| error("Invalid diamond symbol"))?;
        let w = num(w)? * scale * 0.5;
        let h = num(h)? * scale * 0.5;
        let p = [
            Point::new(w, 0.0),
            Point::new(0.0, h),
            Point::new(-w, 0.0),
            Point::new(0.0, -h),
        ];
        return Ok(Some(Geometry {
            pads: vec![geometry::custom(
                vec![
                    p.iter()
                        .enumerate()
                        .map(|(i, &a)| geometry::edge(a, p[(i + 1) % 4], 0.0, None, false))
                        .collect(),
                ],
                context,
            )?],
            strokes: Vec::new(),
        }));
    }
    if let Some(dim) = name.strip_prefix("ths") {
        let v = dim.split('x').map(num).collect::<Result<Vec<_>, _>>()?;
        if v.len() != 5 {
            return Err(error("Invalid thermal symbol"));
        }
        let outer = v[0] * scale * 0.5;
        let inner = v[1] * scale * 0.5;
        let angle = v[2].to_radians();
        let spokes = v[3] as usize;
        let gap = v[4] * scale * 0.5;
        if inner <= 0.0
            || outer <= inner
            || !(2..=64).contains(&spokes)
            || gap <= 0.0
            || gap >= inner
            || v[3].fract() != 0.0
        {
            return Err(error("Invalid thermal dimensions"));
        }
        let pitch = std::f64::consts::TAU / spokes as f64;
        let ao = (gap / outer).asin();
        let ai = (gap / inner).asin();
        if ai * 2.0 >= pitch {
            return Err(error("Thermal gaps overlap"));
        }
        let point = |r: f64, a: f64| Point::new(r * a.cos(), r * a.sin());
        let mut pads = Vec::new();
        for i in 0..spokes {
            let a = angle + i as f64 * pitch;
            let b = a + pitch;
            let p = [
                point(outer, a + ao),
                point(outer, b - ao),
                point(inner, b - ai),
                point(inner, a + ai),
            ];
            pads.push(geometry::custom(
                vec![vec![
                    geometry::edge(p[0], p[1], 0.0, Some(Point::default()), false),
                    geometry::edge(p[1], p[2], 0.0, None, false),
                    geometry::edge(p[2], p[3], 0.0, Some(Point::default()), true),
                    geometry::edge(p[3], p[0], 0.0, None, false),
                ]],
                context,
            )?);
        }
        return Ok(Some(Geometry {
            pads,
            strokes: Vec::new(),
        }));
    }
    Ok(None)
}
