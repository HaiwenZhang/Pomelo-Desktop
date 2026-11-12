//! Orientation cube in the canvas corner. It is drawn from the live camera, a click
//! on a face looks from that side, and dragging it orbits like the canvas does.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::i18n::Locale;
use std::time::{Duration, Instant};

/// Logical pixels of the square the cube is drawn in.
const CUBE_SIZE: f32 = 84.0;
/// Pixels per cube unit; the projected unit cube stays inside the square.
const CUBE_SCALE: f64 = 44.0;
const TRANSITION: Duration = Duration::from_millis(220);

/// Faces as (outward normal, view that looks at it, short label).
const FACES: [([f64; 3], CameraView3d, Key); 6] = [
    ([0.0, 0.0, 1.0], CameraView3d::Top, Key::CubeFaceTop),
    ([0.0, 0.0, -1.0], CameraView3d::Bottom, Key::CubeFaceBottom),
    ([0.0, -1.0, 0.0], CameraView3d::Front, Key::CubeFaceFront),
    ([0.0, 1.0, 0.0], CameraView3d::Back, Key::CubeFaceBack),
    ([-1.0, 0.0, 0.0], CameraView3d::Left, Key::CubeFaceLeft),
    ([1.0, 0.0, 0.0], CameraView3d::Right, Key::CubeFaceRight),
];

/// Standard views in menu order with their names.
pub(in crate::viewport) const STANDARD_VIEWS: [(CameraView3d, Key); 9] = [
    (CameraView3d::Top, Key::View3dTop),
    (CameraView3d::Bottom, Key::View3dBottom),
    (CameraView3d::Front, Key::View3dFront),
    (CameraView3d::Back, Key::View3dBack),
    (CameraView3d::Left, Key::View3dLeft),
    (CameraView3d::Right, Key::View3dRight),
    (CameraView3d::Isometric, Key::View3dIsometric),
    (CameraView3d::Dimetric, Key::View3dDimetric),
    (CameraView3d::Trimetric, Key::View3dTrimetric),
];

#[derive(Clone, Copy)]
pub(in crate::viewport) struct Transition {
    from: [f64; 4],
    to: [f64; 4],
    started: Instant,
}

/// One visible face projected into cube-square pixels, farthest first.
struct ProjectedFace {
    corners: [(f32, f32); 4],
    center: (f32, f32),
    facing: f64,
    view: CameraView3d,
    label: Key,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn project_faces(camera: Camera3d) -> Vec<ProjectedFace> {
    let [right, up, back] = camera.axes();
    let half = f64::from(CUBE_SIZE) * 0.5;
    let project = |p: [f64; 3]| {
        (
            (half + dot(p, right) * CUBE_SCALE * 0.5) as f32,
            (half - dot(p, up) * CUBE_SCALE * 0.5) as f32,
        )
    };
    let mut faces: Vec<_> = FACES
        .iter()
        .filter_map(|&(normal, view, label)| {
            let facing = dot(normal, back);
            if facing <= 0.02 {
                return None;
            }
            // Two in-face axes perpendicular to the normal.
            let axis = normal.iter().position(|v| v.abs() > 0.5)?;
            let mut tangents = (0..3).filter(|i| *i != axis).map(|i| {
                let mut v = [0.0; 3];
                v[i] = 1.0;
                v
            });
            let (a, b) = (tangents.next()?, tangents.next()?);
            let corner = |sa: f64, sb: f64| {
                project(std::array::from_fn(|i| normal[i] + a[i] * sa + b[i] * sb))
            };
            Some(ProjectedFace {
                corners: [
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ],
                center: project(normal),
                facing,
                view,
                label,
            })
        })
        .collect();
    faces.sort_by(|a, b| a.facing.total_cmp(&b.facing));
    faces
}

fn contains(corners: &[(f32, f32); 4], point: (f32, f32)) -> bool {
    let mut sign = 0.0_f32;
    for i in 0..4 {
        let (ax, ay) = corners[i];
        let (bx, by) = corners[(i + 1) % 4];
        let side = (bx - ax) * (point.1 - ay) - (by - ay) * (point.0 - ax);
        if side.abs() < f32::EPSILON {
            continue;
        }
        if sign != 0.0 && side.signum() != sign {
            return false;
        }
        sign = side.signum();
    }
    true
}

impl BoardViewport {
    /// Jump to a standard view, animated unless the user turned transitions off.
    pub(in crate::viewport) fn set_standard_view_3d(
        &mut self,
        view: CameraView3d,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.camera.is_view(view) && self.view3d.transition.is_none() {
            return;
        }
        self.record_3d_view();
        let to = view.orientation();
        if self.view3d.settings.animate_views {
            self.view3d.transition = Some(Transition {
                from: self.view3d.camera.orientation(),
                to,
                started: Instant::now(),
            });
        } else {
            self.view3d.camera.set_orientation(to);
        }
        cx.notify();
    }

