//! Named views and navigation history.
//!
//! A named view stores the camera and document display settings. Older snapshots
//! restore their original layer visibility and color mode only.
//! History records completed navigation actions only, never individual pointer moves.
use crate::{
    display::{BoardDisplay, ColorMode},
    interaction::Camera,
    model::LayerId,
};
use std::collections::{BTreeSet, VecDeque};

pub const MAX_NAMED_VIEWS: usize = 64;
pub const HISTORY_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedView {
    pub id: u32,
    pub name: String,
    pub camera: Camera,
    pub hidden_layers: BTreeSet<LayerId>,
    pub color_mode: ColorMode,
    /// Complete display state; absent in legacy views with only the fields above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<BoardDisplay>,
}

impl NamedView {
    pub fn capture(id: u32, name: &str, camera: Camera, display: &BoardDisplay) -> Self {
        Self {
            id,
            name: super::highlight::normalized_name(name),
            camera,
            hidden_layers: display.hidden_layers.clone(),
            color_mode: display.color_mode,
            display: Some(display.clone()),
        }
    }

    /// Apply the snapshot, preserving legacy views' original narrower scope.
    /// Returns whether the document display changed.
    pub fn apply_display(&self, display: &mut BoardDisplay) -> bool {
        if let Some(saved) = &self.display {
            if saved == display {
                return false;
            }
            *display = saved.clone();
        } else {
            if display.hidden_layers == self.hidden_layers && display.color_mode == self.color_mode
            {
                return false;
            }
            display.hidden_layers = self.hidden_layers.clone();
            display.color_mode = self.color_mode;
        }
        true
    }

    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.chars().count() <= super::highlight::MAX_NAME_CHARS
            && self.camera.is_renderable()
            && self.hidden_layers.len() <= 4096
            && self.display.as_ref().is_none_or(BoardDisplay::valid)
    }
}

/// Views that differ only by floating-point noise are the same history entry.
pub fn equivalent(a: Camera, b: Camera) -> bool {
    let scale = a.pixels_per_mm.max(b.pixels_per_mm);
    // Half a logical pixel of centre movement or 0.1% zoom is not a new place.
    a.flipped == b.flipped
        && (a.pixels_per_mm - b.pixels_per_mm).abs() <= scale * 1e-3
        && (a.center.x - b.center.x).abs() * scale <= 0.5
        && (a.center.y - b.center.y).abs() * scale <= 0.5
}

#[derive(Debug, Default, Clone)]
pub struct NavigationHistory {
    back: VecDeque<Camera>,
    forward: Vec<Camera>,
}

impl NavigationHistory {
    /// Record that a navigation action moved the view away from `previous`.
    /// Equivalent consecutive entries merge; a new action clears the forward stack.
    pub fn record(&mut self, previous: Camera, next: Camera) {
        if !previous.is_renderable() || equivalent(previous, next) {
            return;
        }
        if !self
            .back
            .back()
            .is_some_and(|last| equivalent(*last, previous))
        {
            self.back.push_back(previous);
            if self.back.len() > HISTORY_CAPACITY {
                self.back.pop_front();
            }
        }
        self.forward.clear();
    }

    /// Step back from `current`; returns the camera to restore.
    pub fn back(&mut self, current: Camera) -> Option<Camera> {
        let target = self.back.pop_back()?;
        self.forward.push(current);
        Some(target)
    }

