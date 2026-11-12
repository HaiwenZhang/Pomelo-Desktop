//! Document-local display colors. Source geometry and source colors remain immutable.
pub mod net;
pub mod pattern;
use crate::{
    display::{DisplayCategory, LayerPrimitive},
    model::{LayerId, NetId},
};
use net::{NetColorKind, NetColors};
use pattern::FillPattern;
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
    pub etch_pattern: Option<FillPattern>,
    pub pin_pattern: Option<FillPattern>,
    pub via_pattern: Option<FillPattern>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoardAppearance {
    pub background: Option<RgbColor>,
    pub grid: Option<RgbColor>,
    pub drill: Option<RgbColor>,
    pub layers: BTreeMap<LayerId, LayerColors>,
    pub nets: BTreeMap<NetId, NetColors>,
    /// Temporarily suppress custom net assignments without clearing them.
    pub hide_net_colors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorTarget {
    Background,
    Grid,
    Drill,
    Etch(LayerId),
    Pin(LayerId),
    Via(LayerId),
    Net(NetId, NetColorKind),
}
impl ColorTarget {
    pub fn layer(layer: LayerId, primitive: LayerPrimitive) -> Self {
        match primitive {
            // Copper shares the trace color, as on the layer color row.
            LayerPrimitive::Traces | LayerPrimitive::Copper | LayerPrimitive::Components => {
                Self::Etch(layer)
            }
            LayerPrimitive::Pads => Self::Pin(layer),
            LayerPrimitive::Vias => Self::Via(layer),
        }
    }
}
impl BoardAppearance {
    /// Per-category assignment wins over the whole-net default in either color mode.
    pub fn net_material(&self, net: NetId, category: DisplayCategory) -> Option<RgbColor> {
        if self.hide_net_colors {
            return None;
        }
        self.nets.get(&net)?.material(category)
    }
    pub fn pattern(&self, target: ColorTarget) -> Option<FillPattern> {
        match target {
            ColorTarget::Etch(layer) => self.layers.get(&layer)?.etch_pattern,
            ColorTarget::Pin(layer) => self.layers.get(&layer)?.pin_pattern,
            ColorTarget::Via(layer) => self.layers.get(&layer)?.via_pattern,
            _ => None,
        }
    }
    pub fn set_pattern(&mut self, target: ColorTarget, pattern: Option<FillPattern>) {
        let layer = match target {
            ColorTarget::Etch(layer) => {
                self.layers.entry(layer).or_default().etch_pattern = pattern;
                layer
            }
            ColorTarget::Pin(layer) => {
                self.layers.entry(layer).or_default().pin_pattern = pattern;
                layer
            }
            ColorTarget::Via(layer) => {
                self.layers.entry(layer).or_default().via_pattern = pattern;
                layer
            }
            _ => return,
        };
        if self.layers.get(&layer) == Some(&LayerColors::default()) {
            self.layers.remove(&layer);
        }
    }
    /// Text, boundaries, holes and overlays remain readable independently of fill stipple.
    pub fn material_pattern(
        &self,
        layer: LayerId,
        category: DisplayCategory,
    ) -> Option<FillPattern> {
        let target = match category {
            DisplayCategory::Zone
            | DisplayCategory::Trace
            | DisplayCategory::BondWire
            | DisplayCategory::Drawing => ColorTarget::Etch(layer),
            DisplayCategory::Pin => ColorTarget::Pin(layer),
            DisplayCategory::Via => ColorTarget::Via(layer),
            _ => return None,
        };
        self.pattern(target)
    }
    pub fn color(&self, target: ColorTarget) -> Option<RgbColor> {
        match target {
            ColorTarget::Background => self.background,
            ColorTarget::Grid => self.grid,
            ColorTarget::Drill => self.drill,
            ColorTarget::Etch(layer) => self.layers.get(&layer)?.etch,
            ColorTarget::Pin(layer) => self.layers.get(&layer)?.pin,
            ColorTarget::Via(layer) => self.layers.get(&layer)?.via,
            ColorTarget::Net(net, kind) => self.nets.get(&net)?.color(kind),
        }
    }
    pub fn set_color(&mut self, target: ColorTarget, color: Option<RgbColor>) {
        if let ColorTarget::Net(net, kind) = target {
            self.nets.entry(net).or_default().set(kind, color);
            if self.nets.get(&net) == Some(&NetColors::default()) {
                self.nets.remove(&net);
            }
            return;
        }
        let layer = match target {
            ColorTarget::Grid => {
                self.grid = color;
                return;
            }
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
            ColorTarget::Net(_, _) => return,
        };
        if self.layers.get(&layer) == Some(&LayerColors::default()) {
            self.layers.remove(&layer);
        }
    }
    /// Copper shapes, traces and copper-layer source text share the Etch material.
    /// Drawing-layer overrides cover their graphics and source text; generated labels keep their own colors.
    pub fn material(&self, layer: LayerId, category: DisplayCategory) -> Option<[f32; 4]> {
        use DisplayCategory::*;
        let target = match category {
            Zone | ZoneOutline | Trace | BondWire => ColorTarget::Etch(layer),
            Text if layer.is_copper() => ColorTarget::Etch(layer),
            Drawing | Text if !layer.is_copper() => ColorTarget::Etch(layer),
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
    fn material_patterns_round_trip_and_restore_independently_of_colors() {
        let layer = LayerId(0);
        let mut appearance: BoardAppearance =
            serde_json::from_str(r#"{"layers":{"0":{"etch":[38,255,38]}}}"#).unwrap();
        assert_eq!(appearance.pattern(ColorTarget::Etch(layer)), None);
        appearance.set_pattern(ColorTarget::Etch(layer), Some(FillPattern::DiagonalDown));
        appearance.set_pattern(ColorTarget::Pin(layer), Some(FillPattern::Horizontal));
        appearance.set_pattern(ColorTarget::Via(layer), Some(FillPattern::Vertical));
        let restored: BoardAppearance =
            serde_json::from_str(&serde_json::to_string(&appearance).unwrap()).unwrap();
        assert_eq!(restored, appearance);
        assert_eq!(
            appearance.material_pattern(layer, DisplayCategory::Text),
            None
        );
        assert_eq!(
            appearance.material_pattern(layer, DisplayCategory::ZoneOutline),
            None
        );
        assert_eq!(
            appearance.material_pattern(layer, DisplayCategory::Zone),
            Some(FillPattern::DiagonalDown)
        );
        appearance.set_color(ColorTarget::Etch(layer), None);
        assert_eq!(
            appearance.pattern(ColorTarget::Etch(layer)),
            Some(FillPattern::DiagonalDown)
        );
        appearance.set_pattern(ColorTarget::Etch(layer), None);
        assert_eq!(
            appearance.pattern(ColorTarget::Pin(layer)),
            Some(FillPattern::Horizontal)
        );
        appearance.set_pattern(ColorTarget::Pin(layer), None);
        appearance.set_pattern(ColorTarget::Via(layer), None);
        assert!(appearance.layers.is_empty());
    }
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
            appearance.material(LayerId::allegro_drawing(1), DisplayCategory::Text),
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
