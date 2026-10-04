use pomelo_core::{
    i18n::Locale,
    model::{LayerId, ObjectId, Point, TextAlignment},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportError, ImportOptions,
    allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::IndexLimits,
        semantics::{
            CacheLimits,
            drawing::{DrawingBuilder, DrawingLimits},
            layers::drawing_layer,
            scene::{SceneBuilder, SceneLimits},
            text::{TextBuilder, TextLimits},
        },
    },
};

#[test]
fn importer_reads_real_file_to_shared_scene_and_attaches_source_paths_to_warnings() {
    let db = db(
        vec![
            track(2, 3),
            edge(3, 2),
            font(1, 1000, 500),
            font(4, 1000, 500),
            wrapper(10, 999, 0x0006, 12345, 1),
        ],
        10,
        0,
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("电路板 日本語 한국어.brd");
    std::fs::write(&path, db.source_bytes()).unwrap();
    let importer = pomelo_import::allegro::AllegroImporter;
    let board = importer
        .import(
            &path,
            &ImportOptions::default(),
            &context(&CancellationToken::default()),
        )
        .unwrap();
    assert!(board.source.scene_supported);
    assert_eq!(board.source.layout_version, 174);
    assert_eq!(board.scene.segments.len(), 1);
    assert_eq!(board.scene.diagnostics.len(), 2);
    assert!(
        board
            .scene
            .diagnostics
            .iter()
            .any(|d| d.code.as_ref() == "BRD_TEXT_CONTENT_MISSING")
    );
    for diagnostic in &board.scene.diagnostics {
        assert_eq!(diagnostic.path.as_deref(), Some(path.as_path()));
    }
    assert!(std::sync::Arc::ptr_eq(
        board.scene.diagnostics[0].path.as_ref().unwrap(),
        board.scene.diagnostics[1].path.as_ref().unwrap()
    ));
    assert_eq!(std::fs::read(&path).unwrap(), db.source_bytes());
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        importer.import(&path, &ImportOptions::default(), &context(&token)),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn scene_cli_journal_has_source_identity_all_rows_and_a_complete_marker_without_gpu_claims() {
    use sha2::{Digest, Sha256};
    let db = db(vec![track(2, 3), edge(3, 2)], 0, 0);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.brd");
    let report = dir.path().join("scene.jsonl");
    std::fs::write(&path, db.source_bytes()).unwrap();
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", "ja", "decode-scene"])
        .arg(&path)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = std::fs::read_to_string(&report).unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "metadata");
    assert_eq!(
        rows[0]["sha256"],
        format!("{:x}", Sha256::digest(db.source_bytes()))
    );
    assert_eq!(rows[0]["scene_validated"], false);
    assert_eq!(rows[0]["counts"]["segments"], 1);
    assert_eq!(
        rows.iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["metadata", "layers", "segments", "bounds", "complete"]
    );
    assert_eq!(rows.last().unwrap()["data"], rows[0]["counts"]);
    assert_eq!(std::fs::read(&path).unwrap(), db.source_bytes());
}

fn put(b: &mut [u8], at: usize, value: u32) {
    b[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn record(kind: u8, key: u32, size: usize) -> Vec<u8> {
    let mut b = vec![0; size];
    b[0] = kind;
    put(&mut b, 4, key);
    b
}
fn font(key: u32, height: u32, width: u32) -> Vec<u8> {
    let mut b = record(0x36, key, 104);
    b[2..4].copy_from_slice(&8u16.to_le_bytes());
    put(&mut b, 16, 1);
    put(&mut b, 20, 1);
    for (at, value) in [
        (44, height),
        (48, width),
        (52, if height == 0 { 0 } else { 30 }),
        (56, if height == 0 { 0 } else { 40 }),
        (68, if height == 0 { 0 } else { 10 }),
    ] {
        put(&mut b, at, value);
    }
    b
}
fn wrapper(key: u32, next: u32, layer: u16, graphic: u32, props: u32) -> Vec<u8> {
    let mut b = record(0x30, key, 60);
    b[2..4].copy_from_slice(&layer.to_le_bytes());
    for (at, value) in [
        (8, next),
        (20, props),
        (32, graphic),
        (44, (-3000i32) as u32),
        (48, 2000),
        (56, 90000),
    ] {
        put(&mut b, at, value);
    }
    b
}
fn text(key: u32, value: &str) -> Vec<u8> {
    let n = value.len() + 1;
    let mut b = record(0x31, key, 28 + n.next_multiple_of(4));
    b[22..24].copy_from_slice(&(n as u16).to_le_bytes());
    b[28..28 + value.len()].copy_from_slice(value.as_bytes());
    b
}
fn footprint(key: u32, text: u32, graphic: u32) -> Vec<u8> {
    let mut b = record(0x2d, key, 72);
    b[2] = 1;
    for (at, value) in [
        (28, 180000),
        (32, 100000),
        (36, 200000),
        (44, graphic),
        (52, text),
    ] {
        put(&mut b, at, value);
    }
    b
}
fn graphic(key: u32, next: u32, parent: u32, first: u32) -> Vec<u8> {
    let mut b = record(0x14, key, 36);
    b[2..4].copy_from_slice(&0xf901u16.to_le_bytes());
    for (at, value) in [(8, next), (12, parent), (24, first)] {
        put(&mut b, at, value);
    }
    b
}
fn edge(key: u32, next: u32) -> Vec<u8> {
    let mut b = record(0x16, key, 44);
    for (at, value) in [
        (8, next),
        (24, 100),
        (28, 1000),
        (32, 2000),
        (36, 3000),
        (40, 4000),
    ] {
        put(&mut b, at, value);
    }
    b
}
fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}
fn db(records: Vec<Vec<u8>>, text_head: u32, graphic_head: u32) -> BrdDatabase {
    let mut b = vec![0; 0x1200];
    for (at, value) in [
        (0, 0x140900),
        (0x26c, 1000),
        (0x8c, 999),
        (0x90, text_head),
        (0x5c, 888),
        (0x60, graphic_head),
    ] {
        put(&mut b, at, value);
    }
    b[0x180] = 3;
    put(&mut b, 0x428 + 6 * 8 + 4, 9_000_000);
    for r in records {
        b.extend(r);
    }
    let mut layers = vec![0; 24];
    layers[0] = 0x2a;
    layers[2] = 1;
    put(&mut layers, 12, 0x8000);
    put(&mut layers, 20, 9_000_000);
    b.extend(layers);
    b.extend([0; 4]);
    BrdDatabase::read(
        b,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context(&CancellationToken::default()),
    )
    .unwrap()
}

fn track(key: u32, first: u32) -> Vec<u8> {
    let mut b = record(5, key, 68);
    b[2..4].copy_from_slice(&6u16.to_le_bytes());
    put(&mut b, 56, first);
    b
}

#[test]
fn complete_scene_combines_tracks_text_and_dimension_geometry_without_expanding_bounds_for_annotations()
 {
    let db = db(
        vec![
            font(1, 1000, 500),
            track(2, 3),
            edge(3, 2),
            wrapper(10, 999, 0xf901, 20, 1),
            text(20, "10 mm"),
            graphic(30, 888, 888, 31),
            edge(31, 30),
        ],
        10,
        30,
    );
    let scene = SceneBuilder::new(&db, SceneLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(
        (
            scene.layers.len(),
            scene.segments.len(),
            scene.texts.len(),
            scene.drawings.len(),
            scene.drawing_layers.len()
        ),
        (1, 1, 1, 1, 1)
    );
    assert_eq!(scene.bounds.min, Point::new(0.95, 1.95));
    assert_eq!(scene.bounds.max, Point::new(3.05, 4.05));
    assert_eq!(scene.drawing_layers[0].id, LayerId::DIMENSION);
    assert_eq!(scene.texts[0].text, "10 mm");
    assert!(scene.special_layers.is_empty());
}

#[test]
fn annotation_only_board_returns_localized_no_geometry_error() {
    let db = db(
        vec![
            font(1, 1000, 500),
            wrapper(10, 999, 0xf901, 20, 1),
            text(20, "10 mm"),
        ],
        10,
        0,
    );
    let error = SceneBuilder::new(&db, SceneLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap_err();
    assert!(matches!(error, ImportError::NoGeometry));
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.code.as_ref(), "BRD_NO_GEOMETRY");
    for locale in Locale::ALL {
        assert!(diagnostic.message.render(locale).is_ok());
    }
}

#[test]
fn graphic_board_outline_alone_supplies_fit_bounds_including_stroke_width() {
    let mut outline = graphic(30, 888, 888, 31);
    outline[2..4].copy_from_slice(&0xea01u16.to_le_bytes());
    let db = db(vec![outline, edge(31, 30)], 0, 30);
    let scene = SceneBuilder::new(&db, SceneLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(scene.outline.len(), 1);
    assert_eq!(scene.bounds.min, Point::new(0.95, 1.95));
    assert_eq!(scene.bounds.max, Point::new(3.05, 4.05));
}

#[test]
fn stored_board_outline_supplies_fit_bounds_including_stroke_width() {
    let mut outline = record(0x28, 30, 76);
    outline[2..4].copy_from_slice(&0xfd01u16.to_le_bytes());
    put(&mut outline, 40, 31);
    let db = db(vec![outline, edge(31, 30)], 0, 0);
    let scene = SceneBuilder::new(&db, SceneLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(scene.outline.len(), 1);
    assert_eq!(scene.bounds.min, Point::new(0.95, 1.95));
    assert_eq!(scene.bounds.max, Point::new(3.05, 4.05));
}

#[test]
fn scene_output_object_budgets_and_cancellation_fail_before_publication_and_allow_fresh_retry() {
    let db = db(vec![track(2, 3), edge(3, 2)], 0, 0);
    let limits = SceneLimits {
        max_output_bytes: 1,
        ..SceneLimits::default()
    };
    assert!(matches!(
        SceneBuilder::new(&db, limits).build(&context(&CancellationToken::default())),
        Err(ImportError::GeometryLimit { .. })
    ));
    let limits = SceneLimits {
        max_objects: 1,
        ..SceneLimits::default()
    };
    assert!(matches!(
        SceneBuilder::new(&db, limits).build(&context(&CancellationToken::default())),
        Err(ImportError::InvalidRecord {
            field: "SCENE_OBJECT_COUNT",
            ..
        })
    ));
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        SceneBuilder::new(&db, SceneLimits::default()).build(&context(&token)),
        Err(ImportError::Cancelled)
    ));
    assert_eq!(
        SceneBuilder::new(&db, SceneLimits::default())
            .build(&context(&CancellationToken::default()))
            .unwrap()
            .segments
            .len(),
        1
    );
}

#[test]
fn placed_text_preserves_source_strings_signed_absolute_coordinates_and_font_properties() {
    let db = db(
        vec![
            font(1, 1000, 500),
            wrapper(10, 100, 0xfb0d, 20, 0x03030001),
            text(20, "R1 中文 日本語 한국어"),
            footprint(100, 10, 0),
            wrapper(11, 0, 0xfb0d, 21, 1),
            text(21, "LIBRARY"),
        ],
        10,
        0,
    );
    let output = TextBuilder::new(&db, TextLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(output.texts.len(), 1);
    let t = &output.texts[0];
    assert_eq!(
        (t.id, t.owner_id, t.at, t.align, t.mirrored),
        (
            ObjectId(10),
            Some(ObjectId(100)),
            Point::new(-3.0, 2.0),
            TextAlignment::Center,
            true
        )
    );
    assert_eq!(
        (t.width, t.height, t.spacing, t.line_spacing, t.stroke_width),
        (0.5, 1.0, 0.03, 0.04, 0.01)
    );
    assert!((t.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
    assert_eq!(t.text, "R1 中文 日本語 한국어");
    assert_eq!(output.drawing_layers[0].id, LayerId(0x1fb0d));
    assert!(output.drawing_layers[0].default_visible);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| d.code.as_ref() == "BRD_TEXT_LINK_TYPE")
    );
}
#[test]
fn zero_size_font_is_valid_but_drc_and_unplaced_library_text_are_excluded() {
    let db = db(
        vec![
            font(1, 0, 0),
            wrapper(10, 11, 0x0106, 20, 1),
            text(20, "Zero"),
            wrapper(11, 999, 0x0105, 21, 1),
            text(21, "DRC"),
            wrapper(12, 0, 0x0106, 22, 1),
            text(22, "LIB"),
        ],
        10,
        0,
    );
    let o = TextBuilder::new(&db, TextLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(
        o.texts
            .iter()
            .map(|t| (&*t.text, t.height))
            .collect::<Vec<_>>(),
        vec![("Zero", 0.0)]
    );
    assert!(o.drawing_layers.is_empty());
    assert!(o.diagnostics.is_empty());
}
#[test]
fn text_warnings_keep_codes_positions_and_render_in_five_languages() {
    let db = db(
        vec![
            font(1, 1000, 0),
            font(2, 1000, 500),
            wrapper(10, 11, 0x0106, 20, 1),
            text(20, "Bad font"),
            wrapper(11, 12, 0x0106, 9999, 1),
        ],
        10,
        0,
    );
    let o = TextBuilder::new(&db, TextLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    assert!(o.texts.is_empty());
    for code in [
        "BRD_TEXT_FONT_TABLES_MULTIPLE",
        "BRD_TEXT_LINK_MISSING",
        "BRD_TEXT_FONT_INVALID",
        "BRD_TEXT_CONTENT_MISSING",
    ] {
        assert!(
            o.diagnostics.iter().any(|d| d.code.as_ref() == code),
            "{code}"
        );
    }
    for d in &o.diagnostics {
        for locale in Locale::ALL {
            let m = d.message.render(locale).unwrap();
            assert!(!m.contains("%{"));
            assert_ne!(m, d.message.key.as_str());
        }
    }
}
#[test]
fn text_chain_cycles_and_resource_limits_fail_without_publishing_partial_text() {
    let db = db(
        vec![
            font(1, 100, 50),
            wrapper(10, 10, 0x0106, 20, 1),
            text(20, "Cycle"),
        ],
        10,
        0,
    );
    assert!(matches!(
        TextBuilder::new(&db, TextLimits::default()).build(&context(&CancellationToken::default())),
        Err(ImportError::ReferenceCycle { key: 10, .. })
    ));
    let limits = TextLimits {
        objects: CacheLimits {
            max_bytes: 1,
            max_entries: 100,
        },
        ..TextLimits::default()
    };
    assert!(matches!(
        TextBuilder::new(&db, limits).build(&context(&CancellationToken::default())),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        TextBuilder::new(&db, TextLimits::default()).build(&context(&token)),
        Err(ImportError::Cancelled)
    ));
}
#[test]
fn drawing_layers_use_localized_reserved_labels_and_preserve_custom_names() {
    let top = drawing_layer(0xfb0d, "Ignored".into());
    let custom = drawing_layer(0x0101, "User %{class} 中文".into());
    let unknown = drawing_layer(0x017f, String::new());
    assert!(top.source_name.is_empty());
    assert!(top.default_visible);
    for locale in Locale::ALL {
        assert!(!top.display_name(locale).contains("Ignored"));
        assert!(custom.display_name(locale).ends_with("User %{class} 中文"));
        assert!(!unknown.display_name(locale).contains("%{"));
    }
    assert_eq!(
        top.display_name(Locale::SimplifiedChinese),
        "位号 · 顶面丝印"
    );
}
#[test]
fn dimension_graphics_follow_header_membership_and_placed_owner_chains() {
    let db = db(
        vec![
            graphic(10, 888, 888, 20),
            edge(20, 20),
            graphic(11, 100, 100, 21),
            edge(21, 11),
            footprint(100, 0, 11),
        ],
        0,
        10,
    );
    let o = DrawingBuilder::new(&db, DrawingLimits::default())
        .build(&[], &context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(
        o.drawings
            .iter()
            .map(|d| (d.id, d.owner_id, d.graphic_ids.clone()))
            .collect::<Vec<_>>(),
        vec![
            (ObjectId(10), None, vec![ObjectId(10)]),
            (ObjectId(100), Some(ObjectId(100)), vec![ObjectId(11)])
        ]
    );
    assert_eq!(o.drawings[1].segments[0].a, Point::new(1.0, 2.0));
    assert_eq!(o.drawings[1].segments[0].layer, LayerId::DIMENSION);
    assert!(o.diagnostics.is_empty());
}
#[test]
fn dimension_text_only_owner_group_keeps_references_into_scene_texts() {
    let db = db(
        vec![
            font(1, 1000, 500),
            wrapper(10, 100, 0xf901, 20, 1),
            text(20, "10 mm"),
            footprint(100, 10, 0),
        ],
        0,
        0,
    );
    let texts = TextBuilder::new(&db, TextLimits::default())
        .build(&context(&CancellationToken::default()))
        .unwrap();
    let drawings = DrawingBuilder::new(&db, DrawingLimits::default())
        .build(&texts.texts, &context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(drawings.drawings.len(), 1);
    assert_eq!(drawings.drawings[0].text_ids, vec![ObjectId(10)]);
    assert!(drawings.drawings[0].segments.is_empty());
}
#[test]
fn dimension_missing_path_and_orphan_are_localized_warnings() {
    let db = db(
        vec![graphic(10, 888, 888, 9999), graphic(11, 0, 7777, 0)],
        0,
        10,
    );
    let o = DrawingBuilder::new(&db, DrawingLimits::default())
        .build(&[], &context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(o.drawings.len(), 1);
    assert_eq!(
        o.diagnostics
            .iter()
            .map(|d| d.code.as_ref())
            .collect::<Vec<_>>(),
        vec!["BRD_DRAWING_PATH_MISSING", "BRD_DRAWING_ORPHAN"]
    );
    for d in &o.diagnostics {
        for l in Locale::ALL {
            assert!(d.message.render(l).is_ok());
        }
    }
}
#[test]
fn non_initial_path_cycles_and_owner_cycles_are_fatal() {
    let db = db(
        vec![graphic(10, 888, 888, 20), edge(20, 21), edge(21, 21)],
        0,
        10,
    );
    assert!(matches!(
        DrawingBuilder::new(&db, DrawingLimits::default())
            .build(&[], &context(&CancellationToken::default())),
        Err(ImportError::ReferenceCycle { key: 21, .. })
    ));
    let db = db_fn_owner_cycle();
    assert!(matches!(
        DrawingBuilder::new(&db, DrawingLimits::default())
            .build(&[], &context(&CancellationToken::default())),
        Err(ImportError::ReferenceCycle { key: 10, .. })
    ));
}
fn db_fn_owner_cycle() -> BrdDatabase {
    db(vec![graphic(10, 10, 888, 0)], 0, 10)
}
#[test]
fn drawings_respect_object_chain_budgets_and_cancellation() {
    let db = db(
        vec![graphic(10, 888, 888, 20), edge(20, 21), edge(21, 10)],
        0,
        10,
    );
    let mut limits = DrawingLimits::default();
    limits.chain.max_records = 1;
    assert!(matches!(
        DrawingBuilder::new(&db, limits).build(&[], &context(&CancellationToken::default())),
        Err(ImportError::InvalidRecord {
            field: "CHAIN_RECORD_COUNT",
            ..
        })
    ));
    let limits = DrawingLimits {
        objects: CacheLimits {
            max_bytes: 1,
            max_entries: 100,
        },
        ..DrawingLimits::default()
    };
    assert!(matches!(
        DrawingBuilder::new(&db, limits).build(&[], &context(&CancellationToken::default())),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        DrawingBuilder::new(&db, DrawingLimits::default()).build(&[], &context(&token)),
        Err(ImportError::Cancelled)
    ));
}
