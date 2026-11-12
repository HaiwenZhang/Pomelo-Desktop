//! Canvas overlay for review tools: measurement paths, clearance lines, snap markers,
//! drag rectangles and their labels. Geometry is stored in board space and projected
//! with the current camera, so the overlay follows panning and zooming exactly.
use super::{
    review::{ReviewTool, format_length},
    *,
};
use pomelo_core::review::measure::{MeasureKind, SnapKind};

struct OverlayLine {
    points: Vec<Point<Pixels>>,
    dashed: bool,
}

/// Board point to canvas-local logical pixels.
fn project(
    camera: pomelo_core::interaction::Camera,
    size: pomelo_core::model::Point,
    point: pomelo_core::model::Point,
) -> Point<Pixels> {
    let view = camera.board_to_view(point, size.x, size.y);
    point_px(view.x as f32, view.y as f32)
}

fn point_px(x: f32, y: f32) -> Point<Pixels> {
    gpui_kit::point(px(x), px(y))
}

impl BoardViewport {
    /// Tool hint shown above the canvas while a review tool is active.
    pub(super) fn review_hint(&self, locale: pomelo_core::i18n::Locale) -> Option<String> {
        let review = &self.review;
        let key = match review.tool? {
            ReviewTool::Measure(kind) => {
                let points = review.active.as_ref().map_or(0, |m| m.points.len());
                match (kind, points) {
                    (_, 0) => Key::MeasureHintFirst,
                    (MeasureKind::Distance, _) => Key::MeasureHintSecond,
                    (MeasureKind::Polyline, _) => Key::MeasureHintNext,
                }
            }
            ReviewTool::Clearance => {
                if let Some(first) = review.clearance_first {
                    let (kind, id) =
                        selection_identity(pomelo_core::selection::SelectionTarget::Object(first));
                    let object = Message::new(Key::ReviewObjectLabel)
                        .arg("kind", text(locale, kind))
                        .arg("id", id)
                        .display(locale);
                    return Some(
                        Message::new(Key::ClearanceHintSecond)
                            .arg("object", object)
                            .display(locale),
                    );
                }
                Key::ClearanceHintFirst
            }
            ReviewTool::BoxSelect => Key::BoxSelectHint,
            ReviewTool::ZoomBox => Key::ZoomBoxHint,
            ReviewTool::PinPath if review.path_pending => Key::PinPathPending,
            ReviewTool::PinPath if review.path_first.is_some() => Key::PinPathHintSecond,
            ReviewTool::PinPath => Key::PinPathHintFirst,
        };
        Some(text(locale, key))
    }

