use pomelo_core::i18n::{Locale, MessageKey, text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const START: usize = 0x1200;
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn source() -> Vec<u8> {
    let mut bytes = vec![0; START + 24 + 80 + 68 + 4];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    bytes[0x180] = 3;
    put(&mut bytes, 0x428 + 6 * 8 + 4, 77);
    bytes[START] = 0x2a;
    bytes[START + 2] = 1;
    put(&mut bytes, START + 12, 0x8000);
    put(&mut bytes, START + 20, 77);
    let via = START + 24;
    bytes[via] = 0x33;
    put(&mut bytes, via + 4, 42);
    put(&mut bytes, via + 44, 999);
    let track = via + 80;
    bytes[track] = 5;
    bytes[track + 2..track + 4].copy_from_slice(&0x0906_u16.to_le_bytes());
    put(&mut bytes, track + 4, 90);
    bytes
}
fn probe(source: &Path, request: &Path, report: &Path, locale: Locale) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", locale.tag(), "decode-routing"])
        .arg(source)
        .arg("--records")
        .arg(request)
        .arg("--report")
        .arg(report)
        .output()
        .unwrap()
}

#[test]
fn routing_cli_retains_objects_offsets_and_stable_diagnostics_across_five_languages() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("走线与过孔.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("report.jsonl");
    let bytes = source();
    fs::write(&file, &bytes).unwrap();
    fs::write(&request, serde_json::to_vec(&json!({"schema_version":1,"sha256":hex::encode(Sha256::digest(&bytes)),"source_size":bytes.len(),"spans":[
        {"offset":START+24,"byte_length":80,"key":42,"record_type":51},
        {"offset":START+104,"byte_length":68,"key":90,"record_type":5},
        {"offset":START+24,"byte_length":76,"key":42,"record_type":51}
    ]})).unwrap()).unwrap();
    let mut baseline = None;
    let mut translated = std::collections::BTreeSet::new();
    for locale in Locale::ALL {
        let result = probe(&file, &request, &report, locale);
        assert!(!result.status.success());
        let rows: Vec<Value> = fs::read_to_string(&report)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0]["stage"], "routing");
        assert_eq!(rows[0]["scene_validated"], false);
        assert!(rows[1]["routing"]["via"].is_null());
        assert_eq!(rows[2]["routing"]["segments"], json!([]));
        assert_eq!(rows[3]["error"]["code"], "BRD_INVALID_RECORD");
        assert_eq!(
            rows[4]["diagnostics"][0]["code"],
            "BRD_VIA_DEFINITION_UNSUPPORTED"
        );
        assert_eq!(rows[4]["diagnostics"][0]["object"], 42);
        assert_eq!(rows[4]["diagnostics"][0]["offset"], START + 24);
        assert_eq!(
            rows[4]["diagnostics"][1]["code"],
            "BRD_TRACK_LAYER_UNDEFINED"
        );
        let stable = json!({"metadata":rows[0],"via":rows[1]["routing"],"track":rows[2]["routing"],"error":rows[3]["error"],"warnings":rows[4]["diagnostics"]});
        if let Some(baseline) = &baseline {
            assert_eq!(&stable, baseline);
        } else {
            baseline = Some(stable);
        }
        translated.insert(rows[4]["localized_messages"].to_string());
    }
    assert_eq!(translated.len(), 5);
    assert_eq!(fs::read(&file).unwrap(), bytes);
}

#[test]
fn changed_routing_source_is_rejected_before_replacing_an_existing_report() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("board.brd");
    let request = dir.path().join("request.json");
    let report = dir.path().join("report.jsonl");
    let bytes = source();
    fs::write(&file, &bytes).unwrap();
    fs::write(&report, "previous evidence\n").unwrap();
    fs::write(&request, serde_json::to_vec(&json!({"schema_version":1,"sha256":"0".repeat(64),"source_size":bytes.len(),"spans":[]})).unwrap()).unwrap();
    for locale in Locale::ALL {
        let result = probe(&file, &request, &report, locale);
        assert!(!result.status.success());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains(&text(locale, MessageKey::CliSourceMismatch))
        );
        assert_eq!(fs::read_to_string(&report).unwrap(), "previous evidence\n");
    }
}
