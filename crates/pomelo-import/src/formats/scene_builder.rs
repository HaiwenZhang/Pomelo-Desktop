//! Shared scene construction for native source readers.
use super::ParsedBoard;
use pomelo_core::{
    geometry::pad::PadPlacement,
    model::{BoardScene, Bounds, Point},
};
pub(super) fn new_parsed_board(version: impl Into<serde_json::Value>) -> ParsedBoard {
    ParsedBoard {
        scene: BoardScene {
            layers: Vec::new(),
            special_layers: Vec::new(),
            nets: Default::default(),
            segments: Vec::new(),
            pins: Vec::new(),
            components: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            outline: Vec::new(),
            texts: Vec::new(),
            drawing_layers: Vec::new(),
            drawings: Vec::new(),
            bounds: Bounds {
                min: Point::new(f64::INFINITY, f64::INFINITY),
                max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
            },
            diagnostics: Vec::new(),
        },
        zones: Vec::new(),
        drawing_layers: Vec::new(),
        special_layers: Vec::new(),
        diagnostics: Vec::new(),
        info: serde_json::json!({"version":version.into()}),
    }
}
pub(super) fn update_scene_bounds(output: &mut ParsedBoard) {
    let scene = &mut output.scene;
    let bounds = &mut scene.bounds;
    let mut include = |b: Bounds| {
        bounds.include(b.min);
        bounds.include(b.max);
    };
    for segment in scene
        .segments
        .iter()
        .chain(&scene.outline)
        .chain(scene.drawings.iter().flat_map(|d| &d.segments))
    {
        if let Some(b) = segment.bounds() {
            include(b);
        }
    }
    for pin in &scene.pins {
        for pad in &pin.pads {
            if let Some(b) = pad.bounds(PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            }) {
                include(b);
            }
        }
        let r = pin.drill * 0.5;
        include(Bounds {
            min: Point::new(pin.at.x - r, pin.at.y - r),
            max: Point::new(pin.at.x + r, pin.at.y + r),
        });
    }
    for via in &scene.vias {
        for pad in via.pads.iter() {
            if let Some(b) = pad.bounds(PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            }) {
                include(b);
            }
        }
        let r = via.drill * 0.5;
        include(Bounds {
            min: Point::new(via.at.x - r, via.at.y - r),
            max: Point::new(via.at.x + r, via.at.y + r),
        });
    }
    for zone in &output.zones {
        for ring in &zone.rings {
            for &p in ring {
                bounds.include(p);
            }
        }
        for path in &zone.paths {
            for edge in path {
                if let Some(b) = edge.bounds() {
                    bounds.include(b.min);
                    bounds.include(b.max);
                }
            }
        }
    }
}
