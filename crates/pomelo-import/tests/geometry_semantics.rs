use pomelo_core::{
    geometry::{COPPER_CHORD_TOLERANCE_MM, PathError, arc_sweep, flatten_path},
    i18n::Locale,
    model::{Layer, LayerFunction, LayerId, Point},
    task::CancellationToken,
};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    allegro::{
        database::{BrdDatabase, LocatedRecord, ReferenceLocation},
        decoder::{
            DecodeLimits, DecodedRecord,
            fixed::{Arc, FixedRecord},
        },
        index::{FileOffset, IndexLimits, RecordKey, RecordSpan},
        semantics::{
            geometry::{GeometryDecoder, GeometryLimits, decode_edge},
            layers::{function_from_flags, read_layers},
            units::millimetres_per_unit,
        },
    },
};

const START: usize = 0x1200;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn record(kind: u8, key: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = kind;
    put(&mut bytes, 4, key);
    bytes
}
fn line(kind: u8, key: u32, next: u32, a: (i32, i32), b: (i32, i32)) -> Vec<u8> {
    let mut bytes = record(kind, key, 44);
    for (offset, value) in [
        (8, next),
        (24, 200),
        (28, a.0 as u32),
        (32, a.1 as u32),
        (36, b.0 as u32),
        (40, b.1 as u32),
    ] {
        put(&mut bytes, offset, value);
    }
    bytes
}
fn source(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; START];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes.extend_from_slice(&[0; 4]);
    bytes
}
fn database(bytes: Vec<u8>) -> BrdDatabase {
    let token = CancellationToken::default();
    BrdDatabase::read(
        bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context(&token),
    )
    .unwrap()
}
fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}
fn origin() -> ReferenceLocation {
    ReferenceLocation {
        offset: FileOffset(0x428),
        field: "FirstSegmentPtr",
    }
}
fn arc_record(center_x: f64, center_y: f64, clockwise: bool) -> LocatedRecord {
    LocatedRecord {
        span: RecordSpan {
            offset: FileOffset(START as u32),
            byte_length: 84,
            key: RecordKey(42),
            record_type: 1,
        },
        fields: DecodedRecord::Fixed(FixedRecord::Arc(Arc {
            key: RecordKey(42),
            center_x,
            center_y,
            start_x: 1000,
            start_y: 0,
            end_x: 0,
            end_y: 2000,
            width: 100,
            radius: 123456.0,
            sub_type: if clockwise { 64 } else { 0 },
            ..Arc::default()
        })),
    }
}

#[test]
fn all_five_source_units_convert_to_millimetres_without_guessing() {
    let actual: Vec<_> = (1..=5)
        .map(|units| millimetres_per_unit(units, 1000).unwrap())
        .collect();
    assert_eq!(
        actual,
        vec![0.0254 / 1000.0, 25.4 / 1000.0, 0.001, 0.01, 0.000001]
    );
    assert!(matches!(
        millimetres_per_unit(9, 1000),
        Err(ImportError::UnsupportedUnits { units: 9, .. })
    ));
    assert!(matches!(
        millimetres_per_unit(3, 0),
        Err(ImportError::InvalidDivisor)
    ));
}

#[test]
fn layer_roles_mask_source_flags_and_do_not_infer_from_names() {
    assert_eq!(
        [
            None,
            Some(0x8000),
            Some(0x0100),
            Some(0x4000),
            Some(0xc000),
            Some(0x800f)
        ]
        .map(function_from_flags),
        [
            LayerFunction::Unknown,
            LayerFunction::Conductor,
            LayerFunction::Plane,
            LayerFunction::Dielectric,
            LayerFunction::Unknown,
            LayerFunction::Conductor
        ]
    );
}

#[test]
fn missing_layer_names_change_language_without_changing_source_names() {
    let mut layer = Layer {
        id: LayerId(2),
        name: String::new(),
        function: LayerFunction::Unknown,
        color: "#58b5ed".into(),
        source_flags: None,
    };
    let names: std::collections::BTreeSet<_> = Locale::ALL
        .into_iter()
        .map(|locale| layer.display_name(locale))
        .collect();
    assert_eq!(names.len(), 5);
    layer.name = "原始%{index}".into();
    for locale in Locale::ALL {
        assert_eq!(layer.display_name(locale), "原始%{index}");
    }
}

