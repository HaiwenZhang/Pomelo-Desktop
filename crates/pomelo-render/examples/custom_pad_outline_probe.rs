//! Custom pad GPU geometry differential adapter; never linked into the desktop app.
use pomelo_core::task::CancellationToken;
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use pomelo_render::pads::{PadLimits, PreparedPads};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        (2..=3).contains(&args.len()),
        "CUSTOM_PAD_OUTLINE_PROBE_USAGE"
    );
    let source = Path::new(&args[0]);
    let cancel = CancellationToken::default();
    let encoding = args
        .get(2)
        .map_or(Some(TextEncoding::Utf8), |tag| {
            tag.to_str().and_then(TextEncoding::from_tag)
        })
        .ok_or_else(|| anyhow::anyhow!("CUSTOM_PAD_OUTLINE_ENCODING"))?;
    let options = ImportOptions {
        text_encoding: encoding,
        ..ImportOptions::default()
    };
    let scene = AllegroImporter
        .import(
            source,
            &options,
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )?
        .scene;
    let prepared = PreparedPads::build(&scene.pins, &scene.vias, PadLimits::default(), &cancel)?;
    let point = |p: [f32; 4]| {
        [
            f64::from(p[0]) + f64::from(p[2]),
            f64::from(p[1]) + f64::from(p[3]),
        ]
    };
    let mut edges: Vec<_> = prepared
        .custom_outlines
        .iter()
        .flat_map(|source| &source.instances)
        .collect();
    edges.sort_unstable_by_key(|edge| edge.flags[1]);
    fs::write(
        &args[1],
        serde_json::to_vec(&json!({
            "sha256": format!("{:x}", Sha256::digest(fs::read(source)?)),
            "edges": edges.iter().map(|edge| json!({
                "owner": if edge.flags[3] & 16 != 0 { "pin" } else { "via" },
                "id": edge.ids[0], "layer": edge.ids[2], "net": edge.ids[3],
                "a": point(edge.a), "b": point(edge.b),
                "arc": (edge.flags[0] == 1).then(|| json!({
                    "center": point(edge.center), "radius": f64::from(edge.arc[0]) + f64::from(edge.arc[1]),
                    "start": edge.arc[2], "sweep": edge.arc[3],
                })),
            })).collect::<Vec<_>>(),
        }))?,
    )?;
    Ok(())
}
