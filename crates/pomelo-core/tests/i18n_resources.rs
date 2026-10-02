use pomelo_core::i18n::{LanguagePreference, Locale, Message, MessageError, MessageKey};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn placeholders(value: &str) -> BTreeSet<&str> {
    value
        .split("%{")
        .skip(1)
        .map(|part| part.split_once('}').expect("unterminated placeholder").0)
        .collect()
}

#[test]
fn five_language_catalogs_match_the_message_registry_and_parameter_schemas() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    let expected: BTreeSet<_> = MessageKey::ALL
        .iter()
        .map(|key| key.as_str().to_owned())
        .collect();
    for locale in Locale::ALL {
        let source = std::fs::read_to_string(root.join(format!("{}.yml", locale.tag()))).unwrap();
        let catalog: BTreeMap<String, serde_json::Value> = serde_saphyr::from_str(&source).unwrap();
        assert_eq!(catalog["_version"], 1);
        assert_eq!(
            catalog
                .keys()
                .filter(|key| *key != "_version")
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected,
            "{}",
            locale.tag()
        );
        for &key in MessageKey::ALL {
            let template = catalog[key.as_str()].as_str().unwrap();
            assert!(
                !template.trim().is_empty() && !template.contains("TODO"),
                "{}:{}",
                locale.tag(),
                key.as_str()
            );
            assert_eq!(
                placeholders(template),
                key.parameters().iter().copied().collect(),
                "{}:{}",
                locale.tag(),
                key.as_str()
            );
            let mut message = Message::new(key);
            let mut expected_message = template.to_owned();
            for &parameter in key.parameters() {
                message = message.arg(parameter, "sample");
                expected_message = expected_message.replace(&format!("%{{{parameter}}}"), "sample");
            }
            assert_eq!(
                message.render(locale).unwrap(),
                expected_message,
                "embedded catalog differs from source: {}:{}",
                locale.tag(),
                key.as_str()
            );
        }
    }
}

#[test]
fn missing_and_unexpected_parameters_are_rejected_instead_of_leaking_templates() {
    assert_eq!(
        Message::new(MessageKey::UnsupportedMagic).render(Locale::Japanese),
        Err(MessageError::MissingArgument("magic"))
    );
    assert_eq!(
        Message::new(MessageKey::InvalidDivisor)
            .arg("extra", "value")
            .render(Locale::Korean),
        Err(MessageError::UnexpectedArgument("extra".into()))
    );
    assert_ne!(
        Message::new(MessageKey::UnsupportedMagic).display(Locale::Japanese),
        MessageKey::UnsupportedMagic.as_str()
    );
}

#[test]
fn source_names_with_placeholder_syntax_are_not_recursively_translated() {
    let value = "Board %{version} 中文 日本語 한국어";
    let message = Message::new(MessageKey::HeaderFormat)
        .arg("format", value)
        .arg("version", 181_u16)
        .arg("writer", "Allegro");
    for locale in Locale::ALL {
        assert!(message.render(locale).unwrap().contains(value));
    }
}

#[test]
fn concurrent_formatting_uses_the_requested_language_without_global_mutation() {
    let expected: Vec<_> = Locale::ALL
        .into_iter()
        .map(|locale| Message::new(MessageKey::InvalidDivisor).display(locale))
        .collect();
    assert_eq!(expected.iter().collect::<BTreeSet<_>>().len(), 5);
    let handles: Vec<_> = Locale::ALL
        .into_iter()
        .zip(expected)
        .map(|(locale, expected)| {
            std::thread::spawn(move || {
                for _ in 0..500 {
                    assert_eq!(
                        Message::new(MessageKey::InvalidDivisor).display(locale),
                        expected
                    );
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn system_locale_mapping_prioritizes_script_and_supports_common_os_tags() {
    for (tag, expected) in [
        ("zh_Hant_CN", Locale::TraditionalChinese),
        ("zh-Hans-TW", Locale::SimplifiedChinese),
        ("ZH_hk.UTF-8", Locale::TraditionalChinese),
        ("zh-MO", Locale::TraditionalChinese),
        ("zh-SG", Locale::SimplifiedChinese),
        ("zh", Locale::SimplifiedChinese),
        ("ja_JP.UTF-8", Locale::Japanese),
        ("ko-KR", Locale::Korean),
        ("en-US", Locale::English),
    ] {
        assert_eq!(Locale::from_system_tag(tag), Some(expected), "{tag}");
    }
    assert_eq!(
        LanguagePreference::System.resolve(Some("de-DE")),
        Locale::English
    );
    assert_eq!(
        LanguagePreference::Explicit(Locale::Korean).resolve(Some("en-US")),
        Locale::Korean
    );
}

#[test]
fn preferences_serialize_to_canonical_tags_and_reject_unknown_values() {
    for locale in Locale::ALL {
        let preference = LanguagePreference::Explicit(locale);
        let encoded = serde_json::to_string(&preference).unwrap();
        assert_eq!(encoded, format!("\"{}\"", locale.tag()));
        assert_eq!(
            serde_json::from_str::<LanguagePreference>(&encoded).unwrap(),
            preference
        );
    }
    assert_eq!(
        serde_json::from_str::<LanguagePreference>("\"system\"").unwrap(),
        LanguagePreference::System
    );
    assert!(serde_json::from_str::<LanguagePreference>("\"unknown\"").is_err());
}

#[test]
fn yaml_reader_rejects_duplicate_message_keys() {
    let duplicate = "hello: first\nhello: second\n";
    assert!(serde_saphyr::from_str::<BTreeMap<String, String>>(duplicate).is_err());
}