#[test]
fn layer_map_resolves_source_string_zero_and_preserves_properties_and_color() {
    let mut list = vec![0; 24];
    list[0] = 0x2a;
    list[2] = 1;
    put(&mut list, 8, 0);
    put(&mut list, 12, 0x800f);
    put(&mut list, 20, 77);
    let mut bytes = vec![0; START];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    put(&mut bytes, 0x194, 1);
    put(&mut bytes, 0x428 + 6 * 8 + 4, 77);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(b"TOP\0");
    bytes.extend_from_slice(&list);
    bytes.extend_from_slice(&[0; 4]);
    let database = database(bytes);
    let token = CancellationToken::default();
    let layers = read_layers(&database, &context(&token)).unwrap();
    assert_eq!(
        (
            layers[0].name.as_str(),
            layers[0].function,
            layers[0].source_flags,
            layers[0].color.as_str()
        ),
        ("TOP", LayerFunction::Conductor, Some(0x800f), "#58b5ed")
    );
}

#[test]
fn arcs_use_endpoint_radius_and_hatch_averages_radii_after_ties_even_rounding() {
    let record = arc_record(0.5, -1.5, false);
    let ordinary = decode_edge(&record, 1.0, false).unwrap().arc.unwrap();
    assert_eq!(ordinary.center, Point::new(0.5, -1.5));
    assert_eq!(
        ordinary.radius,
        Point::new(1000.0, 0.0).distance(ordinary.center)
    );
    let hatch = decode_edge(&record, 1.0, true).unwrap().arc.unwrap();
    assert_eq!(hatch.center, Point::new(0.0, -2.0));
    assert_eq!(hatch.radius, ((1000_f64).hypot(2.0) + 2002.0) / 2.0);
    assert!(hatch.sweep > 0.0);
    assert!(
        decode_edge(&arc_record(0.0, 0.0, true), 1.0, false)
            .unwrap()
            .arc
            .unwrap()
            .sweep
            < 0.0
    );
}

#[test]
fn equal_and_wrapped_end_angles_produce_full_circles_in_both_directions() {
    use std::f64::consts::{PI, TAU};
    assert_eq!(
        [
            arc_sweep(0.0, 0.0, false),
            arc_sweep(0.0, 0.0, true),
            arc_sweep(-PI, PI, true),
            arc_sweep(PI, -PI, false)
        ],
        [TAU, -TAU, -TAU, TAU]
    );
}

#[test]
fn all_line_variants_keep_signed_coordinates_and_closed_chains_stop_once() {
    let database = database(source(&[
        line(0x15, 1, 2, (-1000, 0), (0, 1000)),
        line(0x16, 2, 3, (0, 1000), (1000, 0)),
        line(0x17, 3, 1, (1000, 0), (-1000, 0)),
    ]));
    let token = CancellationToken::default();
    let path = GeometryDecoder::new(&database, GeometryLimits::default())
        .unwrap()
        .read_path(RecordKey(1), false, None, origin(), &context(&token))
        .unwrap();
    assert_eq!(
        path.iter()
            .map(|edge| (edge.id.0, edge.a.x, edge.width, edge.layer))
            .collect::<Vec<_>>(),
        [
            (1, -1.0, 0.2, LayerId::UNASSIGNED),
            (2, 0.0, 0.2, LayerId::UNASSIGNED),
            (3, 1.0, 0.2, LayerId::UNASSIGNED)
        ]
    );
}

