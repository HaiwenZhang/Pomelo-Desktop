//! Pad-space transforms and bounds shared by drawing and picking.

use crate::model::{BackdrillDefinition, Bounds, DrillShape, LayerId, Pad, PadKind, Point};

#[derive(Debug, Clone, Copy, Default)]
pub struct PadPlacement {
    pub at: Point,
    pub angle: f64,
    pub mirrored: bool,
}

impl Point {
    pub fn rotate(self, angle: f64) -> Self {
        let (sine, cosine) = angle.sin_cos();
        Self::new(
            self.x * cosine - self.y * sine,
            self.x * sine + self.y * cosine,
        )
    }
}

impl Pad {
    /// Custom fill contours in source pad space; preserves mirror/rotation/offset order.
    /// Curved contours are the stored fill approximation, not exact analytic path picking.
    pub fn custom_covers_fill(
        &self,
        point: Point,
        owner: PadPlacement,
        cancel: &crate::task::CancellationToken,
    ) -> Result<Option<bool>, crate::copper::MeshError> {
        if cancel.is_cancelled() {
            return Err(crate::copper::MeshError::Cancelled);
        }
        let Some(custom) = &self.custom else {
            return Ok(None);
        };
        if ![
            point.x,
            point.y,
            owner.at.x,
            owner.at.y,
            owner.angle,
            self.offset.x,
            self.offset.y,
        ]
        .into_iter()
        .all(f64::is_finite)
        {
            return Err(crate::copper::MeshError::Invalid {
                ring: 0,
                field: "PLACEMENT",
            });
        }
        let mut local = Point::new(
            point.x - owner.at.x - self.offset.x,
            point.y - owner.at.y - self.offset.y,
        )
        .rotate(-owner.angle);
        if owner.mirrored {
            local.y = -local.y;
        }
        crate::copper::contours_cover(custom.contours.iter().map(Vec::as_slice), local, cancel)
            .map(Some)
    }

    /// Corner parameter shared with the Web analytic pad contract.
    pub fn corner_radius(&self) -> f64 {
        let minimum = self.width.min(self.height);
        let corner = match self.kind.0 {
            11 | 12 => minimum * 0.5,
            27 | 28 => {
                if self.corner > 0.0 {
                    self.corner
                } else {
                    minimum * 0.25
                }
            }
            3 => minimum * (1.0 - std::f64::consts::FRAC_1_SQRT_2),
            _ => 0.0,
        };
        corner.min(minimum * 0.5)
    }

    /// Signed board-space distance for analytic families; negative means inside copper.
    /// Custom contours require their independent mesh/path implementation.
    pub fn analytic_distance(&self, point: Point, owner: PadPlacement) -> Option<f64> {
        if self.custom.is_some()
            || !self.kind.is_analytic()
            || ![
                point.x,
                point.y,
                owner.at.x,
                owner.at.y,
                owner.angle,
                self.offset.x,
                self.offset.y,
                self.width,
                self.height,
                self.corner,
            ]
            .into_iter()
            .all(f64::is_finite)
            || self.width < 0.0
            || self.height < 0.0
        {
            return None;
        }
        let local = Point::new(
            point.x - owner.at.x - self.offset.x,
            point.y - owner.at.y - self.offset.y,
        )
        .rotate(-owner.angle);
        if self.kind == PadKind::CIRCLE {
            return Some(local.x.hypot(local.y) - self.width * 0.5);
        }
        if self.kind == PadKind::DONUT {
            let inner = self.inner_diameter?;
            if !inner.is_finite() || inner < 0.0 || inner > self.width {
                return None;
            }
            let radius = local.x.hypot(local.y);
            return Some((radius - self.width * 0.5).max(inner * 0.5 - radius));
        }
        let corner = self.corner_radius();
        let x = local.x.abs() - self.width * 0.5;
        let y = local.y.abs() - self.height * 0.5;
        Some(if matches!(self.kind.0, 3 | 28) {
            x.max(y)
                .max((x + y + corner) * std::f64::consts::FRAC_1_SQRT_2)
        } else {
            (x + corner).max(0.0).hypot((y + corner).max(0.0))
                + (x + corner).max(y + corner).min(0.0)
                - corner
        })
    }

    pub fn circle(layer: LayerId, diameter: f64) -> Self {
        Self {
            layer,
            width: diameter,
            height: diameter,
            offset: Point::default(),
            kind: PadKind::CIRCLE,
            corner: 0.0,
            inner_diameter: None,
            custom: None,
            backdrill: false,
            backdrill_base: false,
        }
    }

    pub fn supported(&self) -> bool {
        self.kind.is_analytic()
            || self
                .custom
                .as_ref()
                .is_some_and(|geometry| !geometry.contours.is_empty())
    }

    pub fn to_world(&self, local: Point, owner: PadPlacement) -> Point {
        // Custom contours mirror their local Y; placement offsets are already in board space.
        let local = Point::new(
            local.x,
            if owner.mirrored && self.custom.is_some() {
                -local.y
            } else {
                local.y
            },
        );
        let rotated = local.rotate(owner.angle);
        Point::new(
            owner.at.x + self.offset.x + rotated.x,
            owner.at.y + self.offset.y + rotated.y,
        )
    }

