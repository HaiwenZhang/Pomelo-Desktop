use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportError, TextEncoding,
    allegro::{
        decoder::{DecodeLimits, DecodedRecord, RecordDecoder, variable::VariableRecord},
        header::BrdHeader,
        index::{FileOffset, RecordKey, RecordSpan},
    },
};

const START: usize = 0x1200;
const VERSIONS: [u16; 13] = [
    152, 157, 160, 162, 164, 165, 166, 172, 174, 175, 180, 181, 251,
];
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixture(version: u16, kind: u8, size: usize) -> (Vec<u8>, RecordSpan) {
    let magic = match version {
        152 => 0x120500,
        157 => 0x120f00,
        160 => 0x130000,
        162 => 0x130400,
        164 => 0x130c00,
        165 => 0x131000,
        166 => 0x131500,
        172 => 0x140400,
        174 => 0x140900,
        175 => 0x141400,
        180 => 0x150000,
        181 => 0x150200,
        251 => 0x160100,
        _ => unreachable!(),
    };
    let mut bytes = vec![0; START + size];
    put32(&mut bytes, 0, magic);
    put32(&mut bytes, 0x26c, 1000);
    put32(&mut bytes, 0x28c, 1000);
    if version < 160 {
        put16(&mut bytes, START, u16::from(kind) << 10);
    } else {
        bytes[START] = kind;
    }
    let key = if [0x27, 0x3b].contains(&kind) {
        0
    } else {
        u32::MAX
    };
    let key_offset = match kind {
        0x2a => {
            if version >= 174 {
                8
            } else {
                4
            }
        }
        0x21 => 8,
        _ => 4,
    };
    if key != 0 {
        put32(&mut bytes, START + key_offset, key);
    }
    (
        bytes,
        RecordSpan {
            offset: FileOffset(START as u32),
            byte_length: size as u32,
            key: RecordKey(key),
            record_type: kind,
        },
    )
}
fn decode(
    bytes: &[u8],
    span: &RecordSpan,
    limits: &DecodeLimits,
) -> Result<DecodedRecord, ImportError> {
    let header = BrdHeader::read(bytes, TextEncoding::Utf8)?;
    let cancellation = CancellationToken::default();
    RecordDecoder::new(bytes, &header, TextEncoding::Utf8).decode(
        span,
        limits,
        &ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        },
    )
}
fn variable(bytes: &[u8], span: &RecordSpan) -> VariableRecord {
    let DecodedRecord::Variable(record) = decode(bytes, span, &DecodeLimits::default()).unwrap()
    else {
        panic!()
    };
    record
}

#[test]
fn field_word_count_is_independent_of_declared_size_and_dimension_bytes_are_not_text() {
    let (mut bytes, span) = fixture(174, 3, 52);
    bytes[START + 16] = 0x72;
    put16(&mut bytes, START + 18, 56);
    put32(&mut bytes, START + 24, 6);
    for index in 0..6 {
        put32(&mut bytes, START + 28 + index * 4, u32::MAX - index as u32);
    }
    let VariableRecord::Field(record) = variable(&bytes, &span) else {
        panic!()
    };
    assert_eq!(
        record.words.unwrap(),
        (0..6).map(|i| u32::MAX - i).collect::<Vec<_>>()
    );

    let (mut bytes, span) = fixture(175, 3, 104);
    put16(&mut bytes, START + 2, 755);
    bytes[START + 16] = 0x73;
    put16(&mut bytes, START + 18, 80);
    bytes[START + 24] = 0xff;
    bytes[START + 103] = 0x80;
    let VariableRecord::Field(record) = variable(&bytes, &span) else {
        panic!()
    };
    assert_eq!(record.payload_kind, Some("dimension-settings"));
    assert_eq!(record.value_bytes.unwrap(), bytes[START + 24..]);
    assert!(record.value.is_none());
}

#[test]
fn embedded_models_preserve_every_byte_and_align_past_a_non_word_length() {
    let (mut bytes, span) = fixture(174, 0x3b, 188);
    put32(&mut bytes, START + 4, 5);
    bytes[START + 8..START + 16].copy_from_slice(b"STEP3D_X");
    put32(&mut bytes, START + 176, 0x10000);
    bytes[START + 180..START + 185].copy_from_slice(&[0xff, 0, 0x80, 6, 7]);
    let VariableRecord::Property(record) = variable(&bytes, &span) else {
        panic!()
    };
    assert_eq!(record.value_bytes.unwrap(), [0xff, 0, 0x80, 6, 7]);
    assert_eq!(record.payload_kind, Some("embedded-model"));
    assert!(record.value.is_none());
}

