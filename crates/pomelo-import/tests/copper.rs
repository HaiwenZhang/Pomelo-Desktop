use pomelo_core::{
    i18n::Locale,
    model::{LayerId, NetId, ObjectId, Point},
    task::CancellationToken,
};

#[path = "copper/kind.rs"]
mod kind;
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::{IndexLimits, RecordKey},
        semantics::{
            CacheLimits,
            connectivity::{NetworkLimits, NetworkMap},
            copper::{CopperDecoder, CopperLimits},
        },
    },
};

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn record(kind: u8, key: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = kind;
    put(&mut bytes, 4, key);
    bytes
}
fn assignment(key: u32, net: u32, first: u32) -> Vec<u8> {
    let mut bytes = record(4, key, 24);
    put(&mut bytes, 12, net);
    put(&mut bytes, 16, first);
    bytes
}
fn shape(key: u32, layer: u16, first: u32, hole: u32, hatch: bool) -> Vec<u8> {
    let mut bytes = record(0x28, key, 76);
    bytes[2..4].copy_from_slice(&layer.to_le_bytes());
    put(&mut bytes, 8, 900);
    put(&mut bytes, 20, if hatch { 2 } else { 0 });
    put(&mut bytes, 36, hole);
    put(&mut bytes, 40, first);
    bytes
}
fn keepout(key: u32, next: u32, first: u32) -> Vec<u8> {
    let mut bytes = record(0x34, key, 36);
    put(&mut bytes, 8, next);
    put(&mut bytes, 24, first);
    bytes
}
fn edge(key: u32, next: u32, a: Point, b: Point) -> Vec<u8> {
    let mut bytes = record(0x16, key, 44);
    for (at, value) in [
        (8, next),
        (24, 200),
        (28, a.x as i32 as u32),
        (32, a.y as i32 as u32),
        (36, b.x as i32 as u32),
        (40, b.y as i32 as u32),
    ] {
        put(&mut bytes, at, value);
    }
    bytes
}
fn square(first: u32, owner: u32, x: f64, y: f64, size: f64) -> Vec<Vec<u8>> {
    let points = [
        Point::new(x, y),
        Point::new(x + size, y),
        Point::new(x + size, y + size),
        Point::new(x, y + size),
    ];
    (0..4)
        .map(|i| {
            edge(
                first + i as u32,
                if i == 3 { owner } else { first + i as u32 + 1 },
                points[i],
                points[(i + 1) % 4],
            )
        })
        .collect()
}
fn rectangle(kind: u8, key: u32, layer: u16) -> Vec<u8> {
    let mut bytes = record(kind, key, if kind == 0x0e { 68 } else { 56 });
    bytes[2..4].copy_from_slice(&layer.to_le_bytes());
    put(&mut bytes, 8, 900);
    let at = if kind == 0x0e { 36 } else { 24 };
    for (i, value) in [1000, 2000, 4000, 6000].into_iter().enumerate() {
        put(&mut bytes, at + 4 * i, value);
    }
    let last = bytes.len() - 4;
    put(&mut bytes, last, 90000);
    bytes
}
fn database(records: &[Vec<u8>]) -> BrdDatabase {
    let mut bytes = vec![0; 0x1200];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    for record in records {
        bytes.extend(record);
    }
    bytes.extend([0; 4]);
    BrdDatabase::read(
        bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context(&CancellationToken::default()),
    )
    .unwrap()
}
fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}
fn networks(db: &BrdDatabase) -> NetworkMap {
    NetworkMap::build(
        db,
        &NetworkLimits::default(),
        &context(&CancellationToken::default()),
    )
    .unwrap()
}

#[test]
fn assigned_computed_copper_keeps_outer_and_hole_coverage_with_analytic_paths() {
    let mut records = vec![
        assignment(900, 0, 10),
        shape(10, 0x0106, 100, 20, false),
        keepout(20, 10, 200),
    ];
    records.extend(square(100, 10, 0.0, 0.0, 10000.0));
    records.extend(square(200, 20, 2000.0, 2000.0, 4000.0));
    let db = database(&records);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    let object = decoder
        .shape(RecordKey(10), &context(&CancellationToken::default()))
        .unwrap();
    let zone = object.zone.unwrap();
    assert_eq!(
        (zone.id, zone.layer, zone.net),
        (ObjectId(10), LayerId(1), NetId(0))
    );
    assert_eq!(
        (
            zone.paths.len(),
            zone.mesh.vertices.len(),
            zone.mesh.outer_count,
            zone.mesh.indices.len()
        ),
        (2, 8, 6, 12)
    );
    assert_eq!(zone.mesh.ring_bounds[0].min, Point::new(0.0, 0.0));
    assert_eq!(zone.mesh.ring_bounds[0].max, Point::new(10.0, 10.0));
    assert!(object.segments.is_empty());
    assert!(decoder.take_diagnostics().is_empty());
}

#[test]
fn unrelated_class_unassigned_shape_and_undefined_shape_layer_are_skipped() {
    let db = database(&[
        assignment(900, 9, 10),
        shape(10, 0x0206, 999, 0, false),
        shape(11, 0x0006, 999, 0, false),
        shape(12, 0x0007, 999, 0, false),
    ]);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    for key in [10, 11, 12] {
        assert!(
            decoder
                .shape(RecordKey(key), &context(&CancellationToken::default()))
                .unwrap()
                .zone
                .is_none()
        );
    }
    assert!(decoder.take_diagnostics().is_empty());
}

