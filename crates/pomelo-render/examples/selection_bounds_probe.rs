//! Actual importer and MSDF glyph adapter; no GPU or window is initialized.
use anyhow::{Context, ensure};
use pomelo_core::{
    display::BoardDisplay, model::BoardScene, picking_index::SegmentIndex,
    selection::SelectionTarget, task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use pomelo_render::text::msdf::{MsdfFont, PreparedGlyphs};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
struct Request {
    source: PathBuf,
    encoding: String,
    baseline: bool,
    queries: Vec<Query>,
}
#[derive(Deserialize)]
struct Query {
    key: String,
    target: SelectionTarget,
    display: Option<BoardDisplay>,
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 2, "SELECTION_BOUNDS_PROBE_USAGE");
    let request: Request = serde_json::from_slice(&fs::read(&args[0])?)?;
    let bytes = fs::read(&request.source)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let cancel = CancellationToken::default();
    let scene: Arc<BoardScene> = if request.encoding == "synthetic" {
        Arc::new(serde_json::from_slice(&bytes)?)
    } else {
        AllegroImporter
            .import(
                &request.source,
                &ImportOptions {
                    text_encoding: TextEncoding::from_tag(&request.encoding)
                        .context("SELECTION_BOUNDS_ENCODING")?,
                    ..ImportOptions::default()
                },
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )?
            .scene
    };
    drop(bytes);
    let font = Arc::new(
        MsdfFont::bundled(scene.texts.iter().map(|t| t.text.as_str()), &cancel)
            .map_err(|d| anyhow::anyhow!("{}", d.code))?,
    );
    let glyphs = PreparedGlyphs::build(&scene.texts, font, 512 * 1024 * 1024, &cancel)
        .map_err(|d| anyhow::anyhow!("{}", d.code))?;
    let index = SegmentIndex::build(Arc::clone(&scene), 4_000_000, &cancel)?
        .with_text_quads(&glyphs.pick_quads, &cancel)?;
    let mut results = Vec::new();
    for (position, query) in request.queries.into_iter().enumerate() {
        if position % 1024 == 0 {
            eprintln!("SELECTION_BOUNDS_PROGRESS completed={position}");
        }
        let bounds = index.selection_bounds(query.target, &cancel)?;
        let baseline = if request.baseline {
            query.target.bounds(&scene, &cancel)?
        } else {
            None
        };
        let anchor = query
            .display
            .as_ref()
            .map(|display| index.selection_anchor(query.target, display, &cancel))
            .transpose()?
            .flatten();
        results.push(
            json!({"key":query.key,"bounds":bounds,"source_bounds":baseline,"anchor":anchor}),
        );
    }
    fs::write(
        &args[1],
        serde_json::to_vec(&json!({"sha256":sha256,"results":results}))?,
    )?;
    Ok(())
}
