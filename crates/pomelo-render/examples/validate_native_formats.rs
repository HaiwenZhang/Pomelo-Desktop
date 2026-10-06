//! Validate native render preparation and picking for every supplied board format.
use pomelo_core::{picking_index::BoardPickingIndex, task::CancellationToken};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use pomelo_render::{
    copper::{CopperLimits, PreparedCopper},
    drills::PreparedDrills,
    pads::{PadLimits, PreparedPads},
    tracks::{PreparedTracks, TraceLimits},
};
use std::{path::PathBuf, sync::Arc};

fn main() -> anyhow::Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(!paths.is_empty(), "usage: validate_formats <board> ...");
    let cancel = CancellationToken::default();
    for path in paths {
        let start = std::time::Instant::now();
        let board = FormatImporter.import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )?;
        let s = &board.scene;
        let _tracks = PreparedTracks::build_with_outline(
            &s.segments,
            &s.outline,
            TraceLimits::default(),
            &cancel,
        )?;
        let _drawings =
            PreparedTracks::build_drawings(&s.drawings, TraceLimits::default(), &cancel)?;
        let _outlines =
            PreparedTracks::build_zone_outlines(&s.zones, TraceLimits::default(), &cancel)?;
        let copper = PreparedCopper::build_scene(Arc::clone(s), CopperLimits::default(), &cancel)?;
        let _pads = PreparedPads::build(&s.pins, &s.vias, PadLimits::default(), &cancel)?;
        let _drills = PreparedDrills::build(&s.pins, &s.vias, PadLimits::default(), &cancel)?;
        let _picking = BoardPickingIndex::build(Arc::clone(s), usize::MAX, &cancel)?;
        println!(
            "{}: native preparation and picking passed; copper vertices={} indices={}; elapsed={:.2}s",
            path.display(),
            copper.vertex_count(),
            copper.index_count(),
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
