//! Language preference schema and persistence.
use super::json_file::JsonFileStore;
use pomelo_core::i18n::LanguagePreference;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LanguageFile {
    pub(super) schema_version: u32,
    pub(super) language: LanguagePreference,
}

#[derive(Debug, Clone)]
pub struct LanguageStore(pub(super) JsonFileStore);

impl LanguageStore {
    pub fn platform_default() -> Option<Self> {
        JsonFileStore::in_configuration_directory("language.json").map(Self)
    }
    pub fn load(&self) -> Result<LanguagePreference, Diagnostic> {
        let Some(parsed) = self.0.load_file::<LanguageFile>()? else {
            return Ok(LanguagePreference::System);
        };
        if parsed.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.language)
    }

    pub fn save(&self, language: LanguagePreference) -> Result<(), Diagnostic> {
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&LanguageFile {
            schema_version: 1,
            language,
        })
    }
}
