//! Offline classification evidence, explicitly separate from GPU/Allegro validation.
use anyhow::{Context, ensure};
use pomelo_core::{model::ZoneKind, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 2, "ZONE_KIND_PROBE_USAGE");
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    ensure!(
        output.extension().is_some_and(|ext| ext == "json"),
        "ZONE_KIND_PROBE_OUTPUT"
    );
    let token = CancellationToken::default();
    let board = AllegroImporter.import(
        &source,
        &ImportOptions {
            text_encoding: TextEncoding::from_tag("windows-1252")
                .context("ZONE_KIND_PROBE_ENCODING")?,
            ..Default::default()
        },
        &ImportContext {
            cancellation: &token,
            progress: &|_| {},
        },
    )?;
    let mut counts = [0usize; 3];
    let mut target = Vec::new();
    for zone in &board.scene.zones {
        counts[match zone.kind {
            ZoneKind::Unknown => 0,
            ZoneKind::Static => 1,
            ZoneKind::Dynamic => 2,
        }] += 1;
        if [2458525349, 2458822263].contains(&zone.id.0) {
            target.push(json!({"id":zone.id,"kind":zone.kind}));
        }
    }
    let report = json!({"source":source,"identity":board.identity,"source_metadata":board.source,"counts":{"unknown":counts[0],"static":counts[1],"dynamic":counts[2]},"targets":target,"diagnostics":board.scene.diagnostics,"visual_validated":false,"gpu_validated":false});
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
