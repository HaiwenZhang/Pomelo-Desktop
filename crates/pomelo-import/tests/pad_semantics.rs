use std::sync::Arc;

use pomelo_core::{
    i18n::Locale,
    model::{LayerId, Pad, PadKind, Point},
    pad::PadPlacement,
    task::CancellationToken,
};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    allegro::{
        database::BrdDatabase,
        decoder::{
            DecodeLimits,
            fixed::Via,
            variable::{Padstack, PadstackComponent},
        },
        index::{IndexLimits, RecordKey},
        semantics::{
            CacheLimits,
            geometry::GeometryLimits,
            pad::{PadDecoder, apply_backdrill},
            padstack::{PadstackResolver, bond_finger_angle},
        },
    },
};

fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn component(kind: u16, width: i32, height: i32) -> PadstackComponent {
    PadstackComponent {
        r#type: kind,
        width,
        height,
        z1: 0,
        offset_x: 0,
        offset_y: 0,
        z2: 0,
        shape_ptr: 0,
    }
}
fn stack(layers: u16, pad_type: u16, plated: bool, drill: u32) -> Padstack {
    let mut components: Vec<_> = (0..21 + usize::from(layers) * 4)
        .map(|index| {
            if index == 5 {
                component(2, 500, 500)
            } else if index >= 21 && (index - 21) % 4 == 2 {
                component(2, 400, 400)
            } else {
                component(0, 0, 0)
            }
        })
        .collect();
    components[14] = component(2, 900, 900);
    let mut metadata = vec![0; 29];
    metadata[0] = (-600_i32) as u32;
    metadata[7] = (-600_i32) as u32;
    Padstack {
        key: RecordKey(1),
        layer_count: layers,
        pad_type: Some(pad_type),
        plated,
        flags: if plated { 0x20 } else { 0 },
        drill_size: drill,
        num_fixed_comp_entries: 21,
        num_comps_per_layer: 4,
        drill_metadata_words: Some(metadata),
        components,
        ..Padstack::default()
    }
}
fn encoded_stack(stack: &Padstack, version: u16) -> Vec<u8> {
    let header = if version >= 180 { 224 } else { 192 };
    let mut bytes = vec![0; header + stack.components.len() * 36];
    bytes[0] = 0x1c;
    bytes[3] = stack.start_layer as u8;
    put(&mut bytes, 4, stack.key.0);
    bytes[28] = stack.pad_type.unwrap_or(0) as u8;
    bytes[30] = if stack.plated { 0x20 } else { 0 };
    bytes[44..46].copy_from_slice(&stack.layer_count.to_le_bytes());
    put(&mut bytes, 64, stack.drill_size);
    put(&mut bytes, 76, stack.slot_x);
    put(&mut bytes, 80, stack.slot_y);
    if let Some(metadata) = &stack.drill_metadata_words {
        for (index, &word) in metadata
            .iter()
            .take(if version >= 180 { 29 } else { 21 })
            .enumerate()
        {
            put(&mut bytes, 108 + index * 4, word);
        }
    }
    for (index, pad) in stack.components.iter().enumerate() {
        let start = header + index * 36;
        bytes[start] = pad.r#type as u8;
        for (offset, value) in [
            (8, pad.width as u32),
            (12, pad.height as u32),
            (16, pad.z1 as u32),
            (20, pad.offset_x as u32),
            (24, pad.offset_y as u32),
            (28, pad.z2),
            (32, pad.shape_ptr),
        ] {
            put(&mut bytes, start + offset, value);
        }
    }
    bytes
}
fn wrapper(t2: u16, words: [u32; 6]) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    bytes[0] = 0x2f;
    bytes[2..4].copy_from_slice(&t2.to_le_bytes());
    put(&mut bytes, 4, 2);
    for (index, value) in words.into_iter().enumerate() {
        put(&mut bytes, 8 + index * 4, value);
    }
    bytes
}
fn database(version: u16, records: &[Vec<u8>]) -> BrdDatabase {
    let mut bytes = vec![0; 0x1200];
    put(
        &mut bytes,
        0,
        if version >= 180 { 0x150200 } else { 0x140900 },
    );
    put(&mut bytes, 0x26c, 1000);
    put(&mut bytes, 0x28c, 1000);
    bytes[0x180] = 3;
    bytes[0x1ac] = 3;
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes.extend_from_slice(&[0; 4]);
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
fn decoder(db: &BrdDatabase) -> PadDecoder<'_> {
    PadDecoder::new(db, CacheLimits::default(), GeometryLimits::default()).unwrap()
}

