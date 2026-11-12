pub(super) mod controls;
pub(super) mod measure;
pub(super) mod settings;
pub(super) mod view_cube;

use super::*;
use pomelo_core::{
    interaction::camera3d::{Camera3d, CameraView3d},
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};
use pomelo_render::{
    backend::native::{SolidFrame, SolidRenderer, SolidTelemetry},
    scene::board3d::{PreparedBoard3d, PreviewStackup},
};
use std::sync::atomic::Ordering;

struct MaterialCache {
    display: Arc<pomelo_core::display::BoardDisplay>,
    set: Arc<BTreeSet<SelectedObject>>,
    groups: Option<Arc<pomelo_render::backend::native::ObjectColors>>,
    hovered: Option<SelectedObject>,
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
    pub scene: Option<Arc<PreparedBoard3d>>,
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
    pub hovered: Option<SelectedObject>,
    /// Click position and the objects already selected there, nearest first.
    pub cycle: Option<(Point<Pixels>, Vec<SelectedObject>)>,
    /// Frame the first 3D scene around the 2D focus once it has been fitted.
    pub follow_2d: bool,
    pub transition: Option<view_cube::Transition>,
    pub cube_bounds: Option<Bounds<Pixels>>,
    /// Pointer position of a cube drag and whether it has moved enough to orbit.
    pub cube_drag: Option<(Point<Pixels>, bool)>,
    /// Measurement in progress and completed 3D measurements.
    pub measuring: Option<measure::Measure3d>,
    pub measurements: Vec<measure::Measure3d>,
    /// Display-space point the current orbit turns around.
    pub orbit_center: Option<[f64; 3]>,
    /// Conductor layers in the prepared scene, and the request that produced it.
    pub prepared_layers: BTreeSet<pomelo_core::model::LayerId>,
    pub requested_layers: Option<BTreeSet<pomelo_core::model::LayerId>>,
    pub detail: pomelo_render::scene::board3d::Detail3d,
    pub renderer_stale: bool,
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
            hovered: None,
            cycle: None,
            follow_2d: false,
            transition: None,
            cube_bounds: None,
            cube_drag: None,
            measuring: None,
            measurements: Vec::new(),
            orbit_center: None,
            prepared_layers: BTreeSet::new(),
            requested_layers: None,
            detail: Default::default(),
            renderer_stale: false,
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
                self.start_3d_preparation(cx);
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
    /// Prepare the 3D scene in the background with the current stack assumption.
    /// Conductor layers the 3D scene should contain: the visible ones, or the
    /// active or top layer when none is visible.
    fn requested_3d_layers(&self) -> BTreeSet<pomelo_core::model::LayerId> {
        let conductors: Vec<_> = self
            .scene
            .layers
            .iter()
            .filter(|l| {
                l.id.is_copper() && l.function != pomelo_core::model::LayerFunction::Dielectric
            })
            .map(|l| l.id)
            .collect();
        let visible: BTreeSet<_> = conductors
            .iter()
            .copied()
            .filter(|id| self.display.layer_visible(*id))
            .collect();
        if !visible.is_empty() {
            return visible;
        }
        self.display
            .active_layer
            .filter(|id| conductors.contains(id))
            .or(conductors.first().copied())
            .into_iter()
            .collect()
    }

