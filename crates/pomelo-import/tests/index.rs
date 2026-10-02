use pomelo_core::task::{CancellationToken, ImportStage};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions, TextEncoding,
    allegro::index::{BrdIndex, IndexLimits, RecordKey},
};

const START: usize = 0x1200;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn board(version: u16, strings: u32) -> Vec<u8> {
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
    let mut bytes = vec![0; START];
    put32(&mut bytes, 0, magic);
    put32(&mut bytes, 0x26c, 1000);
    put32(&mut bytes, 0x28c, 1000);
    put32(
        &mut bytes,
        if version >= 180 { 0x34 } else { 0x194 },
        strings,
    );
    bytes
}
fn record(kind: u8, version: u16, key: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    if version < 160 {
        put16(&mut bytes, 0, u16::from(kind) << 10);
    } else {
        bytes[0] = kind;
    }
    put32(&mut bytes, 4, key);
    bytes
}
fn string(bytes: &mut Vec<u8>, key: u32, value: &[u8]) {
    bytes.extend_from_slice(&key.to_le_bytes());
    bytes.extend_from_slice(value);
    bytes.push(0);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}
fn scan(bytes: &[u8]) -> Result<BrdIndex, ImportError> {
    let cancellation = CancellationToken::default();
    BrdIndex::read(
        bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        &ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        },
    )
}

#[test]
fn boundaries_and_unsigned_keys_are_exact_for_all_thirteen_families() {
    for version in [
        152, 157, 160, 162, 164, 165, 166, 172, 174, 175, 180, 181, 251,
    ] {
        let length = if version >= 174 { 24 } else { 20 };
        let mut bytes = board(version, 2);
        string(&mut bytes, 0, b"Layer");
        string(&mut bytes, u32::MAX, "日本語".as_bytes());
        let offset = bytes.len();
        bytes.extend(record(4, version, u32::MAX, length));
        bytes.extend(record(4, version, 0, length));
        bytes.extend(record(4, version, 0, length));
        bytes.extend([0; 4]);
        let index = scan(&bytes).unwrap();
        let found = index.record(RecordKey(u32::MAX)).unwrap();
        assert_eq!(
            (found.offset.0 as usize, found.byte_length as usize),
            (offset, length)
        );
        assert_eq!(index.strings[&u32::MAX], "日本語");
        assert_eq!(index.summary().keyed_records, 1);
        assert_eq!(index.records_of_type(4).count(), 3);
        assert!(index.record(RecordKey(0)).is_none());
        assert_eq!(index.end_offset.0 as usize, offset + 3 * length);
    }
}

#[test]
fn every_truncated_fixed_body_is_rejected_even_when_key_is_zero() {
    for version in [152, 174] {
        let length = if version >= 174 { 24 } else { 20 };
        let body = record(4, version, 0, length);
        for end in 1..length {
            let mut bytes = board(version, 0);
            bytes.extend_from_slice(&body[..end]);
            assert!(
                matches!(scan(&bytes), Err(ImportError::OutOfBounds { .. })),
                "version={version} prefix={end}"
            );
        }
    }
}

#[test]
fn duplicate_string_zero_and_duplicate_record_nonzero_retain_source_positions() {
    let mut bytes = board(174, 2);
    string(&mut bytes, 0, b"A");
    string(&mut bytes, 0, b"B");
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::DuplicateString {
            key: 0,
            offset: 4616
        })
    ));
    let mut bytes = board(174, 0);
    bytes.extend(record(4, 174, 9, 24));
    bytes.extend(record(4, 174, 9, 24));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::DuplicateRecord {
            key: 9,
            offset: 4632
        })
    ));
}

#[test]
fn layer_list_key_is_at_the_tail_and_keyless_records_do_not_enter_identity_index() {
    let mut bytes = board(174, 0);
    let mut layers = record(0x2a, 174, 999, 36);
    put16(&mut layers, 2, 2);
    put32(&mut layers, 32, 17);
    bytes.extend(layers);
    bytes.extend(record(0x35, 174, 17, 124));
    let index = scan(&bytes).unwrap();
    assert_eq!(index.record(RecordKey(17)).unwrap().record_type, 0x2a);
    assert!(index.record(RecordKey(999)).is_none());
    assert_eq!(index.records()[1].key, RecordKey(0));
}

