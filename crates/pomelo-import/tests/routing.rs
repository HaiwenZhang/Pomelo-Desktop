use pomelo_core::{
    i18n::Locale,
    model::{LayerId, NetId, ObjectId, Point},
    task::CancellationToken,
};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::{IndexLimits, RecordKey},
        semantics::{
            connectivity::{NetworkLimits, NetworkMap},
            placement::{PlacementDecoder, PlacementLimits},
        },
    },
};
use std::sync::Arc;

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn record(kind: u8, key: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = kind;
    put(&mut bytes, 4, key);
    bytes
}
fn stack(key: u32, start: u8, layers: u16, kind: u8, plated: bool, drill: u32) -> Vec<u8> {
    let mut bytes = record(0x1c, key, 192 + (21 + usize::from(layers) * 4) * 36);
    bytes[3] = start;
    bytes[28] = kind;
    bytes[30] = if plated { 0x20 } else { 0 };
    bytes[44..46].copy_from_slice(&layers.to_le_bytes());
    put(&mut bytes, 64, drill);
    put(&mut bytes, 108, (-600_i32) as u32);
    for index in 0..layers {
        let at = 192 + (23 + usize::from(index) * 4) * 36;
        bytes[at] = 6;
        for (offset, value) in [
            (8, 400 + u32::from(index) * 100),
            (12, 300),
            (20, 100),
            (24, 200),
        ] {
            put(&mut bytes, at + offset, value);
        }
    }
    for (index, size) in [(5, 500), (14, 900)] {
        let at = 192 + index * 36;
        bytes[at] = 2;
        put(&mut bytes, at + 8, size);
        put(&mut bytes, at + 12, size);
    }
    bytes
}
fn via(key: u32, stack: u32, bits: u16) -> Vec<u8> {
    let mut bytes = record(0x33, key, 80);
    bytes[2..4].copy_from_slice(&bits.to_le_bytes());
    for (at, value) in [(12, 999), (32, 1000), (36, 2000), (44, stack)] {
        put(&mut bytes, at, value);
    }
    bytes
}
fn assignment(key: u32, net: u32, first: u32) -> Vec<u8> {
    let mut bytes = record(4, key, 24);
    put(&mut bytes, 12, net);
    put(&mut bytes, 16, first);
    bytes
}
fn track(key: u32, layer: u16, first: u32) -> Vec<u8> {
    let mut bytes = record(5, key, 68);
    bytes[2..4].copy_from_slice(&layer.to_le_bytes());
    put(&mut bytes, 56, first);
    bytes
}
fn edge(key: u32, next: u32, owner: u32) -> Vec<u8> {
    let mut bytes = record(0x16, key, 44);
    for (at, value) in [
        (8, next),
        (12, owner),
        (16, 160),
        (24, 50),
        (28, 11000),
        (32, 22000),
        (36, 15000),
        (40, 26000),
    ] {
        put(&mut bytes, at, value);
    }
    bytes
}
fn field(key: u32, next: u32, header: u16, value: &str) -> Vec<u8> {
    let length = (value.len() + 1).next_multiple_of(4);
    let mut bytes = record(3, key, 24 + length);
    bytes[2..4].copy_from_slice(&header.to_le_bytes());
    put(&mut bytes, 8, next);
    bytes[16] = 104;
    bytes[18..20].copy_from_slice(&(length as u16).to_le_bytes());
    bytes[24..24 + value.len()].copy_from_slice(value.as_bytes());
    bytes
}
fn wrapper(key: u32, layer: u16, words: [u32; 6]) -> Vec<u8> {
    let mut bytes = record(0x2f, key, 32);
    bytes[2..4].copy_from_slice(&layer.to_le_bytes());
    for (i, word) in words.into_iter().enumerate() {
        put(&mut bytes, 8 + 4 * i, word);
    }
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
        &ImportContext {
            cancellation: &CancellationToken::default(),
            progress: &|_| {},
        },
    )
    .unwrap()
}
fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}
fn wire_records() -> Vec<Vec<u8>> {
    let mut fp = record(0x2d, 1, 72);
    put(&mut fp, 32, 10000);
    put(&mut fp, 36, 20000);
    put(&mut fp, 48, 4);
    let mut pad = record(0x0d, 6, 48);
    put(&mut pad, 20, 1000);
    put(&mut pad, 24, 2000);
    put(&mut pad, 28, 20);
    let mut pin = record(0x32, 4, 84);
    for (at, value) in [(8, 90), (12, 90), (24, 1), (28, 1), (36, 6)] {
        put(&mut pin, at, value);
    }
    let mut finger = via(5, 3, 0xc012);
    for (at, value) in [
        (8, 4),
        (12, 90),
        (28, 1),
        (32, 15000),
        (36, 26000),
        (60, 90000),
    ] {
        put(&mut finger, at, value);
    }
    let mut wire = track(10, 0xfd06, 7);
    for (at, value) in [
        (8, 5),
        (12, 90),
        (16, 160),
        (28, 1),
        (36, 4),
        (48, 5),
        (60, 8),
    ] {
        put(&mut wire, at, value);
    }
    vec![
        fp,
        stack(2, 0, 1, 26, false, 0),
        stack(3, 0, 1, 30, false, 0),
        pad,
        pin,
        finger,
        wire,
        edge(7, 10, 10),
        field(8, 9, 400, "TOP"),
        field(9, 10, 540, "Au"),
        wrapper(20, 0xfc00, [2, 4, 1, 0, 0, 8]),
        assignment(90, 77, 10),
    ]
}
fn alter(records: &mut [Vec<u8>], key: u32, at: usize, value: u32) {
    let bytes = records
        .iter_mut()
        .find(|r| u32::from_le_bytes(r[4..8].try_into().unwrap()) == key)
        .unwrap();
    put(bytes, at, value);
}