    pub fn bounds(&self, owner: PadPlacement) -> Option<Bounds> {
        if ![
            owner.at.x,
            owner.at.y,
            owner.angle,
            self.offset.x,
            self.offset.y,
            self.width,
            self.height,
        ]
        .into_iter()
        .all(f64::is_finite)
        {
            return None;
        }
        if let Some(custom) = &self.custom {
            return Bounds::from_points(
                custom
                    .contours
                    .iter()
                    .flatten()
                    .copied()
                    .map(|point| self.to_world(point, owner)),
            );
        }
        if matches!(self.kind, PadKind::CIRCLE | PadKind::DONUT) {
            let center = Point::new(owner.at.x + self.offset.x, owner.at.y + self.offset.y);
            let radius = self.width * 0.5;
            return Bounds::from_points([
                Point::new(center.x - radius, center.y - radius),
                Point::new(center.x + radius, center.y + radius),
            ]);
        }
        let half_width = self.width * 0.5;
        let half_height = self.height * 0.5;
        Bounds::from_points(
            [
                Point::new(-half_width, -half_height),
                Point::new(half_width, -half_height),
                Point::new(half_width, half_height),
                Point::new(-half_width, half_height),
            ]
            .map(|point| self.to_world(point, owner)),
        )
    }
}

impl DrillShape {
    /// Drill display is independent of the copper pad's donut opening.
    pub fn pad(self) -> Option<Pad> {
        if self.width <= 0.0
            || self.height <= 0.0
            || !self.width.is_finite()
            || !self.height.is_finite()
        {
            return None;
        }
        let mut pad = Pad::circle(LayerId::UNASSIGNED, self.width);
        pad.height = self.height;
        if self.width != self.height {
            pad.kind = PadKind(11);
        }
        Some(pad)
    }
}

impl BackdrillDefinition {
    pub fn contains_layer(&self, layer: LayerId) -> bool {
        self.spans.iter().any(|span| {
            layer >= span.start_layer.min(span.stop_layer)
                && layer <= span.start_layer.max(span.stop_layer)
        })
    }
}

#[cfg(test)]
mod analytic_tests {
    use super::*;

    #[test]
    fn custom_fill_inverse_transform_preserves_holes_on_both_sides() {
        let mut pad = Pad::circle(LayerId(1), 4.0);
        pad.kind = PadKind::CUSTOM;
        pad.offset = Point::new(3.0, 4.0);
        pad.custom = Some(std::sync::Arc::new(crate::model::CustomPadGeometry {
            contours: vec![
                vec![
                    Point::new(0.0, 0.0),
                    Point::new(4.0, 0.0),
                    Point::new(4.0, 2.0),
                    Point::new(0.0, 2.0),
                ],
                vec![
                    Point::new(1.0, 0.5),
                    Point::new(2.0, 0.5),
                    Point::new(2.0, 1.5),
                    Point::new(1.0, 1.5),
                ],
            ],
            paths: vec![],
        }));
        let cancel = crate::task::CancellationToken::default();
        for (mirrored, x) in [(false, 12.0), (true, 14.0)] {
            let owner = PadPlacement {
                at: Point::new(10.0, 20.0),
                angle: std::f64::consts::FRAC_PI_2,
                mirrored,
            };
            assert_eq!(
                pad.custom_covers_fill(Point::new(x, 27.0), owner, &cancel)
                    .unwrap(),
                Some(true)
            );
            assert_eq!(
                pad.custom_covers_fill(Point::new(x, 25.5), owner, &cancel)
                    .unwrap(),
                Some(false)
            );
            assert_eq!(
                pad.custom_covers_fill(Point::new(20.0, 20.0), owner, &cancel)
                    .unwrap(),
                Some(false)
            );
        }
        cancel.cancel();
        assert!(matches!(
            pad.custom_covers_fill(Point::default(), PadPlacement::default(), &cancel),
            Err(crate::copper::MeshError::Cancelled)
        ));
    }

    #[test]
    fn donut_copper_opening_is_independent_of_drill_display() {
        let mut pad = Pad::circle(LayerId(1), 4.0);
        pad.kind = PadKind::DONUT;
        pad.inner_diameter = Some(2.0);
        let owner = PadPlacement::default();
        assert_eq!(
            pad.analytic_distance(Point::new(0.0, 0.0), owner),
            Some(1.0)
        );
        assert_eq!(
            pad.analytic_distance(Point::new(1.5, 0.0), owner),
            Some(-0.5)
        );
        pad.inner_diameter = Some(5.0);
        assert_eq!(pad.analytic_distance(Point::default(), owner), None);
    }

    #[test]
    fn rotated_oblong_uses_board_space_offset_without_rotating_it_again() {
        let mut pad = Pad::circle(LayerId(1), 4.0);
        pad.height = 2.0;
        pad.kind = PadKind(11);
        pad.offset = Point::new(3.0, 0.0);
        let owner = PadPlacement {
            at: Point::new(10.0, 20.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: true,
        };
        assert_eq!(pad.corner_radius(), 1.0);
        assert!(
            (pad.analytic_distance(Point::new(13.0, 22.0), owner)
                .unwrap())
            .abs()
                < 1e-12
        );
        assert!(
            pad.analytic_distance(Point::new(15.0, 20.0), owner)
                .unwrap()
                > 0.9
        );
    }

    #[test]
    fn rounded_and_chamfered_corners_keep_distinct_coverage() {
        let mut pad = Pad::circle(LayerId(1), 4.0);
        pad.kind = PadKind(27);
        assert_eq!(pad.corner_radius(), 1.0);
        let point = Point::new(1.6, 1.6);
        assert!(
            pad.analytic_distance(point, PadPlacement::default())
                .unwrap()
                < 0.0
        );
        pad.kind = PadKind(28);
        assert!(
            pad.analytic_distance(point, PadPlacement::default())
                .unwrap()
                > 0.0
        );
        pad.corner = 100.0;
        assert_eq!(pad.corner_radius(), 2.0);
    }
}