#[test]
fn legacy_inline_strings_validate_encoding_instead_of_being_skipped() {
    let mut bytes = board(152, 0);
    let mut component = record(7, 152, 1, 64);
    component[8] = 0xff;
    bytes.extend(component);
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::InvalidEncoding(4616))
    ));
    let options = ImportOptions {
        text_encoding: TextEncoding::Windows1252,
        ..ImportOptions::default()
    };
    let cancellation = CancellationToken::default();
    assert!(
        BrdIndex::read(
            &bytes,
            &options,
            &IndexLimits::default(),
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {}
            }
        )
        .is_ok()
    );
}

#[test]
fn legacy_pin_number_has_three_link_words_after_the_inline_name() {
    let mut bytes = board(152, 0);
    bytes.extend(record(0x08, 152, 1, 52));
    bytes.extend(record(0x11, 152, 2, 52));
    bytes.extend(record(0x0d, 152, 3, 68));
    bytes.extend(record(0x0f, 152, 4, 84));
    bytes.extend(record(0x10, 152, 5, 56));
    bytes.extend(record(4, 152, 6, 20));
    let index = scan(&bytes).unwrap();
    assert_eq!(
        index.record(RecordKey(6)).unwrap().offset.0,
        (START + 312) as u32
    );
    assert_eq!(
        index
            .records()
            .iter()
            .map(|r| r.byte_length)
            .collect::<Vec<_>>(),
        [52, 52, 68, 84, 56, 20]
    );
}

#[test]
fn aligned_zero_gap_resumes_only_in_v18_and_unknown_nonzero_tags_fail() {
    for (version, expected) in [(174, 1), (180, 2)] {
        let mut bytes = board(version, 0);
        bytes.extend(record(4, version, 1, 24));
        bytes.extend([0; 64]);
        bytes.extend(record(4, version, 2, 24));
        assert_eq!(scan(&bytes).unwrap().records().len(), expected);
    }
    let mut bytes = board(180, 0);
    bytes.extend([0; 64]);
    bytes.extend(record(0x3d, 180, 1, 16));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::UnknownRecord {
            record_type: 0x3d,
            offset: 4672
        })
    ));
    let mut bytes = board(174, 0);
    bytes.extend(record(0x3f, 174, 1, 16));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::UnknownRecord {
            record_type: 0x3f,
            offset: 4608
        })
    ));
}

#[test]
fn legacy_padstack_omits_final_z2_and_v251_font_table_uses_64_byte_entries() {
    let mut bytes = board(165, 0);
    bytes.extend(record(0x1c, 165, 7, 88 + 11 * 28 - 4));
    bytes.extend(record(4, 165, 8, 20));
    let index = scan(&bytes).unwrap();
    assert_eq!(
        index.record(RecordKey(8)).unwrap().offset.0,
        (START + 392) as u32
    );
    let mut bytes = board(251, 0);
    let mut fonts = record(0x36, 251, 7, 36 + 2 * 64);
    put16(&mut fonts, 2, 8);
    put32(&mut fonts, 16, 2);
    put32(&mut fonts, 20, 1);
    bytes.extend(fonts);
    bytes.extend(record(4, 251, 8, 24));
    assert_eq!(
        scan(&bytes).unwrap().record(RecordKey(8)).unwrap().offset.0,
        (START + 164) as u32
    );
}

#[test]
fn embedded_binary_is_not_decoded_and_padding_preserves_the_next_record() {
    let mut bytes = board(174, 0);
    let mut property = record(0x3b, 174, 4, 180 + 4);
    property[8..16].copy_from_slice(b"STEP3D_A");
    put32(&mut property, 176, 0x10000);
    property[180..184].copy_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
    bytes.extend(property);
    bytes.extend(record(4, 174, 2, 24));
    let index = scan(&bytes).unwrap();
    assert_eq!(index.records()[0].byte_length, 184);
    assert_eq!(index.summary().keyed_records, 1);
    // Only the declared byte belongs to the asset; its padding is not decoded either.
    put32(&mut bytes, START + 4, 1);
    assert_eq!(
        scan(&bytes).unwrap().record(RecordKey(2)).unwrap().offset.0,
        4792
    );
}

