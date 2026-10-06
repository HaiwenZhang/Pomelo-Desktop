use pomelo_core::{
    i18n::Locale,
    model::{LayerId, NetId, ObjectId, Point},
    task::CancellationToken,
};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    formats::allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::{IndexLimits, RecordKey},
        semantics::{
            connectivity::{NetworkLimits, NetworkMap},
            placement::{PlacementDecoder, PlacementLimits},
        },
    },
};

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixture(back: bool, rotation: u32, variant: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0x1200];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    let mut fp = vec![0; 72];
    fp[0] = 0x2d;
    fp[2] = u8::from(back);
    for (at, value) in [(4, 1), (28, rotation), (32, 10000), (36, 20000), (48, 4)] {
        put(&mut fp, at, value);
    }
    bytes.extend(fp);
    let count = if variant == 0 { 2 } else { 1 };
    let mut stack = vec![0; 192 + (21 + count * 4) * 36];
    stack[0] = 0x1c;
    stack[3] = if variant == 0 { 1 } else { 0 };
    put(&mut stack, 4, 2);
    stack[28] = if variant == 8 {
        26
    } else if variant == 64 {
        10
    } else {
        4
    };
    stack[44] = count as u8;
    for i in 0..count {
        let p = 192 + (23 + 4 * i) * 36;
        stack[p] = 6;
        for (at, value) in [(8, 1000), (12, 500), (20, 100), (24, 200)] {
            put(&mut stack, p + at, value);
        }
    }
    bytes.extend(stack);
    let mut pad = vec![0; 48];
    pad[0] = 0x0d;
    for (at, value) in [
        (4, 3),
        (20, 1000),
        (24, 2000),
        (28, if variant == 0 { 2 } else { 20 }),
        (44, 30000),
    ] {
        put(&mut pad, at, value);
    }
    bytes.extend(pad);
    let mut placed = vec![0; 84];
    placed[0] = 0x32;
    for (at, value) in [(4, 4), (8, 10), (24, 1), (28, 1), (36, 3), (52, 999)] {
        put(&mut placed, at, value);
    }
    bytes.extend(placed);
    for (id, net, item) in [(10, 99, 4), (11, 0, 0)] {
        let mut assignment = vec![0; 24];
        assignment[0] = 4;
        for (at, value) in [(4, id), (12, net), (16, item)] {
            put(&mut assignment, at, value);
        }
        bytes.extend(assignment);
    }
    if variant != 0 {
        let mut wrapper = vec![0; 32];
        let layer: u16 = if variant == 8 { 0xfc00 } else { 0x0200 };
        wrapper[0] = 0x2f;
        wrapper[2..4].copy_from_slice(&layer.to_le_bytes());
        put(&mut wrapper, 4, 20);
        for (i, value) in [
            2,
            4,
            if variant == 64 { 7 * 65536 + 1 } else { 1 },
            0,
            0,
            variant,
        ]
        .into_iter()
        .enumerate()
        {
            put(&mut wrapper, 8 + 4 * i, value);
        }
        bytes.extend(wrapper);
    }
    bytes.extend([0; 4]);
    bytes
}
fn db(bytes: Vec<u8>) -> BrdDatabase {
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
fn changed(bytes: &mut [u8], kind: u8, at: usize, value: u32) {
    let database = db(bytes.to_vec());
    let start = database
        .index()
        .records_of_type(kind)
        .next()
        .unwrap()
        .offset
        .0 as usize;
    put(bytes, start + at, value);
}
fn decode(
    bytes: Vec<u8>,
    limits: PlacementLimits,
) -> Result<
    (
        pomelo_import::formats::allegro::semantics::placement::PlacedFootprint,
        Vec<pomelo_core::model::Diagnostic>,
    ),
    ImportError,
> {
    let database = db(bytes);
    let token = CancellationToken::default();
    let context = ImportContext {
        cancellation: &token,
        progress: &|_| {},
    };
    let nets = NetworkMap::build(&database, &NetworkLimits::default(), &context)?;
    let mut decoder = PlacementDecoder::new(&database, &nets, 4, limits)?;
    let placed = decoder.footprint(RecordKey(1), &context)?;
    Ok((placed, decoder.take_diagnostics()))
}
fn near(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-12, "{left} != {right}");
}

