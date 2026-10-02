use pomelo_core::i18n::{Locale, MessageKey, text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const OFFSET: usize = 0x1200;

fn source() -> Vec<u8> {
    let mut bytes = vec![0; OFFSET + 24];
    for (offset, value) in [(0, 0x140900_u32), (0x26c, 1000), (0x28c, 1000)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[OFFSET] = 4;
    bytes[OFFSET + 1] = 5;
    bytes[OFFSET + 4..OFFSET + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes
}

fn probe(source: &Path, request: &Path, report: &Path, locale: Locale) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", locale.tag(), "decode-fixed"])
        .arg(source)
        .arg("--records")
        .arg(request)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap()
}

#[test]
fn malformed_request_is_localized_before_reading_the_source_or_creating_a_report() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("missing.brd");
    let request = directory.path().join("request.json");
    let report = directory.path().join("report.jsonl");
    fs::write(&request, "{broken}").unwrap();
    for locale in Locale::ALL {
        let output = probe(&source, &request, &report, locale);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains(&text(locale, MessageKey::CliDecodeRequestInvalid))
        );
        assert!(!report.exists());
    }
}

#[test]
fn a_changed_source_is_rejected_without_overwriting_an_existing_report() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("synthetic.brd");
    let request = directory.path().join("request.json");
    let report = directory.path().join("report.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(&report, "previous evidence\n").unwrap();
    fs::write(
        &request,
        serde_json::to_vec(&json!({
            "schema_version": 1, "sha256": "0".repeat(64),
            "source_size": bytes.len(), "spans": []
        }))
        .unwrap(),
    )
    .unwrap();
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
    assert_eq!(fs::read(&source_path).unwrap(), bytes);
}

#[test]
fn successful_fields_and_failed_span_diagnostics_keep_identity_in_all_five_locales() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("synthetic.brd");
    let request = directory.path().join("request.json");
    let report = directory.path().join("report.jsonl");
    let bytes = source();
    fs::write(&source_path, &bytes).unwrap();
    fs::write(
        &request,
        serde_json::to_vec(&json!({
            "schema_version": 1, "sha256": format!("{:x}", Sha256::digest(&bytes)),
            "source_size": bytes.len(), "spans": [
                {"offset": OFFSET, "byte_length": 24, "key": u32::MAX, "record_type": 4},
                {"offset": OFFSET, "byte_length": 20, "key": u32::MAX, "record_type": 4}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let mut expected = None;
    let mut translations = std::collections::BTreeSet::new();
    for locale in Locale::ALL {
        let output = probe(&source_path, &request, &report, locale);
        assert!(!output.status.success());
        let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary["failed"], 1);
        let rows: Vec<Value> = fs::read_to_string(&report)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["fields"]["Key"], u32::MAX);
        assert!(rows[0]["error"].is_null());
        assert_eq!(rows[1]["offset"], OFFSET);
        assert_eq!(rows[1]["error"]["code"], "BRD_INVALID_RECORD");
        assert_eq!(rows[1]["locale"], locale.tag());
        assert!(!rows[1]["scene_validated"].as_bool().unwrap());
        translations.insert(rows[1]["localized_message"].as_str().unwrap().to_owned());
        if let Some(expected) = &expected {
            assert_eq!(&rows[1]["error"], expected);
        } else {
            expected = Some(rows[1]["error"].clone());
        }
    }
    assert_eq!(translations.len(), 5);
    assert_eq!(fs::read(&source_path).unwrap(), bytes);
}

#[test]
fn all_record_cli_decodes_variable_text_with_identical_fields_in_all_five_locales() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("text.brd");
    let request = directory.path().join("request.json");
    let report = directory.path().join("report.jsonl");
    let mut bytes = source();
    bytes.resize(OFFSET + 32, 0);
    bytes[OFFSET] = 0x31;
    bytes[OFFSET + 22..OFFSET + 24].copy_from_slice(&4_u16.to_le_bytes());
    bytes[OFFSET + 28..].copy_from_slice(b"PCB!");
    fs::write(&source_path, &bytes).unwrap();
    fs::write(
        &request,
        serde_json::to_vec(&json!({
            "schema_version": 1, "sha256": format!("{:x}", Sha256::digest(&bytes)),
            "source_size": bytes.len(), "spans": [
                {"offset": OFFSET, "byte_length": 32, "key": u32::MAX, "record_type": 0x31}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let mut expected = None;
    for locale in Locale::ALL {
        let output = Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
            .args(["--locale", locale.tag(), "decode-records"])
            .arg(&source_path)
            .arg("--records")
            .arg(&request)
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let row: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        assert_eq!(row["stage"], "records");
        assert_eq!(row["fields"]["Value"], "PCB!");
        assert_eq!(row["fields"]["Key"], u32::MAX);
        assert_eq!(row["locale"], locale.tag());
        if let Some(expected) = &expected {
            assert_eq!(&row["fields"], expected);
        } else {
            expected = Some(row["fields"].clone());
        }
    }
    assert_eq!(fs::read(&source_path).unwrap(), bytes);
}