    pub(super) fn render_review_overlay(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &App,
    ) -> Vec<AnyElement> {
        if self.view3d.active {
            return vec![];
        }
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        let review = &self.review;
        let unit = self.display.length_unit;
        if size.x <= 0.0 || size.y <= 0.0 || !camera.is_renderable() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut dots = Vec::new();
        let mut labels: Vec<(Point<Pixels>, String)> = Vec::new();
        let mut add_measurement =
            |measurement: &pomelo_core::review::measure::Measurement,
             lines: &mut Vec<OverlayLine>,
             dots: &mut Vec<Point<Pixels>>| {
                let points: Vec<_> = measurement
                    .points
                    .iter()
                    .map(|point| project(camera, size, point.position))
                    .collect();
                dots.extend(points.iter().copied());
                if let Some(&last) = points.last()
                    && points.len() >= 2
                {
                    let length = measurement.total_length();
                    let leg = measurement.legs().last();
                    let label = if let Some(leg) = leg {
                        Message::new(Key::MeasureLeg)
                            .arg("dx", format_length(leg.dx, unit, locale))
                            .arg("dy", format_length(leg.dy, unit, locale))
                            .arg("distance", format_length(leg.distance, unit, locale))
                            .arg("angle", review::format_angle(leg.angle_degrees))
                            .display(locale)
                    } else {
                        format_length(length, unit, locale)
                    };
                    let label = if measurement.kind == MeasureKind::Polyline {
                        let total = Message::new(Key::MeasureTotal)
                            .arg("length", format_length(length, unit, locale))
                            .display(locale);
                        format!("{label}\n{total}")
                    } else {
                        label
                    };
                    labels.push((last, label));
                    let first = points[points.len() - 2];
                    if (f32::from(last.x - first.x)).abs() > 12.0
                        && (f32::from(last.y - first.y)).abs() > 12.0
                    {
                        lines.push(OverlayLine {
                            points: vec![
                                first,
                                point_px(f32::from(last.x), f32::from(first.y)),
                                last,
                            ],
                            dashed: true,
                        });
                    }
                }
                lines.push(OverlayLine {
                    points,
                    dashed: false,
                });
            };
        for measurement in &review.measurements {
            add_measurement(measurement, &mut lines, &mut dots);
        }
        if let Some(active) = &review.active {
            let preview = review
                .preview
                .filter(|_| !active.points.is_empty())
                .map_or_else(|| active.clone(), |point| active.with_preview(point));
            add_measurement(&preview, &mut lines, &mut dots);
        }
        if let Some(record) = review.clearance {
            let a = project(camera, size, record.result.a);
            let b = project(camera, size, record.result.b);
            lines.push(OverlayLine {
                points: vec![a, b],
                dashed: true,
            });
            dots.extend([a, b]);
            labels.push((
                point_px(
                    (f32::from(a.x) + f32::from(b.x)) * 0.5,
                    (f32::from(a.y) + f32::from(b.y)) * 0.5,
                ),
                format_length(record.result.distance_mm, unit, locale),
            ));
        }
        let snap = review
            .preview
            .filter(|point| point.snap != SnapKind::Free)
            .map(|point| project(camera, size, point.position));
        let area_drag = review.drag.or_else(|| {
            review
                .zoom_anchor
                .filter(|_| review.tool == Some(ReviewTool::ZoomBox))
                .zip(self.pointer_position)
                .map(|(start, current)| review::AreaDrag { start, current })
        });
        let area = area_drag.zip(self.bounds).map(|(drag, bounds)| {
            let a = drag.start - bounds.origin;
            let b = drag.current - bounds.origin;
            let crossing = f32::from(b.x) < f32::from(a.x);
            (
                Bounds::from_corners(
                    point_px(
                        f32::from(a.x).min(f32::from(b.x)),
                        f32::from(a.y).min(f32::from(b.y)),
                    ),
                    point_px(
                        f32::from(a.x).max(f32::from(b.x)),
                        f32::from(a.y).max(f32::from(b.y)),
                    ),
                ),
                crossing && review.tool == Some(ReviewTool::BoxSelect),
            )
        });
        let color = crate::theme::canvas_measure();
        let mut elements = Vec::new();
        if !lines.is_empty() || !dots.is_empty() || snap.is_some() || area.is_some() {
            elements.push(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let origin = bounds.origin;
                        let at = |point: Point<Pixels>| origin + point;
                        for line in &lines {
                            if line.points.len() < 2 {
                                continue;
                            }
                            let mut builder = PathBuilder::stroke(px(2.0));
                            if line.dashed {
                                builder = builder.dash_array(&[px(6.0), px(4.0)]);
                            }
                            builder.move_to(at(line.points[0]));
                            for &point in &line.points[1..] {
                                builder.line_to(at(point));
                            }
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, color);
                            }
                        }
                        for &dot in &dots {
                            let center = at(dot);
                            window.paint_quad(quad(
                                Bounds::centered_at(center, gpui_kit::size(px(7.0), px(7.0))),
                                px(3.5),
                                color,
                                px(0.0),
                                color,
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some(snap) = snap {
                            window.paint_quad(outline(
                                Bounds::centered_at(at(snap), gpui_kit::size(px(14.0), px(14.0))),
                                color,
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some((area, crossing)) = area {
                            let area = Bounds {
                                origin: at(area.origin),
                                size: area.size,
                            };
                            let mut fill_color = color;
                            fill_color.a = 0.12;
                            window.paint_quad(quad(
                                area,
                                px(0.0),
                                fill_color,
                                px(1.0),
                                color,
                                if crossing {
                                    BorderStyle::Dashed
                                } else {
                                    BorderStyle::Solid
                                },
                            ));
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .into_any_element(),
            );
        }
        let theme = cx.theme();
        for (position, label) in labels {
            let (x, y) = (f32::from(position.x), f32::from(position.y));
            if !x.is_finite()
                || !y.is_finite()
                || x < 0.0
                || y < 0.0
                || x > size.x as f32
                || y > size.y as f32
            {
                continue;
            }

            elements.push(
                div()
                    .absolute()
                    .left(px(
                        (f32::from(position.x) + 8.0).clamp(8.0, (size.x as f32 - 370.0).max(8.0))
                    ))
                    .top(px(
                        (f32::from(position.y) - 26.0).clamp(8.0, (size.y as f32 - 32.0).max(8.0))
                    ))
                    .max_w(px((size.x as f32 - 16.0).max(1.0)))
                    .px_1()
                    .rounded_sm()
                    .bg(crate::theme::canvas_toolbar_surface())
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(color)
                    .child(label)
                    .into_any_element(),
            );
        }
        if let Some(hint) = self.review_hint(locale) {
            elements.push(
                div()
                    .absolute()
                    .top_24()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .max_w(relative(0.8))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(crate::theme::canvas_toolbar_surface())
                            .text_xs()
                            .text_color(crate::theme::canvas_colors().1)
                            .child(hint),
                    )
                    .into_any_element(),
            );
        }
        elements
    }
}