#[test]
fn legacy_restricted_padstacks_compact_populated_layers_and_keep_signed_dimensions() {
    let (mut bytes, span) = fixture(152, 0x1c, 84 + 22 * 28 - 4);
    bytes[START + 3] = 2;
    bytes[START + 41] = 1;
    put16(&mut bytes, START + 44, 1);
    put16(&mut bytes, START + 50, 4);
    let first = START + 84 + 13 * 28;
    bytes[first] = 1;
    put32(&mut bytes, first + 4, (-10_i32) as u32);
    put32(&mut bytes, first + 20, u32::MAX);
    bytes[START + 84 + 16 * 28] = 2;
    let VariableRecord::Padstack(record) = variable(&bytes, &span) else {
        panic!()
    };
    assert_eq!(
        (
            record.start_layer,
            record.layer_count,
            record.components.len()
        ),
        (3, 2, 16)
    );
    assert_eq!(
        (record.components[10].width, record.components[10].shape_ptr),
        (-10, u32::MAX)
    );
    assert!(record.plated);
    assert_eq!(record.restricted_layer_span, Some(true));
    assert!(record.drill_metadata_words.is_none());
}

#[test]
fn modern_padstacks_retain_extended_drill_metadata_and_last_shape_word() {
    let (mut bytes, span) = fixture(180, 0x1c, 224 + 21 * 36);
    bytes[START + 30] = 0x20;
    put32(&mut bytes, START + 108, u32::MAX);
    put32(&mut bytes, START + 220, 42);
    put32(&mut bytes, START + 224 + 20 * 36 + 32, 0x80000000);
    let VariableRecord::Padstack(record) = variable(&bytes, &span) else {
        panic!()
    };
    let words = record.drill_metadata_words.unwrap();
    assert_eq!((words.len(), words[0], words[28]), (29, u32::MAX, 42));
    assert_eq!(record.components.last().unwrap().shape_ptr, 0x80000000);
    assert!(record.plated);
    assert!(record.restricted_layer_span.is_none());
}

#[test]
fn fonts_distinguish_spacing_word_positions_capacity_and_used_count() {
    for version in [174, 181, 251] {
        let stride = if version == 251 { 64 } else { 68 };
        let (mut bytes, span) = fixture(version, 0x36, 36 + stride * 2);
        put16(&mut bytes, START + 2, 8);
        put32(&mut bytes, START + 16, 2);
        put32(&mut bytes, START + 20, 1);
        let entry = START + 36;
        put32(&mut bytes, entry + 8, 100);
        put32(&mut bytes, entry + 12, 200);
        let spacing = if version == 181 { 20 } else { 16 };
        put32(&mut bytes, entry + spacing, 300);
        put32(&mut bytes, entry + spacing + 4, 400);
        put32(
            &mut bytes,
            entry + if version == 251 { 28 } else { 32 },
            500,
        );
        let VariableRecord::DefinitionTable(record) = variable(&bytes, &span) else {
            panic!()
        };
        assert_eq!(record.items_offset, START + 36);
        assert_eq!(record.stride, stride);
        let fonts = record.fonts.unwrap();
        assert_eq!(fonts.len(), 1);
        assert_eq!(
            (
                fonts[0].height,
                fonts[0].width,
                fonts[0].character_space,
                fonts[0].line_space,
                fonts[0].stroke_width
            ),
            (100, 200, 300, 400, 500)
        );
    }
}

#[test]
fn variable_text_preserves_unicode_signed_coordinates_and_the_null_termination_rule() {
    let (mut bytes, span) = fixture(174, 0x31, 36);
    put32(&mut bytes, START + 12, (-123_i32) as u32);
    put16(&mut bytes, START + 22, 8);
    bytes[START + 28..START + 34].copy_from_slice("走线".as_bytes());
    bytes[START + 35] = 0xff;
    let VariableRecord::TextGraphic(record) = variable(&bytes, &span) else {
        panic!()
    };
    assert_eq!((record.coords_x, record.value.as_str()), (-123, "走线"));
}

#[test]
fn record_and_collection_budgets_fail_before_decoding_or_allocating_hostile_counts() {
    let (mut bytes, span) = fixture(174, 0x3c, 28);
    put32(&mut bytes, START + 12, 3);
    assert!(matches!(
        decode(
            &bytes,
            &span,
            &DecodeLimits {
                max_allocation_bytes: 4,
                ..Default::default()
            }
        ),
        Err(ImportError::DecodeLimit {
            actual: 12,
            limit: 4,
            ..
        })
    ));
    assert!(matches!(
        decode(
            &bytes,
            &span,
            &DecodeLimits {
                max_entries: 2,
                ..Default::default()
            }
        ),
        Err(ImportError::InvalidRecord {
            field: "COLLECTION_COUNT",
            value: 3,
            ..
        })
    ));
    put32(&mut bytes, START + 12, 1_000_001);
    assert!(matches!(
        decode(&bytes, &span, &DecodeLimits::default()),
        Err(ImportError::InvalidRecord {
            field: "REFERENCE_COUNT",
            ..
        })
    ));
    let (mut bytes, span) = fixture(174, 0x31, 36);
    put16(&mut bytes, START + 22, 8);
    assert!(matches!(
        decode(
            &bytes,
            &span,
            &DecodeLimits {
                max_text_bytes: 7,
                ..Default::default()
            }
        ),
        Err(ImportError::DecodeLimit {
            offset: START,
            actual: 8,
            limit: 7
        })
    ));
}