#[test]
fn both_rectangle_layouts_rotate_about_the_first_corner() {
    for kind in [0x0e, 0x24] {
        let db = database(&[assignment(900, 9, 10), rectangle(kind, 10, 0x0106)]);
        let nets = networks(&db);
        let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
        let zone = decoder
            .rectangle(RecordKey(10), &context(&CancellationToken::default()))
            .unwrap()
            .unwrap();
        let expected = [
            Point::new(1.0, 2.0),
            Point::new(1.0, 5.0),
            Point::new(-3.0, 5.0),
            Point::new(-3.0, 2.0),
        ];
        for (actual, expected) in zone.mesh.vertices.iter().zip(expected) {
            assert!(actual.distance(expected) < 1e-12);
        }
        assert!(
            zone.paths[0]
                .iter()
                .all(|edge| edge.track_id == ObjectId(10)
                    && edge.net == NetId(9)
                    && edge.layer == LayerId(1))
        );
    }
}

#[test]
fn missing_rectangle_layer_is_a_five_language_error_with_source_identity() {
    let db = database(&[assignment(900, 9, 10), rectangle(0x24, 10, 0xff06)]);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    let error = decoder
        .rectangle(RecordKey(10), &context(&CancellationToken::default()))
        .unwrap_err();
    assert!(matches!(
        error,
        ImportError::CopperLayerUndefined {
            key: 10,
            layer: 255,
            ..
        }
    ));
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.object, Some(ObjectId(10)));
    for locale in Locale::ALL {
        assert!(!diagnostic.message.render(locale).unwrap().contains("%{"));
    }
}

#[test]
fn empty_assigned_boundary_returns_a_localized_warning() {
    let db = database(&[assignment(900, 9, 10), shape(10, 6, 0, 0, false)]);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    assert!(
        decoder
            .shape(RecordKey(10), &context(&CancellationToken::default()))
            .unwrap()
            .zone
            .is_none()
    );
    let diagnostics = decoder.take_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.as_ref(), "BRD_COPPER_BOUNDARY_EMPTY");
    for locale in Locale::ALL {
        assert!(
            !diagnostics[0]
                .message
                .render(locale)
                .unwrap()
                .contains("%{")
        );
    }
}

#[test]
fn hatch_outer_auxiliary_and_hole_paths_become_strokes_instead_of_a_zone() {
    let mut source = shape(10, 0x0106, 100, 20, true);
    put(&mut source, 44, 30);
    let mut hatch = record(0x20, 30, 80);
    put(&mut hatch, 8, 10);
    put(&mut hatch, 12, 110);
    let db = database(&[
        assignment(900, 9, 10),
        source,
        keepout(20, 10, 120),
        hatch,
        edge(100, 10, Point::new(0.0, 0.0), Point::new(1000.0, 0.0)),
        edge(110, 30, Point::new(0.0, 1000.0), Point::new(1000.0, 1000.0)),
        edge(120, 20, Point::new(0.0, 2000.0), Point::new(1000.0, 2000.0)),
    ]);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    let output = decoder
        .shape(RecordKey(10), &context(&CancellationToken::default()))
        .unwrap();
    assert!(output.zone.is_none());
    assert_eq!(
        output.segments.iter().map(|s| s.id.0).collect::<Vec<_>>(),
        [100, 110, 120]
    );
    assert!(
        output
            .segments
            .iter()
            .all(|s| s.track_id == ObjectId(10) && s.layer == LayerId(1) && s.net == NetId(9))
    );
}

#[test]
fn cyclic_and_missing_hatch_links_fail_before_publishing_a_partial_object() {
    for next in [30, 999] {
        let mut source = shape(10, 6, 0, 0, true);
        put(&mut source, 44, 30);
        let mut hatch = record(0x20, 30, 80);
        put(&mut hatch, 8, next);
        let db = database(&[assignment(900, 9, 10), source, hatch]);
        let nets = networks(&db);
        let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
        let error = decoder
            .shape(RecordKey(10), &context(&CancellationToken::default()))
            .unwrap_err();
        assert!(matches!(
            error,
            ImportError::ReferenceCycle { key: 30, .. }
                | ImportError::MissingReference { key: 999, .. }
        ));
    }
}

#[test]
fn outline_does_not_require_network_ownership() {
    let mut records = vec![shape(10, 0xfd01, 100, 0, false)];
    records.extend(square(100, 10, 0.0, 0.0, 1000.0));
    let db = database(&records);
    let nets = networks(&db);
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    let output = decoder
        .shape(RecordKey(10), &context(&CancellationToken::default()))
        .unwrap();
    assert_eq!(output.outline.len(), 4);
    assert!(output.zone.is_none());
}

#[test]
fn mesh_budget_object_budget_and_cancellation_prevent_partial_results() {
    let mut records = vec![assignment(900, 9, 10), shape(10, 6, 100, 0, false)];
    records.extend(square(100, 10, 0.0, 0.0, 1000.0));
    let db = database(&records);
    let nets = networks(&db);
    let mut limits = CopperLimits::default();
    limits.mesh.max_allocation_bytes = 1;
    let mut decoder = CopperDecoder::new(&db, &nets, 2, limits).unwrap();
    assert!(matches!(
        decoder.shape(RecordKey(10), &context(&CancellationToken::default())),
        Err(ImportError::CopperMesh { .. })
    ));
    let limits = CopperLimits {
        objects: CacheLimits {
            max_bytes: 1,
            max_entries: 1,
        },
        ..CopperLimits::default()
    };
    let mut decoder = CopperDecoder::new(&db, &nets, 2, limits).unwrap();
    assert!(
        decoder
            .shape(RecordKey(10), &context(&CancellationToken::default()))
            .is_err()
    );
    let mut decoder = CopperDecoder::new(&db, &nets, 2, CopperLimits::default()).unwrap();
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        decoder.shape(RecordKey(10), &context(&token)),
        Err(ImportError::Cancelled)
    ));
}
