//! Native HFSS 3D Layout / self-describing EDB DEF import.
mod binary;
mod geometry;
mod pads;
mod paths;
mod text;
use super::{
    ParsedBoard, SourceDrawingLayer, SourceSpecialLayer, SourceZone, geometry as shared,
    scene_builder,
};
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
struct Component {
    reference: String,
    at: Point,
    angle: f64,
    mirrored: bool,
}
impl Component {
    fn read(group: &Object) -> Result<Self, ImportError> {
        let transform = text::statement(group.field(2)?.text()?)?.1;
        let t = transform.call()?;
        if t.name != "f" || geometry::quantity(t.arg("s")?, false)? != 1.0 {
            return Err(error("Unsupported component scale transform"));
        }
        let at = Point::new(
            geometry::quantity(t.arg("x")?, false)? * 1000.0,
            geometry::quantity(t.arg("y")?, false)? * 1000.0,
        );
        let angle = geometry::quantity(t.arg("r")?, true)?;
        if !at.is_finite() || !angle.is_finite() {
            return Err(error("Invalid component transform"));
        }
        Ok(Self {
            reference: group.field(1)?.text()?.to_owned(),
            at,
            angle,
            mirrored: match t.arg("m")? {
                text::Value::Bool(value) => *value,
                _ => return Err(error("Invalid component mirror flag")),
            },
        })
    }
    fn place(&self, local: Point) -> Point {
        let p = self.vector(local);
        Point::new(self.at.x + p.x, self.at.y + p.y)
    }
    fn vector(&self, local: Point) -> Point {
        Point::new(local.x, if self.mirrored { -local.y } else { local.y }).rotate(self.angle)
    }
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
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<ParsedBoard, ImportError> {
    let archive = binary::read(bytes, context)?;
    let root = archive.root;
    let groups = root.field(2)?.object(Some(1))?.field(0)?.array()?;
    let mut cells = Vec::new();
    for group in groups {
        for c in group.object(Some(2))?.field(1)?.array()? {
            cells.push(c.object(Some(3))?);
        }
    }
    let cell = *cells.first().ok_or_else(|| error("Missing board cell"))?;
    let metadata = text::read(cell.field(0)?.text()?)?;
    if metadata.prop("dn")?.text()?.is_empty() {
        return Err(error("Unnamed board cell"));
    }
    let layout = cell.field(4)?.object(Some(4))?;
    let source_layers = text::read(cell.field(2)?.text()?)?;
    let mut layers = Vec::new();
    let mut layer_ids = HashMap::new();
    let mut source_ids = HashSet::new();
    let mut output = scene_builder::new_parsed_board(archive.version);
    if cells.len() > 1 {
        output.diagnostics.push(format!(
            "HFSS archive contains {} cells; displaying the first cell '{}'; other cells are not loaded",
            cells.len(), metadata.prop("dn")?.text()?
        ));
    }
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
        if layout.field(4)?.array()?.is_empty() && layout.field(5)?.array()?.is_empty() {
            return Err(ImportError::NoGeometry);
        }
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
        let c = value.object(None)?;
        let group = if c.schema == 21 {
            c
        } else {
            value.object(Some(22))?.field(0)?.object(Some(21))?
        };
        let base = group.field(0)?.object(Some(10))?;
        if components
            .insert(
                base.field(0)?.object(Some(5))?.field(0)?.integer()?,
                Component::read(group)?,
            )
            .is_some()
        {
            return Err(error("Duplicate component"));
        }
    }
    let mut primitives = Vec::new();
    let mut primitive_ids = HashMap::new();
    let mut voids = HashMap::<i32, Vec<usize>>::new();
    let mut unlayered = HashSet::new();
    for value in layout.field(4)?.array()? {
        context.check_cancelled()?;
        let p = value.object(None)?;
        if p.schema == 39 {
            if !output
                .diagnostics
                .iter()
                .any(|d| d.contains("point terminals"))
            {
                output
                    .diagnostics
                    .push("HFSS simulation point terminals are not displayed".into());
            }
            continue;
        }
        if !matches!(p.schema, 12..=17 | 51) {
            return Err(error("Unsupported primitive schema"));
        }
        let i = info(p)?;
        if !source_ids.contains(&i.layer) {
            if i.layer >= 0 {
                return Err(error(format!(
                    "Primitive {} references missing source layer {}",
                    i.id, i.layer
                )));
            }
            // Negative IDs (e.g. -100) are EDB's "no layer" sentinel for orphaned primitives.
            unlayered.insert(i.id);
            if !output
                .diagnostics
                .iter()
                .any(|d| d.contains("without a layer"))
            {
                output
                    .diagnostics
                    .push("HFSS primitives without a layer are not displayed".into());
            }
            continue;
        }
        net(i.net, &net_ids)?;
        let index = primitives.len();
        if primitive_ids.insert(i.id, index).is_some() {
            return Err(error("Duplicate primitive"));
        }
        primitives.push((p, i));
    }
    for (index, (_, child)) in primitives
        .iter()
        .enumerate()
        .filter(|(_, (_, i))| i.parent != -1 && !unlayered.contains(&i.parent))
    {
        let parent_index = *primitive_ids
            .get(&child.parent)
            .ok_or_else(|| error("Missing void parent"))?;
        let (shape, parent) = &primitives[parent_index];
        let parent_id = if parent.layer == child.layer {
            child.parent
        } else {
            // Copied layer polygons can retain the original layer's parent ID.
            // Rebind only when the declared parent has one identical peer on this layer.
            let mut peers = primitives.iter().filter(|(p, i)| {
                i.parent == -1
                    && i.layer == child.layer
                    && i.component == child.component
                    && p.schema == shape.schema
                    && p.fields[1..] == shape.fields[1..]
            });
            let (_, peer) = peers.next().ok_or_else(|| error("Mismatched void layer"))?;
            if peers.next().is_some() {
                return Err(error("Ambiguous copied void parent"));
            }
            if !output
                .diagnostics
                .iter()
                .any(|d| d.contains("copied void parent"))
            {
                output.diagnostics.push("HFSS copied void parent references were matched to identical polygons on their own layers".into());
            }
            peer.id
        };
        voids.entry(parent_id).or_default().push(index);
    }
    for (&parent, children) in &voids {
        let parent = *primitive_ids
            .get(&parent)
            .ok_or_else(|| error("Missing void parent"))?;
        let i = &primitives[parent].1;
        if i.parent != -1
            || children
                .iter()
                .any(|&c| primitives[c].1.component != i.component)
        {
            return Err(error("Nested void or mismatched void component"));
        }
    }
    let mut next = 1u32;
    for (p, i) in &primitives {
        context.check_cancelled()?;
        if i.parent != -1 {
            continue;
        }
        let component = if i.component == -1 {
            None
        } else {
            Some(
                components
                    .get(&i.component)
                    .ok_or_else(|| error("Primitive references missing component"))?,
            )
        };
        let place = |mut path: Vec<Segment>| {
            if let Some(component) = component {
                for edge in &mut path {
                    *edge =
                        shared::transform(edge, component.at, component.angle, component.mirrored);
                }
            }
            path
        };
        let source = layers
            .iter()
            .find(|l| l.id == i.layer)
            .ok_or_else(|| error("Missing source layer"))?;
        let net = net(i.net, &net_ids)?;
        if p.schema == 17 {
            if !output
                .diagnostics
                .iter()
                .any(|d| d.contains("text annotations"))
            {
                output
                    .diagnostics
                    .push("HFSS text annotations are not displayed".into());
            }
            continue;
        }
        if p.schema == 16 {
            // Bond wires may be anchored on wirebond or signal layers; only voids are invalid.
            if voids.contains_key(&i.id) {
                return Err(error(format!("Bond wire {} has voids", i.id)));
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
            for mut edge in place(geometry::polygon_object(path.field(6)?.object(Some(36))?)?) {
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
                    reference: component.map(|c| c.reference.clone()).unwrap_or_default(),
                    pin_name: String::new(),
                });
                output.scene.segments.push(edge);
            }
            continue;
        }
        if !layer_ids.contains_key(&i.layer) {
            let mut paths = Vec::new();
            if p.schema == 14 {
                let width = p.field(4)?.quantity()? * 1000.0;
                if width < 0.0 {
                    return Err(error("Invalid outline width"));
                }
                let path = place(geometry::polygon_object(p.field(6)?.object(Some(36))?)?)
                    .into_iter()
                    .map(|mut s| {
                        s.width = width;
                        s
                    })
                    .collect();
                paths.push(path);
            } else {
                for path in self::paths::outlines(p, context)? {
                    paths.push(place(path));
                }
                for &hole in voids.get(&i.id).map(Vec::as_slice).unwrap_or_default() {
                    paths.push(place(geometry::primitive(primitives[hole].0)?));
                }
            }
            if source.kind == "outline" {
                for mut s in paths.into_iter().flatten() {
                    s.id = ObjectId(next);
                    next += 1;
                    s.track_id = s.id;
                    s.layer = LayerId::UNASSIGNED;
                    s.net = NetId(0);
                    output.scene.outline.push(s);
                }
            } else {
                let source_index = layers
                    .iter()
                    .position(|l| l.id == source.id)
                    .ok_or_else(|| error("Missing graphic layer"))?;
                let layer = LayerId::special(2 + source_index as u32);
                if !output.drawing_layers.iter().any(|l| l.id == layer) {
                    output.drawing_layers.push(SourceDrawingLayer {
                        id: layer,
                        name: source.name.clone(),
                        color: "#d8d8b0".into(),
                        default_visible: source.kind == "silkscreen",
                    });
                }
                let id = ObjectId(next);
                next += 1;
                let segments = paths
                    .into_iter()
                    .flatten()
                    .map(|mut edge| {
                        edge.id = ObjectId(next);
                        next += 1;
                        edge.track_id = id;
                        edge.layer = layer;
                        edge.net = net;
                        edge
                    })
                    .collect();
                output.scene.drawings.push(BoardDrawing {
                    id,
                    owner_id: None,
                    layer,
                    net,
                    graphic_ids: vec![id],
                    segments,
                    text_ids: vec![],
                });
                if p.schema != 14
                    && !output
                        .diagnostics
                        .iter()
                        .any(|d| d.contains("non-copper filled graphics"))
                {
                    output
                        .diagnostics
                        .push("HFSS non-copper filled graphics are rendered as boundaries".into());
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
        if p.schema == 14
            && (1..=3).all(|field| {
                p.field(field)
                    .and_then(|v| v.integer())
                    .is_ok_and(|v| v == 0)
            })
            && !voids.contains_key(&i.id)
        {
            let width = p.field(4)?.quantity()? * 1000.0;
            if width < 0.0 {
                return Err(error("Negative trace width"));
            }
            let track = ObjectId(next);
            next += 1;
            for mut s in place(geometry::polygon_object(p.field(6)?.object(Some(36))?)?) {
                s.id = ObjectId(next);
                next += 1;
                s.track_id = track;
                s.layer = layer;
                s.net = net;
                s.width = width;
                output.scene.segments.push(s);
            }
        } else {
            let outlines = self::paths::outlines(p, context)?;
            let regions = if p.schema == 51 {
                outlines.into_iter().map(|p| vec![p]).collect()
            } else {
                vec![outlines]
            };
            for region in regions {
                let mut paths: Vec<_> = region.into_iter().map(&place).collect();
                for &hole in voids.get(&i.id).map(Vec::as_slice).unwrap_or_default() {
                    paths.extend(
                        self::paths::outlines(primitives[hole].0, context)?
                            .into_iter()
                            .map(&place),
                    );
                }
                if paths.is_empty() || paths[0].is_empty() {
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
    scene_builder::update_scene_bounds(&mut output);
    Ok(output)
}

#[cfg(test)]
mod tests;
