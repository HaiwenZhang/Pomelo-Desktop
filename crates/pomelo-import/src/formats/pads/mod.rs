//! Native PADS binary import. The reference TS layouts are ported into checked Rust readers.
mod binary;
mod connectivity;
mod copper;
mod definitions;
mod outline;
mod owners;
mod routes;

use super::Output;
use crate::{ImportContext, ImportError};
use pomelo_core::{
    geometry::pad::PadPlacement,
    model::{BoardScene, Bounds, Layer, LayerFunction, LayerId, NetId, Point},
};
use std::collections::{BTreeMap, HashMap};

pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<Output, ImportError> {
    let reader = binary::Reader::read(bytes, context)?;
    let metadata = definitions::metadata(&reader, context)?;
    let stacks = definitions::stacks(&reader, context)?;
    let (footprints, instances) =
        definitions::footprints(&reader, &stacks, &metadata.placements, context)?;
    let physical: HashMap<_, _> = metadata
        .layers
        .iter()
        .filter(|l| l.kind == 1)
        .enumerate()
        .map(|(i, l)| (l.id, i as u32))
        .collect();
    let assignments = connectivity::pin_nets(&reader, &metadata.nets, context)?;
    let junctions = connectivity::junctions(&reader, &metadata.nets, context)?;
    let mut diagnostics = vec!["PADS ordinary graphics and text are not yet imported".into()];
    let (pins, vias, components) = owners::build(
        &reader,
        &metadata,
        &stacks,
        &footprints,
        &instances,
        &assignments,
        &junctions,
        &physical,
        context,
        &mut diagnostics,
    )?;
    let segments = routes::routes(&reader, &metadata.layers, &physical, &junctions, context)?;
    let outline = outline::outline(&reader, context)?;
    if outline.is_empty() {
        diagnostics.push("PADS has no dedicated board outline".into());
    }
    let zones = copper::zones(
        &reader,
        &metadata.nets,
        &junctions,
        &physical,
        context,
        &mut diagnostics,
    )?;
    let colors = [
        "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd",
    ];
    let layers = metadata
        .layers
        .into_iter()
        .filter(|l| l.kind == 1)
        .enumerate()
        .map(|(i, l)| Layer {
            id: LayerId(i as u32),
            name: l.name,
            function: LayerFunction::Conductor,
            color: colors[i % colors.len()].into(),
            source_flags: None,
        })
        .collect();
    let nets: BTreeMap<_, _> = metadata
        .nets
        .into_iter()
        .enumerate()
        .filter(|(_, n)| !n.name.is_empty())
        .map(|(i, n)| (NetId(i as u32 + 1), n.name))
        .collect();
    let mut bounds = Bounds {
        min: Point::new(f64::INFINITY, f64::INFINITY),
        max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    for edge in segments.iter().chain(&outline) {
        if let Some(b) = edge.bounds() {
            bounds.include(b.min);
            bounds.include(b.max);
        }
    }
    for pin in &pins {
        for pad in &pin.pads {
            if let Some(b) = pad.bounds(PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            }) {
                bounds.include(b.min);
                bounds.include(b.max);
            }
        }
        bounds.include(Point::new(
            pin.at.x - pin.drill * 0.5,
            pin.at.y - pin.drill * 0.5,
        ));
        bounds.include(Point::new(
            pin.at.x + pin.drill * 0.5,
            pin.at.y + pin.drill * 0.5,
        ));
    }
    for via in &vias {
        for pad in via.pads.iter() {
            if let Some(b) = pad.bounds(PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            }) {
                bounds.include(b.min);
                bounds.include(b.max);
            }
        }
    }
    for zone in &zones {
        for ring in &zone.rings {
            for &point in ring {
                bounds.include(point);
            }
        }
    }
    let scene = BoardScene {
        layers,
        special_layers: Vec::new(),
        nets,
        segments,
        pins,
        components,
        vias,
        zones: Vec::new(),
        outline,
        texts: Vec::new(),
        drawing_layers: Vec::new(),
        drawings: Vec::new(),
        bounds,
        diagnostics: Vec::new(),
    };
    Ok(Output {
        scene,
        zones,
        drawing_layers: Vec::new(),
        special_layers: Vec::new(),
        diagnostics,
        info: serde_json::json!({"version":reader.version}),
    })
}
