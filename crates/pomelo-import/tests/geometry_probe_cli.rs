use pomelo_core::i18n::{Locale, MessageKey, text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const START: usize = 0x1200;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn source() -> Vec<u8> {
    let mut bytes = vec![0; START + 24 + 44 + 4];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    put(&mut bytes, 0x428 + 6 * 8 + 4, 77);
    bytes[START] = 0x2a;
    bytes[START + 2] = 1;
    put(&mut bytes, START + 8, 999);
    put(&mut bytes, START + 12, 0x8000);
    put(&mut bytes, START + 20, 77);
    bytes[START + 24] = 0x15;
    put(&mut bytes, START + 28, 42);
    put(&mut bytes, START + 24 + 24, 200);
    put(&mut bytes, START + 24 + 28, (-1000_i32) as u32);
    put(&mut bytes, START + 24 + 36, 1000);
    bytes
}
fn probe(source: &Path, request: &Path, report: &Path, locale: Locale) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", locale.tag(), "decode-geometry"])
        .arg(source)
        .arg("--records")
        .arg(request)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap()
}
fn rows(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn geometry_probe_keeps_metadata_source_identity_and_fields_across_five_locales() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("线路.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("geometry.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(&request,serde_json::to_vec(&json!({"schema_version":1,"sha256":format!("{:x}",Sha256::digest(&bytes)),"source_size":bytes.len(),"spans":[
        {"offset":START+24,"byte_length":44,"key":42,"record_type":21},
        {"offset":START+24,"byte_length":40,"key":42,"record_type":21}
    ]})).unwrap()).unwrap();
    let mut expected = None;
    let mut messages = std::collections::BTreeSet::new();
    for locale in Locale::ALL {
        let result = probe(&source_path, &request, &report, locale);
        assert!(!result.status.success());
        let rows = rows(&report);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["kind"], "metadata");
        assert_eq!(rows[0]["scale"], 0.001);
        assert_eq!(rows[0]["layers"][0]["name"], "");
        assert_eq!(rows[0]["layers"][0]["function"], "Conductor");
        assert_eq!(rows[1]["geometry"]["edge"]["a"]["x"], -1.0);
        assert_eq!(rows[1]["geometry"]["edge"]["width"], 0.2);
        assert_eq!(rows[2]["error"]["code"], "BRD_INVALID_RECORD");
        assert_eq!(rows[2]["locale"], locale.tag());
        assert_eq!(rows[0]["scene_validated"], false);
        assert_eq!(rows[1]["scene_validated"], false);
        messages.insert(rows[2]["localized_message"].as_str().unwrap().to_owned());
        let fields =
            json!({"metadata":rows[0],"geometry":rows[1]["geometry"],"error":rows[2]["error"]});
        if let Some(expected) = &expected {
            assert_eq!(&fields, expected);
        } else {
            expected = Some(fields);
        }
    }
    assert_eq!(messages.len(), 5);
    assert_eq!(fs::read(&source_path).unwrap(), bytes);
}

#[test]
fn geometry_probe_rejects_changed_source_before_replacing_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("board.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("geometry.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(&report, "previous evidence\n").unwrap();
    fs::write(&request,serde_json::to_vec(&json!({"schema_version":1,"sha256":"0".repeat(64),"source_size":bytes.len(),"spans":[]})).unwrap()).unwrap();
    for locale in Locale::ALL {
        let result = probe(&source_path, &request, &report, locale);
        assert!(!result.status.success());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains(&text(locale, MessageKey::CliSourceMismatch))
        );
        assert_eq!(fs::read_to_string(&report).unwrap(), "previous evidence\n");
    }
}

#[test]
fn malformed_geometry_request_does_not_create_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("absent.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("geometry.jsonl");
    fs::write(&request, "{broken}").unwrap();
    let result = probe(&source_path, &request, &report, Locale::Japanese);
    assert!(!result.status.success());
    assert!(!report.exists());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains(&text(Locale::Japanese, MessageKey::CliDecodeRequestInvalid))
    );
}