#[test]
fn hostile_counts_fail_before_capacity_allocation_and_budget_is_enforced() {
    let mut bytes = board(174, u32::MAX);
    assert!(matches!(scan(&bytes), Err(ImportError::OutOfBounds { .. })));
    bytes = board(174, 0);
    let mut definition = record(0x36, 174, 1, 36);
    put16(&mut definition, 2, 8);
    put32(&mut definition, 16, u32::MAX);
    bytes.extend(definition);
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::InvalidRecord {
            field: "DEFINITION_CAPACITY",
            ..
        })
    ));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let limits = IndexLimits {
        max_index_bytes: 2048,
        ..IndexLimits::default()
    };
    assert!(matches!(
        BrdIndex::read(&bytes, &ImportOptions::default(), &limits, &context),
        Err(ImportError::InvalidRecord { .. })
    ));
    let mut bytes = board(174, 0);
    bytes.extend(record(4, 174, 1, 24));
    assert!(matches!(
        BrdIndex::read(&bytes, &ImportOptions::default(), &limits, &context),
        Err(ImportError::IndexLimit {
            actual: 2144,
            limit: 2048
        })
    ));
}

#[test]
fn invalid_blob_size_constraint_end_and_unsupported_metadata_are_typed_errors() {
    let mut bytes = board(174, 0);
    bytes.extend(record(0x21, 174, 8, 12));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::InvalidRecord {
            field: "BLOB_LENGTH",
            value: 8,
            ..
        })
    ));
    let mut bytes = board(174, 0);
    bytes.extend(record(0x27, 174, 0, 12));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::InvalidRecord {
            field: "CONSTRAINT_END",
            ..
        })
    ));
    let mut bytes = board(165, 0);
    bytes.extend(record(0x1a, 165, 7, 88));
    assert!(matches!(
        scan(&bytes),
        Err(ImportError::UnsupportedRecordLayout {
            record_type: 0x1a,
            version: 165,
            ..
        })
    ));
}

#[test]
fn string_bounds_and_progress_cancellation_apply_to_indexing_and_file_loading() {
    let mut bytes = board(174, 1);
    string(&mut bytes, 7, b"long text");
    let cancellation = CancellationToken::default();
    let limits = IndexLimits {
        max_text_bytes: 3,
        ..IndexLimits::default()
    };
    assert!(matches!(
        BrdIndex::read(
            &bytes,
            &ImportOptions::default(),
            &limits,
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {}
            }
        ),
        Err(ImportError::IndexLimit {
            actual: 4,
            limit: 3
        })
    ));
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|progress| {
            assert_eq!(progress.stage, ImportStage::Indexing);
            cancellation.cancel();
        },
    };
    assert!(matches!(
        BrdIndex::read(
            &bytes,
            &ImportOptions::default(),
            &IndexLimits::default(),
            &context
        ),
        Err(ImportError::Cancelled)
    ));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.brd");
    std::fs::write(&path, &bytes).unwrap();
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|progress| {
            assert_eq!(progress.stage, ImportStage::Reading);
            cancellation.cancel();
        },
    };
    assert!(matches!(
        pomelo_import::source::read_path(&path, &ImportOptions::default(), &context),
        Err(ImportError::Cancelled)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn cli_index_evidence_contains_exact_fields_and_never_changes_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("fixture.brd");
    let mut bytes = board(174, 1);
    string(&mut bytes, 7, b"GND");
    bytes.extend(record(4, 174, u32::MAX, 24));
    std::fs::write(&source, &bytes).unwrap();
    let report = directory.path().join("report.jsonl");
    let data = directory.path().join("data");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["check", "--stage", "index", "--cases-dir"])
        .arg(directory.path())
        .arg("--report")
        .arg(&report)
        .arg("--index-data-dir")
        .arg(&data)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(result["index"]["keyed_records"], 1);
    let evidence = std::fs::read(data.join("fixture.brd.idx")).unwrap();
    assert_eq!(&evidence[..8], b"PMIDX001");
    assert_eq!(
        u32::from_le_bytes(evidence[24..28].try_into().unwrap()),
        u32::MAX
    );
    assert_eq!(&evidence[40..], b"GND");
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn file_budget_and_pre_cancelled_requests_fail_before_loading_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("budget.brd");
    let bytes = board(174, 0);
    std::fs::write(&path, &bytes).unwrap();
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let options = ImportOptions {
        max_file_bytes: 8,
        ..ImportOptions::default()
    };
    assert!(matches!(
        pomelo_import::source::read_path(&path, &options, &context),
        Err(ImportError::ResourceLimit {
            actual: 4608,
            limit: 8
        })
    ));
    assert!(matches!(
        BrdIndex::read(&bytes, &options, &IndexLimits::default(), &context),
        Err(ImportError::ResourceLimit {
            actual: 4608,
            limit: 8
        })
    ));
    cancellation.cancel();
    assert!(matches!(
        pomelo_import::source::read_path(&directory.path().join("missing.brd"), &options, &context),
        Err(ImportError::Cancelled)
    ));
}
