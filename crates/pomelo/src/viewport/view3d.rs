pub(super) mod controls;
pub(super) mod view_cube;

use super::*;
use pomelo_core::{
    interaction::camera3d::{Camera3d, CameraView3d},
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};
use pomelo_render::{
    backend::native::{SolidFrame, SolidRenderer, SolidTelemetry, display_bounds},
    scene::board3d::{PreparedBoard3d, PreviewStackup},
};
use std::sync::atomic::Ordering;

struct MaterialCache {
    display: Arc<pomelo_core::display::BoardDisplay>,
    selected: Option<SelectionTarget>,
    related: Arc<BTreeSet<SelectedObject>>,
    settings: pomelo_core::display3d::Display3d,
    colors: Arc<Vec<[f32; 4]>>,
}
pub(super) struct View3d {
    pub active: bool,
    pub camera: Camera3d,
    pub size: [f64; 2],
    pub settings: pomelo_core::display3d::Display3d,
    /// Cameras before and after the current one, for the view history buttons.
    pub history: Vec<Camera3d>,
    pub future: Vec<Camera3d>,
    scene: Option<Arc<PreparedBoard3d>>,
    pending: bool,
    error: Option<String>,
    task: Option<Task<()>>,
    cancel: CancellationToken,
    renderer: Option<Result<GpuPainterHandle<SolidFrame>, String>>,
    telemetry: Arc<SolidTelemetry>,
    pub initialized: bool,
    pan_drag: bool,
    materials: Option<MaterialCache>,
    /// A saved camera was applied; keep it instead of fitting the first scene.
    pub camera_restored: bool,
    /// Frame the first 3D scene around the 2D focus once it has been fitted.
    pub follow_2d: bool,
    pub transition: Option<view_cube::Transition>,
    pub cube_bounds: Option<Bounds<Pixels>>,
    /// Pointer position of a cube drag and whether it has moved enough to orbit.
    pub cube_drag: Option<(Point<Pixels>, bool)>,
}
impl Default for View3d {
    fn default() -> Self {
        let mut camera = Camera3d::default();
        camera.set_view(CameraView3d::Isometric);
        Self {
            active: false,
            camera,
            size: [0.0; 2],
            settings: Default::default(),
            history: Vec::new(),
            future: Vec::new(),
            scene: None,
            pending: false,
            error: None,
            task: None,
            cancel: CancellationToken::default(),
            renderer: None,
            telemetry: Arc::default(),
            initialized: false,
            pan_drag: false,
            materials: None,
            camera_restored: false,
            follow_2d: false,
            transition: None,
            cube_bounds: None,
            cube_drag: None,
        }
    }
}
impl Drop for View3d {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl BoardViewport {
    pub(super) fn toggle_3d(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view3d.active = !self.view3d.active;
        self.invalidate_hover();
        self.pointer_gesture = None;
        self.pan_start = None;
        self.last_pick = None;
        self.search_cancel.cancel();
        self.locate_pending = false;
        self.search_pending = false;
        self.restoring_selection = None;
        self.review.tool = None;
        self.review.drag = None;
        self.review.active = None;
        if self.view3d.active {
            self.view3d.error = None;
            if self.view3d.initialized {
                self.follow_2d_camera();
            } else if !self.view3d.camera_restored {
                self.view3d.follow_2d = true;
            }
            if self.view3d.renderer.is_none() {
                self.view3d.renderer = Some(
                    window
                        .register_gpu_painter(pomelo_render::backend::native::painter(
                            SolidRenderer::new(self.view3d.telemetry.clone()),
                        ))
                        .map_err(|e| format!("{e:#}")),
                );
            }
            if self.view3d.scene.is_none() && !self.view3d.pending {
                self.view3d.pending = true;
                self.view3d.error = None;
                self.view3d.cancel = CancellationToken::default();
                let cancel = self.view3d.cancel.clone();
                let source = self.scene.clone();
                let budget = self.preparation_budget.clone();
                let worker = cx.background_spawn(async move {
                    pomelo_render::preparation_memory::PreparationMemory::new(budget)
                        .prepare_board3d(&source, PreviewStackup::default(), &cancel)
                });
                self.view3d.task = Some(cx.spawn(async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update(cx, |this, cx| {
                        this.view3d.pending = false;
                        match result {
                            Ok(scene) => {
                                this.view3d.scene = Some(Arc::new(scene));
                                this.view3d.initialized =
                                    std::mem::take(&mut this.view3d.camera_restored);
                                this.view3d.materials = None;
                            }
                            Err(e) => this.view3d.error = Some(e.to_string()),
                        }
                        cx.notify();
                    });
                }));
            }
        } else {
            self.view3d.transition = None;
            if self.view3d.initialized {
                self.follow_3d_camera();
            }
            self.view3d.renderer = None;
            self.view3d.telemetry.frames.store(0, Ordering::Relaxed);
            self.view3d
                .telemetry
                .pending_pages
                .store(0, Ordering::Relaxed);
            self.view3d
                .telemetry
                .uploaded_bytes
                .store(0, Ordering::Relaxed);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }
    /// Record the current camera before a deliberate view change.
    pub(super) fn record_3d_view(&mut self) {
        const LIMIT: usize = 50;
        let v = &mut self.view3d;
        if v.history.last() != Some(&v.camera) {
            v.history.push(v.camera);
            if v.history.len() > LIMIT {
                v.history.remove(0);
            }
        }
        v.future.clear();
    }
    pub(super) fn step_3d_history(&mut self, back: bool) -> bool {
        let v = &mut self.view3d;
        let (from, to) = if back {
            (&mut v.history, &mut v.future)
        } else {
            (&mut v.future, &mut v.history)
        };
        let Some(camera) = from.pop() else {
            return false;
        };
        to.push(v.camera);
        v.camera = camera;
        true
    }
    /// Keep the board point and zoom of the 2D view when switching to 3D. A 2D
    /// view showing more than the fitted 3D view keeps the fitted framing.
    fn follow_2d_camera(&mut self) {
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        if !self.navigation.has_view() || size.y <= 0.0 || camera.pixels_per_mm <= 0.0 {
            return;
        }
        let pivot = self.view3d.camera.pivot();
        let height = (size.y / camera.pixels_per_mm).min(self.view3d.camera.view_height());
        self.view3d
            .camera
            .look_at([camera.center.x, camera.center.y, pivot[2]], height);
    }
    /// Return to 2D at the 3D focus and zoom.
    fn follow_3d_camera(&mut self) {
        let size = self.navigation.size();
        let height = self.view3d.camera.view_height();
        if !self.navigation.has_view() || size.y <= 0.0 || height <= 0.0 {
            return;
        }
        let pivot = self.view3d.camera.pivot();
        let camera = pomelo_core::interaction::Camera {
            center: pomelo_core::model::Point::new(pivot[0], pivot[1]),
            pixels_per_mm: size.y / height,
            ..self.navigation.camera()
        };
        self.navigation.restore_camera(camera);
    }
    pub(super) fn fit_3d(&mut self) -> bool {
        if let Some(scene) = &self.view3d.scene {
            self.view3d.initialized = self.view3d.camera.fit(
                display_bounds(scene.bounds, self.view3d.settings.z_scale),
                self.view3d.size,
                24.0,
            );
            self.view3d.initialized
        } else {
            false
        }
    }
    pub(super) fn locate_3d(
        &mut self,
        target: SelectionTarget,
        xy: pomelo_core::model::Bounds,
    ) -> bool {
        let Some(scene) = &self.view3d.scene else {
            return false;
        };
        let mut bounds: Option<pomelo_core::geometry3d::Bounds3> = None;
        for source in &scene.sources {
            let matches = match target {
                SelectionTarget::Object(object) => source.object == object,
                SelectionTarget::Net(net) => source.net == net,
                _ => true,
            };
            if !matches {
                continue;
            }
            if let Some(b) = &mut bounds {
                for i in 0..3 {
                    b.min[i] = b.min[i].min(source.bounds.min[i]);
                    b.max[i] = b.max[i].max(source.bounds.max[i]);
                }
            } else {
                bounds = Some(source.bounds);
            }
        }
        let Some(mut bounds) = bounds else {
            return false;
        };
        if !matches!(target, SelectionTarget::Object(_) | SelectionTarget::Net(_)) {
            bounds.min[0] = xy.min.x;
            bounds.min[1] = xy.min.y;
            bounds.max[0] = xy.max.x;
            bounds.max[1] = xy.max.y;
        }
        let previous = self.view3d.camera;
        let moved = self.view3d.camera.fit(
            display_bounds(bounds, self.view3d.settings.z_scale),
            self.view3d.size,
            24.0,
        );
        if moved && previous != self.view3d.camera {
            let located = std::mem::replace(&mut self.view3d.camera, previous);
            self.record_3d_view();
            self.view3d.camera = located;
        }
        moved
    }
    fn materials_3d(&mut self) -> Arc<Vec<[f32; 4]>> {
        let v = &mut self.view3d;
        if let Some(cache) = &v.materials
            && Arc::ptr_eq(&cache.display, &self.display)
            && cache.selected == self.selected_target
            && Arc::ptr_eq(&cache.related, &self.selected_related_objects)
            && cache.settings == v.settings
        {
            return cache.colors.clone();
        }
        let Some(scene) = &v.scene else {
            return Arc::new(vec![]);
        };
        let selected = if let Some(SelectionTarget::Object(object)) = self.selected_target {
            Some(object)
        } else {
            None
        };
        let net = if let Some(SelectionTarget::Net(net)) = self.selected_target {
            Some(net)
        } else {
            None
        };
        let transparent = v.settings.mode == pomelo_core::display3d::RenderMode3d::Wireframe;
        let mut colors = scene.materials(&self.scene, &self.display, transparent, selected, net);
        for (i, source) in scene.sources.iter().enumerate() {
            if colors[i + 1][3] > 0.0 && self.selected_related_objects.contains(&source.object) {
                colors[i + 1] = [1.0, 0.35, 0.05, 1.0];
            }
        }
        let colors = Arc::new(colors);
        v.materials = Some(MaterialCache {
            display: self.display.clone(),
            selected: self.selected_target,
            related: self.selected_related_objects.clone(),
            settings: v.settings,
            colors: colors.clone(),
        });
        colors
    }
    pub(super) fn render_canvas_3d(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> frame::CanvasPresentation {
        let locale = i18n::current(cx);
        if self.advance_3d_transition() {
            _window.request_animation_frame();
        }
        let materials = self.materials_3d();
        let renderer = self
            .view3d
            .renderer
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .cloned();
        let failure = self
            .view3d
            .error
            .clone()
            .or_else(|| {
                self.view3d
                    .renderer
                    .as_ref()
                    .and_then(|r| r.as_ref().err())
                    .cloned()
            })
            .or_else(|| renderer.as_ref().and_then(|r| r.status().last_error));
        let scene = self.view3d.scene.clone();
        let total_bytes = scene.as_ref().map_or(0, |s| s.upload_bytes());
        let uploaded_bytes = self.view3d.telemetry.uploaded_bytes.load(Ordering::Relaxed);
        let ready = scene.is_some()
            && self.view3d.telemetry.frames.load(Ordering::Relaxed) > 0
            && self.view3d.telemetry.pending_pages.load(Ordering::Relaxed) == 0
            && failure.is_none();
        let viewport = if let (Some(scene), Some(renderer)) = (scene, renderer) {
            let layout_view = cx.entity().downgrade();
            let paint_view = layout_view.clone();
            let mut snapshot = SolidFrame {
                scene,
                camera: self.view3d.camera,
                materials,
                z_scale: self.view3d.settings.z_scale,
            };
            canvas(
                move |bounds, _, cx| {
                    let _ = layout_view.update(cx, |this, _| {
                        this.bounds = Some(bounds);
                        this.view3d.size =
                            [f64::from(bounds.size.width), f64::from(bounds.size.height)];
                        if !this.view3d.initialized && this.fit_3d() && this.view3d.follow_2d {
                            this.view3d.follow_2d = false;
                            this.follow_2d_camera();
                        }
                        snapshot.camera = this.view3d.camera;
                    });
                    Arc::new(snapshot)
                },
                move |bounds, snapshot, window, cx| {
                    let moving = paint_view.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = moving.update(cx, |this, cx| {
                                if this.pointer_gesture.is_some() {
                                    this.move_pan(event, window, cx);
                                    cx.stop_propagation();
                                }
                            });
                        }
                    });
                    let releasing = paint_view.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = releasing.update(cx, |this, cx| {
                                if this
                                    .pointer_gesture
                                    .as_ref()
                                    .is_some_and(|g| g.button == event.button)
                                {
                                    if bounds.contains(&event.position) {
                                        this.stop_pan(event, window, cx);
                                    } else {
                                        this.cancel_pan(event, window, cx);
                                    }
                                }
                            });
                        }
                    });
                    if let Err(e) = window.paint_gpu(bounds, &renderer, snapshot) {
                        let _ = paint_view.update(cx, |this, cx| {
                            this.view3d.error = Some(format!("{e:#}"));
                            cx.notify();
                        });
                    }
                    let before = renderer.status();
                    let telemetry = paint_view
                        .upgrade()
                        .map(|v| v.read(cx).view3d.telemetry.clone());
                    let before_pending = telemetry
                        .as_ref()
                        .map_or(0, |t| t.pending_pages.load(Ordering::Relaxed));
                    let after_view = paint_view.clone();
                    let renderer = renderer.clone();
                    repaint_after_frame(window, after_view, move || {
                        let after = renderer.status();
                        let pending = telemetry
                            .as_ref()
                            .map_or(0, |t| t.pending_pages.load(Ordering::Relaxed));
                        (before.encoded != after.encoded && before.encoded == 0)
                            || before.last_error != after.last_error
                            || pending > 0
                            || before_pending != pending
                    });
                    if pomelo_render::frame_timing::continuous() {
                        window.request_animation_frame();
                    }
                },
            )
            .size_full()
            .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(text(
                    locale,
                    if self.view3d.pending {
                        Key::Preparing3d
                    } else {
                        Key::Preview3dUnavailable
                    },
                ))
                .into_any_element()
        };
        frame::CanvasPresentation {
            viewport: div()
                .relative()
                .size_full()
                .child(viewport)
                .into_any_element(),
            ready,
            geometry_ready: ready,
            failure,
            uploaded_bytes,
            total_bytes,
        }
    }
    pub(super) fn start_3d_drag(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.view3d.pan_drag =
            self.pan_tool || event.modifiers.shift || event.button == MouseButton::Middle;
        self.pointer_gesture = Some(gesture::PointerGesture::new(
            event.position,
            event.button,
            self.view3d.pan_drag,
        ));
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn move_3d_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self
            .pointer_gesture
            .as_ref()
            .is_some_and(|g| event.pressed_button != Some(g.button))
        {
            self.pointer_gesture = None;
        }
        if let Some(g) = &mut self.pointer_gesture
            && let Some(delta) = g.advance(event.position)
        {
            let delta = [f64::from(delta.x), f64::from(delta.y)];
            self.view3d.transition = None;
            if self.view3d.pan_drag {
                self.view3d.camera.pan(delta, self.view3d.size);
            } else {
                self.view3d
                    .camera
                    .orbit(-delta[0] * 0.008, -delta[1] * 0.008);
            }
            cx.notify();
        }
    }
    pub(super) fn stop_3d_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .pointer_gesture
            .as_ref()
            .is_some_and(|g| g.button == event.button)
        {
            return;
        }
        let clicked = self.pointer_gesture.take().is_some_and(|g| g.is_click());
        if clicked {
            let materials = self.materials_3d();
            if let Some(scene) = self.view3d.scene.clone()
                && let Some(bounds) = self.bounds
            {
                let p = event.position - bounds.origin;
                if let Some(mut ray) = self.view3d.camera.pick_ray(
                    [f64::from(p.x), f64::from(p.y)],
                    self.view3d.size,
                    display_bounds(scene.bounds, self.view3d.settings.z_scale),
                ) {
                    ray.origin[2] /= self.view3d.settings.z_scale;
                    ray.direction[2] /= self.view3d.settings.z_scale;
                    if let Some(object) = scene.pick(ray, &materials, self.pick_filter) {
                        let target = pomelo_core::selection::canvas_target(
                            &self.scene,
                            object,
                            self.selection_mode,
                            &CancellationToken::default(),
                        )
                        .ok();
                        if let Some(target) = target {
                            self.prepare_target(target, false, window, cx);
                        }
                    } else {
                        self.clear_selection(&ClearSelection, window, cx);
                    }
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn repaint_after_frame<V: 'static>(
    window: &Window,
    view: WeakEntity<V>,
    needs_redraw: impl FnOnce() -> bool + 'static,
) {
    window.on_next_frame(move |_, cx| {
        if needs_redraw() {
            // Frame callbacks run outside a rendered view; notify the entity directly.
            let _ = view.update(cx, |_, cx| cx.notify());
        }
    });
}

#[cfg(test)]
mod frame_tests {
    use super::repaint_after_frame;
    use gpui::{Context, IntoElement, Render, Window, div};
    use std::{cell::Cell, rc::Rc};
    struct UploadView {
        remaining: usize,
        draws: Rc<Cell<usize>>,
    }
    impl Render for UploadView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.draws.set(self.draws.get() + 1);
            let pending = self.remaining > 0;
            self.remaining = self.remaining.saturating_sub(1);
            repaint_after_frame(window, cx.entity().downgrade(), move || pending);
            div()
        }
    }
    #[gpui::test]
    fn upload_progress_repaints_from_frame_callback_and_stops_when_ready(
        cx: &mut gpui::TestAppContext,
    ) {
        let draws = Rc::new(Cell::new(0));
        let window = cx.add_window(|_, _| UploadView {
            remaining: 40,
            draws: draws.clone(),
        });
        let raw: gpui::AnyWindowHandle = window.into();
        cx.run_until_parked();
        raw.update(cx, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
        for _ in 0..45 {
            raw.update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
            cx.run_until_parked();
            raw.update(cx, |_, window, cx| {
                let _ = window.draw(cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        window
            .update(cx, |view, _, _| assert_eq!(view.remaining, 0))
            .unwrap();
        assert!(draws.get() >= 41 && draws.get() < 50);
        let ready = draws.get();
        raw.update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
        raw.update(cx, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
        assert_eq!(
            draws.get(),
            ready,
            "completed uploads must stop requesting redraws"
        );
    }
}
