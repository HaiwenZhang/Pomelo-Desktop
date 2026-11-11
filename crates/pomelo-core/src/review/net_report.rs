//! Per-net facts for signal and power review: routed length per layer, how each
//! via is used and where it leaves a stub, the nearest plane layers and the pins.
//!
//! Lengths are 2D centerline lengths without via barrels. Via use is inferred from
//! copper of the same net touching the via on a layer, which is stated in the UI.
use crate::{
    model::{BoardScene, LayerFunction, LayerId, NetId, ObjectId, Point, Segment},
    task::{CancellationToken, Cancelled},
};
use std::collections::{BTreeMap, BTreeSet};

/// Extra reach when matching a trace end to a via, beyond the via's pad radius.
const TOUCH_MM: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct ViaUse {
    pub id: ObjectId,
    pub at: Point,
    pub padstack: String,
    /// Plated span in stack order, when the source gives one.
    pub span: Option<(LayerId, LayerId)>,
    /// Layers where this net's copper reaches the via, in stack order.
    pub used: Vec<LayerId>,
    /// Unused barrel ranges as (last used layer, span end), top side first.
    pub stubs: Vec<(LayerId, LayerId)>,
    /// Layers of the stub ranges, counted from the used layer to the span end.
    pub stub_layers: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetReport {
    pub net: NetId,
    /// Routed length per layer, in stack order.
    pub layer_lengths: Vec<(LayerId, f64)>,
    pub total_length: f64,
    pub segments: usize,
    pub vias: Vec<ViaUse>,
    /// (component reference, pin name) of every pin on the net.
    pub pins: Vec<(String, String)>,
    /// Routed layer with the nearest plane layer above and below it.
    pub adjacent_planes: Vec<(LayerId, Option<LayerId>, Option<LayerId>)>,
}

impl NetReport {
    pub fn stubbed_vias(&self) -> usize {
        self.vias.iter().filter(|via| !via.stubs.is_empty()).count()
    }
}

pub fn segment_length(segment: &Segment) -> f64 {
    match segment.arc {
        Some(arc) => (arc.sweep * arc.radius).abs(),
        None => segment.a.distance(segment.b),
    }
}

/// Conductor layers in physical stack order.
fn stack(scene: &BoardScene) -> Vec<(LayerId, LayerFunction)> {
    scene
        .layers
        .iter()
        .filter(|layer| layer.id.is_copper() && layer.function != LayerFunction::Dielectric)
        .map(|layer| (layer.id, layer.function))
        .collect()
}

pub fn report(
    scene: &BoardScene,
    net: NetId,
    cancel: &CancellationToken,
) -> Result<NetReport, Cancelled> {
    let stack = stack(scene);
    let position: BTreeMap<LayerId, usize> = stack
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let mut lengths: BTreeMap<LayerId, f64> = BTreeMap::new();
    let mut ends: BTreeMap<LayerId, Vec<Point>> = BTreeMap::new();
    let mut segments = 0;
    for (i, segment) in scene.segments.iter().enumerate() {
        if i % 4096 == 0 {
            cancel.check()?;
        }
        if segment.net != net {
            continue;
        }
        segments += 1;
        *lengths.entry(segment.layer).or_default() += segment_length(segment);
        ends.entry(segment.layer)
            .or_default()
            .extend([segment.a, segment.b]);
    }
    let zones: Vec<_> = scene.zones.iter().filter(|zone| zone.net == net).collect();
    let pins: Vec<_> = scene.pins.iter().filter(|pin| pin.net == net).collect();
    let mut vias = Vec::new();
    for via in scene.vias.iter().filter(|via| via.net == net) {
        cancel.check()?;
        let span = via.start_layer.zip(via.end_layer).and_then(|(a, b)| {
            let (pa, pb) = (*position.get(&a)?, *position.get(&b)?);
            Some(if pa <= pb { (pa, pb) } else { (pb, pa) })
        });
        let reach = |layer: LayerId| {
            let pad = via
                .pads
                .iter()
                .filter(|pad| pad.layer == layer)
                .map(|pad| pad.width.max(pad.height) * 0.5)
                .fold(0.0_f64, f64::max);
            pad.max(via.drill * 0.5) + TOUCH_MM
        };
        let mut used = BTreeSet::new();
        for (layer, points) in &ends {
            let r = reach(*layer);
            if points.iter().any(|p| p.distance(via.at) <= r) {
                used.insert(*layer);
            }
        }
        for zone in &zones {
            if zone.mesh.covers_fill(via.at, cancel).unwrap_or(false) {
                used.insert(zone.layer);
            }
        }
        for pin in &pins {
            for pad in &pin.pads {
                if pin.at.distance(via.at) <= pad.width.max(pad.height) * 0.5 + TOUCH_MM {
                    used.insert(pad.layer);
                }
            }
        }
        let mut used: Vec<_> = used
            .into_iter()
            .filter(|layer| position.contains_key(layer))
            .collect();
        used.sort_by_key(|layer| position[layer]);
        let mut stubs = Vec::new();
        let mut stub_layers = 0;
        if let (Some((top, bottom)), Some(first), Some(last)) = (span, used.first(), used.last()) {
            let (first, last) = (position[first], position[last]);
            if first > top {
                stubs.push((stack[top].0, stack[first].0));
                stub_layers += first - top;
            }
            if last < bottom {
                stubs.push((stack[last].0, stack[bottom].0));
                stub_layers += bottom - last;
            }
        }
        vias.push(ViaUse {
            id: via.id,
            at: via.at,
            padstack: via.padstack_name.clone(),
            span: span.map(|(a, b)| (stack[a].0, stack[b].0)),
            used,
            stubs,
            stub_layers,
        });
    }
    let mut layer_lengths: Vec<_> = lengths.into_iter().collect();
    layer_lengths.sort_by_key(|(layer, _)| position.get(layer).copied().unwrap_or(usize::MAX));
    let total_length = layer_lengths.iter().map(|(_, length)| length).sum();
    let adjacent_planes = layer_lengths
        .iter()
        .filter_map(|(layer, _)| {
            let at = *position.get(layer)?;
            let above = stack[..at]
                .iter()
                .rev()
                .find(|(_, f)| *f == LayerFunction::Plane)
                .map(|(id, _)| *id);
            let below = stack[at + 1..]
                .iter()
                .find(|(_, f)| *f == LayerFunction::Plane)
                .map(|(id, _)| *id);
            Some((*layer, above, below))
        })
        .collect();
    let mut pin_names: Vec<_> = pins
        .iter()
        .map(|pin| (pin.reference.clone(), pin.name.clone()))
        .collect();
    pin_names.sort();
    Ok(NetReport {
        net,
        layer_lengths,
        total_length,
        segments,
        vias,
        pins: pin_names,
        adjacent_planes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DrillShape, Layer, Pad, PadKind, Via};

    fn layer(id: u32, function: LayerFunction) -> Layer {
        Layer {
            id: LayerId(id),
            name: format!("L{id}"),
            function,
            color: "#ffffff".into(),
            source_flags: None,
        }
    }

    fn segment(id: u32, layer: u32, a: (f64, f64), b: (f64, f64)) -> Segment {
        Segment {
            id: ObjectId(id),
            track_id: ObjectId(0),
            layer: LayerId(layer),
            net: NetId(7),
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
            width: 0.1,
            arc: None,
            bond_wire: None,
        }
    }

    fn pad(layer: u32) -> Pad {
        Pad {
            layer: LayerId(layer),
            width: 0.4,
            height: 0.4,
            offset: Point::default(),
            kind: PadKind(2),
            corner: 0.0,
            inner_diameter: None,
            custom: None,
            backdrill: false,
            backdrill_base: false,
        }
    }

    fn pin(id: u32, at: (f64, f64), layer: u32) -> crate::model::Pin {
        crate::model::Pin {
            id: ObjectId(id),
            owner_id: ObjectId(100),
            net: NetId(7),
            name: id.to_string(),
            reference: "U1".into(),
            at: Point::new(at.0, at.1),
            angle: 0.0,
            mirrored: false,
            drill: 0.0,
            drill_shape: DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
                rectangular: false,
                custom: None,
            },
            pads: vec![pad(layer)],
            stackup_region: None,
            die: None,
        }
    }

    #[test]
    fn pin_path_follows_traces_through_vias_and_reports_loops() {
        let mut scene = super::super::tests::empty_scene();
        scene.layers = vec![
            layer(0, LayerFunction::Conductor),
            layer(1, LayerFunction::Conductor),
        ];
        scene.pins = vec![pin(1, (0.0, 0.0), 0), pin(2, (10.0, 0.0), 1)];
        scene.segments = vec![
            segment(1, 0, (0.0, 0.0), (4.0, 0.0)),
            segment(2, 1, (4.0, 0.0), (10.0, 0.0)),
        ];
        let via = |id: u32, at: (f64, f64)| Via {
            id: ObjectId(id),
            net: NetId(7),
            at: Point::new(at.0, at.1),
            drill: 0.2,
            drill_shape: DrillShape {
                width: 0.2,
                height: 0.2,
                plated: true,
                rectangular: false,
                custom: None,
            },
            padstack: ObjectId(1),
            padstack_name: "VIA".into(),
            start_layer: Some(LayerId(0)),
            end_layer: Some(LayerId(1)),
            pads: vec![pad(0), pad(1)].into(),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        };
        scene.vias = vec![via(9, (4.0, 0.0))];
        let cancel = CancellationToken::default();
        let path = pin_path(&scene, ObjectId(1), ObjectId(2), &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(path.length, 10.0);
        assert_eq!(path.vias, 1);
        assert!(!path.alternatives);
        // A detour on the bottom layer makes a loop with a longer second path.
        scene.segments.push(segment(3, 0, (4.0, 0.0), (4.0, 3.0)));
        scene.segments.push(segment(4, 0, (4.0, 3.0), (0.0, 0.0)));
        let path = pin_path(&scene, ObjectId(1), ObjectId(2), &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(path.length, 10.0);
        assert!(path.alternatives);
        scene.vias.clear();
        assert_eq!(
            pin_path(&scene, ObjectId(1), ObjectId(2), &cancel).unwrap(),
            Err(PathError::NotConnected)
        );
    }

    #[test]
    fn lengths_planes_and_stubs_follow_the_stack() {
        let mut scene = super::super::tests::empty_scene();
        scene.layers = vec![
            layer(0, LayerFunction::Conductor),
            layer(1, LayerFunction::Plane),
            layer(2, LayerFunction::Conductor),
            layer(3, LayerFunction::Plane),
            layer(4, LayerFunction::Conductor),
        ];
        scene.segments = vec![
            segment(1, 0, (0.0, 0.0), (3.0, 4.0)),
            segment(2, 2, (3.0, 4.0), (3.0, 10.0)),
        ];
        scene.vias = vec![Via {
            id: ObjectId(9),
            net: NetId(7),
            at: Point::new(3.0, 4.0),
            drill: 0.2,
            drill_shape: DrillShape {
                width: 0.2,
                height: 0.2,
                plated: true,
                rectangular: false,
                custom: None,
            },
            padstack: ObjectId(1),
            padstack_name: "VIA".into(),
            start_layer: Some(LayerId(0)),
            end_layer: Some(LayerId(4)),
            pads: (0..5).map(pad).collect::<Vec<_>>().into(),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        }];
        let report = report(&scene, NetId(7), &CancellationToken::default()).unwrap();
        assert_eq!(report.segments, 2);
        assert_eq!(report.layer_lengths, [(LayerId(0), 5.0), (LayerId(2), 6.0)]);
        assert_eq!(report.total_length, 11.0);
        let via = &report.vias[0];
        assert_eq!(via.used, [LayerId(0), LayerId(2)]);
        assert_eq!(via.stubs, [(LayerId(2), LayerId(4))]);
        assert_eq!(via.stub_layers, 2);
        assert_eq!(report.stubbed_vias(), 1);
        assert_eq!(
            report.adjacent_planes,
            [
                (LayerId(0), None, Some(LayerId(1))),
                (LayerId(2), Some(LayerId(1)), Some(LayerId(3))),
            ]
        );
    }
}

/// Routed path between two pins of one net.
#[derive(Debug, Clone, PartialEq)]
pub struct PinPath {
    /// Shortest 2D centerline length along the net's traces.
    pub length: f64,
    pub segments: usize,
    pub vias: usize,
    /// The routing has loops, so other paths of different length also connect the pins.
    pub alternatives: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    DifferentNets,
    NotConnected,
}

/// Shortest routed path between two pins over the net's traces and vias. Copper
/// areas are not traversed; a path through a plane reports as not connected.
pub fn pin_path(
    scene: &BoardScene,
    from: ObjectId,
    to: ObjectId,
    cancel: &CancellationToken,
) -> Result<Result<PinPath, PathError>, Cancelled> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let pin = |id| scene.pins.iter().find(|pin| pin.id == id);
    let (Some(a), Some(b)) = (pin(from), pin(to)) else {
        return Ok(Err(PathError::NotConnected));
    };
    if a.net != b.net || a.net.0 == 0 {
        return Ok(Err(PathError::DifferentNets));
    }
    let net = a.net;
    // Nodes are (layer, quantized point); vias and pins join their layers at a point.
    let key = |layer: LayerId, p: Point| {
        (
            layer.0,
            (p.x * 1e3).round() as i64,
            (p.y * 1e3).round() as i64,
        )
    };
    let mut nodes: BTreeMap<(u32, i64, i64), usize> = BTreeMap::new();
    let mut points: Vec<(LayerId, Point)> = Vec::new();
    let mut node = |layer: LayerId, p: Point, nodes: &mut BTreeMap<_, usize>| {
        *nodes.entry(key(layer, p)).or_insert_with(|| {
            points.push((layer, p));
            points.len() - 1
        })
    };
    // (to, length, is via)
    let mut edges: Vec<Vec<(usize, f64, bool)>> = Vec::new();
    let connect =
        |edges: &mut Vec<Vec<(usize, f64, bool)>>, a: usize, b: usize, w: f64, via: bool| {
            let need = a.max(b) + 1;
            if edges.len() < need {
                edges.resize(need, Vec::new());
            }
            edges[a].push((b, w, via));
            edges[b].push((a, w, via));
        };
    for (i, segment) in scene.segments.iter().enumerate() {
        if i % 4096 == 0 {
            cancel.check()?;
        }
        if segment.net != net {
            continue;
        }
        let u = node(segment.layer, segment.a, &mut nodes);
        let v = node(segment.layer, segment.b, &mut nodes);
        connect(&mut edges, u, v, segment_length(segment), false);
    }
    // Join a point object to every node on its layers within its reach.
    let attach =
        |center: Point, reach: &dyn Fn(LayerId) -> Option<f64>, points: &[(LayerId, Point)]| {
            points
                .iter()
                .enumerate()
                .filter(|(_, (layer, p))| reach(*layer).is_some_and(|r| p.distance(center) <= r))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
    let hub = |edges: &mut Vec<Vec<(usize, f64, bool)>>,
               members: &[usize],
               points: &mut Vec<(LayerId, Point)>,
               center: Point,
               via: bool| {
        points.push((LayerId::UNASSIGNED, center));
        let id = points.len() - 1;
        for &m in members {
            let need = id.max(m) + 1;
            if edges.len() < need {
                edges.resize(need, Vec::new());
            }
            edges[id].push((m, 0.0, via));
            edges[m].push((id, 0.0, via));
        }
        id
    };
    for via in scene.vias.iter().filter(|via| via.net == net) {
        cancel.check()?;
        let reach = |layer: LayerId| {
            via.pads
                .iter()
                .filter(|pad| pad.layer == layer)
                .map(|pad| pad.width.max(pad.height) * 0.5 + TOUCH_MM)
                .reduce(f64::max)
                .or(Some(via.drill * 0.5 + TOUCH_MM))
        };
        let members = attach(via.at, &reach, &points);
        hub(&mut edges, &members, &mut points, via.at, true);
    }
    let pin_hub = |pin: &crate::model::Pin,
                   edges: &mut Vec<Vec<(usize, f64, bool)>>,
                   points: &mut Vec<(LayerId, Point)>| {
        let reach = |layer: LayerId| {
            pin.pads
                .iter()
                .filter(|pad| pad.layer == layer)
                .map(|pad| pad.width.max(pad.height) * 0.5 + TOUCH_MM)
                .reduce(f64::max)
        };
        let members = attach(pin.at, &reach, points);
        hub(edges, &members, points, pin.at, false)
    };
    let start = pin_hub(a, &mut edges, &mut points);
    let goal = pin_hub(b, &mut edges, &mut points);
    edges.resize(points.len(), Vec::new());
    let mut best = vec![f64::INFINITY; points.len()];
    let mut previous: Vec<Option<(usize, bool)>> = vec![None; points.len()];
    let mut queue = BinaryHeap::new();
    best[start] = 0.0;
    queue.push((Reverse(0_u64), start));
    while let Some((Reverse(bits), at)) = queue.pop() {
        let distance = f64::from_bits(bits);
        if distance > best[at] {
            continue;
        }
        for &(next, weight, via) in &edges[at] {
            let candidate = distance + weight;
            if candidate < best[next] {
                best[next] = candidate;
                previous[next] = Some((at, via));
                queue.push((Reverse(candidate.to_bits()), next));
            }
        }
    }
    if !best[goal].is_finite() {
        return Ok(Err(PathError::NotConnected));
    }
    let (mut segments, mut vias, mut at) = (0, 0, goal);
    while let Some((from, via)) = previous[at] {
        if via {
            vias += 1;
        } else if points[at].0 != LayerId::UNASSIGNED && points[from].0 != LayerId::UNASSIGNED {
            segments += 1;
        }
        at = from;
    }
    // A connected component with more edges than a tree has loops.
    let reached: Vec<_> = (0..points.len()).filter(|i| best[*i].is_finite()).collect();
    let component_edges = reached.iter().map(|i| edges[*i].len()).sum::<usize>() / 2;
    Ok(Ok(PinPath {
        length: best[goal],
        segments,
        // Each via is entered and left through its hub.
        vias: vias / 2,
        alternatives: component_edges >= reached.len(),
    }))
}
