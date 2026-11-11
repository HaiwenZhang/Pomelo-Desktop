//! Visibility views: named combinations of layer and category visibility.
//!
//! Unlike named views they never store a camera, so restoring one keeps the
//! current position in both 2D and 3D.
use crate::{
    display::{BoardDisplay, LayerPrimitives},
    model::{Layer, LayerFunction, LayerId, NetId},
};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_VISIBILITY_VIEWS: usize = 32;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilityView {
    pub id: u32,
    pub name: String,
    pub hidden_layers: BTreeSet<LayerId>,
    #[serde(default)]
    pub layer_primitives: BTreeMap<LayerId, LayerPrimitives>,
    #[serde(default)]
    pub layer_opacity: BTreeMap<LayerId, f32>,
    #[serde(default)]
    pub hidden_nets: BTreeSet<NetId>,
}

impl VisibilityView {
    pub fn capture(id: u32, name: &str, display: &BoardDisplay) -> Self {
        Self {
            id,
            name: super::highlight::normalized_name(name),
            hidden_layers: display.hidden_layers.clone(),
            layer_primitives: display.layer_primitives.clone(),
            layer_opacity: display.layer_opacity.clone(),
            hidden_nets: display.hidden_nets.clone(),
        }
    }

    /// Returns whether the display changed.
    pub fn apply(&self, display: &mut BoardDisplay) -> bool {
        let before = (
            display.hidden_layers.clone(),
            display.layer_primitives.clone(),
            display.layer_opacity.clone(),
            display.hidden_nets.clone(),
        );
        display.hidden_layers = self.hidden_layers.clone();
        display.layer_primitives = self.layer_primitives.clone();
        display.layer_opacity = self.layer_opacity.clone();
        display.hidden_nets = self.hidden_nets.clone();
        before
            != (
                display.hidden_layers.clone(),
                display.layer_primitives.clone(),
                display.layer_opacity.clone(),
                display.hidden_nets.clone(),
            )
    }

    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.chars().count() <= super::highlight::MAX_NAME_CHARS
            && self
                .layer_opacity
                .values()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    }
}

/// Built-in layer selections. They change conductor layers only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityPreset {
    All,
    /// The outer conductor layers.
    Surface,
    /// Conductor layers that carry routing rather than planes.
    Signal,
    PowerGround,
}

impl VisibilityPreset {
    pub const ALL: [Self; 4] = [Self::All, Self::Surface, Self::Signal, Self::PowerGround];

    /// Conductor layers this preset shows, in stack order.
    pub fn shown(self, layers: &[Layer]) -> Vec<LayerId> {
        let copper: Vec<_> = layers
            .iter()
            .filter(|layer| layer.id.is_copper() && layer.function != LayerFunction::Dielectric)
            .collect();
        match self {
            Self::All => copper.iter().map(|layer| layer.id).collect(),
            Self::Surface => {
                let mut ids: Vec<_> = copper
                    .first()
                    .into_iter()
                    .chain(copper.last())
                    .map(|layer| layer.id)
                    .collect();
                ids.dedup();
                ids
            }
            Self::Signal => copper
                .iter()
                .filter(|layer| layer.function != LayerFunction::Plane)
                .map(|layer| layer.id)
                .collect(),
            Self::PowerGround => copper
                .iter()
                .filter(|layer| layer.function == LayerFunction::Plane)
                .map(|layer| layer.id)
                .collect(),
        }
    }

    /// Show the preset's conductor layers and hide the other conductor layers.
    /// Returns whether the display changed.
    pub fn apply(self, layers: &[Layer], display: &mut BoardDisplay) -> bool {
        let shown: BTreeSet<_> = self.shown(layers).into_iter().collect();
        let mut changed = false;
        for layer in layers
            .iter()
            .filter(|layer| layer.id.is_copper() && layer.function != LayerFunction::Dielectric)
        {
            changed |= if shown.contains(&layer.id) {
                display.hidden_layers.remove(&layer.id)
            } else {
                display.hidden_layers.insert(layer.id)
            };
        }
        changed
    }

    /// Whether the display currently matches the preset on conductor layers.
    pub fn matches(self, layers: &[Layer], display: &BoardDisplay) -> bool {
        let shown: BTreeSet<_> = self.shown(layers).into_iter().collect();
        layers
            .iter()
            .filter(|layer| layer.id.is_copper() && layer.function != LayerFunction::Dielectric)
            .all(|layer| display.layer_visible(layer.id) == shown.contains(&layer.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack() -> Vec<Layer> {
        [
            LayerFunction::Conductor,
            LayerFunction::Plane,
            LayerFunction::Conductor,
            LayerFunction::Plane,
            LayerFunction::Conductor,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, function)| Layer {
            id: LayerId(i as u32),
            name: format!("L{i}"),
            function,
            color: "#ffffff".into(),
            source_flags: None,
        })
        .collect()
    }

    #[test]
    fn presets_select_outer_signal_and_plane_layers() {
        let layers = stack();
        let ids = |preset: VisibilityPreset| -> Vec<u32> {
            preset.shown(&layers).iter().map(|id| id.0).collect()
        };
        assert_eq!(ids(VisibilityPreset::All), [0, 1, 2, 3, 4]);
        assert_eq!(ids(VisibilityPreset::Surface), [0, 4]);
        assert_eq!(ids(VisibilityPreset::Signal), [0, 2, 4]);
        assert_eq!(ids(VisibilityPreset::PowerGround), [1, 3]);
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(LayerId::BOND_TOP);
        assert!(VisibilityPreset::PowerGround.apply(&layers, &mut display));
        assert!(VisibilityPreset::PowerGround.matches(&layers, &display));
        assert!(!VisibilityPreset::All.matches(&layers, &display));
        assert!(display.hidden_layers.contains(&LayerId::BOND_TOP));
        assert!(!VisibilityPreset::PowerGround.apply(&layers, &mut display));
    }

    #[test]
    fn views_restore_layers_cells_and_opacity_without_a_camera() {
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(LayerId(2));
        display.set_layer_alpha(LayerId(1), 0.4);
        let view = VisibilityView::capture(3, " Inner ", &display);
        assert_eq!(view.name, "Inner");
        let mut other = BoardDisplay::default();
        assert!(view.apply(&mut other));
        assert_eq!(other.hidden_layers, display.hidden_layers);
        assert_eq!(other.layer_alpha(LayerId(1)), 0.4);
        assert!(!view.apply(&mut other));
        assert!(view.valid());
    }
}
