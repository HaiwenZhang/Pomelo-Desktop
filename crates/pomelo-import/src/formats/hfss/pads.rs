//! Pad definitions and bindings are decoded once and shared by placed instances.
use super::super::{ParsedBoard, SourceSpecialLayer, geometry as shared};
use super::{
    Layer,
    binary::Object,
    error, geometry,
    text::{self, Block, Call, Value},
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

struct Definition {
    id: i32,
    name: String,
    layers: HashMap<i32, Call>,
    hole: geometry::Drill,
    plated: bool,
}
struct Binding {
    definition: i32,
    first: i32,
    last: i32,
    die: bool,
    templates: Vec<Pad>,
}
fn integer(v: &Value) -> Result<i32, ImportError> {
    let n = v.number()?;
    if n.fract() != 0.0 || n < i32::MIN as f64 || n > i32::MAX as f64 {
        return Err(error("Invalid padstack integer"));
    }
    Ok(n as i32)
}
fn call<'a>(b: &'a Block, name: &str) -> Result<&'a Call, ImportError> {
    let mut calls = b.calls.iter().filter(|c| c.name == name);
    let first = calls
        .next()
        .ok_or_else(|| error(format!("Missing {name} call")))?;
    if calls.next().is_some() {
        return Err(error(format!("Duplicate {name} call")));
    }
    Ok(first)
}
fn rotate(p: Point, angle: f64) -> Point {
    let (s, c) = angle.sin_cos();
    Point::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Source tables and scene are shared across layout readers"
)]
pub(super) fn build(
    root: &Object,
    cell: &Object,
    layout: &Object,
    layers: &[Layer],
    layer_ids: &HashMap<i32, LayerId>,
    net_ids: &HashMap<i32, NetId>,
    components: &HashMap<i32, String>,
    output: &mut ParsedBoard,
    next: &mut u32,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let metadata = text::read(root.field(0)?.text()?)?;
    let mut definitions = HashMap::new();
    if let Some(pds) = metadata.children.iter().find(|c| c.name == "pds") {
        for record in &pds.children {
            context.check_cancelled()?;
            if record.name != "pd" {
                return Err(error("Unknown padstack definition"));
            }
            let id = integer(record.prop("id")?)?;
            let source = record.require_child("psd")?;
            let mut pads = HashMap::new();
            if let Some(pds) = source.children.iter().find(|c| c.name == "pds") {
                for layer in &pds.children {
                    if layer.name != "lgm" {
                        return Err(error("Unknown padstack layer"));
                    }
                    let id = integer(layer.prop("id")?)?;
                    layer.prop("lay")?.text()?;
                    if pads.insert(id, call(layer, "pad")?.clone()).is_some() {
                        return Err(error("Duplicate padstack layer"));
                    }
                }
            }
            let hole = geometry::drill(call(source, "hle")?, context)?;
            let plated = source
                .properties
                .get("plt")
                .and_then(|v| v.number().ok())
                .is_some_and(|n| n > 0.0);
            let definition = Definition {
                id,
                name: source.prop("nam")?.text()?.into(),
                layers: pads,
                hole,
                plated,
            };
            if definitions.insert(id, definition).is_some() {
                return Err(error("Duplicate padstack definition"));
            }
        }
    }
    let source_ids: HashSet<_> = layers.iter().map(|l| l.id).collect();
    let mut bindings = HashMap::new();
    for value in cell.field(3)?.array()? {
        context.check_cancelled()?;
        let record = value.object(Some(6))?;
        let id = record.field(0)?.integer()?;
        let source = text::read(record.field(1)?.text()?)?;
        let definition = integer(source.prop("def")?)?;
        let def = definitions
            .get(&definition)
            .ok_or_else(|| error("Missing padstack definition"))?;
        let first = integer(source.prop("fl")?)?;
        let last = integer(source.prop("tl")?)?;
        if !matches!(source.prop("flp")?, Value::Bool(_)) {
            return Err(error("Invalid padstack flip flag"));
        }
        let forward = call(source.require_child("lm")?, "forward")?
            .args
            .iter()
            .map(|a| integer(&a.1))
            .collect::<Result<Vec<_>, _>>()?;
        if forward
            .iter()
            .any(|id| *id != -1 && !def.layers.contains_key(id))
        {
            return Err(error("Missing mapped padstack layer"));
        }
        let usage = source.prop("pum")?.text()?;
        let mut used = HashSet::new();
        let mut seen = HashSet::new();
        if !usage.is_empty() {
            let entries = usage
                .strip_suffix(':')
                .ok_or_else(|| error("Invalid pad usage"))?
                .split(':')
                .collect::<Vec<_>>();
            if entries.len() % 2 != 0 {
                return Err(error("Invalid pad usage"));
            }
            for pair in entries.as_chunks::<2>().0 {
                if pair[0].is_empty() || !pair[0].bytes().all(|b| b.is_ascii_digit()) {
                    return Err(error("Invalid pad usage layer"));
                }
                let layer = pair[0]
                    .parse::<i32>()
                    .map_err(|_| error("Invalid pad usage layer"))?;
                let flags = match pair[1] {
                    "0" => 0,
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    _ => return Err(error("Unknown pad usage flags")),
                };
                if !seen.insert(layer) || !source_ids.contains(&layer) {
                    return Err(error("Duplicate or missing pad usage layer"));
                }
                if flags & 1 != 0 {
                    used.insert(layer);
                }
            }
        }
        let die = last == 0
            && source.properties.get("sbl").and_then(|v| v.number().ok()) == Some(-100.0)
            && def.layers.len() == 1
            && def.layers.contains_key(&first)
            && forward.is_empty()
            && used.is_empty();
        if !source_ids.contains(&first) || (!die && !source_ids.contains(&last)) {
            return Err(error("Missing padstack span"));
        }
        let mut templates = Vec::new();
        if die {
            if let Some(pad) = geometry::pad(&def.layers[&first], LayerId::BOND_TOP, context)? {
                templates.push(pad);
            }
        } else {
            for layer in layers {
                if used.contains(&layer.id)
                    && let Some(&display) = layer_ids.get(&layer.id)
                {
                    let mapped = usize::try_from(layer.id)
                        .ok()
                        .and_then(|i| forward.get(i))
                        .copied()
                        .unwrap_or(-1);
                    if mapped != -1
                        && let Some(pad) = geometry::pad(&def.layers[&mapped], display, context)?
                    {
                        templates.push(pad);
                    }
                }
            }
        }
        if bindings
            .insert(
                id,
                Binding {
                    definition,
                    first,
                    last,
                    die,
                    templates,
                },
            )
            .is_some()
        {
            return Err(error("Duplicate padstack binding"));
        }
    }
    let mut owners = HashMap::<i32, usize>::new();
    for value in layout.field(5)?.array()? {
        context.check_cancelled()?;
        let p = value.object(Some(19))?;
        if p.field(9)?.integer()? != 0
            || !p.field(8)?.text()?.is_empty()
            || !p.field(11)?.array()?.is_empty()
            || !p.field(12)?.text()?.is_empty()
        {
            return Err(error("Unsupported drill override or padstack extensions"));
        }
        let pin = p.field(10)?.integer()?;
        if pin != 0 && pin != 1 {
            return Err(error("Invalid pin flag"));
        }
        let base = p.field(0)?.object(Some(10))?;
        let component = base.field(2)?.integer()?;
        let reference = if component == -1 {
            String::new()
        } else {
            components
                .get(&component)
                .cloned()
                .ok_or_else(|| error("Pad references missing component"))?
        };
        let binding_id = p.field(1)?.integer()?;
        let binding = bindings
            .get(&binding_id)
            .ok_or_else(|| error("Missing padstack binding"))?;
        let def = &definitions[&binding.definition];
        let rotation = p.field(4)?.numeric()?;
        let hole = &def.hole;
        let offset = rotate(hole.offset, rotation);
        let at = Point::new(
            p.field(2)?.numeric()? * 1000.0 + offset.x,
            p.field(3)?.numeric()? * 1000.0 + offset.y,
        );
        let angle = rotation + hole.angle;
        let mut pads = binding.templates.clone();
        for pad in &mut pads {
            let origin = rotate(pad.offset, rotation);
            pad.offset = Point::new(origin.x - offset.x, origin.y - offset.y);
            if hole.angle != 0.0 && pad.kind != PadKind::CIRCLE {
                let paths = shared::pad_paths(pad)
                    .into_iter()
                    .map(|path| {
                        path.iter()
                            .map(|s| shared::transform(s, Point::default(), -hole.angle, false))
                            .collect()
                    })
                    .collect();
                let mut custom = shared::custom(paths, context)?;
                custom.layer = pad.layer;
                custom.offset = pad.offset;
                *pad = custom;
            }
        }
        let id = ObjectId(*next);
        *next += 1;
        let net = super::net(base.field(1)?.integer()?, net_ids)?;
        let drill_shape = DrillShape {
            width: hole.width,
            height: hole.height,
            plated: def.plated,
        };
        if pin == 1 || binding.die {
            if binding.die
                && !output
                    .special_layers
                    .iter()
                    .any(|l| l.id == LayerId::BOND_TOP)
            {
                output.special_layers.push(SourceSpecialLayer {
                    id: LayerId::BOND_TOP,
                    name: "BOND TOP".into(),
                    color: "#d7cd58".into(),
                    kind: SpecialLayerKind::DiePad,
                });
            }
            let owner_id = if component == -1 {
                ObjectId(0)
            } else {
                let index = if let Some(&index) = owners.get(&component) {
                    index
                } else {
                    let index = output.scene.components.len();
                    let owner = ObjectId(*next);
                    *next += 1;
                    output.scene.components.push(ComponentPlacement {
                        id: owner,
                        source_reference: u32::try_from(component).ok().map(ObjectId),
                        reference: reference.clone(),
                        at,
                        angle,
                        mirrored: false,
                        pins: Vec::new(),
                    });
                    owners.insert(component, index);
                    index
                };
                output.scene.components[index].pins.push(id);
                output.scene.components[index].id
            };
            output.scene.pins.push(Pin {
                id,
                owner_id,
                net,
                name: p.field(6)?.text()?.into(),
                reference,
                at,
                angle,
                mirrored: false,
                drill: hole.height,
                drill_shape,
                pads,
                stackup_region: None,
                die: binding.die.then(|| DiePad {
                    source_reference: ObjectId(binding_id as u32),
                    padstack_name: def.name.clone(),
                }),
            });
        } else {
            let first = *layer_ids
                .get(&binding.first)
                .ok_or_else(|| error("Drill start is not copper"))?;
            let last = *layer_ids
                .get(&binding.last)
                .ok_or_else(|| error("Drill end is not copper"))?;
            output.scene.vias.push(Via {
                id,
                net,
                at,
                drill: hole.height,
                drill_shape,
                padstack: ObjectId(def.id as u32),
                padstack_name: def.name.clone(),
                start_layer: Some(first.min(last)),
                end_layer: Some(first.max(last)),
                pads: Arc::from(pads),
                backdrill: None,
                stackup_region: None,
                angle,
                mirrored: false,
                finger: None,
            });
        }
    }
    Ok(())
}
