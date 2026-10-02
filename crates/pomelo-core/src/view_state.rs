//! Versioned viewing state; matching content and decoding identity precede restoration.
use crate::{
    display::BoardDisplay,
    interaction::{Camera, SelectionMode},
    selection::SelectionTarget,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub sha256: [u8; 32],
    pub format: String,
    pub encoding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    pub schema_version: u32,
    pub source: SourceIdentity,
    pub camera: Camera,
    pub display: BoardDisplay,
    pub selection_mode: SelectionMode,
    #[serde(default)]
    pub pick_filter: crate::picking::PickFilter,
    pub selection: Option<SelectionTarget>,
}

impl ViewState {
    /// Validate before writing or restoring; callers attach configuration paths.
    pub fn validate(&self) -> Result<(), crate::model::Diagnostic> {
        if self.schema_version != 1
            || self.source.format.is_empty()
            || self.source.format.len() > 64
            || self.source.encoding.is_empty()
            || self.source.encoding.len() > 64
            || !self.camera.is_renderable()
            || self.display.hidden_layers.len() > 4096
            || self.display.layer_order.len() > 4096
            || self
                .display
                .layer_order
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.display.layer_order.len()
            || !self.display.copper_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.display.copper_opacity)
        {
            return Err(crate::model::Diagnostic::error(
                "VIEW_STATE_INVALID",
                crate::i18n::MessageKey::ConfigInvalid,
            ));
        }
        Ok(())
    }

    /// A changed file or decoding option invalidates the whole saved view.
    /// A matching selection still requires source-object existence validation
    /// by the host before asynchronous inspector/highlight work is scheduled.
    pub fn matching(
        &self,
        source: &SourceIdentity,
    ) -> Result<Option<&Self>, crate::model::Diagnostic> {
        self.validate()?;
        Ok((&self.source == source).then_some(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, ObjectId};
    fn state() -> ViewState {
        ViewState {
            schema_version: 1,
            source: SourceIdentity {
                sha256: [7; 32],
                format: "allegro".into(),
                encoding: "utf-8".into(),
            },
            camera: Camera::default(),
            display: BoardDisplay::default(),
            selection_mode: SelectionMode::Object,
            pick_filter: crate::picking::PickFilter::default(),
            selection: Some(SelectionTarget::Object(
                crate::selection::SelectedObject::Pin(ObjectId(7)),
            )),
        }
    }
    #[test]
    fn roundtrip_preserves_camera_visibility_and_typed_selection() {
        let mut original = state();
        original.camera.center = crate::model::Point::new(23.5, -17.0);
        original.camera.flipped = true;
        original.display.hidden_layers.insert(LayerId(4));
        original.display.show_drills = false;
        original.display.show_copper = false;
        original.display.show_texts = false;
        original.display.show_drawings = false;
        original.display.copper_opacity = 0.75;
        original.display.layer_order = vec![LayerId(4), LayerId(2)];
        let decoded: ViewState =
            serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
        let restored = decoded.matching(&original.source).unwrap().unwrap();
        assert_eq!(restored.camera.center, original.camera.center);
        assert!(restored.camera.flipped);
        assert_eq!(restored.selection, original.selection);
        assert!(!restored.display.layer_visible(LayerId(4)));
        assert!(!restored.display.show_drills);
        assert!(!restored.display.show_copper);
        assert!(!restored.display.show_texts);
        assert!(!restored.display.show_drawings);
        assert_eq!(restored.display.copper_opacity, 0.75);
        assert_eq!(restored.display.layer_order, original.display.layer_order);
    }
    #[test]
    fn content_and_decoding_changes_reject_old_view() {
        let original = state();
        let mut source = original.source.clone();
        source.sha256[0] ^= 1;
        assert!(original.matching(&source).unwrap().is_none());
        source = original.source.clone();
        source.encoding = "windows-1252".into();
        assert!(original.matching(&source).unwrap().is_none());
    }
    #[test]
    fn filters_roundtrip_and_legacy_state_defaults_without_accepting_unknown_bits() {
        for bits in 0..16 {
            let mut original = state();
            original.pick_filter = bits.try_into().unwrap();
            let decoded: ViewState =
                serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
            assert_eq!(u8::from(decoded.pick_filter), bits);
        }
        let mut json = serde_json::to_value(state()).unwrap();
        json.as_object_mut().unwrap().remove("pick_filter");
        let legacy: ViewState = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(u8::from(legacy.pick_filter), 15);
        json["pick_filter"] = serde_json::json!(16);
        assert!(serde_json::from_value::<ViewState>(json).is_err());
    }
    #[test]
    fn invalid_camera_and_schema_return_five_language_diagnostics() {
        for invalid in [
            0.0,
            -1.0,
            f64::INFINITY,
            f64::NAN,
            f64::MAX,
            f64::MIN_POSITIVE,
        ] {
            let mut value = state();
            value.camera.pixels_per_mm = invalid;
            let error = value.validate().unwrap_err();
            for locale in crate::i18n::Locale::ALL {
                assert!(!error.message.display(locale).to_string().is_empty());
            }
        }
        let mut value = state();
        value.schema_version = 2;
        assert!(value.validate().is_err());
    }

    #[test]
    fn unrepresentable_center_cannot_restore_or_replace_navigation() {
        let mut value = state();
        value.camera.center.x = f64::MAX;
        assert!(value.validate().is_err());
        let mut navigation = crate::interaction::ViewportNavigation::default();
        let previous = navigation.camera();
        assert!(!navigation.restore_camera(value.camera));
        assert_eq!(navigation.camera().center, previous.center);
        assert!(!navigation.has_view());
    }
    #[test]
    fn opacity_rejects_invalid_values_and_legacy_display_uses_product_default() {
        for opacity in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            let mut value = state();
            value.display.copper_opacity = opacity;
            assert!(value.validate().is_err());
        }
        let display: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        assert_eq!(display.copper_opacity, 0.35);
        assert!(display.show_copper);
        assert!(display.show_texts);
        assert!(display.show_drawings);
    }
}