#[test]
fn owner_links_and_non_edge_records_terminate_before_decoding() {
    let database = database(source(&[
        line(0x15, 1, 2, (0, 0), (1000, 0)),
        record(4, 2, 24),
    ]));
    let token = CancellationToken::default();
    let decoder = GeometryDecoder::new(&database, GeometryLimits::default()).unwrap();
    assert_eq!(
        decoder
            .read_path(
                RecordKey(1),
                false,
                Some(RecordKey(2)),
                origin(),
                &context(&token)
            )
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        decoder
            .read_path(RecordKey(1), false, None, origin(), &context(&token))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn non_closing_cycles_and_missing_links_report_the_referring_record() {
    let database = database(source(&[
        line(0x15, 1, 2, (0, 0), (1000, 0)),
        line(0x15, 2, 3, (0, 0), (1000, 0)),
        line(0x15, 3, 2, (0, 0), (1000, 0)),
    ]));
    let token = CancellationToken::default();
    let error = GeometryDecoder::new(&database, GeometryLimits::default())
        .unwrap()
        .read_path(RecordKey(1), false, None, origin(), &context(&token))
        .unwrap_err();
    assert!(
        matches!(error, ImportError::ReferenceCycle { key: 2, offset, .. } if offset == START+88)
    );
    let error = GeometryDecoder::new(&database, GeometryLimits::default())
        .unwrap()
        .read_path(RecordKey(99), false, None, origin(), &context(&token))
        .unwrap_err();
    assert!(matches!(
        error,
        ImportError::MissingReference {
            key: 99,
            offset: 0x428,
            ..
        }
    ));
}

#[test]
fn shape_outer_and_hole_contours_keep_analytic_paths_and_remove_closing_points() {
    let mut shape = record(0x28, 100, 76);
    put(&mut shape, 36, 200);
    put(&mut shape, 40, 1);
    let mut hole = record(0x34, 200, 36);
    put(&mut hole, 8, 100);
    put(&mut hole, 24, 10);
    let database = database(source(&[
        shape,
        hole,
        line(0x15, 1, 2, (0, 0), (10000, 0)),
        line(0x15, 2, 3, (10000, 0), (10000, 10000)),
        line(0x15, 3, 4, (10000, 10000), (0, 10000)),
        line(0x15, 4, 100, (0, 10000), (0, 0)),
        line(0x15, 10, 11, (3000, 3000), (7000, 3000)),
        line(0x15, 11, 12, (7000, 3000), (7000, 7000)),
        line(0x15, 12, 13, (7000, 7000), (3000, 7000)),
        line(0x15, 13, 200, (3000, 7000), (3000, 3000)),
    ]));
    let token = CancellationToken::default();
    let contours = GeometryDecoder::new(&database, GeometryLimits::default())
        .unwrap()
        .read_contours(RecordKey(100), &context(&token))
        .unwrap();
    assert_eq!(
        contours.paths.iter().map(Vec::len).collect::<Vec<_>>(),
        [4, 4]
    );
    assert_eq!(
        contours.rings,
        [
            vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0)
            ],
            vec![
                Point::new(3.0, 3.0),
                Point::new(7.0, 3.0),
                Point::new(7.0, 7.0),
                Point::new(3.0, 7.0)
            ]
        ]
    );
}

#[test]
fn copper_arc_subdivision_obeys_chord_tolerance_and_retains_stored_endpoint() {
    let edge = decode_edge(&arc_record(0.0, 0.0, false), 0.001, false).unwrap();
    let token = CancellationToken::default();
    let points = flatten_path(
        std::slice::from_ref(&edge),
        COPPER_CHORD_TOLERANCE_MM,
        1000,
        &token,
    )
    .unwrap();
    assert_eq!(
        (*points.first().unwrap(), *points.last().unwrap()),
        (edge.a, edge.b)
    );
    let arc = edge.arc.unwrap();
    for pair in points[1..points.len() - 1].windows(2) {
        let chord = pair[0].distance(pair[1]);
        assert!(
            arc.radius - (arc.radius.powi(2) - (chord / 2.0).powi(2)).sqrt()
                <= COPPER_CHORD_TOLERANCE_MM + 1e-12
        );
    }
}

