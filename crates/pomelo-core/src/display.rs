//! Per-document visibility, independent of GPU resources and source layer order.
use crate::model::LayerId;
use std::collections::BTreeSet;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDisplay {
    #[serde(default)]
    pub length_unit: crate::units::LengthUnit,
    pub hidden_layers: BTreeSet<LayerId>,
    pub show_drills: bool,
    #[serde(default = "default_visible")]
    pub show_copper: bool,
    #[serde(default = "default_visible")]
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
    0.35
}
fn default_visible() -> bool {
    true
}
impl Default for BoardDisplay {
    fn default() -> Self {
        Self {
            length_unit: crate::units::LengthUnit::default(),
            hidden_layers: BTreeSet::new(),
            show_drills: true,
            show_copper: true,
            show_texts: true,
            show_drawings: true,
            copper_opacity: default_copper_opacity(),
            layer_order: Vec::new(),
        }
    }
}
impl BoardDisplay {
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
    /// Ignore stale IDs and append newly available layers in their source order.
    pub fn ordered_layers(&self, source: impl IntoIterator<Item = LayerId>) -> Vec<LayerId> {
        let source: Vec<_> = source.into_iter().collect();
        let available: BTreeSet<_> = source.iter().copied().collect();
        let mut seen = BTreeSet::new();
        self.layer_order
            .iter()
            .copied()
            .chain(source)
            .filter(|layer| available.contains(layer) && seen.insert(*layer))
            .collect()
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
        self.show_drills
            && scope.is_none_or(|layers| layers.iter().any(|layer| self.layer_visible(*layer)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_unit_defaults_for_legacy_state_and_round_trips_mils() {
        let legacy: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        assert_eq!(legacy.length_unit, crate::units::LengthUnit::Millimeters);
        let display = BoardDisplay {
            length_unit: crate::units::LengthUnit::Mils,
            ..legacy
        };
        let restored: BoardDisplay =
            serde_json::from_str(&serde_json::to_string(&display).unwrap()).unwrap();
        assert_eq!(restored.length_unit, crate::units::LengthUnit::Mils);
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
    fn moving_layer_to_edge_preserves_other_layers_and_rejects_absent_target() {
        let source = [LayerId(1), LayerId(2), LayerId(3)];
        let mut display = BoardDisplay::default();
        assert!(display.move_layer_to_edge(LayerId(2), true, &source));
        assert_eq!(
            display.layer_order,
            vec![LayerId(1), LayerId(3), LayerId(2)]
        );
        assert!(!display.move_layer_to_edge(LayerId(2), true, &source));
        assert!(!display.move_layer_to_edge(LayerId(99), false, &source));
        assert!(display.move_layer_to_edge(LayerId(2), false, &source));
        assert_eq!(
            display.layer_order,
            vec![LayerId(2), LayerId(1), LayerId(3)]
        );
    }
    #[test]
    fn repeated_opacity_steps_reach_exact_endpoints_and_restore_default() {
        let mut display = BoardDisplay::default();
        for _ in 0..7 {
            display.adjust_copper_opacity(-5);
        }
        assert_eq!(display.copper_opacity, 0.0);
        for _ in 0..20 {
            display.adjust_copper_opacity(5);
        }
        assert_eq!(display.copper_opacity, 1.0);
        display.adjust_copper_opacity(5);
        assert_eq!(display.copper_opacity, 1.0);
        for _ in 0..13 {
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
