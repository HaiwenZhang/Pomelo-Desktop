//! Native KiCad board reader, with source layer/net identities shared across object families.
use super::{
    ParsedBoard, SourceDrawingLayer, SourceZone, scene_builder,
    sexpr::{self, Node, Value, error},
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc as Shared,
};

fn point(node: &Node) -> Result<Point, ImportError> {
    Ok(Point::new(node.number(1)?, -node.number(2)?))
}
fn id(value: f64) -> Result<u32, ImportError> {
    if value < 0.0 || value > u32::MAX as f64 || value.fract() != 0.0 {
        Err(error("Invalid integer identifier"))
    } else {
        Ok(value as u32)
    }
}
fn arc(a: Point, m: Point, b: Point) -> Result<Option<Arc>, ImportError> {
    let mx = m.x - a.x;
    let my = m.y - a.y;
    let bx = b.x - a.x;
    let by = b.y - a.y;
    let cross = mx * by - my * bx;
    let span = bx.hypot(by);
    if span == 0.0 {
        let radius = mx.hypot(my) * 0.5;
        if radius <= 0.0 {
            return Err(error("Coincident arc points"));
        }
        let center = Point::new((a.x + m.x) * 0.5, (a.y + m.y) * 0.5);
        return Ok(Some(Arc {
            center,
            radius,
            start: (a.y - center.y).atan2(a.x - center.x),
            sweep: std::f64::consts::TAU,
        }));
    }
    if cross.abs() / span <= 1e-9 {
        let projection = mx * bx + my * by;
        if projection >= -1e-12 && projection <= bx * bx + by * by + 1e-12 {
            return Ok(None);
        }
        return Err(error("Degenerate arc midpoint outside endpoints"));
    }
    let mm = mx * mx + my * my;
    let bb = bx * bx + by * by;
    let center = Point::new(
        a.x + (mm * by - bb * my) / (2.0 * cross),
        a.y + (bb * mx - mm * bx) / (2.0 * cross),
    );
    let radius = a.distance(center);
    let start = (a.y - center.y).atan2(a.x - center.x);
    let ccw = ((b.y - center.y).atan2(b.x - center.x) - start).rem_euclid(std::f64::consts::TAU);
    let mid = ((m.y - center.y).atan2(m.x - center.x) - start).rem_euclid(std::f64::consts::TAU);
    let sweep = if mid <= ccw + 1e-9 {
        ccw
    } else {
        ccw - std::f64::consts::TAU
    };
    if !radius.is_finite() || radius <= 0.0 || sweep.abs() < 1e-9 {
        return Err(error("Invalid arc radius/sweep"));
    }
    Ok(Some(Arc {
        center,
        radius,
        start,
        sweep,
    }))
}
fn net(
    node: &Node,
    output: &mut ParsedBoard,
    names: &mut HashMap<String, u32>,
) -> Result<NetId, ImportError> {
    let Some(field) = node.find_child("net") else {
        return Ok(NetId(0));
    };
    let value = field.atom(1)?;
    if value.is_empty() {
        return Ok(NetId(0));
    }
    if value.bytes().all(|b| b.is_ascii_digit()) {
        let value = value
            .parse::<u32>()
            .map_err(|_| error("Invalid net number"))?;
        if value != 0 && !output.scene.nets.contains_key(&NetId(value)) {
            return Err(error("Unknown net number"));
        }
        return Ok(NetId(value));
    }
    if let Some(&id) = names.get(value) {
        return Ok(NetId(id));
    }
    let next = output.scene.nets.keys().next_back().map_or(Ok(1), |n| {
        n.0.checked_add(1).ok_or_else(|| error("Net ID overflow"))
    })?;
    names.insert(value.into(), next);
    output.scene.nets.insert(NetId(next), value.into());
    Ok(NetId(next))
}
fn segment(
    a: Point,
    b: Point,
    width: f64,
    layer: LayerId,
    net: NetId,
    id: u32,
    arc: Option<Arc>,
) -> Result<Segment, ImportError> {
    if width < 0.0 {
        return Err(error("Negative stroke width"));
    }
    Ok(Segment {
        id: ObjectId(id),
        track_id: ObjectId(id),
        layer,
        net,
        a,
        b,
        width,
        arc,
        bond_wire: None,
    })
}
fn split_ring(ring: Vec<Point>) -> Vec<Vec<Point>> {
    let count = ring.len();
    let mut next: Vec<_> = (0..count).map(|i| (i + 1) % count).collect();
    let mut pending = HashMap::<[u64; 4], Vec<usize>>::new();
    let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    let mut bridges = 0;
    for i in 0..count {
        let j = next[i];
        let a = ring[i];
        let b = ring[j];
        if a == b {
            continue;
        }
        let forward = [bits(a.x), bits(a.y), bits(b.x), bits(b.y)];
        let reverse = [bits(b.x), bits(b.y), bits(a.x), bits(a.y)];
        if let Some(mate) = pending.get_mut(&reverse).and_then(Vec::pop) {
            next[i] = (mate + 1) % count;
            next[mate] = j;
            bridges += 1;
        } else {
            pending.entry(forward).or_default().push(i);
        }
    }
    if bridges == 0 {
        return vec![ring];
    }
    let mut seen = vec![false; count];
    let mut contours = Vec::new();
    for start in 0..count {
        if seen[start] {
            continue;
        }
        let mut at = start;
        let mut points = Vec::new();
        let mut area = 0.0;
        loop {
            if seen[at] {
                return vec![ring];
            }
            seen[at] = true;
            let following = next[at];
            let a = ring[at];
            let b = ring[following];
            if points.last() != Some(&a) {
                points.push(a);
            }
            area += a.x * b.y - b.x * a.y;
            at = following;
            if at == start {
                break;
            }
        }
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return vec![ring];
        }
        contours.push((points, area));
    }
    let Some(outer) = contours
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i)
    else {
        return vec![ring];
    };
    let sign = contours[outer].1.signum();
    if sign == 0.0
        || contours
            .iter()
            .enumerate()
            .any(|(i, c)| i != outer && c.1.signum() == sign)
    {
        return vec![ring];
    }
    let outer = contours.remove(outer);
    std::iter::once(outer.0)
        .chain(contours.into_iter().map(|c| c.0))
        .collect()
}
fn layer(name: &str, layers: &HashMap<String, u32>) -> Result<LayerId, ImportError> {
    layers
        .get(name)
        .copied()
        .map(LayerId)
        .ok_or_else(|| error(format!("Unknown copper layer {name}")))
}
fn pad_kind(name: &str) -> Result<PadKind, ImportError> {
    Ok(PadKind(match name {
        "circle" => 2,
        "rect" => 6,
        "oval" => 11,
        "roundrect" => 27,
        "custom" => 22,
        _ => return Err(error(format!("Unsupported pad shape {name}"))),
    }))
}

pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<ParsedBoard, ImportError> {
    let root = sexpr::read(bytes, context)?;
    if root.head() != "kicad_pcb" {
        return Err(error("Root is not kicad_pcb"));
    }
    let version = id(root.required("version")?.number(1)?)?;
    let mut output = scene_builder::new_parsed_board(version);
    if root.children("layers").count() != 1 {
        return Err(error("Missing or duplicate layer table"));
    }
    let colors = [
        "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd",
    ];
    let mut layers = HashMap::new();
    for value in &root.required("layers")?.values {
        if let Value::List(node) = value {
            let name = node.atom(1)?;
            if !name.ends_with(".Cu") {
                continue;
            }
            let id = output.scene.layers.len() as u32;
            if layers.insert(name.to_owned(), id).is_some() {
                return Err(error("Duplicate copper layer"));
            }
            output.scene.layers.push(Layer {
                id: LayerId(id),
                name: name.into(),
                function: LayerFunction::Conductor,
                color: colors[id as usize % colors.len()].into(),
                source_flags: None,
            });
        }
    }
    if layers.len() < 2 {
        return Err(error("Board needs two copper layers"));
    }
    let mut net_names = HashMap::new();
    let mut seen = HashSet::new();
    for node in root.children("net") {
        let id = id(node.number(1)?)?;
        if !seen.insert(id) {
            return Err(error("Duplicate net ID"));
        }
        let name = node.atom(2)?.to_owned();
        if id != 0 {
            net_names.insert(name.clone(), id);
            output.scene.nets.insert(NetId(id), name);
        }
    }
    for kind in ["segment", "arc"] {
        for node in root.children(kind) {
            context.check_cancelled()?;
            let a = point(node.required("start")?)?;
            let b = point(node.required("end")?)?;
            let width = node.required("width")?.number(1)?;
            if width <= 0.0 {
                return Err(error("Nonpositive track width"));
            }
            let layer = layer(node.required("layer")?.atom(1)?, &layers)?;
            let net = net(node, &mut output, &mut net_names)?;
            let arc = if kind == "arc" {
                arc(a, point(node.required("mid")?)?, b)?
            } else {
                None
            };
            output.scene.segments.push(segment(
                a,
                b,
                width,
                layer,
                net,
                0x40000000 + output.scene.segments.len() as u32,
                arc,
            )?);
        }
    }
    for node in root.children("via") {
        context.check_cancelled()?;
        if node.find_child("padstack").is_some() {
            return Err(error("Per-layer via padstack not supported"));
        }
        let at = point(node.required("at")?)?;
        let size = node.required("size")?.number(1)?;
        let drill = node.required("drill")?.number(1)?;
        if size <= 0.0 || drill < 0.0 || drill >= size {
            return Err(error("Invalid via size/drill"));
        }
        let pair = node.required("layers")?;
        let a = layer(pair.atom(1)?, &layers)?.0;
        let b = layer(pair.atom(2)?, &layers)?.0;
        if a == b {
            return Err(error("Zero via layer span"));
        }
        let net = net(node, &mut output, &mut net_names)?;
        let index = output.scene.vias.len() as u32;
        output.scene.vias.push(Via {
            id: ObjectId(0x50000000 + index),
            net,
            at,
            drill,
            drill_shape: DrillShape {
                width: drill,
                height: drill,
                plated: true,
            },
            padstack: ObjectId(index),
            padstack_name: String::new(),
            start_layer: Some(LayerId(a.min(b))),
            end_layer: Some(LayerId(a.max(b))),
            pads: (a.min(b)..=a.max(b))
                .map(|i| Pad::circle(LayerId(i), size))
                .collect::<Vec<_>>()
                .into(),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        });
    }
    for (index, fp) in root
        .children("footprint")
        .chain(root.children("module"))
        .enumerate()
    {
        context.check_cancelled()?;
        let at_node = fp.required("at")?;
        let origin = point(at_node)?;
        let rotation = at_node.number_or(3, 0.0)?.to_radians();
        let back = fp.required("layer")?.atom(1)? == "B.Cu";
        let reference = fp
            .children("property")
            .find(|p| p.atom(1).ok() == Some("Reference"))
            .or_else(|| {
                fp.children("fp_text")
                    .find(|p| p.atom(1).ok() == Some("reference"))
            })
            .and_then(|p| p.atom(2).ok())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("FP{}", index + 1));
        let component_id = ObjectId(0x79000000 + index as u32);
        let mut component = ComponentPlacement {
            id: component_id,
            source_reference: None,
            reference: reference.clone(),
            at: origin,
            angle: rotation,
            mirrored: back,
            pins: Vec::new(),
        };
        for node in fp.children("pad") {
            context.check_cancelled()?;
            let name = node.atom(1)?.to_owned();
            let kind = node.atom(2)?;
            if !["smd", "thru_hole", "np_thru_hole", "connect"].contains(&kind) {
                return Err(error("Unsupported pad type"));
            }
            let shape = node.atom(3)?;
            let at_node = node.required("at")?;
            let shift = point(at_node)?.rotate(rotation);
            let at = Point::new(origin.x + shift.x, origin.y + shift.y);
            let angle = at_node.number_or(3, 0.0)?.to_radians();
            let size = node.required("size")?;
            let width = size.number(1)?;
            let height = size.number(2)?;
            if width < 0.0 || height < 0.0 {
                return Err(error("Negative pad size"));
            }
            let mut copper = HashSet::new();
            for v in &node.required("layers")?.values[1..] {
                let Value::Atom(name) = v else {
                    return Err(error("Invalid pad layer field"));
                };
                if name == "*.Cu" {
                    copper.extend(layers.values().copied());
                } else if name == "F&B.Cu" {
                    copper.insert(0);
                    copper.insert(layers.len() as u32 - 1);
                } else if name.ends_with(".Cu") {
                    copper.insert(layer(name, &layers)?.0);
                }
            }
            let mut drill_shape = DrillShape {
                width: 0.0,
                height: 0.0,
                plated: kind != "np_thru_hole",
            };
            if let Some(drill) = node.find_child("drill") {
                let oval = drill.atom(1)? == "oval";
                drill_shape.width = drill.number(if oval { 2 } else { 1 })?;
                drill_shape.height = if oval {
                    drill.number(3)?
                } else {
                    drill_shape.width
                };
                if drill_shape.width <= 0.0 || drill_shape.height <= 0.0 {
                    return Err(error("Invalid pad drill"));
                }
                if drill.find_child("offset").is_some() {
                    output
                        .diagnostics
                        .push("KiCad offset drill position is not represented".into());
                }
            }
            let mut overrides = HashMap::new();
            if let Some(stack) = node.find_child("padstack") {
                for row in stack.children("layer") {
                    let name = row.atom(1)?;
                    let shape = row.required("shape")?.atom(1)?;
                    let size = row.required("size")?;
                    let targets: Vec<_> = if name == "Inner" {
                        copper
                            .iter()
                            .copied()
                            .filter(|&i| i > 0 && i < layers.len() as u32 - 1)
                            .collect()
                    } else {
                        vec![layer(name, &layers)?.0]
                    };
                    if targets.is_empty() {
                        return Err(error("Unsupported padstack layer"));
                    }
                    for target in targets {
                        if name == "Inner" && overrides.contains_key(&target) {
                            continue;
                        }
                        overrides.insert(target, (shape, size.number(1)?, size.number(2)?));
                    }
                }
            }
            let mut custom = Vec::new();
            if shape == "custom" {
                for v in &node.required("primitives")?.values[1..] {
                    let Value::List(poly) = v else {
                        return Err(error("Invalid custom pad primitive"));
                    };
                    if poly.head() != "gr_poly" {
                        return Err(error("Unsupported custom pad primitive"));
                    }
                    let ring = poly
                        .required("pts")?
                        .children("xy")
                        .map(point)
                        .map(|p| p.map(|p| Point::new(p.x, if back { -p.y } else { p.y })))
                        .collect::<Result<Vec<_>, _>>()?;
                    if ring.len() < 3 {
                        return Err(error("Short custom pad polygon"));
                    }
                    custom.push(ring);
                }
                if custom.is_empty() {
                    return Err(error("Empty custom pad"));
                }
            }
            let mut copper: Vec<_> = copper.into_iter().collect();
            copper.sort_unstable();
            let mut pads = Vec::new();
            for layer in copper {
                let (shape, width, height) = overrides
                    .get(&layer)
                    .copied()
                    .unwrap_or((shape, width, height));
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                let kind = pad_kind(shape)?;
                if shape == "circle" && (width - height).abs() > 1e-6 {
                    return Err(error("Unequal circular pad dimensions"));
                }
                pads.push(Pad {
                    layer: LayerId(layer),
                    width,
                    height,
                    offset: Point::default(),
                    kind,
                    corner: if shape == "roundrect" {
                        node.required("roundrect_rratio")?.number(1)? * width.min(height)
                    } else {
                        0.0
                    },
                    inner_diameter: None,
                    custom: if shape == "custom" {
                        Some(Shared::new(CustomPadGeometry {
                            contours: custom.clone(),
                            paths: Vec::new(),
                        }))
                    } else {
                        None
                    },
                    backdrill: false,
                    backdrill_base: false,
                });
            }
            let id = ObjectId(0x70000000 + output.scene.pins.len() as u32);
            let net = net(node, &mut output, &mut net_names)?;
            component.pins.push(id);
            output.scene.pins.push(Pin {
                id,
                owner_id: component_id,
                net,
                name,
                reference: reference.clone(),
                at,
                angle,
                mirrored: back,
                drill: drill_shape.width.max(drill_shape.height),
                drill_shape,
                pads,
                stackup_region: None,
                die: None,
            });
        }
        output.scene.components.push(component);
    }
    if !output.scene.components.is_empty() {
        output
            .diagnostics
            .push("KiCad footprint graphics and text are not yet imported".into());
    }
    for node in root.children("zone") {
        context.check_cancelled()?;
        let fills: Vec<_> = node.children("filled_polygon").collect();
        if fills.is_empty() {
            if node.find_child("keepout").is_none() {
                output
                    .diagnostics
                    .push("KiCad design zone without saved fill; no repour performed".into());
            }
            continue;
        }
        let net = net(node, &mut output, &mut net_names)?;
        for fill in fills {
            let layer = layer(fill.required("layer")?.atom(1)?, &layers)?;
            let ring = fill
                .required("pts")?
                .children("xy")
                .map(point)
                .collect::<Result<Vec<_>, _>>()?;
            if ring.len() < 3 {
                return Err(error("Short filled polygon"));
            }
            output.zones.push(SourceZone {
                id: ObjectId(0x78000000 + output.zones.len() as u32),
                layer,
                net,
                paths: Vec::new(),
                rings: split_ring(ring),
            });
        }
    }
    graphics(&root, &mut output, context)?;
    scene_builder::update_scene_bounds(&mut output);
    Ok(output)
}