    /// Prepare the visible conductor layers in the background. When memory runs
    /// short the preparation steps down in detail instead of failing.
    pub(super) fn start_3d_preparation(&mut self, cx: &mut Context<Self>) {
        let settings = self.view3d.settings;
        let stackup = PreviewStackup {
            board_mm: settings.board_mm,
            copper_mm: settings.copper_mm,
            dielectric: true,
            ..PreviewStackup::default()
        };
        let requested = self.requested_3d_layers();
        let all: BTreeSet<_> = self
            .scene
            .layers
            .iter()
            .filter(|l| {
                l.id.is_copper() && l.function != pomelo_core::model::LayerFunction::Dielectric
            })
            .map(|l| l.id)
            .collect();
        self.view3d.requested_layers = Some(requested.clone());
        self.view3d.pending = true;
        self.view3d.error = None;
        self.view3d.cancel.cancel();
        self.view3d.cancel = CancellationToken::default();
        // The previous scene's memory is released before a larger one is built.
        if self.view3d.scene.take().is_some() {
            self.view3d.camera_restored |= self.view3d.initialized;
            self.view3d.materials = None;
            // The GPU painter still holds the old scene; replace it on the next frame.
            self.view3d.renderer_stale = true;
        }
        let cancel = self.view3d.cancel.clone();
        let source = self.scene.clone();
        let budget = self.preparation_budget.clone();
        let worker = cx.background_spawn(async move {
            use pomelo_render::scene::board3d::{DETAIL_LADDER, layer_subset, out_of_memory};
            let mut last = None;
            for detail in DETAIL_LADDER {
                let layers = detail.layers(&source, &requested);
                let subset;
                let board = if layers == all {
                    source.as_ref()
                } else {
                    subset = layer_subset(&source, &layers);
                    &subset
                };
                match pomelo_render::preparation_memory::PreparationMemory::new(budget.clone())
                    .prepare_board3d(board, detail.stackup(stackup), &cancel)
                {
                    Ok(scene) => return Ok((scene, detail, layers)),
                    Err(error) if out_of_memory(&error) => last = Some(error),
                    Err(error) => return Err(error),
                }
            }
            Err(last.expect("the detail ladder is not empty"))
        });
        self.view3d.task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.view3d.pending = false;
                match result {
                    Ok((scene, detail, layers)) => {
                        this.view3d.scene = Some(Arc::new(scene));
                        this.view3d.detail = detail;
                        this.view3d.prepared_layers = layers;
                        this.view3d.initialized = std::mem::take(&mut this.view3d.camera_restored);
                        this.view3d.materials = None;
                    }
                    Err(e) => this.view3d.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// Rebuild when a layer that is not in the 3D scene becomes visible. Hiding
    /// layers needs no rebuild: their objects simply stop drawing.
    pub(super) fn ensure_3d_layers(&mut self, cx: &mut Context<Self>) {
        if !self.view3d.active || self.view3d.pending || self.view3d.scene.is_none() {
            return;
        }
        let requested = self.requested_3d_layers();
        let missing = requested
            .iter()
            .any(|id| !self.view3d.prepared_layers.contains(id));
        // A request that already had to be reduced is not retried until it changes.
        if missing && self.view3d.requested_layers.as_ref() != Some(&requested) {
            self.start_3d_preparation(cx);
        }
    }

    /// Rebuild the 3D scene after the stack assumption changed.
    pub(super) fn reprepare_3d(&mut self, cx: &mut Context<Self>) {
        self.view3d.cancel.cancel();
        self.view3d.task = None;
        self.view3d.scene = None;
        self.view3d.materials = None;
        self.view3d.pending = false;
        self.view3d.camera_restored = self.view3d.initialized || self.view3d.camera_restored;
        if self.view3d.active {
            self.start_3d_preparation(cx);
        }
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
                scene
                    .z_map(
                        self.view3d.settings.z_scale,
                        self.view3d.settings.stretch
                            == pomelo_core::display3d::ZStretch::Dielectric,
                    )
                    .bounds(scene.bounds),
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
        let z_map = scene.z_map(
            self.view3d.settings.z_scale,
            self.view3d.settings.stretch == pomelo_core::display3d::ZStretch::Dielectric,
        );
        let moved = self
            .view3d
            .camera
            .fit(z_map.bounds(bounds), self.view3d.size, 24.0);
        if moved && previous != self.view3d.camera {
            let located = std::mem::replace(&mut self.view3d.camera, previous);
            self.record_3d_view();
            self.view3d.camera = located;
        }
        moved
    }
    /// Current height map of the 3D view.
    pub(super) fn z_map_3d(&self) -> Option<pomelo_render::scene::board3d::ZMap> {
        let scene = self.view3d.scene.as_ref()?;
        Some(scene.z_map(
            self.view3d.settings.z_scale,
            self.view3d.settings.stretch == pomelo_core::display3d::ZStretch::Dielectric,
        ))
    }
    fn materials_3d(&mut self) -> Arc<Vec<[f32; 4]>> {
        let set = Arc::clone(self.review.set.objects());
        let groups = self.review.object_colors.clone();
        let v = &mut self.view3d;
        if let Some(cache) = &v.materials
            && Arc::ptr_eq(&cache.display, &self.display)
            && cache.selected == self.selected_target
            && Arc::ptr_eq(&cache.related, &self.selected_related_objects)
            && Arc::ptr_eq(&cache.set, &set)
            && cache.hovered == v.hovered
            && cache.groups.as_ref().map(Arc::as_ptr) == groups.as_ref().map(Arc::as_ptr)
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
        let colors = Arc::new(scene.materials(
            &self.scene,
            &self.display,
            &pomelo_render::scene::board3d::MaterialOptions {
                mode: v.settings.mode,
                selected,
                selected_net: net,
                related: &self.selected_related_objects,
                secondary: &set,
                hovered: v.hovered.filter(|_| v.settings.hover_preview),
                unselected_opacity: v.settings.unselected_opacity,
                selected_solid: v.settings.selected_solid,
                show_dielectric: v.settings.show_dielectric,
                group_colors: groups.as_deref(),
            },
        ));
        v.materials = Some(MaterialCache {
            display: self.display.clone(),
            selected: self.selected_target,
            related: self.selected_related_objects.clone(),
            set,
            groups,
            hovered: v.hovered,
            settings: v.settings,
            colors: colors.clone(),
        });
        colors
    }
    /// Materials of the current frame for the CPU image path.
    pub(super) fn materials_for_export(&mut self) -> Arc<Vec<[f32; 4]>> {
        self.materials_3d()
    }
    /// Object under a canvas position, using the stretch-aware ray split.
    fn pick_3d(
        &mut self,
        position: Point<Pixels>,
    ) -> Option<pomelo_core::selection::SelectedObject> {
        let materials = self.materials_3d();
        let scene = self.view3d.scene.clone()?;
        let bounds = self.bounds?;
        let z_map = self.z_map_3d()?;
        let p = position - bounds.origin;
        let ray = self.view3d.camera.pick_ray(
            [f64::from(p.x), f64::from(p.y)],
            self.view3d.size,
            z_map.bounds(scene.bounds),
        )?;
        z_map
            .model_rays(ray)
            .into_iter()
            .find_map(|ray| scene.pick(ray, &materials, self.pick_filter))
    }
    /// Pick while ignoring the objects already visited by click cycling.
    fn pick_3d_excluding(
        &mut self,
        position: Point<Pixels>,
    ) -> Option<pomelo_core::selection::SelectedObject> {
        let seen = self
            .view3d
            .cycle
            .as_ref()
            .map(|(_, seen)| seen.clone())
            .unwrap_or_default();
        if seen.is_empty() {
            return self.pick_3d(position);
        }
        let scene = self.view3d.scene.clone()?;
        let mut materials = (*self.materials_3d()).clone();
        let count = scene.sources.len() + 1;
        for (i, source) in scene.sources.iter().enumerate() {
            if seen.contains(&source.object) {
                materials[i + 1][3] = 0.0;
                if let Some(edge) = materials.get_mut(count + i + 1) {
                    edge[3] = 0.0;
                }
            }
        }
        let bounds = self.bounds?;
        let z_map = self.z_map_3d()?;
        let p = position - bounds.origin;
        let ray = self.view3d.camera.pick_ray(
            [f64::from(p.x), f64::from(p.y)],
            self.view3d.size,
            z_map.bounds(scene.bounds),
        )?;
        z_map
            .model_rays(ray)
            .into_iter()
            .find_map(|ray| scene.pick(ray, &materials, self.pick_filter))
    }
    /// Screen-space dashed lines over the unused part of the selected net's vias.
    fn render_stub_overlay(&self, cx: &App) -> Option<AnyElement> {
        if !self.view3d.settings.show_stubs {
            return None;
        }
        let report = self.nets.report.as_ref()?.1.as_ref()?;
        let scene = self.view3d.scene.as_ref()?;
        let z_map = self.z_map_3d()?;
        let size = self.view3d.size;
        let matrix = self.view3d.camera.clip_matrix(
            size,
            z_map.bounds(scene.bounds),
            [scene.origin.x, scene.origin.y, 0.0],
        )?;
        let index: BTreeMap<_, _> = scene
            .layers
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        let preview = scene.preview;
        let pitch = if scene.layers.len() > 1 {
            (preview.board_mm - preview.copper_mm) / (scene.layers.len() - 1) as f64
        } else {
            0.0
        };
        let top = |i: usize| preview.board_mm * 0.5 - pitch * i as f64;
        let project = |x: f64, y: f64, z: f64| {
            let p = [
                (x - scene.origin.x) as f32,
                (y - scene.origin.y) as f32,
                z_map.apply(z) as f32,
                1.0,
            ];
            let clip: [f32; 4] =
                std::array::from_fn(|row| (0..4).map(|col| matrix[col][row] * p[col]).sum());
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
        let mut lines = Vec::new();
        for via in &report.vias {
            for (from, to) in &via.stubs {
                let (Some(&a), Some(&b)) = (index.get(from), index.get(to)) else {
                    continue;
                };
                let (za, zb) = if b > a {
                    (top(a) - preview.copper_mm, top(b) - preview.copper_mm)
                } else {
                    (top(a), top(b))
                };
                lines.push((
                    project(via.at.x, via.at.y, za),
                    project(via.at.x, via.at.y, zb),
                ));
            }
        }
        if lines.is_empty() {
            return None;
        }
        let color = cx.theme().danger;
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    for ((ax, ay), (bx, by)) in &lines {
                        let length = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
                        if !length.is_finite() || length < 1.0 {
                            continue;
                        }
                        let (dx, dy) = ((bx - ax) / length, (by - ay) / length);
                        let mut t = 0.0;
                        while t < length {
                            let end = (t + 6.0).min(length);
                            let mut path = gpui::PathBuilder::stroke(px(2.0));
                            path.move_to(bounds.origin + point(px(ax + dx * t), px(ay + dy * t)));
                            path.line_to(
                                bounds.origin + point(px(ax + dx * end), px(ay + dy * end)),
                            );
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color);
                            }
                            t += 10.0;
                        }
                    }
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }
    /// Display-space point under the pointer: the surface of the object hit, or
    /// the board's mid plane when nothing is under it.
    pub(super) fn pointer_anchor_3d(&mut self, position: Point<Pixels>) -> Option<[f64; 3]> {
        let scene = self.view3d.scene.clone()?;
        let bounds = self.bounds?;
        let z_map = self.z_map_3d()?;
        let local = position - bounds.origin;
        let ray = self.view3d.camera.pick_ray(
            [f64::from(local.x), f64::from(local.y)],
            self.view3d.size,
            z_map.bounds(scene.bounds),
        )?;
        let model_z = self
            .pick_3d(position)
            .and_then(|object| scene.sources.iter().find(|s| s.object == object))
            .map_or(0.0, |source| {
                0.5 * (source.bounds.min[2] + source.bounds.max[2])
            });
        let z = z_map.apply(model_z);
        if ray.direction[2].abs() < 1e-9 {
            return None;
        }
        let t = (z - ray.origin[2]) / ray.direction[2];
        let point = [
            ray.origin[0] + ray.direction[0] * t,
            ray.origin[1] + ray.direction[1] * t,
            z,
        ];
        point.into_iter().all(f64::is_finite).then_some(point)
    }
    /// Track the object under the pointer for the hover preview.
    pub(super) fn hover_3d(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.view3d.settings.hover_preview || self.pointer_gesture.is_some() {
            return;
        }
        let hovered = self.pick_3d(position);
        if hovered != self.view3d.hovered {
            self.view3d.hovered = hovered;
            cx.notify();
        }
    }
    pub(super) fn render_canvas_3d(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> frame::CanvasPresentation {
        let locale = i18n::current(cx);
        if std::mem::take(&mut self.view3d.renderer_stale) && self.view3d.renderer.is_some() {
            self.view3d.renderer = Some(
                _window
                    .register_gpu_painter(pomelo_render::backend::native::painter(
                        SolidRenderer::new(self.view3d.telemetry.clone()),
                    ))
                    .map_err(|e| format!("{e:#}")),
            );
        }
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
                dielectric_only: self.view3d.settings.stretch
                    == pomelo_core::display3d::ZStretch::Dielectric,
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
                .children(self.render_stub_overlay(cx))
                .children(self.render_measure_overlay_3d(locale, cx))
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
        self.view3d.pan_drag = self.pan_tool
            || event.modifiers.shift
            || matches!(event.button, MouseButton::Middle | MouseButton::Right);
        // Orbit about the surface under the pointer, so that point stays put.
        self.view3d.orbit_center = if self.view3d.pan_drag {
            None
        } else {
            self.pointer_anchor_3d(event.position)
        };
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
                let (yaw, pitch) = (-delta[0] * 0.008, -delta[1] * 0.008);
                match self.view3d.orbit_center {
                    Some(center) => self.view3d.camera.orbit_around(center, yaw, pitch),
                    None => self.view3d.camera.orbit(yaw, pitch),
                };
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
        // Only a left click selects or measures; right and middle clicks just end a pan.
        let clicked = self
            .pointer_gesture
            .take()
            .is_some_and(|g| g.is_click() && g.button == MouseButton::Left);
        if clicked && self.measure_click_3d(event.position, event.click_count >= 2, cx) {
            cx.stop_propagation();
            return;
        }
        if clicked && self.view3d.scene.is_some() {
            // Clicking the same spot again steps to the next object behind the last hit.
            let same_spot = self
                .view3d
                .cycle
                .as_ref()
                .is_some_and(|(at, _)| (*at - event.position).magnitude() < 3.0);
            if !same_spot {
                self.view3d.cycle = Some((event.position, Vec::new()));
            }
            let mut object = self.pick_3d_excluding(event.position);
            if object.is_none() && same_spot {
                self.view3d.cycle = Some((event.position, Vec::new()));
                object = self.pick_3d_excluding(event.position);
            }
            if let Some(object) = object
                && event.modifiers.shift
            {
                // Shift-click edits the selection set, as in 2D.
                self.review.set.toggle(object);
                self.review.notice = None;
            } else if let Some(object) = object {
                if let Some((_, seen)) = &mut self.view3d.cycle {
                    seen.push(object);
                }
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
                self.view3d.cycle = None;
                self.clear_selection(&ClearSelection, window, cx);
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