#[test]
fn opaque_constraint_extent_consumes_no_allocation_budget_but_stays_inside_its_span() {
    let (bytes, span) = fixture(174, 0x27, 4096);
    let mut header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    header.constraint_end = (START + 4097) as u32;
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let limits = DecodeLimits {
        max_allocation_bytes: 0,
        max_text_bytes: 0,
        max_entries: 0,
    };
    assert!(
        RecordDecoder::new(&bytes, &header, TextEncoding::Utf8)
            .decode(&span, &limits, &context)
            .is_ok()
    );
    header.constraint_end += 4;
    assert!(matches!(
        RecordDecoder::new(&bytes, &header, TextEncoding::Utf8).decode(&span, &limits, &context),
        Err(ImportError::InvalidRecord {
            field: "RECORD_BOUNDARY",
            ..
        })
    ));
}

#[test]
fn every_variable_layout_obeys_version_specific_empty_boundaries() {
    for version in VERSIONS {
        for kind in [
            3, 0x1a, 0x1c, 0x1d, 0x1e, 0x1f, 0x21, 0x27, 0x2a, 0x31, 0x36, 0x3b, 0x3c,
        ] {
            let size = match kind {
                3 => 16 + usize::from(version >= 172) * 8,
                0x1a => 88 + usize::from(version >= 174) * 4,
                0x1c => {
                    if version < 165 {
                        360
                    } else if version < 172 {
                        392
                    } else if version < 180 {
                        948
                    } else {
                        980
                    }
                }
                0x1d => {
                    if version < 160 {
                        32
                    } else if version < 172 {
                        24
                    } else {
                        28
                    }
                }
                0x1e => {
                    if version >= 251 {
                        32
                    } else if version >= 172 {
                        28
                    } else if version == 160 {
                        20
                    } else {
                        24
                    }
                }
                0x1f => {
                    if version < 160 {
                        68
                    } else if version < 172 {
                        32
                    } else {
                        36
                    }
                }
                0x21 => 12,
                0x27 => 4,
                0x2a => 8 + usize::from(version >= 174) * 4,
                0x31 => 24 + usize::from(version >= 174) * 4,
                0x36 => 28 + usize::from(version >= 172) * 4 + usize::from(version >= 174) * 4,
                0x3b => 176 + usize::from(version >= 172) * 4,
                0x3c => 12 + usize::from(version >= 174) * 4,
                _ => unreachable!(),
            };
            let (mut bytes, span) = fixture(version, kind, size);
            match kind {
                3 => bytes[START + if version >= 172 { 16 } else { 12 }] = 0x65,
                0x21 => put32(&mut bytes, START + 4, 12),
                0x27 => {
                    // constraintEnd is read from the header; set it after decoding the other fields.
                    let mut header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
                    header.constraint_end = (START + 5) as u32;
                    let cancellation = CancellationToken::default();
                    assert!(
                        RecordDecoder::new(&bytes, &header, TextEncoding::Utf8)
                            .decode(
                                &span,
                                &DecodeLimits::default(),
                                &ImportContext {
                                    cancellation: &cancellation,
                                    progress: &|_| {}
                                }
                            )
                            .is_ok()
                    );
                    continue;
                }
                0x36 => put16(&mut bytes, START + 2, 8),
                _ => {}
            }
            let result = decode(&bytes, &span, &DecodeLimits::default());
            if kind == 0x1a && ![152, 157, 172, 174, 251].contains(&version) {
                assert!(matches!(
                    result,
                    Err(ImportError::UnsupportedRecordLayout { .. })
                ));
            } else {
                assert!(
                    result.is_ok(),
                    "version={version}, kind={kind:x}, error={result:?}"
                );
            }
        }
    }
}

#[test]
fn every_prefix_of_a_counted_key_list_is_rejected_and_cancellation_precedes_decode() {
    let (mut bytes, span) = fixture(174, 0x3c, 28);
    put32(&mut bytes, START + 12, 3);
    put32(&mut bytes, START + 16, u32::MAX);
    for length in 1..28 {
        let short = RecordSpan {
            byte_length: length,
            ..span
        };
        assert!(decode(&bytes, &short, &DecodeLimits::default()).is_err());
    }
    let header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        RecordDecoder::new(&bytes, &header, TextEncoding::Utf8).decode(
            &span,
            &DecodeLimits::default(),
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {}
            }
        ),
        Err(ImportError::Cancelled)
    ));
}
