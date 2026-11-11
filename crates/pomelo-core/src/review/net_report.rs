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
