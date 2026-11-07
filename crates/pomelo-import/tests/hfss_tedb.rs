//! Local PyEDB corpus regression. Example databases remain outside this repository.
use pomelo_core::{model::Point, pad::PadPlacement, search::SearchIndex, task::CancellationToken};
use pomelo_import::{
    BoardImporter, ImportContext, ImportError, ImportOptions, formats::FormatImporter,
};
use std::path::PathBuf;

#[test]
#[ignore = "requires POMELO_HFSS_TEDB pointing at pyedb/tests/example_models/TEDB"]
fn tedb_examples_import_all_nonempty_layouts_and_preserve_polygon_drills() {
    let root = PathBuf::from(std::env::var_os("POMELO_HFSS_TEDB").expect("set POMELO_HFSS_TEDB"));
    // Layers, nets, pins, vias: independent object families must not disappear on import.
    let cases = [
        ("ANSYS-HSD_V1", [8, 348, 3563, 2136]),
        ("ANSYS-HSD_V1_cut", [8, 81, 452, 345]),
        ("chip", [6, 0, 6, 0]),
        ("chip_flipped_stackup", [5, 0, 6, 0]),
        ("chip_offset", [6, 0, 6, 0]),
        ("chip_solder", [6, 0, 6, 0]),
        ("chip_zoffset", [6, 0, 6, 0]),
        ("chip_zoffset_solder", [6, 0, 6, 0]),
        ("clip", [2, 0, 0, 0]),
        ("component_no_ref_pins", [2, 17, 16, 94]),
        ("dc_flow", [8, 35, 184, 127]),
        ("edge_ports", [3, 0, 0, 0]),
        ("example_arbitrary_wave_ports", [1, 13, 0, 12]),
        ("example_package", [4, 97, 400, 321]),
        ("lam_for_bottom_place", [3, 5, 18, 0]),
        ("lam_for_top_place", [3, 7, 18, 0]),
        ("lam_for_top_place_no_setups", [3, 7, 18, 0]),
        ("lam_with_mold", [3, 6, 6, 0]),
        ("merge_via_4layers", [4, 3, 0, 42]),
        ("MicrostripSpliGnd", [2, 4, 2, 0]),
        ("MicrostripSplitGND", [2, 4, 2, 0]),
        ("multi_cells", [8, 348, 3563, 2136]),
        ("Multizone_GroundVoids", [2, 23, 86, 110]),
        ("Package", [4, 116, 400, 296]),
        ("padstacks", [2, 1, 0, 23]),
        ("simple", [6, 5, 126, 88]),
        ("test_merge_polygon", [4, 2, 0, 1]),
        ("test_path_length", [1, 5, 0, 0]),
        ("test_pin_group", [6, 432, 1951, 1137]),
        ("test_replace_vias", [2, 6, 0, 5]),
        ("test_sources", [6, 432, 1951, 1137]),
        ("unitcell", [2, 2, 0, 48]),
        ("via_fence_generic_project", [2, 3, 0, 165]),
        ("vias_300", [4, 4, 0, 301]),
    ];
    let cancel = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    for (name, counts) in cases {
        let path = root.join(format!("{name}.aedb")).join("edb.def");
        let imported = FormatImporter
            .import(&path, &ImportOptions::default(), &context)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let scene = &imported.scene;
        assert_eq!(
            [
                scene.layers.len(),
                scene.nets.len(),
                scene.pins.len(),
                scene.vias.len()
            ],
            counts,
            "{name}"
        );
        assert!(scene.bounds.is_valid(), "{name}");
        assert!(
            SearchIndex::build(scene, &cancel).unwrap().is_some(),
            "{name}"
        );
        if matches!(name, "Package" | "multi_cells" | "vias_300") {
            assert!(
                scene.diagnostics.iter().any(|d| d
                    .technical_details
                    .as_ref()
                    .is_some_and(|s| s.contains("displaying the first cell"))),
                "{name}"
            );
        }
        if name == "padstacks" {
            let via = scene
                .vias
                .iter()
                .find(|v| v.padstack_name == "MyVia_poly")
                .unwrap();
            let geometry = via
                .drill_shape
                .custom
                .as_ref()
                .expect("the original polygon hole");
            assert_eq!(geometry.paths[0].len(), 8);
            let hole = via.drill_shape.pad().unwrap();
            let owner = PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            let corner = hole.to_world(Point::new(0.29, 0.29), owner);
            assert_eq!(
                hole.custom_covers_fill(via.at, owner, &cancel).unwrap(),
                Some(true)
            );
            assert_eq!(
                hole.custom_covers_fill(corner, owner, &cancel).unwrap(),
                Some(false)
            );
            assert!(scene.diagnostics.is_empty(), "{:?}", scene.diagnostics);
        }
        println!("{name}: imported");
    }
    let empty = root.join("test_107.aedb/edb.def");
    assert!(matches!(
        FormatImporter.import(&empty, &ImportOptions::default(), &context),
        Err(ImportError::NoGeometry)
    ));
}
