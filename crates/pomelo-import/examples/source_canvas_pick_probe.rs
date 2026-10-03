//! Source-only picking oracle; the render crate's canvas probe uses MSDF quads.
use anyhow::{Context, ensure};
use pomelo_core::{
    display::BoardDisplay,
    interaction::SelectionMode,
    model::Point,
    picking::{PickFilter, PickQuery},
    picking_index::SegmentIndex,
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
struct Request {
    source: PathBuf,
    encoding: String,
    states: Vec<State>,
}
#[derive(Deserialize)]
struct State {
    name: String,
    display: BoardDisplay,
    queries: Vec<Query>,
}
#[derive(Deserialize)]
struct Query {
    point: [f64; 2],
    scale: f64,
}

fn object(object: SelectedObject) -> Value {
    match object {
        SelectedObject::Segment(id) => json!(["segment", id.0]),
        SelectedObject::Pin(id) => json!(["pin", id.0]),
        SelectedObject::Via(id) => json!(["via", id.0]),
        SelectedObject::Zone(id) => json!(["zone", id.0]),
        SelectedObject::Drawing(id) => json!(["drawing", id.0]),
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 2, "CANVAS_PICK_PROBE_USAGE");
    let output = PathBuf::from(&args[1]);
    ensure!(
        output
            .extension()
            .is_some_and(|extension| extension == "json"),
        "CANVAS_PICK_REPORT_EXTENSION"
    );
    let request: Request = serde_json::from_slice(&fs::read(&args[0])?)?;
    let bytes = fs::read(&request.source)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    drop(bytes);
    let cancel = CancellationToken::default();
    let options = ImportOptions {
        text_encoding: TextEncoding::from_tag(&request.encoding).context("CANVAS_PICK_ENCODING")?,
        ..ImportOptions::default()
    };
    let imported = AllegroImporter.import(
        &request.source,
        &options,
        &ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        },
    )?;
    let scene = &imported.scene;
    let index = SegmentIndex::build(std::sync::Arc::clone(scene), 4_000_000, &cancel)?;
    let mut states = Vec::new();
    for state in request.states {
        let mut results = Vec::new();
        for query in state.queries {
            let query_mm = PickQuery::new(
                Point::new(query.point[0], query.point[1]),
                5.0 / query.scale,
            )
            .context("CANVAS_PICK_QUERY")?;
            let hits = index.query_visible_objects(
                query_mm,
                query.scale,
                PickFilter::all(),
                &state.display,
                64,
                &cancel,
            )?;
            let mut modes = Vec::new();
            for mode in [
                SelectionMode::Object,
                SelectionMode::Track,
                SelectionMode::Net,
                SelectionMode::Component,
            ] {
                let selection = pomelo_core::selection::resolve_canvas_candidates(
                    scene, &hits, mode, 1, &cancel,
                )?
                .first()
                .map(|candidate| candidate.target);
                modes.push(match selection {
                    Some(SelectionTarget::Object(id)) => json!(["object", object(id)]),
                    Some(SelectionTarget::Track(id)) => json!(["track", id.0]),
                    Some(SelectionTarget::Net(id)) => json!(["net", id.0]),
                    Some(SelectionTarget::ComponentGroup(anchor)) => {
                        json!(["component", anchor.reference(scene, &cancel)?])
                    }
                    Some(SelectionTarget::Component(id)) => json!([
                        "component",
                        scene
                            .components
                            .iter()
                            .find(|component| component.id == id)
                            .context("CANVAS_PICK_COMPONENT")?
                            .reference
                    ]),
                    None => Value::Null,
                });
            }
            results.push(json!({"hit": hits.first().map(|hit| object(hit.object)), "modes": modes,
                "candidates":hits.iter().take(8).map(|hit|json!([object(hit.object),hit.distance_mm])).collect::<Vec<_>>() }));
        }
        states.push(json!({"name":state.name,"results":results}));
    }
    fs::write(
        output,
        serde_json::to_vec(&json!({"sha256":digest,"states":states}))?,
    )?;
    Ok(())
}
