use pomelo_import::{ImportError, TextEncoding, allegro::reader::Reader};

#[test]
fn explicit_source_code_pages_decode_without_using_the_ui_language() {
    for (encoding, bytes, expected) in [
        (TextEncoding::Gbk, [0xd6, 0xd0, 0xce, 0xc4], "中文"),
        (TextEncoding::Big5, [0xa4, 0xa4, 0xa4, 0xe5], "中文"),
        (TextEncoding::ShiftJis, [0x93, 0xfa, 0x96, 0x7b], "日本"),
        (TextEncoding::Windows1252, [0x63, 0x61, 0x66, 0xe9], "café"),
    ] {
        assert_eq!(
            Reader::new(&bytes, encoding).fixed_string(4).unwrap(),
            expected
        );
        assert_eq!(
            TextEncoding::from_tag(encoding.tag()).unwrap().tag(),
            encoding.tag()
        );
    }
}

#[test]
fn utf8_bom_matches_web_text_decoder_without_changing_alignment_or_error_offset() {
    let bytes = [0xef, 0xbb, 0xbf, b'A', 0, 0, 0, 0];
    let mut reader = Reader::new(&bytes, TextEncoding::Utf8);
    assert_eq!(reader.fixed_string(4).unwrap(), "A");
    assert_eq!(reader.offset(), 4);
    assert_eq!(
        Reader::new(&bytes, TextEncoding::Windows1252)
            .fixed_string(4)
            .unwrap(),
        "ï»¿A"
    );
    let invalid = [0xef, 0xbb, 0xbf, 0xff];
    assert!(matches!(
        Reader::new(&invalid, TextEncoding::Utf8).fixed_string(4),
        Err(ImportError::InvalidEncoding(0))
    ));
}

#[test]
fn allegro_float_reads_high_little_endian_word_before_low_word() {
    for value in [0.0_f64, -0.0, 1.25, -123.456, f64::INFINITY] {
        let bits = value.to_bits();
        let bytes: Vec<_> = ((bits >> 32) as u32)
            .to_le_bytes()
            .into_iter()
            .chain((bits as u32).to_le_bytes())
            .collect();
        assert_eq!(
            Reader::new(&bytes, TextEncoding::Utf8)
                .float()
                .unwrap()
                .to_bits(),
            bits
        );
    }
}

#[test]
fn v15_record_tag_preserves_ten_bit_flags() {
    let bytes = ((0x31_u16 << 10) | 0x3ab).to_le_bytes();
    let mut reader = Reader::new(&bytes, TextEncoding::Utf8);
    assert_eq!(
        (
            reader.record_type(152).unwrap(),
            reader.u8().unwrap(),
            reader.offset()
        ),
        (0x31, 0x3ab, 2)
    );
}

#[test]
fn fixed_string_aligns_from_absolute_file_offset() {
    let mut reader = Reader::new(b"xabc\0\0\0\0", TextEncoding::Utf8);
    reader.seek(1).unwrap();
    let value = reader.fixed_string(4).unwrap();
    assert_eq!((value.as_str(), reader.offset()), ("abc", 8));
}

#[test]
fn integer_reads_reject_every_truncated_prefix() {
    for length in 0..4 {
        let bytes = vec![0; length];
        assert!(matches!(
            Reader::new(&bytes, TextEncoding::Utf8).u32(),
            Err(ImportError::OutOfBounds { .. })
        ));
    }
}

#[test]
fn huge_skip_rejects_overflow_without_advancing_cursor() {
    let mut reader = Reader::new(&[1, 2], TextEncoding::Utf8);
    reader.seek(1).unwrap();
    assert!(reader.skip(usize::MAX).is_err());
    assert_eq!(reader.offset(), 1);
}

#[test]
fn invalid_utf8_reports_source_offset() {
    let mut reader = Reader::new(&[1, 0xff, 0, 0], TextEncoding::Utf8);
    reader.seek(1).unwrap();
    assert!(matches!(
        reader.cstring(),
        Err(ImportError::InvalidEncoding(1))
    ));
}
