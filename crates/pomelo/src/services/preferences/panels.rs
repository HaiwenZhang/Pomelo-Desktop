//! Window panel layout preferences.
use super::json_file::JsonFileStore;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

/// Persisted and interactive width limits of the side panels, in logical pixels.
pub const LEFT_PANEL_WIDTH: RangeInclusive<f32> = 200.0..=440.0;
pub const RIGHT_PANEL_WIDTH: RangeInclusive<f32> = 240.0..=440.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPreferences {
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    pub left_width: f32,
    pub right_width: f32,
    /// Optional surfaces floating over the canvas. The canvas toolbar itself
    /// is always shown, so these can be hidden without losing a way back.
    #[serde(default)]
    pub overlays: CanvasOverlays,
}
impl Default for PanelPreferences {
    fn default() -> Self {
        Self {
            left_collapsed: false,
            right_collapsed: false,
            left_width: 288.0,
            right_width: 320.0,
            overlays: CanvasOverlays::default(),
        }
    }
}

/// Which canvas overlays are shown; every overlay starts visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CanvasOverlays {
    /// Location chip naming the current selection.
    pub location: bool,
    /// Per-layer stack under the pointer.
    pub probe: bool,
    /// Scale bar and board axes.
    pub scale: bool,
    /// Measurement, clearance and path marks drawn over the board.
    pub review_marks: bool,
    /// 3D view cube.
    pub view_cube: bool,
}
impl Default for CanvasOverlays {
    fn default() -> Self {
        Self {
            location: true,
            probe: true,
            scale: true,
            review_marks: true,
            view_cube: true,
        }
    }
}
impl CanvasOverlays {
    pub fn all(visible: bool) -> Self {
        Self {
            location: visible,
            probe: visible,
            scale: visible,
            review_marks: visible,
            view_cube: visible,
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
        if !LEFT_PANEL_WIDTH.contains(&panels.left_width)
            || !RIGHT_PANEL_WIDTH.contains(&panels.right_width)
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