    pub fn forward(&mut self, current: Camera) -> Option<Camera> {
        let target = self.forward.pop()?;
        self.back.push_back(current);
        if self.back.len() > HISTORY_CAPACITY {
            self.back.pop_front();
        }
        Some(target)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;

    fn camera(x: f64, scale: f64) -> Camera {
        Camera {
            center: Point::new(x, 0.0),
            pixels_per_mm: scale,
            flipped: false,
        }
    }

    #[test]
    fn back_and_forward_walk_recorded_actions() {
        let mut history = NavigationHistory::default();
        let (a, b, c) = (camera(0.0, 10.0), camera(10.0, 10.0), camera(20.0, 20.0));
        history.record(a, b);
        history.record(b, c);
        assert_eq!(history.back(c), Some(b));
        assert_eq!(history.back(b), Some(a));
        assert_eq!(history.back(a), None);
        assert_eq!(history.forward(a), Some(b));
        assert_eq!(history.forward(b), Some(c));
        assert!(!history.can_go_forward());
    }

    #[test]
    fn a_new_action_clears_forward_and_equivalent_entries_merge() {
        let mut history = NavigationHistory::default();
        let (a, b) = (camera(0.0, 10.0), camera(10.0, 10.0));
        history.record(a, b);
        history.back(b);
        assert!(history.can_go_forward());
        history.record(a, camera(30.0, 10.0));
        assert!(!history.can_go_forward());
        // Sub-pixel noise does not create entries.
        history.record(camera(30.0, 10.0), camera(30.01, 10.0));
        history.record(camera(30.0, 10.0), camera(40.0, 10.0));
        let mut steps = 0;
        let mut current = camera(40.0, 10.0);
        while let Some(previous) = history.back(current) {
            current = previous;
            steps += 1;
        }
        assert_eq!(steps, 2);
    }

    #[test]
    fn capacity_is_bounded() {
        let mut history = NavigationHistory::default();
        for index in 0..(HISTORY_CAPACITY + 10) {
            history.record(camera(index as f64, 10.0), camera(index as f64 + 0.5, 10.0));
        }
        let mut steps = 0;
        let mut current = camera(0.0, 1.0);
        while let Some(previous) = history.back(current) {
            current = previous;
            steps += 1;
        }
        assert_eq!(steps, HISTORY_CAPACITY);
    }

    #[test]
    fn named_views_restore_full_display_even_when_layer_visibility_and_color_mode_match() {
        use crate::{
            appearance::{ColorTarget, RgbColor, pattern::FillPattern},
            display::{DisplayCategory, LayerPrimitive, LayerPriority},
        };
        let mut display = BoardDisplay {
            color_mode: ColorMode::Net,
            filled: false,
            show_texts: true,
            active_layer: Some(LayerId(2)),
            active_layer_labels_only: true,
            horizontal_pin_names: true,
            show_backdrills: false,
            show_drawings: false,
            copper_opacity: 0.72,
            global_opacity: 0.38,
            length_unit: crate::units::LengthUnit::Mils,
            trace_render_mode: crate::display::TraceRenderMode::Sdf,
            layer_order: vec![LayerId(2), LayerId(1)],
            priorities: vec![LayerPriority {
                layer: LayerId(2),
                category: DisplayCategory::Pin,
            }],
            ..BoardDisplay::default()
        };
        display.hidden_layers.insert(LayerId(3));
        display.set_primitive(LayerId(2), LayerPrimitive::Pads, false);
        display.label_options.zone_names = false;
        display.shadow.enabled = true;
        display.shadow.brightness = 0.4;
        display.grid.visible = true;
        display.grid.spacing_mm = 0.25;
        display.object_filter.minimum_pixels = 21;
        display
            .appearance
            .set_color(ColorTarget::Background, Some(RgbColor([12, 34, 56])));
        display.appearance.set_pattern(
            ColorTarget::Etch(LayerId(2)),
            Some(FillPattern::DiagonalDown),
        );
        let view = NamedView::capture(1, " Top ", camera(1.0, 2.0), &display);
        let view: NamedView = serde_json::from_str(&serde_json::to_string(&view).unwrap()).unwrap();
        assert_eq!(view.name, "Top");
        let mut restored = BoardDisplay {
            hidden_layers: display.hidden_layers.clone(),
            color_mode: display.color_mode,
            ..BoardDisplay::default()
        };
        assert!(view.apply_display(&mut restored));
        assert_eq!(restored, display);
        assert!(!view.apply_display(&mut restored));
        assert!(view.valid());
    }

    #[test]
    fn legacy_named_views_preserve_their_original_display_scope() {
        let mut saved = BoardDisplay {
            color_mode: ColorMode::Net,
            ..Default::default()
        };
        saved.hidden_layers.insert(LayerId(3));
        let mut json =
            serde_json::to_value(NamedView::capture(1, "Legacy", camera(1.0, 2.0), &saved))
                .unwrap();
        json.as_object_mut().unwrap().remove("display");
        let legacy: NamedView = serde_json::from_value(json).unwrap();
        let mut current = BoardDisplay {
            copper_opacity: 0.82,
            show_texts: true,
            ..Default::default()
        };
        current.grid.visible = true;
        let mut expected = current.clone();
        expected.hidden_layers = saved.hidden_layers;
        expected.color_mode = saved.color_mode;
        assert!(legacy.apply_display(&mut current));
        assert_eq!(current, expected);
        assert!(legacy.valid());
    }

    #[test]
    fn named_views_reject_invalid_nested_display_settings() {
        let display = BoardDisplay::default();
        for invalid in [f32::NAN, -0.1, 1.1] {
            let mut view = NamedView::capture(1, "Invalid", camera(1.0, 2.0), &display);
            view.display.as_mut().unwrap().shadow.brightness = invalid;
            assert!(!view.valid());
        }
        let mut view = NamedView::capture(1, "Invalid", camera(1.0, 2.0), &display);
        view.display.as_mut().unwrap().layer_order = vec![LayerId(1), LayerId(1)];
        assert!(!view.valid());
    }
}
