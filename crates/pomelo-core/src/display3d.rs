//! Persisted 3D viewing options. They change presentation only, never source geometry.
use crate::interaction::camera3d::Camera3d;
use serde::{Deserialize, Serialize};

/// How faces and edges of 3D objects are drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode3d {
    /// Faces follow per-layer opacity and per-cell fill/outline choices.
    #[default]
    Default,
    /// Outlines only, so inner layers stay visible.
    Wireframe,
    /// Opaque faces regardless of per-layer opacity.
    Solid,
}

/// Which parts of the stack grow with the Z display factor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZStretch {
    /// Metal keeps its thickness; only the gaps between layers grow.
    #[default]
    Dielectric,
    /// Metal and dielectric grow together.
    All,
}

/// Z display factors offered as presets; any value in [`Z_SCALE_RANGE`] is valid.
pub const Z_SCALE_PRESETS: [f64; 6] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0];
pub const Z_SCALE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=100.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display3d {
    pub mode: RenderMode3d,
    pub z_scale: f64,
    pub stretch: ZStretch,
    /// Opacity kept by unselected objects while something is selected; 1 disables dimming.
    pub unselected_opacity: f32,
    /// Preview the object under the pointer in the selection color.
    pub hover_preview: bool,
    /// Draw the selection with faces even where its cell or the mode shows outlines.
    pub selected_solid: bool,
    /// Draw the unused part of plated holes in a separate style.
    pub show_stubs: bool,
    /// Show dielectric slabs between conductor layers.
    pub show_dielectric: bool,
    /// Short transitions when jumping to a standard view.
    pub animate_views: bool,
    /// Assumed board thickness when the source has no stack-up; display only.
    pub board_mm: f64,
    /// Assumed copper thickness of every conductor layer; display only.
    pub copper_mm: f64,
}

/// Assumed thicknesses offered in the stack settings.
pub const BOARD_PRESETS_MM: [f64; 5] = [0.8, 1.0, 1.6, 2.0, 2.4];
pub const COPPER_PRESETS_MM: [f64; 3] = [0.018, 0.035, 0.07];

impl Default for Display3d {
    fn default() -> Self {
        Self {
            mode: RenderMode3d::Default,
            z_scale: 5.0,
            stretch: ZStretch::Dielectric,
            unselected_opacity: 0.3,
            hover_preview: true,
            selected_solid: true,
            show_stubs: true,
            show_dielectric: false,
            animate_views: true,
            board_mm: 1.6,
            copper_mm: 0.035,
        }
    }
}

impl Display3d {
    pub fn valid(&self) -> bool {
        self.z_scale.is_finite()
            && Z_SCALE_RANGE.contains(&self.z_scale)
            && self.unselected_opacity.is_finite()
            && (0.0..=1.0).contains(&self.unselected_opacity)
            && (0.1..=10.0).contains(&self.board_mm)
            && (0.001..=0.5).contains(&self.copper_mm)
    }
}

/// Serializable form of [`Camera3d`]; restoring validates it again.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera3dState {
    pub pivot: [f64; 3],
    pub orientation: [f64; 4],
    pub view_height: f64,
}

impl Camera3dState {
    pub fn capture(camera: Camera3d) -> Self {
        Self {
            pivot: camera.pivot(),
            orientation: camera.orientation(),
            view_height: camera.view_height(),
        }
    }
    pub fn camera(self) -> Option<Camera3d> {
        Camera3d::new(self.pivot, self.orientation, self.view_height)
    }
}

/// 3D part of a saved document view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct View3dState {
    /// The document reopens in 3D.
    pub active: bool,
    /// Absent until the 3D scene has been framed once.
    pub camera: Option<Camera3dState>,
    pub display: Display3d,
}

impl View3dState {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn valid(&self) -> bool {
        self.display.valid() && self.camera.is_none_or(|camera| camera.camera().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_state_roundtrips_and_rejects_degenerate_values() {
        let camera = Camera3d::new([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 2.0], 40.0).unwrap();
        let state = Camera3dState::capture(camera);
        let decoded: Camera3dState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(decoded.camera(), Some(camera));
        let mut bad = state;
        bad.view_height = 0.0;
        assert!(bad.camera().is_none());
        bad = state;
        bad.orientation = [0.0; 4];
        assert!(bad.camera().is_none());
    }

    #[test]
    fn display_defaults_fill_missing_fields_and_bounds_are_checked() {
        let restored: Display3d = serde_json::from_str(r#"{"mode":"solid"}"#).unwrap();
        assert_eq!(restored.mode, RenderMode3d::Solid);
        assert_eq!(restored.z_scale, Display3d::default().z_scale);
        assert_eq!(restored.stretch, ZStretch::Dielectric);
        for z_scale in [0.5, 101.0, f64::NAN] {
            assert!(
                !Display3d {
                    z_scale,
                    ..Default::default()
                }
                .valid()
            );
        }
        assert!(
            !Display3d {
                unselected_opacity: 1.5,
                ..Default::default()
            }
            .valid()
        );
    }
}
