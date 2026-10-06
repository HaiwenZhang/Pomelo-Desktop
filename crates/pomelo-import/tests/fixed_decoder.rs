use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportError, TextEncoding,
    formats::allegro::{
        decoder::{
            RecordDecoder,
            fixed::{FixedRecord, InlineOrReference},
        },
        header::BrdHeader,
        index::{FileOffset, RecordKey, RecordSpan},
    },
};

const OFFSET: usize = 0x1200;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
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
    let mut bytes = vec![0; OFFSET + size];
    put(&mut bytes, 0, magic);
    put(&mut bytes, 0x26c, 1000);
    put(&mut bytes, 0x28c, 1000);
    if version < 160 {
        bytes[OFFSET..OFFSET + 2].copy_from_slice(&(u16::from(kind) << 10).to_le_bytes());
    } else {
        bytes[OFFSET] = kind;
    }
    let key = if kind == 0x35 { 0 } else { u32::MAX };
    if kind != 0x35 {
        put(&mut bytes, OFFSET + 4, key);
    }
    (
        bytes,
        RecordSpan {
            offset: FileOffset(OFFSET as u32),
            byte_length: size as u32,
            key: RecordKey(key),
            record_type: kind,
        },
    )
}
fn decode(bytes: &[u8], span: &RecordSpan) -> Result<FixedRecord, ImportError> {
    let header = BrdHeader::read(bytes, TextEncoding::Utf8)?;
    let cancellation = CancellationToken::default();
    RecordDecoder::new(bytes, &header, TextEncoding::Utf8).decode_fixed(
        span,
        &ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        },
    )
}
fn float(bytes: &mut [u8], offset: usize, value: f64) {
    let bits = value.to_bits();
    put(bytes, offset, (bits >> 32) as u32);
    put(bytes, offset + 4, bits as u32);
}

