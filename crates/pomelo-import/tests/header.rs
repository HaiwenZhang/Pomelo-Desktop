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