#[test]
fn front_rotation_binds_source_identity_net_and_pad_offsets() {
    let (placed, diagnostics) =
        decode(fixture(false, 90000, 0), PlacementLimits::default()).unwrap();
    assert!(diagnostics.is_empty());
    assert_eq!(placed.component.pins, [ObjectId(4)]);
    let pin = &placed.pins[0];
    assert_eq!(
        (pin.id, pin.owner_id, pin.net),
        (ObjectId(4), ObjectId(1), NetId(99))
    );
    assert_eq!(pin.at, Point::new(8.0, 21.0));
    near(pin.angle, 2.0 * std::f64::consts::PI / 3.0);
    assert_eq!(
        pin.pads.iter().map(|pad| pad.layer).collect::<Vec<_>>(),
        [LayerId(1), LayerId(2)]
    );
    near(
        pin.pads[0].offset.x,
        -(0.1 * 0.5 + 0.2 * 3.0_f64.sqrt() / 2.0),
    );
    near(pin.pads[0].offset.y, 0.1 * 3.0_f64.sqrt() / 2.0 - 0.2 * 0.5);
}

#[test]
fn back_mirrors_source_x_and_reverses_layers_with_correct_pad_angle() {
    let (placed, _) = decode(fixture(true, 90000, 0), PlacementLimits::default()).unwrap();
    let pin = &placed.pins[0];
    assert!(pin.mirrored && placed.component.mirrored);
    assert_eq!(pin.at, Point::new(8.0, 19.0));
    near(pin.angle, 4.0 * std::f64::consts::PI / 3.0);
    assert_eq!(
        pin.pads.iter().map(|pad| pad.layer).collect::<Vec<_>>(),
        [LayerId(2), LayerId(1)]
    );
    near(
        pin.pads[0].offset.x,
        -(0.1 * 0.5 + 0.2 * 3.0_f64.sqrt() / 2.0),
    );
    near(
        pin.pads[0].offset.y,
        -(0.1 * 3.0_f64.sqrt() / 2.0 - 0.2 * 0.5),
    );
}

#[test]
fn source_grid_rounding_precedes_unit_conversion_with_signed_half_ties() {
    for back in [false, true] {
        let mut bytes = fixture(back, 60000, 0);
        changed(&mut bytes, 0x0d, 20, 1);
        changed(&mut bytes, 0x0d, 24, 0);
        let (placed, _) = decode(bytes, PlacementLimits::default()).unwrap();
        near(placed.pins[0].at.x, if back { 9.999 } else { 10.001 });
        near(placed.pins[0].at.y, if back { 19.999 } else { 20.001 });
    }
}

#[test]
fn explicit_net_pointer_including_zero_precedes_chain_ownership() {
    let mut bytes = fixture(false, 0, 0);
    changed(&mut bytes, 0x32, 12, 11);
    let (placed, _) = decode(bytes, PlacementLimits::default()).unwrap();
    assert_eq!(placed.pins[0].net, NetId(0));
}

#[test]
fn embedded_and_region_wrappers_override_physical_layer_mapping() {
    for variant in [16, 64] {
        let (placed, _) = decode(fixture(true, 0, variant), PlacementLimits::default()).unwrap();
        let pin = &placed.pins[0];
        assert_eq!(pin.pads[0].layer, LayerId(2));
        if variant == 64 {
            let region = pin.stackup_region.unwrap();
            assert_eq!((region.source_reference, region.code), (ObjectId(20), 7));
        }
    }
}

#[test]
fn front_die_uses_special_layer_and_back_die_is_diagnosed_in_all_five_locales() {
    let (front, _) = decode(fixture(false, 0, 8), PlacementLimits::default()).unwrap();
    assert_eq!(front.pins[0].pads[0].layer, LayerId(0x20000));
    assert_eq!(
        front.pins[0].die.as_ref().unwrap().source_reference,
        ObjectId(20)
    );
    let (back, diagnostics) = decode(fixture(true, 0, 8), PlacementLimits::default()).unwrap();
    assert!(back.pins.is_empty());
    assert_eq!(diagnostics[0].code.as_ref(), "BRD_DIE_BACK_UNSUPPORTED");
    let translated: std::collections::BTreeSet<_> = Locale::ALL
        .iter()
        .map(|locale| diagnostics[0].message.display(*locale))
        .collect();
    assert_eq!(translated.len(), 5);
}

