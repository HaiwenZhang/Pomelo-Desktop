//! Native HFSS 3D Layout / EDB 12.1 DEF import.
mod binary;
mod geometry;
mod pads;
mod text;
use super::{Output, SourceSpecialLayer, SourceZone, geometry as shared, native};
use crate::{ImportContext, ImportError};
use binary::Object;
use pomelo_core::model::*;
use std::collections::{HashMap, HashSet};
fn error(details: impl Into<String>) -> ImportError {
    ImportError::Format {
        format: "HFSS 3D Layout".into(),
        details: details.into(),
    }
}
struct Layer {
    id: i32,
    name: String,
    kind: String,
}
struct Info {
    id: i32,
    net: i32,
    component: i32,
    layer: i32,
    parent: i32,
}
fn info(p: &Object) -> Result<Info, ImportError> {
    let p = if p.schema == 16 {
        p.field(0)?.object(Some(14))?
    } else {
        p
    };
    let i = p.field(0)?.object(Some(11))?;
    let base = i.field(0)?.object(Some(10))?;
    Ok(Info {
        id: base.field(0)?.object(Some(5))?.field(0)?.integer()?,
        net: base.field(1)?.integer()?,
        component: base.field(2)?.integer()?,
        layer: i.field(1)?.integer()?,
        parent: i.field(4)?.integer()?,
    })
}
fn net(raw: i32, ids: &HashMap<i32, NetId>) -> Result<NetId, ImportError> {
    if raw == -1 {
        Ok(NetId(0))
    } else {
        ids.get(&raw)
            .copied()
            .ok_or_else(|| error("Missing source network"))
    }
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<Output, ImportError> {
    let root = binary::read(bytes, context)?;
    let groups = root.field(2)?.object(Some(1))?.field(0)?.array()?;
    let mut cells = Vec::new();
    for group in groups {
        for c in group.object(Some(2))?.field(1)?.array()? {
            cells.push(c.object(Some(3))?);
        }
    }
    if cells.len() != 1 {
        return Err(error("Only one board cell is supported"));
    }
    let cell = cells[0];
    let metadata = text::read(cell.field(0)?.text()?)?;
    if metadata.prop("dn")?.text()?.is_empty() {
        return Err(error("Unnamed board cell"));
    }
    let layout = cell.field(4)?.object(Some(4))?;
    let source_layers = text::read(cell.field(2)?.text()?)?;
    let mut layers = Vec::new();
    let mut layer_ids = HashMap::new();
    let mut source_ids = HashSet::new();
    let mut output = native::output("12.1");
    let colors = [
        "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd", "#a5b8df", "#e18d61",
    ];
    for source in &source_layers.calls {
        let layer = if source.name == "SLayer" {
            source
                .args
                .first()
                .ok_or_else(|| error("Missing SLayer argument"))?
                .1
                .call()?
        } else {
            source
        };
        if layer.name != "Layer" {
            continue;
        }
        let id = match layer.arg("ID")? {
            text::Value::Text(v) => v.parse::<f64>().map_err(|_| error("Invalid layer ID"))?,
            v => v.number()?,
        };
        if id.fract() != 0.0 || id < i32::MIN as f64 || id > i32::MAX as f64 {
            return Err(error("Invalid source layer ID"));
        }
        let id = id as i32;
        if !source_ids.insert(id) {
            return Err(error("Duplicate source layer"));
        }
        let name = layer.arg("N")?.text()?.to_owned();
        let kind = layer.arg("T")?.text()?.to_owned();
        if kind == "signal" {
            let display = LayerId(output.scene.layers.len() as u32);
            layer_ids.insert(id, display);
            output.scene.layers.push(pomelo_core::model::Layer {
                id: display,
                name: name.clone(),
                function: LayerFunction::Conductor,
                color: colors[display.0 as usize % colors.len()].into(),
                source_flags: None,
            });
        }
        layers.push(Layer { id, name, kind });
    }
    if layer_ids.is_empty() {
        return Err(error("No copper layers"));
    }
    let mut net_ids = HashMap::new();
    for value in layout.field(1)?.array()? {
        let n = value.object(Some(7))?;
        let source = n.field(0)?.object(Some(5))?.field(0)?.integer()?;
        let id = NetId(net_ids.len() as u32 + 1);
        if net_ids.insert(source, id).is_some() {
            return Err(error("Duplicate network"));
        }
        output.scene.nets.insert(id, n.field(1)?.text()?.into());
    }
    let mut components = HashMap::new();
    for value in layout.field(6)?.array()? {
        context.check_cancelled()?;
        let c = value.object(Some(22))?;
        let group = c.field(0)?.object(Some(21))?;
        let base = group.field(0)?.object(Some(10))?;
        let transform = text::statement(group.field(2)?.text()?)?.1;
        let t = transform.call()?;
        if t.name != "f"
            || geometry::quantity(t.arg("x")?, false)? != 0.0
            || geometry::quantity(t.arg("y")?, false)? != 0.0
            || geometry::quantity(t.arg("r")?, true)? != 0.0
            || geometry::quantity(t.arg("s")?, false)? != 1.0
            || !matches!(t.arg("m")?, text::Value::Bool(false))
        {
            return Err(error("Unsupported component transform"));
        }
        components.insert(
            base.field(0)?.object(Some(5))?.field(0)?.integer()?,
            group.field(1)?.text()?.to_owned(),
        );
    }
    let mut primitives = Vec::new();
    let mut primitive_ids = HashMap::new();
    let mut voids = HashMap::<i32, Vec<usize>>::new();
    for value in layout.field(4)?.array()? {
        context.check_cancelled()?;
        let p = value.object(None)?;
        if !(12..=16).contains(&p.schema) {
            return Err(error("Unsupported primitive schema"));
        }
        let i = info(p)?;
        if !source_ids.contains(&i.layer) {
            return Err(error("Primitive references missing source layer"));
        }
        net(i.net, &net_ids)?;
        let index = primitives.len();
        if primitive_ids.insert(i.id, index).is_some() {
            return Err(error("Duplicate primitive"));
        }
        if i.parent != -1 {
            voids.entry(i.parent).or_default().push(index);
        }
        primitives.push((p, i));
    }
    for (&parent, children) in &voids {
        let parent = *primitive_ids
            .get(&parent)
            .ok_or_else(|| error("Missing void parent"))?;
        let i = &primitives[parent].1;
        if i.parent != -1 || children.iter().any(|&c| primitives[c].1.layer != i.layer) {
            return Err(error("Nested void or mismatched void layer"));
        }
    }
    let mut next = 1u32;
    for (p, i) in &primitives {
        context.check_cancelled()?;
        if i.parent != -1 {
            continue;
        }
        let source = layers
            .iter()
            .find(|l| l.id == i.layer)
            .ok_or_else(|| error("Missing source layer"))?;
        let net = net(i.net, &net_ids)?;
        if p.schema == 16 {
            if source.kind != "wirebond" || voids.contains_key(&i.id) {
                return Err(error("Invalid bond wire layer/geometry"));
            }
            let path = p.field(0)?.object(Some(14))?;
            let width = path.field(4)?.quantity()? * 1000.0;
            if width <= 0.0 {
                return Err(error("Invalid bond wire width"));
            }
            if !output
                .special_layers
                .iter()
                .any(|l| l.id == LayerId::BOND_WIRE_TOP)
            {
                output.special_layers.push(SourceSpecialLayer {
                    id: LayerId::BOND_WIRE_TOP,
                    name: source.name.clone(),
                    color: "#e4d95b".into(),
                    kind: SpecialLayerKind::BondWire,
                });
            }
            let track = ObjectId(next);
            next += 1;
            for mut edge in geometry::polygon_object(path.field(6)?.object(Some(36))?)? {
                edge.id = ObjectId(next);
                next += 1;
                edge.track_id = track;
                edge.layer = LayerId::BOND_WIRE_TOP;
                edge.net = net;
                edge.width = width;
                edge.bond_wire = Some(BondWireInfo {
                    profile: p.field(1)?.text()?.into(),
                    material: Some(p.field(3)?.text()?.into()),
                    source_pin: ObjectId(u32::MAX),
                    finger: ObjectId(u32::MAX),
                    reference: components.get(&i.component).cloned().unwrap_or_default(),
                    pin_name: String::new(),
                });
                output.scene.segments.push(edge);
            }
            continue;
        }
        if source.kind == "outline" {
            let mut paths = Vec::new();
            if p.schema == 14 {
                let width = p.field(4)?.quantity()? * 1000.0;
                if width < 0.0 {
                    return Err(error("Invalid outline width"));
                }
                let path = geometry::polygon_object(p.field(6)?.object(Some(36))?)?
                    .into_iter()
                    .map(|mut s| {
                        s.width = width;
                        s
                    })
                    .collect();
                paths.push(path);
            } else {
                paths.push(geometry::primitive(p)?);
                for &hole in voids.get(&i.id).map(Vec::as_slice).unwrap_or_default() {
                    paths.push(geometry::primitive(primitives[hole].0)?);
                }
            }
            for path in paths {
                for mut s in path {
                    s.id = ObjectId(next);
                    next += 1;
                    s.track_id = s.id;
                    s.layer = LayerId::UNASSIGNED;
                    s.net = NetId(0);
                    output.scene.outline.push(s);
                }
            }
            continue;
        }
        let layer = *layer_ids.get(&i.layer).ok_or_else(|| {
            error(format!(
                "Unsupported noncopper primitive layer {}",
                source.name
            ))
        })?;
        if p.schema == 14 {
            if !(1..=3).all(|field| {
                p.field(field)
                    .and_then(|v| v.integer())
                    .is_ok_and(|v| v == 0)
            }) || voids.contains_key(&i.id)
            {
                return Err(error("Nonround/voided trace not supported"));
            }
            let width = p.field(4)?.quantity()? * 1000.0;
            if width < 0.0 {
                return Err(error("Negative trace width"));
            }
            let track = ObjectId(next);
            next += 1;
            for mut s in geometry::polygon_object(p.field(6)?.object(Some(36))?)? {
                s.id = ObjectId(next);
                next += 1;
                s.track_id = track;
                s.layer = layer;
                s.net = net;
                s.width = width;
                output.scene.segments.push(s);
            }
        } else {
            let mut paths = vec![geometry::primitive(p)?];
            for &hole in voids.get(&i.id).map(Vec::as_slice).unwrap_or_default() {
                paths.push(geometry::primitive(primitives[hole].0)?);
            }
            if paths[0].is_empty() {
                return Err(error("Empty copper area"));
            }
            let id = ObjectId(next);
            next += 1;
            for path in &mut paths {
                for s in path {
                    s.id = id;
                    s.track_id = id;
                    s.layer = layer;
                    s.net = net;
                }
            }
            let rings = shared::rings(&paths, context)?;
            output.zones.push(SourceZone {
                id,
                layer,
                net,
                paths,
                rings,
            });
        }
    }
    pads::build(
        &root,
        cell,
        layout,
        &layers,
        &layer_ids,
        &net_ids,
        &components,
        &mut output,
        &mut next,
        context,
    )?;
    native::bounds(&mut output);
    Ok(output)
}