#[test]
fn legacy_segment_swaps_width_and_flags_and_retains_signed_coordinates() {
    let (mut bytes, span) = fixture(152, 0x15, 40);
    put(&mut bytes, OFFSET + 16, 250);
    put(&mut bytes, OFFSET + 20, 0xdeadbeef);
    put(&mut bytes, OFFSET + 24, (-123_i32) as u32);
    put(&mut bytes, OFFSET + 36, (-456_i32) as u32);
    let FixedRecord::Segment(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert_eq!(
        (record.width, record.flags, record.start_x, record.end_y),
        (250, 0xdeadbeef, -123, -456)
    );
}
#[test]
fn modern_arc_reads_double_words_without_rounding_large_coordinates() {
    let (mut bytes, span) = fixture(174, 1, 84);
    float(&mut bytes, OFFSET + 44, -123456789.125);
    float(&mut bytes, OFFSET + 52, 0.0625);
    float(&mut bytes, OFFSET + 60, 1.25);
    let FixedRecord::Arc(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert_eq!(
        (record.center_x, record.center_y, record.radius),
        (-123456789.125, 0.0625, 1.25)
    );
}
#[test]
fn legacy_arc_normalizes_width_word_and_integer_centers_to_doubles() {
    let (mut bytes, span) = fixture(157, 1, 68);
    put(&mut bytes, OFFSET + 16, 250);
    put(&mut bytes, OFFSET + 20, 99);
    put(&mut bytes, OFFSET + 40, (-250_i32) as u32);
    put(&mut bytes, OFFSET + 48, 3000);
    let FixedRecord::Arc(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert_eq!(
        (
            record.width,
            record.unknown1,
            record.center_x,
            record.radius
        ),
        (250, 99, -250.0, 3000.0)
    );
}
#[test]
fn legacy_flags_keep_all_ten_bits_and_key_keeps_all_32_bits() {
    let (mut bytes, span) = fixture(152, 4, 20);
    bytes[OFFSET..OFFSET + 2].copy_from_slice(&((4_u16 << 10) | 0x2ab).to_le_bytes());
    let FixedRecord::NetAssignment(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert_eq!((record.r#type, record.key), (0x2ab, RecordKey(u32::MAX)));
}
#[test]
fn declared_span_prevents_reading_into_the_following_record() {
    let (bytes, mut span) = fixture(174, 4, 24);
    span.byte_length = 20;
    assert!(matches!(
        decode(&bytes, &span),
        Err(ImportError::InvalidRecord {
            field: "RECORD_BOUNDARY",
            ..
        })
    ));
}
#[test]
fn wrong_identity_and_tag_do_not_decode_as_the_requested_record() {
    let (bytes, mut span) = fixture(174, 4, 24);
    span.key = RecordKey(1);
    assert!(matches!(
        decode(&bytes, &span),
        Err(ImportError::InvalidRecord {
            field: "RECORD_KEY",
            value: 4294967295,
            ..
        })
    ));
    span.record_type = 5;
    assert!(matches!(
        decode(&bytes, &span),
        Err(ImportError::InvalidRecord {
            field: "RECORD_TYPE",
            value: 4,
            ..
        })
    ));
}
#[test]
fn names_preserve_legacy_inline_text_and_modern_string_identity() {
    let (mut bytes, span) = fixture(152, 0x10, 56);
    bytes[OFFSET + 8..OFFSET + 11].copy_from_slice(b"VCC");
    let FixedRecord::FunctionInstance(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert!(matches!(record.function_name,InlineOrReference::Inline(name) if name=="VCC"));
    let (mut bytes, span) = fixture(174, 0x10, 40);
    put(&mut bytes, OFFSET + 28, 0xffffffff);
    let FixedRecord::FunctionInstance(record) = decode(&bytes, &span).unwrap() else {
        panic!()
    };
    assert!(matches!(
        record.function_name,
        InlineOrReference::Reference(u32::MAX)
    ));
}
#[test]
fn footprint_parent_is_normalized_for_legacy_reference_and_missing_fields_remain_absent() {
    let (mut bytes, span) = fixture(166, 0x2d, 64);
    put(&mut bytes, OFFSET + 12, 7);
    let record = decode(&bytes, &span).unwrap();
    let json = serde_json::to_value(record).unwrap();
    assert_eq!(json["InstRef"], 7);
    assert!(json.get("Unknown1").is_none());
}
#[test]
fn component_reference_is_inline_in_v15_and_a_string_key_in_modern_records() {
    let (mut bytes, span) = fixture(157, 7, 64);
    bytes[OFFSET + 8..OFFSET + 12].copy_from_slice(b"U42\0");
    let json = serde_json::to_value(decode(&bytes, &span).unwrap()).unwrap();
    assert_eq!(json["RefDes"], "U42");
    assert!(json.get("RefDesStrPtr").is_none());

    let (mut bytes, span) = fixture(174, 7, 48);
    put(&mut bytes, OFFSET + 28, 0xfedcba98);
    let json = serde_json::to_value(decode(&bytes, &span).unwrap()).unwrap();
    assert_eq!(json["RefDesStrPtr"], 0xfedcba98_u32);
    assert!(json.get("RefDes").is_none());
}
#[test]
fn legacy_early_return_layouts_do_not_synthesize_modern_metadata_fields() {
    for (kind, fields) in [
        (0x10, &["Unknown3"][..]),
        (0x2d, &["Unknown2", "Unknown3"][..]),
        (0x30, &["Unknown5"][..]),
    ] {
        let (bytes, span) = fixture(157, kind, length(kind, 157));
        let json = serde_json::to_value(decode(&bytes, &span).unwrap()).unwrap();
        for field in fields {
            assert!(json.get(field).is_none(), "type={kind:x}, field={field}");
        }
        let (bytes, span) = fixture(174, kind, length(kind, 174));
        let json = serde_json::to_value(decode(&bytes, &span).unwrap()).unwrap();
        for field in fields {
            assert_eq!(json[*field], 0, "type={kind:x}, field={field}");
        }
    }
}
#[test]
fn unsupported_variable_record_and_cancelled_queries_are_explicit() {
    let (bytes, span) = fixture(174, 3, 24);
    assert!(matches!(
        decode(&bytes, &span),
        Err(ImportError::UnsupportedRecordLayout { record_type: 3, .. })
    ));
    let header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        RecordDecoder::new(&bytes, &header, TextEncoding::Utf8).decode_fixed(
            &span,
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {}
            }
        ),
        Err(ImportError::Cancelled)
    ));
}

// Independent byte-length expectations, including inline-string legacy records.
fn length(kind: u8, version: u16) -> usize {
    let modern = version >= 160;
    let v172 = usize::from(version >= 172) * 4;
    let v174 = usize::from(version >= 174) * 4;
    match kind {
        1 => (if modern { 80 } else { 68 }) + v172,
        4 => 20 + v174,
        5 => (if modern { 60 } else { 48 }) + 2 * v172,
        6 => 36 + v172,
        7 => {
            if modern {
                40 + 2 * v172
            } else {
                64
            }
        }
        8 => {
            if modern {
                24 + 2 * v172
            } else {
                52
            }
        }
        9 => (if modern { 44 } else { 36 }) + v172 + v174,
        10 => (if modern { 68 } else { 64 }) + v172 + v174,
        12 => (if modern { 56 } else { 48 }) + 2 * v172 + v174,
        13 => {
            if modern {
                40 + v172 + v174
            } else {
                68
            }
        }
        14 => (if modern { 60 } else { 56 }) + 2 * v172,
        15 => {
            if modern {
                (if version >= 190 { 28 } else { 56 }) + v172 + v174
            } else {
                84
            }
        }
        16 => {
            if modern {
                32 + v172 + v174
            } else {
                56
            }
        }
        17 => {
            if modern {
                24 + v174
            } else {
                52
            }
        }
        18 => 24 + usize::from(version >= 165) * 4 + v174,
        20 => (if modern { 32 } else { 28 }) + v172,
        21..=23 => 40 + v172,
        27 => (if modern { 56 } else { 52 }) + v172,
        32 => {
            if version >= 174 {
                80
            } else {
                40
            }
        }
        34 => 40 + v172,
        35 => (if modern { 68 } else { 64 }) + usize::from(version >= 164) * 16 + v174,
        36 => 52 + v172,
        38 => 20 + v172 + v174,
        40 => (if modern { 68 } else { 64 }) + 2 * v172,
        41 => 56,
        43 => 68 + usize::from(version >= 164) * 4 + v172,
        44 => (if modern { 36 } else { 28 }) + 2 * v172,
        45 => (if modern { 64 } else { 60 }) + 2 * v172,
        46 => 36 + v172,
        47 => 32,
        48 => (if modern { 44 } else { 40 }) + 3 * v172 + v174,
        50 => (if modern { 76 } else { 72 }) + 2 * v172,
        51 => (if modern { 72 } else { 68 }) + 2 * v172,
        52 => (if modern { 32 } else { 28 }) + v172,
        53 => 124,
        55 => 428 + v174,
        56 => {
            if version >= 166 {
                52 + v174
            } else {
                64
            }
        }
        57 => 60,
        58 => 16 + v174,
        62 => 44,
        _ => unreachable!(),
    }
}
#[test]
fn every_fixed_layout_consumes_exact_declared_boundary_in_all_thirteen_versions() {
    for version in [
        152, 157, 160, 162, 164, 165, 166, 172, 174, 175, 180, 181, 251,
    ] {
        for kind in
            (1..=62).filter(|&kind| pomelo_import::formats::allegro::decoder::fixed::supports(kind))
        {
            let (bytes, span) = fixture(version, kind, length(kind, version));
            assert!(
                decode(&bytes, &span).is_ok(),
                "version={version} type=0x{kind:x}: {:?}",
                decode(&bytes, &span)
            );
        }
    }
}