#[test]
fn unsupported_definition_continues_fp_chain_without_following_network_next() {
    let mut bytes = fixture(false, 0, 0);
    changed(&mut bytes, 0x0d, 28, 999);
    let (placed, diagnostics) = decode(bytes, PlacementLimits::default()).unwrap();
    assert!(placed.pins.is_empty());
    assert_eq!(
        diagnostics[0].code.as_ref(),
        "BRD_PIN_DEFINITION_UNSUPPORTED"
    );
    assert_eq!(diagnostics[0].object, Some(ObjectId(4)));
    assert_eq!(
        diagnostics[0].message.args["stack"],
        pomelo_core::i18n::MessageArg::Unsigned(999)
    );
}

#[test]
fn misplaced_parent_cycle_and_missing_fp_links_are_rejected() {
    let mut wrong = fixture(false, 0, 0);
    changed(&mut wrong, 0x32, 28, 99);
    assert!(matches!(
        decode(wrong, PlacementLimits::default()),
        Err(ImportError::InvalidRecord {
            field: "ParentFp",
            ..
        })
    ));
    let mut cycle = fixture(false, 0, 0);
    changed(&mut cycle, 0x32, 24, 4);
    assert!(matches!(
        decode(cycle, PlacementLimits::default()),
        Err(ImportError::ReferenceCycle {
            field: "NextInFp",
            ..
        })
    ));
    let mut missing = fixture(false, 0, 0);
    changed(&mut missing, 0x32, 24, 999);
    assert!(matches!(
        decode(missing, PlacementLimits::default()),
        Err(ImportError::MissingReference {
            field: "NextInFp",
            ..
        })
    ));
}

#[test]
fn object_budgets_and_invalid_layer_mapping_prevent_partial_publication() {
    let mut limits = PlacementLimits::default();
    limits.objects.max_bytes = 512;
    assert!(matches!(
        decode(fixture(false, 0, 0), limits),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
    let mut bytes = fixture(false, 0, 0);
    bytes[0x1200 + 72 + 3] = 4;
    assert!(matches!(
        decode(bytes, PlacementLimits::default()),
        Err(ImportError::InvalidGeometry {
            field: "PIN_LAYER",
            ..
        })
    ));
}

#[test]
fn cancellation_before_placement_keeps_shared_network_data_valid_for_retry() {
    let database = db(fixture(false, 0, 0));
    let token = CancellationToken::default();
    let context = ImportContext {
        cancellation: &token,
        progress: &|_| {},
    };
    let nets = NetworkMap::build(&database, &NetworkLimits::default(), &context).unwrap();
    let mut decoder =
        PlacementDecoder::new(&database, &nets, 4, PlacementLimits::default()).unwrap();
    token.cancel();
    assert!(matches!(
        decoder.footprint(RecordKey(1), &context),
        Err(ImportError::Cancelled)
    ));
    assert_eq!(nets.owner(RecordKey(4)), Some(NetId(99)));
}

#[test]
fn cancelling_inside_a_large_footprint_does_not_publish_partial_pins_and_a_fresh_build_retries() {
    let mut bytes = fixture(false, 0, 0);
    changed(&mut bytes, 0x32, 24, 1000);
    bytes.truncate(bytes.len() - 4);
    for key in 1000..=1256 {
        let mut placed = vec![0; 84];
        placed[0] = 0x32;
        for (at, value) in [
            (4, key),
            (24, if key == 1256 { 1 } else { key + 1 }),
            (28, 1),
            (36, 3),
        ] {
            put(&mut placed, at, value);
        }
        bytes.extend(placed);
    }
    bytes.extend([0; 4]);
    let database = db(bytes);
    let initial = CancellationToken::default();
    let nets = NetworkMap::build(
        &database,
        &NetworkLimits::default(),
        &ImportContext {
            cancellation: &initial,
            progress: &|_| {},
        },
    )
    .unwrap();
    let cancelled = CancellationToken::default();
    let notify = |progress: pomelo_core::task::ImportProgress| {
        if progress.completed >= 256 {
            cancelled.cancel();
        }
    };
    let mut decoder =
        PlacementDecoder::new(&database, &nets, 4, PlacementLimits::default()).unwrap();
    assert!(matches!(
        decoder.footprint(
            RecordKey(1),
            &ImportContext {
                cancellation: &cancelled,
                progress: &notify
            }
        ),
        Err(ImportError::Cancelled)
    ));
    let mut retry = PlacementDecoder::new(&database, &nets, 4, PlacementLimits::default()).unwrap();
    let complete = retry
        .footprint(
            RecordKey(1),
            &ImportContext {
                cancellation: &initial,
                progress: &|_| {},
            },
        )
        .unwrap();
    assert_eq!(complete.pins.len(), 258);
    assert_eq!(complete.component.pins.len(), 258);
}
