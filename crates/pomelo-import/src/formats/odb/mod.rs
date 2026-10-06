//! Native ODB++ archive, connectivity and saved-feature import.
mod archive;
mod features;
mod symbols;
use super::{
    ParsedBoard, SourceDrawingLayer, SourceSpecialLayer, SourceZone, geometry, scene_builder,
};
use crate::{ImportContext, ImportError};
use archive::Archive;
use features::Kind;
use pomelo_core::model::*;
use std::collections::{HashMap, HashSet};
fn error(details: impl Into<String>) -> ImportError {
    ImportError::Format {
        format: "ODB++".into(),
        details: details.into(),
    }
}
fn num(value: &str) -> Result<f64, ImportError> {
    let n = value
        .parse::<f64>()
        .map_err(|_| error(format!("Invalid number {value}")))?;
    if !n.is_finite() {
        return Err(error("Non-finite number"));
    }
    Ok(n)
}
fn scale(unit: &str) -> Result<f64, ImportError> {
    match unit {
        "MM" => Ok(1.0),
        "INCH" => Ok(25.4),
        _ => Err(error(format!("Unsupported unit {unit}"))),
    }
}
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}
fn fields(text: &str) -> HashMap<String, String> {
    lines(text)
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().into(), v.trim().into()))
        .collect()
}
fn blocks(text: &str, kind: &str) -> Result<Vec<HashMap<String, String>>, ImportError> {
    let mut result = Vec::new();
    let mut remaining = text;
    while let Some((_, after)) = remaining.split_once(kind) {
        let after = after.trim_start();
        if !after.starts_with('{') {
            remaining = after;
            continue;
        }
        let (body, after) = after[1..]
            .split_once('}')
            .ok_or_else(|| error("Unterminated matrix block"))?;
        result.push(fields(body));
        remaining = after;
    }
    Ok(result)
}
fn required<'a>(fields: &'a HashMap<String, String>, key: &str) -> Result<&'a str, ImportError> {
    fields
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| error(format!("Missing field {key}")))
}
fn copper(kind: &str) -> bool {
    matches!(kind, "SIGNAL" | "POWER_GROUND" | "MIXED")
}
struct SourceLayer {
    name: String,
    kind: String,
    display: Option<LayerId>,
    start: String,
    end: String,
}
#[derive(Clone)]
struct Toe {
    reference: String,
    name: String,
    at: Point,
}
struct Subnet {
    net: NetId,
    kind: String,
    toe: Option<Toe>,
}
struct Connections {
    subnets: Vec<Subnet>,
    features: HashMap<String, HashMap<usize, usize>>,
}
fn connections(
    archive: &mut Archive<'_>,
    step: &str,
    units: f64,
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<Connections, ImportError> {
    let mut toes = HashMap::new();
    for side in ["T", "B"] {
        let text = archive.text(
            &format!(
                "steps/{step}/layers/comp_+_{}/components",
                if side == "T" { "top" } else { "bot" }
            ),
            false,
        )?;
        let mut component = -1;
        let mut reference = String::new();
        let mut toe = 0;
        let mut scale = units;
        for line in lines(&text) {
            context.check_cancelled()?;
            if let Some(unit) = line.strip_prefix("UNITS=") {
                scale = super::odb::scale(unit)?;
                continue;
            }
            let t: Vec<_> = line
                .split(';')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let token = |i| {
                t.get(i)
                    .copied()
                    .ok_or_else(|| error("Truncated component/toe record"))
            };
            match t.first().copied() {
                Some("CMP") => {
                    component += 1;
                    reference = token(6)?.into();
                    toe = 0;
                }
                Some("TOP") => {
                    toes.insert(
                        format!("{side}:{component}:{toe}"),
                        Toe {
                            reference: reference.clone(),
                            name: token(8)?.into(),
                            at: Point::new(num(token(2)?)? * scale, num(token(3)?)? * scale),
                        },
                    );
                    toe += 1;
                }
                _ => {}
            }
        }
    }
    let text = archive.text(&format!("steps/{step}/eda/data"), false)?;
    let mut result = Connections {
        subnets: Vec::new(),
        features: HashMap::new(),
    };
    let mut net_index = 0u32;
    let mut net = NetId(0);
    let mut subnet = None;
    let mut layers = Vec::new();
    for line in lines(&text) {
        context.check_cancelled()?;
        let t: Vec<_> = line
            .split(';')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let token = |i| {
            t.get(i)
                .copied()
                .ok_or_else(|| error("Truncated EDA record"))
        };
        match t.first().copied() {
            Some("LYR") => layers = t[1..].iter().map(|s| s.to_ascii_lowercase()).collect(),
            Some("NET") => {
                net_index += 1;
                let name = token(1)?;
                net = NetId(if name == "$NONE$" { 0 } else { net_index });
                if net.0 != 0 {
                    output.scene.nets.insert(net, name.into());
                }
                subnet = None;
            }
            Some("SNT") => {
                let kind = token(1)?;
                let toe = if kind == "TOP" {
                    Some(
                        toes.get(&format!("{}:{}:{}", token(2)?, token(3)?, token(4)?))
                            .cloned()
                            .ok_or_else(|| error("Missing toeprint reference"))?,
                    )
                } else {
                    None
                };
                subnet = Some(result.subnets.len());
                result.subnets.push(Subnet {
                    net,
                    kind: kind.into(),
                    toe,
                });
            }
            Some("FID") => {
                if let Some(subnet) = subnet {
                    let name: String = layers
                        .get(num(token(2)?)? as usize)
                        .cloned()
                        .ok_or_else(|| error("Missing EDA layer"))?;
                    let index = num(token(3)?)? as usize;
                    let map = result.features.entry(name).or_default();
                    if map.insert(index, subnet).is_some_and(|old| old != subnet) {
                        return Err(error("Conflicting feature network mappings"));
                    }
                }
            }
            Some("PKG") => break,
            _ => {}
        }
    }
    Ok(result)
}
struct Owner {
    id: ObjectId,
    net: NetId,
    at: Point,
    angle: f64,
    mirror: bool,
    subnet: Option<usize>,
    pads: Vec<Pad>,
    drill: DrillShape,
    start: LayerId,
    end: LayerId,
}
#[expect(
    clippy::too_many_arguments,
    reason = "Joins source subnet ownership with placed geometry and identity allocation"
)]
fn owner(
    owners: &mut Vec<Owner>,
    mapping: &mut HashMap<usize, usize>,
    subnet: Option<usize>,
    connections: &Connections,
    at: Point,
    angle: f64,
    mirror: bool,
    next: &mut u32,
    copper_count: usize,
) -> usize {
    if let Some(index) = subnet.and_then(|s| mapping.get(&s).copied()) {
        return index;
    }
    let source = subnet.map(|s| &connections.subnets[s]);
    let index = owners.len();
    owners.push(Owner {
        id: ObjectId(*next),
        net: source.map_or(NetId(0), |s| s.net),
        at: source.and_then(|s| s.toe.as_ref()).map_or(at, |t| t.at),
        angle,
        mirror,
        subnet,
        pads: Vec::new(),
        drill: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        start: LayerId(0),
        end: LayerId(copper_count as u32 - 1),
    });
    *next += 1;
    if let Some(subnet) =
        subnet.filter(|&s| matches!(connections.subnets[s].kind.as_str(), "VIA" | "TOP"))
    {
        mapping.insert(subnet, index);
    }
    index
}
fn attach(
    target: &mut Owner,
    pad: &Pad,
    at: Point,
    angle: f64,
    mirror: bool,
    layer: LayerId,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let mut p = if (angle - target.angle).abs() < 1e-10 && mirror == target.mirror {
        pad.clone()
    } else {
        let paths = geometry::pad_paths(pad)
            .into_iter()
            .map(|path| {
                path.iter()
                    .map(|s| {
                        let placed = geometry::transform(s, Point::default(), angle, mirror);
                        geometry::transform(
                            &placed,
                            Point::default(),
                            if target.mirror {
                                target.angle
                            } else {
                                -target.angle
                            },
                            target.mirror,
                        )
                    })
                    .collect()
            })
            .collect();
        geometry::custom(paths, context)?
    };
    p.layer = layer;
    p.offset = Point::new(at.x - target.at.x, at.y - target.at.y);
    target.pads.push(p);
    Ok(())
}
#[expect(
    clippy::too_many_arguments,
    reason = "Drill attachment validates geometry and layer span against the same source owner"
)]
fn drill(
    target: &mut Owner,
    at: Point,
    mut angle: f64,
    mut mirror: bool,
    width: f64,
    height: f64,
    plated: bool,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let old_at = target.at;
    let old_angle = target.angle;
    let old_mirror = target.mirror;
    if width == height {
        angle = old_angle;
        mirror = old_mirror;
    }
    let pads = std::mem::take(&mut target.pads);
    target.at = at;
    target.angle = angle;
    target.mirror = mirror;
    for mut pad in pads {
        let at = Point::new(old_at.x + pad.offset.x, old_at.y + pad.offset.y);
        pad.offset = Point::default();
        let layer = pad.layer;
        attach(target, &pad, at, old_angle, old_mirror, layer, context)?;
    }
    target.drill = DrillShape {
        width,
        height,
        plated,
    };
    Ok(())
}
fn add_edge(
    output: &mut ParsedBoard,
    mut edge: Segment,
    layer: LayerId,
    net: NetId,
    next: &mut u32,
    outline: bool,
) {
    edge.id = ObjectId(*next);
    edge.track_id = edge.id;
    *next += 1;
    edge.layer = layer;
    edge.net = net;
    if outline {
        output.scene.outline.push(edge);
    } else {
        output.scene.segments.push(edge);
    }
}
fn add_zone(
    output: &mut ParsedBoard,
    mut paths: Vec<Vec<Segment>>,
    rings: Option<Vec<Vec<Point>>>,
    layer: LayerId,
    net: NetId,
    next: &mut u32,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    if paths.first().is_none_or(Vec::is_empty) {
        return Ok(());
    }
    let rings = if let Some(r) = rings {
        r
    } else {
        geometry::rings(&paths, context)?
    };
    if rings.first().is_none_or(|r| r.len() < 3) {
        return Ok(());
    }
    let id = ObjectId(*next);
    *next += 1;
    for path in &mut paths {
        for s in path {
            s.id = id;
            s.track_id = id;
            s.layer = layer;
            s.net = net;
        }
    }
    output.zones.push(SourceZone {
        id,
        layer,
        net,
        paths,
        rings,
    });
    Ok(())
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<ParsedBoard, ImportError> {
    let mut archive = Archive::read(bytes, context)?;
    let meta = fields(&archive.text("misc/info", false)?);
    let matrix = archive.text("matrix/matrix", true)?;
    let steps = blocks(&matrix, "STEP")?;
    if steps.len() != 1 {
        return Err(error("Only one board step is supported"));
    }
    let step = required(&steps[0], "NAME")?.to_ascii_lowercase();
    let header = archive.text(&format!("steps/{step}/stephdr"), true)?;
    if header.contains("STEP-REPEAT") {
        return Err(error("Step-repeat panels not supported"));
    }
    let header = fields(&header);
    let units = scale(
        header
            .get("UNITS")
            .or_else(|| meta.get("UNITS"))
            .map_or("INCH", String::as_str),
    )?;
    let mut rows = blocks(&matrix, "LAYER")?;
    rows.sort_by_key(|r| {
        r.get("ROW")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    });
    let mut output = scene_builder::new_parsed_board(format!(
        "{}.{}",
        meta.get("ODB_VERSION_MAJOR").map_or("?", String::as_str),
        meta.get("ODB_VERSION_MINOR").map_or("0", String::as_str)
    ));
    let colors = [
        "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd", "#a5b8df", "#e18d61",
    ];
    let mut layers = Vec::new();
    let mut seen = HashSet::new();
    let mut copper_ids = HashMap::new();
    for row in rows {
        let source_name = required(&row, "NAME")?;
        let name = source_name.to_ascii_lowercase();
        let kind = required(&row, "TYPE")?.to_owned();
        if !seen.insert(name.clone()) || row.get("POLARITY").is_some_and(|p| p != "POSITIVE") {
            return Err(error("Duplicate layer/negative layer polarity"));
        }
        let display = if copper(&kind) {
            let id = LayerId(output.scene.layers.len() as u32);
            copper_ids.insert(name.clone(), id);
            output.scene.layers.push(Layer {
                id,
                name: source_name.into(),
                function: if kind == "POWER_GROUND" {
                    LayerFunction::Plane
                } else {
                    LayerFunction::Conductor
                },
                color: colors[id.0 as usize % colors.len()].into(),
                source_flags: None,
            });
            Some(id)
        } else if kind != "DIELECTRIC" && kind != "COMPONENT" {
            let id = LayerId(0x30000 + output.special_layers.len() as u32);
            let label = format!("{kind} / {name}");
            output.special_layers.push(SourceSpecialLayer {
                id,
                name: label.clone(),
                color: if kind == "SILK_SCREEN" {
                    "#e4dfcc"
                } else {
                    "#94b0ba"
                }
                .into(),
                kind: SpecialLayerKind::Graphic,
            });
            output.drawing_layers.push(SourceDrawingLayer {
                id,
                name: label,
                color: "#94b0ba".into(),
                default_visible: kind == "SILK_SCREEN"
                    && !name.contains("bot")
                    && !name.contains("ssb"),
            });
            Some(id)
        } else {
            None
        };
        layers.push(SourceLayer {
            name,
            kind,
            display,
            start: row
                .get("START_NAME")
                .map_or(String::new(), |s| s.to_ascii_lowercase()),
            end: row
                .get("END_NAME")
                .map_or(String::new(), |s| s.to_ascii_lowercase()),
        });
    }
    if output.scene.layers.is_empty() {
        return Err(error("No copper layers"));
    }
    let mut connections = connections(&mut archive, &step, units, &mut output, context)?;
    let mut symbols = symbols::Symbols::new();
    layers.sort_by_key(|l| {
        if copper(&l.kind) {
            0
        } else if l.kind == "DRILL" {
            1
        } else if l.display.is_some() {
            2
        } else {
            3
        }
    });
    let mut owners = Vec::new();
    let mut owner_ids = HashMap::new();
    let mut next = 1u32;
    let mut copper_bounds = None;
    for l in layers {
        context.check_cancelled()?;
        if !copper(&l.kind) && copper_bounds.is_none() {
            scene_builder::update_scene_bounds(&mut output);
            copper_bounds = Some(output.scene.bounds);
        }
        let text = archive.text(&format!("steps/{step}/layers/{}/features", l.name), false)?;
        let mappings = connections.features.remove(&l.name).unwrap_or_default();
        let mut seen = HashSet::new();
        let display = l.display.unwrap_or(LayerId::UNASSIGNED);
        features::parse(&text, units, context, |feature| {
            let subnet = mappings.get(&feature.index).copied();
            let net = subnet.map_or(NetId(0), |s| connections.subnets[s].net);
            if subnet.is_some() {
                seen.insert(feature.index);
            }
            if l.display.is_none() && l.kind != "DRILL" {
                return Err(error("Unsupported nonphysical feature layer"));
            }
            match feature.kind {
                Kind::Surface(contours) => {
                    if l.kind == "DRILL" {
                        return Err(error("Drill surface not supported"));
                    }
                    for island in symbols::islands(contours, context)? {
                        add_zone(
                            &mut output,
                            island.paths,
                            Some(island.rings),
                            display,
                            net,
                            &mut next,
                            context,
                        )?;
                    }
                }
                Kind::Line(mut stroke, symbol) => {
                    let brush = symbols.read(&symbol, &mut archive, units, context)?;
                    if brush.pads.len() != 1
                        || brush.pads[0].kind != PadKind::CIRCLE
                        || !brush.strokes.is_empty()
                    {
                        return Err(error("Nonround line brush"));
                    }
                    stroke.width = brush.pads[0].width;
                    if l.kind == "DRILL" && stroke.width > 0.0 {
                        if stroke.arc.is_some() {
                            return Err(error("Curved drill slot not supported"));
                        }
                        let at = Point::new(
                            (stroke.a.x + stroke.b.x) * 0.5,
                            (stroke.a.y + stroke.b.y) * 0.5,
                        );
                        let angle = (stroke.b.y - stroke.a.y).atan2(stroke.b.x - stroke.a.x);
                        let i = owner(
                            &mut owners,
                            &mut owner_ids,
                            subnet,
                            &connections,
                            at,
                            angle,
                            false,
                            &mut next,
                            copper_ids.len(),
                        );
                        drill(
                            &mut owners[i],
                            at,
                            angle,
                            false,
                            stroke.a.distance(stroke.b) + stroke.width,
                            stroke.width,
                            feature.attrs.get(".drill").is_none_or(|v| v != "1"),
                            context,
                        )?;
                    } else {
                        add_edge(&mut output, stroke, display, net, &mut next, false);
                    }
                }
                Kind::Pad {
                    at,
                    angle,
                    mirror,
                    symbol,
                } => {
                    let symbol = symbols.read(&symbol, &mut archive, units, context)?;
                    if l.kind == "DRILL" {
                        if symbol.pads.len() != 1
                            || ![2, 11].contains(&symbol.pads[0].kind.0)
                            || !symbol.strokes.is_empty()
                        {
                            return Err(error("Unsupported hole aperture"));
                        }
                        let i = owner(
                            &mut owners,
                            &mut owner_ids,
                            subnet,
                            &connections,
                            at,
                            angle,
                            mirror,
                            &mut next,
                            copper_ids.len(),
                        );
                        let shape = &symbol.pads[0];
                        drill(
                            &mut owners[i],
                            at,
                            angle,
                            mirror,
                            shape.width,
                            shape.height,
                            feature.attrs.get(".drill").is_none_or(|v| v != "1"),
                            context,
                        )?;
                        if subnet.is_some_and(|s| connections.subnets[s].kind == "VIA") {
                            owners[i].start = *copper_ids
                                .get(&l.start)
                                .ok_or_else(|| error("Missing drill start layer"))?;
                            owners[i].end = *copper_ids
                                .get(&l.end)
                                .ok_or_else(|| error("Missing drill end layer"))?;
                        }
                    } else if copper(&l.kind) {
                        let i = owner(
                            &mut owners,
                            &mut owner_ids,
                            subnet,
                            &connections,
                            at,
                            angle,
                            mirror,
                            &mut next,
                            copper_ids.len(),
                        );
                        for pad in &symbol.pads {
                            attach(&mut owners[i], pad, at, angle, mirror, display, context)?;
                        }
                        for stroke in &symbol.strokes {
                            add_edge(
                                &mut output,
                                geometry::transform(stroke, at, angle, mirror),
                                display,
                                net,
                                &mut next,
                                false,
                            );
                        }
                    } else {
                        for pad in &symbol.pads {
                            if let Some(inner) = pad.inner_diameter {
                                let radius = (pad.width + inner) * 0.25;
                                let width = (pad.width - inner) * 0.5;
                                let s = geometry::edge(
                                    Point::new(radius, 0.0),
                                    Point::new(radius, 0.0),
                                    width,
                                    Some(Point::default()),
                                    false,
                                );
                                add_edge(
                                    &mut output,
                                    geometry::transform(&s, at, angle, mirror),
                                    display,
                                    net,
                                    &mut next,
                                    false,
                                );
                            } else {
                                let paths = geometry::pad_paths(pad)
                                    .into_iter()
                                    .map(|p| {
                                        p.iter()
                                            .map(|s| geometry::transform(s, at, angle, mirror))
                                            .collect()
                                    })
                                    .collect();
                                add_zone(
                                    &mut output,
                                    paths,
                                    None,
                                    display,
                                    net,
                                    &mut next,
                                    context,
                                )?;
                            }
                        }
                        for stroke in &symbol.strokes {
                            add_edge(
                                &mut output,
                                geometry::transform(stroke, at, angle, mirror),
                                display,
                                net,
                                &mut next,
                                false,
                            );
                        }
                    }
                }
            }
            Ok(())
        })?;
        if seen.len() != mappings.len() {
            return Err(error(format!(
                "Unresolved feature references on {}",
                l.name
            )));
        }
    }
    if !connections.features.is_empty() {
        return Err(error("EDA references undefined matrix layer"));
    }
    let mut component_ids = HashMap::new();
    for o in owners {
        let source = o.subnet.map(|s| &connections.subnets[s]);
        if source.is_some_and(|s| s.kind == "VIA") {
            output.scene.vias.push(Via {
                id: o.id,
                net: o.net,
                at: o.at,
                drill: o.drill.width,
                drill_shape: o.drill,
                padstack: ObjectId(o.subnet.unwrap_or(0) as u32 + 1),
                padstack_name: String::new(),
                start_layer: Some(o.start),
                end_layer: Some(o.end),
                pads: o.pads.into(),
                backdrill: None,
                stackup_region: None,
                angle: o.angle,
                mirrored: o.mirror,
                finger: None,
            });
        } else {
            let toe = source.and_then(|s| s.toe.as_ref());
            let reference = toe.map_or("", |t| t.reference.as_str());
            let component = if reference.is_empty() {
                None
            } else if let Some(&i) = component_ids.get(reference) {
                Some(i)
            } else {
                let i = output.scene.components.len();
                let id = ObjectId(next);
                next += 1;
                component_ids.insert(reference.to_owned(), i);
                output.scene.components.push(ComponentPlacement {
                    id,
                    source_reference: None,
                    reference: reference.into(),
                    at: o.at,
                    angle: o.angle,
                    mirrored: o.mirror,
                    pins: Vec::new(),
                });
                Some(i)
            };
            let owner_id = if let Some(i) = component {
                output.scene.components[i].pins.push(o.id);
                output.scene.components[i].id
            } else {
                ObjectId(0)
            };
            output.scene.pins.push(Pin {
                id: o.id,
                owner_id,
                net: o.net,
                name: toe.map_or(String::new(), |t| t.name.clone()),
                reference: reference.into(),
                at: o.at,
                angle: o.angle,
                mirrored: o.mirror,
                drill: o.drill.width,
                drill_shape: o.drill,
                pads: o.pads,
                stackup_region: None,
                die: None,
            });
        }
    }
    let profile = archive.text(&format!("steps/{step}/profile"), true)?;
    features::parse(&profile, units, context, |feature| {
        let Kind::Surface(contours) = feature.kind else {
            return Err(error("Profile must be a surface"));
        };
        for c in contours {
            for edge in c.path {
                add_edge(
                    &mut output,
                    edge,
                    LayerId::UNASSIGNED,
                    NetId(0),
                    &mut next,
                    true,
                );
            }
        }
        Ok(())
    })?;
    scene_builder::update_scene_bounds(&mut output);
    if let Some(b) = copper_bounds.filter(|b| b.is_valid()) {
        output.scene.bounds = b;
        for edge in &output.scene.outline {
            if let Some(b) = edge.bounds() {
                output.scene.bounds.include(b.min);
                output.scene.bounds.include(b.max);
            }
        }
    }
    if archive.opaque > 0 {
        output.diagnostics.push(format!("ODB++ {} opaque description properties omitted; geometry/references decoded independently",archive.opaque));
    }
    Ok(output)
}
