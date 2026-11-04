//! Full importer-to-render preparation check, without a window or GPU.
use std::{path::PathBuf, sync::Arc};

use pomelo_core::{
    copper::MeshLimits, picking_index::BoardPickingIndex, search::SearchIndex,
    task::CancellationToken,
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
    let placements = pomelo_render::scene::placement::prepare(&scene, limits, &cancel)?;
    let pads = PreparedPads::build(&scene.pins, &scene.vias, PadLimits::default(), &cancel)?;
    let custom = pads.build_custom_meshes(limits, &MeshLimits::default(), &cancel)?;
    let drills = PreparedDrills::build(&scene.pins, &scene.vias, PadLimits::default(), &cancel)?;
    let search = SearchIndex::build(&scene, &cancel)?.expect("search must complete");
    let picking = BoardPickingIndex::build(Arc::clone(&scene), traces.max_instances, &cancel)?;
    let mut placement_picks = 0;
    for component in scene
        .components
        .iter()
        .filter(|component| component.body.is_some())
        .take(16)
    {
        use pomelo_core::{
            display::{BoardDisplay, PlacementQuickview},
            geometry::{path_bounds, path_contains},
            interaction::Camera,
            model::Point,
            picking::{PickFilter, PickQuery},
            selection::SelectedObject,
        };
        let body = component.body.as_ref().unwrap();
        let mut point = None;
        'regions: for region in &body.regions {
            let Some(outer) = region.paths.first() else {
                continue;
            };
            let Some(bounds) = path_bounds(outer) else {
                continue;
            };
            for x in [0.25, 0.5, 0.75] {
                for y in [0.25, 0.5, 0.75] {
                    let p = Point::new(
                        bounds.min.x + (bounds.max.x - bounds.min.x) * x,
                        bounds.min.y + (bounds.max.y - bounds.min.y) * y,
                    );
                    if !path_contains(outer, p, &cancel)? {
                        continue;
                    }
                    let mut hole = false;
                    for path in region.paths.iter().skip(1) {
                        hole |= path_contains(path, p, &cancel)?;
                    }
                    if !hole {
                        point = Some(p);
                        break 'regions;
                    }
                }
            }
        }
        let Some(point) = point else { continue };
        let camera = Camera {
            center: point,
            pixels_per_mm: 100.0,
            flipped: false,
        };
        let query = PickQuery::from_viewport(
            camera,
            Point::new(50.0, 50.0),
            Point::new(100.0, 100.0),
            5.0,
        )
        .unwrap();
        let mut display = BoardDisplay {
            quickview: Some(PlacementQuickview {
                layer: body.layer,
                pins: false,
            }),
            ..Default::default()
        };
        display
            .hidden_layers
            .extend(scene.layers.iter().map(|layer| layer.id));
        let hits =
            picking.query_visible_hits(query, 100.0, PickFilter::all(), &display, 64, &cancel)?;
        assert!(
            hits.iter()
                .any(|hit| hit.anchor.object == SelectedObject::Placement(component.id)),
            "source placement {} must remain a real selectable hit",
            component.reference
        );
        placement_picks += 1;
    }
    assert!(
        placement_picks > 0
            || !scene
                .components
                .iter()
                .any(|component| component.body.is_some())
    );
    eprintln!("verified_source_placement_picks={placement_picks}");

    assert_eq!(picking.segment_count(), scene.segments.len());
    assert!(copper.vertices.len() <= limits.max_vertices);
    assert!(copper.indices.len() <= limits.max_indices);
    assert!(copper.upload_bytes() <= limits.max_bytes);
    assert!(placements.upload_bytes() <= limits.max_bytes);
    eprintln!(
        "placement_bodies={} placement_regions={} placement_vertices={} placement_indices={}",
        scene.components.iter().filter(|c| c.body.is_some()).count(),
        placements.batches.len(),
        placements.vertices.len(),
        placements.indices.len()
    );
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
