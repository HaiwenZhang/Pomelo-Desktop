//! Diagnose production geometry preparation with explicit source input.
use std::{path::PathBuf, time::Instant};

use pomelo_core::task::CancellationToken;
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
use pomelo_render::{
    copper::{CopperLimits, PreparedCopper},
    tracks::{PreparedTracks, TraceLimits},
};

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("usage: geometry_memory <board.brd>"))?,
    );
    let cancel = CancellationToken::default();
    let started = Instant::now();
    let imported = AllegroImporter.import(
        &path,
        &ImportOptions::default(),
        &ImportContext {
            cancellation: &cancel,
            progress: &|progress| eprintln!("import {:?} {}", progress.stage, progress.completed),
        },
    )?;
    eprintln!("import_ms={}", started.elapsed().as_millis());
    let scene = &imported.scene;
    let stages = [
        (
            "tracks",
            PreparedTracks::build_with_outline(
                &scene.segments,
                &scene.outline,
                TraceLimits::default(),
                &cancel,
            ),
        ),
        (
            "drawings",
            PreparedTracks::build_drawings(&scene.drawings, TraceLimits::default(), &cancel),
        ),
        (
            "zone_outlines",
            PreparedTracks::build_zone_outlines(&scene.zones, TraceLimits::default(), &cancel),
        ),
    ];
    // Retain every successful output together, matching production residency.
    for (name, result) in &stages {
        match result {
            Ok(prepared) => println!(
                "{name} instances={} batches={} resident_bytes={}",
                prepared.instances.len(),
                prepared.batches.len(),
                prepared.instances.capacity() * size_of::<pomelo_render::tracks::TraceInstance>()
                    + prepared.batches.capacity() * size_of::<pomelo_render::tracks::TraceBatch>()
            ),
            Err(error) => println!("{name} error={error}"),
        }
    }
    let copper = PreparedCopper::build(&scene.zones, CopperLimits::default(), &cancel);
    match &copper {
        Ok(prepared) => println!(
            "copper vertices={} indices={} upload_bytes={}",
            prepared.vertices.len(),
            prepared.indices.len(),
            prepared.upload_bytes()
        ),
        Err(error) => println!("copper error={error}"),
    }
    let pads = pomelo_render::pads::PreparedPads::build(
        &scene.pins,
        &scene.vias,
        pomelo_render::pads::PadLimits::default(),
        &cancel,
    );
    let custom = pads.as_ref().map(|pads| {
        pads.build_custom_meshes(
            CopperLimits::default(),
            &pomelo_core::copper::MeshLimits::default(),
            &cancel,
        )
    });
    let drills = pomelo_render::drills::PreparedDrills::build(
        &scene.pins,
        &scene.vias,
        pomelo_render::pads::PadLimits::default(),
        &cancel,
    );
    let font = pomelo_render::text::msdf::MsdfFont::bundled(
        scene
            .texts
            .iter()
            .map(|text| text.text.as_str())
            .chain(scene.nets.values().map(String::as_str)),
        &cancel,
    )
    .map_err(|error| anyhow::anyhow!("font: {error:?}"))?;
    let texts = pomelo_render::text::msdf::PreparedGlyphs::build(
        &scene.texts,
        std::sync::Arc::new(font),
        512 * 1024 * 1024,
        &cancel,
    );
    println!(
        "msdf_text={:?}",
        texts.as_ref().map(|source| source.pick_quads.len())
    );
    let picking = pomelo_core::picking_index::SegmentIndex::build(
        std::sync::Arc::clone(&imported.scene),
        TraceLimits::default().max_instances,
        &cancel,
    )
    .and_then(|index| match &texts {
        Ok(source) => index.with_text_quads(&source.pick_quads, &cancel),
        Err(_) => Ok(index),
    });
    println!(
        "pads={:?} custom={:?} drills={:?} picking={:?}",
        pads.as_ref().map(|_| ()),
        custom.as_ref().map(|result| result.as_ref().map(|_| ())),
        drills.as_ref().map(|_| ()),
        picking.as_ref().map(|_| ())
    );
    anyhow::ensure!(
        stages.iter().all(|(_, result)| result.is_ok())
            && copper.is_ok()
            && pads.is_ok()
            && custom.as_ref().is_ok_and(Result::is_ok)
            && drills.is_ok()
            && texts.is_ok()
            && picking.is_ok(),
        "geometry preparation failed; see stage diagnostics"
    );
    Ok(())
}
