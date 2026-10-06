//! Full importer-to-render preparation check, without a window or GPU.
use std::{path::PathBuf, sync::Arc};

use pomelo_core::{
    copper::MeshLimits, picking_index::SegmentIndex, search::SearchIndex, task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, formats::allegro::AllegroImporter,
};
use pomelo_render::{
    copper::{CopperLimits, PreparedCopper},
    drills::PreparedDrills,
    pads::{PadLimits, PreparedPads},
    tracks::{PreparedTracks, TraceLimits},
};

#[test]
#[ignore = "requires POMELO_PREPARE_CASE pointing to a local BRD file"]
fn real_board_import_and_prepare_with_production_limits() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::var_os("POMELO_PREPARE_CASE").expect("set POMELO_PREPARE_CASE to a BRD file"),
    );
    let encoding = std::env::var("POMELO_PREPARE_ENCODING").unwrap_or_else(|_| "utf-8".into());
    let options = ImportOptions {
        text_encoding: TextEncoding::from_tag(&encoding)
            .ok_or_else(|| anyhow::anyhow!("unsupported encoding: {encoding}"))?,
        ..Default::default()
    };
    let cancel = CancellationToken::default();
    let board = AllegroImporter.import(
        &path,
        &options,
        &ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        },
    )?;
    let scene = board.scene;
    let traces = TraceLimits::default();
    let tracks =
        PreparedTracks::build_with_outline(&scene.segments, &scene.outline, traces, &cancel)?;
    let drawings = PreparedTracks::build_drawings(&scene.drawings, traces, &cancel)?;
    let outlines = PreparedTracks::build_zone_outlines(&scene.zones, traces, &cancel)?;
    let limits = CopperLimits::default();
    let copper = PreparedCopper::build(&scene.zones, limits, &cancel)?;
    let pads = PreparedPads::build(&scene.pins, &scene.vias, PadLimits::default(), &cancel)?;
    let custom = pads.build_custom_meshes(limits, &MeshLimits::default(), &cancel)?;
    let drills = PreparedDrills::build(&scene.pins, &scene.vias, PadLimits::default(), &cancel)?;
    let search = SearchIndex::build(&scene, &cancel)?.expect("search must complete");
    let picking = SegmentIndex::build(Arc::clone(&scene), traces.max_instances, &cancel)?;

    assert_eq!(picking.segment_count(), scene.segments.len());
    assert!(copper.vertices.len() <= limits.max_vertices);
    assert!(copper.indices.len() <= limits.max_indices);
    assert!(copper.upload_bytes() <= limits.max_bytes);
    for &index in &custom.indices {
        assert!((index as usize) < custom.vertices.len());
    }
    // Keep useful preparation statistics in the test output, without a report CLI.
    eprintln!(
        "tracks={} drawings={} outlines={} copper_vertices={} copper_indices={} \
         custom_vertices={} custom_indices={} pads={} drills={} search_entries={}",
        tracks.instances.len(),
        drawings.instances.len(),
        outlines.instances.len(),
        copper.vertices.len(),
        copper.indices.len(),
        custom.vertices.len(),
        custom.indices.len(),
        pads.analytic.len(),
        drills.geometry.analytic.len(),
        search.entries().len(),
    );
    Ok(())
}
