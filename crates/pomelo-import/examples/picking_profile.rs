//! Measure actual canvas picking at fitted and local scales on explicit inputs.
use pomelo_core::{
    display::BoardDisplay,
    model::Point,
    picking::{PickCategory, PickFilter, PickQuery},
    picking_index::BoardPickingIndex,
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, formats::allegro::AllegroImporter,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> anyhow::Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(
        !paths.is_empty(),
        "usage: picking_profile <board.brd|package.mcm> ..."
    );
    for path in paths {
        let cancel = CancellationToken::default();
        let started = Instant::now();
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
        let imported_us = started.elapsed().as_micros();
        eprintln!(
            "{} paths={} edges={}",
            path.display(),
            board
                .scene
                .zones
                .iter()
                .map(|z| z.paths.len())
                .sum::<usize>(),
            board
                .scene
                .zones
                .iter()
                .flat_map(|z| &z.paths)
                .map(Vec::len)
                .sum::<usize>()
        );
        let started = Instant::now();
        let index = BoardPickingIndex::build(Arc::clone(&board.scene), u32::MAX as usize, &cancel)?;
        let index_us = started.elapsed().as_micros();
        let bounds = board.scene.bounds;
        let fit = (1280.0 / (bounds.max.x - bounds.min.x).max(1e-9))
            .min(720.0 / (bounds.max.y - bounds.min.y).max(1e-9));
        let mut points = vec![
            (
                "outside",
                Point::new(bounds.max.x + 100.0, bounds.max.y + 100.0),
            ),
            (
                "center",
                Point::new(
                    (bounds.min.x + bounds.max.x) * 0.5,
                    (bounds.min.y + bounds.max.y) * 0.5,
                ),
            ),
        ];
        if let Some(via) = board.scene.vias.first() {
            points.push(("first_via", via.at));
        }
        if let Some(pin) = board.scene.pins.first() {
            points.push(("first_pin", pin.at));
        }
        if let Some(point) = board
            .scene
            .zones
            .first()
            .and_then(|zone| zone.mesh.vertices.first())
        {
            points.push(("first_zone_vertex", *point));
        }
        let display = BoardDisplay::default();
        for (scale, pixels_per_mm) in [("fit", fit), ("local", fit * 16.0)] {
            for &(position, point) in &points {
                let query = PickQuery::new(point, 5.0 / pixels_per_mm)
                    .ok_or_else(|| anyhow::anyhow!("invalid fitted query"))?;
                let mut no_zones = PickFilter::all();
                no_zones.set(PickCategory::Zone, false);
                for (filter_name, filter) in [("all", PickFilter::all()), ("no_zones", no_zones)] {
                    let mut previous = None;
                    for trial in 1..=3 {
                        let started = Instant::now();
                        let hits = index.query_visible_hits(
                            query,
                            pixels_per_mm,
                            filter,
                            &display,
                            64,
                            &cancel,
                        )?;
                        let elapsed_us = started.elapsed().as_micros();
                        let serialized = serde_json::to_value(&hits)?;
                        if let Some(previous) = &previous {
                            anyhow::ensure!(
                                previous == &serialized,
                                "unstable repeated picking results"
                            );
                        }
                        previous = Some(serialized.clone());
                        println!(
                            "{}",
                            serde_json::json!({
                                "case": path, "identity": board.identity, "import_us": imported_us,
                                "index_us": index_us, "scale": scale, "pixels_per_mm": pixels_per_mm,
                                "position": position, "point": point, "trial": trial,
                                "filter": filter_name, "elapsed_us": elapsed_us, "hits": serialized,
                            })
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
