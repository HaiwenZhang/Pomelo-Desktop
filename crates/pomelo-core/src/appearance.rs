//! Document-local display colors. Source geometry and source colors remain immutable.
use crate::{
    display::{DisplayCategory, LayerPrimitive},
    model::LayerId,
};
use std::collections::BTreeMap;

/// Opaque sRGB bytes; opacity is a separate display setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RgbColor(pub [u8; 3]);
impl RgbColor {
    pub fn rgba(self) -> [f32; 4] {
        let [r, g, b] = self.0;
        [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ]
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayerColors {
    pub etch: Option<RgbColor>,
    pub pin: Option<RgbColor>,
    pub via: Option<RgbColor>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoardAppearance {
    pub background: Option<RgbColor>,
    pub drill: Option<RgbColor>,
    pub layers: BTreeMap<LayerId, LayerColors>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorTarget {
    Background,
    Drill,
    Etch(LayerId),
    Pin(LayerId),
    Via(LayerId),
}
impl ColorTarget {
    pub fn layer(layer: LayerId, primitive: LayerPrimitive) -> Self {
        match primitive {
            LayerPrimitive::Traces => Self::Etch(layer),
            LayerPrimitive::Pads => Self::Pin(layer),
            LayerPrimitive::Vias => Self::Via(layer),
        }
    }
}
impl BoardAppearance {
    pub fn color(&self, target: ColorTarget) -> Option<RgbColor> {
        match target {
            ColorTarget::Background => self.background,
            ColorTarget::Drill => self.drill,
            ColorTarget::Etch(layer) => self.layers.get(&layer)?.etch,
            ColorTarget::Pin(layer) => self.layers.get(&layer)?.pin,
            ColorTarget::Via(layer) => self.layers.get(&layer)?.via,
        }
    }
    pub fn set_color(&mut self, target: ColorTarget, color: Option<RgbColor>) {
        let layer = match target {
            ColorTarget::Background => {
                self.background = color;
                return;
            }
            ColorTarget::Drill => {
                self.drill = color;
                return;
            }
            ColorTarget::Etch(layer) => {
                self.layers.entry(layer).or_default().etch = color;
                layer
            }
            ColorTarget::Pin(layer) => {
                self.layers.entry(layer).or_default().pin = color;
                layer
            }
            ColorTarget::Via(layer) => {
                self.layers.entry(layer).or_default().via = color;
                layer
            }
        };
        if self.layers.get(&layer) == Some(&LayerColors::default()) {
            self.layers.remove(&layer);
        }
    }
    /// Copper shapes, traces and copper-layer source text share the Etch material.
    /// Drawing-layer text and generated labels retain their existing colors.
    pub fn material(&self, layer: LayerId, category: DisplayCategory) -> Option<[f32; 4]> {
        use DisplayCategory::*;
        let target = match category {
            Zone | ZoneOutline | Trace | BondWire => ColorTarget::Etch(layer),
            Text if layer.0 < 0x10000 => ColorTarget::Etch(layer),
            Pin => ColorTarget::Pin(layer),
            Via => ColorTarget::Via(layer),
            Drill => ColorTarget::Drill,
            _ => return None,
        };
        self.color(target).map(RgbColor::rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn etch_override_covers_copper_text_without_recoloring_pin_via_or_drawing() {
        let mut appearance = BoardAppearance::default();
        appearance.set_color(ColorTarget::Etch(LayerId(0)), Some(RgbColor([38, 255, 38])));
        for category in [
            DisplayCategory::Trace,
            DisplayCategory::Zone,
            DisplayCategory::ZoneOutline,
            DisplayCategory::Text,
        ] {
            assert_eq!(
                appearance.material(LayerId(0), category),
                Some(RgbColor([38, 255, 38]).rgba())
            );
        }
        for category in [
            DisplayCategory::Pin,
            DisplayCategory::Via,
            DisplayCategory::Drawing,
        ] {
            assert_eq!(appearance.material(LayerId(0), category), None);
        }
        assert_eq!(
            appearance.material(LayerId(0x10001), DisplayCategory::Text),
            None
        );
    }
    #[test]
    fn restoring_a_category_retains_other_overrides_and_removes_empty_layer() {
        let mut appearance = BoardAppearance::default();
        appearance.set_color(ColorTarget::Pin(LayerId(0)), Some(RgbColor([255, 0, 0])));
        appearance.set_color(ColorTarget::Via(LayerId(0)), Some(RgbColor([0, 0, 255])));
        appearance.set_color(ColorTarget::Pin(LayerId(0)), None);
        assert_eq!(
            appearance.color(ColorTarget::Via(LayerId(0))),
            Some(RgbColor([0, 0, 255]))
        );
        appearance.set_color(ColorTarget::Via(LayerId(0)), None);
        assert!(appearance.layers.is_empty());
    }
    #[test]
    fn serialized_colors_reject_invalid_channels_and_unknown_fields() {
        assert!(serde_json::from_str::<RgbColor>("[256,0,0]").is_err());
        assert!(serde_json::from_str::<RgbColor>("[0,0]").is_err());
        assert!(serde_json::from_str::<BoardAppearance>(r#"{"opacity":0.5}"#).is_err());
    }
}
