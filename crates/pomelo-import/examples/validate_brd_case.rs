//! Full CPU import smoke check, with compact JSON instead of exporting every scene object.
use pomelo_core::{i18n::Locale, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde_json::json;
use std::{collections::BTreeMap, io::Write, path::PathBuf, time::Instant};

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
