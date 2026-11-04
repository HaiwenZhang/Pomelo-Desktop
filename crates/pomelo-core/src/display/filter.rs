//! Screen-size display filtering. Sizes are measured in logical pixels.
use crate::model::Bounds;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilterKind {
    Text,
    Via,
    Pin,
    Copper,
    Void,
    EmbeddedName,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectFilter {
    pub minimum_pixels: u16,
    pub text: bool,
    pub vias: bool,
    pub pins: bool,
    pub copper: bool,
    pub voids: bool,
    pub embedded_names: bool,
}
impl Default for ObjectFilter {
    fn default() -> Self {
        Self {
            minimum_pixels: 0,
            text: true,
            vias: true,
            pins: true,
            copper: true,
            voids: true,
            embedded_names: true,
        }
    }
}
impl ObjectFilter {
    pub fn enabled(self, kind: FilterKind) -> bool {
        match kind {
            FilterKind::Text => self.text,
            FilterKind::Via => self.vias,
            FilterKind::Pin => self.pins,
            FilterKind::Copper => self.copper,
            FilterKind::Void => self.voids,
            FilterKind::EmbeddedName => self.embedded_names,
        }
    }
    pub fn set(&mut self, kind: FilterKind, enabled: bool) {
        *match kind {
            FilterKind::Text => &mut self.text,
            FilterKind::Via => &mut self.vias,
            FilterKind::Pin => &mut self.pins,
            FilterKind::Copper => &mut self.copper,
            FilterKind::Void => &mut self.voids,
            FilterKind::EmbeddedName => &mut self.embedded_names,
        } = enabled;
    }
    pub fn threshold(self, kind: FilterKind) -> f32 {
        if self.enabled(kind) {
            f32::from(self.minimum_pixels)
        } else {
            0.0
        }
    }
    pub fn visible(self, kind: FilterKind, size_mm: f64, pixels_per_mm: f64) -> bool {
        let threshold = self.threshold(kind);
        threshold == 0.0
            || (size_mm.is_finite()
                && pixels_per_mm.is_finite()
                && size_mm.max(0.0) * pixels_per_mm >= f64::from(threshold))
    }
    pub fn bounds_visible(self, kind: FilterKind, bounds: Bounds, pixels_per_mm: f64) -> bool {
        self.visible(
            kind,
            (bounds.max.x - bounds.min.x).min(bounds.max.y - bounds.min.y),
            pixels_per_mm,
        )
    }
    pub fn pad_visible(
        self,
        kind: FilterKind,
        pad: &crate::model::Pad,
        owner: crate::pad::PadPlacement,
        scale: f64,
    ) -> bool {
        if pad.custom.is_some() {
            pad.bounds(owner)
                .is_none_or(|b| self.bounds_visible(kind, b, scale))
        } else {
            self.visible(kind, pad.width.min(pad.height), scale)
        }
    }
    pub fn valid(self) -> bool {
        self.minimum_pixels <= 100
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_threshold_is_inclusive_category_specific_and_zoom_dependent() {
        let mut filter = ObjectFilter {
            minimum_pixels: 20,
            ..Default::default()
        };
        assert!(!filter.visible(FilterKind::Via, 0.5, 39.0));
        assert!(filter.visible(FilterKind::Via, 0.5, 40.0));
        filter.vias = false;
        assert!(filter.visible(FilterKind::Via, 0.5, 1.0));
        assert!(!filter.visible(FilterKind::Pin, 0.5, 1.0));
        assert!(ObjectFilter::default().visible(FilterKind::Text, 0.0, 1.0));
    }
}
