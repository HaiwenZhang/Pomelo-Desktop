//! Per-document display settings, independent of GPU resources and source layer order.
pub mod filter;
pub mod grid;
use crate::model::{LayerId, NetId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    #[default]
    Layer,
    Net,
}

/// Composition of base traces; selection and hover keep their overlay rendering.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceRenderMode {
    /// Union coverage before applying opacity once per layer.
    #[default]
    Stencil,
    /// Blend smooth signed-distance coverage per segment.
    Sdf,
}

/// Allegro-style display shadowing; independent of material transparency.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowMode {
    pub enabled: bool,
    pub dim_active: bool,
    pub brightness: f32,
}
impl Default for ShadowMode {
    fn default() -> Self {
        Self {
            enabled: false,
            dim_active: false,
            brightness: 0.6,
        }
    }
}

/// A transient Last snapshot. Restoring it swaps the two visibility states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerVisibility {
    hidden: BTreeSet<LayerId>,
    primitives: BTreeMap<LayerId, LayerPrimitives>,
    quickview: Option<PlacementQuickview>,
}
impl LayerVisibility {
    pub fn capture(display: &BoardDisplay) -> Self {
        Self {
            hidden: display.hidden_layers.clone(),
            primitives: display.layer_primitives.clone(),
            quickview: display.quickview,
        }
    }
    pub fn swap(&mut self, display: &mut BoardDisplay) {
        std::mem::swap(&mut self.hidden, &mut display.hidden_layers);
        std::mem::swap(&mut self.primitives, &mut display.layer_primitives);
        std::mem::swap(&mut self.quickview, &mut display.quickview);
    }
}

