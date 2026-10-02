use pomelo_core::i18n::Locale;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, process::Command};

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn header() -> Vec<u8> {
    let mut bytes = vec![0; 0x1200];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    bytes
}
fn run(
    locale: Locale,
    command: &str,
    source: &std::path::Path,
    args: &[&std::ffi::OsStr],
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", locale.tag(), command])
        .arg(source)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn full_network_report_keeps_source_identity_and_does_not_claim_scene_validation() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("网络.brd");
    let report = dir.path().join("network.json");
    let mut bytes = header();
    for (key, net, item) in [(7, 99, 20), (20, 0, 0)] {
        let mut record = vec![0; 24];
        record[0] = 4;
        for (at, value) in [(4, key), (12, net), (16, item)] {
            put(&mut record, at, value);
        }
        bytes.extend(record);
    }
    bytes.extend([0; 4]);
    fs::write(&source, &bytes).unwrap();
    assert!(
        run(
            Locale::Japanese,
            "decode-connectivity",
            &source,
            &["--report".as_ref(), report.as_os_str()]
        )
        .status
        .success()
    );
    let data: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(data["network"]["owners"], json!({"20":99}));
    assert_eq!(data["network"]["assignment_count"], 2);
    assert_eq!(data["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(data["scene_validated"], false);
    assert_eq!(fs::read(&source).unwrap(), bytes);
}

#[test]
fn failed_network_reports_localize_errors_without_publishing_partial_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("连接缺失.brd");
    let report = dir.path().join("network.json");
    let mut bytes = header();
    let mut record = vec![0; 24];
    record[0] = 4;
    put(&mut record, 4, 7);
    put(&mut record, 16, 999);
    bytes.extend(record);
    bytes.extend([0; 4]);
    fs::write(&source, &bytes).unwrap();
    let mut messages = std::collections::BTreeSet::new();
    let mut expected = None;
    for locale in Locale::ALL {
        let output = run(
            locale,
            "decode-connectivity",
            &source,
            &["--report".as_ref(), report.as_os_str()],
        );
        assert!(!output.status.success());
        let data: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(data["network"], Value::Null);
        assert_eq!(data["error"]["offset"], 0x1200);
        assert_eq!(data["error"]["code"], "BRD_MISSING_REFERENCE");
        if let Some(previous) = &expected {
            assert_eq!(&data["error"], previous);
        } else {
            expected = Some(data["error"].clone());
        }
        messages.insert(data["localized_message"].as_str().unwrap().to_owned());
    }
    assert_eq!(messages.len(), 5);
    assert_eq!(fs::read(&source).unwrap(), bytes);
}

#[test]
fn report_path_cannot_replace_the_brd_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("保护.brd");
    let mut bytes = header();
    bytes.extend([0; 4]);
    fs::write(&source, &bytes).unwrap();
    assert!(
        !run(
            Locale::English,
            "decode-connectivity",
            &source,
            &["--report".as_ref(), source.as_os_str()]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(&source).unwrap(), bytes);
}

#[test]
fn placement_warning_cli_preserves_source_offsets_and_translates_five_languages() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("引脚.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("placement.jsonl");
    let mut bytes = header();
    put(&mut bytes, 0x428 + 6 * 8 + 4, 77);
    let mut layers = vec![0; 24];
    layers[0] = 0x2a;
    layers[2] = 1;
    put(&mut layers, 12, 0x8000);
    put(&mut layers, 20, 77);
    bytes.extend(layers);
    let mut fp = vec![0; 72];
    fp[0] = 0x2d;
    put(&mut fp, 4, 1);
    put(&mut fp, 48, 4);
    bytes.extend(fp);
    let mut pin = vec![0; 84];
    pin[0] = 0x32;
    put(&mut pin, 4, 4);
    put(&mut pin, 24, 1);
    put(&mut pin, 28, 1);
    put(&mut pin, 36, 999);
    bytes.extend(pin);
    bytes.extend([0; 4]);
    fs::write(&source, &bytes).unwrap();
    fs::write(&request,serde_json::to_vec(&json!({"schema_version":1,"sha256":format!("{:x}",Sha256::digest(&bytes)),"source_size":bytes.len(),
        "spans":[{"offset":0x1200+24,"byte_length":72,"key":1,"record_type":45}]})).unwrap()).unwrap();
    let mut messages = std::collections::BTreeSet::new();
    let mut expected = None;
    for locale in Locale::ALL {
        let output = run(
            locale,
            "decode-placement",
            &source,
            &[
                "--records".as_ref(),
                request.as_os_str(),
                "--report".as_ref(),
                report.as_os_str(),
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows: Vec<Value> = fs::read_to_string(&report)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows[1]["placement"]["pins"], json!([]));
        assert_eq!(
            rows[2]["diagnostics"][0]["code"],
            "BRD_PIN_DEFINITION_UNSUPPORTED"
        );
        assert_eq!(rows[2]["diagnostics"][0]["offset"], 0x1200 + 96);
        assert_eq!(rows[2]["diagnostics"][0]["object"], 4);
        if let Some(previous) = &expected {
            assert_eq!(&rows[2]["diagnostics"], previous);
        } else {
            expected = Some(rows[2]["diagnostics"].clone());
        }
        messages.insert(
            rows[2]["localized_messages"][0]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert_eq!(messages.len(), 5);
    assert_eq!(fs::read(&source).unwrap(), bytes);
}
