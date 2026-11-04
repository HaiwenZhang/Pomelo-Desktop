//! Synthetic Allegro 18.1 coverage without distributing source board data.
use pomelo_core::{
    model::{LayerId, NetId, ObjectId, Point},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportError, ImportOptions,
    formats::{
        FormatImporter,
        allegro::{
            database::BrdDatabase,
            decoder::{DecodeLimits, DecodedRecord, variable::VariableRecord},
            header::BrdHeader,
            index::{BrdIndex, IndexLimits, RecordKey},
        },
    },
};

const START: usize = 0x1200;
const SENTINEL: u32 = 0x8000_0010;

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn record(kind: u8, key: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = kind;
    put(&mut bytes, 4, key);
    bytes
}

fn context(token: &CancellationToken) -> ImportContext<'_> {
    ImportContext {
        cancellation: token,
        progress: &|_| {},
    }
}

fn source() -> Vec<u8> {
    let mut bytes = vec![0; START];
    for (offset, value) in [
        (0, 0x150242),
        (0x14, 9),
        (0x34, 2),
        (0x40, SENTINEL),
        (0x84, 0),
        (0x88, 888),
        (0xb4, 10),
        (0xb8, 999),
        (0x28c, 1000),
        (0x428 + 6 * 8 + 4, 9_000_000),
    ] {
        put(&mut bytes, offset, value);
    }
    bytes[0x144..0x148].copy_from_slice(b"18.1");
    bytes[0x1ac] = 3;
    for (key, text) in [(1_u32, "TOP"), (2, "DATA_P")] {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
    }

    // 0x1A precedes all geometry, so a wrong size also breaks the following records.
    let mut paired = record(0x1a, 100, 92);
    put(&mut paired, 8, SENTINEL);
    for index in 0..20 {
        put(&mut paired, 12 + index * 4, 0xa000_0000 + index as u32);
    }
    bytes.extend(paired);
    // V18 permits aligned zero gaps between record groups.
    bytes.extend([0; 16]);

    let mut layers = vec![0; 24];
    layers[0] = 0x2a;
    layers[2] = 1;
    put(&mut layers, 8, 1);
    put(&mut layers, 12, 0x8000);
    put(&mut layers, 20, 9_000_000);
    bytes.extend(layers);

    let mut net = record(0x1b, 99, 60);
    put(&mut net, 12, 2);
    bytes.extend(net);
    let mut assignment = record(4, 90, 24);
    put(&mut assignment, 12, 99);
    put(&mut assignment, 16, 2);
    bytes.extend(assignment);
    let mut track = record(5, 2, 68);
    track[2] = 6;
    put(&mut track, 8, SENTINEL);
    put(&mut track, 56, 3);
    bytes.extend(track);
    let mut edge = record(0x16, 3, 44);
    for (offset, value) in [
        (8, 2),
        (24, 100),
        (28, 1000),
        (32, 2000),
        (36, 3000),
        (40, 4000),
    ] {
        put(&mut edge, offset, value);
    }
    bytes.extend(edge);

    let mut font = record(0x36, 1, 104);
    font[2] = 8;
    for (offset, value) in [
        (16, 1),
        (20, 1),
        (44, 1000),
        (48, 500),
        (56, 30),
        (60, 40),
        (68, 10),
    ] {
        put(&mut font, offset, value);
    }
    bytes.extend(font);
    let mut wrapper = record(0x30, 10, 60);
    wrapper[2..4].copy_from_slice(&0xf901_u16.to_le_bytes());
    for (offset, value) in [(8, 999), (20, 1), (32, 20), (44, 1000), (48, 2000)] {
        put(&mut wrapper, offset, value);
    }
    bytes.extend(wrapper);
    let mut text = record(0x31, 20, 36);
    text[22] = 5;
    text[28..32].copy_from_slice(b"18.1");
    bytes.extend(text);
    bytes.extend([0; 4]);
    bytes
}

#[test]
fn v181_header_preserves_relocated_metadata_head_first_lists_and_sentinels() {
    let options = ImportOptions::default();
    let header = BrdHeader::read(&source(), options.text_encoding).unwrap();
    assert_eq!(header.version, 181);
    assert_eq!(header.writer_version, "18.1");
    assert_eq!((header.units, header.divisor), (3, 1000));
    assert_eq!((header.text_list.head, header.text_list.tail), (10, 999));
    assert_eq!(
        (header.graphic_list.head, header.graphic_list.tail),
        (0, 888)
    );
    assert_eq!(header.sentinel_keys, [SENTINEL, 888, 999]);
}

