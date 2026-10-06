use super::super::geometry;
use super::{error, lines, num, scale};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{Point, Segment};
use std::{collections::HashMap, sync::Arc};
#[derive(Clone)]
pub(super) struct Symbol {
    pub name: String,
    pub scale: f64,
}
pub(super) struct Contour {
    pub hole: bool,
    pub path: Vec<Segment>,
}
pub(super) enum Kind {
    Line(Segment, Arc<Symbol>),
    Pad {
        at: Point,
        angle: f64,
        mirror: bool,
        symbol: Arc<Symbol>,
    },
    Surface(Vec<Contour>),
}
pub(super) struct Feature {
    pub index: usize,
    pub attrs: HashMap<String, String>,
    pub kind: Kind,
}
pub(super) fn parse(
    text: &str,
    units: f64,
    context: &ImportContext<'_>,
    mut visit: impl FnMut(Feature) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let mut units = units;
    let mut index = 0;
    let mut symbols = HashMap::new();
    let mut names = HashMap::new();
    let mut values = HashMap::new();
    let mut surface: Option<Feature> = None;
    let mut contour: Option<Contour> = None;
    let mut first = Point::default();
    let mut previous = first;
    for (work, line) in lines(text).enumerate() {
        if work % 512 == 0 {
            context.check_cancelled()?;
        }
        if let Some(unit) = line
            .strip_prefix("UNITS=")
            .or_else(|| line.strip_prefix("U "))
        {
            units = scale(unit.trim())?;
            continue;
        }
        if line.starts_with("ID=") {
            continue;
        }
        let mut t = line.split_whitespace();
        let head = t.next().unwrap_or("");
        if let Some(id) = head.strip_prefix('$') {
            let name = t.next().ok_or_else(|| error("Missing symbol name"))?;
            let unit = t.next().unwrap_or("");
            symbols.insert(
                num(id)? as i32,
                Arc::new(Symbol {
                    name: name.into(),
                    scale: if unit == "M" {
                        1.0
                    } else if unit == "I" {
                        25.4
                    } else {
                        units
                    } / 1000.0,
                }),
            );
            continue;
        }
        if head.starts_with('@') || head.starts_with('&') {
            let id = num(&head[1..])? as i32;
            let value = line[head.len()..].trim().to_owned();
            if head.starts_with('@') {
                names.insert(id, value);
            } else {
                values.insert(id, value);
            }
            continue;
        }
        let mut parts = line.split(';');
        let command = parts.next().unwrap_or("");
        let attrs = parts.next().unwrap_or("");
        let t: Vec<_> = command.split_whitespace().collect();
        let token = |i| {
            t.get(i)
                .copied()
                .ok_or_else(|| error("Truncated feature record"))
        };
        let pt = |i| {
            Ok::<_, ImportError>(Point::new(
                num(token(i)?)? * units,
                num(token(i + 1)?)? * units,
            ))
        };
        let positive = |i| {
            if token(i)? == "P" {
                Ok(())
            } else {
                Err(error("Negative feature polarity not supported"))
            }
        };
        let symbol = |i| {
            symbols
                .get(&(num(token(i)?)? as i32))
                .cloned()
                .ok_or_else(|| error("Undefined aperture symbol"))
        };
        match t.first().copied().unwrap_or("") {
            "F" => continue,
            "OB" => {
                if surface.is_none() || contour.is_some() || !["I", "H"].contains(&token(3)?) {
                    return Err(error("Invalid OB contour"));
                }
                first = pt(1)?;
                previous = first;
                contour = Some(Contour {
                    hole: token(3)? == "H",
                    path: Vec::new(),
                });
                continue;
            }
            "OS" | "OC" => {
                let tag = token(0)?;
                let c = contour
                    .as_mut()
                    .ok_or_else(|| error("Contour edge without OB"))?;
                let next = pt(1)?;
                if tag == "OC" && !["Y", "N"].contains(&token(5)?) {
                    return Err(error("Invalid contour arc direction"));
                }
                if tag == "OC" || next != previous {
                    c.path.push(geometry::edge(
                        previous,
                        next,
                        0.0,
                        if tag == "OC" { Some(pt(3)?) } else { None },
                        tag == "OC" && token(5)? == "Y",
                    ));
                }
                previous = next;
                continue;
            }
            "OE" => {
                let c = contour.take().ok_or_else(|| error("OE without OB"))?;
                if first.distance(previous) > 1e-7 {
                    return Err(error("Open contour"));
                }
                let Some(Feature {
                    kind: Kind::Surface(contours),
                    ..
                }) = surface.as_mut()
                else {
                    return Err(error("OE without surface"));
                };
                contours.push(c);
                continue;
            }
            "SE" => {
                if contour.is_some() {
                    return Err(error("Unterminated contour"));
                }
                visit(surface.take().ok_or_else(|| error("SE without surface"))?)?;
                continue;
            }
            _ => {}
        }
        if surface.is_some() {
            return Err(error("Surface missing SE"));
        }
        let mut attributes = HashMap::new();
        for pair in attrs.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, "true"));
            if key == "ID" {
                continue;
            }
            if let Some(name) = names.get(&(num(key)? as i32)) {
                let value = if name == ".geometry" {
                    value
                        .parse::<i32>()
                        .ok()
                        .and_then(|id| values.get(&id))
                        .map_or(value, |v| v.as_str())
                } else {
                    value
                };
                attributes.insert(name.clone(), value.into());
            }
        }
        let kind = match token(0)? {
            "S" => {
                positive(1)?;
                Kind::Surface(Vec::new())
            }
            "L" | "A" => {
                let arc = token(0)? == "A";
                positive(if arc { 8 } else { 6 })?;
                if arc && !["Y", "N"].contains(&token(10)?) {
                    return Err(error("Invalid arc direction"));
                }
                Kind::Line(
                    geometry::edge(
                        pt(1)?,
                        pt(3)?,
                        0.0,
                        if arc { Some(pt(5)?) } else { None },
                        arc && token(10)? == "Y",
                    ),
                    symbol(if arc { 7 } else { 5 })?,
                )
            }
            "P" => {
                positive(4)?;
                let code = num(token(6)?)?;
                if code.fract() != 0.0 || !(0.0..=9.0).contains(&code) || token(3)? == "-1" {
                    return Err(error("Invalid pad orientation/resize"));
                }
                let code = code as u32;
                let mirror = (4..=7).contains(&code) || code == 9;
                let degrees = if code < 8 {
                    (code % 4) as f64 * 90.0
                } else {
                    num(token(7)?)?
                };
                Kind::Pad {
                    at: pt(1)?,
                    angle: if mirror { degrees + 180.0 } else { -degrees }.to_radians(),
                    mirror,
                    symbol: symbol(3)?,
                }
            }
            tag => return Err(error(format!("Unsupported feature {tag}"))),
        };
        let feature = Feature {
            index,
            attrs: attributes,
            kind,
        };
        index += 1;
        if matches!(feature.kind, Kind::Surface(_)) {
            surface = Some(feature);
        } else {
            visit(feature)?;
        }
    }
    if surface.is_some() || contour.is_some() {
        return Err(error("Truncated surface"));
    }
    Ok(())
}
