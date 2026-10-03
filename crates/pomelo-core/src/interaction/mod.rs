//! Board camera uses viewport-local logical pixels. DPI belongs to the host.
pub mod component_reference;
pub mod picking;
pub mod picking_index;
pub mod selection;
pub(crate) mod selection_bounds;
pub(crate) mod spatial;
pub(crate) mod zone_index;

use crate::model::{Bounds, Point};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub center: Point,
    pub pixels_per_mm: f64,
    pub flipped: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: Point::default(),
            pixels_per_mm: 10.0,
            flipped: false,
        }
    }
}

/// Physical source length and corresponding logical screen length of a scale bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleBar {
    pub millimeters: f64,
    pub pixels: f64,
}

impl Camera {
    /// Select readable ticks in the display unit while returning source millimeters.
    pub fn scale_bar_in_unit(
        self,
        maximum_pixels: f64,
        unit: crate::units::LengthUnit,
    ) -> Option<ScaleBar> {
        let mut unit_camera = self;
        unit_camera.pixels_per_mm *= unit.millimeters_per_unit();
        let mut scale = unit_camera.scale_bar(maximum_pixels)?;
        scale.millimeters = unit.to_millimeters(scale.millimeters);
        Some(scale)
    }
    /// Choose a 1/2/5 source-space scale that fits the available logical pixels.
    /// DPI conversion belongs to the canvas; this uses the same units as navigation.
    pub fn scale_bar(self, maximum_pixels: f64) -> Option<ScaleBar> {
        if !self.is_renderable() || !maximum_pixels.is_finite() || maximum_pixels <= 0.0 {
            return None;
        }
        let maximum_mm = maximum_pixels / self.pixels_per_mm;
        if !maximum_mm.is_finite() || maximum_mm <= 0.0 {
            return None;
        }
        let decade = 10.0_f64.powf(maximum_mm.log10().floor());
        let normalized = maximum_mm / decade;
        let step = if normalized >= 5.0 {
            5.0
        } else if normalized >= 2.0 {
            2.0
        } else {
            1.0
        };
        let millimeters = step * decade;
        let pixels = millimeters * self.pixels_per_mm;
        (millimeters.is_finite() && millimeters > 0.0 && pixels.is_finite() && pixels > 0.0)
            .then_some(ScaleBar {
                millimeters,
                pixels,
            })
    }
    /// GPU camera uniforms use f32, including the high part of split coordinates.
    pub fn is_renderable(self) -> bool {
        [self.center.x, self.center.y, self.pixels_per_mm]
            .into_iter()
            .all(|value| value.is_finite() && (value as f32).is_finite())
            && (self.pixels_per_mm as f32) > 0.0
    }

    pub fn fit(&mut self, bounds: Bounds, width: f64, height: f64, padding: f64) -> bool {
        if !bounds.is_valid()
            || !width.is_finite()
            || !height.is_finite()
            || !padding.is_finite()
            || padding < 0.0
            || width <= 2.0 * padding
            || height <= 2.0 * padding
        {
            return false;
        }
        let board_width = (bounds.max.x - bounds.min.x).max(1e-6);
        let board_height = (bounds.max.y - bounds.min.y).max(1e-6);
        let center = bounds.center();
        let scale =
            ((width - 2.0 * padding) / board_width).min((height - 2.0 * padding) / board_height);
        if !center.x.is_finite() || !center.y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        self.center = center;
        self.pixels_per_mm = scale;
        true
    }

    /// Fit the board in its logical canvas, with the Web viewer's 86% occupancy.
    /// Desktop panels sit outside this canvas, so no overlay insets are needed.
    pub fn fit_board(&mut self, bounds: Bounds, width: f64, height: f64) -> bool {
        if !bounds.is_valid()
            || ![width, height]
                .into_iter()
                .all(|n| n.is_finite() && n > 0.0)
        {
            return false;
        }
        let next = Self {
            center: bounds.center(),
            pixels_per_mm: (width.max(1.0) / (bounds.max.x - bounds.min.x).max(0.001))
                .min(height.max(1.0) / (bounds.max.y - bounds.min.y).max(0.001))
                * 0.86,
            flipped: self.flipped,
        };
        if !next.is_renderable() {
            return false;
        }
        *self = next;
        true
    }