#[test]
fn v181_paired_nets_preserve_payload_and_following_record_boundaries() {
    let token = CancellationToken::default();
    let database = BrdDatabase::read(
        source(),
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context(&token),
    )
    .unwrap();
    let record = database
        .get(RecordKey(100), &context(&token))
        .unwrap()
        .unwrap();
    assert_eq!(record.span.byte_length, 92);
    let DecodedRecord::Variable(VariableRecord::PairedNets(paired)) = record.fields else {
        panic!("expected paired-net payload");
    };
    assert_eq!(paired.unknown, Some(SENTINEL));
    assert_eq!(
        (paired.members[0].net, paired.members[1].net),
        (0xa000_0000, 0xa000_000a)
    );
    assert_eq!(paired.members[1].metadata.last(), Some(&0xa000_0013));
    assert_eq!(database.index().records().len(), 9);
}

#[test]
fn v181_truncated_paired_net_records_are_rejected() {
    let bytes = source();
    let header = BrdHeader::read(&bytes, ImportOptions::default().text_encoding).unwrap();
    let token = CancellationToken::default();
    let index = BrdIndex::read(
        &bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        &context(&token),
    )
    .unwrap();
    let start = index.record(RecordKey(100)).unwrap().offset.0 as usize;
    assert_eq!(header.version, 181);
    for length in 1..92 {
        assert!(
            BrdIndex::read(
                &bytes[..start + length],
                &ImportOptions::default(),
                &IndexLimits::default(),
                &context(&token)
            )
            .is_err(),
            "accepted prefix {length}"
        );
    }
}

#[test]
fn v181_imports_tracks_net_names_text_and_header_sentinels_into_shared_scene() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board-18.1.brd");
    let bytes = source();
    std::fs::write(&path, &bytes).unwrap();
    let token = CancellationToken::default();
    let importer = FormatImporter;
    let board = importer
        .import(&path, &ImportOptions::default(), &context(&token))
        .unwrap();
    assert_eq!(board.source.layout_version, 181);
    assert_eq!(board.source.writer_version, "18.1");
    assert_eq!(board.scene.layers[0].name, "TOP");
    assert_eq!(board.scene.nets[&NetId(99)], "DATA_P");
    assert!(
        board.scene.diagnostics.is_empty(),
        "{:?}",
        board.scene.diagnostics
    );
    assert_eq!(board.scene.segments.len(), 1);
    let edge = &board.scene.segments[0];
    assert_eq!(
        (edge.id, edge.track_id, edge.net, edge.layer),
        (ObjectId(3), ObjectId(2), NetId(99), LayerId(0))
    );
    assert_eq!(board.scene.texts.len(), 1);
    let text = &board.scene.texts[0];
    assert_eq!(text.text, "18.1");
    assert_eq!((text.spacing, text.line_spacing), (0.03, 0.04));
    assert_eq!(board.scene.bounds.min, Point::new(0.95, 1.95));
    assert_eq!(board.scene.bounds.max, Point::new(3.05, 4.05));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    token.cancel();
    assert!(matches!(
        importer.import(&path, &ImportOptions::default(), &context(&token)),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn v180_imports_the_same_paired_net_layout_with_its_own_header_offsets() {
    let mut bytes = source();
    put(&mut bytes, 0, 0x150042);
    bytes[0x124..0x128].copy_from_slice(b"18.0");
    bytes[0x18c] = 3;
    put(&mut bytes, 0x26c, 1000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board-18.0.brd");
    std::fs::write(&path, &bytes).unwrap();
    let token = CancellationToken::default();
    let board = FormatImporter
        .import(&path, &ImportOptions::default(), &context(&token))
        .unwrap();
    assert_eq!(board.source.layout_version, 180);
    assert_eq!(board.source.writer_version, "18.0");
    assert_eq!(board.scene.segments.len(), 1);
    assert_eq!(board.scene.texts.len(), 1);
    assert!(board.scene.diagnostics.is_empty());
}

#[test]
fn v181_unknown_chain_links_are_not_treated_as_header_sentinels() {
    let token = CancellationToken::default();
    let mut bytes = source();
    let index = BrdIndex::read(
        &bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        &context(&token),
    )
    .unwrap();
    let track = index.record(RecordKey(2)).unwrap().offset.0 as usize;
    put(&mut bytes, track + 8, SENTINEL + 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing-link-18.1.brd");
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        FormatImporter.import(&path, &ImportOptions::default(), &context(&token)),
        Err(ImportError::MissingReference { key, .. }) if key == SENTINEL + 1
    ));
}
