//! Read-only board-space grid, thinned at small scales instead of shimmering.
use crate::{interaction::Camera, model::Point};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridOptions {
    pub visible: bool,
    pub spacing_mm: f64,
}
impl Default for GridOptions {
    fn default() -> Self {
        Self {
            visible: false,
            spacing_mm: 0.5,
        }
    }
}
impl GridOptions {
    pub fn valid(self) -> bool {
        self.spacing_mm.is_finite() && (1e-6..=1e6).contains(&self.spacing_mm)
    }
    /// Points keep their source origin and spacing through Pan, Zoom and board flip.
    /// Only integer powers of two are skipped; there are at most 16,384 visible dots.
    pub fn points(self, camera: Camera, size: Point) -> Vec<Point> {
        if !self.visible
            || !self.valid()
            || !camera.is_renderable()
            || !size.is_finite()
            || size.x <= 0.0
            || size.y <= 0.0
        {
            return Vec::new();
        }
        let mut step = self.spacing_mm;
        let half = Point::new(
            size.x * 0.5 / camera.pixels_per_mm,
            size.y * 0.5 / camera.pixels_per_mm,
        );
        for _ in 0..128 {
            let columns = (half.x * 2.0 / step).ceil() + 1.0;
            let rows = (half.y * 2.0 / step).ceil() + 1.0;
            if step * camera.pixels_per_mm >= 10.0 && columns * rows <= 16_384.0 {
                break;
            }
            step *= 2.0;
        }
        let x0 = ((camera.center.x - half.x) / step).ceil();
        let x1 = ((camera.center.x + half.x) / step).floor();
        let y0 = ((camera.center.y - half.y) / step).ceil();
        let y1 = ((camera.center.y + half.y) / step).floor();
        if ![x0, x1, y0, y1]
            .into_iter()
            .all(|n| n.is_finite() && n.abs() < (1u64 << 52) as f64)
        {
            return Vec::new();
        }
        let mut points = Vec::new();
        for y in y0 as i64..=y1 as i64 {
            for x in x0 as i64..=x1 as i64 {
                if points.len() >= 16_384 {
                    return points;
                }
                points.push(Point::new(x as f64 * step, y as f64 * step));
            }
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_preserves_world_origin_and_bounds_the_dot_budget_at_extreme_zoom() {
        let grid = GridOptions {
            visible: true,
            ..Default::default()
        };
        for scale in [0.01, 10.0, 1e7] {
            let camera = Camera {
                center: Point::new(-4.0, 8.0),
                pixels_per_mm: scale,
                flipped: true,
            };
            let size = Point::new(1920.0, 1080.0);
            let points = grid.points(camera, size);
            assert!(!points.is_empty());
            assert!(points.len() <= 16_384);
            for point in points {
                assert!((point.x / grid.spacing_mm).fract().abs() < 1e-8);
                assert!((point.y / grid.spacing_mm).fract().abs() < 1e-8);
                let screen = camera.board_to_view(point, size.x, size.y);
                assert!(
                    screen.x >= -1e-6
                        && screen.x <= size.x + 1e-6
                        && screen.y >= -1e-6
                        && screen.y <= size.y + 1e-6
                );
            }
        }
    }
}