/// Independent embedded names and via span labels; source board text is separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LabelOptions {
    pub track_names: bool,
    pub pin_names: bool,
    pub via_names: bool,
    pub thru_labels: bool,
    pub bb_labels: bool,
    pub zone_names: bool,
}
impl Default for LabelOptions {
    fn default() -> Self {
        Self {
            track_names: true,
            pin_names: true,
            via_names: false,
            thru_labels: true,
            bb_labels: true,
            zone_names: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    TrackNames,
    PinNames,
    ViaNames,
    ZoneNames,
    ThroughSpans,
    BlindBuriedSpans,
}
impl LabelKind {
    pub const ALL: [Self; 6] = [
        Self::TrackNames,
        Self::PinNames,
        Self::ViaNames,
        Self::ZoneNames,
        Self::ThroughSpans,
        Self::BlindBuriedSpans,
    ];
}
impl LabelOptions {
    pub fn enabled(self, kind: LabelKind) -> bool {
        match kind {
            LabelKind::TrackNames => self.track_names,
            LabelKind::PinNames => self.pin_names,
            LabelKind::ViaNames => self.via_names,
            LabelKind::ZoneNames => self.zone_names,
            LabelKind::ThroughSpans => self.thru_labels,
            LabelKind::BlindBuriedSpans => self.bb_labels,
        }
    }
    pub fn set(&mut self, kind: LabelKind, enabled: bool) {
        *match kind {
            LabelKind::TrackNames => &mut self.track_names,
            LabelKind::PinNames => &mut self.pin_names,
            LabelKind::ViaNames => &mut self.via_names,
            LabelKind::ZoneNames => &mut self.zone_names,
            LabelKind::ThroughSpans => &mut self.thru_labels,
            LabelKind::BlindBuriedSpans => &mut self.bb_labels,
        } = enabled;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerPrimitive {
    Traces,
    Vias,
    Pads,
    Copper,
    Components,
}

impl LayerPrimitive {
    /// Columns of the layer matrix, in panel order.
    pub const MATRIX: [Self; 5] = [
        Self::Copper,
        Self::Traces,
        Self::Pads,
        Self::Vias,
        Self::Components,
    ];
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// How one object category of one layer is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMode {
    Fill,
    Outline,
    Hidden,
}
impl CellMode {
    pub fn visible(self) -> bool {
        self != Self::Hidden
    }
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
    Placement,
}

/// Temporary placement view, independent of the underlying copper visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementQuickview {
    pub layer: LayerId,
    pub pins: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LayerPriority {
    pub layer: LayerId,
    pub category: DisplayCategory,
}

/// A sparse per-layer override. Copper, traces, pads and vias are visible by
/// default; component bodies only appear when asked for. Visible cells are
/// filled unless their bit in `outline` asks for outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerPrimitives {
    pub traces: bool,
    pub vias: bool,
    pub pads: bool,
    #[serde(default = "default_visible")]
    pub copper: bool,
    #[serde(default)]
    pub components: bool,
    /// Bit per [`LayerPrimitive`]; absent in views saved before outline cells.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outline: u8,
}
fn is_zero(value: &u8) -> bool {
    *value == 0
}
impl Default for LayerPrimitives {
    fn default() -> Self {
        Self {
            traces: true,
            vias: true,
            pads: true,
            copper: true,
            components: false,
            outline: 0,
        }
    }
}
impl LayerPrimitives {
    pub fn visible(self, kind: LayerPrimitive) -> bool {
        match kind {
            LayerPrimitive::Traces => self.traces,
            LayerPrimitive::Vias => self.vias,
            LayerPrimitive::Pads => self.pads,
            LayerPrimitive::Copper => self.copper,
            LayerPrimitive::Components => self.components,
        }
    }
    pub fn mode(self, kind: LayerPrimitive) -> CellMode {
        if !self.visible(kind) {
            CellMode::Hidden
        } else if self.outline & kind.bit() != 0 {
            CellMode::Outline
        } else {
            CellMode::Fill
        }
    }
    pub fn set_visible(&mut self, kind: LayerPrimitive, visible: bool) {
        match kind {
            LayerPrimitive::Traces => self.traces = visible,
            LayerPrimitive::Vias => self.vias = visible,
            LayerPrimitive::Pads => self.pads = visible,
            LayerPrimitive::Copper => self.copper = visible,
            LayerPrimitive::Components => self.components = visible,
        }
    }
    pub fn set_mode(&mut self, kind: LayerPrimitive, mode: CellMode) {
        self.set_visible(kind, mode.visible());
        if mode == CellMode::Outline {
            self.outline |= kind.bit();
        } else {
            self.outline &= !kind.bit();
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDisplay {
    #[serde(default)]
    pub quickview: Option<PlacementQuickview>,
    #[serde(default)]
    pub object_filter: filter::ObjectFilter,
    #[serde(default)]
    pub grid: grid::GridOptions,
    #[serde(default)]
    pub shadow: ShadowMode,
    #[serde(default)]
    pub appearance: crate::appearance::BoardAppearance,
    #[serde(default)]
    pub priorities: Vec<LayerPriority>,
    #[serde(default)]
    pub active_layer: Option<LayerId>,
    #[serde(default = "default_visible")]
    pub filled: bool,
    #[serde(default)]
    pub color_mode: ColorMode,
    #[serde(default)]
    pub trace_render_mode: TraceRenderMode,
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
    /// Draw confirmed static copper as solid fill instead of the screen-space stipple.
    #[serde(default)]
    pub static_shapes_fill_solid: bool,
    #[serde(default)]
    pub show_texts: bool,
    /// Keep embedded pin net names horizontal, independently of footprint rotation.
    #[serde(default)]
    pub horizontal_pin_names: bool,
    /// Generated names may be limited to the active layer; source text is separate.
    #[serde(default)]
    pub active_layer_labels_only: bool,
    #[serde(default)]
    pub label_options: LabelOptions,
    #[serde(default = "default_visible")]
    pub show_drawings: bool,
    #[serde(default = "default_copper_opacity")]
    pub copper_opacity: f32,
    /// Opacity of ordinary objects and labels. Copper fill has its own opacity.
    #[serde(default = "default_global_opacity")]
    pub global_opacity: f32,
    /// Explicit drawing order, from bottom to top; empty uses source order.
    #[serde(default)]
    pub layer_order: Vec<LayerId>,
    /// Sparse per-layer opacity in 0..=1; absent layers are opaque.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layer_opacity: BTreeMap<LayerId, f32>,
    /// Nets hidden in both 2D and 3D; their objects are neither drawn nor picked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_nets: BTreeSet<NetId>,
}
fn default_copper_opacity() -> f32 {
    0.25
}
fn default_global_opacity() -> f32 {
    1.0
}
fn default_visible() -> bool {
    true
}
impl Default for BoardDisplay {
    fn default() -> Self {
        Self {
            quickview: None,
            object_filter: filter::ObjectFilter::default(),
            grid: grid::GridOptions::default(),
            shadow: ShadowMode::default(),
            appearance: crate::appearance::BoardAppearance::default(),
            priorities: Vec::new(),
            active_layer: None,
            filled: true,
            color_mode: ColorMode::default(),
            trace_render_mode: TraceRenderMode::default(),
            length_unit: crate::units::LengthUnit::default(),
            hidden_layers: BTreeSet::new(),
            layer_primitives: BTreeMap::new(),
            show_drills: true,
            show_backdrills: true,
            show_copper: true,
            static_shapes_fill_solid: false,
            show_texts: false,
            horizontal_pin_names: false,
            active_layer_labels_only: false,
            label_options: LabelOptions::default(),
            show_drawings: true,
            copper_opacity: default_copper_opacity(),
            global_opacity: default_global_opacity(),
            layer_order: Vec::new(),
            layer_opacity: BTreeMap::new(),
            hidden_nets: BTreeSet::new(),
        }
    }
}
impl BoardDisplay {
    /// Bounds shared by the live view and saved display snapshots.
    pub fn valid(&self) -> bool {
        self.quickview.is_none_or(|view| view.layer.is_copper())
            && self.hidden_layers.len() <= 4096
            && self.layer_primitives.len() <= 4096
            && self.layer_order.len() <= 4096
            && self.priorities.len() <= 4096
            && self.appearance.layers.len() <= 4096
            && self.appearance.nets.len() <= 4096
            && self
                .layer_order
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                == self.layer_order.len()
            && self.copper_opacity.is_finite()
            && (0.0..=1.0).contains(&self.copper_opacity)
            && self.global_opacity.is_finite()
            && (0.0..=1.0).contains(&self.global_opacity)
            && self.object_filter.valid()
            && self.grid.valid()
            && self.shadow.brightness.is_finite()
            && (0.0..=1.0).contains(&self.shadow.brightness)
            && self
                .layer_opacity
                .values()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    }
    /// Opacity multiplier of one layer; 1 unless the user lowered it.
    pub fn layer_alpha(&self, layer: LayerId) -> f32 {
        self.layer_opacity.get(&layer).copied().unwrap_or(1.0)
    }
    pub fn set_layer_alpha(&mut self, layer: LayerId, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha >= 1.0 || !alpha.is_finite() {
            self.layer_opacity.remove(&layer);
        } else {
            self.layer_opacity.insert(layer, alpha);
        }
    }
    /// Draw mode of a cell, including whole-layer visibility and the placement quickview.
    pub fn cell_mode(&self, layer: LayerId, kind: LayerPrimitive) -> CellMode {
        if !self.primitive_visible(layer, kind) {
            return CellMode::Hidden;
        }
        match self.primitives(layer).mode(kind) {
            CellMode::Hidden => CellMode::Fill,
            mode => mode,
        }
    }
    /// Shadowing changes RGB brightness, never coverage, picking or source colors.
    pub fn layer_brightness(&self, layer: LayerId) -> f32 {
        if self.shadow.enabled
            && self.active_layer.is_some()
            && (self.shadow.dim_active || self.active_layer != Some(layer))
        {
            self.shadow.brightness.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
    /// Compare only inputs that determine physical/backdrill visibility.
    pub fn same_drill_visibility(&self, other: &Self) -> bool {
        self.show_drills == other.show_drills
            && self.show_backdrills == other.show_backdrills
            && self.hidden_layers == other.hidden_layers
            && self
                .layer_primitives
                .keys()
                .chain(other.layer_primitives.keys())
                .all(|&layer| self.primitives(layer).vias == other.primitives(layer).vias)
    }

    /// Geometry and embedded material inputs used by MSDF label layout.
    /// Colors, opacity and trace composition are uniforms; rank affects the dense-label budget.
    pub fn same_label_layout(&self, other: &Self) -> bool {
        self.quickview == other.quickview
            && self.object_filter == other.object_filter
            && self.active_layer_labels_only == other.active_layer_labels_only
            && self.active_layer == other.active_layer
            && self.layer_order == other.layer_order
            && self.priorities == other.priorities
            && self.label_options == other.label_options
            && self.horizontal_pin_names == other.horizontal_pin_names
            && self.show_copper == other.show_copper
            && self.copper_opacity == other.copper_opacity
            && self.hidden_layers == other.hidden_layers
            && self
                .layer_primitives
                .keys()
                .chain(other.layer_primitives.keys())
                .all(|&layer| self.primitives(layer) == other.primitives(layer))
    }

    /// Back-to-front rank. An active copper layer wins over manual promotion;
    /// otherwise drills, vias and pins sit above etch across the entire stack.
    pub fn display_rank(&self, layer: LayerId, category: DisplayCategory) -> [i64; 5] {
        use DisplayCategory::*;
        let etch = matches!(category, Zone | ZoneOutline | Trace | Placement)
            || (category == Text && layer.is_copper());
        let group_category = match category {
            Zone | ZoneOutline | Placement => Trace,
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
            Zone | ZoneOutline | Trace | BondWire | Text | Placement => 2,
            Pin => 3,
            Via => 4,
            Drill => 5,
        };
        let part = match category {
            Zone => 0,
            ZoneOutline => 1,
            Trace | BondWire | Drawing => 2,
            Text => 3,
            Placement => 4,
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
        let mut state = self
            .layer_primitives
            .get(&layer)
            .copied()
            .unwrap_or_default();
        if let Some(view) = self.quickview.filter(|view| view.layer == layer) {
            state.pads = view.pins;
        }
        state
    }
    pub fn set_primitive(&mut self, layer: LayerId, kind: LayerPrimitive, visible: bool) {
        if let Some(view) = self.quickview.as_mut().filter(|view| view.layer == layer)
            && kind == LayerPrimitive::Pads
        {
            view.pins = visible;
            return;
        }
        let mut state = self
            .layer_primitives
            .get(&layer)
            .copied()
            .unwrap_or_default();
        state.set_visible(kind, visible);
        self.store_primitives(layer, state);
    }
    /// Set fill, outline or hidden for one cell; outlines survive hide and show.
    pub fn set_cell_mode(&mut self, layer: LayerId, kind: LayerPrimitive, mode: CellMode) {
        if let Some(view) = self.quickview.as_mut().filter(|view| view.layer == layer)
            && kind == LayerPrimitive::Pads
        {
            view.pins = mode.visible();
        }
        let mut state = self
            .layer_primitives
            .get(&layer)
            .copied()
            .unwrap_or_default();
        state.set_mode(kind, mode);
        self.store_primitives(layer, state);
    }
    fn store_primitives(&mut self, layer: LayerId, state: LayerPrimitives) {
        if state == LayerPrimitives::default() {
            self.layer_primitives.remove(&layer);
        } else {
            self.layer_primitives.insert(layer, state);
        }
    }
    pub fn primitive_visible(&self, layer: LayerId, kind: LayerPrimitive) -> bool {
        if kind == LayerPrimitive::Pads
            && let Some(view) = self.quickview.filter(|view| view.layer == layer)
        {
            return view.pins;
        }
        self.layer_visible(layer) && self.primitives(layer).visible(kind)
    }
    pub fn category_visible(&self, layer: LayerId, category: DisplayCategory) -> bool {
        if category == DisplayCategory::Placement {
            return self.quickview.is_some_and(|view| view.layer == layer)
                || self.primitive_visible(layer, LayerPrimitive::Components);
        }
        if matches!(
            category,
            DisplayCategory::Zone | DisplayCategory::ZoneOutline
        ) {
            return self.primitive_visible(layer, LayerPrimitive::Copper);
        }
        self.layer_visible(layer)
            || (category == DisplayCategory::Pin
                && self.primitive_visible(layer, LayerPrimitive::Pads))
    }
    pub fn object_visible(&self, layer: LayerId, object: crate::selection::SelectedObject) -> bool {
        use crate::selection::SelectedObject;
        match object {
            SelectedObject::Segment(_) => self.primitive_visible(layer, LayerPrimitive::Traces),
            SelectedObject::Pin(_) => self.primitive_visible(layer, LayerPrimitive::Pads),
            SelectedObject::Via(_) => self.primitive_visible(layer, LayerPrimitive::Vias),
            SelectedObject::Zone(_) => {
                self.show_copper && self.primitive_visible(layer, LayerPrimitive::Copper)
            }
            SelectedObject::Drawing(_) => self.show_drawings && self.layer_visible(layer),
            SelectedObject::Placement(_) => {
                self.category_visible(layer, DisplayCategory::Placement)
            }
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
    fn placement_quickviews_temporarily_show_pins_without_changing_copper_masks() {
        let layer = LayerId(0);
        let mut display = BoardDisplay::default();
        display.hidden_layers.insert(layer);
        let original = display.clone();
        display.quickview = Some(PlacementQuickview { layer, pins: true });
        assert!(display.primitive_visible(layer, LayerPrimitive::Pads));
        assert!(display.category_visible(layer, DisplayCategory::Placement));
        for category in [
            DisplayCategory::Zone,
            DisplayCategory::Trace,
            DisplayCategory::Via,
            DisplayCategory::Text,
        ] {
            assert!(!display.category_visible(layer, category));
        }
        assert!(!display.same_label_layout(&original));
        assert!(display.same_drill_visibility(&original));
        display.set_primitive(layer, LayerPrimitive::Pads, false);
        assert!(!display.primitive_visible(layer, LayerPrimitive::Pads));
        assert!(display.category_visible(layer, DisplayCategory::Placement));
        assert_eq!(display.layer_primitives, original.layer_primitives);
        let mut last = LayerVisibility::capture(&display);
        display.quickview = None;
        assert_eq!(display, original);
        last.swap(&mut display);
        assert!(display.quickview.is_some());
        display.quickview = None;
        assert_eq!(display, original);
    }
    #[test]
    fn unrelated_display_changes_keep_layout_and_drill_visibility_caches() {
        let original = BoardDisplay::default();
        let mut changed = original.clone();
        changed.trace_render_mode = TraceRenderMode::Sdf;
        changed.color_mode = ColorMode::Net;
        changed.global_opacity = 0.3;
        changed.filled = false;
        changed.show_texts = true;
        changed.show_drawings = false;
        changed.appearance.set_color(
            crate::appearance::ColorTarget::Background,
            Some(crate::appearance::RgbColor([12, 34, 56])),
        );
        assert!(original.same_label_layout(&changed));
        assert!(original.same_drill_visibility(&changed));
        changed
            .layer_primitives
            .insert(LayerId(1), LayerPrimitives::default());
        assert!(original.same_label_layout(&changed));
        assert!(original.same_drill_visibility(&changed));
    }
    #[test]
    fn layer_ranking_invalidates_label_budget_but_not_drill_geometry() {
        let original = BoardDisplay::default();
        for changed in [
            BoardDisplay {
                active_layer: Some(LayerId(2)),
                ..original.clone()
            },
            BoardDisplay {
                layer_order: vec![LayerId(2), LayerId(1)],
                ..original.clone()
            },
            BoardDisplay {
                priorities: vec![LayerPriority {
                    layer: LayerId(2),
                    category: DisplayCategory::Trace,
                }],
                ..original.clone()
            },
        ] {
            assert!(!original.same_label_layout(&changed));
            assert!(original.same_drill_visibility(&changed));
        }
    }
    #[test]
    fn semantic_cache_keys_invalidate_only_their_geometry_inputs() {
        let original = BoardDisplay::default();
        let mut changed = original.clone();
        changed.show_drills = false;
        assert!(original.same_label_layout(&changed));
        assert!(!original.same_drill_visibility(&changed));
        changed = original.clone();
        changed.horizontal_pin_names = true;
        assert!(!original.same_label_layout(&changed));
        assert!(original.same_drill_visibility(&changed));
        changed = original.clone();
        changed.copper_opacity = 0.5;
        assert!(!original.same_label_layout(&changed));
        assert!(original.same_drill_visibility(&changed));
        changed = original.clone();
        changed.set_primitive(LayerId(1), LayerPrimitive::Pads, false);
        assert!(!original.same_label_layout(&changed));
        assert!(original.same_drill_visibility(&changed));
        changed.set_primitive(LayerId(1), LayerPrimitive::Vias, false);
        assert!(!original.same_drill_visibility(&changed));
        changed = original.clone();
        changed.hidden_layers.insert(LayerId(1));
        assert!(!original.same_label_layout(&changed));
        assert!(!original.same_drill_visibility(&changed));
    }
    #[test]
    fn label_categories_restore_independently_and_keep_legacy_defaults() {
        let mut display: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        let defaults = LabelOptions::default();
        assert_eq!(display.label_options, defaults);
        for kind in LabelKind::ALL {
            display.label_options = defaults;
            display.label_options.set(kind, !defaults.enabled(kind));
            let restored: BoardDisplay =
                serde_json::from_slice(&serde_json::to_vec(&display).unwrap()).unwrap();
            for other in LabelKind::ALL {
                assert_eq!(
                    restored.label_options.enabled(other),
                    if kind == other {
                        !defaults.enabled(other)
                    } else {
                        defaults.enabled(other)
                    }
                );
            }
        }
        let partial: LabelOptions = serde_json::from_str(r#"{"bb_labels":false}"#).unwrap();
        assert!(!partial.bb_labels);
        assert!(
            partial.track_names && partial.pin_names && partial.thru_labels && partial.zone_names
        );
        assert!(!partial.via_names);
    }
    #[test]
    fn horizontal_pin_names_preserves_legacy_orientation_and_saved_choice() {
        let legacy: BoardDisplay =
            serde_json::from_str(r#"{"hidden_layers":[],"show_drills":true}"#).unwrap();
        assert!(!legacy.horizontal_pin_names);
        let horizontal = BoardDisplay {
            horizontal_pin_names: true,
            ..legacy
        };
        let restored: BoardDisplay =
            serde_json::from_slice(&serde_json::to_vec(&horizontal).unwrap()).unwrap();
        assert!(restored.horizontal_pin_names);
    }
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
    fn matrix_cells_keep_outline_choices_and_old_views_still_decode() {
        let layer = LayerId(2);
        let mut display = BoardDisplay::default();
        assert_eq!(
            display.cell_mode(layer, LayerPrimitive::Copper),
            CellMode::Fill
        );
        assert_eq!(
            display.cell_mode(layer, LayerPrimitive::Components),
            CellMode::Hidden
        );
        display.set_cell_mode(layer, LayerPrimitive::Copper, CellMode::Outline);
        display.set_primitive(layer, LayerPrimitive::Copper, false);
        assert!(!display.category_visible(layer, DisplayCategory::Zone));
        display.set_primitive(layer, LayerPrimitive::Copper, true);
        assert_eq!(
            display.cell_mode(layer, LayerPrimitive::Copper),
            CellMode::Outline
        );
        display.set_cell_mode(layer, LayerPrimitive::Copper, CellMode::Fill);
        assert!(display.layer_primitives.is_empty());
        let old: LayerPrimitives =
            serde_json::from_str(r#"{"traces":true,"vias":false,"pads":true}"#).unwrap();
        assert!(old.copper && !old.components && !old.vias && old.outline == 0);
        display.set_cell_mode(layer, LayerPrimitive::Components, CellMode::Fill);
        assert!(display.category_visible(layer, DisplayCategory::Placement));
        let restored: BoardDisplay =
            serde_json::from_slice(&serde_json::to_vec(&display).unwrap()).unwrap();
        assert_eq!(restored, display);
    }
    #[test]
    fn layer_opacity_is_sparse_and_validated() {
        let mut display = BoardDisplay::default();
        display.set_layer_alpha(LayerId(1), 0.3);
        assert_eq!(display.layer_alpha(LayerId(1)), 0.3);
        assert_eq!(display.layer_alpha(LayerId(2)), 1.0);
        display.set_layer_alpha(LayerId(1), 1.0);
        assert!(display.layer_opacity.is_empty());
        display.layer_opacity.insert(LayerId(1), 1.5);
        assert!(!display.valid());
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

#[cfg(test)]
mod display_reference_tests {
    use super::*;
    #[test]
    fn shadow_preserves_active_layer_and_independent_alpha_inputs() {
        let mut display = BoardDisplay {
            active_layer: Some(LayerId(0)),
            ..Default::default()
        };
        display.shadow.enabled = true;
        assert_eq!(display.layer_brightness(LayerId(0)), 1.0);
        assert_eq!(display.layer_brightness(LayerId(2)), 0.6);
        display.shadow.dim_active = true;
        assert_eq!(display.layer_brightness(LayerId(0)), 0.6);
        assert_eq!(
            (display.global_opacity, display.copper_opacity),
            (1.0, 0.25)
        );
        display.active_layer = None;
        assert_eq!(display.layer_brightness(LayerId(2)), 1.0);
    }
    #[test]
    fn last_visibility_swaps_category_masks_without_changing_colors_or_active_layer() {
        let mut display = BoardDisplay {
            active_layer: Some(LayerId(0)),
            ..Default::default()
        };
        display.hidden_layers.insert(LayerId(2));
        display.set_primitive(LayerId(0), LayerPrimitive::Pads, false);
        let mut last = LayerVisibility::capture(&display);
        display.hidden_layers.clear();
        display.layer_primitives.clear();
        last.swap(&mut display);
        assert!(!display.layer_visible(LayerId(2)));
        assert!(!display.primitive_visible(LayerId(0), LayerPrimitive::Pads));
        assert_eq!(display.active_layer, Some(LayerId(0)));
        last.swap(&mut display);
        assert!(display.layer_visible(LayerId(2)));
        assert!(display.primitive_visible(LayerId(0), LayerPrimitive::Pads));
    }
}
