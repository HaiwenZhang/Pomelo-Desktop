//! Explicit local EDB regression; proprietary input stays outside the repository.
use pomelo_core::{
    model::{LayerId, NetId, ObjectId},
    search::SearchIndex,
    task::CancellationToken,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[test]
#[ignore = "requires POMELO_HFSS_12_LAYER_CASE pointing at the local edb.def"]
fn twelve_layer_edb_preserves_geometry_connectivity_and_search() {
    let path = PathBuf::from(
        std::env::var_os("POMELO_HFSS_12_LAYER_CASE").expect("set POMELO_HFSS_12_LAYER_CASE"),
    );
    let cancel = CancellationToken::default();
    let board = FormatImporter
        .import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    assert_board(&board, [12, 4561, 116737, 24349, 7262, 16155, 1441]);
    assert!(
        board
            .scene
            .vias
            .iter()
            .all(|via| via.drill_shape.plated && via.drill_shape.pad().is_some())
    );
}

fn assert_board(board: &pomelo_import::ImportedBoard, counts: [usize; 7]) {
    let scene = &board.scene;
    assert_eq!(board.source.format, "hfss");
    assert_eq!(
        [
            scene.layers.len(),
            scene.nets.len(),
            scene.segments.len(),
            scene.pins.len(),
            scene.components.len(),
            scene.vias.len(),
            scene.zones.len(),
        ],
        counts
    );
    if matches!(board.source.writer_version.as_str(), "8.6" | "9.5") && scene.layers.len() == 12 {
        assert_eq!(scene.diagnostics.len(), 2);
        assert!(scene.diagnostics.iter().all(|d| {
            d.technical_details
                .as_ref()
                .is_some_and(|s| s.contains("invalid hole plating ratio"))
        }));
    } else {
        assert!(scene.diagnostics.is_empty(), "{:?}", scene.diagnostics);
    }
    assert!(scene.bounds.is_valid());
    assert!(
        SearchIndex::build(scene, &CancellationToken::default())
            .unwrap()
            .is_some()
    );
    let layers: HashSet<_> = scene
        .layers
        .iter()
        .map(|layer| layer.id)
        .chain(scene.special_layers.iter().map(|layer| layer.id))
        .collect();
    let net_exists = |id| id == NetId(0) || scene.nets.contains_key(&id);
    let pins: HashMap<_, _> = scene.pins.iter().map(|pin| (pin.id, pin)).collect();
    assert_eq!(pins.len(), scene.pins.len());
    let mut members = HashSet::new();
    for component in &scene.components {
        for id in &component.pins {
            assert_eq!(pins[id].owner_id, component.id);
            assert!(members.insert(*id));
        }
    }
    for pin in &scene.pins {
        assert!(net_exists(pin.net) && pin.at.x.is_finite() && pin.at.y.is_finite());
        assert!(pin.owner_id == ObjectId(0) || members.contains(&pin.id));
        for pad in &pin.pads {
            assert!(layers.contains(&pad.layer));
            assert!(pad.width.is_finite() && pad.height.is_finite());
        }
    }
    for via in &scene.vias {
        assert!(net_exists(via.net) && via.at.x.is_finite() && via.at.y.is_finite());
        if let (Some(first), Some(last)) = (via.start_layer, via.end_layer) {
            assert!(first <= last && layers.contains(&first) && layers.contains(&last));
        }
    }
    let mut edge_ids = HashSet::new();
    for edge in scene.segments.iter().chain(&scene.outline) {
        assert!(edge_ids.insert(edge.id));
        assert!(edge.bounds().is_some() && net_exists(edge.net));
        assert!(edge.layer == LayerId::UNASSIGNED || layers.contains(&edge.layer));
    }
    for zone in &scene.zones {
        assert!(layers.contains(&zone.layer) && net_exists(zone.net));
        assert!(!zone.mesh.indices.is_empty());
    }
}

#[test]
#[ignore = "requires POMELO_EDB_CASES pointing at the local multi-version case directory"]
fn edb_archive_versions_preserve_all_board_families() {
    let root = PathBuf::from(std::env::var_os("POMELO_EDB_CASES").expect("set POMELO_EDB_CASES"));
    let mut paths = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("edb.def"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    assert!(paths.len() >= 18, "expected all local EDB version samples");
    let cancel = CancellationToken::default();
    let mut versions = HashSet::new();
    for path in paths {
        let board = FormatImporter
            .import(
                &path,
                &ImportOptions::default(),
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let counts = match (board.scene.layers.len(), board.scene.nets.len()) {
            (12, 4561) => [12, 4561, 116737, 24349, 7262, 16155, 1441],
            (8, 5905) => [8, 5905, 307322, 27744, 5095, 16880, 1221],
            (8, 334) => [8, 334, 7998, 1726, 293, 1117, 72],
            other => panic!("unexpected sample family {other:?}"),
        };
        versions.insert(board.source.writer_version.clone());
        assert_board(&board, counts);
        assert!(
            board
                .scene
                .vias
                .iter()
                .filter(|via| via.drill_shape.pad().is_some())
                .count()
                > 1000
        );
        if !matches!(board.source.writer_version.as_str(), "8.6" | "9.5")
            || board.scene.layers.len() != 12
        {
            assert!(board.scene.vias.iter().all(|via| via.drill_shape.plated));
        }
    }
    for version in ["7.3", "8.6", "9.5", "10.6", "12.1"] {
        assert!(
            versions.contains(version),
            "missing archive version {version}"
        );
    }
}

#[test]
#[ignore = "requires POMELO_HFSS_T40_CASE pointing at the local edb.def"]
fn transformed_components_rectangular_holes_and_graphic_layers_import_together() {
    let path =
        PathBuf::from(std::env::var_os("POMELO_HFSS_T40_CASE").expect("set POMELO_HFSS_T40_CASE"));
    let cancel = CancellationToken::default();
    let board = FormatImporter
        .import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let scene = &board.scene;
    assert_eq!(board.source.writer_version, "9.0");
    assert_eq!(
        [
            scene.layers.len(),
            scene.nets.len(),
            scene.segments.len(),
            scene.pins.len(),
            scene.components.len(),
            scene.vias.len(),
            scene.zones.len(),
            scene.drawings.len()
        ],
        [8, 348, 8334, 3563, 509, 2136, 234, 291]
    );
    assert!(
        scene
            .vias
            .iter()
            .all(|via| via.drill_shape.plated && via.drill_shape.pad().is_some())
    );
    assert_eq!(
        scene
            .pins
            .iter()
            .filter(|pin| pin.drill_shape.rectangular)
            .count(),
        8
    );
    assert_eq!(
        scene
            .pins
            .iter()
            .filter(|pin| pin.drill_shape.pad().is_some() && !pin.drill_shape.plated)
            .count(),
        3
    );
    let pin = scene
        .pins
        .iter()
        .find(|pin| pin.reference == "C380" && pin.name == "1")
        .unwrap();
    assert!((pin.at.x - 19.2999995).abs() < 1e-8);
    assert!((pin.at.y - 50.88999982).abs() < 1e-8);
    assert!(pin.angle.abs() < 1e-12);
    assert!(
        scene
            .drawing_layers
            .iter()
            .any(|layer| layer.source_name == "Top Overlay")
    );
    assert!(
        scene
            .drawing_layers
            .iter()
            .any(|layer| layer.source_name == "Bottom Solder")
    );
    assert_eq!(scene.diagnostics.len(), 1);
    assert!(
        scene.diagnostics[0]
            .technical_details
            .as_ref()
            .is_some_and(|d| d.contains("boundaries"))
    );
    assert!(SearchIndex::build(scene, &cancel).unwrap().is_some());
}
