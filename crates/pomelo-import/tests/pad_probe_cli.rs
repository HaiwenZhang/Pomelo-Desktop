use pomelo_core::i18n::{Locale, MessageKey, text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const START: usize = 0x1200;
const STACK_SIZE: usize = 192 + 25 * 36;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn source() -> Vec<u8> {
    let mut bytes = vec![0; START + 24 + STACK_SIZE + 4];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    put(&mut bytes, 0x428 + 6 * 8 + 4, 77);
    bytes[START] = 0x2a;
    bytes[START + 2] = 1;
    put(&mut bytes, START + 12, 0x8000);
    put(&mut bytes, START + 20, 77);
    let stack = START + 24;
    bytes[stack] = 0x1c;
    put(&mut bytes, stack + 4, 42);
    bytes[stack + 28] = 4;
    bytes[stack + 30] = 0x20;
    bytes[stack + 44] = 1;
    put(&mut bytes, stack + 64, 200);
    let pad = stack + 192 + 23 * 36;
    bytes[pad] = 29;
    put(&mut bytes, pad + 8, 1000);
    put(&mut bytes, pad + 12, 2000);
    bytes
}
fn probe(source: &Path, request: &Path, report: &Path, locale: Locale) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", locale.tag(), "decode-padstack"])
        .arg(source)
        .arg("--records")
        .arg(request)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap()
}

#[test]
fn pad_probe_preserves_definition_and_warning_identity_while_translating_five_locales() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("焊盘.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("padstack.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(&request, serde_json::to_vec(&json!({"schema_version":1,"sha256":hex::encode(Sha256::digest(&bytes)),"source_size":bytes.len(),"spans":[
        {"offset":START+24,"byte_length":STACK_SIZE,"key":42,"record_type":28},
        {"offset":START+24,"byte_length":40,"key":42,"record_type":28}
    ]})).unwrap()).unwrap();
    let mut expected = None;
    let mut messages = std::collections::BTreeSet::new();
    for locale in Locale::ALL {
        let result = probe(&source_path, &request, &report, locale);
        assert!(!result.status.success());
        let rows: Vec<Value> = fs::read_to_string(&report)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["stage"], "padstack");
        assert_eq!(rows[0]["scene_validated"], false);
        assert_eq!(
            rows[1]["padstack"]["definition"]["regular_pads"][0]["kind"],
            29
        );
        assert_eq!(rows[1]["padstack"]["definition"]["drill"]["width"], 0.2);
        assert_eq!(rows[2]["error"]["code"], "BRD_INVALID_RECORD");
        assert_eq!(rows[3]["diagnostics"][0]["code"], "BRD_PAD_UNSUPPORTED");
        assert_eq!(rows[3]["diagnostics"][0]["object"], 42);
        assert_eq!(rows[3]["diagnostics"][0]["offset"], START + 24);
        assert_eq!(rows[3]["diagnostics"][0]["message"]["args"]["pad_type"], 29);
        messages.insert(
            rows[3]["localized_messages"][0]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        let stable = json!({"metadata":rows[0],"pads":rows[1]["padstack"],"error":rows[2]["error"],"warnings":rows[3]["diagnostics"]});
        if let Some(expected) = &expected {
            assert_eq!(&stable, expected);
        } else {
            expected = Some(stable);
        }
    }
    assert_eq!(messages.len(), 5);
    assert_eq!(fs::read(&source_path).unwrap(), bytes);
}

#[test]
fn pad_probe_rejects_changed_source_before_overwriting_existing_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("board.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("padstack.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(&report, "previous evidence\n").unwrap();
    fs::write(&request, serde_json::to_vec(&json!({"schema_version":1,"sha256":"0".repeat(64),"source_size":bytes.len(),"spans":[]})).unwrap()).unwrap();
    for locale in Locale::ALL {
        let output = probe(&source_path, &request, &report, locale);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains(&text(locale, MessageKey::CliSourceMismatch))
        );
        assert_eq!(fs::read_to_string(&report).unwrap(), "previous evidence\n");
    }
}