    pub fn board_to_view(self, point: Point, width: f64, height: f64) -> Point {
        let sign = if self.flipped { -1.0 } else { 1.0 };
        Point::new(
            width * 0.5 + sign * (point.x - self.center.x) * self.pixels_per_mm,
            height * 0.5 - (point.y - self.center.y) * self.pixels_per_mm,
        )
    }

    pub fn view_to_board(self, point: Point, width: f64, height: f64) -> Point {
        let sign = if self.flipped { -1.0 } else { 1.0 };
        Point::new(
            self.center.x + sign * (point.x - width * 0.5) / self.pixels_per_mm,
            self.center.y - (point.y - height * 0.5) / self.pixels_per_mm,
        )
    }

    pub fn zoom_at(&mut self, anchor: Point, width: f64, height: f64, factor: f64) -> bool {
        if !factor.is_finite()
            || factor <= 0.0
            || !anchor.x.is_finite()
            || !anchor.y.is_finite()
            || !width.is_finite()
            || width <= 0.0
            || !height.is_finite()
            || height <= 0.0
            || !self.pixels_per_mm.is_finite()
            || self.pixels_per_mm <= 0.0
        {
            return false;
        }
        let before = self.view_to_board(anchor, width, height);
        let mut next = *self;
        next.pixels_per_mm = (self.pixels_per_mm * factor).clamp(0.01, 1e7);
        let after = next.view_to_board(anchor, width, height);
        next.center.x += before.x - after.x;
        next.center.y += before.y - after.y;
        if !next.center.x.is_finite() || !next.center.y.is_finite() {
            return false;
        }
        *self = next;
        true
    }

    /// Move the displayed board by a viewport-local logical pixel delta.
    pub fn pan(&mut self, delta: Point) -> bool {
        if !delta.x.is_finite()
            || !delta.y.is_finite()
            || !self.pixels_per_mm.is_finite()
            || self.pixels_per_mm <= 0.0
        {
            return false;
        }
        let sign = if self.flipped { -1.0 } else { 1.0 };
        let center = Point::new(
            self.center.x - sign * delta.x / self.pixels_per_mm,
            self.center.y + delta.y / self.pixels_per_mm,
        );
        if !center.x.is_finite() || !center.y.is_finite() {
            return false;
        }
        self.center = center;
        true
    }
}

/// Per-document camera state, independent of GPUI and display DPI.
#[derive(Debug, Default)]
pub struct ViewportNavigation {
    camera: Camera,
    size: Point,
    initialized: bool,
    fit_scale: f64,
}

impl ViewportNavigation {
    /// Restore a validated camera before layout without resize auto-fit replacing it.
    pub fn restore_camera(&mut self, camera: Camera) -> bool {
        if !camera.is_renderable() {
            return false;
        }
        self.camera = camera;
        self.initialized = true;
        true
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }
    /// A restored/manual camera is meaningful even before its first layout.
    pub fn has_view(&self) -> bool {
        self.initialized || (self.size.x > 0.0 && self.size.y > 0.0)
    }
    pub fn size(&self) -> Point {
        self.size
    }

    /// Zoom relative to the latest explicit fit; view changes do not reset it.
    pub fn zoom_percent(&self) -> f64 {
        self.camera.pixels_per_mm
            / if self.fit_scale > 0.0 {
                self.fit_scale
            } else {
                10.0
            }
            * 100.0
    }

