//! Windows native board canvas. Business GPU resources stay in pomelo-render.

use crate::i18n;
use crate::inspector::{prepare_inspection, selection_identity};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    base::{Disableable, Selectable},
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
        scroll::ScrollableElement,
    },
    *,
};
use pomelo_core::{
    i18n::{Message, MessageKey as Key, text},
    interaction::ViewportNavigation,
    model::BoardScene,
};
use pomelo_render::{
    backend::d3d11::{BoardFrame, BoardRenderer, CopperTelemetry, TraceFrame, TraceTelemetry},
    tracks::PreparedTracks,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

actions!(
    pomelo,
    [
        FocusSearch,
        LeaveSearch,
        ClearSelection,
        NextCandidate,
        FitBoard,
        FlipBoard,
        ZoomIn,
        ZoomOut,
        PanLeft,
        PanRight,
        PanUp,
        PanDown
    ]
);

pub struct BoardViewport {
    hovered: Option<(pomelo_core::selection::SelectedObject, PickContext)>,
    hover_notice: Option<Message>,
    hover_cancel: pomelo_core::task::CancellationToken,
    hover_task: Option<Task<()>>,
    last_pick: Option<PickContext>,
    candidate_position: Option<(usize, usize)>,
    tracks: Arc<PreparedTracks>,
    drawings: Arc<PreparedTracks>,
    texts: Option<Arc<pomelo_render::text_instances::PreparedTextInstances>>,
    copper: Arc<pomelo_render::copper::PreparedCopper>,
    pads: Arc<pomelo_render::pads::PreparedPads>,
    drills: Arc<pomelo_render::pads::PreparedPads>,
    scene: Arc<BoardScene>,
    search: Arc<pomelo_core::search::SearchIndex>,
    picking: Arc<pomelo_core::picking_index::SegmentIndex>,
    search_input: Entity<InputState>,
    search_results: Vec<pomelo_core::search::SearchEntry>,
    selected_target: Option<pomelo_core::selection::SelectionTarget>,
    restoring_selection: Option<(
        pomelo_core::selection::SelectionTarget,
        pomelo_core::task::CancellationToken,
    )>,
    selected_source_labels: Vec<Message>,
    selected_related_net: Option<pomelo_core::model::NetId>,
    selected_related_component: Option<pomelo_core::model::ObjectId>,
    selection_mode: pomelo_core::interaction::SelectionMode,
    pick_filter: pomelo_core::picking::PickFilter,
    selected_summary: Option<pomelo_core::selection::SelectionSummary>,
    selected_members: Option<pomelo_core::selection::SelectionMembers>,
    members_offset: usize,
    members_page_revision: u64,
    selected_pins: Arc<BTreeSet<pomelo_core::model::ObjectId>>,
    selected_bonds: Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
    search_cancel: pomelo_core::task::CancellationToken,
    search_task: Option<Task<()>>,
    search_pending: bool,
    search_notice: Option<Message>,
    locate_pending: bool,
    pending_message: Key,
    locate_details: Option<pomelo_core::geometry::PathError>,
    search_query: String,
    layer_scroll: UniformListScrollHandle,
    display: Arc<pomelo_core::display::BoardDisplay>,
    colors: Arc<BTreeMap<pomelo_core::model::LayerId, [f32; 4]>>,
    renderer: Result<NativeGpuHandle, String>,
    telemetry: Arc<TraceTelemetry>,
    copper_telemetry: Arc<CopperTelemetry>,
    pad_telemetry: Arc<TraceTelemetry>,
    custom_pad_telemetry: Arc<CopperTelemetry>,
    drill_telemetry: Arc<TraceTelemetry>,
    drawing_telemetry: Arc<TraceTelemetry>,
    text_telemetry: Arc<TraceTelemetry>,
    paint_error: Option<String>,
    navigation: ViewportNavigation,
    bounds: Option<Bounds<Pixels>>,
    drag_position: Option<Point<Pixels>>,
    pointer_position: Option<Point<Pixels>>,
    focus: FocusHandle,
}

fn snapshot_selection(
    selected: Option<pomelo_core::selection::SelectionTarget>,
    restoring: Option<&(
        pomelo_core::selection::SelectionTarget,
        pomelo_core::task::CancellationToken,
    )>,
) -> Option<pomelo_core::selection::SelectionTarget> {
    selected.or_else(|| {
        restoring
            .filter(|(_, cancel)| !cancel.is_cancelled())
            .map(|(target, _)| *target)
    })
}

struct PickContext {
    local: pomelo_core::model::Point,
    camera: pomelo_core::interaction::Camera,
    size: pomelo_core::model::Point,
    display: Arc<pomelo_core::display::BoardDisplay>,
    mode: pomelo_core::interaction::SelectionMode,
}

impl PickContext {
    fn matches_view(
        &self,
        camera: pomelo_core::interaction::Camera,
        size: pomelo_core::model::Point,
        display: &Arc<pomelo_core::display::BoardDisplay>,
        mode: pomelo_core::interaction::SelectionMode,
    ) -> bool {
        self.size == size
            && self.mode == mode
            && self.camera.center == camera.center
            && self.camera.pixels_per_mm == camera.pixels_per_mm
            && self.camera.flipped == camera.flipped
            && Arc::ptr_eq(&self.display, display)
    }
}

fn cycle_target(
    candidates: &[pomelo_core::selection::SelectionCandidate],
    previous: Option<pomelo_core::selection::SelectionTarget>,
) -> Option<pomelo_core::selection::SelectionTarget> {
    if candidates.is_empty() {
        return None;
    }
    let index = previous
        .and_then(|target| {
            candidates
                .iter()
                .position(|candidate| candidate.target == target)
        })
        .map_or(0, |index| (index + 1) % candidates.len());
    Some(candidates[index].target)
}

fn candidate_position(
    candidates: &[pomelo_core::selection::SelectionCandidate],
    target: pomelo_core::selection::SelectionTarget,
) -> Option<(usize, usize)> {
    candidates
        .iter()
        .position(|candidate| candidate.target == target)
        .map(|index| (index + 1, candidates.len()))
}

fn source_color(color: &str) -> Option<[f32; 4]> {
    let color = color.strip_prefix('#')?;
    if color.len() != 6 || !color.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(color, 16).ok()?;
    Some([
        ((value >> 16) & 255) as f32 / 255.0,
        ((value >> 8) & 255) as f32 / 255.0,
        (value & 255) as f32 / 255.0,
        1.0,
    ])
}

impl BoardViewport {
    pub fn snapshot(
        &self,
        source: &pomelo_core::view_state::SourceIdentity,
    ) -> Option<pomelo_core::view_state::ViewState> {
        if !self.navigation.has_view() {
            return None;
        }
        Some(pomelo_core::view_state::ViewState {
            schema_version: 1,
            source: source.clone(),
            camera: self.navigation.camera(),
            display: (*self.display).clone(),
            selection_mode: self.selection_mode,
            pick_filter: self.pick_filter,
            selection: snapshot_selection(self.selected_target, self.restoring_selection.as_ref()),
        })
    }

    pub fn new(
        prepared: &crate::document::PreparedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tracks = Arc::clone(&prepared.tracks);
        let drawings = Arc::clone(&prepared.drawings);
        let texts = prepared.texts.clone();
        let copper = Arc::clone(&prepared.copper);
        let pads = Arc::clone(&prepared.pads);
        let drills = Arc::clone(&prepared.drills);
        let scene = Arc::clone(&prepared.board.scene);
        let search = Arc::clone(&prepared.search);
        let telemetry = Arc::new(TraceTelemetry::default());
        let copper_telemetry = Arc::new(CopperTelemetry::default());
        let pad_telemetry = Arc::new(TraceTelemetry::default());
        let custom_pad_telemetry = Arc::new(CopperTelemetry::default());
        let drill_telemetry = Arc::new(TraceTelemetry::default());
        let drawing_telemetry = Arc::new(TraceTelemetry::default());
        let text_telemetry = Arc::new(TraceTelemetry::default());
        let renderer = window
            .register_gpu_renderer(BoardRenderer::new(
                Arc::clone(&telemetry),
                Arc::clone(&copper_telemetry),
                Arc::clone(&pad_telemetry),
                Arc::clone(&custom_pad_telemetry),
                Arc::clone(&drill_telemetry),
                Arc::clone(&drawing_telemetry),
                Arc::clone(&text_telemetry),
            ))
            .map_err(|error| format!("{error:#}"));
        let colors =
            scene
                .layers
                .iter()
                .filter_map(|layer| source_color(&layer.color).map(|color| (layer.id, color)))
                .chain(
                    scene.special_layers.iter().filter_map(|layer| {
                        source_color(&layer.color).map(|color| (layer.id, color))
                    }),
                )
                .chain(
                    scene.drawing_layers.iter().filter_map(|layer| {
                        source_color(&layer.color).map(|color| (layer.id, color))
                    }),
                )
                .collect();
        let mut initial_display = pomelo_core::display::BoardDisplay::default();
        for layer in &scene.drawing_layers {
            if !layer.default_visible {
                initial_display.hidden_layers.insert(layer.id);
            }
        }
        let search_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&search_input, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.query_search(input.read(cx).value().to_string(), window, cx);
            } else if matches!(
                event,
                InputEvent::PressEnter {
                    secondary: false,
                    shift: false
                }
            ) && !this.search_pending
            {
                let first = this
                    .search_results
                    .iter()
                    .find(|entry| matches!(entry.target, pomelo_core::search::SearchTarget::Net(_)))
                    .or_else(|| this.search_results.first());
                if let Some(entry) = first {
                    this.locate_search(entry.target, window, cx);
                }
            }
        })
        .detach();
        let mut viewport = Self {
            search_input,
            search_results: Vec::new(),
            selected_target: None,
            restoring_selection: None,
            selected_source_labels: Vec::new(),
            selected_related_net: None,
            selected_related_component: None,
            selection_mode: pomelo_core::interaction::SelectionMode::Net,
            last_pick: None,
            candidate_position: None,
            selected_summary: None,
            selected_members: None,
            hovered: None,
            hover_notice: None,
            hover_cancel: pomelo_core::task::CancellationToken::default(),
            hover_task: None,
            pick_filter: pomelo_core::picking::PickFilter::all(),
            members_offset: 0,
            members_page_revision: 0,
            selected_pins: Arc::new(BTreeSet::new()),
            selected_bonds: Arc::new(BTreeSet::new()),
            search_cancel: pomelo_core::task::CancellationToken::default(),
            search_task: None,
            search_pending: false,
            search_notice: None,
            locate_pending: false,
            pending_message: Key::Locating,
            locate_details: None,
            search_query: String::new(),
            layer_scroll: UniformListScrollHandle::new(),
            tracks,
            drawings,
            copper,
            pads,
            drills,
            scene,
            search,
            picking: Arc::clone(&prepared.picking),
            display: Arc::new(initial_display),
            colors: Arc::new(colors),
            renderer,
            telemetry,
            copper_telemetry,
            pad_telemetry,
            custom_pad_telemetry,
            drill_telemetry,
            drawing_telemetry,
            text_telemetry,
            texts,
            paint_error: None,
            navigation: ViewportNavigation::default(),
            bounds: None,
            drag_position: None,
            pointer_position: None,
            focus: cx.focus_handle(),
        };
        if let Some(state) = &prepared.restored_view
            && viewport.navigation.restore_camera(state.camera)
        {
            viewport.display = Arc::new(state.display.clone());
            viewport.selection_mode = state.selection_mode;
            viewport.pick_filter = state.pick_filter;
            viewport.invalidate_hover();
            if let Some(target) = state.selection {
                viewport.prepare_target(target, false, window, cx);
            }
        }
        viewport
    }

    fn query_search(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.last_pick = None;
        self.candidate_position = None;
        self.search_cancel.cancel();
        self.search_cancel = pomelo_core::task::CancellationToken::default();
        self.search_query = query.clone();
        self.search_results.clear();
        self.search_notice = None;
        self.locate_pending = false;
        self.locate_details = None;
        self.search_pending = !query.trim().is_empty();
        let cancel = self.search_cancel.clone();
        let worker_cancel = cancel.clone();
        let search = Arc::clone(&self.search);
        let worker = cx.background_spawn(async move {
            search
                .find_cancellable(&query, 20, &worker_cancel)
                .map(|entries| entries.into_iter().cloned().collect::<Vec<_>>())
        });
        self.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let results = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                if let Some(results) = results {
                    this.search_results = results;
                }
                this.search_pending = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn locate_search(
        &mut self,
        target: pomelo_core::search::SearchTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locate_target(target.into(), window, cx);
    }

    fn locate_target(
        &mut self,
        target: pomelo_core::selection::SelectionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_target(target, true, window, cx);
    }

    fn prepare_target(
        &mut self,
        target: pomelo_core::selection::SelectionTarget,
        move_camera: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_pick = None;
        self.search_notice = None;
        self.search_pending = false;
        self.locate_pending = true;
        self.pending_message = Key::Locating;
        self.locate_details = None;
        self.search_cancel.cancel();
        self.search_cancel = pomelo_core::task::CancellationToken::default();
        let cancel = self.search_cancel.clone();
        let worker_cancel = cancel.clone();
        let scene = Arc::clone(&self.scene);
        if !move_camera {
            self.restoring_selection = Some((target, cancel.clone()));
        }
        let worker = cx.background_spawn(async move {
            let bounds = target
                .exists(&scene, &worker_cancel)
                .and_then(|exists| {
                    if exists {
                        target.bounds(&scene, &worker_cancel)
                    } else {
                        Ok(None)
                    }
                })
                .and_then(|bounds| {
                    target
                        .summarize(&scene, &worker_cancel)
                        .and_then(|summary| {
                            target
                                .members(&scene, 256, &worker_cancel)
                                .and_then(|members| {
                                    prepare_inspection(target, &scene, &worker_cancel)
                                        .map(|inspection| (bounds, summary, members, inspection))
                                })
                        })
                });
            let pins = match target {
                pomelo_core::selection::SelectionTarget::Component(id) => scene
                    .components
                    .iter()
                    .find(|component| component.id == id)
                    .map(|component| component.pins.iter().copied().collect())
                    .unwrap_or_default(),
                pomelo_core::selection::SelectionTarget::Object(
                    pomelo_core::selection::SelectedObject::Pin(id),
                ) => BTreeSet::from([id]),
                _ => BTreeSet::new(),
            };
            (bounds, Arc::new(pins))
        });
        self.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let (result, pins) = worker.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                this.locate_pending = false;
                this.restoring_selection = None;
                match result {
                    Ok((Some(bounds), summary, members, inspection))
                        if !move_camera || this.navigation.locate(bounds, 2.0) =>
                    {
                        this.invalidate_hover();
                        this.members_offset = 0;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.selected_members = Some(members);
                        this.selected_target = Some(target);
                        this.selected_related_net = inspection.net;
                        this.selected_related_component = inspection.component;
                        this.selected_source_labels = inspection.labels;
                        this.selected_bonds = inspection.bonds;
                        this.selected_summary = Some(summary);
                        this.selected_pins = pins;
                        window.focus(&this.focus, cx);
                    }
                    Ok((None, _, _, _)) => {
                        this.search_notice = Some(Message::new(Key::SearchLocateEmpty))
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return,
                    Err(error) => {
                        this.search_notice = Some(match &error {
                            pomelo_core::geometry::PathError::Invalid(id) => {
                                Message::new(Key::SearchLocateInvalid).arg("object", id.0)
                            }
                            _ => Message::new(Key::SearchLocateFailed),
                        });
                        this.locate_details = Some(error);
                    }
                    Ok((Some(_), _, _, _)) => {
                        this.search_notice = Some(Message::new(Key::SearchLocateFailed))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.search_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    fn page_members(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.locate_pending {
            return;
        }
        let Some(target) = self.selected_target else {
            return;
        };
        self.search_cancel.cancel();
        self.search_cancel = pomelo_core::task::CancellationToken::default();
        let cancel = self.search_cancel.clone();
        let worker_cancel = cancel.clone();
        let scene = Arc::clone(&self.scene);
        self.search_pending = false;
        self.locate_pending = true;
        self.search_notice = None;
        self.locate_details = None;
        self.pending_message = Key::LoadingMembers;
        let worker =
            cx.background_spawn(
                async move { target.members_page(&scene, offset, 256, &worker_cancel) },
            );
        self.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() || this.selected_target != Some(target) {
                    return;
                }
                this.locate_pending = false;
                match result {
                    Ok(members) => {
                        this.members_offset = offset;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.selected_members = Some(members);
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return,
                    Err(error) => {
                        this.search_notice = Some(match &error {
                            pomelo_core::geometry::PathError::Invalid(id) => {
                                Message::new(Key::MembersInvalid).arg("object", id.0)
                            }
                            _ => Message::new(Key::MembersFailed),
                        });
                        this.locate_details = Some(error);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn leave_search(&mut self, _: &LeaveSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search_cancel.cancel();
        self.search_pending = false;
        self.locate_pending = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn clear_selection(&mut self, _: &ClearSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.last_pick = None;
        self.candidate_position = None;
        self.search_cancel.cancel();
        self.search_pending = false;
        self.locate_pending = false;
        self.search_notice = None;
        self.locate_details = None;
        self.selected_target = None;
        self.selected_source_labels.clear();
        self.selected_related_net = None;
        self.selected_related_component = None;
        self.selected_summary = None;
        self.selected_members = None;
        self.selected_pins = Arc::new(BTreeSet::new());
        self.selected_bonds = Arc::new(BTreeSet::new());
        cx.notify();
    }

    fn pick_trace(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let local = event.position - bounds.origin;
        self.pick_at(
            pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            window,
            cx,
        );
    }

    fn next_candidate(&mut self, _: &NextCandidate, window: &mut Window, cx: &mut Context<Self>) {
        if self.locate_pending {
            return;
        }
        let Some(last) = self.last_pick.as_ref().filter(|last| {
            last.matches_view(
                self.navigation.camera(),
                self.navigation.size(),
                &self.display,
                self.selection_mode,
            )
        }) else {
            return;
        };
        self.pick_at(last.local, window, cx);
    }

    fn pick_at(
        &mut self,
        local: pomelo_core::model::Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        let Some(query) = pomelo_core::picking::PickQuery::from_viewport(
            camera,
            local,
            self.navigation.size(),
            5.0,
        ) else {
            return;
        };
        self.search_cancel.cancel();
        self.search_cancel = pomelo_core::task::CancellationToken::default();
        let cancel = self.search_cancel.clone();
        let worker_cancel = cancel.clone();
        let index = Arc::clone(&self.picking);
        let scene = Arc::clone(&self.scene);
        let display = Arc::clone(&self.display);
        let worker_display = Arc::clone(&display);
        let mode = self.selection_mode;
        let filter = self.pick_filter;
        let pick_context = PickContext {
            local,
            camera,
            size,
            display: Arc::clone(&display),
            mode,
        };
        let previous = self
            .last_pick
            .as_ref()
            .filter(|last| {
                last.local == pick_context.local && last.matches_view(camera, size, &display, mode)
            })
            .and(self.selected_target);
        self.search_pending = false;
        self.locate_pending = true;
        self.pending_message = Key::Selecting;
        self.search_notice = None;
        self.locate_details = None;
        let worker = cx.background_spawn(async move {
            if mode == pomelo_core::interaction::SelectionMode::Component {
                let hits = query.pins(&scene, filter, &worker_display, 64, &worker_cancel)?;
                let mut hits: Vec<_> = hits
                    .into_iter()
                    .map(|hit| pomelo_core::picking::ObjectHit {
                        object: pomelo_core::selection::SelectedObject::Pin(hit.pin.id),
                        distance_mm: hit.distance_mm,
                    })
                    .collect();
                hits.extend(
                    query
                        .vias_where(&scene, filter, &worker_display, 64, &worker_cancel, |via| {
                            via.finger
                                .as_ref()
                                .and_then(|finger| finger.source_pin)
                                .is_some()
                        })?
                        .into_iter()
                        .map(|hit| pomelo_core::picking::ObjectHit {
                            object: pomelo_core::selection::SelectedObject::Via(hit.via.id),
                            distance_mm: hit.distance_mm,
                        }),
                );
                hits.extend(
                    index
                        .query_where(
                            query,
                            filter,
                            &worker_display,
                            64,
                            &worker_cancel,
                            |segment| segment.bond_wire.is_some(),
                        )?
                        .into_iter()
                        .map(|hit| pomelo_core::picking::ObjectHit {
                            object: pomelo_core::selection::SelectedObject::Segment(hit.segment.id),
                            distance_mm: hit.distance_mm,
                        }),
                );
                let priority = |object| match object {
                    pomelo_core::selection::SelectedObject::Pin(_) => 0,
                    pomelo_core::selection::SelectedObject::Via(_) => 1,
                    _ => 2,
                };
                hits.sort_by(|left, right| {
                    left.distance_mm
                        .total_cmp(&right.distance_mm)
                        .then_with(|| priority(left.object).cmp(&priority(right.object)))
                });
                let candidates = pomelo_core::selection::resolve_candidates(
                    &scene,
                    &hits,
                    mode,
                    64,
                    &worker_cancel,
                )?;
                let target = cycle_target(&candidates, previous);
                let Some(target @ pomelo_core::selection::SelectionTarget::Component(id)) = target
                else {
                    return Ok(None);
                };
                let summary = target.summarize(&scene, &worker_cancel)?;
                let mut pins = BTreeSet::new();
                for component in &scene.components {
                    if worker_cancel.is_cancelled() {
                        return Err(pomelo_core::geometry::PathError::Cancelled);
                    }
                    if component.id != id {
                        continue;
                    }
                    for &pin in &component.pins {
                        if worker_cancel.is_cancelled() {
                            return Err(pomelo_core::geometry::PathError::Cancelled);
                        }
                        pins.insert(pin);
                    }
                    break;
                }
                return Ok(Some((
                    target,
                    summary,
                    Arc::new(pins),
                    candidate_position(&candidates, target),
                    target.members(&scene, 256, &worker_cancel)?,
                    prepare_inspection(target, &scene, &worker_cancel)?,
                )));
            }
            if matches!(
                mode,
                pomelo_core::interaction::SelectionMode::Object
                    | pomelo_core::interaction::SelectionMode::Net
            ) {
                let hits = index.query_objects(
                    query,
                    filter,
                    &worker_display,
                    64,
                    &worker_cancel,
                    mode == pomelo_core::interaction::SelectionMode::Net,
                )?;
                let candidates = pomelo_core::selection::resolve_candidates(
                    &scene,
                    &hits,
                    mode,
                    64,
                    &worker_cancel,
                )?;
                let Some(target) = cycle_target(&candidates, previous) else {
                    return Ok(None);
                };
                let summary = target.summarize(&scene, &worker_cancel)?;
                let pins = match target {
                    pomelo_core::selection::SelectionTarget::Object(
                        pomelo_core::selection::SelectedObject::Pin(id),
                    ) => BTreeSet::from([id]),
                    _ => BTreeSet::new(),
                };
                return Ok(Some((
                    target,
                    summary,
                    Arc::new(pins),
                    candidate_position(&candidates, target),
                    target.members(&scene, 256, &worker_cancel)?,
                    prepare_inspection(target, &scene, &worker_cancel)?,
                )));
            }
            let hits = index.query_where(
                query,
                filter,
                &worker_display,
                64,
                &worker_cancel,
                |segment| {
                    mode != pomelo_core::interaction::SelectionMode::Net || segment.net.0 != 0
                },
            )?;
            let hits: Vec<_> = hits
                .into_iter()
                .map(|hit| pomelo_core::picking::ObjectHit {
                    object: pomelo_core::selection::SelectedObject::Segment(hit.segment.id),
                    distance_mm: hit.distance_mm,
                })
                .collect();
            let candidates = pomelo_core::selection::resolve_candidates(
                &scene,
                &hits,
                mode,
                64,
                &worker_cancel,
            )?;
            if let Some(target) = cycle_target(&candidates, previous) {
                let summary = target.summarize(&scene, &worker_cancel)?;
                return Ok::<_, pomelo_core::geometry::PathError>(Some((
                    target,
                    summary,
                    Arc::new(BTreeSet::new()),
                    candidate_position(&candidates, target),
                    target.members(&scene, 256, &worker_cancel)?,
                    prepare_inspection(target, &scene, &worker_cancel)?,
                )));
            }
            Ok(None)
        });
        self.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                this.locate_pending = false;
                let current = this.navigation.camera();
                if !Arc::ptr_eq(&display, &this.display)
                    || this.pick_filter != filter
                    || this.navigation.size() != size
                    || current.center != camera.center
                    || current.pixels_per_mm != camera.pixels_per_mm
                    || current.flipped != camera.flipped
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some((target, summary, pins, position, members, inspection))) => {
                        this.members_offset = 0;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.selected_members = Some(members);
                        this.candidate_position = position;
                        this.last_pick = Some(pick_context);
                        this.selected_target = Some(target);
                        this.selected_related_net = inspection.net;
                        this.selected_related_component = inspection.component;
                        this.selected_source_labels = inspection.labels;
                        this.selected_bonds = inspection.bonds;
                        this.selected_summary = Some(summary);
                        this.selected_pins = pins;
                    }
                    Ok(None) => {
                        this.candidate_position = None;
                        this.last_pick = None;
                        this.selected_target = None;
                        this.selected_source_labels.clear();
                        this.selected_related_net = None;
                        this.selected_related_component = None;
                        this.selected_summary = None;
                        this.selected_members = None;
                        this.selected_pins = Arc::new(BTreeSet::new());
                        this.selected_bonds = Arc::new(BTreeSet::new());
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return,
                    Err(error) => {
                        this.search_notice = Some(match &error {
                            pomelo_core::geometry::PathError::Invalid(id) => {
                                Message::new(Key::PickInvalid).arg("object", id.0)
                            }
                            _ => Message::new(Key::PickFailed),
                        });
                        this.locate_details = Some(error);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn fit_board(&mut self, _: &FitBoard, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.fit(self.scene.bounds) {
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn flip_board(&mut self, _: &FlipBoard, _: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_hover();
        self.navigation.flip();
        cx.notify();
    }

    fn zoom(&mut self, factor: f64, cx: &mut Context<Self>) {
        let size = self.navigation.size();
        if self.navigation.zoom_at(
            pomelo_core::model::Point::new(size.x * 0.5, size.y * 0.5),
            factor,
        ) {
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn pan(&mut self, x: f64, y: f64, cx: &mut Context<Self>) {
        let size = self.navigation.size();
        // Keyboard movement is proportional to the viewport, independent of DPI.
        if self.navigation.pan(pomelo_core::model::Point::new(
            x * size.x * 0.1,
            y * size.y * 0.1,
        )) {
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let steps = match event.delta {
            ScrollDelta::Lines(delta) => f64::from(delta.y),
            // Trackpad pixel deltas use a smooth, bounded zoom curve.
            ScrollDelta::Pixels(delta) => f64::from(f32::from(delta.y)) / 40.0,
        };
        if !steps.is_finite() || steps == 0.0 {
            return;
        }
        let anchor = event.position - bounds.origin;
        if self.navigation.zoom_at(
            pomelo_core::model::Point::new(
                f64::from(f32::from(anchor.x)),
                f64::from(f32::from(anchor.y)),
            ),
            1.2_f64.powf(steps.clamp(-10.0, 10.0)),
        ) {
            cx.stop_propagation();
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn start_pan(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.hover_cancel.cancel();
        self.hovered = None;
        self.hover_notice = None;
        window.focus(&self.focus, cx);
        self.drag_position = Some(event.position);
        cx.stop_propagation();
        cx.notify();
    }

    fn invalidate_hover(&mut self) {
        self.hover_cancel.cancel();
        self.hover_task = None;
        self.hovered = None;
        self.hover_notice = None;
    }

    fn hover_at(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let local = position - bounds.origin;
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        let Some(query) = pomelo_core::picking::PickQuery::from_viewport(
            camera,
            pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            size,
            5.0,
        ) else {
            return;
        };
        self.hover_cancel.cancel();
        self.hover_cancel = pomelo_core::task::CancellationToken::default();
        let cancel = self.hover_cancel.clone();
        let worker_cancel = cancel.clone();
        let index = Arc::clone(&self.picking);
        let display = Arc::clone(&self.display);
        let worker_display = Arc::clone(&display);
        let filter = self.pick_filter;
        let context = PickContext {
            local: pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            camera,
            size,
            display,
            mode: self.selection_mode,
        };
        let delay = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(40));
        let worker = cx.background_spawn(async move {
            delay.await;
            if worker_cancel.is_cancelled() {
                return Err(pomelo_core::geometry::PathError::Cancelled);
            }
            index.query_objects(query, filter, &worker_display, 1, &worker_cancel, false)
        });
        self.hover_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled()
                    || this.pick_filter != filter
                    || !context.matches_view(
                        this.navigation.camera(),
                        this.navigation.size(),
                        &this.display,
                        this.selection_mode,
                    )
                {
                    return;
                }
                this.hover_notice = None;
                match result {
                    Ok(hits) => {
                        this.hovered = hits.first().map(|hit| (hit.object, context));
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return,
                    Err(error) => {
                        this.hovered = None;
                        this.hover_notice = Some(match error {
                            pomelo_core::geometry::PathError::Invalid(id) => {
                                Message::new(Key::HoverInvalid).arg("object", id.0)
                            }
                            _ => Message::new(Key::HoverFailed),
                        });
                    }
                }
                cx.notify();
            });
        }));
    }

    fn move_pan(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.pointer_position != Some(event.position) {
            self.pointer_position = Some(event.position);
            cx.notify();
        }
        if event.pressed_button != Some(MouseButton::Middle) {
            if self.drag_position.take().is_some() {
                cx.notify();
            }
            if event.pressed_button.is_none() {
                self.hover_at(event.position, window, cx);
            }
            return;
        }
        if let Some(previous) = self.drag_position {
            self.drag_position = Some(event.position);
            let delta = event.position - previous;
            if self.navigation.pan(pomelo_core::model::Point::new(
                f64::from(f32::from(delta.x)),
                f64::from(f32::from(delta.y)),
            )) {
                cx.notify();
            }
        }
    }

    fn stop_pan(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag_position.take().is_some() {
            cx.notify();
        }
    }
}

impl Drop for BoardViewport {
    fn drop(&mut self) {
        self.search_cancel.cancel();
        self.hover_cancel.cancel();
    }
}

impl Render for BoardViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = i18n::current(cx);
        let mut seen = BTreeSet::new();
        let mut layers: Vec<_> = self
            .scene
            .layers
            .iter()
            .map(|layer| (layer.id, layer.display_name(locale)))
            .chain(
                self.scene
                    .special_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale))),
            )
            .chain(
                self.scene
                    .drawing_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale))),
            )
            .filter(|(id, _)| seen.insert(*id))
            .collect();
        let rendered_layers = self
            .tracks
            .batches
            .iter()
            .filter(|batch| !batch.outline)
            .map(|batch| batch.layer)
            .chain(self.copper.batches.iter().map(|batch| batch.layer))
            .chain(self.drawings.batches.iter().map(|batch| batch.layer))
            .chain(
                self.texts
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
            .chain(self.pads.batches.iter().map(|batch| batch.layer))
            .chain(
                self.pads
                    .custom_mesh
                    .iter()
                    .flat_map(|mesh| mesh.batches.iter().map(|batch| batch.layer)),
            );
        for id in rendered_layers {
            if seen.insert(id) {
                layers.push((
                    id,
                    Message::new(Key::DefaultLayerName)
                        .arg("index", u64::from(id.0) + 1)
                        .display(locale),
                ));
            }
        }
        let layer_order = Arc::new(
            self.display
                .ordered_layers(layers.iter().map(|(id, _)| *id)),
        );
        let ranks: BTreeMap<_, _> = layer_order
            .iter()
            .enumerate()
            .map(|(rank, id)| (*id, rank))
            .collect();
        layers.sort_by_key(|(id, _)| ranks[id]);
        let list_order = Arc::clone(&layer_order);
        let layer_rows = uniform_list(
            "board-layers",
            layers.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let (id, label) = &layers[index];
                        let id = *id;
                        let bottom_order = Arc::clone(&list_order);
                        let top_order = Arc::clone(&list_order);
                        crate::panels::layers::row(
                            locale,
                            crate::panels::layers::LayerRow {
                                id,
                                label: label.clone(),
                                visible: this.display.layer_visible(id),
                                at_bottom: list_order.first() == Some(&id),
                                at_top: list_order.last() == Some(&id),
                            },
                            crate::panels::layers::LayerCommands {
                                visibility: Box::new(cx.listener(
                                    move |this, visible: &bool, _, cx| {
                                        let display = Arc::make_mut(&mut this.display);
                                        let changed = if *visible {
                                            display.hidden_layers.remove(&id)
                                        } else {
                                            display.hidden_layers.insert(id)
                                        };
                                        if changed {
                                            this.invalidate_hover();
                                            cx.notify();
                                        }
                                    },
                                )),
                                to_bottom: Box::new(cx.listener(move |this, _, _, cx| {
                                    if Arc::make_mut(&mut this.display).move_layer_to_edge(
                                        id,
                                        false,
                                        &bottom_order,
                                    ) {
                                        this.layer_scroll
                                            .scroll_to_item(0, ScrollStrategy::Nearest);
                                        this.invalidate_hover();
                                        cx.notify();
                                    }
                                })),
                                to_top: Box::new(cx.listener(move |this, _, _, cx| {
                                    if Arc::make_mut(&mut this.display)
                                        .move_layer_to_edge(id, true, &top_order)
                                    {
                                        this.layer_scroll.scroll_to_item(
                                            top_order.len().saturating_sub(1),
                                            ScrollStrategy::Nearest,
                                        );
                                        this.invalidate_hover();
                                        cx.notify();
                                    }
                                })),
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.layer_scroll)
        .flex_1()
        .min_h_0();
        let layer_panel =
            div()
                .w_64()
                .flex_shrink_0()
                .min_h_0()
                .flex()
                .flex_col()
                .border_r_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .child(text(locale, Key::SearchBoard)),
                )
                .child(div().px_3().pb_2().child(
                    Input::new(&self.search_input).aria_label(text(locale, Key::SearchBoard)),
                ))
                .child(crate::panels::search::results(
                    locale,
                    &self.search_results,
                    self.selected_target,
                    |target| {
                        Box::new(cx.listener(move |this, _, window, cx| {
                            this.locate_search(target, window, cx)
                        }))
                    },
                ))
                .when(self.locate_pending, |panel| {
                    panel.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_sm()
                            .child(text(locale, self.pending_message)),
                    )
                })
                .when_some(self.locate_details.as_ref(), |panel, details| {
                    let diagnostic = match details {
                        pomelo_core::geometry::PathError::Cancelled => Message::new(Key::Cancelled),
                        pomelo_core::geometry::PathError::Invalid(id) => {
                            Message::new(if self.pending_message == Key::Selecting {
                                Key::PickInvalid
                            } else if self.pending_message == Key::LoadingMembers {
                                Key::MembersInvalid
                            } else {
                                Key::SearchLocateInvalid
                            })
                            .arg("object", id.0)
                        }
                        pomelo_core::geometry::PathError::PointLimit { actual, limit } => {
                            Message::new(Key::GeometryLimit)
                                .arg("actual", *actual)
                                .arg("limit", *limit)
                        }
                    };
                    panel.child(
                        div()
                            .px_3()
                            .text_sm()
                            .child(text(locale, Key::TechnicalDetails))
                            .child(diagnostic.display(locale)),
                    )
                })
                .when_some(self.search_notice.as_ref(), |panel, message| {
                    panel.child(div().px_3().py_1().text_sm().child(message.display(locale)))
                })
                .when(self.search_pending, |panel| {
                    panel.child(div().px_3().text_sm().child(text(locale, Key::Searching)))
                })
                .when(
                    !self.search_pending
                        && !self.search_query.trim().is_empty()
                        && self.search_results.is_empty(),
                    |panel| {
                        panel.child(
                            div()
                                .px_3()
                                .text_sm()
                                .child(text(locale, Key::NoSearchResults)),
                        )
                    },
                )
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .child(text(locale, Key::Layers)),
                )
                .child(crate::panels::picking::controls(
                    locale,
                    self.pick_filter,
                    self.selection_mode,
                    cx,
                    |category| {
                        Box::new(cx.listener(move |this, enabled: &bool, _, cx| {
                            if this.pick_filter.contains(category) == *enabled {
                                return;
                            }
                            this.pick_filter.set(category, *enabled);
                            this.hover_cancel.cancel();
                            this.hovered = None;
                            this.hover_notice = None;
                            this.last_pick = None;
                            this.candidate_position = None;
                            this.search_cancel.cancel();
                            this.search_pending = false;
                            this.locate_pending = false;
                            this.search_notice = None;
                            this.locate_details = None;
                            cx.notify();
                        }))
                    },
                ))
                .children(crate::panels::display::controls(
                    locale,
                    &self.display,
                    crate::panels::display::DisplayCommands {
                        drills: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                            if this.display.show_drills != *visible {
                                Arc::make_mut(&mut this.display).show_drills = *visible;
                                cx.notify();
                            }
                        })),
                        copper: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                            Arc::make_mut(&mut this.display).show_copper = *visible;
                            this.invalidate_hover();
                            cx.notify();
                        })),
                        texts: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                            Arc::make_mut(&mut this.display).show_texts = *visible;
                            cx.notify();
                        })),
                        drawings: Box::new(cx.listener(|this, visible: &bool, _, cx| {
                            Arc::make_mut(&mut this.display).show_drawings = *visible;
                            cx.notify();
                        })),
                        decrease_opacity: Box::new(cx.listener(|this, _, _, cx| {
                            Arc::make_mut(&mut this.display).adjust_copper_opacity(-5);
                            cx.notify();
                        })),
                        increase_opacity: Box::new(cx.listener(|this, _, _, cx| {
                            Arc::make_mut(&mut this.display).adjust_copper_opacity(5);
                            cx.notify();
                        })),
                        reset_order: Box::new(cx.listener(|this, _, _, cx| {
                            Arc::make_mut(&mut this.display).layer_order.clear();
                            this.layer_scroll.scroll_to_item(0, ScrollStrategy::Nearest);
                            this.invalidate_hover();
                            cx.notify();
                        })),
                    },
                ))
                .child(
                    div()
                        .id("layer-list")
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .child(layer_rows)
                        .vertical_scrollbar(&self.layer_scroll),
                );

        let inspector = div()
            .w_64()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(div().px_3().py_2().child(text(locale, Key::Inspector)))
            .child(
                div()
                    .id("inspector-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .text_sm()
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(locale, Key::PickCycleHint)),
                            )
                            .when_some(
                                self.candidate_position.filter(|_| {
                                    self.last_pick.as_ref().is_some_and(|last| {
                                        last.matches_view(
                                            self.navigation.camera(),
                                            self.navigation.size(),
                                            &self.display,
                                            self.selection_mode,
                                        )
                                    })
                                }),
                                |body, (current, total)| {
                                    body.child(
                                        Message::new(Key::PickCandidatePosition)
                                            .arg("current", current)
                                            .arg("total", total)
                                            .display(locale),
                                    )
                                },
                            )
                            .when_some(self.selected_target, |body, target| {
                                let (kind, id) = selection_identity(target);
                                body.children(self.selected_source_labels.iter().map(|message| {
                                    div().child(crate::inspector::display_source_field(
                                        message,
                                        locale,
                                        self.display.length_unit,
                                    ))
                                }))
                                .child(
                                    Message::new(Key::SelectionIdentity)
                                        .arg("kind", text(locale, kind))
                                        .arg("id", id)
                                        .display(locale),
                                )
                            })
                            .when_some(self.selected_related_net, |body, net| {
                                body.child(
                                    Button::new("inspect-related-net")
                                        .ghost()
                                        .label(
                                            Message::new(Key::InspectRelatedNet)
                                                .arg("id", net.0)
                                                .display(locale),
                                        )
                                        .disabled(self.locate_pending)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.locate_search(
                                                pomelo_core::search::SearchTarget::Net(net),
                                                window,
                                                cx,
                                            )
                                        })),
                                )
                            })
                            .when_some(self.selected_related_component, |body, component| {
                                body.child(
                                    Button::new("inspect-related-component")
                                        .ghost()
                                        .label(
                                            Message::new(Key::InspectRelatedComponent)
                                                .arg("id", component.0)
                                                .display(locale),
                                        )
                                        .disabled(self.locate_pending)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.locate_search(
                                                pomelo_core::search::SearchTarget::Component(
                                                    component,
                                                ),
                                                window,
                                                cx,
                                            )
                                        })),
                                )
                            })
                            .when_some(self.selected_members.as_ref(), |body, members| {
                                body.child(
                                    Message::new(Key::SourceMembersShown)
                                        .arg("shown", members.objects.len())
                                        .arg("total", members.total)
                                        .display(locale),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_1()
                                        .child(
                                            Message::new(Key::MembersPage)
                                                .arg("page", self.members_offset / 256 + 1)
                                                .arg("pages", members.total.div_ceil(256).max(1))
                                                .display(locale),
                                        )
                                        .child(
                                            Button::new("members-previous")
                                                .ghost()
                                                .label(text(locale, Key::MembersPrevious))
                                                .disabled(
                                                    self.locate_pending || self.members_offset == 0,
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.page_members(
                                                        this.members_offset.saturating_sub(256),
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            Button::new("members-next")
                                                .ghost()
                                                .label(text(locale, Key::MembersNext))
                                                .disabled(
                                                    self.locate_pending
                                                        || self.members_offset.saturating_add(256)
                                                            >= members.total,
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.page_members(
                                                        this.members_offset.saturating_add(256),
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("selection-source-members")
                                        .max_h_48()
                                        .min_h_0()
                                        .overflow_y_scrollbar()
                                        // A published page starts at its first row. Failed or
                                        // cancelled requests retain the current scroll state.
                                        .id(("source-members-page", self.members_page_revision))
                                        .children(members.objects.iter().map(|&object| {
                                            let (kind, id) = selection_identity(
                                                pomelo_core::selection::SelectionTarget::Object(
                                                    object,
                                                ),
                                            );
                                            let target =
                                                pomelo_core::selection::SelectionTarget::Object(
                                                    object,
                                                );
                                            let category =
                                                pomelo_core::picking::PickCategory::from(object)
                                                    as u8;
                                            Button::new((
                                                "source-member",
                                                (u64::from(category) << 32) | u64::from(id),
                                            ))
                                            .ghost()
                                            .selected(self.selected_target == Some(target))
                                            .disabled(self.locate_pending)
                                            .label(
                                                Message::new(Key::LocateMember)
                                                    .arg("kind", text(locale, kind))
                                                    .arg("id", id)
                                                    .display(locale),
                                            )
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.locate_target(target, window, cx)
                                                }),
                                            )
                                        })),
                                )
                            })
                            .when(self.selected_summary.is_none(), |body| {
                                body.child(text(locale, Key::InspectorEmpty))
                            })
                            .when_some(self.selected_summary, |body, summary| {
                                let body = body.children(
                                    [
                                        (Key::SelectionSegments, summary.segments),
                                        (Key::SelectionPins, summary.pins),
                                        (Key::SelectionVias, summary.vias),
                                        (Key::SelectionZones, summary.zones),
                                    ]
                                    .into_iter()
                                    .map(|(key, count)| {
                                        div().child(
                                            Message::new(key).arg("count", count).display(locale),
                                        )
                                    }),
                                );
                                body.when(
                                    matches!(
                                        self.selected_target,
                                        Some(
                                            pomelo_core::selection::SelectionTarget::Net(_)
                                                | pomelo_core::selection::SelectionTarget::Track(_)
                                                | pomelo_core::selection::SelectionTarget::Object(
                                                    pomelo_core::selection::SelectedObject::Segment(
                                                        _
                                                    )
                                                )
                                        )
                                    ),
                                    |body| {
                                        body.child(
                                            div().child(
                                                Message::new(match self.display.length_unit {
                                                    pomelo_core::units::LengthUnit::Millimeters => {
                                                        Key::SelectionLength
                                                    }
                                                    pomelo_core::units::LengthUnit::Mils => {
                                                        Key::SelectionLengthMils
                                                    }
                                                })
                                                .arg(
                                                    "length",
                                                    format!(
                                                        "{:.6}",
                                                        self.display.length_unit.from_millimeters(
                                                            summary.centerline_length_mm
                                                        )
                                                    ),
                                                )
                                                .display(locale),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(text(locale, Key::SelectionLengthScope)),
                                        )
                                    },
                                )
                            }),
                    ),
            );
        let theme = cx.theme();
        let pointer_coordinates =
            self.pointer_position
                .zip(self.bounds)
                .and_then(|(position, bounds)| {
                    let local = position - bounds.origin;
                    let size = self.navigation.size();
                    let point =
                        pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y));
                    if point.x < 0.0
                        || point.y < 0.0
                        || point.x > size.x
                        || point.y > size.y
                        || !self.navigation.camera().is_renderable()
                    {
                        return None;
                    }
                    let board = self
                        .navigation
                        .camera()
                        .view_to_board(point, size.x, size.y);
                    (board.x.is_finite() && board.y.is_finite()).then(|| {
                        crate::inspector::display_source_field(
                            &Message::new(Key::SourcePosition)
                                .arg("x", board.x.to_string())
                                .arg("y", board.y.to_string()),
                            locale,
                            self.display.length_unit,
                        )
                    })
                });
        let scale_bar = self.navigation.camera().scale_bar_in_unit(
            (self.navigation.size().x - 32.0).min(120.0),
            self.display.length_unit,
        );
        let fallback: Rgba = theme.foreground.into();
        let stats = self.telemetry.snapshot();
        let copper_stats = self.copper_telemetry.snapshot();
        let pad_stats = self.pad_telemetry.snapshot();
        let custom_pad_stats = self.custom_pad_telemetry.snapshot();
        let drill_stats = self.drill_telemetry.snapshot();
        let drawing_stats = self.drawing_telemetry.snapshot();
        let text_stats = self.text_telemetry.snapshot();
        let custom_bytes = self
            .pads
            .custom_mesh
            .as_ref()
            .map_or(0, |mesh| mesh.upload_bytes());
        let renderer = self.renderer.as_ref().ok().cloned();
        let status = renderer.as_ref().map(NativeGpuHandle::status);
        let failure = self
            .renderer
            .as_ref()
            .err()
            .cloned()
            .or_else(|| self.paint_error.clone())
            .or_else(|| status.as_ref().and_then(|status| status.last_error.clone()));
        let ready = stats.uploaded_instances == self.tracks.instances.len() as u64
            && copper_stats.uploaded_bytes == self.copper.upload_bytes() as u64
            && pad_stats.uploaded_instances == self.pads.analytic.len() as u64
            && custom_pad_stats.uploaded_bytes == custom_bytes as u64
            && drill_stats.uploaded_instances == self.drills.analytic.len() as u64
            && drawing_stats.uploaded_instances == self.drawings.instances.len() as u64
            && text_stats.uploaded_instances
                == self
                    .texts
                    .as_ref()
                    .map_or(0, |source| source.instances.len()) as u64;
        if failure.is_none()
            && (!ready || status.as_ref().is_some_and(|status| status.submitted == 0))
        {
            window.request_animation_frame();
        }
        if let Some(path) = std::env::var_os("POMELO_BOARD_GPU_TRACE") {
            let specs = window.gpu_specs();
            let report = serde_json::json!({
                "backend": "GPUI_D3D11", "shader_owner": "pomelo-render", "scope": "traces_outline_copper_pads_drills_dimension_strokes_texts",
                "cpu_pixel_readback": false, "expected_instances": self.tracks.instances.len(),
                "ready": ready, "status": {
                    "submitted": status.as_ref().map(|status| status.submitted),
                    "presented": status.as_ref().map(|status| status.presented),
                    "last_error": failure,
                },
                "statistics": stats,
                "copper_statistics": copper_stats,
                "pad_statistics": pad_stats,
                "custom_pad_statistics": custom_pad_stats,
                "drill_statistics": drill_stats,
                "drawing_statistics": drawing_stats,
                "expected_drawing_instances": self.drawings.instances.len(),
                "text_statistics": text_stats,
                "expected_text_instances": self.texts.as_ref().map_or(0, |source| source.instances.len()),
                "expected_drills": self.drills.analytic.len(),
                "adapter": specs.as_ref().map(|specs| &specs.device_name),
                "software_emulated": specs.as_ref().map(|specs| specs.is_software_emulated),
                "board_bounds": self.scene.bounds,
                "source_trace_count": self.scene.segments.len(),
                "source_outline_count": self.scene.outline.len(),
                "copper_prepared": {
                    "zones": self.copper.batches.len(),
                    "vertices": self.copper.vertices.len(),
                    "indices": self.copper.indices.len(),
                    "gpu_drawn": copper_stats.draw_calls > 0,
                },
                "pads_prepared": {
                    "analytic": self.pads.analytic.len(),
                    "custom": self.pads.custom.len(),
                    "gpu_drawn": pad_stats.draw_calls > 0,
                    "custom_gpu_drawn": custom_pad_stats.draw_calls > 0,
                    "custom_vertices": self.pads.custom_mesh.as_ref().map(|mesh| mesh.vertices.len()),
                    "custom_indices": self.pads.custom_mesh.as_ref().map(|mesh| mesh.indices.len()),
                },
                "selected_target": self.selected_target.map(|target| format!("{target:?}")),
                "selection_mode": format!("{:?}", self.selection_mode),
                "hovered_object": self.hovered.as_ref().filter(|(_, context)| context.matches_view(
                    self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                )).map(|(object, _)| format!("{object:?}")),
                "pick_filter": {
                    "segment": self.pick_filter.contains(pomelo_core::picking::PickCategory::Segment),
                    "pin": self.pick_filter.contains(pomelo_core::picking::PickCategory::Pin),
                    "via": self.pick_filter.contains(pomelo_core::picking::PickCategory::Via),
                    "zone": self.pick_filter.contains(pomelo_core::picking::PickCategory::Zone),
                },
                "selection_query": {
                    "search_pending": self.search_pending,
                    "selection_pending": self.locate_pending,
                    "candidate_position": self.last_pick.as_ref().filter(|context| context.matches_view(
                        self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                    )).and(self.candidate_position),
                },
                "members": self.selected_members.as_ref().map(|members| serde_json::json!({
                    "offset": self.members_offset,
                    "page_size": 256,
                    "total": members.total,
                    "objects": members.objects.iter().map(|object| {
                        use pomelo_core::selection::SelectedObject;
                        let (category, id) = match *object {
                            SelectedObject::Segment(id) => ("segment", id),
                            SelectedObject::Pin(id) => ("pin", id),
                            SelectedObject::Via(id) => ("via", id),
                            SelectedObject::Zone(id) => ("zone", id),
                        };
                        serde_json::json!({"category":category,"source_id":id.0})
                    }).collect::<Vec<_>>(),
                })),
                "search_entries": self.search.entries().len(),
                "pick_index_segments": self.picking.segment_count(),
                "display": {
                    "hidden_layers": self.display.hidden_layers.iter().map(|layer| layer.0).collect::<Vec<_>>(),
                    "show_drills": self.display.show_drills,
                    "show_copper": self.display.show_copper,
                    "show_texts": self.display.show_texts,
                    "show_drawings": self.display.show_drawings,
                    "layer_order": &*layer_order,
                    "copper_opacity": self.display.copper_opacity,
                },
                "camera": {
                    "center": self.navigation.camera().center,
                    "logical_pixels_per_mm": self.navigation.camera().pixels_per_mm,
                    "flipped": self.navigation.camera().flipped,
                    "logical_viewport_size": self.navigation.size(),
                },
            });
            if let Err(error) = std::fs::write(path, report.to_string()) {
                eprintln!("{}: {error}", text(locale, Key::FileIoFailed));
            }
        }
        let mut frame = TraceFrame {
            hovered_object: self
                .hovered
                .as_ref()
                .filter(|(_, context)| {
                    context.matches_view(
                        self.navigation.camera(),
                        self.navigation.size(),
                        &self.display,
                        self.selection_mode,
                    )
                })
                .map(|(object, _)| {
                    let color: Rgba = theme.ring.into();
                    (*object, [color.r, color.g, color.b, color.a])
                }),
            highlighted_related_objects: (!self.selected_bonds.is_empty()).then(|| {
                let color: Rgba = theme.primary.into();
                (
                    Arc::clone(&self.selected_bonds),
                    [color.r, color.g, color.b, color.a],
                )
            }),
            highlighted_trace: self.selected_target.and_then(|target| {
                use pomelo_core::selection::{SelectedObject, SelectionTarget};
                let selection = match target {
                    SelectionTarget::Object(SelectedObject::Segment(id)) => {
                        pomelo_render::backend::d3d11::TraceSelection::Segment(id)
                    }
                    SelectionTarget::Track(id) => {
                        pomelo_render::backend::d3d11::TraceSelection::Track(id)
                    }
                    _ => return None,
                };
                let color: Rgba = theme.primary.into();
                Some((selection, [color.r, color.g, color.b, color.a]))
            }),
            tracks: Arc::clone(&self.tracks),
            bounds: self.scene.bounds,
            camera: None,
            scale_factor: window.scale_factor(),
            colors: Arc::clone(&self.colors),
            fallback_color: [fallback.r, fallback.g, fallback.b, fallback.a],
            highlighted_object: self.selected_target.and_then(|target| {
                if let pomelo_core::selection::SelectionTarget::Object(object) = target {
                    let color: Rgba = theme.primary.into();
                    Some((object, [color.r, color.g, color.b, color.a]))
                } else {
                    None
                }
            }),
            highlighted_objects: if matches!(
                self.selected_target,
                Some(
                    pomelo_core::selection::SelectionTarget::Component(_)
                        | pomelo_core::selection::SelectionTarget::Object(
                            pomelo_core::selection::SelectedObject::Pin(_)
                        )
                )
            ) {
                let color: Rgba = theme.primary.into();
                Some((
                    Arc::clone(&self.selected_pins),
                    [color.r, color.g, color.b, color.a],
                ))
            } else {
                None
            },
            highlighted_net: self.selected_target.and_then(|target| match target {
                pomelo_core::selection::SelectionTarget::Net(net) => {
                    let color: Rgba = theme.primary.into();
                    Some((net, [color.r, color.g, color.b, color.a]))
                }
                _ => None,
            }),
        };
        let weak = cx.entity().downgrade();
        let layout_view = weak.clone();
        let telemetry = Arc::clone(&self.telemetry);
        let copper_telemetry = Arc::clone(&self.copper_telemetry);
        let pad_telemetry = Arc::clone(&self.pad_telemetry);
        let custom_pad_telemetry = Arc::clone(&self.custom_pad_telemetry);
        let drill_telemetry = Arc::clone(&self.drill_telemetry);
        let drawing_telemetry = Arc::clone(&self.drawing_telemetry);
        let text_telemetry = Arc::clone(&self.text_telemetry);
        let texts = self.texts.clone();
        let drawings = Arc::clone(&self.drawings);
        let drills = Arc::clone(&self.drills);
        let display = Arc::clone(&self.display);
        let drill_color: Rgba = theme.muted_foreground.into();
        let pads = Arc::clone(&self.pads);
        let copper = Arc::clone(&self.copper);
        let copper_opacity = self.display.copper_opacity;
        let viewport = canvas(
            move |bounds, _, cx| {
                let mut resized = false;
                frame.camera = layout_view
                    .update(cx, |this, _| {
                        let previous_size = this.navigation.size();
                        this.bounds = Some(bounds);
                        this.navigation.resize(
                            this.scene.bounds,
                            f64::from(f32::from(bounds.size.width)),
                            f64::from(f32::from(bounds.size.height)),
                        );
                        resized = this.navigation.size() != previous_size;
                        if resized {
                            this.invalidate_hover();
                        }
                        this.navigation.camera()
                    })
                    .ok();
                if resized {
                    frame.hovered_object = None;
                }
                Arc::new(BoardFrame {
                    drawings: Some(drawings),
                    texts,
                    traces: frame,
                    display,
                    pads: Some(pads),
                    drills: Some(drills),
                    drill_color: [drill_color.r, drill_color.g, drill_color.b, drill_color.a],
                    copper,
                    copper_opacity,
                    layer_order,
                })
            },
            move |bounds, frame, window, cx| {
                if let Some(renderer) = &renderer
                    && let Err(error) = window.paint_gpu(bounds, renderer, frame)
                {
                    let details = format!("{error:#}");
                    let _ = weak.update(cx, |this, cx| {
                        this.paint_error = Some(details);
                        cx.notify();
                    });
                }
                if let Some(renderer) = renderer {
                    let weak = weak.clone();
                    let before = renderer.status();
                    let before_uploaded = telemetry.snapshot().uploaded_instances;
                    let before_copper = copper_telemetry.snapshot().uploaded_bytes;
                    let before_pads = pad_telemetry.snapshot().uploaded_instances;
                    let before_custom = custom_pad_telemetry.snapshot().uploaded_bytes;
                    let before_drills = drill_telemetry.snapshot().uploaded_instances;
                    let before_drawings = drawing_telemetry.snapshot().uploaded_instances;
                    let before_texts = text_telemetry.snapshot().uploaded_instances;
                    window.on_next_frame(move |_, cx| {
                        let after = renderer.status();
                        let uploaded = telemetry.snapshot().uploaded_instances;
                        if before.last_error != after.last_error
                            || before.resets != after.resets
                            || before_uploaded != uploaded
                            || before_copper != copper_telemetry.snapshot().uploaded_bytes
                            || before_pads != pad_telemetry.snapshot().uploaded_instances
                            || before_custom != custom_pad_telemetry.snapshot().uploaded_bytes
                            || before_drills != drill_telemetry.snapshot().uploaded_instances
                            || before_drawings != drawing_telemetry.snapshot().uploaded_instances
                            || before_texts != text_telemetry.snapshot().uploaded_instances
                            || (before.presented == 0 && after.presented > 0)
                        {
                            let _ = weak.update(cx, |_, cx| cx.notify());
                        }
                    });
                }
            },
        )
        .size_full();
        let mut view = div()
            .id("board-viewport")
            .key_context("BoardWorkspace")
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::leave_search))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::next_candidate))
            .on_action(cx.listener(Self::fit_board))
            .on_action(cx.listener(Self::flip_board))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom(1.2, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom(1.0 / 1.2, cx)))
            .on_action(cx.listener(|this, _: &PanLeft, _, cx| this.pan(1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanRight, _, cx| this.pan(-1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &PanUp, _, cx| this.pan(0.0, 1.0, cx)))
            .on_action(cx.listener(|this, _: &PanDown, _, cx| this.pan(0.0, -1.0, cx)))
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .children(
                        [
                            (
                                pomelo_core::interaction::SelectionMode::Object,
                                Key::ModeObject,
                                0u32,
                            ),
                            (
                                pomelo_core::interaction::SelectionMode::Track,
                                Key::ModeTrack,
                                1u32,
                            ),
                            (
                                pomelo_core::interaction::SelectionMode::Net,
                                Key::ModeNet,
                                2u32,
                            ),
                            (
                                pomelo_core::interaction::SelectionMode::Component,
                                Key::ModeComponent,
                                3u32,
                            ),
                        ]
                        .into_iter()
                        .map(|(mode, key, id)| {
                            Button::new(("selection-mode", id))
                                .ghost()
                                .label(text(locale, key))
                                .selected(self.selection_mode == mode)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.clear_selection(&ClearSelection, window, cx);
                                    this.selection_mode = mode;
                                    this.invalidate_hover();
                                    window.focus(&this.focus, cx);
                                }))
                        }),
                    )
                    .children(
                        [
                            (
                                pomelo_core::units::LengthUnit::Millimeters,
                                Key::UnitMillimeters,
                                0u32,
                            ),
                            (pomelo_core::units::LengthUnit::Mils, Key::UnitMils, 1u32),
                        ]
                        .into_iter()
                        .map(|(unit, key, id)| {
                            Button::new(("length-unit", id))
                                .ghost()
                                .label(text(locale, key))
                                .selected(self.display.length_unit == unit)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    Arc::make_mut(&mut this.display).length_unit = unit;
                                    cx.notify();
                                }))
                        }),
                    )
                    .child(
                        Button::new("clear-selection")
                            .ghost()
                            .label(text(locale, Key::ClearSelection))
                            .disabled(self.selected_target.is_none() && !self.locate_pending)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_selection(&ClearSelection, window, cx)
                            })),
                    )
                    .child(
                        Button::new("fit-board")
                            .ghost()
                            .label(text(locale, Key::FitBoard))
                            .disabled(self.bounds.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.fit_board(&FitBoard, window, cx);
                                window.focus(&this.focus, cx);
                            })),
                    )
                    .child(
                        Button::new("zoom-in")
                            .ghost()
                            .label(text(locale, Key::ZoomIn))
                            .disabled(self.bounds.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.zoom(1.2, cx);
                                window.focus(&this.focus, cx);
                            })),
                    )
                    .child(
                        Button::new("zoom-out")
                            .ghost()
                            .label(text(locale, Key::ZoomOut))
                            .disabled(self.bounds.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.zoom(1.0 / 1.2, cx);
                                window.focus(&this.focus, cx);
                            })),
                    )
                    .child(
                        Button::new("flip-board")
                            .ghost()
                            .label(text(locale, Key::FlipBoard))
                            .selected(self.navigation.camera().flipped)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.flip_board(&FlipBoard, window, cx);
                                window.focus(&this.focus, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(layer_panel)
                    .child(
                        div()
                            .id("board-canvas")
                            .relative()
                            .key_context("BoardViewport")
                            .track_focus(&self.focus)
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_hidden()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .focus_visible(|style| style.border_color(theme.ring))
                            .cursor(if self.drag_position.is_some() {
                                CursorStyle::ClosedHand
                            } else {
                                CursorStyle::Arrow
                            })
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::pick_trace))
                            .on_mouse_down(MouseButton::Middle, cx.listener(Self::start_pan))
                            .on_mouse_move(cx.listener(Self::move_pan))
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if !*hovered {
                                    this.pointer_position = None;
                                    this.hover_cancel.cancel();
                                    this.hovered = None;
                                    this.hover_notice = None;
                                    cx.notify();
                                }
                            }))
                            .on_mouse_up(MouseButton::Middle, cx.listener(Self::stop_pan))
                            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::stop_pan))
                            .on_scroll_wheel(cx.listener(Self::scroll))
                            .child(viewport)
                            .when_some(scale_bar, |canvas, scale| {
                                canvas.child(
                                    div()
                                        .absolute()
                                        .left_3()
                                        .bottom_3()
                                        .text_sm()
                                        .text_color(theme.foreground)
                                        .child(
                                            Message::new(match self.display.length_unit {
                                                pomelo_core::units::LengthUnit::Millimeters => {
                                                    Key::ScaleDistance
                                                }
                                                pomelo_core::units::LengthUnit::Mils => {
                                                    Key::ScaleDistanceMils
                                                }
                                            })
                                            .arg(
                                                "distance",
                                                self.display
                                                    .length_unit
                                                    .from_millimeters(scale.millimeters)
                                                    .to_string(),
                                            )
                                            .display(locale),
                                        )
                                        .child(
                                            div()
                                                // This width represents board geometry projected
                                                // into logical screen pixels, not UI spacing.
                                                .w(px(scale.pixels as f32))
                                                .h_2()
                                                .border_b_1()
                                                .border_l_1()
                                                .border_r_1()
                                                .border_color(theme.foreground),
                                        ),
                                )
                            }),
                    )
                    .child(inspector),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(locale, Key::NavigationHint))
                    .when_some(pointer_coordinates, |footer, coordinates| {
                        footer
                            .flex()
                            .gap_3()
                            .items_center()
                            .child(div().child(coordinates))
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(locale, Key::TraceViewportScope)),
            );
        let hover_text: SharedString = if let Some((object, context)) = &self.hovered
            && context.matches_view(
                self.navigation.camera(),
                self.navigation.size(),
                &self.display,
                self.selection_mode,
            ) {
            let (kind, id) =
                selection_identity(pomelo_core::selection::SelectionTarget::Object(*object));
            Message::new(Key::HoverObject)
                .arg("kind", text(locale, kind))
                .arg("id", id)
                .display(locale)
                .into()
        } else if let Some(message) = &self.hover_notice {
            message.display(locale).into()
        } else {
            SharedString::default()
        };
        view = view.child(div().text_sm().min_h_5().child(hover_text));
        if let Some(details) = failure {
            view = view
                .child(
                    div()
                        .text_color(theme.danger)
                        .child(text(locale, Key::GpuFailed)),
                )
                .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                .child(div().text_sm().child(details));
        } else if !ready {
            view = view.child(
                div().text_sm().child(
                    Message::new(Key::TraceUpload)
                        .arg(
                            "completed",
                            stats.uploaded_instances * 128
                                + copper_stats.uploaded_bytes
                                + pad_stats.uploaded_instances * 128
                                + custom_pad_stats.uploaded_bytes
                                + drill_stats.uploaded_instances * 128,
                        )
                        .arg(
                            "total",
                            self.tracks.instances.len() * 128
                                + self.copper.upload_bytes()
                                + self.pads.analytic.len() * 128
                                + custom_bytes
                                + self.drills.analytic.len() * 128,
                        )
                        .display(locale),
                ),
            );
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn snapshots_preserve_pending_restore_but_not_cancelled_previous_generation() {
        use pomelo_core::{model::NetId, selection::SelectionTarget, task::CancellationToken};
        let old = SelectionTarget::Net(NetId(7));
        let new = SelectionTarget::Net(NetId(9));
        let cancellation = CancellationToken::default();
        let pending = (old, cancellation.clone());
        assert_eq!(snapshot_selection(None, Some(&pending)), Some(old));
        assert_eq!(snapshot_selection(Some(new), Some(&pending)), Some(new));
        cancellation.cancel();
        let _next_generation = CancellationToken::default();
        assert_eq!(snapshot_selection(None, Some(&pending)), None);
        assert_eq!(snapshot_selection(Some(new), Some(&pending)), Some(new));
    }
    #[::core::prelude::v1::test]
    fn candidate_context_rejects_view_and_display_changes() {
        use pomelo_core::{
            interaction::{Camera, SelectionMode},
            model::Point as BoardPoint,
        };
        let camera = Camera {
            center: BoardPoint::new(1.0, 2.0),
            pixels_per_mm: 3.0,
            flipped: false,
        };
        let size = BoardPoint::new(800.0, 600.0);
        let display = Arc::new(pomelo_core::display::BoardDisplay::default());
        let context = PickContext {
            local: BoardPoint::new(10.0, 20.0),
            camera,
            size,
            display: display.clone(),
            mode: SelectionMode::Net,
        };
        assert!(context.matches_view(camera, size, &display, SelectionMode::Net));
        assert!(!context.matches_view(
            Camera {
                flipped: true,
                ..camera
            },
            size,
            &display,
            SelectionMode::Net
        ));
        assert!(!context.matches_view(
            Camera {
                pixels_per_mm: 6.0,
                ..camera
            },
            size,
            &display,
            SelectionMode::Net
        ));
        assert!(!context.matches_view(
            Camera {
                center: BoardPoint::default(),
                ..camera
            },
            size,
            &display,
            SelectionMode::Net
        ));
        assert!(!context.matches_view(
            camera,
            BoardPoint::new(900.0, 600.0),
            &display,
            SelectionMode::Net
        ));
        assert!(!context.matches_view(camera, size, &display, SelectionMode::Object));
        assert!(!context.matches_view(
            camera,
            size,
            &Arc::new(pomelo_core::display::BoardDisplay::default()),
            SelectionMode::Net
        ));
    }
    #[::core::prelude::v1::test]
    fn candidate_cycle_wraps_and_resets_if_previous_target_is_absent() {
        use pomelo_core::{
            model::{NetId, ObjectId},
            selection::{SelectedObject, SelectionCandidate, SelectionTarget},
        };
        let candidates = [
            SelectionCandidate {
                target: SelectionTarget::Net(NetId(7)),
                object: SelectedObject::Pin(ObjectId(1)),
                distance_mm: 0.0,
            },
            SelectionCandidate {
                target: SelectionTarget::Net(NetId(8)),
                object: SelectedObject::Via(ObjectId(2)),
                distance_mm: 0.1,
            },
        ];
        assert_eq!(cycle_target(&candidates, None), Some(candidates[0].target));
        assert_eq!(
            cycle_target(&candidates, Some(candidates[0].target)),
            Some(candidates[1].target)
        );
        assert_eq!(
            cycle_target(&candidates, Some(candidates[1].target)),
            Some(candidates[0].target)
        );
        assert_eq!(
            cycle_target(&candidates, Some(SelectionTarget::Net(NetId(99)))),
            Some(candidates[0].target)
        );
        assert_eq!(cycle_target(&[], None), None);
        assert_eq!(
            candidate_position(&candidates, candidates[0].target),
            Some((1, 2))
        );
        assert_eq!(
            candidate_position(&candidates, candidates[1].target),
            Some((2, 2))
        );
        assert_eq!(
            candidate_position(&candidates, SelectionTarget::Net(NetId(99))),
            None
        );
        assert_eq!(candidate_position(&[], candidates[0].target), None);
    }
    #[::core::prelude::v1::test]
    fn source_color_is_strict_and_does_not_slice_unicode() {
        assert_eq!(
            source_color("#ff8000"),
            Some([1.0, 128.0 / 255.0, 0.0, 1.0])
        );
        for invalid in ["#中文", "#zz0000", "red", "#ff00", "#ff0000ff", "#+12345"] {
            assert!(source_color(invalid).is_none());
        }
    }
}