#[test]
fn geometry_limits_and_cancellation_reject_work_before_publication() {
    let database = database(source(&[line(0x15, 1, 0, (0, 0), (1000, 0))]));
    let token = CancellationToken::default();
    let decoder = GeometryDecoder::new(
        &database,
        GeometryLimits {
            max_allocation_bytes: 0,
            ..GeometryLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        decoder.read_path(RecordKey(1), false, None, origin(), &context(&token)),
        Err(ImportError::GeometryLimit { .. })
    ));
    let edge = decode_edge(&arc_record(0.0, 0.0, false), 0.001, false).unwrap();
    assert!(matches!(
        flatten_path(
            std::slice::from_ref(&edge),
            COPPER_CHORD_TOLERANCE_MM,
            2,
            &token
        ),
        Err(PathError::PointLimit { .. })
    ));
    assert!(matches!(
        decode_edge(&arc_record(f64::NAN, 0.0, false), 0.001, false),
        Err(ImportError::InvalidGeometry { .. })
    ));
    token.cancel();
    assert!(matches!(
        decoder.read_path(RecordKey(0), false, None, origin(), &context(&token)),
        Err(ImportError::Cancelled)
    ));
    assert!(matches!(
        flatten_path(&[edge], COPPER_CHORD_TOLERANCE_MM, 1000, &token),
        Err(PathError::Cancelled)
    ));
}

#[test]
fn empty_hole_cycles_and_hole_count_limits_are_not_silently_discarded() {
    let mut shape = record(0x28, 100, 76);
    put(&mut shape, 36, 200);
    let mut hole = record(0x34, 200, 36);
    put(&mut hole, 8, 200);
    let database = database(source(&[shape, hole]));
    let token = CancellationToken::default();
    let decoder = GeometryDecoder::new(&database, GeometryLimits::default()).unwrap();
    assert!(matches!(
        decoder.read_contours(RecordKey(100), &context(&token)),
        Err(ImportError::ReferenceCycle { key: 200, .. })
    ));
    let decoder = GeometryDecoder::new(
        &database,
        GeometryLimits {
            max_paths: 1,
            ..GeometryLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        decoder.read_contours(RecordKey(100), &context(&token)),
        Err(ImportError::InvalidRecord {
            field: "GEOMETRY_PATH_COUNT",
            ..
        })
    ));
}

#[test]
fn geometry_edge_budget_is_checked_before_following_long_chains() {
    let database = database(source(&[
        line(0x15, 1, 2, (0, 0), (1000, 0)),
        line(0x15, 2, 0, (1000, 0), (2000, 0)),
    ]));
    let token = CancellationToken::default();
    let decoder = GeometryDecoder::new(
        &database,
        GeometryLimits {
            max_edges: 1,
            ..GeometryLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        decoder.read_path(RecordKey(1), false, None, origin(), &context(&token)),
        Err(ImportError::InvalidRecord {
            field: "GEOMETRY_EDGE_COUNT",
            ..
        })
    ));
}

#[test]
fn indexed_offset_lookup_can_decode_zero_key_records_but_not_interior_bytes() {
    let database = database(source(&[line(0x15, 0, 0, (0, 0), (1000, 0))]));
    let token = CancellationToken::default();
    assert!(
        database
            .get(RecordKey(0), &context(&token))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        database
            .get_at_offset(FileOffset(START as u32), &context(&token))
            .unwrap()
            .unwrap()
            .span
            .key,
        RecordKey(0)
    );
    assert!(
        database
            .get_at_offset(FileOffset(START as u32 + 4), &context(&token))
            .unwrap()
            .is_none()
    );
}

#[test]
fn non_hole_terminators_do_not_allocate_or_decode_unrelated_variable_payloads() {
    let mut shape = record(0x28, 100, 76);
    put(&mut shape, 36, 77);
    let mut layer = vec![0; 24];
    layer[0] = 0x2a;
    layer[2] = 1;
    put(&mut layer, 20, 77);
    let token = CancellationToken::default();
    let database = BrdDatabase::read(
        source(&[shape, layer]),
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits {
            max_allocation_bytes: 0,
            ..DecodeLimits::default()
        },
        &context(&token),
    )
    .unwrap();
    assert!(matches!(
        database.get(RecordKey(77), &context(&token)),
        Err(ImportError::DecodeLimit { .. })
    ));
    let decoder = GeometryDecoder::new(&database, GeometryLimits::default()).unwrap();
    assert!(
        decoder
            .read_shape_paths(RecordKey(100), &context(&token))
            .unwrap()
            .is_empty()
    );
}