#[test]
fn via_reverse_and_flip_have_distinct_order_and_preserve_stack_recipe() {
    let db = database(&[
        stack(100, 1, 2, 4, true, 200),
        via(1, 100, 0),
        via(2, 100, 0x3000),
        via(3, 100, 0x2000),
        via(4, 100, 0),
    ]);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    let normal = decoder.via(RecordKey(1), &ctx).unwrap().unwrap();
    let reverse = decoder.via(RecordKey(2), &ctx).unwrap().unwrap();
    let flip = decoder.via(RecordKey(3), &ctx).unwrap().unwrap();
    let repeated = decoder.via(RecordKey(4), &ctx).unwrap().unwrap();
    let layers = |v: &pomelo_core::model::Via| v.pads.iter().map(|p| p.layer.0).collect::<Vec<_>>();
    assert_eq!(
        (layers(&normal), layers(&reverse), layers(&flip)),
        (vec![1, 2], vec![4, 3], vec![3, 4])
    );
    assert_eq!(normal.at, Point::new(1.0, 2.0));
    assert_eq!(normal.net, NetId(0));
    assert!(Arc::ptr_eq(&normal.pads, &repeated.pads));
    assert!(!Arc::ptr_eq(&normal.pads, &reverse.pads));
    assert_eq!(reverse.pads[0].width, normal.pads[0].width);
    token.cancel();
    assert!(matches!(
        decoder.via(RecordKey(1), &ctx),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn via_net_comes_from_ownership_instead_of_its_direct_pointer() {
    let mut source = via(1, 100, 0);
    put(&mut source, 8, 90);
    let db = database(&[stack(100, 0, 1, 4, true, 200), source, assignment(90, 9, 1)]);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    assert_eq!(
        PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default())
            .unwrap()
            .via(RecordKey(1), &ctx)
            .unwrap()
            .unwrap()
            .net,
        NetId(9)
    );
}

#[test]
fn zero_layer_stack_keeps_the_via_identity_without_a_physical_span() {
    let db = database(&[stack(100, 0, 0, 0, true, 0), via(1, 100, 0x8012)]);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let via = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default())
        .unwrap()
        .via(RecordKey(1), &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(
        (via.id, via.at, via.start_layer, via.end_layer),
        (ObjectId(1), Point::new(1.0, 2.0), None, None)
    );
    assert!(via.pads.is_empty());
    near(via.drill, 0.0);
}

#[test]
fn supported_wire_binds_die_pin_finger_profile_and_material() {
    let db = database(&wire_records());
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    let segments = decoder.track(RecordKey(10), &ctx).unwrap();
    let wire = &segments[0];
    assert_eq!(
        (wire.id, wire.track_id, wire.layer, wire.net),
        (ObjectId(7), ObjectId(10), LayerId::BOND_WIRE_TOP, NetId(77))
    );
    assert_eq!(
        (wire.a, wire.b),
        (Point::new(11.0, 22.0), Point::new(15.0, 26.0))
    );
    let info = wire.bond_wire.as_ref().unwrap();
    assert_eq!(
        (
            info.profile.as_str(),
            info.material.as_deref(),
            info.source_pin,
            info.finger
        ),
        ("TOP", Some("Au"), ObjectId(4), ObjectId(5))
    );
    let finger = decoder.via(RecordKey(5), &ctx).unwrap().unwrap();
    assert_eq!(finger.finger.unwrap().source_pin, Some(ObjectId(4)));
    near(finger.angle, std::f64::consts::FRAC_PI_2);
    near(finger.pads[0].offset.x, -0.2);
    near(finger.pads[0].offset.y, 0.1);
    assert!(decoder.take_diagnostics().is_empty());
}

#[test]
fn wire_endpoint_validation_uses_unrounded_source_coordinates() {
    let mut records = wire_records();
    for (key, at, value) in [
        (1, 32, 0),
        (1, 36, 0),
        (1, 28, 45000),
        (6, 20, 1),
        (6, 24, 0),
        (7, 28, 0),
        (7, 32, 0),
    ] {
        alter(&mut records, key, at, value);
    }
    let db = database(&records);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    assert_eq!(decoder.track(RecordKey(10), &ctx).unwrap().len(), 1);
}

#[test]
fn ambiguous_finger_source_stays_unset_even_after_a_repeated_original_link() {
    let mut records = wire_records();
    let mut second = record(0x32, 12, 84);
    put(&mut second, 28, 1);
    put(&mut second, 36, 6);
    records.push(second);
    for (key, pin) in [(11, 12), (13, 4)] {
        let mut wire = track(key, 0xfd06, 0);
        put(&mut wire, 28, 1);
        put(&mut wire, 36, pin);
        put(&mut wire, 48, 5);
        records.push(wire);
    }
    let db = database(&records);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let finger = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default())
        .unwrap()
        .via(RecordKey(5), &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(finger.finger.unwrap().source_pin, None);
}

#[test]
fn wire_unknown_endpoints_profile_flags_and_attribute_cycles_are_localized_diagnostics() {
    for (key, at, value) in [
        (7, 28, 11002),
        (7, 16, 159),
        (4, 28, 99),
        (8, 8, 8),
        (9, 8, 0),
        (8, 24, u32::from_le_bytes(*b"BOT\0")),
    ] {
        let mut records = wire_records();
        alter(&mut records, key, at, value);
        let db = database(&records);
        let token = CancellationToken::default();
        let ctx = context(&token);
        let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
        let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
        assert!(decoder.track(RecordKey(10), &ctx).unwrap().is_empty());
        let diagnostics = decoder.take_diagnostics();
        assert_eq!(diagnostics[0].code.as_ref(), "BRD_BOND_WIRE_UNSUPPORTED");
        for locale in Locale::ALL {
            let message = diagnostics[0].message.display(locale);
            assert!(message.contains("10"));
            assert!(!message.contains("%{"));
        }
    }
}

#[test]
fn physical_track_preserves_owner_layer_and_rejects_cycles_and_resource_exhaustion() {
    let mut source = track(10, 0x0206, 7);
    put(&mut source, 8, 90);
    let records = [
        source,
        edge(7, 8, 10),
        edge(8, 10, 10),
        assignment(90, 42, 10),
    ];
    let db = database(&records);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    let output = decoder.track(RecordKey(10), &ctx).unwrap();
    assert_eq!(output.len(), 2);
    assert!(output.iter().all(|s| s.track_id == ObjectId(10)
        && s.net == NetId(42)
        && s.layer == LayerId(2)
        && s.bond_wire.is_none()));
    let mut limits = PlacementLimits::default();
    limits.geometry.max_edges = 1;
    assert!(matches!(
        PlacementDecoder::new(&db, &nets, 6, limits)
            .unwrap()
            .track(RecordKey(10), &ctx),
        Err(ImportError::InvalidRecord {
            field: "TRACK_EDGE_COUNT",
            ..
        })
    ));
    let mut records = records.to_vec();
    alter(&mut records, 8, 8, 7);
    let cyclic = database(&records);
    let nets = NetworkMap::build(&cyclic, &NetworkLimits::default(), &ctx).unwrap();
    assert!(matches!(
        PlacementDecoder::new(&cyclic, &nets, 6, PlacementLimits::default())
            .unwrap()
            .track(RecordKey(10), &ctx),
        Err(ImportError::ReferenceCycle { key: 7, .. })
    ));
}

#[test]
fn backdrill_pads_are_shared_keep_physical_spans_and_separate_placement_flags() {
    let mut first = via(1, 20, 0x3100);
    put(&mut first, 60, 90000);
    let second = via(2, 21, 0x3000);
    let mut definition = stack(100, 0, 6, 4, true, 200);
    for index in 0..6 {
        let at = 192 + (23 + index * 4) * 36;
        definition[at] = 2;
        put(&mut definition, at + 12, 400 + index as u32 * 100);
        put(&mut definition, at + 20, 0);
        put(&mut definition, at + 24, 0);
    }
    let db = database(&[
        definition,
        first,
        second,
        wrapper(20, 0, [100, 1, 6, 0, 0x0102, 32]),
        wrapper(21, 0, [100, 2, 6, 0, 0x0102, 32]),
    ]);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    let first = decoder.via(RecordKey(1), &ctx).unwrap().unwrap();
    let second = decoder.via(RecordKey(2), &ctx).unwrap().unwrap();
    assert!(Arc::ptr_eq(&first.pads, &second.pads));
    assert!(!first.mirrored);
    near(first.angle, 0.0);
    let drill = first.backdrill.unwrap();
    assert!(drill.mirrored);
    near(drill.rotation_degrees, 90.0);
    assert_eq!(
        (
            drill.definition.spans[0].start_layer,
            drill.definition.spans[0].stop_layer
        ),
        (LayerId(0), LayerId(1))
    );
    assert!(
        first
            .pads
            .iter()
            .filter(|p| p.layer == LayerId(2))
            .all(|p| !p.backdrill)
    );
}

#[test]
fn unsupported_via_and_finger_and_missing_track_layer_have_five_language_messages() {
    let db = database(&[
        via(1, 999, 0),
        stack(100, 0, 1, 30, false, 0),
        via(2, 100, 0),
        track(10, 0x0906, 0),
    ]);
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut decoder = PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default()).unwrap();
    assert!(decoder.via(RecordKey(1), &ctx).unwrap().is_none());
    assert!(decoder.via(RecordKey(2), &ctx).unwrap().is_none());
    assert!(decoder.track(RecordKey(10), &ctx).unwrap().is_empty());
    let warnings = decoder.take_diagnostics();
    assert_eq!(warnings.len(), 3);
    for warning in warnings {
        for locale in Locale::ALL {
            let text = warning.message.display(locale);
            assert!(!text.contains("%{"));
            assert!(!text.starts_with("import."));
        }
    }
}

#[test]
fn bond_attributes_and_finger_link_map_enforce_independent_budgets() {
    let db = database(&wire_records());
    let token = CancellationToken::default();
    let ctx = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &ctx).unwrap();
    let mut limits = PlacementLimits::default();
    limits.chain.max_records = 1;
    assert!(matches!(
        PlacementDecoder::new(&db, &nets, 6, limits)
            .unwrap()
            .track(RecordKey(10), &ctx),
        Err(ImportError::InvalidRecord {
            field: "BOND_ATTRIBUTE_COUNT",
            ..
        })
    ));
    let mut limits = PlacementLimits::default();
    limits.links.max_bytes = 0;
    assert!(matches!(
        PlacementDecoder::new(&db, &nets, 6, limits)
            .unwrap()
            .via(RecordKey(5), &ctx),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
}

#[test]
fn cancellation_inside_a_track_returns_no_partial_result_and_a_fresh_retry_completes() {
    let mut records = vec![track(10, 0x0006, 1000)];
    for index in 0..600 {
        records.push(edge(
            1000 + index,
            if index == 599 { 10 } else { 1001 + index },
            10,
        ));
    }
    let db = database(&records);
    let token = CancellationToken::default();
    let plain = context(&token);
    let nets = NetworkMap::build(&db, &NetworkLimits::default(), &plain).unwrap();
    let progress = |event: pomelo_core::task::ImportProgress| {
        if event.stage == pomelo_core::task::ImportStage::BuildingGeometry && event.completed >= 256
        {
            token.cancel();
        }
    };
    let ctx = ImportContext {
        cancellation: &token,
        progress: &progress,
    };
    assert!(matches!(
        PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default())
            .unwrap()
            .track(RecordKey(10), &ctx),
        Err(ImportError::Cancelled)
    ));
    let fresh = CancellationToken::default();
    assert_eq!(
        PlacementDecoder::new(&db, &nets, 6, PlacementLimits::default())
            .unwrap()
            .track(RecordKey(10), &context(&fresh))
            .unwrap()
            .len(),
        600
    );
}
