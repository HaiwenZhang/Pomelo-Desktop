//! Development-only complete importer check without exporting gigabytes of geometry.
use std::{fs, io::Write, path::PathBuf, process::ExitCode, time::Instant};

use anyhow::{Context, ensure};
use serde_json::json;

use pomelo_core::{i18n::Locale, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};

fn run() -> anyhow::Result<ExitCode> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 3, "IMPORT_CASE_PROBE_USAGE");
    let source = PathBuf::from(&args[0]);
    let encoding =
        TextEncoding::from_tag(&args[1].to_string_lossy()).context("IMPORT_CASE_PROBE_ENCODING")?;
    let output = PathBuf::from(&args[2]);
    ensure!(
        output.extension().is_some_and(|ext| ext == "json"),
        "IMPORT_CASE_PROBE_REPORT"
    );
    let bytes = fs::metadata(&source)?.len();
    let token = CancellationToken::default();
    let start = Instant::now();
    let result = AllegroImporter.import(
        &source,
        &ImportOptions {
            text_encoding: encoding,
            ..Default::default()
        },
        &ImportContext {
            cancellation: &token,
            progress: &|_| {},
        },
    );
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut report = json!({
        "schema_version": 1, "stage": "complete-import", "path": source,
        "bytes": bytes, "encoding": encoding.tag(), "elapsed_ms": elapsed_ms,
        "scene_validated": false, "gpu_validated": false,
    });
    let status = match result {
        Ok(board) => {
            let s = &board.scene;
            report["status"] = json!("passed");
            report["identity"] = json!(board.identity);
            report["source"] = json!(board.source);
            report["bounds"] = json!(s.bounds);
            report["counts"] = json!({
                "layers":s.layers.len(), "special_layers":s.special_layers.len(),
                "nets":s.nets.len(), "segments":s.segments.len(), "vias":s.vias.len(),
                "pins":s.pins.len(), "components":s.components.len(), "zones":s.zones.len(),
                "outline":s.outline.len(), "texts":s.texts.len(),
                "drawing_layers":s.drawing_layers.len(), "drawings":s.drawings.len(),
                "diagnostics":s.diagnostics.len(),
            });
            report["diagnostics"] = json!(s.diagnostics);
            report["error"] = serde_json::Value::Null;
            ExitCode::SUCCESS
        }
        Err(error) => {
            let diagnostic = error.diagnostic().with_path(&source);
            report["status"] = json!("failed");
            report["localized_messages"] = json!(
                Locale::ALL
                    .into_iter()
                    .map(|locale| (locale.tag(), diagnostic.message.display(locale)))
                    .collect::<std::collections::BTreeMap<_, _>>()
            );
            report["error"] = json!(diagnostic);
            ExitCode::FAILURE
        }
    };
    let mut writer = std::io::BufWriter::new(fs::File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.flush()?;
    Ok(status)
}

fn main() -> anyhow::Result<ExitCode> {
    run()
}
