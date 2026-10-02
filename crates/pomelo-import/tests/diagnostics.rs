use pomelo_core::i18n::Locale;
use pomelo_import::ImportError;

#[test]
fn every_current_import_error_has_complete_translations_and_stable_structured_fields() {
    let errors = [
        ImportError::NoGeometry,
        ImportError::OutOfBounds {
            offset: 128,
            requested: 8,
            length: 132,
        },
        ImportError::UnsupportedMagic(0xdeadbeef),
        ImportError::InvalidDivisor,
        ImportError::UnsupportedUnits {
            units: 255,
            divisor: 1000,
        },
        ImportError::InvalidGeometry {
            key: 42,
            offset: 5120,
            field: "ARC_CENTER",
        },
        ImportError::GeometryLimit {
            offset: 5120,
            actual: 2048,
            limit: 1024,
        },
        ImportError::SemanticCacheLimit {
            offset: 5120,
            actual: 2048,
            limit: 1024,
        },
        ImportError::InvalidEncoding(100),
        ImportError::DuplicateString {
            key: 0,
            offset: 4608,
        },
        ImportError::DuplicateRecord {
            key: u32::MAX,
            offset: 8192,
        },
        ImportError::UnknownRecord {
            record_type: 0x3f,
            offset: 5120,
        },
        ImportError::UnalignedRecord(5121),
        ImportError::InvalidRecord {
            offset: 5120,
            field: "BLOB_LENGTH",
            value: 8,
        },
        ImportError::UnsupportedRecordLayout {
            record_type: 0x1a,
            version: 165,
            offset: 5120,
        },
        ImportError::IndexLimit {
            actual: 2048,
            limit: 1024,
        },
        ImportError::DecodeLimit {
            offset: 5120,
            actual: 2048,
            limit: 1024,
        },
        ImportError::MissingReference {
            key: u32::MAX,
            offset: 5120,
            field: "Next",
        },
        ImportError::ReferenceType {
            key: u32::MAX,
            actual: 4,
            expected: vec![1, 0x15],
            offset: 5120,
            field: "Next",
        },
        ImportError::ReferenceCycle {
            key: u32::MAX,
            offset: 5120,
            field: "Next",
        },
        ImportError::Cancelled,
        ImportError::ResourceLimit {
            actual: 200,
            limit: 100,
        },
        ImportError::Io(std::io::ErrorKind::NotFound.into()),
        ImportError::Io(std::io::ErrorKind::PermissionDenied.into()),
        ImportError::Io(std::io::ErrorKind::UnexpectedEof.into()),
        ImportError::SceneNotImplemented,
        ImportError::CopperLayerUndefined {
            key: 42,
            layer: 255,
            offset: 5120,
        },
        ImportError::CopperMesh {
            key: 42,
            offset: 5120,
            source: pomelo_core::copper::MeshError::Invalid {
                ring: 2,
                field: "COORDINATES",
            },
        },
        ImportError::CopperMesh {
            key: 42,
            offset: 5120,
            source: pomelo_core::copper::MeshError::Limit {
                resource: "bytes",
                actual: 2048,
                limit: 1024,
            },
        },
    ];
    for error in errors {
        let diagnostic = error.diagnostic();
        let before = serde_json::to_value(&diagnostic).unwrap();
        for locale in Locale::ALL {
            let message = diagnostic.message.render(locale).unwrap();
            assert!(
                !message.contains("%{") && !message.starts_with(diagnostic.message.key.as_str())
            );
        }
        assert_eq!(before, serde_json::to_value(&diagnostic).unwrap());
    }
}

#[test]
fn cli_reports_retain_error_codes_and_parameters_across_all_five_languages() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("truncated.brd");
    std::fs::write(&path, [0]).unwrap();
    let mut expected = None;
    let mut translations = std::collections::BTreeSet::new();
    for locale in Locale::ALL {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
            .args(["--locale", locale.tag(), "probe"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["error"]["code"], "IMPORT_OUT_OF_BOUNDS");
        assert_eq!(report["locale"], locale.tag());
        let summary = report["localized_message"].as_str().unwrap();
        assert!(String::from_utf8(output.stderr).unwrap().contains(summary));
        translations.insert(summary.to_owned());
        if let Some(expected) = &expected {
            assert_eq!(&report["error"], expected);
        } else {
            expected = Some(report["error"].clone());
        }
    }
    assert_eq!(translations.len(), 5);
}

#[test]
fn cli_help_is_localized_and_unknown_options_do_not_silently_succeed() {
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", "ja", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout).unwrap().contains("使い方"));
    let invalid = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
        .args(["--locale", "ko", "manifest", "--unexpected", "value"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("사용법")
    );
}

#[test]
fn cli_rejects_unknown_encodings_with_a_message_in_each_supported_language() {
    for locale in Locale::ALL {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pcb_inspect"))
            .args(["--locale", locale.tag(), "--encoding", "unknown", "--help"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(message.contains("unknown") && !message.contains("cli.unsupported_encoding"));
    }
}
