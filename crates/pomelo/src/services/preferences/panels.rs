//! Window panel layout preferences.
use super::json_file::JsonFileStore;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPreferences {
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    pub left_width: f32,
    pub right_width: f32,
}
impl Default for PanelPreferences {
    fn default() -> Self {
        Self {
            left_collapsed: false,
            right_collapsed: false,
            left_width: 256.0,
            right_width: 288.0,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelFile {
    schema_version: u32,
    panels: PanelPreferences,
}
#[derive(Clone)]
pub struct PanelStore(pub(super) JsonFileStore);
impl PanelStore {
    pub fn platform_default() -> Option<Self> {
        JsonFileStore::in_configuration_directory("panels.json").map(Self)
    }
    pub fn load(&self) -> Result<PanelPreferences, Diagnostic> {
        let Some(parsed) = self.0.load_file::<PanelFile>()? else {
            return Ok(PanelPreferences::default());
        };
        self.validate(parsed.panels)?;
        if parsed.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.panels)
    }
    fn validate(&self, panels: PanelPreferences) -> Result<(), Diagnostic> {
        if !panels.left_width.is_finite()
            || !panels.right_width.is_finite()
            || !(200.0..=440.0).contains(&panels.left_width)
            || !(240.0..=440.0).contains(&panels.right_width)
        {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "PANEL_LAYOUT_INVALID"));
        }
        Ok(())
    }
    pub fn save(&self, panels: PanelPreferences) -> Result<(), Diagnostic> {
        self.validate(panels)?;
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&PanelFile {
            schema_version: 1,
            panels,
        })
    }
}
