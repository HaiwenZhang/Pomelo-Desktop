//! External large-board regressions. Private BRD input is never checked in.
use pomelo_core::{
    copper::MeshLimits, memory::MemoryBudget, picking_index::SegmentIndex, search::SearchIndex,
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use pomelo_render::{
    copper::PreparedCopper,
    drills::PreparedDrills,
    pads::PreparedPads,
    preparation_memory::PreparationMemory,
    text::msdf::{LabelIndex, MsdfFont, PreparedGlyphs},
    tracks::PreparedTracks,
};
use std::{path::PathBuf, sync::Arc};

fn prepare_case(name: &str) -> anyhow::Result<()> {
    let root = std::env::var_os("POMELO_LARGE_BOARD_CASES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("E:/PCB/brd_cases"));
    let path = if name == "ARM_Test.mcm" {
        std::env::var_os("POMELO_LARGE_MCM_CASE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/PCB/mcm/ARM_Test.mcm"))
    } else {
        root.join(name)
    };
    let started = std::time::Instant::now();
    let mut checkpoint = started;
    let mut record_stage = |stage: &str| {
        let now = std::time::Instant::now();
        eprintln!(
            "case={name} stage={stage} elapsed_us={}",
            now.duration_since(checkpoint).as_micros()
        );
        checkpoint = now;
    };
    let cancel = CancellationToken::default();
    let board = AllegroImporter.import(
        &path,
        &ImportOptions {
            text_encoding: TextEncoding::Windows1252,
            ..Default::default()
        },
        &ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        },
    )?;
    record_stage("import");
    let scene = &board.scene;
    let budget = MemoryBudget::new(0);
    let mut memory = PreparationMemory::new(budget.clone());
    let tracks = memory.prepare_tracks(|limits| {
        PreparedTracks::build_with_outline(&scene.segments, &scene.outline, limits, &cancel)
    })?;
    record_stage("tracks");
    let drawings = memory.prepare_tracks(|limits| {
        PreparedTracks::build_drawings(&scene.drawings, limits, &cancel)
    })?;
    record_stage("drawings");
    let outlines = memory.prepare_tracks(|limits| {
        PreparedTracks::build_zone_outlines(&scene.zones, limits, &cancel)
    })?;
    record_stage("zone_outlines");
    // Diagnostic comparison only; production always uses the source-backed path.
    let eager_copper = std::env::var_os("POMELO_EAGER_COPPER").is_some();
    let copper = memory.prepare_copper(|limits| {
        if eager_copper {
            PreparedCopper::build(&scene.zones, limits, &cancel)
        } else {
            PreparedCopper::build_scene(Arc::clone(scene), limits, &cancel)
        }
    })?;
    eprintln!(
        "case={name} eager_copper={eager_copper} copper_resident_bytes={} copper_upload_bytes={}",
        copper.allocation_bytes(),
        copper.upload_bytes()
    );
    record_stage("copper");
    let pads = memory
        .prepare_pads(|limits| PreparedPads::build(&scene.pins, &scene.vias, limits, &cancel))?;
    record_stage("pads");
    let custom = memory.prepare_copper(|limits| {
        pads.build_custom_meshes(limits, &MeshLimits::default(), &cancel)
    })?;
    record_stage("custom_meshes");
    let drills = memory.prepare_pads(|limits| {
        PreparedDrills::build(&scene.pins, &scene.vias, limits, &cancel)
            .map(|drills| drills.geometry)
    })?;
    record_stage("drills");
    let font = Arc::new(
        MsdfFont::bundled(
            scene
                .texts
                .iter()
                .map(|text| text.text.as_str())
                .chain(scene.nets.values().map(String::as_str)),
            &cancel,
        )
        .map_err(|diagnostic| anyhow::anyhow!("font: {diagnostic:?}"))?,
    );
    record_stage("font");
    let labels = LabelIndex::build(Arc::clone(scene), Arc::clone(&font), &cancel)
        .map_err(|diagnostic| anyhow::anyhow!("labels: {diagnostic:?}"))?;
    record_stage("labels");
    let mut texts = PreparedGlyphs::build(&scene.texts, font, 512 * 1024 * 1024, &cancel)
        .map_err(|diagnostic| anyhow::anyhow!("text: {diagnostic:?}"))?;
    texts.bind_drawing_owners(scene);
    record_stage("text");
    let search =
        SearchIndex::build(scene, &cancel)?.ok_or_else(|| anyhow::anyhow!("cancelled search"))?;
    record_stage("search");
    let picking = SegmentIndex::build(Arc::clone(scene), u32::MAX as usize, &cancel)?
        .with_text_quads(&texts.pick_quads, &cancel)?;
    record_stage("picking");
    assert_eq!(picking.segment_count(), scene.segments.len());
    assert!(!tracks.instances.is_empty());
    assert!(copper.vertex_count() > 0);
    assert!(texts.objects.len() <= scene.texts.len());
    assert!(
        custom
            .indices
            .iter()
            .all(|&index| (index as usize) < custom.vertices.len())
    );
    // Hold every output to this point, so the test exercises simultaneous residency.
    eprintln!(
        "case={name} elapsed_ms={} tracks={} drawings={} outlines={} copper_vertices={} copper_indices={} pads={} custom_vertices={} drills={} text_objects={} glyphs={} pick_quads={} search_entries={}",
        started.elapsed().as_millis(),
        tracks.instances.len(),
        drawings.instances.len(),
        outlines.instances.len(),
        copper.vertex_count(),
        copper.index_count(),
        pads.analytic.len(),
        custom.vertices.len(),
        drills.analytic.len(),
        texts.objects.len(),
        texts.instances.len(),
        texts.pick_quads.len(),
        search.entries().len()
    );
    eprintln!(
        "shared_geometry_resident_bytes={} shared_limit_bytes={}",
        budget.used(),
        budget.limit()
    );
    drop(labels);
    Ok(())
}

#[test]
#[ignore = "requires local 15061-1b.brd; optional POMELO_LARGE_BOARD_CASES directory"]
fn large_board_15061_with_text_picking() -> anyhow::Result<()> {
    prepare_case("15061-1b.brd")
}

#[test]
#[ignore = "requires local ntpcb_320mb.brd; optional POMELO_LARGE_BOARD_CASES directory"]
fn large_board_ntpcb_320mb_with_text_picking() -> anyhow::Result<()> {
    prepare_case("ntpcb_320mb.brd")
}

#[test]
#[ignore = "requires local ARM_Test.mcm; optional POMELO_LARGE_MCM_CASE path"]
fn large_mcm_arm_with_text_picking() -> anyhow::Result<()> {
    prepare_case("ARM_Test.mcm")
}