    /// Advance a running view transition; returns true while more frames are needed.
    pub(in crate::viewport) fn advance_3d_transition(&mut self) -> bool {
        let Some(transition) = self.view3d.transition else {
            return false;
        };
        let t = (transition.started.elapsed().as_secs_f64() / TRANSITION.as_secs_f64()).min(1.0);
        // Ease out so the motion settles instead of stopping abruptly.
        let eased = 1.0 - (1.0 - t).powi(3);
        self.view3d
            .camera
            .set_orientation(pomelo_core::interaction::camera3d::slerp(
                transition.from,
                transition.to,
                eased,
            ));
        if t >= 1.0 {
            self.view3d.transition = None;
            return false;
        }
        true
    }

    fn cube_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.view3d.cube_bounds?;
        let local = position - bounds.origin;
        Some((local.x.as_f32(), local.y.as_f32()))
    }

    fn cube_click(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(point) = self.cube_point(position) else {
            return;
        };
        if let Some(face) = project_faces(self.view3d.camera)
            .into_iter()
            .rev()
            .find(|face| contains(&face.corners, point))
        {
            self.set_standard_view_3d(face.view, cx);
        }
    }

    pub(in crate::viewport) fn render_view_cube(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let faces = project_faces(self.view3d.camera);
        let theme = cx.theme();
        let fill = crate::theme::canvas_toolbar_surface();
        let border = crate::theme::canvas_colors().2;
        let highlight = theme.primary;
        let current = STANDARD_VIEWS
            .iter()
            .find(|(view, _)| self.view3d.camera.is_view(*view));
        let paint_faces: Vec<_> = faces
            .iter()
            .map(|face| (face.corners, current.is_some_and(|(v, _)| *v == face.view)))
            .collect();
        let bounds_view = cx.entity().downgrade();
        let labels: Vec<AnyElement> = faces
            .iter()
            .filter(|face| face.facing > 0.35)
            .map(|face| {
                let selected = current.is_some_and(|(v, _)| *v == face.view);
                div()
                    .absolute()
                    .left(px(face.center.0 - 12.0))
                    .top(px(face.center.1 - 8.0))
                    .w(px(24.0))
                    .text_center()
                    .text_xs()
                    .text_color(if selected {
                        highlight
                    } else {
                        crate::theme::canvas_colors().1
                    })
                    .child(text(locale, face.label))
                    .into_any_element()
            })
            .collect();
        let cube = div()
            .id("view-cube")
            .relative()
            .size(px(CUBE_SIZE))
            .cursor(CursorStyle::PointingHand)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = bounds_view.update(cx, |this, _| {
                            this.view3d.cube_bounds = Some(bounds);
                        });
                    },
                    move |bounds, _, window, _| {
                        for (corners, selected) in &paint_faces {
                            for (stroke, color) in [
                                (
                                    false,
                                    if *selected {
                                        highlight.opacity(0.35)
                                    } else {
                                        fill
                                    },
                                ),
                                (true, if *selected { highlight } else { border }),
                            ] {
                                let mut path = if stroke {
                                    gpui::PathBuilder::stroke(px(1.0))
                                } else {
                                    gpui::PathBuilder::fill()
                                };
                                for (i, (x, y)) in corners.iter().enumerate() {
                                    let p = bounds.origin + point(px(*x), px(*y));
                                    if i == 0 {
                                        path.move_to(p);
                                    } else {
                                        path.line_to(p);
                                    }
                                }
                                path.close();
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, color);
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(labels)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.view3d.cube_drag = Some((event.position, false));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some((last, _)) = this.view3d.cube_drag else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    this.view3d.cube_drag = None;
                    return;
                }
                let delta = event.position - last;
                if delta.x.as_f32().abs() + delta.y.as_f32().abs() < 2.0 {
                    return;
                }
                this.view3d.transition = None;
                this.view3d.camera.orbit(
                    -f64::from(delta.x.as_f32()) * 0.02,
                    -f64::from(delta.y.as_f32()) * 0.02,
                );
                this.view3d.cube_drag = Some((event.position, true));
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    if let Some((_, dragged)) = this.view3d.cube_drag.take()
                        && !dragged
                    {
                        this.cube_click(event.position, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .tooltip({
                let hint: SharedString = text(locale, Key::ViewCubeHint).into();
                move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
                }
            });
        let view = cx.entity().downgrade();
        let label: SharedString = current
            .map_or_else(
                || text(locale, Key::StandardViews),
                |(_, key)| text(locale, *key),
            )
            .into();
        let menu_label = text(locale, Key::StandardViews);
        let animate = self.view3d.settings.animate_views;
        let views = Button::new("standard-views")
            .custom(crate::theme::canvas_toolbar_button(cx))
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .disabled(self.view3d.scene.is_none())
            .dropdown_menu(move |mut menu, window, _| {
                menu = menu
                    .min_w(window.rem_size() * 10.0)
                    .label(menu_label.clone());
                for (view_kind, key) in STANDARD_VIEWS {
                    let view = view.clone();
                    menu = menu.item(PopupMenuItem::new(text(locale, key)).on_click(
                        move |_, _, cx| {
                            let _ = view
                                .update(cx, |this, cx| this.set_standard_view_3d(view_kind, cx));
                        },
                    ));
                }
                let view = view.clone();
                menu.separator().item(
                    PopupMenuItem::new(text(locale, Key::AnimateViews))
                        .checked(animate)
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.view3d.settings.animate_views = !animate;
                                cx.notify();
                            });
                        }),
                )
            });
        super::super::layout::canvas_bar(vec![
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(cube)
                .child(views)
                .into_any_element(),
        ])
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{CUBE_SIZE, Camera3d, CameraView3d, contains, project_faces};

    #[test]
    fn top_view_shows_only_the_top_face_and_clicks_inside_it() {
        let mut camera = Camera3d::default();
        camera.set_view(CameraView3d::Top);
        let faces = project_faces(camera);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].view, CameraView3d::Top);
        let center = (CUBE_SIZE * 0.5, CUBE_SIZE * 0.5);
        assert!(contains(&faces[0].corners, center));
        assert!(!contains(&faces[0].corners, (1.0, 1.0)));
    }

    #[test]
    fn isometric_view_shows_three_faces_nearest_last() {
        let mut camera = Camera3d::default();
        camera.set_view(CameraView3d::Isometric);
        let faces = project_faces(camera);
        assert_eq!(faces.len(), 3);
        assert!(faces.windows(2).all(|w| w[0].facing <= w[1].facing));
        for face in &faces {
            for (x, y) in face.corners {
                assert!((0.0..=CUBE_SIZE).contains(&x) && (0.0..=CUBE_SIZE).contains(&y));
            }
        }
    }
}