    /// Fit only on first layout. Resizing preserves the current camera, as on Web.
    pub fn resize(&mut self, bounds: Bounds, width: f64, height: f64) -> bool {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return false;
        }
        if !self.initialized || self.fit_scale == 0.0 {
            let mut fitted = self.camera;
            if !fitted.fit_board(bounds, width, height) {
                return false;
            }
            if !self.initialized {
                self.camera = fitted;
            }
            self.fit_scale = fitted.pixels_per_mm;
            self.initialized = true;
        }
        self.size = Point::new(width, height);
        true
    }

    pub fn fit(&mut self, bounds: Bounds) -> bool {
        if !self.camera.fit_board(bounds, self.size.x, self.size.y) {
            return false;
        }
        self.fit_scale = self.camera.pixels_per_mm;
        self.initialized = true;
        true
    }

    /// Locate a query result with the Web focus rule: 3 mm minimum span,
    /// 72% occupancy and at most 288 logical pixels/mm. Keep the fit baseline.
    pub fn locate(&mut self, bounds: Bounds) -> bool {
        if !bounds.is_valid() || self.size.x <= 0.0 || self.size.y <= 0.0 {
            return false;
        }
        let next = Camera {
            center: bounds.center(),
            pixels_per_mm: 400_f64
                .min(self.size.x.max(1.0) / (bounds.max.x - bounds.min.x).max(3.0))
                .min(self.size.y.max(1.0) / (bounds.max.y - bounds.min.y).max(3.0))
                * 0.72,
            flipped: self.camera.flipped,
        };
        if !next.is_renderable() {
            return false;
        }
        self.camera = next;
        self.initialized = true;
        true
    }

    pub fn zoom_at(&mut self, anchor: Point, factor: f64) -> bool {
        if !self
            .camera
            .zoom_at(anchor, self.size.x, self.size.y, factor)
        {
            return false;
        }
        self.initialized = true;
        true
    }

    pub fn pan(&mut self, delta: Point) -> bool {
        if self.size.x <= 0.0 || self.size.y <= 0.0 || !self.camera.pan(delta) {
            return false;
        }
        self.initialized = true;
        true
    }

    pub fn flip(&mut self) {
        self.camera.flipped = !self.camera.flipped;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Object,
    Track,
    Net,
    Component,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mil_scale_ticks_preserve_source_geometry_projection() {
        let camera = Camera::default();
        let scale = camera
            .scale_bar_in_unit(100.0, crate::units::LengthUnit::Mils)
            .unwrap();
        assert!(
            (crate::units::LengthUnit::Mils.from_millimeters(scale.millimeters) - 200.0).abs()
                < 1e-10
        );
        assert!((scale.pixels - scale.millimeters * camera.pixels_per_mm).abs() < 1e-10);
    }

    #[test]
    fn scale_bar_tracks_zoom_and_ignores_pan_and_flip() {
        let mut camera = Camera::default();
        assert_eq!(
            camera.scale_bar(100.0),
            Some(ScaleBar {
                millimeters: 10.0,
                pixels: 100.0
            })
        );
        camera.pixels_per_mm = 37.0;
        camera.center = Point::new(300.0, -900.0);
        camera.flipped = true;
        assert_eq!(
            camera.scale_bar(100.0),
            Some(ScaleBar {
                millimeters: 2.0,
                pixels: 74.0
            })
        );
    }

    #[test]
    fn scale_bar_fits_across_supported_zoom_range() {
        for exponent in -3..=7 {
            let camera = Camera {
                pixels_per_mm: 3.7 * 10.0_f64.powi(exponent),
                ..Camera::default()
            };
            let scale = camera.scale_bar(100.0).unwrap();
            assert!(scale.pixels <= 100.0 && scale.pixels >= 20.0);
            let source_pixels = camera
                .board_to_view(Point::new(scale.millimeters, 0.0), 200.0, 200.0)
                .x
                - 100.0;
            assert!((source_pixels - scale.pixels).abs() < 1e-10);
        }
    }

    #[test]
    fn scale_bar_rejects_invalid_viewport_widths() {
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(Camera::default().scale_bar(width).is_none());
        }
    }

    #[test]
    fn restored_camera_survives_first_layout_and_invalid_restore_is_atomic() {
        let mut navigation = ViewportNavigation::default();
        assert!(!navigation.has_view());
        let camera = Camera {
            center: Point::new(23.0, -7.0),
            pixels_per_mm: 37.6,
            flipped: true,
        };
        assert!(navigation.restore_camera(camera));
        assert!(navigation.has_view());
        let bounds = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(100.0, 80.0),
        };
        assert!(navigation.resize(bounds, 900.0, 700.0));
        assert_eq!(navigation.camera().center, camera.center);
        assert_eq!(navigation.camera().pixels_per_mm, camera.pixels_per_mm);
        assert!(navigation.camera().flipped);
        assert!(!navigation.restore_camera(Camera {
            pixels_per_mm: f64::NAN,
            ..camera
        }));
        assert_eq!(navigation.camera().center, camera.center);
        assert_eq!(navigation.camera().pixels_per_mm, camera.pixels_per_mm);
        assert!(navigation.fit(bounds));
        assert_ne!(navigation.camera().center, camera.center);
    }

    #[test]
    fn locating_a_point_retains_side_and_view_on_resize_then_fit_restores_board() {
        let board = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(100.0, 80.0),
        };
        let target = Bounds {
            min: Point::new(12.0, 18.0),
            max: Point::new(12.0, 18.0),
        };
        let mut nav = ViewportNavigation::default();
        assert!(nav.resize(board, 800.0, 600.0));
        nav.flip();
        assert!(nav.locate(target));
        let camera = nav.camera();
        assert_eq!(camera.center, target.center());
        assert!(camera.flipped);
        assert!((camera.pixels_per_mm - 144.0).abs() < 1e-12);
        assert!(nav.resize(board, 400.0, 300.0));
        assert_eq!(nav.camera().center, camera.center);
        assert_eq!(nav.camera().pixels_per_mm, camera.pixels_per_mm);
        assert!(!nav.locate(Bounds {
            min: Point::new(f64::NAN, 0.0),
            ..target
        }));
        assert_eq!(nav.camera().center, camera.center);
        assert!(nav.fit(board));
        assert_eq!(nav.camera().center, board.center());
    }

    #[test]
    fn flipped_camera_round_trips_board_coordinates() {
        let camera = Camera {
            center: Point::new(80.0, 20.0),
            pixels_per_mm: 125.0,
            flipped: true,
        };
        let board = Point::new(82.45, 31.2);
        let restored =
            camera.view_to_board(camera.board_to_view(board, 900.0, 700.0), 900.0, 700.0);
        assert!(restored.distance(board) < 1e-12);
    }

    #[test]
    fn zoom_preserves_board_point_under_pointer() {
        let mut camera = Camera::default();
        let anchor = Point::new(40.0, 320.0);
        let before = camera.view_to_board(anchor, 900.0, 700.0);
        camera.zoom_at(anchor, 900.0, 700.0, 1.5);
        assert!(before.distance(camera.view_to_board(anchor, 900.0, 700.0)) < 1e-12);
    }

    #[test]
    fn fit_rejects_viewport_smaller_than_padding() {
        let mut camera = Camera::default();
        let bounds = Bounds {
            min: Point::default(),
            max: Point::new(10.0, 10.0),
        };
        assert!(!camera.fit(bounds, 20.0, 20.0, 12.0));
    }

    #[test]
    fn dragging_moves_board_by_the_same_pixels_on_either_side() {
        for flipped in [false, true] {
            let mut camera = Camera {
                flipped,
                ..Camera::default()
            };
            let board = Point::new(2.0, 3.0);
            let before = camera.board_to_view(board, 800.0, 600.0);
            assert!(camera.pan(Point::new(25.0, -18.0)));
            let after = camera.board_to_view(board, 800.0, 600.0);
            assert!(after.distance(Point::new(before.x + 25.0, before.y - 18.0)) < 1e-12);
        }
    }

    #[test]
    fn navigation_preserves_view_on_resize_and_only_explicit_fit_changes_scale() {
        let bounds = Bounds {
            min: Point::default(),
            max: Point::new(20.0, 10.0),
        };
        let mut nav = ViewportNavigation::default();
        assert!(nav.resize(bounds, 800.0, 600.0));
        assert!(nav.zoom_at(Point::new(50.0, 200.0), 2.0));
        let camera = nav.camera();
        assert!(nav.resize(bounds, 400.0, 300.0));
        assert_eq!(nav.camera().center, camera.center);
        assert_eq!(nav.camera().pixels_per_mm, camera.pixels_per_mm);
        nav.flip();
        assert!(nav.fit(bounds));
        assert_eq!(nav.camera().center, bounds.center());
        assert!(nav.camera().flipped);
        assert!(nav.resize(bounds, 800.0, 600.0));
        assert!((nav.camera().pixels_per_mm - 17.2).abs() < 1e-12);
        assert!(nav.fit(bounds));
        assert!((nav.camera().pixels_per_mm - 34.4).abs() < 1e-12);
    }

    #[test]
    fn invalid_navigation_does_not_poison_the_camera() {
        let mut camera = Camera::default();
        let before = camera;
        assert!(!camera.pan(Point::new(f64::NAN, 1.0)));
        assert!(!camera.zoom_at(Point::new(f64::INFINITY, 0.0), 100.0, 100.0, 2.0));
        assert!(!camera.zoom_at(Point::default(), 0.0, 100.0, 2.0));
        assert_eq!(camera.center, before.center);
        assert_eq!(camera.pixels_per_mm, before.pixels_per_mm);
    }
}
