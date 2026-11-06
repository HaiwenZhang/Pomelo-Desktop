//! Synthetic archive compatibility tests; no proprietary board data is embedded.
use super::*;
use crate::{BoardImporter, ImportOptions, ImportedBoard, formats::FormatImporter};
use pomelo_core::task::CancellationToken;

type Schema = (i32, Vec<(usize, u32)>);
fn int(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}
fn string(value: &str) -> Vec<u8> {
    let mut out = (value.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(value.as_bytes());
    out
}
fn quantity(value: f64) -> Vec<u8> {
    let mut out = value.to_le_bytes().to_vec();
    out.extend(string(""));
    out
}
fn object(schema: i32, fields: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = int(schema);
    for value in fields {
        out.extend(value);
    }
    out
}
fn array(values: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = int(values.len() as i32);
    out.extend(int(5));
    for value in values {
        out.extend(value);
    }
    out
}
fn schemas() -> Vec<Schema> {
    vec![
        (0, vec![(0, 4), (2, 5)]),
        (1, vec![(0, 6)]),
        (2, vec![(1, 6)]),
        (3, vec![(0, 4), (2, 4), (3, 6), (4, 5)]),
        (4, vec![(1, 6), (4, 6), (5, 6), (6, 6)]),
        (5, vec![(0, 1)]),
        (7, vec![(0, 5), (1, 4)]),
        (10, vec![(0, 5), (1, 1), (2, 1)]),
        (11, vec![(0, 5), (1, 1), (4, 1)]),
        (
            12,
            vec![
                (0, 5),
                (1, 1),
                (2, 3),
                (3, 3),
                (4, 3),
                (5, 3),
                (6, 3),
                (7, 3),
            ],
        ),
    ]
}

fn body(extension: bool) -> Vec<u8> {
    let base = object(10, vec![object(5, vec![int(2)]), int(1), int(-1)]);
    let primitive = object(
        12,
        vec![
            object(11, vec![base, int(0), int(-1)]),
            int(2),
            quantity(0.0),
            quantity(0.0),
            quantity(0.001),
            quantity(0.001),
            quantity(0.0),
            quantity(0.0),
        ],
    );
    let layout = object(
        4,
        vec![
            array(vec![object(
                7,
                vec![object(5, vec![int(1)]), string("NET")],
            )]),
            array(vec![primitive]),
            array(vec![]),
            array(vec![]),
        ],
    );
    let cell = object(
        3,
        vec![
            string("$begin 'cell'\ndn='Synthetic'\n$end 'cell'"),
            string("Layer(ID='0',N='Top',T='signal')"),
            array(vec![]),
            layout,
        ],
    );
    let groups = object(1, vec![array(vec![object(2, vec![array(vec![cell])])])]);
    let mut fields = vec![string("$begin 'EDB'\n$end 'EDB'"), groups];
    if extension {
        fields.push(object(100, vec![string("future metadata"), quantity(2.5)]));
    }
    object(0, fields)
}
fn archive(header: &str, schemas: &[Schema], body: Vec<u8>) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(string(header));
    out.extend(int(-1));
    out.extend(int(schemas.len() as i32));
    for (id, fields) in schemas {
        out.extend(int(*id));
        out.extend(int(fields.len() as i32));
        for (field, kind) in fields {
            out.extend(int(*field as i32));
            out.extend(int(*kind as i32));
        }
    }
    out.extend(int(-1));
    out.extend(int(1));
    out.extend(body);
    out.extend(int(-1));
    out
}
fn header(version: &str) -> String {
    format!("$begin 'Hdr'\nVersion='{version}'\nEncrypted=false\n$end 'Hdr'\n")
}
fn import(bytes: &[u8]) -> Result<ImportedBoard, ImportError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edb.def");
    std::fs::write(&path, bytes).unwrap();
    FormatImporter.import(
        &path,
        &ImportOptions::default(),
        &ImportContext {
            cancellation: &CancellationToken::default(),
            progress: &|_| {},
        },
    )
}
#[test]
fn compatible_observed_and_unseen_versions_import_the_same_copper_and_report_actual_version() {
    // 9.4/11.0/13.2/99.0 are synthetic capability tests, not claims about real releases.
    for version in ["7.3", "8.6", "10.6", "12.1", "9.4", "11.0", "13.2", "99.0"] {
        let board = import(&archive(&header(version), &schemas(), body(false))).unwrap();
        assert_eq!(board.source.writer_version, version);
        assert_eq!(board.scene.nets[&NetId(1)], "NET");
        assert_eq!(board.scene.zones.len(), 1);
        let zone = &board.scene.zones[0];
        assert_eq!(zone.net, NetId(1));
        assert!(
            zone.mesh
                .covers_fill(Point::new(0.5, 0.5), &CancellationToken::default())
                .unwrap()
        );
        assert!(
            !zone
                .mesh
                .covers_fill(Point::new(1.5, 0.5), &CancellationToken::default())
                .unwrap()
        );
    }
}
#[test]
fn new_fields_and_object_types_decode_from_their_own_schema() {
    let mut schemas = schemas();
    schemas[0].1.push((60, 5));
    schemas.push((100, vec![(3, 4), (7, 3)]));
    let board = import(&archive(&header("13.2"), &schemas, body(true))).unwrap();
    assert_eq!(board.scene.zones.len(), 1);
}
#[test]
fn schema_declarations_and_encoded_fields_can_be_reordered() {
    let mut schemas = schemas();
    schemas[0].1.reverse();
    schemas.reverse();
    let original = body(false); // Root schema ID occupies four bytes; its text occupies the next block.
    let text_len = 4 + "$begin 'EDB'\n$end 'EDB'".len();
    let mut reordered = int(0);
    reordered.extend_from_slice(&original[4 + text_len..]);
    reordered.extend_from_slice(&original[4..4 + text_len]);
    assert_eq!(
        import(&archive(&header("13.2"), &schemas, reordered))
            .unwrap()
            .scene
            .zones
            .len(),
        1
    );
}
#[test]
fn changed_core_field_encoding_is_rejected_even_for_an_observed_version() {
    let mut schemas = schemas();
    schemas[0].1[0].1 = 2;
    let result = import(&archive(&header("12.1"), &schemas, body(false)));
    let error = result.err().unwrap().to_string();
    assert!(
        error.contains("Incompatible DEF schema 0 field 0") && error.contains('@'),
        "{error}"
    );
}
#[test]
fn missing_core_field_is_rejected_instead_of_filled_with_a_default() {
    let mut schemas = schemas();
    schemas[0].1.pop();
    assert!(
        import(&archive(&header("13.2"), &schemas, body(false)))
            .err()
            .unwrap()
            .to_string()
            .contains("schema 0 field 2")
    );
}
#[test]
fn encryption_and_malformed_versions_remain_unsupported() {
    for text in [
        header("0.1"),
        header("12"),
        header("12.1.2"),
        header("-1.0"),
        header("12.x"),
        header("12.1").replace("false", "true"),
    ] {
        assert!(import(&archive(&text, &schemas(), body(false))).is_err());
    }
}
#[test]
fn declared_but_unused_new_encoding_is_harmless_and_used_new_encoding_fails() {
    let mut schemas = schemas();
    schemas.push((100, vec![(0, 99)]));
    assert!(import(&archive(&header("13.2"), &schemas, body(false))).is_ok());
    schemas[0].1.push((60, 5));
    let mut bytes = body(false);
    bytes.extend(object(100, vec![vec![0; 8]]));
    let error = import(&archive(&header("13.2"), &schemas, bytes))
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("field type 99"), "{error}");
}
#[test]
fn truncated_extensions_and_trailing_bytes_are_rejected() {
    let bytes = archive(&header("13.2"), &schemas(), body(false));
    assert!(import(&bytes[..bytes.len() - 1]).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(import(&trailing).is_err());
}
