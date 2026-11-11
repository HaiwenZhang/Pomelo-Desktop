//! Two-point and multi-segment measurement in 3D. Points snap to pad and via
//! centers, trace ends or the surface of the layer under the pointer, and are
//! stored in model millimetres so the Z stretch never changes a reading.
use super::*;
use pomelo_core::selection::MemberLayers;
use pomelo_core::{i18n::Locale, review::measure::MeasureKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::viewport) enum Snap3d {
    PadCenter,
    ViaCenter,
    TraceEnd,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::viewport) struct Point3d {
    pub at: [f64; 3],
    pub snap: Snap3d,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::viewport) struct Measure3d {
    pub kind: MeasureKind,
    pub points: Vec<Point3d>,
}

impl Measure3d {
    pub fn length(&self) -> f64 {
        self.points
            .windows(2)
            .map(|w| {
                let d: [f64; 3] = std::array::from_fn(|i| w[1].at[i] - w[0].at[i]);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .sum()
    }
    fn delta(&self) -> Option<[f64; 3]> {
        let (a, b) = (self.points.first()?, self.points.last()?);
        Some(std::array::from_fn(|i| b.at[i] - a.at[i]))
    }
}

impl BoardViewport {
    /// Model height of the top of a layer in the current 3D stack.
    fn layer_top_3d(&self, layer: pomelo_core::model::LayerId) -> Option<f64> {
        let scene = self.view3d.scene.as_ref()?;
        let index = scene.layers.iter().position(|l| *l == layer)?;
        let p = scene.preview;
        let pitch = if scene.layers.len() > 1 {
            (p.board_mm - p.copper_mm) / (scene.layers.len() - 1) as f64
        } else {
            0.0
        };
        Some(p.board_mm * 0.5 - pitch * index as f64)
    }

    /// 3D point under the pointer with snapping, in model coordinates.
    fn measure_point_3d(&mut self, position: Point<Pixels>) -> Option<Point3d> {
        let scene = self.view3d.scene.clone()?;
        let bounds = self.bounds?;
        let z_map = self.z_map_3d()?;
        let local = position - bounds.origin;
        let ray = self.view3d.camera.pick_ray(
            [f64::from(local.x), f64::from(local.y)],
            self.view3d.size,
            z_map.bounds(scene.bounds),
        )?;
        let hit = self.pick_3d(position);
        let layer = hit.and_then(|object| {
            scene
                .sources
                .iter()
                .find(|source| source.object == object)
                .and_then(|source| match source.layers {
                    MemberLayers::Single(layer) | MemberLayers::Span(layer, _) => Some(layer),
                    MemberLayers::None => None,
                })
        });
        let z = layer
            .and_then(|layer| self.layer_top_3d(layer))
            .unwrap_or(scene.preview.board_mm * 0.5);
        // Intersect the display ray with that layer's display height.
        let display_z = z_map.apply(z);
        if ray.direction[2].abs() < f64::EPSILON {
            return None;
        }
        let t = (display_z - ray.origin[2]) / ray.direction[2];
        let mut at = [
            ray.origin[0] + ray.direction[0] * t,
            ray.origin[1] + ray.direction[1] * t,
            z,
        ];
        let mut snap = Snap3d::Surface;
        let reach = self.view3d.camera.view_height() / self.view3d.size[1].max(1.0) * 10.0;
        use pomelo_core::selection::SelectedObject;
        match hit {
            Some(SelectedObject::Pin(id)) => {
                if let Some(pin) = self.scene.pins.iter().find(|p| p.id == id) {
                    at[0] = pin.at.x;
                    at[1] = pin.at.y;
                    snap = Snap3d::PadCenter;
                }
            }
            Some(SelectedObject::Via(id)) => {
                if let Some(via) = self.scene.vias.iter().find(|v| v.id == id) {
                    at[0] = via.at.x;
                    at[1] = via.at.y;
                    snap = Snap3d::ViaCenter;
                }
            }
            Some(SelectedObject::Segment(id)) => {
                if let Some(segment) = self.scene.segments.iter().find(|s| s.id == id) {
                    let point = pomelo_core::model::Point::new(at[0], at[1]);
                    let end = [segment.a, segment.b]
                        .into_iter()
                        .min_by(|a, b| a.distance(point).total_cmp(&b.distance(point)));
                    if let Some(end) = end.filter(|end| end.distance(point) <= reach) {
                        at[0] = end.x;
                        at[1] = end.y;
                        snap = Snap3d::TraceEnd;
                    }
                }
            }
            _ => {}
        }
        at.iter()
            .all(|v| v.is_finite())
            .then_some(Point3d { at, snap })
    }

    /// Place a measurement point; returns false when no 3D tool is active.
    pub(in crate::viewport) fn measure_click_3d(
        &mut self,
        position: Point<Pixels>,
        double: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(review::ReviewTool::Measure(kind)) = self.review.tool else {
            return false;
        };
        let Some(point) = self.measure_point_3d(position) else {
            return true;
        };
        let active = self.view3d.measuring.get_or_insert_with(|| Measure3d {
            kind,
            points: Vec::new(),
        });
        active.points.push(point);
        if (kind == MeasureKind::Distance && active.points.len() == 2)
            || (double && active.points.len() >= 2)
        {
            self.finish_measure_3d();
        }
        cx.notify();
        true
    }

    pub(in crate::viewport) fn finish_measure_3d(&mut self) {
        if let Some(active) = self.view3d.measuring.take()
            && active.points.len() >= 2
        {
            self.view3d.measurements.push(active);
        }
    }

    pub(in crate::viewport) fn cancel_measure_3d(&mut self) -> bool {
        self.view3d.measuring.take().is_some()
    }

    /// Lines, points and readouts of 3D measurements, projected with the live camera.
    pub(in crate::viewport) fn render_measure_overlay_3d(
        &self,
        locale: Locale,
        cx: &App,
    ) -> Option<AnyElement> {
        if self.view3d.measurements.is_empty() && self.view3d.measuring.is_none() {
            return None;
        }
        let scene = self.view3d.scene.as_ref()?;
        let z_map = self.z_map_3d()?;
        let size = self.view3d.size;
        let matrix = self.view3d.camera.clip_matrix(
            size,
            z_map.bounds(scene.bounds),
            [scene.origin.x, scene.origin.y, 0.0],
        )?;
        let origin = scene.origin;
        let project = move |p: [f64; 3]| {
            let v = [
                (p[0] - origin.x) as f32,
                (p[1] - origin.y) as f32,
                z_map.apply(p[2]) as f32,
                1.0,
            ];
            let clip: [f32; 4] =
                std::array::from_fn(|row| (0..4).map(|col| matrix[col][row] * v[col]).sum());
            let w = if clip[3].abs() > f32::EPSILON {
                clip[3]
            } else {
                1.0
            };
            (
                (clip[0] / w * 0.5 + 0.5) * size[0] as f32,
                (0.5 - clip[1] / w * 0.5) * size[1] as f32,
            )
        };
        let unit = self.display.length_unit;
        let length = |mm: f64| super::super::review::format_length(mm, unit, locale);
        let mut paths: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut labels = Vec::new();
        for measurement in self
            .view3d
            .measurements
            .iter()
            .chain(&self.view3d.measuring)
        {
            let points: Vec<_> = measurement.points.iter().map(|p| project(p.at)).collect();
            if let (Some(&last), Some(delta)) = (points.last(), measurement.delta())
                && points.len() >= 2
            {
                let assumed = delta[2].abs() > 1e-9;
                let label = Message::new(if assumed {
                    Key::Measure3dLabelAssumed
                } else {
                    Key::Measure3dLabel
                })
                .arg("distance", length(measurement.length()))
                .arg("dx", length(delta[0]))
                .arg("dy", length(delta[1]))
                .arg("dz", length(delta[2]))
                .display(locale);
                labels.push((last, label));
            }
            paths.push(points);
        }
        let color = crate::theme::canvas_measure();
        let theme = cx.theme();
        let label_elements = labels.into_iter().map(|((x, y), label)| {
            div()
                .absolute()
                .left(px(x + 8.0))
                .top(px(y - 10.0))
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(theme.popover)
                .border_1()
                .border_color(color)
                .text_xs()
                .whitespace_nowrap()
                .child(label)
        });
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            for points in &paths {
                                for (x, y) in points {
                                    let mut dot = gpui::PathBuilder::fill();
                                    let c = bounds.origin + point(px(*x), px(*y));
                                    dot.move_to(c + point(px(-3.0), px(0.0)));
                                    dot.line_to(c + point(px(0.0), px(-3.0)));
                                    dot.line_to(c + point(px(3.0), px(0.0)));
                                    dot.line_to(c + point(px(0.0), px(3.0)));
                                    dot.close();
                                    if let Ok(path) = dot.build() {
                                        window.paint_path(path, color);
                                    }
                                }
                                if points.len() < 2 {
                                    continue;
                                }
                                let mut line = gpui::PathBuilder::stroke(px(1.5));
                                for (i, (x, y)) in points.iter().enumerate() {
                                    let p = bounds.origin + point(px(*x), px(*y));
                                    if i == 0 {
                                        line.move_to(p);
                                    } else {
                                        line.line_to(p);
                                    }
                                }
                                if let Ok(path) = line.build() {
                                    window.paint_path(path, color);
                                }
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .children(label_elements)
                .into_any_element(),
        )
    }
}