fn graphics(
    root: &Node,
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let mut drawing_layers = HashMap::new();
    let mut next_segment = 0;
    for kind in ["gr_line", "gr_arc", "gr_rect", "gr_poly", "gr_circle"] {
        for node in root.children(kind) {
            context.check_cancelled()?;
            let name = node.required("layer")?.atom(1)?;
            let edge = name == "Edge.Cuts";
            let layer = if edge {
                LayerId::UNASSIGNED
            } else if let Some(&id) = drawing_layers.get(name) {
                LayerId(id)
            } else {
                let id = 0x20000 + output.drawing_layers.len() as u32;
                drawing_layers.insert(name.to_owned(), id);
                output.drawing_layers.push(SourceDrawingLayer {
                    id: LayerId(id),
                    name: name.into(),
                    color: "#90a7b9".into(),
                    default_visible: true,
                });
                LayerId(id)
            };
            let width = if let Some(stroke) = node.find_child("stroke") {
                stroke.required("width")?.number(1)?
            } else {
                node.find_child("width")
                    .map(|w| w.number(1))
                    .transpose()?
                    .unwrap_or(0.0)
            };
            if node
                .find_child("fill")
                .and_then(|n| n.atom(1).ok())
                .is_some_and(|v| v == "yes" || v == "solid")
            {
                output
                    .diagnostics
                    .push("KiCad filled graphic rendered as boundary".into());
            }
            let mut segments = Vec::new();
            let mut push = |a, b, arc| -> Result<(), ImportError> {
                let id = 0x60000000 + next_segment;
                next_segment += 1;
                segments.push(segment(a, b, width, layer, NetId(0), id, arc)?);
                Ok(())
            };
            match kind {
                "gr_line" => push(
                    point(node.required("start")?)?,
                    point(node.required("end")?)?,
                    None,
                )?,
                "gr_arc" => {
                    let a = point(node.required("start")?)?;
                    let m = point(node.required("mid")?)?;
                    let b = point(node.required("end")?)?;
                    push(a, b, arc(a, m, b)?)?;
                }
                "gr_rect" => {
                    let a = point(node.required("start")?)?;
                    let b = point(node.required("end")?)?;
                    let corners = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
                    for i in 0..4 {
                        push(corners[i], corners[(i + 1) % 4], None)?;
                    }
                }
                "gr_circle" => {
                    let center = point(node.required("center")?)?;
                    let end = point(node.required("end")?)?;
                    let radius = end.distance(center);
                    if radius <= 0.0 {
                        return Err(error("Invalid graphic circle"));
                    }
                    let start = (end.y - center.y).atan2(end.x - center.x);
                    for i in 0..4 {
                        let t = start + i as f64 * std::f64::consts::FRAC_PI_2;
                        let next = t + std::f64::consts::FRAC_PI_2;
                        push(
                            Point::new(center.x + radius * t.cos(), center.y + radius * t.sin()),
                            Point::new(
                                center.x + radius * next.cos(),
                                center.y + radius * next.sin(),
                            ),
                            Some(Arc {
                                center,
                                radius,
                                start: t,
                                sweep: std::f64::consts::FRAC_PI_2,
                            }),
                        )?;
                    }
                }
                _ => {
                    let mut first = None;
                    let mut last = None;
                    for value in &node.required("pts")?.values[1..] {
                        let Value::List(value) = value else {
                            return Err(error("Invalid polygon vertex"));
                        };
                        let (a, b, arc) = if value.head() == "xy" {
                            let p = point(value)?;
                            (p, p, None)
                        } else if value.head() == "arc" {
                            let a = point(value.required("start")?)?;
                            let b = point(value.required("end")?)?;
                            (a, b, arc(a, point(value.required("mid")?)?, b)?)
                        } else {
                            return Err(error("Unknown polygon vertex type"));
                        };
                        if let Some(previous) = last
                            && previous != a
                        {
                            push(previous, a, None)?;
                        }
                        first.get_or_insert(a);
                        if a != b || arc.is_some() {
                            push(a, b, arc)?;
                        }
                        last = Some(b);
                    }
                    let a = first.ok_or_else(|| error("Empty graphic polygon"))?;
                    let b = last.ok_or_else(|| error("Empty graphic polygon"))?;
                    if a != b {
                        push(b, a, None)?;
                    }
                }
            }
            if edge {
                output.scene.outline.extend(segments);
            } else {
                let id = ObjectId(0x68000000 + output.scene.drawings.len() as u32);
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
        }
    }
    if root.children("gr_text").next().is_some() {
        output
            .diagnostics
            .push("KiCad board text is not yet imported".into());
    }
    Ok(())
}
