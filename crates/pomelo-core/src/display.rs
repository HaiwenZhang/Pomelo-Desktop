//! Per-document display settings, independent of GPU resources and source layer order.
use crate::model::LayerId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    #[default]
    Layer,
    Net,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerPrimitive {
    Traces,
    Vias,
    Pads,
}

/// Submission categories shared by the canvas and picking (Pomelo Web BoardDisplay).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayCategory {
    Outline,
    Drawing,
    Zone,
    ZoneOutline,
    #[serde(rename = "etch")]
    Trace,
    BondWire,
    Text,
    Pin,
    Via,
    Drill,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct LayerPriority {
    pub layer: LayerId,
    pub category: DisplayCategory,
}

/// A sparse per-layer override; all three categories are visible by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerPrimitives {
    pub traces: bool,
    pub vias: bool,
    pub pads: bool,
}
impl Default for LayerPrimitives {
    fn default() -> Self {
        Self {
            traces: true,
            vias: true,
            pads: true,
        }
    }
}
impl LayerPrimitives {
    pub fn visible(self, kind: LayerPrimitive) -> bool {
        match kind {
            LayerPrimitive::Traces => self.traces,
            LayerPrimitive::Vias => self.vias,
            LayerPrimitive::Pads => self.pads,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDisplay {
    #[serde(default)]
    pub priorities: Vec<LayerPriority>,
    #[serde(default)]
    pub active_layer: Option<LayerId>,
    #[serde(default = "default_visible")]
    pub filled: bool,
    #[serde(default)]
    pub color_mode: ColorMode,
    #[serde(default)]
    pub length_unit: crate::units::LengthUnit,
    pub hidden_layers: BTreeSet<LayerId>,
    #[serde(default)]
    pub layer_primitives: BTreeMap<LayerId, LayerPrimitives>,
    pub show_drills: bool,
    #[serde(default = "default_visible")]
    pub show_backdrills: bool,
    #[serde(default = "default_visible")]
    pub show_copper: bool,
    #[serde(default)]
    pub show_texts: bool,
    #[serde(default = "default_visible")]
    pub show_drawings: bool,
    #[serde(default = "default_copper_opacity")]
    pub copper_opacity: f32,
    /// Explicit drawing order, from bottom to top; empty uses source order.
    #[serde(default)]
    pub layer_order: Vec<LayerId>,
}
fn default_copper_opacity() -> f32 {
    0.25
}
fn default_visible() -> bool {
    true
}
impl Default for BoardDisplay {
    fn default() -> Self {
        Self {
            priorities: Vec::new(),
            active_layer: None,
            filled: true,
            color_mode: ColorMode::default(),
            length_unit: crate::units::LengthUnit::default(),
            hidden_layers: BTreeSet::new(),
            layer_primitives: BTreeMap::new(),
            show_drills: true,
            show_backdrills: true,
            show_copper: true,
            show_texts: false,
            show_drawings: true,
            copper_opacity: default_copper_opacity(),
            layer_order: Vec::new(),
        }
    }
}
impl BoardDisplay {
    /// Back-to-front rank. An active copper layer wins over manual promotion;
    /// otherwise drills, vias and pins sit above etch across the entire stack.
    pub fn display_rank(&self, layer: LayerId, category: DisplayCategory) -> [i64; 5] {
        use DisplayCategory::*;
        let etch = matches!(category, Zone | ZoneOutline | Trace)
            || (category == Text && layer.0 < 0x10000);
        let group_category = match category {
            Zone | ZoneOutline => Trace,
            Text if etch => Trace,
            Drawing => Text,
            _ => category,
        };
        let promoted = if self.priorities.is_empty() {
            self.layer_order
                .iter()
                .position(|id| *id == layer)
                .map_or(0, |index| index as i64 + 1)
        } else {
            self.priorities
                .iter()
                .position(|priority| priority.layer == layer && priority.category == group_category)
                .map_or(0, |index| (self.priorities.len() - index) as i64)
        };
        let active = etch && self.active_layer == Some(layer);
        let group = match category {
            Outline => 0,
            Drawing => 1,
            Text if !etch => 1,
            Zone | ZoneOutline | Trace | BondWire | Text => 2,
            Pin => 3,
            Via => 4,
            Drill => 5,
        };
        let part = match category {
            Zone => 0,
            ZoneOutline => 1,
            Trace | BondWire | Drawing => 2,
            Text => 3,
            _ => 0,
        };
        [
            if active { 2 } else { i64::from(promoted > 0) },
            promoted,
            group,
            -i64::from(layer.0),
            part,
        ]
    }
    pub fn primitives(&self, layer: LayerId) -> LayerPrimitives {
        self.layer_primitives
            .get(&layer)
            .copied()
            .unwrap_or_default()
    }
    pub fn set_primitive(&mut self, layer: LayerId, kind: LayerPrimitive, visible: bool) {
        let mut state = self.primitives(layer);
        match kind {
            LayerPrimitive::Traces => state.traces = visible,
            LayerPrimitive::Vias => state.vias = visible,
            LayerPrimitive::Pads => state.pads = visible,
        }
        if state == LayerPrimitives::default() {
            self.layer_primitives.remove(&layer);
        } else {
            self.layer_primitives.insert(layer, state);
        }
    }
    pub fn primitive_visible(&self, layer: LayerId, kind: LayerPrimitive) -> bool {
        self.layer_visible(layer) && self.primitives(layer).visible(kind)
    }
    pub fn object_visible(&self, layer: LayerId, object: crate::selection::SelectedObject) -> bool {
        use crate::selection::SelectedObject;
        match object {
            SelectedObject::Segment(_) => self.primitive_visible(layer, LayerPrimitive::Traces),
            SelectedObject::Pin(_) => self.primitive_visible(layer, LayerPrimitive::Pads),
            SelectedObject::Via(_) => self.primitive_visible(layer, LayerPrimitive::Vias),
            SelectedObject::Zone(_) => self.show_copper && self.layer_visible(layer),
            SelectedObject::Drawing(_) => self.show_drawings && self.layer_visible(layer),
        }
    }
    pub fn move_layer_to_edge(&mut self, layer: LayerId, top: bool, source: &[LayerId]) -> bool {
        let mut order = self.ordered_layers(source.iter().copied());
        let Some(index) = order.iter().position(|candidate| *candidate == layer) else {
            return false;
        };
        let target = if top { order.len() - 1 } else { 0 };
        if index == target {
            return false;
        }
        order.remove(index);
        order.insert(target, layer);
        self.layer_order = order;
        true
    }
    /// Back-to-front layer order for painting; ignore stale IDs and append new layers.
    pub fn ordered_layers(&self, source: impl IntoIterator<Item = LayerId>) -> Vec<LayerId> {
        let mut source: Vec<_> = source.into_iter().collect();
        source.sort_unstable_by(|a, b| b.cmp(a));
        let available: BTreeSet<_> = source.iter().copied().collect();
        let mut seen = BTreeSet::new();
        self.layer_order
            .iter()
            .copied()
            .chain(source)
            .filter(|layer| available.contains(layer) && seen.insert(*layer))
            .collect()
    }
    /// Front-to-back presentation order: physical TOP (ID zero) appears first
    /// by default. Reordering remains consistent with the painting order.
    pub fn layers_front_to_back(&self, source: impl IntoIterator<Item = LayerId>) -> Vec<LayerId> {
        let mut layers = self.ordered_layers(source);
        layers.reverse();
        layers
    }
    /// Adjust whole percentage points so repeated UI steps reach exact endpoints.
    pub fn adjust_copper_opacity(&mut self, percentage_points: i16) {
        let percent = (self.copper_opacity * 100.0).round() as i32;
        self.copper_opacity = (percent + i32::from(percentage_points)).clamp(0, 100) as f32 / 100.0;
    }

    pub fn layer_visible(&self, layer: LayerId) -> bool {
        !self.hidden_layers.contains(&layer)
    }
    /// Via drills require at least one visible source pad layer; pin drills have no scope.
    pub fn drill_visible(&self, scope: Option<&[LayerId]>) -> bool {
        self.show_drills && self.drill_scope_visible(scope)
    }
    /// Backdrill patterns are independent of the ordinary drill-center switch.
    pub fn backdrill_visible(&self, scope: &[LayerId]) -> bool {
        self.show_backdrills && self.drill_scope_visible(Some(scope))
    }
    /// Labels share layer scope, without depending on either drill display switch.
    pub fn drill_scope_visible(&self, scope: Option<&[LayerId]>) -> bool {
        scope.is_none_or(|layers| {
            layers
                .iter()
                .any(|layer| self.primitive_visible(*layer, LayerPrimitive::Vias))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layer_categories_are_independent_sparse_and_compatible_with_legacy_state() {
        let mut display: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        for kind in [
            LayerPrimitive::Traces,
            LayerPrimitive::Vias,
            LayerPrimitive::Pads,
        ] {
            assert!(display.primitive_visible(LayerId(1), kind));
            display.set_primitive(LayerId(1), kind, false);
            assert!(!display.primitive_visible(LayerId(1), kind));
            assert!(display.primitive_visible(LayerId(2), kind));
            let restored: BoardDisplay =
                serde_json::from_slice(&serde_json::to_vec(&display).unwrap()).unwrap();
            assert_eq!(
                restored.primitives(LayerId(1)),
                display.primitives(LayerId(1))
            );
            display.set_primitive(LayerId(1), kind, true);
            assert!(display.layer_primitives.is_empty());
        }
        display.hidden_layers.insert(LayerId(1));
        assert!(!display.primitive_visible(LayerId(1), LayerPrimitive::Traces));
        assert!(
            display.primitives(LayerId(1)).traces,
            "whole-layer visibility must preserve category choices"
        );
    }
    #[test]
    fn via_drill_scope_obeys_category_overrides_without_affecting_pin_drills() {
        let mut display = BoardDisplay::default();
        for layer in [LayerId(1), LayerId(2)] {
            display.set_primitive(layer, LayerPrimitive::Pads, false);
        }
        assert!(display.drill_visible(Some(&[LayerId(1), LayerId(2)])));
        display.set_primitive(LayerId(1), LayerPrimitive::Vias, false);
        assert!(display.drill_visible(Some(&[LayerId(1), LayerId(2)])));
        display.set_primitive(LayerId(2), LayerPrimitive::Vias, false);
        assert!(!display.drill_visible(Some(&[LayerId(1), LayerId(2)])));
        assert!(display.drill_visible(None));
        display.set_primitive(LayerId(2), LayerPrimitive::Vias, true);
        assert!(display.drill_visible(Some(&[LayerId(1), LayerId(2)])));
    }
    #[test]
    fn display_unit_defaults_for_legacy_state_and_round_trips_mils() {
        let legacy: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        assert_eq!(legacy.length_unit, crate::units::LengthUnit::Millimeters);
        assert_eq!(legacy.color_mode, ColorMode::Layer);
        let display = BoardDisplay {
            color_mode: ColorMode::Net,
            length_unit: crate::units::LengthUnit::Mils,
            ..legacy
        };
        let restored: BoardDisplay =
            serde_json::from_str(&serde_json::to_string(&display).unwrap()).unwrap();
        assert_eq!(restored.length_unit, crate::units::LengthUnit::Mils);
        assert_eq!(restored.color_mode, ColorMode::Net);
        let mut json = serde_json::to_value(&restored).unwrap();
        json["color_mode"] = serde_json::json!("unsupported");
        assert!(serde_json::from_value::<BoardDisplay>(json).is_err());
    }
    #[test]
    fn explicit_order_keeps_new_layers_and_ignores_stale_or_duplicate_ids() {
        let display = BoardDisplay {
            layer_order: vec![LayerId(3), LayerId(99), LayerId(3), LayerId(1)],
            ..Default::default()
        };
        assert_eq!(
            display.ordered_layers([LayerId(1), LayerId(2), LayerId(3), LayerId(2)]),
            vec![LayerId(3), LayerId(1), LayerId(2)]
        );
        assert_eq!(
            BoardDisplay::default().ordered_layers([LayerId(2), LayerId(1)]),
            vec![LayerId(2), LayerId(1)]
        );
    }
    #[test]
    fn layer_list_starts_with_top_and_manual_moves_keep_painting_consistent() {
        let source = [LayerId(0), LayerId(1), LayerId(2), LayerId(65540)];
        let mut display = BoardDisplay::default();
        assert_eq!(display.layers_front_to_back(source), source);
        for category in [
            DisplayCategory::Trace,
            DisplayCategory::Pin,
            DisplayCategory::Via,
            DisplayCategory::Zone,
        ] {
            assert!(
                display.display_rank(LayerId(0), category)
                    > display.display_rank(LayerId(2), category)
            );
        }
        assert!(display.move_layer_to_edge(LayerId(2), true, &source));
        assert_eq!(
            display.layers_front_to_back(source).first(),
            Some(&LayerId(2))
        );
        assert_eq!(display.ordered_layers(source).last(), Some(&LayerId(2)));
        assert!(display.move_layer_to_edge(LayerId(2), false, &source));
        assert_eq!(
            display.layers_front_to_back(source).last(),
            Some(&LayerId(2))
        );
        assert_eq!(display.ordered_layers(source).first(), Some(&LayerId(2)));
    }
    #[test]
    fn moving_layer_to_edge_preserves_other_layers_and_rejects_absent_target() {
        let source = [LayerId(1), LayerId(2), LayerId(3)];
        let mut display = BoardDisplay::default();
        assert!(display.move_layer_to_edge(LayerId(2), true, &source));
        assert_eq!(
            display.layer_order,
            vec![LayerId(3), LayerId(1), LayerId(2)]
        );
        assert!(!display.move_layer_to_edge(LayerId(2), true, &source));
        assert!(!display.move_layer_to_edge(LayerId(99), false, &source));
        assert!(display.move_layer_to_edge(LayerId(2), false, &source));
        assert_eq!(
            display.layer_order,
            vec![LayerId(2), LayerId(3), LayerId(1)]
        );
    }
    #[test]
    fn repeated_opacity_steps_reach_exact_endpoints_and_restore_default() {
        let mut display = BoardDisplay::default();
        for _ in 0..5 {
            display.adjust_copper_opacity(-5);
        }
        assert_eq!(display.copper_opacity, 0.0);
        for _ in 0..20 {
            display.adjust_copper_opacity(5);
        }
        assert_eq!(display.copper_opacity, 1.0);
        display.adjust_copper_opacity(5);
        assert_eq!(display.copper_opacity, 1.0);
        for _ in 0..15 {
            display.adjust_copper_opacity(-5);
        }
        assert_eq!(display.copper_opacity, default_copper_opacity());
    }
    #[test]
    fn via_drills_follow_visible_pad_layers_and_empty_scopes_stay_hidden() {
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(LayerId(1));
        assert!(!display.drill_visible(Some(&[])));
        assert!(!display.drill_visible(Some(&[LayerId(1)])));
        assert!(display.drill_visible(Some(&[LayerId(1), LayerId(2)])));
        assert!(display.drill_visible(None));
        display.show_drills = false;
        assert!(!display.drill_visible(None));
    }
}
