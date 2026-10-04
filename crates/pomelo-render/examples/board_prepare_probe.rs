//! Headless desktop geometry preparation with production defaults, never visual acceptance.
use pomelo_core::{i18n::Locale, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use pomelo_render::{
    copper::{CopperLimits, PreparedCopper},
    drills::PreparedDrills,
    pads::{PadLimits, PreparedPads},
    tracks::{PreparedTracks, TraceLimits},
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(args.len() == 3, "BOARD_PREPARE_PROBE_USAGE");
    let path = PathBuf::from(&args[0]);
    let encoding = args[1]
        .to_str()
        .and_then(TextEncoding::from_tag)
        .ok_or_else(|| anyhow::anyhow!("BOARD_PREPARE_ENCODING"))?;
    let output = PathBuf::from(&args[2]);
    let cancellation = CancellationToken::default();
    let started = Instant::now();
    let mut stage = "import";
    let prepared = (|| -> anyhow::Result<serde_json::Value> {
        let board = AllegroImporter.import(
            &path,
            &ImportOptions {
                text_encoding: encoding,
                ..Default::default()
            },
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {},
            },
        )?;
        let scene = board.scene;
        stage = "tracks";
        let tracks = PreparedTracks::build_with_outline(
            &scene.segments,
            &scene.outline,
            TraceLimits::default(),
            &cancellation,
        )?;
        stage = "drawings";
        let drawings =
            PreparedTracks::build_drawings(&scene.drawings, TraceLimits::default(), &cancellation)?;
        stage = "zone-outlines";
        let outlines = PreparedTracks::build_zone_outlines(
            &scene.zones,
            TraceLimits::default(),
            &cancellation,
        )?;
        stage = "copper";
        let limits = CopperLimits::default();
        let copper = PreparedCopper::build(&scene.zones, limits, &cancellation)?;
        stage = "pads";
        let mut pads = PreparedPads::build(
            &scene.pins,
            &scene.vias,
            PadLimits::default(),
            &cancellation,
        )?;
        stage = "custom-pad-copper";
        let custom = pads.build_custom_meshes(
            limits,
            &pomelo_core::copper::MeshLimits::default(),
            &cancellation,
        )?;
        let custom_vertices = custom.vertices.len();
        let custom_indices = custom.indices.len();
        pads.custom_mesh = Some(Arc::new(custom));
        stage = "drills";
        let drills = PreparedDrills::build(
            &scene.pins,
            &scene.vias,
            PadLimits::default(),
            &cancellation,
        )?;
        stage = "search";
        let _search = pomelo_core::search::SearchIndex::build(&scene, &cancellation)?
            .ok_or(pomelo_render::tracks::PrepareError::Cancelled)?;
        stage = "picking";
        let picking = pomelo_core::picking_index::SegmentIndex::build(
            Arc::clone(&scene),
            TraceLimits::default().max_instances,
            &cancellation,
        )?;
        Ok(
            json!({"status":"passed", "source": board.source, "identity":board.identity,
          "counts":{"segments":scene.segments.len(), "zones":scene.zones.len(), "vertices":copper.vertices.len(),
            "indices":copper.indices.len(), "copper_batches":copper.batches.len(), "custom_vertices":custom_vertices,
            "custom_indices":custom_indices, "pads":pads.analytic.len(), "drills":drills.geometry.analytic.len(),
            "tracks":tracks.instances.len(), "drawings":drawings.instances.len(), "outlines":outlines.instances.len(), "pick_segments":picking.segment_count()},
          "copper_limits":{"max_vertices":limits.max_vertices,"max_indices":limits.max_indices,"max_zones":limits.max_zones,"max_bytes":limits.max_bytes},
          "copper_upload_bytes":copper.upload_bytes(),
          "copper_prepared_bytes":copper.upload_bytes()+copper.batches.len()*std::mem::size_of::<pomelo_render::copper::CopperBatch>(),
          "minimum_shared_4mib_upload_frames":copper.upload_bytes().div_ceil(4*1024*1024)}),
        )
    })();
    let mut report = match prepared {
        Ok(report) => report,
        Err(error) => {
            let diagnostic = error
                .downcast_ref::<pomelo_render::tracks::PrepareError>()
                .map(|e| e.diagnostic())
                .or_else(|| {
                    error
                        .downcast_ref::<pomelo_import::ImportError>()
                        .map(|e| e.diagnostic())
                });
            json!({"status":"failed","error":error.to_string(),"diagnostic":diagnostic,
              "localized_messages":diagnostic.as_ref().map(|d|Locale::ALL.into_iter().map(|l|(l.tag(),d.message.display(l))).collect::<std::collections::BTreeMap<_,_>>())})
        }
    };
    report["path"] = json!(path);
    report["stage"] = json!(stage);
    report["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    report["gpu_validated"] = json!(false);
    report["visual_validated"] = json!(false);
    report["font_pipeline_changed"] = json!(false);
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    anyhow::ensure!(report["status"] == "passed", "BOARD_PREPARE_FAILED");
    Ok(())
}
