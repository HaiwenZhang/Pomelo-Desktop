//! Baked Unicode collation, independent of UI translations and native APIs.

use icu_collator::{
    Collator,
    options::{AlternateHandling, CollatorOptions, Strength},
};
use icu_locale_core::locale;
use icu_provider_blob::BlobDataProvider;

use crate::{
    i18n::{Locale, MessageKey},
    model::Diagnostic,
};

#[derive(Debug, thiserror::Error)]
#[error("SEARCH_COLLATION_UNAVAILABLE")]
pub struct CollationError {
    locale: Locale,
    details: Box<str>,
}

impl CollationError {
    pub fn diagnostic(&self) -> Diagnostic {
        let mut diagnostic = Diagnostic::error(
            "SEARCH_COLLATION_UNAVAILABLE",
            MessageKey::SearchCollationUnavailable,
        )
        .with_details(self.details.to_string());
        diagnostic.message = diagnostic.message.arg("locale", self.locale.tag());
        diagnostic
    }
}

pub(super) fn options() -> CollatorOptions {
    let mut options = CollatorOptions::default();
    // Intl.Collator's default sort uses variant sensitivity and keeps punctuation.
    options.strength = Some(Strength::Tertiary);
    options.alternate_handling = Some(AlternateHandling::NonIgnorable);
    options
}

pub(super) fn for_locale(language: Locale) -> Result<Collator, CollationError> {
    let locale = match language {
        Locale::English => locale!("en"),
        Locale::SimplifiedChinese => locale!("zh-CN"),
        Locale::TraditionalChinese => locale!("zh-TW"),
        Locale::Japanese => locale!("ja"),
        Locale::Korean => locale!("ko"),
    };
    // ICU4X's built-in data uses implicit Han order; ECMAScript's ICU uses
    // Unihan. This blob freezes the same root and locale tailorings together.
    let provider =
        BlobDataProvider::try_new_from_static_blob(include_bytes!("data/unihan.postcard"))
            .map_err(|error| CollationError {
                locale: language,
                details: error.to_string().into_boxed_str(),
            })?;
    Collator::try_new_with_buffer_provider(&provider, locale.into(), options()).map_err(|error| {
        CollationError {
            locale: language,
            details: error.to_string().into_boxed_str(),
        }
    })
}

pub(super) fn query(text: &str) -> String {
    // ECMAScript trim includes BOM and excludes NEL; Rust str::trim differs.
    text.trim_matches(|ch| {
        matches!(ch,
            '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
            '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    })
    .to_lowercase()
}
