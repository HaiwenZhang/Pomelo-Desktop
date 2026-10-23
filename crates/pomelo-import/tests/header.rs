use pomelo_import::{
    ImportError, TextEncoding,
    formats::allegro::header::{BrdHeader, HEADER_BYTES, resolve_version},
};

fn write(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(magic: u32) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_BYTES];
    write(&mut bytes, 0, magic);
    write(&mut bytes, 0x26c, 1000);
    write(&mut bytes, 0x28c, 1000);
    bytes
}

#[test]
fn supported_layout_families_ignore_low_revision_byte() {
    for (magic, version) in [
        (0x120500, 152),
        (0x120f00, 157),
        (0x130000, 160),
        (0x130400, 162),
        (0x130c00, 164),
        (0x131000, 165),
        (0x131500, 166),
        (0x140400, 172),
        (0x140900, 174),
        (0x141400, 175),
        (0x150000, 180),
        (0x150200, 181),
        (0x160100, 251),
        (0x170300, 261),
    ] {
        assert_eq!(resolve_version(magic | 0x42).unwrap(), version);
    }
}

#[test]
fn legacy_header_reads_tail_first_lists() {
    let mut bytes = fixture(0x131500);
    write(&mut bytes, 0x8c, 111);
    write(&mut bytes, 0x90, 222);
    let header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    assert_eq!((header.text_list.head, header.text_list.tail), (222, 111));
}

#[test]
fn v251_header_reads_relocated_tail_first_lists() {
    let mut bytes = fixture(0x160100);
    write(&mut bytes, 0xb0, 111);
    write(&mut bytes, 0xb4, 222);
    let header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    assert_eq!(
        (header.version, header.text_list.head, header.text_list.tail),
        (251, 222, 111)
    );
}

#[test]
fn zero_divisor_is_rejected() {
    let mut bytes = fixture(0x140400);
    write(&mut bytes, 0x26c, 0);
    assert!(matches!(
        BrdHeader::read(&bytes, TextEncoding::Utf8),
        Err(ImportError::InvalidDivisor)
    ));
}

#[test]
fn v15_layer_map_requires_complete_header() {
    let bytes = fixture(0x120500);
    for length in 0..HEADER_BYTES {
        assert!(
            BrdHeader::read(&bytes[..length], TextEncoding::Utf8).is_err(),
            "accepted truncated prefix {length}"
        );
    }
}

#[test]
fn unknown_magic_is_rejected() {
    assert!(matches!(
        resolve_version(0xdeadbeef),
        Err(ImportError::UnsupportedMagic(0xdeadbeef))
    ));
}

#[test]
fn v261_header_preserves_metadata_lists_and_sentinels() {
    let mut bytes = fixture(0x170300);
    write(&mut bytes, 0x14, 12345);
    write(&mut bytes, 0x28, 0x2000);
    write(&mut bytes, 0x34, 42);
    write(&mut bytes, 0x60, 101);
    write(&mut bytes, 0x64, 999);
    write(&mut bytes, 0x80, 105);
    write(&mut bytes, 0x84, 222);
    write(&mut bytes, 0xb0, 111);
    write(&mut bytes, 0xb4, 333);
    bytes[0x144..0x148].copy_from_slice(b"26.1");
    bytes[0x1ac] = 3;
    write(&mut bytes, 0x26c, 0);
    write(&mut bytes, 0x28c, 10000);
    write(&mut bytes, 0x428, 6);
    write(&mut bytes, 0x42c, 444);
    let header = BrdHeader::read(&bytes, TextEncoding::Utf8).unwrap();
    assert_eq!(header.version, 261);
    assert_eq!(header.writer_version, "26.1");
    assert_eq!(header.object_count, 12345);
    assert_eq!(header.constraint_end, 0x2000);
    assert_eq!(header.string_count, 42);
    assert_eq!((header.units, header.divisor), (3, 10000));
    assert_eq!((header.text_list.head, header.text_list.tail), (333, 111));
    assert_eq!(
        (header.graphic_list.head, header.graphic_list.tail),
        (222, 105)
    );
    assert_eq!(header.sentinel_keys, [101, 105, 111]);
    assert_eq!(
        (header.layer_map[0].class_id, header.layer_map[0].record_id),
        (6, 444)
    );
}
