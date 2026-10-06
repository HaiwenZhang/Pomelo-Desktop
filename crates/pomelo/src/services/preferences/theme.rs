//! Theme preference schema and persistence.
use super::json_file::JsonFileStore;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    Dark,
    Light,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    schema_version: u32,
    theme: ThemePreference,
}

#[derive(Clone)]
pub struct ThemeStore(pub(super) JsonFileStore);

impl ThemeStore {
    pub fn platform_default() -> Option<Self> {
        JsonFileStore::in_configuration_directory("theme.json").map(Self)
    }

    pub fn load(&self) -> Result<ThemePreference, Diagnostic> {
        let Some(parsed) = self.0.load_file::<ThemeFile>()? else {
            return Ok(ThemePreference::default());
        };
        if parsed.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.theme)
    }

    pub fn save(&self, theme: ThemePreference) -> Result<(), Diagnostic> {
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&ThemeFile {
            schema_version: 1,
            theme,
        })
    }
}
