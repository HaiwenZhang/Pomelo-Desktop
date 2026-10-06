//! Native Altium PcbDoc source import.
mod compound;
mod objects;
use super::{ParsedBoard, SourceDrawingLayer, geometry, scene_builder};
use crate::{ImportContext, ImportError};
use compound::Compound;
use pomelo_core::model::*;
use std::collections::{HashMap, HashSet};
const MM: f64 = 0.00000254;
fn error(details: impl Into<String>) -> ImportError {
    ImportError::Format {
        format: "Altium".into(),
        details: details.into(),
    }
}
struct View<'a>(&'a [u8]);
impl<'a> View<'a> {
    fn range(&self, at: usize, len: usize) -> Result<&'a [u8], ImportError> {
        self.0
            .get(
                at..at
                    .checked_add(len)
                    .ok_or_else(|| error("Field offset overflow"))?,
            )
            .ok_or_else(|| error(format!("Truncated field {at}+{len}/{}", self.0.len())))
    }
    fn u8(&self, at: usize) -> Result<u8, ImportError> {
        Ok(self.range(at, 1)?[0])
    }
    fn u16(&self, at: usize) -> Result<u16, ImportError> {
        let b = self.range(at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&self, at: usize) -> Result<u32, ImportError> {
        let b = self.range(at, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&self, at: usize) -> Result<i32, ImportError> {
        Ok(self.u32(at)? as i32)
    }
    fn f64(&self, at: usize) -> Result<f64, ImportError> {
        let b = self.range(at, 8)?;
        let value = f64::from_le_bytes(b.try_into().map_err(|_| error("Invalid f64"))?);
        if !value.is_finite() {
            return Err(error("Non-finite f64"));
        }
        Ok(value)
    }
    fn length(&self, at: usize) -> Result<f64, ImportError> {
        Ok(self.i32(at)? as f64 * MM)
    }
    fn point(&self, at: usize) -> Result<Point, ImportError> {
        Ok(Point::new(self.length(at)?, -self.length(at + 4)?))
    }
}
type Properties = HashMap<String, String>;
fn latin(bytes: &[u8]) -> String {
    encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()
}
fn properties(raw: &[u8]) -> Result<Properties, ImportError> {
    let mut fields = HashMap::new();
    let mut overrides = HashMap::new();
    for item in raw.split(|&b| b == 0 || b == b'|') {
        if let Some(eq) = item.iter().position(|&b| b == b'=') {
            let mut key = latin(&item[..eq]).trim().to_ascii_uppercase();
            let utf8 = key.starts_with("%UTF8%");
            if utf8 {
                key = key[6..].into();
            }
            let value = if utf8 {
                std::str::from_utf8(&item[eq + 1..])
                    .map_err(|_| error("Invalid UTF-8 property"))?
                    .to_owned()
            } else {
                latin(&item[eq + 1..])
            };
            if !key.is_empty() {
                if utf8 {
                    overrides.insert(key, value.trim().into());
                } else {
                    fields.insert(key, value.trim().into());
                }
            }
        }
    }
    fields.extend(overrides);
    Ok(fields)
}
fn property_records(
    bytes: &[u8],
    count: usize,
    context: &ImportContext<'_>,
) -> Result<Vec<Properties>, ImportError> {
    let v = View(bytes);
    let mut at = 0;
    let mut records = Vec::new();
    while at < bytes.len() {
        context.check_cancelled()?;
        let word = v.u32(at)?;
        at += 4;
        let len = (word & 0xffffff) as usize;
        let raw = v.range(at, len)?;
        at += len;
        records.push(if word >> 24 == 0 {
            properties(raw)?
        } else {
            Properties::new()
        });
    }
    if records.len() != count {
        return Err(error("Property record count mismatch"));
    }
    Ok(records)
}
fn records<'a>(
    bytes: &'a [u8],
    kind: u8,
    parts: usize,
    count: usize,
    context: &ImportContext<'_>,
    mut visit: impl FnMut(usize, Vec<&'a [u8]>) -> Result<(), ImportError>,
) -> Result<(), ImportError> {
    let v = View(bytes);
    let mut at = 0;
    for index in 0..count {
        context.check_cancelled()?;
        if v.u8(at)? != kind {
            return Err(error("Binary record type mismatch"));
        }
        at += 1;
        let mut payloads = Vec::new();
        for _ in 0..parts {
            let len = v.u32(at)? as usize;
            at += 4;
            payloads.push(v.range(at, len)?);
            at += len;
        }
        visit(index, payloads)?;
    }
    if at != bytes.len() {
        return Err(error("Unexpected binary record suffix"));
    }
    Ok(())
}
fn mil(value: &str) -> Result<f64, ImportError> {
    let value = value.trim();
    let len = value.len();
    if len < 3 || !value[len - 3..].eq_ignore_ascii_case("mil") {
        return Err(error("Missing mil source unit"));
    }
    let mm = value[..len - 3]
        .parse::<f64>()
        .map_err(|_| error("Invalid mil value"))?
        * 0.0254;
    if !mm.is_finite() {
        return Err(error("Invalid mil value"));
    }
    Ok(mm)
}
fn number(fields: &Properties, key: &str, default: f64) -> Result<f64, ImportError> {
    let value = fields
        .get(key)
        .map(|v| v.parse::<f64>())
        .transpose()
        .map_err(|_| error(format!("Invalid property {key}")))?
        .unwrap_or(default);
    if !value.is_finite() {
        return Err(error("Non-finite property"));
    }
    Ok(value)
}
fn dimension(fields: &Properties, key: &str) -> Result<f64, ImportError> {
    mil(fields
        .get(key)
        .ok_or_else(|| error(format!("Missing dimension {key}")))?)
}
fn raw_layer(id: u32) -> Option<u32> {
    if (0x01000001..=0x0100001f).contains(&id) {
        Some(id - 0x01000000)
    } else if id == 0x0100ffff {
        Some(32)
    } else if (0x01010001..=0x01010010).contains(&id) {
        Some(id - 0x01010001 + 39)
    } else {
        None
    }
}
struct Layers {
    v6: HashMap<u32, LayerId>,
    v7: HashMap<u32, LayerId>,
    raw: Vec<u32>,
}
fn layers(board: &Properties, output: &mut ParsedBoard) -> Result<Layers, ImportError> {
    let collect = |prefix: &str, suffix: &str| -> Result<Vec<(u32, u32, String)>, ImportError> {
        let mut rows = Vec::new();
        for (key, value) in board {
            if let Some(slot) = key
                .strip_prefix(prefix)
                .and_then(|s| s.strip_suffix(suffix))
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            {
                let slot = slot
                    .parse::<u32>()
                    .map_err(|_| error("Invalid layer slot"))?;
                let id = value
                    .parse::<u32>()
                    .map_err(|_| error("Invalid layer ID"))?;
                if raw_layer(id).is_some() {
                    let base = &key[..key.len() - 7];
                    rows.push((
                        slot,
                        id,
                        board
                            .get(&format!("{base}NAME"))
                            .cloned()
                            .unwrap_or_else(|| format!("Layer {slot}")),
                    ));
                }
            }
        }
        rows.sort_by_key(|r| r.0);
        Ok(rows)
    };
    let mut rows = collect("V9_STACK_LAYER", "_LAYERID")?;
    if rows.is_empty() {
        rows = collect("LAYER_V8_", "LAYERID")?;
    }
    if rows.is_empty() {
        let mut raw = 1;
        let mut seen = HashSet::new();
        while raw != 0 && raw != 32 {
            if !(1..=54).contains(&raw) || !seen.insert(raw) {
                return Err(error("Invalid legacy layer chain"));
            }
            rows.push((
                rows.len() as u32,
                if raw >= 39 {
                    0x01010001 + raw - 39
                } else {
                    0x01000000 + raw
                },
                board
                    .get(&format!("LAYER{raw}NAME"))
                    .cloned()
                    .unwrap_or_else(|| format!("Layer {raw}")),
            ));
            raw = number(board, &format!("LAYER{raw}NEXT"), 0.0)? as u32;
        }
        rows.push((
            rows.len() as u32,
            0x0100ffff,
            board
                .get("LAYER32NAME")
                .cloned()
                .unwrap_or_else(|| "BOTTOM".into()),
        ));
    }
    let mut result = Layers {
        v6: HashMap::new(),
        v7: HashMap::new(),
        raw: Vec::new(),
    };
    let colors = [
        "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd", "#a5b8df", "#e18d61",
    ];
    for (_, source, name) in rows {
        if result.v7.contains_key(&source) {
            continue;
        }
        let raw = raw_layer(source).ok_or_else(|| error("Invalid copper layer"))?;
        let id = LayerId(output.scene.layers.len() as u32);
        output.scene.layers.push(Layer {
            id,
            name,
            function: if raw >= 39 {
                LayerFunction::Plane
            } else {
                LayerFunction::Conductor
            },
            color: colors[id.0 as usize % colors.len()].into(),
            source_flags: None,
        });
        result.v6.insert(raw, id);
        result.v7.insert(source, id);
        result.raw.push(raw);
    }
    if result.raw.len() < 2 || result.raw.first() != Some(&1) || result.raw.last() != Some(&32) {
        return Err(error("Invalid top/bottom stack order"));
    }
    Ok(result)
}
fn copper(layers: &Layers, v6: u32, v7: u32) -> Option<LayerId> {
    layers.v7.get(&v7).or_else(|| layers.v6.get(&v6)).copied()
}
fn drawing(output: &mut ParsedBoard, board: &Properties, raw: u32) -> LayerId {
    let id = LayerId(0x20000 + raw);
    if !output.drawing_layers.iter().any(|l| l.id == id) {
        output.drawing_layers.push(SourceDrawingLayer {
            id,
            name: board
                .get(&format!("LAYER{raw}NAME"))
                .cloned()
                .unwrap_or_else(|| format!("Altium Layer {raw}")),
            color: if raw == 33 {
                "#e3e7d3"
            } else if raw == 34 {
                "#d8c5d4"
            } else {
                "#a7a9bd"
            }
            .into(),
            default_visible: raw != 34,
        });
    }
    id
}
fn net(raw: u16, output: &ParsedBoard) -> Result<NetId, ImportError> {
    if raw == 0xffff {
        return Ok(NetId(0));
    }
    let id = NetId(raw as u32 + 1);
    if !output.scene.nets.contains_key(&id) {
        return Err(error("Unknown net reference"));
    }
    Ok(id)
}
fn draw(output: &mut ParsedBoard, id: ObjectId, layer: LayerId, mut segments: Vec<Segment>) {
    let first = output
        .scene
        .drawings
        .iter()
        .rev()
        .find_map(|drawing| drawing.segments.last())
        .map_or(0x6a000000, |segment| segment.id.0 + 1);
    for (index, s) in segments.iter_mut().enumerate() {
        s.layer = layer;
        s.track_id = id;
        s.id = ObjectId(first + index as u32);
    }
    output.scene.drawings.push(BoardDrawing {
        id,
        owner_id: None,
        layer,
        net: NetId(0),
        graphic_ids: vec![id],
        segments,
        text_ids: Vec::new(),
    });
}
fn outline(board: &Properties, output: &mut ParsedBoard) -> Result<(), ImportError> {
    let mut first = None;
    let mut last = None;
    let mut push = |a, b, arc| {
        let mut s = geometry::edge(a, b, 0.05, None, false);
        s.id = ObjectId(0x6f000000 + output.scene.outline.len() as u32);
        s.track_id = s.id;
        s.arc = arc;
        output.scene.outline.push(s);
    };
    for i in 0..100_000 {
        let x = board.get(&format!("VX{i}"));
        let y = board.get(&format!("VY{i}"));
        if x.is_none() && y.is_none() {
            break;
        }
        let p = Point::new(
            mil(x.ok_or_else(|| error("Incomplete outline vertex"))?)?,
            -mil(y.ok_or_else(|| error("Incomplete outline vertex"))?)?,
        );
        let (a, b, arc) = if number(board, &format!("KIND{i}"), 0.0)? != 0.0 {
            let center = Point::new(
                dimension(board, &format!("CX{i}"))?,
                -dimension(board, &format!("CY{i}"))?,
            );
            let radius = dimension(board, &format!("R{i}"))?;
            if radius <= 0.0 {
                return Err(error("Invalid outline radius"));
            }
            let start = number(board, &format!("SA{i}"), 0.0)?;
            let end = number(board, &format!("EA{i}"), 0.0)?;
            let endpoint = |degrees: f64| {
                Point::new(
                    center.x + radius * degrees.to_radians().cos(),
                    center.y - radius * degrees.to_radians().sin(),
                )
            };
            let a = endpoint(start);
            let b = endpoint(end);
            let from_a = a.distance(p) < b.distance(p);
            let (a, b) = if from_a { (a, b) } else { (b, a) };
            let span = (end - start).rem_euclid(360.0);
            let sweep = if from_a { -1.0 } else { 1.0 }
                * if span == 0.0 { 360.0 } else { span }.to_radians();
            (
                a,
                b,
                Some(Arc {
                    center,
                    radius,
                    start: (a.y - center.y).atan2(a.x - center.x),
                    sweep,
                }),
            )
        } else {
            (p, p, None)
        };
        if let Some(previous) = last
            && a.distance(previous) > 1e-8
        {
            push(previous, a, None);
        }
        first.get_or_insert(a);
        if arc.is_some() {
            push(a, b, arc);
        }
        last = Some(b);
    }
    if let (Some(a), Some(b)) = (last, first)
        && a.distance(b) > 1e-8
    {
        push(a, b, None);
    }
    if first.is_none() {
        return Err(error("Missing physical outline"));
    }
    Ok(())
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<ParsedBoard, ImportError> {
    let compound = Compound::read(bytes, context)?;
    let mut output = scene_builder::new_parsed_board("PcbDoc");
    let properties = |name: &str| -> Result<Vec<Properties>, ImportError> {
        let count = compound.count(name, context)?;
        let data = compound.stream(&format!("{name}/Data"), context)?;
        property_records(&data, count, context)
    };
    let mut boards = properties("Board6")?;
    if boards.len() != 1 {
        return Err(error("Expected exactly one Board6 record"));
    }
    let board = boards.remove(0);
    let layers = layers(&board, &mut output)?;
    outline(&board, &mut output)?;
    for (index, record) in properties("Nets6")?.into_iter().enumerate() {
        let name = record
            .get("NAME")
            .cloned()
            .ok_or_else(|| error("Unnamed net"))?;
        output.scene.nets.insert(NetId(index as u32 + 1), name);
    }
    let components = properties("Components6")?;
    let polygons = properties("Polygons6")?;
    let filled = objects::regions(&compound, &layers, &board, &polygons, &mut output, context)?;
    objects::routes(
        &compound,
        &layers,
        &board,
        &polygons,
        &filled,
        &mut output,
        context,
    )?;
    objects::pads(
        &compound,
        &layers,
        &board,
        &components,
        &mut output,
        context,
    )?;
    objects::fills(&compound, &layers, &board, &mut output, context)?;
    objects::texts(&compound, &layers, &board, &mut output, context)?;
    scene_builder::update_scene_bounds(&mut output);
    if polygons.len() > filled.len() {
        output.diagnostics.push(format!(
            "Altium {} polygons have no saved region fill; no repour performed",
            polygons.len() - filled.len()
        ));
    }
    Ok(output)
}