#[test]
fn definitions_are_shared_and_cached_queries_still_observe_cancellation() {
    let db = database(174, &[encoded_stack(&stack(4, 4, true, 200), 174)]);
    let token = CancellationToken::default();
    let mut resolver = PadstackResolver::new(&db, 4, CacheLimits::default());
    let first = resolver
        .resolve_pin(RecordKey(1), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    let second = resolver
        .resolve_via(RecordKey(1), RecordKey(20), &context(&token))
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&first.stack, &second.stack));
    token.cancel();
    assert!(matches!(
        resolver.definition(RecordKey(1), &context(&token)),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn embedded_pins_keep_opaque_word_three_and_do_not_overrestrict_slot_x_or_pad_type() {
    let mut definition = stack(1, 4, true, 0);
    definition.start_layer = 7;
    definition.slot_x = 55;
    let db = database(
        174,
        &[
            encoded_stack(&definition, 174),
            wrapper(0x0200, [1, 10, 1, 0xdeadbeef, 0, 16]),
        ],
    );
    let token = CancellationToken::default();
    let result = PadstackResolver::new(&db, 4, CacheLimits::default())
        .resolve_pin(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    assert_eq!(result.embedded_layer, Some(LayerId(2)));
    assert!(!result.die && result.region_code.is_none());
    assert_eq!((result.stack.slot_x, result.stack.start_layer), (55, 7));
}

#[test]
fn embedded_pins_reject_wrong_owner_count_category_and_physical_layer() {
    for (t2, words) in [
        (0x0200, [1, 11, 1, 0, 0, 16]),
        (0x0200, [1, 10, 2, 0, 0, 16]),
        (0x0200, [1, 10, 1, 0, 1, 16]),
        (0x0200, [1, 10, 1, 0, 0, 17]),
        (0x0201, [1, 10, 1, 0, 0, 16]),
        (0x0400, [1, 10, 1, 0, 0, 16]),
    ] {
        let db = database(
            174,
            &[
                encoded_stack(&stack(1, 10, false, 0), 174),
                wrapper(t2, words),
            ],
        );
        let token = CancellationToken::default();
        assert!(
            PadstackResolver::new(&db, 4, CacheLimits::default())
                .resolve_pin(RecordKey(2), RecordKey(10), &context(&token))
                .unwrap()
                .is_none(),
            "t2={t2:x}, words={words:?}"
        );
    }
}

#[test]
fn die_pads_do_not_turn_special_layer_252_into_physical_copper() {
    let db = database(
        174,
        &[
            encoded_stack(&stack(1, 26, false, 0), 174),
            wrapper(0xfc00, [1, 10, 1, 0, 0, 8]),
        ],
    );
    let token = CancellationToken::default();
    let result = PadstackResolver::new(&db, 4, CacheLimits::default())
        .resolve_pin(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    assert!(result.die);
    assert!(result.embedded_layer.is_none());
    for (offset, value) in [(28, 10), (30, 0x20), (64, 1), (76, 1), (80, 1)] {
        let mut encoded = encoded_stack(&stack(1, 26, false, 0), 174);
        encoded[offset] = value;
        let db = database(174, &[encoded, wrapper(0xfc00, [1, 10, 1, 0, 0, 8])]);
        assert!(
            PadstackResolver::new(&db, 4, CacheLimits::default())
                .resolve_pin(RecordKey(2), RecordKey(10), &context(&token))
                .unwrap()
                .is_none(),
            "offset={offset}"
        );
    }
}

#[test]
fn region_pins_and_vias_keep_region_identity_separate_from_the_layer_span() {
    let token = CancellationToken::default();
    let db = database(
        174,
        &[
            encoded_stack(&stack(1, 10, false, 0), 174),
            wrapper(0x0200, [1, 10, 0x00070001, 0, 0, 64]),
        ],
    );
    let result = PadstackResolver::new(&db, 4, CacheLimits::default())
        .resolve_pin(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    assert_eq!(
        (result.region_code, result.embedded_layer),
        (Some(7), Some(LayerId(2)))
    );
    let db = database(
        174,
        &[
            encoded_stack(&stack(4, 4, true, 200), 174),
            wrapper(0, [1, 10, 0x00070004, 0, 0, 64]),
        ],
    );
    let result = PadstackResolver::new(&db, 4, CacheLimits::default())
        .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            result.region_code,
            result.stack.start_layer,
            result.stack.layer_count
        ),
        (Some(7), 0, 4)
    );
    assert!(result.backdrill.is_none());
}

#[test]
fn region_vias_reject_unplated_and_blind_definitions() {
    for (layers, plated, drill) in [(4, false, 200), (3, true, 200), (4, true, 0)] {
        let db = database(
            174,
            &[
                encoded_stack(&stack(layers, 4, plated, drill), 174),
                wrapper(0, [1, 10, 0x00070004, 0, 0, 64]),
            ],
        );
        let token = CancellationToken::default();
        assert!(
            PadstackResolver::new(&db, 4, CacheLimits::default())
                .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn backdrills_decode_signed_diameters_from_the_version_specific_metadata_slot() {
    for version in [174, 181] {
        let mut definition = stack(6, 4, true, 200);
        if version == 181 {
            definition.drill_metadata_words.as_mut().unwrap()[0] = 1234;
        }
        let db = database(
            version,
            &[
                encoded_stack(&definition, version),
                wrapper(0, [1, 10, 6, 0xdeadbeef, 0x0102, 32]),
            ],
        );
        let token = CancellationToken::default();
        let resolved = PadstackResolver::new(&db, 6, CacheLimits::default())
            .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
            .unwrap()
            .unwrap();
        let backdrill = resolved.backdrill.unwrap();
        assert_eq!(
            (
                backdrill.display_diameter,
                backdrill.start_pad_diameter,
                backdrill.label_diameter
            ),
            (600.0, 500.0, 400.0)
        );
        assert_eq!(
            backdrill
                .spans
                .iter()
                .map(|span| (
                    span.start_layer.0,
                    span.stop_layer.0,
                    span.protected_layer.0
                ))
                .collect::<Vec<_>>(),
            [(0, 1, 2), (5, 5, 4)]
        );
        let mm = backdrill.to_millimetres(0.001);
        assert_eq!(
            (
                mm.display_diameter,
                mm.start_pad_diameter,
                mm.label_diameter
            ),
            (0.6, 0.5, 0.4)
        );
    }
}

#[test]
fn backdrill_minimum_signed_word_does_not_overflow() {
    let mut definition = stack(4, 4, true, 200);
    definition.drill_metadata_words.as_mut().unwrap()[0] = i32::MIN as u32;
    let db = database(
        174,
        &[
            encoded_stack(&definition, 174),
            wrapper(0, [1, 10, 4, 0, 1, 32]),
        ],
    );
    let token = CancellationToken::default();
    let result = PadstackResolver::new(&db, 4, CacheLimits::default())
        .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    assert_eq!(result.backdrill.unwrap().display_diameter, 2_147_483_648.0);
}

#[test]
fn backdrills_reject_solder_mask_substitutes_eccentric_pads_and_overlapping_cuts() {
    for change in 0..4 {
        let mut definition = stack(4, 4, true, 200);
        let mut encoded = 0x0101;
        match change {
            0 => definition.components[5].r#type = 0,
            1 => definition.components[23].offset_x = 1,
            2 => definition.components[23].height += 1,
            _ => encoded = 0x0202,
        }
        let db = database(
            174,
            &[
                encoded_stack(&definition, 174),
                wrapper(0, [1, 10, 4, 0, encoded, 32]),
            ],
        );
        let token = CancellationToken::default();
        assert!(
            PadstackResolver::new(&db, 4, CacheLimits::default())
                .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
                .unwrap()
                .is_none(),
            "change={change}"
        );
    }
}

#[test]
fn backdrill_geometry_keeps_protected_copper_and_entry_base_before_display_circle() {
    let db = database(
        174,
        &[
            encoded_stack(&stack(6, 4, true, 200), 174),
            wrapper(0, [1, 10, 6, 0, 0x0102, 32]),
        ],
    );
    let token = CancellationToken::default();
    let resolved = PadstackResolver::new(&db, 6, CacheLimits::default())
        .resolve_via(RecordKey(2), RecordKey(10), &context(&token))
        .unwrap()
        .unwrap();
    let mut decoder = decoder(&db);
    let ordinary = decoder
        .regular_pads(&resolved.stack, &context(&token))
        .unwrap();
    let effective = apply_backdrill(
        &ordinary,
        &resolved.backdrill.unwrap().to_millimetres(decoder.scale()),
        6,
        &context(&token),
    )
    .unwrap();
    assert_eq!(
        effective
            .iter()
            .map(|pad| (pad.layer.0, pad.width, pad.backdrill_base, pad.backdrill))
            .collect::<Vec<_>>(),
        [
            (0, 0.5, true, false),
            (0, 0.6, false, true),
            (1, 0.4, true, false),
            (1, 0.6, false, true),
            (2, 0.4, false, false),
            (3, 0.4, false, false),
            (4, 0.4, false, false),
            (5, 0.5, true, false),
            (5, 0.6, false, true)
        ]
    );
    assert_eq!(ordinary.len(), 6);
}

#[test]
fn drill_slots_follow_regular_pad_orientation_and_zero_slot_y_uses_round_drill() {
    let db = database(174, &[]);
    let token = CancellationToken::default();
    let mut decoder = decoder(&db);
    let mut definition = stack(1, 4, true, 200);
    definition.slot_x = 600;
    definition.slot_y = 300;
    definition.components[23] = component(11, 300, 800);
    let drill = decoder.drill(&definition, &context(&token)).unwrap();
    assert_eq!((drill.width, drill.height, drill.plated), (0.3, 0.6, true));
    assert_eq!(drill.pad().unwrap().kind, PadKind(11));
    definition.key = RecordKey(3);
    definition.slot_y = 0;
    let drill = decoder.drill(&definition, &context(&token)).unwrap();
    assert_eq!((drill.width, drill.height), (0.2, 0.2));
}

#[test]
fn circle_and_donut_height_use_width_and_donut_opening_never_comes_from_drill_size() {
    let db = database(174, &[]);
    let token = CancellationToken::default();
    let mut decoder = decoder(&db);
    let circle = decoder
        .shape(
            &component(2, 1000, -10),
            LayerId(0),
            Point::default(),
            RecordKey(1),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    assert_eq!((circle.width, circle.height), (1.0, 1.0));
    let mut donut = component(25, 1000, 0);
    donut.z1 = 300;
    let pad = decoder
        .shape(
            &donut,
            LayerId(0),
            Point::default(),
            RecordKey(1),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    assert_eq!((pad.height, pad.inner_diameter), (1.0, Some(0.3)));
    donut.z1 = 1000;
    assert!(
        decoder
            .shape(
                &donut,
                LayerId(0),
                Point::default(),
                RecordKey(1),
                &context(&token)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(decoder.diagnostics()[0].code.as_ref(), "BRD_PAD_DONUT");
}

#[test]
fn zero_rectangles_are_none_and_unknown_families_remain_unsupported_without_fake_geometry() {
    let db = database(174, &[]);
    let token = CancellationToken::default();
    let mut decoder = decoder(&db);
    assert!(
        decoder
            .shape(
                &component(6, 0, 1000),
                LayerId(0),
                Point::default(),
                RecordKey(1),
                &context(&token)
            )
            .unwrap()
            .is_none()
    );
    assert!(decoder.diagnostics().is_empty());
    for _ in 0..2 {
        let pad = decoder
            .shape(
                &component(29, 1000, 2000),
                LayerId(0),
                Point::default(),
                RecordKey(1),
                &context(&token),
            )
            .unwrap()
            .unwrap();
        assert!(!pad.supported() && pad.custom.is_none());
    }
    assert_eq!(decoder.diagnostics().len(), 1);
    for locale in Locale::ALL {
        assert!(decoder.diagnostics()[0].message.render(locale).is_ok());
    }
}

fn custom_records() -> Vec<Vec<u8>> {
    let mut shape = vec![0; 76];
    shape[0] = 0x28;
    put(&mut shape, 4, 100);
    put(&mut shape, 40, 101);
    let points = [(-1000, 0), (3000, 0), (0, 2000)];
    let mut records = vec![shape];
    for index in 0..3 {
        let mut line = vec![0; 44];
        line[0] = 0x15;
        put(&mut line, 4, 101 + index as u32);
        put(
            &mut line,
            8,
            if index == 2 { 100 } else { 102 + index as u32 },
        );
        for (offset, value) in [
            (28, points[index].0),
            (32, points[index].1),
            (36, points[(index + 1) % 3].0),
            (40, points[(index + 1) % 3].1),
        ] {
            put(&mut line, offset, value as u32);
        }
        records.push(line);
    }
    records
}

#[test]
fn custom_pad_extents_come_from_shared_source_contours_and_offsets_do_not_mutate_the_cache() {
    let db = database(174, &custom_records());
    let token = CancellationToken::default();
    let mut decoder = decoder(&db);
    let mut custom = component(22, 0, -1);
    custom.shape_ptr = 100;
    let first = decoder
        .shape(
            &custom,
            LayerId(0),
            Point::new(3.0, 4.0),
            RecordKey(1),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    let second = decoder
        .shape(
            &custom,
            LayerId(1),
            Point::new(-3.0, -4.0),
            RecordKey(2),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    assert_eq!((first.width, first.height), (4.0, 2.0));
    assert!(Arc::ptr_eq(
        first.custom.as_ref().unwrap(),
        second.custom.as_ref().unwrap()
    ));
    assert_eq!(
        first.custom.as_ref().unwrap().contours[0][0],
        Point::new(-1.0, 0.0)
    );
    assert_eq!(first.custom.as_ref().unwrap().paths[0].len(), 3);
    assert!(decoder.diagnostics().is_empty());
}

#[test]
fn pad_bounds_rotate_local_geometry_and_keep_board_space_offsets_outside_rotation() {
    let mut pad = Pad::circle(LayerId(0), 4.0);
    pad.kind = PadKind(6);
    pad.height = 2.0;
    pad.offset = Point::new(3.0, 4.0);
    let bounds = pad
        .bounds(PadPlacement {
            at: Point::new(10.0, 20.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: true,
        })
        .unwrap();
    assert!((bounds.min.x - 12.0).abs() < 1e-12 && (bounds.max.x - 14.0).abs() < 1e-12);
    assert!((bounds.min.y - 22.0).abs() < 1e-12 && (bounds.max.y - 26.0).abs() < 1e-12);
    let db = database(174, &custom_records());
    let token = CancellationToken::default();
    let mut custom = component(22, 1000, 2000);
    custom.shape_ptr = 100;
    let pad = decoder(&db)
        .shape(
            &custom,
            LayerId(0),
            Point::new(3.0, 4.0),
            RecordKey(1),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    let world = pad.to_world(
        Point::new(1.0, 2.0),
        PadPlacement {
            at: Point::new(10.0, 20.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: true,
        },
    );
    assert_eq!(world, Point::new(15.0, 25.0));
}

#[test]
fn caches_enforce_byte_and_entry_limits_before_insertion() {
    let mut second = stack(1, 4, true, 200);
    second.key = RecordKey(3);
    let db = database(
        174,
        &[
            encoded_stack(&stack(1, 4, true, 200), 174),
            encoded_stack(&second, 174),
        ],
    );
    let token = CancellationToken::default();
    let mut resolver = PadstackResolver::new(
        &db,
        4,
        CacheLimits {
            max_bytes: 0,
            ..CacheLimits::default()
        },
    );
    assert!(matches!(
        resolver.definition(RecordKey(1), &context(&token)),
        Err(ImportError::SemanticCacheLimit { offset: 0x1200, .. })
    ));
    let mut resolver = PadstackResolver::new(
        &db,
        4,
        CacheLimits {
            max_entries: 1,
            ..CacheLimits::default()
        },
    );
    let first = resolver
        .definition(RecordKey(1), &context(&token))
        .unwrap()
        .unwrap();
    assert!(matches!(
        resolver.definition(RecordKey(3), &context(&token)),
        Err(ImportError::InvalidRecord {
            field: "SEMANTIC_CACHE_ENTRY_COUNT",
            ..
        })
    ));
    assert!(Arc::ptr_eq(
        &first,
        &resolver
            .definition(RecordKey(1), &context(&token))
            .unwrap()
            .unwrap()
    ));
    let mut decoder = PadDecoder::new(
        &db,
        CacheLimits {
            max_bytes: 0,
            ..CacheLimits::default()
        },
        GeometryLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        decoder.drill(&second, &context(&token)),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
}

#[test]
fn custom_geometry_work_is_bounded_by_remaining_cache_budget() {
    let db = database(174, &custom_records());
    let token = CancellationToken::default();
    let mut decoder = PadDecoder::new(
        &db,
        CacheLimits {
            max_bytes: 512,
            ..CacheLimits::default()
        },
        GeometryLimits::default(),
    )
    .unwrap();
    let mut custom = component(22, 0, 0);
    custom.shape_ptr = 100;
    assert!(matches!(
        decoder.shape(
            &custom,
            LayerId(0),
            Point::default(),
            RecordKey(1),
            &context(&token)
        ),
        Err(ImportError::GeometryLimit { .. })
    ));
    assert!(decoder.diagnostics().is_empty());
    token.cancel();
    assert!(matches!(
        decoder.shape(
            &component(2, 1000, 1000),
            LayerId(0),
            Point::default(),
            RecordKey(1),
            &context(&token)
        ),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn missing_custom_shapes_and_negative_dimensions_keep_five_language_warnings() {
    let db = database(174, &[]);
    let token = CancellationToken::default();
    let mut decoder = decoder(&db);
    let mut custom = component(22, 1000, 2000);
    custom.shape_ptr = 999;
    let pad = decoder
        .shape(
            &custom,
            LayerId(0),
            Point::default(),
            RecordKey(42),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    assert!(!pad.supported() && pad.custom.is_none());
    assert!(
        decoder
            .shape(
                &component(6, -1000, 2000),
                LayerId(0),
                Point::default(),
                RecordKey(42),
                &context(&token)
            )
            .unwrap()
            .is_none()
    );
    for warning in decoder.diagnostics() {
        let values: std::collections::BTreeSet<_> = Locale::ALL
            .into_iter()
            .map(|locale| warning.message.render(locale).unwrap())
            .collect();
        assert_eq!(values.len(), 5);
    }
    assert_eq!(decoder.diagnostics().len(), 2);
}

#[test]
fn custom_pad_holes_keep_all_fill_rings_and_analytic_boundaries() {
    let mut records = custom_records();
    put(&mut records[0], 36, 200);
    let mut hole = vec![0; 36];
    hole[0] = 0x34;
    put(&mut hole, 4, 200);
    put(&mut hole, 8, 100);
    put(&mut hole, 24, 201);
    records.push(hole);
    let points = [(0, 250), (500, 250), (0, 750)];
    for index in 0..3 {
        let mut line = vec![0; 44];
        line[0] = 0x15;
        put(&mut line, 4, 201 + index as u32);
        put(
            &mut line,
            8,
            if index == 2 { 200 } else { 202 + index as u32 },
        );
        for (offset, value) in [
            (28, points[index].0),
            (32, points[index].1),
            (36, points[(index + 1) % 3].0),
            (40, points[(index + 1) % 3].1),
        ] {
            put(&mut line, offset, value as u32);
        }
        records.push(line);
    }
    let db = database(174, &records);
    let token = CancellationToken::default();
    let mut custom = component(22, 0, 0);
    custom.shape_ptr = 100;
    let pad = decoder(&db)
        .shape(
            &custom,
            LayerId(0),
            Point::default(),
            RecordKey(42),
            &context(&token),
        )
        .unwrap()
        .unwrap();
    let geometry = pad.custom.unwrap();
    assert_eq!(geometry.contours.len(), 2);
    assert_eq!(
        geometry.paths.iter().map(Vec::len).collect::<Vec<_>>(),
        [3, 3]
    );
    assert_eq!(geometry.contours[1][0], Point::new(0.0, 0.25));
    assert_eq!((pad.width, pad.height), (4.0, 2.0));
}

#[test]
fn backdrill_geometry_rejects_nonfinite_dimensions_and_cut_protected_layers() {
    use pomelo_core::model::{BackdrillDefinition, BackdrillSpan};
    let token = CancellationToken::default();
    let mut definition = BackdrillDefinition {
        spans: vec![BackdrillSpan {
            start_layer: LayerId(0),
            stop_layer: LayerId(1),
            protected_layer: LayerId(2),
        }],
        display_diameter: f64::NAN,
        start_pad_diameter: 0.5,
        label_diameter: 0.4,
    };
    assert!(matches!(
        apply_backdrill(&[], &definition, 4, &context(&token)),
        Err(ImportError::InvalidGeometry { .. })
    ));
    definition.display_diameter = 0.6;
    definition.spans[0].protected_layer = LayerId(1);
    assert!(matches!(
        apply_backdrill(&[], &definition, 4, &context(&token)),
        Err(ImportError::InvalidGeometry { .. })
    ));
}

#[test]
fn bond_fingers_use_their_own_rotation_without_inventing_a_drill_or_back_face() {
    let mut definition = stack(1, 30, false, 0);
    definition.start_layer = 2;
    let mut via = Via {
        layer_info: 0xc012,
        unknown5: 90_000,
        ..Via::default()
    };
    assert!(
        (bond_finger_angle(&via, &definition, 4).unwrap() - std::f64::consts::FRAC_PI_2).abs()
            < 1e-12
    );
    via.unknown5 = 360_000;
    assert!(bond_finger_angle(&via, &definition, 4).is_none());
    via.unknown5 = 0;
    definition.drill_size = 1;
    assert!(bond_finger_angle(&via, &definition, 4).is_none());
}
