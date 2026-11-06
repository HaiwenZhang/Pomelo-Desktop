//! Full CPU import smoke check, with compact JSON instead of exporting every scene object.
use pomelo_core::{i18n::Locale, model::BoardScene, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, formats::allegro::AllegroImporter,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
    time::Instant,
};

fn geometry_statistics(scene: &BoardScene) -> serde_json::Value {
    let mut copper_vertices = 0_u64;
    let mut copper_indices = 0_u64;
    let mut copper_rings = 0_u64;
    let mut largest_zone = None;
    let mut largest_zone_indices = 0;
    for zone in &scene.zones {
        copper_vertices += zone.mesh.vertices.len() as u64;
        copper_indices += zone.mesh.indices.len() as u64;
        copper_rings += zone.mesh.ring_offsets.len().saturating_sub(1) as u64;
        if zone.mesh.indices.len() > largest_zone_indices {
            largest_zone = Some(zone.id);
            largest_zone_indices = zone.mesh.indices.len();
        }
    }
    let via_padstacks: BTreeSet<_> = scene.vias.iter().map(|via| via.padstack).collect();
    json!({
        "bounds_mm": scene.bounds,
        "outline_segments": scene.outline.len(),
        "trace_arcs": scene.segments.iter().filter(|segment| segment.arc.is_some()).count(),
        "pin_pad_instances": scene.pins.iter().map(|pin| pin.pads.len() as u64).sum::<u64>(),
        "via_pad_instances": scene.vias.iter().map(|via| via.pads.len() as u64).sum::<u64>(),
        "via_padstack_ids": via_padstacks.len(),
        "vias_without_span": scene.vias.iter().filter(|via| via.start_layer.is_none() || via.end_layer.is_none()).count(),
        "vias_with_backdrill": scene.vias.iter().filter(|via| via.backdrill.is_some()).count(),
        "plated_via_drills": scene.vias.iter().filter(|via| via.drill_shape.plated).count(),
        "plated_pin_drills": scene.pins.iter().filter(|pin| pin.drill_shape.plated && pin.drill_shape.width > 0.0 && pin.drill_shape.height > 0.0).count(),
        "copper_coverage_vertices": copper_vertices,
        "copper_coverage_indices": copper_indices,
        "copper_coverage_rings": copper_rings,
        "largest_zone_by_coverage_indices": { "object": largest_zone, "indices": largest_zone_indices },
        "layers": scene.layers.iter().map(|layer| json!({ "id": layer.id, "name": layer.name, "function": layer.function })).collect::<Vec<_>>(),
        "physical_stackup_available": false,
        "mesh3d_generated": false
    })
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("Missing BRD path"))?,
    );
    let encoding = match args.next() {
        Some(tag) => TextEncoding::from_tag(&tag.to_string_lossy())
            .ok_or_else(|| anyhow::anyhow!("Unknown encoding"))?,
        None => TextEncoding::Auto,
    };
    let options = ImportOptions {
        text_encoding: encoding,
        ..ImportOptions::default()
    };
    let cancellation = CancellationToken::default();
    let started = Instant::now();
    let result = AllegroImporter.import(
        &path,
        &options,
        &ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        },
    );
    let elapsed_ms = started.elapsed().as_millis();
    let bytes = std::fs::metadata(&path)?.len();
    let report = match result {
        Ok(board) => {
            let scene = &board.scene;
            let mut diagnostics = BTreeMap::<&str, usize>::new();
            for diagnostic in &scene.diagnostics {
                *diagnostics.entry(diagnostic.code.as_ref()).or_default() += 1;
            }
            json!({
                "path": path, "bytes": bytes, "requested_encoding": encoding.tag(),
                "status": if scene.diagnostics.is_empty() { "passed" } else { "partial" },
                "encoding": board.identity.encoding, "version": board.source.layout_version,
                "elapsed_ms": elapsed_ms, "scene_built": true, "gpu_validated": false,
                "counts": { "layers": scene.layers.len(), "nets": scene.nets.len(),
                    "segments": scene.segments.len(), "components": scene.components.len(),
                    "pins": scene.pins.len(), "vias": scene.vias.len(), "zones": scene.zones.len(),
                    "texts": scene.texts.len(), "drawings": scene.drawings.len() },
                "geometry": geometry_statistics(scene),
                "diagnostics": diagnostics, "diagnostic_samples": scene.diagnostics.iter().take(5).collect::<Vec<_>>()
            })
        }
        Err(error) => {
            let diagnostic = error.diagnostic();
            json!({ "path": path, "bytes": bytes, "requested_encoding": encoding.tag(),
                "status": "failed", "elapsed_ms": elapsed_ms, "scene_built": false,
                "gpu_validated": false, "error": diagnostic,
                "message": diagnostic.message.display(Locale::SimplifiedChinese) })
        }
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &report)?;
    writeln!(stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::{
        copper::CopperMesh,
        model::{Bounds, LayerId, NetId, ObjectId, Point, Zone, ZoneKind},
    };

    #[test]
    fn copper_statistics_keep_coverage_separate_from_generated_solids() {
        let mut scene = BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: BTreeMap::new(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            diagnostics: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::new(10.0, 10.0),
            },
        };
        let empty = geometry_statistics(&scene);
        assert_eq!(empty["copper_coverage_rings"], 0);
        assert!(empty["largest_zone_by_coverage_indices"]["object"].is_null());
        for (id, count) in [(1, 6), (2, 12)] {
            scene.zones.push(Zone {
                id: ObjectId(id),
                layer: LayerId(0),
                net: NetId(0),
                kind: ZoneKind::Static,
                paths: vec![],
                mesh: CopperMesh {
                    vertices: vec![Point::default(); 8],
                    indices: vec![0; count],
                    ring_offsets: vec![0, 4, 8],
                    ..Default::default()
                },
            });
        }
        let stats = geometry_statistics(&scene);
        assert_eq!(stats["copper_coverage_vertices"], 16);
        assert_eq!(stats["copper_coverage_indices"], 18);
        assert_eq!(stats["copper_coverage_rings"], 4);
        assert_eq!(stats["largest_zone_by_coverage_indices"]["object"], 2);
        assert_eq!(stats["physical_stackup_available"], false);
        assert_eq!(stats["mesh3d_generated"], false);
    }
}
