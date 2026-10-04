//! Native desktop board canvas. Business GPU resources stay in pomelo-render.
use crate::panels::focus_scroll::FocusScroll;
use crate::tooltips::ButtonTooltipExt;
mod appearance;
mod curves;
mod opacity;
mod presentation;
mod toolbar;

pub use crate::actions::FocusSearch;
use crate::i18n;
use crate::inspector::{prepare_inspection, selection_identity};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, ResizableState, Sizable, h_resizable, resizable_panel,
    slider::{SliderEvent, SliderState},
};
use gpui_kit::gpui;
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
    backend::native::{BoardFrame, BoardRenderer, CopperTelemetry, TraceFrame, TraceTelemetry},
    tracks::PreparedTracks,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

actions!(
    pomelo,
    [
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
    hover_selection: Option<(
        pomelo_core::selection::SelectionTarget,
        Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
    )>,
    hovered: Option<(pomelo_core::selection::SelectedObject, PickContext)>,
    hover_notice: Option<Message>,
    hover_cancel: pomelo_core::task::CancellationToken,
    hover_task: Option<Task<()>>,
    last_pick: Option<PickContext>,
    candidate_position: Option<(usize, usize)>,
    tracks: Arc<PreparedTracks>,
    zone_outlines: Arc<PreparedTracks>,
    drawings: Arc<PreparedTracks>,
    texts: Option<Arc<pomelo_render::text::msdf::PreparedGlyphs>>,
    source_strokes: Option<Arc<pomelo_render::text::instances::PreparedTextInstances>>,
    label_index: Option<Arc<pomelo_render::text::msdf::LabelIndex>>,
    label_cache: Option<LabelCache>,
    curve_fill: curves::CurveState,
    copper: Arc<pomelo_render::copper::PreparedCopper>,
    pads: Arc<pomelo_render::pads::PreparedPads>,
    drills: Arc<pomelo_render::pads::PreparedPads>,
    scene: Arc<BoardScene>,
    render_diagnostics: Vec<pomelo_core::model::Diagnostic>,
    diagnostics_expanded: bool,
    diagnostics_page: usize,
    search: Arc<pomelo_core::search::SearchIndex>,
    picking: Arc<pomelo_core::picking_index::SegmentIndex>,
    search_input: Entity<InputState>,
    layer_input: Entity<InputState>,
    layer_query: String,
    input_locale: Option<pomelo_core::i18n::Locale>,
    left_panel: usize,
    show_display: bool,
    file_information_expanded: bool,
    pan_tool: bool,
    copper_slider: Entity<SliderState>,
    global_slider: Entity<SliderState>,
    color_pickers: BTreeMap<
        pomelo_core::appearance::ColorTarget,
        Entity<gpui_kit::component::color_picker::ColorPickerState>,
    >,
    panel_sizes: Entity<ResizableState>,
    panel_layout: Entity<crate::workbench::panel_layout::PanelLayout>,
    search_results: Vec<pomelo_core::search::SearchEntry>,
    selected_target: Option<pomelo_core::selection::SelectionTarget>,
    selected_anchor: Option<pomelo_core::picking_index::SelectionAnchor>,
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
    members_scroll: ScrollHandle,
    members_navigation: crate::panels::diagnostics::NavigationFocus,
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
    layer_scroll: ListState,
    layer_list_ids: Vec<pomelo_core::model::LayerId>,
    expanded_layer: Option<pomelo_core::model::LayerId>,
    layer_scroll_target: Option<usize>,
    display: Arc<pomelo_core::display::BoardDisplay>,
    colors: Arc<BTreeMap<pomelo_core::model::LayerId, [f32; 4]>>,
    renderer: Result<GpuPainterHandle, String>,
    telemetry: Arc<TraceTelemetry>,
    copper_telemetry: Arc<CopperTelemetry>,
    pad_telemetry: Arc<TraceTelemetry>,
    custom_pad_telemetry: Arc<CopperTelemetry>,
    custom_outline_telemetry: Arc<TraceTelemetry>,
    drill_telemetry: Arc<TraceTelemetry>,
    drawing_telemetry: Arc<TraceTelemetry>,
    text_telemetry: Arc<TraceTelemetry>,
    source_stroke_telemetry: Arc<TraceTelemetry>,
    zone_outline_telemetry: Arc<TraceTelemetry>,
    label_telemetry: Arc<TraceTelemetry>,
    paint_error: Option<String>,
    navigation: ViewportNavigation,
    bounds: Option<Bounds<Pixels>>,
    drag_position: Option<Point<Pixels>>,
    pointer_position: Option<Point<Pixels>>,
    focus: FocusHandle,
    diagnostics_navigation: crate::panels::diagnostics::NavigationFocus,
    inspector_focus: crate::panels::focus_scroll::InspectorFocus,
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
        let selection = snapshot_selection(self.selected_target, self.restoring_selection.as_ref());
        Some(pomelo_core::view_state::ViewState {
            schema_version: 1,
            source: source.clone(),
            camera: self.navigation.camera(),
            display: (*self.display).clone(),
            selection_mode: self.selection_mode,
            pick_filter: self.pick_filter,
            selection,
            selection_anchor: selection.and(self.selected_anchor),
        })
    }

    pub fn new(
        prepared: &crate::document::PreparedDocument,
        panel_layout: Entity<crate::workbench::panel_layout::PanelLayout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&panel_layout, |_, _, cx| cx.notify()).detach();
        let tracks = Arc::clone(&prepared.tracks);
        let drawings = Arc::clone(&prepared.drawings);
        let texts = prepared.texts.clone();
        let source_strokes = prepared.source_strokes.clone();
        let copper = Arc::clone(&prepared.copper);
        let pads = Arc::clone(&prepared.pads);
        let drills = Arc::clone(&prepared.drills);
        let scene = Arc::clone(&prepared.board.scene);
        let search = Arc::clone(&prepared.search);
        let telemetry = Arc::new(TraceTelemetry::default());
        let copper_telemetry = Arc::new(CopperTelemetry::default());
        let pad_telemetry = Arc::new(TraceTelemetry::default());
        let custom_pad_telemetry = Arc::new(CopperTelemetry::default());
        let custom_outline_telemetry = Arc::new(TraceTelemetry::default());
        let drill_telemetry = Arc::new(TraceTelemetry::default());
        let drawing_telemetry = Arc::new(TraceTelemetry::default());
        let text_telemetry = Arc::new(TraceTelemetry::default());
        let source_stroke_telemetry = Arc::new(TraceTelemetry::default());
        let zone_outline_telemetry = Arc::new(TraceTelemetry::default());
        let label_telemetry = Arc::new(TraceTelemetry::default());
        let renderer = window
            .register_gpu_painter(pomelo_render::backend::native::painter(
                BoardRenderer::new(
                    Arc::clone(&telemetry),
                    Arc::clone(&copper_telemetry),
                    Arc::clone(&pad_telemetry),
                    Arc::clone(&custom_pad_telemetry),
                    Arc::clone(&drill_telemetry),
                    Arc::clone(&drawing_telemetry),
                    Arc::clone(&text_telemetry),
                )
                .with_source_stroke_telemetry(Arc::clone(&source_stroke_telemetry))
                .with_custom_outline_telemetry(Arc::clone(&custom_outline_telemetry))
                .with_zone_outline_telemetry(Arc::clone(&zone_outline_telemetry))
                .with_label_telemetry(Arc::clone(&label_telemetry)),
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
        cx.observe_global::<crate::theme::ThemeState>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<crate::i18n::LanguageState>(|_, cx| cx.notify())
            .detach();
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
                let first = this.search_results.iter().find(|entry| match entry.target {
                    pomelo_core::search::SearchTarget::Net(_) => this.left_panel == 1,
                    pomelo_core::search::SearchTarget::Component(_)
                    | pomelo_core::search::SearchTarget::ComponentGroup(_) => this.left_panel == 2,
                });
                if let Some(entry) = first {
                    this.locate_search(entry.target, window, cx);
                }
            }
        })
        .detach();
        let layer_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&layer_input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.layer_query = input.read(cx).value().to_lowercase();
                this.layer_scroll_target = Some(0);
                cx.notify();
            }
        })
        .detach();
        let opacity = prepared
            .restored_view
            .as_ref()
            .map_or(initial_display.copper_opacity, |state| {
                state.display.copper_opacity
            });
        let copper_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(opacity * 100.0)
        });
        cx.subscribe(&copper_slider, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                Arc::make_mut(&mut this.display).copper_opacity = value.end() / 100.0;
                cx.notify();
            }
        })
        .detach();
        let global_opacity = prepared
            .restored_view
            .as_ref()
            .map_or(initial_display.global_opacity, |state| {
                state.display.global_opacity
            });
        let global_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(global_opacity * 100.0)
        });
        cx.subscribe(&global_slider, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                Arc::make_mut(&mut this.display).global_opacity = value.end() / 100.0;
                cx.notify();
            }
        })
        .detach();
        let mut viewport = Self {
            diagnostics_navigation: crate::panels::diagnostics::NavigationFocus::new(cx),
            inspector_focus: crate::panels::focus_scroll::InspectorFocus::new(cx),
            search_input,
            layer_input,
            layer_query: String::new(),
            input_locale: None,
            left_panel: 0,
            show_display: false,
            file_information_expanded: true,
            pan_tool: false,
            panel_sizes: panel_layout.read(cx).sizes.clone(),
            panel_layout,
            copper_slider,
            global_slider,
            color_pickers: BTreeMap::new(),
            search_results: Vec::new(),
            selected_target: None,
            selected_anchor: None,
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
            hover_selection: None,
            hover_notice: None,
            hover_cancel: pomelo_core::task::CancellationToken::default(),
            hover_task: None,
            pick_filter: pomelo_core::picking::PickFilter::all(),
            members_offset: 0,
            members_page_revision: 0,
            members_scroll: ScrollHandle::new(),
            members_navigation: crate::panels::diagnostics::NavigationFocus::new(cx),
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
            layer_scroll: ListState::new(0, ListAlignment::Top, px(128.0)),
            layer_list_ids: Vec::new(),
            expanded_layer: scene.layers.first().map(|layer| layer.id),
            layer_scroll_target: None,
            tracks,
            zone_outlines: Arc::clone(&prepared.zone_outlines),
            drawings,
            copper,
            pads,
            drills,
            scene,
            render_diagnostics: prepared.render_diagnostics.clone(),
            diagnostics_expanded: false,
            diagnostics_page: 0,
            search,
            picking: Arc::clone(&prepared.picking),
            display: Arc::new(initial_display),
            colors: Arc::new(colors),
            renderer,
            telemetry,
            copper_telemetry,
            pad_telemetry,
            custom_pad_telemetry,
            custom_outline_telemetry,
            drill_telemetry,
            drawing_telemetry,
            text_telemetry,
            source_stroke_telemetry,
            zone_outline_telemetry,
            label_telemetry,
            texts,
            source_strokes,
            label_index: prepared.label_index.clone(),
            label_cache: None,
            curve_fill: curves::CurveState::default(),
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
                viewport.selected_anchor = state.selection_anchor;
                viewport.prepare_anchored_target(
                    target,
                    false,
                    state.selection_anchor,
                    None,
                    window,
                    cx,
                );
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
        if !query.trim().is_empty() && self.left_panel == 0 {
            self.left_panel = 1;
        }
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
        self.prepare_anchored_target(target, move_camera, None, None, window, cx);
    }

    fn prepare_anchored_target(
        &mut self,
        target: pomelo_core::selection::SelectionTarget,
        move_camera: bool,
        preferred: Option<pomelo_core::picking_index::SelectionAnchor>,
        mode: Option<pomelo_core::interaction::SelectionMode>,
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
        if !move_camera && mode.is_none() {
            self.restoring_selection = Some((target, cancel.clone()));
        }
        let picking = Arc::clone(&self.picking);
        let display = Arc::clone(&self.display);
        let worker_display = Arc::clone(&display);
        let worker = cx.background_spawn(async move {
            let target = if let Some(mode) = mode {
                let Some(anchor) = preferred else {
                    return Err(pomelo_core::geometry::PathError::Cancelled);
                };
                let hits = [pomelo_core::picking::ObjectHit {
                    object: anchor.object,
                    distance_mm: 0.0,
                }];
                pomelo_core::selection::resolve_canvas_candidates(
                    &scene,
                    &hits,
                    mode,
                    1,
                    &worker_cancel,
                )?
                .first()
                .map_or(target, |candidate| candidate.target)
            } else {
                target
            };
            let target = target.canonical_reference(&scene, &worker_cancel)?;
            let bounds = if target.exists(&scene, &worker_cancel)? {
                picking.selection_bounds(target, &worker_cancel)?
            } else {
                None
            };
            let summary = target.summarize(&scene, &worker_cancel)?;
            let members = target.members(&scene, 256, &worker_cancel)?;
            let anchor = picking.selection_anchor_with_preferred(
                target,
                &worker_display,
                preferred,
                &worker_cancel,
            )?;
            let inspection = prepare_inspection(target, anchor, &scene, &worker_cancel)?;
            let pins = Arc::new(target.pin_ids(&scene, &worker_cancel)?);
            Ok::<_, pomelo_core::geometry::PathError>((
                bounds, summary, members, inspection, pins, target, anchor,
            ))
        });
        self.search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                this.locate_pending = false;
                this.restoring_selection = None;
                if !Arc::ptr_eq(&display, &this.display) {
                    this.prepare_anchored_target(target, move_camera, preferred, mode, window, cx);
                    return;
                }
                match result {
                    Ok((Some(bounds), summary, members, inspection, pins, target, anchor))
                        if !move_camera || this.navigation.locate(bounds) =>
                    {
                        this.invalidate_hover();
                        this.members_offset = 0;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.members_scroll.set_offset(point(px(0.), px(0.)));
                        this.selected_members = Some(members);
                        this.selected_target = Some(target);
                        this.selected_anchor = anchor;
                        if mode.is_none() && move_camera {
                            use pomelo_core::{
                                interaction::SelectionMode, selection::SelectionTarget,
                            };
                            this.selection_mode = match target {
                                SelectionTarget::Net(_) => SelectionMode::Net,
                                SelectionTarget::Track(_) => SelectionMode::Track,
                                SelectionTarget::Component(_)
                                | SelectionTarget::ComponentGroup(_) => SelectionMode::Component,
                                SelectionTarget::Object(_) => SelectionMode::Object,
                            };
                        }
                        this.selected_related_net = inspection.net;
                        this.selected_related_component = inspection.component;
                        this.selected_source_labels = inspection.labels;
                        this.selected_bonds = inspection.bonds;
                        this.selected_summary = Some(summary);
                        this.selected_pins = pins;
                        window.focus(&this.focus, cx);
                    }
                    Ok((None, _, _, _, _, _, _)) => {
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
                    Ok((Some(_), _, _, _, _, _, _)) => {
                        this.search_notice = Some(Message::new(Key::SearchLocateFailed))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(crate) fn focus_search(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.panel_layout.read(cx).preferences.left_collapsed {
            self.panel_layout.update(cx, |layout, cx| {
                layout.toggle(crate::workbench::panel_layout::Side::Left, cx)
            });
        }
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
        let paging_focus = window.focused(cx).filter(|focus| {
            *focus == self.members_navigation.previous || *focus == self.members_navigation.next
        });
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
            let _ = this.update_in(cx, |this, window, cx| {
                if cancel.is_cancelled() || this.selected_target != Some(target) {
                    return;
                }
                this.locate_pending = false;
                match result {
                    Ok(members) => {
                        this.members_offset = offset;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.members_scroll.set_offset(point(px(0.), px(0.)));
                        if paging_focus
                            .as_ref()
                            .is_some_and(|focus| focus.is_focused(window))
                        {
                            this.members_navigation.focus_boundary(
                                offset / 256,
                                members.total.div_ceil(256).saturating_sub(1),
                                window,
                                cx,
                            );
                        }
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

    fn clear_selection(&mut self, _: &ClearSelection, window: &mut Window, cx: &mut Context<Self>) {
        self.last_pick = None;
        self.candidate_position = None;
        self.search_cancel.cancel();
        self.search_pending = false;
        self.locate_pending = false;
        self.search_notice = None;
        self.locate_details = None;
        self.selected_target = None;
        self.selected_anchor = None;
        self.selected_source_labels.clear();
        self.selected_related_net = None;
        self.selected_related_component = None;
        self.selected_summary = None;
        self.selected_members = None;
        self.selected_pins = Arc::new(BTreeSet::new());
        self.selected_bonds = Arc::new(BTreeSet::new());
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn set_selection_mode(
        &mut self,
        mode: pomelo_core::interaction::SelectionMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_mode == mode {
            return;
        }
        self.selection_mode = mode;
        self.invalidate_hover();
        if let Some(anchor) = self.selected_anchor {
            self.prepare_anchored_target(
                pomelo_core::selection::SelectionTarget::Object(anchor.object),
                false,
                Some(anchor),
                Some(mode),
                window,
                cx,
            );
        } else {
            self.clear_selection(&ClearSelection, window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn pick_trace(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let local = event.position - bounds.origin;
        // A normal click keeps selecting Web's topmost hit. Candidate cycling
        // is an explicit keyboard command, not a side effect of repeated clicks.
        self.last_pick = None;
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
            let hit_entries = index.query_visible_hits(
                query,
                camera.pixels_per_mm,
                filter,
                &worker_display,
                64,
                &worker_cancel,
            )?;
            let hits: Vec<_> = hit_entries.iter().map(|hit| hit.object_hit()).collect();
            let candidates = pomelo_core::selection::resolve_canvas_candidates(
                &scene,
                &hits,
                mode,
                64,
                &worker_cancel,
            )?;
            let Some(target) = cycle_target(&candidates, previous) else {
                return Ok(None);
            };
            let anchor = candidates
                .iter()
                .find(|candidate| candidate.target == target)
                .and_then(|candidate| {
                    hit_entries
                        .iter()
                        .find(|hit| hit.anchor.object == candidate.object)
                })
                .map(|hit| hit.anchor);
            let summary = target.summarize(&scene, &worker_cancel)?;
            let pins = target.pin_ids(&scene, &worker_cancel)?;
            Ok::<_, pomelo_core::geometry::PathError>(Some((
                target,
                summary,
                Arc::new(pins),
                candidate_position(&candidates, target),
                target.members(&scene, 256, &worker_cancel)?,
                prepare_inspection(target, anchor, &scene, &worker_cancel)?,
                anchor,
            )))
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
                    Ok(Some((target, summary, pins, position, members, inspection, anchor))) => {
                        this.members_offset = 0;
                        this.members_page_revision = this.members_page_revision.wrapping_add(1);
                        this.members_scroll.set_offset(point(px(0.), px(0.)));
                        this.selected_members = Some(members);
                        this.candidate_position = position;
                        this.last_pick = Some(pick_context);
                        this.selected_target = Some(target);
                        this.selected_anchor = anchor;
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
                        this.selected_anchor = None;
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

    pub(crate) fn fit_board(&mut self, _: &FitBoard, _: &mut Window, cx: &mut Context<Self>) {
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
        self.hover_selection = None;
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
        let scene = Arc::clone(&self.scene);
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
        let mode = self.selection_mode;
        let delay = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(40));
        let worker = cx.background_spawn(async move {
            delay.await;
            if worker_cancel.is_cancelled() {
                return Err(pomelo_core::geometry::PathError::Cancelled);
            }
            let hits = index.query_visible_objects(
                query,
                camera.pixels_per_mm,
                filter,
                &worker_display,
                1,
                &worker_cancel,
            )?;
            let Some(hit) = hits.first() else {
                return Ok(None);
            };
            let candidates = pomelo_core::selection::resolve_canvas_candidates(
                &scene,
                &hits,
                mode,
                1,
                &worker_cancel,
            )?;
            let Some(candidate) = candidates.first() else {
                return Ok(None);
            };
            let members = candidate.target.component_objects(&scene, &worker_cancel)?;
            Ok(Some((hit.object, candidate.target, Arc::new(members))))
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
                    Ok(hit) => {
                        this.hover_selection = hit
                            .as_ref()
                            .map(|(_, target, members)| (*target, Arc::clone(members)));
                        this.hovered = hit.map(|(object, _, _)| (object, context));
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return,
                    Err(error) => {
                        this.hovered = None;
                        this.hover_selection = None;
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
        if event.pressed_button != Some(MouseButton::Middle)
            && !(self.pan_tool && event.pressed_button == Some(MouseButton::Left))
        {
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

impl BoardViewport {
    fn diagnostics_panel(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let locale = i18n::current(cx);
        let count = self.scene.diagnostics.len() + self.render_diagnostics.len();
        let mut panel = div().flex().flex_col().min_h_0().gap_2().child(
            self.inspector_focus.diagnostics.wrap(
                Button::new("import-diagnostics")
                    .ghost()
                    .selected(self.diagnostics_expanded)
                    .disabled(count == 0)
                    .label(
                        Message::new(Key::ImportDiagnostics)
                            .arg("count", count)
                            .display(locale),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diagnostics_expanded = !this.diagnostics_expanded;
                        cx.notify();
                    })),
            ),
        );
        if !self.diagnostics_expanded || count == 0 {
            return panel.into_any_element();
        }
        let last_page = (count - 1) / crate::document::DIAGNOSTICS_PAGE_SIZE;
        let page = self.diagnostics_page.min(last_page);
        let range = crate::document::diagnostics_range(count, page);
        let rows = crate::panels::diagnostics::page_rows(
            locale,
            page,
            self.scene
                .diagnostics
                .iter()
                .chain(&self.render_diagnostics)
                .skip(range.start)
                .take(range.len()),
            cx,
        );
        panel = panel.child(rows).child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    self.inspector_focus.previous.wrap(
                        crate::panels::diagnostics::page_button(
                            "diagnostics-previous",
                            text(locale, Key::MembersPrevious),
                            &self.diagnostics_navigation.previous,
                            page == 0,
                            window,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.diagnostics_page = page.saturating_sub(1);
                                this.diagnostics_navigation.focus_boundary(
                                    this.diagnostics_page,
                                    last_page,
                                    window,
                                    cx,
                                );
                                cx.notify();
                            },
                        )),
                    ),
                )
                .child(
                    Message::new(Key::DiagnosticPage)
                        .arg("current", page + 1)
                        .arg("total", last_page + 1)
                        .display(locale),
                )
                .child(
                    self.inspector_focus.next.wrap(
                        crate::panels::diagnostics::page_button(
                            "diagnostics-next",
                            text(locale, Key::MembersNext),
                            &self.diagnostics_navigation.next,
                            page == last_page,
                            window,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.diagnostics_page = (page + 1).min(last_page);
                                this.diagnostics_navigation.focus_boundary(
                                    this.diagnostics_page,
                                    last_page,
                                    window,
                                    cx,
                                );
                                cx.notify();
                            },
                        )),
                    ),
                ),
        );
        panel.into_any_element()
    }
}

impl Render for BoardViewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::workbench::panel_layout::Side;
        let locale = i18n::current(cx);
        let opacity_controls: Vec<_> = if self.show_display {
            vec![self.opacity_control(locale, cx)]
        } else {
            Vec::new()
        };
        let appearance_controls = if self.show_display {
            Some(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(
                        [
                            (
                                "canvas-background-color",
                                pomelo_core::appearance::ColorTarget::Background,
                            ),
                            (
                                "drill-display-color",
                                pomelo_core::appearance::ColorTarget::Drill,
                            ),
                        ]
                        .map(|(id, target)| {
                            let control = self.color_control(target, locale, window, cx);
                            FocusScroll::new(id, &self.inspector_focus.scroll, control)
                        }),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let left_tab_labels: Vec<SharedString> =
            [Key::Layers, Key::SearchNets, Key::SearchComponents]
                .map(|key| text(locale, key).into())
                .into();
        let right_tab_labels: Vec<SharedString> = [Key::Inspector, Key::DisplayPanel]
            .map(|key| text(locale, key).into())
            .into();
        let panel_budget = window.viewport_size().width.as_f32()
            - crate::workbench::panel_layout::MIN_CANVAS_WIDTH
            - crate::workbench::panel_layout::PANEL_FRAME_WIDTH;
        let rail_width = (window.rem_size() * 2.5).as_f32();
        let panels = crate::workbench::panel_layout::fit_to_window(
            self.panel_layout.read(cx).snapshot(cx),
            window.viewport_size().width.as_f32(),
            rail_width,
        );
        let left_panel_width = px(panels.left_width);
        let right_panel_width = px(panels.right_width);
        let left_max_width = px((panel_budget
            - if panels.right_collapsed {
                rail_width
            } else {
                panels.right_width
            })
        .clamp(200.0, 440.0));
        let right_max_width = px((panel_budget
            - if panels.left_collapsed {
                rail_width
            } else {
                panels.left_width
            })
        .clamp(240.0, 440.0));
        let canvas_breadcrumb = self
            .selected_source_labels
            .iter()
            .find_map(|message| message.args.get("name"))
            .map_or_else(
                || text(locale, Key::FullBoard),
                |name| {
                    Message::new(Key::BoardSelection)
                        .arg("name", name.to_string())
                        .display(locale)
                },
            );
        let layer_locale_changed = self.input_locale != Some(locale);
        let search_placeholder: SharedString = text(locale, Key::SearchBoard).into();
        if self.input_locale != Some(locale) {
            self.search_input.update(cx, |input, cx| {
                input.set_placeholder(search_placeholder, window, cx)
            });
        }
        let layer_placeholder: SharedString = text(locale, Key::LayerFilter).into();
        if self.input_locale != Some(locale) {
            self.layer_input.update(cx, |input, cx| {
                input.set_placeholder(layer_placeholder, window, cx)
            });
        }
        self.input_locale = Some(locale);
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
            .chain(
                self.source_strokes
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
        let ranks: BTreeMap<_, _> = self
            .display
            .layers_front_to_back(layers.iter().map(|(id, _)| *id))
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, rank))
            .collect();
        layers.sort_by_key(|(id, _)| ranks[id]);
        let list_order = Arc::clone(&layer_order);
        let all_layers: Arc<Vec<_>> = Arc::new(layers.iter().map(|(id, _)| *id).collect());
        let layer_count = layers.len();
        layers.retain(|(_, name)| name.to_lowercase().contains(&self.layer_query));
        let ids: Vec<_> = layers.iter().map(|(id, _)| *id).collect();
        if ids != self.layer_list_ids {
            self.layer_scroll.reset(ids.len());
            self.layer_list_ids = ids;
        } else if layer_locale_changed
            && let Some(index) = self
                .layer_list_ids
                .iter()
                .position(|id| Some(*id) == self.expanded_layer)
        {
            self.layer_scroll.splice(index..index + 1, 1);
        }
        if let Some(index) = self.layer_scroll_target.take()
            && index < layers.len()
        {
            self.layer_scroll.scroll_to_reveal_item(index);
        }
        let layer_rows = list(
            self.layer_scroll.clone(),
            cx.processor(move |this, index: usize, window, cx| {
                let (id, label) = &layers[index];
                let id = *id;
                let bottom_order = Arc::clone(&list_order);
                let top_order = Arc::clone(&list_order);
                let colors = if this.expanded_layer == Some(id) {
                    [
                        pomelo_core::appearance::ColorTarget::Etch(id),
                        pomelo_core::appearance::ColorTarget::Pin(id),
                        pomelo_core::appearance::ColorTarget::Via(id),
                    ]
                    .map(|target| this.color_control(target, locale, window, cx))
                    .into_iter()
                    .collect()
                } else {
                    Vec::new()
                };
                crate::panels::layers::row(
                    locale,
                    crate::panels::layers::LayerRow {
                        id,
                        label: label.clone(),
                        visible: this.display.layer_visible(id),
                        color: this
                            .display
                            .appearance
                            .material(id, pomelo_core::display::DisplayCategory::Trace)
                            .or_else(|| this.colors.get(&id).copied()),
                        colors,
                        function: this
                            .scene
                            .layers
                            .iter()
                            .find(|layer| layer.id == id)
                            .map(|layer| layer.function),
                        at_bottom: list_order.first() == Some(&id),
                        at_top: list_order.last() == Some(&id),
                        expanded: this.expanded_layer == Some(id),
                        primitives: this.display.primitives(id),
                    },
                    crate::panels::layers::LayerCommands {
                        expand: Box::new(cx.listener(move |this, _, _, cx| {
                            let old = this.expanded_layer;
                            this.expanded_layer = if old == Some(id) { None } else { Some(id) };
                            for (index, layer) in this.layer_list_ids.iter().enumerate() {
                                if Some(*layer) == old || *layer == id {
                                    this.layer_scroll.splice(index..index + 1, 1);
                                }
                            }
                            this.layer_scroll.scroll_to_reveal_item(index);
                            cx.notify();
                        })),
                        primitive: Box::new(cx.listener(
                            move |this,
                                  (kind, visible): &(
                                pomelo_core::display::LayerPrimitive,
                                bool,
                            ),
                                  _,
                                  cx| {
                                Arc::make_mut(&mut this.display).set_primitive(id, *kind, *visible);
                                this.invalidate_hover();
                                cx.notify();
                            },
                        )),
                        visibility: Box::new(cx.listener(move |this, visible: &bool, _, cx| {
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
                        })),
                        to_bottom: Box::new(cx.listener(move |this, _, _, cx| {
                            if Arc::make_mut(&mut this.display).move_layer_to_edge(
                                id,
                                false,
                                &bottom_order,
                            ) {
                                this.layer_scroll_target = Some(0);
                                this.invalidate_hover();
                                cx.notify();
                            }
                        })),
                        to_top: Box::new(cx.listener(move |this, _, _, cx| {
                            if Arc::make_mut(&mut this.display)
                                .move_layer_to_edge(id, true, &top_order)
                            {
                                this.layer_scroll_target = Some(top_order.len().saturating_sub(1));
                                this.invalidate_hover();
                                cx.notify();
                            }
                        })),
                    },
                    cx,
                )
            }),
        )
        .flex_1()
        .min_h_0();
        let layer_panel = div()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(crate::panels::sidebar::header(crate::panels::tabs::render(
                crate::panels::tabs::SidebarTabs {
                    id: "left-panel-tabs",
                    locale,
                    labels: left_tab_labels,
                    selected: self.left_panel,
                    width: left_panel_width - window.rem_size() * 2.5,
                },
                cx.listener(|this, index: &usize, _, cx| {
                    this.left_panel = *index;
                    cx.notify();
                }),
                window,
                cx,
            ), Side::Left, crate::panels::sidebar::toggle_button(Side::Left, false, locale,
                |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleLeftPanel), cx))))
            .when(self.left_panel == 0, |panel| {
                let show = Arc::clone(&all_layers);
                let hide = Arc::clone(&all_layers);
                panel
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(text(locale, Key::Layers)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        Message::new(Key::UiNumber)
                                            .arg("value", layer_count)
                                            .display(locale),
                                    ),
                            ),
                    )
                    .child(
                        div().px_4().pb_2().child(
                            Input::new(&self.layer_input)
                                .prefix(Icon::new(IconName::Search).size_4())
                                .aria_label(text(locale, Key::LayerFilter)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .px_2()
                            .pb_2()
                            .child(
                                Button::new("show-all-layers")
                                    .ghost()
                                    .small()
                                    .label(text(locale, Key::ShowAllLayers))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        for id in show.iter() {
                                            Arc::make_mut(&mut this.display)
                                                .hidden_layers
                                                .remove(id);
                                        }
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("hide-all-layers")
                                    .ghost()
                                    .small()
                                    .label(text(locale, Key::HideAllLayers))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        Arc::make_mut(&mut this.display)
                                            .hidden_layers
                                            .extend(hide.iter().copied());
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            ),
                    )
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
                    )
                    .child(crate::panels::layers::settings(
                        locale,
                        self.display.copper_opacity,
                        &self.copper_slider,
                        cx,
                    ))
            })
            .when(self.left_panel != 0, |panel| {
                let components = self.left_panel == 2;
                let use_index = self.search_query.trim().is_empty();
                let entries: Vec<_> = if use_index {
                    self.search.entries()
                } else {
                    &self.search_results
                }
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    matches!(
                        entry.target,
                        pomelo_core::search::SearchTarget::Component(_) | pomelo_core::search::SearchTarget::ComponentGroup(_)
                    ) == components
                })
                .map(|(index, _)| index)
                .collect();
                let entries = Arc::new(entries);
                let count = entries.len();
                panel
                    .child(
                        div().px_4().py_3().text_sm().child(
                            Message::new(Key::UiNumber)
                                .arg("value", count)
                                .display(locale),
                        ),
                    )
                    .child(
                        uniform_list(
                            "board-entities",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let entry = if use_index {
                                            &this.search.entries()[entries[index]]
                                        } else {
                                            &this.search_results[entries[index]]
                                        };
                                        let target = entry.target;
                                        crate::panels::search::row(
                                            index,
                                            entry,
                                            this.selected_target == Some(target.into()),
                                            match target {
                                                pomelo_core::search::SearchTarget::Net(net) => {
                                                    pomelo_render::scene::colors::net_color(net)
                                                }
                                                pomelo_core::search::SearchTarget::Component(_) | pomelo_core::search::SearchTarget::ComponentGroup(_) => {
                                                    None
                                                }
                                            },
                                            locale,
                                            cx.listener(move |this, _, window, cx| {
                                                this.locate_search(target, window, cx)
                                            }),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .flex_1()
                        .min_h_0(),
                    )
                    .when(self.search_pending, |panel| {
                        panel.child(div().p_3().text_sm().child(text(locale, Key::Searching)))
                    })
            });

        let inspector_details = div()
            .w_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().id("inspector-content").child(
                    div()
                        .px_4()
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_sm()
                        .when(self.selected_target.is_none(), |body| {
                            body.child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text(locale, Key::NothingSelected)),
                            )
                        })
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
                            let network_color = match target {
                                pomelo_core::selection::SelectionTarget::Net(net) => {
                                    pomelo_render::scene::colors::net_color(net)
                                }
                                _ => None,
                            };
                            let identity_color = network_color
                                .map(|[r, g, b, a]| Hsla::from(Rgba { r, g, b, a }))
                                .unwrap_or(cx.theme().primary);
                            let title = self
                                .selected_source_labels
                                .iter()
                                .find_map(|message| {
                                    message.args.get("name").map(ToString::to_string)
                                })
                                .unwrap_or_else(|| {
                                    Message::new(Key::SelectionIdentity)
                                        .arg("kind", text(locale, kind))
                                        .arg("id", id)
                                        .display(locale)
                                });
                            body.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .min_w_0()
                                    .gap_2()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .size_4()
                                            .flex_shrink_0()
                                            .rounded_full()
                                            .bg(identity_color),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .px_2()
                                            .py_1()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .child(text(locale, kind)),
                                    ),
                            )
                            .children(self.selected_source_labels.iter().map(|message| {
                                if let Some((label, value)) =
                                    crate::inspector::source_property(message, locale)
                                {
                                    crate::inspector::property_row(label, value, cx)
                                } else {
                                    div()
                                        .child(crate::inspector::display_source_field(
                                            message,
                                            locale,
                                            self.display.length_unit,
                                        ))
                                        .into_any_element()
                                }
                            }))
                            .child(crate::inspector::property_row(
                                text(locale, Key::PropertyIdentifier),
                                format!("0x{:X}", self.selected_anchor.map_or(id, |anchor| selection_identity(pomelo_core::selection::SelectionTarget::Object(anchor.object)).1)),
                                cx,
                            ))
                            .when_some(self.selected_anchor, |body, anchor| {
                                body.child(crate::inspector::property_row(
                                    text(locale, Key::PropertyHitLayer),
                                    crate::inspector::hit_layer_name(anchor.layer, &self.scene, locale),
                                    cx,
                                ))
                            })
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
                                    }))
                                    .map(|button| {
                                        FocusScroll::new(
                                            "inspect-related-net",
                                            &self.inspector_focus.scroll,
                                            button.w_full(),
                                        )
                                    }),
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
                                            pomelo_core::search::SearchTarget::Component(component),
                                            window,
                                            cx,
                                        )
                                    }))
                                    .map(|button| {
                                        FocusScroll::new(
                                            "inspect-related-component",
                                            &self.inspector_focus.scroll,
                                            button.w_full(),
                                        )
                                    }),
                            )
                        })
                        .when_some(self.selected_summary, |body, summary| {
                            let body = body.children(
                                [
                                    (Key::PropertySegments, summary.segments),
                                    (Key::PropertyPins, summary.pins),
                                    (Key::PropertyVias, summary.vias),
                                    (Key::PropertyZones, summary.zones),
                                    (Key::PropertyDrawings, summary.drawings),
                                ]
                                .into_iter()
                                .map(|(key, count)| {
                                    crate::inspector::property_row(
                                        text(locale, key),
                                        Message::new(Key::UiNumber)
                                            .arg("value", count)
                                            .display(locale),
                                        cx,
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
                                                pomelo_core::selection::SelectedObject::Segment(_)
                                            )
                                    )
                                ),
                                |body| {
                                    body.child(crate::inspector::property_row(
                                    text(locale, Key::PropertyLength),
                                    Message::new(Key::PropertyLengthValue)
                                        .arg(
                                            "unit",
                                            text(
                                                locale,
                                                match self.display.length_unit {
                                                    pomelo_core::units::LengthUnit::Millimeters => {
                                                        Key::UnitMillimeters
                                                    }
                                                    pomelo_core::units::LengthUnit::Mils => {
                                                        Key::UnitMils
                                                    }
                                                },
                                            ),
                                        )
                                        .arg(
                                            "length",
                                            format!(
                                                "{:.6}",
                                                self.display
                                                    .length_unit
                                                    .from_millimeters(summary.centerline_length_mm)
                                            ),
                                        )
                                        .display(locale),
                                    cx,
                                ))
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text(locale, Key::SelectionLengthScope)),
                                )
                                },
                            )
                        })
                        .when_some(self.selected_members.as_ref(), |body, members| {
                            body.child(
                                div()
                                    .pt_3()
                                    .pb_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(text(locale, Key::ConnectedObjects)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        Message::new(Key::SourceMembersShown)
                                            .arg("shown", members.objects.len())
                                            .arg("total", members.total)
                                            .display(locale),
                                    ),
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
                                        crate::panels::diagnostics::page_button(
                                                "members-previous",
                                                text(locale, Key::MembersPrevious),
                                                &self.members_navigation.previous,
                                                self.locate_pending || self.members_offset == 0,
                                                window,
                                                cx,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.page_members(
                                                    this.members_offset.saturating_sub(256),
                                                    window,
                                                    cx,
                                                );
                                            }))
                                            .map(|button| {
                                                FocusScroll::new(
                                                    "members-previous",
                                                    &self.inspector_focus.scroll,
                                                    button,
                                                )
                                            }),
                                    )
                                    .child(
                                        crate::panels::diagnostics::page_button(
                                                "members-next",
                                                text(locale, Key::MembersNext),
                                                &self.members_navigation.next,
                                                self.locate_pending
                                                    || self.members_offset.saturating_add(256)
                                                        >= members.total,
                                                window,
                                                cx,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.page_members(
                                                    this.members_offset.saturating_add(256),
                                                    window,
                                                    cx,
                                                );
                                            }))
                                            .map(|button| {
                                                FocusScroll::new(
                                                    "members-next",
                                                    &self.inspector_focus.scroll,
                                                    button,
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    // The member viewport owns its wheel input. Prevent it
                                    // from also scrolling the surrounding inspector.
                                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                                    .child(
                                        div()
                                            .id("selection-source-members")
                                            .w_full()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .h_24()
                                            .flex_shrink_0()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.members_scroll)
                                            .vertical_scrollbar(&self.members_scroll)
                                            .child(
                                                div().id(("source-members-page", self.members_page_revision)).w_full().min_w_0().flex_none().h_auto().min_h_full()
                                                .flex().flex_col()
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
                                        .w_full()
                                        .h_10()
                                        .flex_shrink_0()
                                        .mb_2()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .rounded_md()
                                        .selected(self.selected_target == Some(target))
                                        .disabled(self.locate_pending)
                                        .native_tooltip(
                                            Message::new(Key::LocateMember)
                                                .arg("kind", text(locale, kind))
                                                .arg("id", id)
                                                .display(locale),
                                        )
                                        .accessibility_label(
                                            Message::new(Key::LocateMember)
                                                .arg("kind", text(locale, kind))
                                                .arg("id", id)
                                                .display(locale),
                                        )
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    Icon::new(
                                                        gpui_kit::assets::IconName::CircuitBoard,
                                                    )
                                                    .size_4()
                                                    .flex_shrink_0(),
                                                )
                                                .child(
                                                    div().flex_1().min_w_0().flex().gap_1()
                                                        .child(div().min_w_0().truncate().child(text(locale, kind)))
                                                        .child(div().flex_shrink_0().child(
                                                            Message::new(Key::UiNumber).arg("value", id).display(locale),
                                                        )),
                                                )
                                                .child(
                                                    Icon::new(
                                                        gpui_kit::assets::IconName::ChevronRight,
                                                    )
                                                    .size_4()
                                                    .flex_shrink_0(),
                                                ),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.locate_target(target, window, cx)
                                            }),
                                        )
                                        .map(|button| FocusScroll::new(
                                            ("source-member", (u64::from(category) << 32) | u64::from(id)),
                                            &self.members_scroll,
                                            button,
                                        ).w_full().min_w_0())
                                            })),
                                            )
                                            .map(|list| FocusScroll::new("source-members-region", &self.inspector_focus.scroll, list.w_full().min_w_0()).w_full().min_w_0()),
                                    ),
                            )
                        }),
                ),
            );
        let theme = cx.theme();
        let inspector = div()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(crate::panels::sidebar::header(crate::panels::tabs::render(
                crate::panels::tabs::SidebarTabs {
                    id: "right-panel-tabs",
                    locale,
                    labels: right_tab_labels,
                    selected: usize::from(self.show_display),
                    width: right_panel_width - window.rem_size() * 2.5,
                },
                cx.listener(|this, index: &usize, _, cx| {
                    this.show_display = *index == 1;
                    cx.notify();
                }),
                window,
                cx,
            ), Side::Right, crate::panels::sidebar::toggle_button(Side::Right, false, locale,
                |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleRightPanel), cx))))
            .child(
                div()
                    .id("right-panel-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.inspector_focus.scroll)
                    .vertical_scrollbar(&self.inspector_focus.scroll)
                    .child(
                        div()
                            .flex_none()
                            .h_auto()
                            .min_h_full()
                            .when(self.locate_pending, |body| {
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .child(text(locale, self.pending_message)),
                                )
                            })
                            .when_some(self.search_notice.as_ref(), |body, message| {
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child(message.display(locale)),
                                )
                            })
                            .when_some(self.locate_details.as_ref(), |body, error| {
                                let message = match error {
                                    pomelo_core::geometry::PathError::Cancelled => {
                                        Message::new(Key::Cancelled)
                                    }
                                    pomelo_core::geometry::PathError::Invalid(id) => {
                                        Message::new(Key::PickInvalid).arg("object", id.0)
                                    }
                                    pomelo_core::geometry::PathError::PointLimit {
                                        actual,
                                        limit,
                                    } => Message::new(Key::GeometryLimit)
                                        .arg("actual", *actual)
                                        .arg("limit", *limit),
                                };
                                body.child(
                                    div()
                                        .p_3()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child(message.display(locale)),
                                )
                            })
                            .when(!self.show_display, |body| {
                                body.child(
                            div()
                                .px_4()
                                .pt_4()
                                .pb_2()
                                .flex()
                                .items_center()
                                .justify_between()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(text(locale, Key::SelectedObject))
                                .when(
                                    self.selected_target.is_some() || self.locate_pending,
                                    |heading| {
                                        heading.child(
                                            Button::new("inspector-clear-selection")
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::X)
                                                .accessibility_label(text(
                                                    locale,
                                                    Key::ClearSelection,
                                                ))
                                                .native_tooltip(text(locale, Key::ClearSelection))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.clear_selection(
                                                        &ClearSelection,
                                                        window,
                                                        cx,
                                                    )
                                                }))
                                                .map(|button| FocusScroll::new("inspector-clear-selection", &self.inspector_focus.scroll, button)),
                                        )
                                    },
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .mx_4()
                                .p_1()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
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
                                            .small()
                                            .when(self.selection_mode == mode, |button| button.primary())
                                            .px_1()
                                            .flex_grow(1.0)
                                            .flex_shrink_0()
                                            .native_tooltip(text(locale, key))
                                            .label(text(locale, key))
                                            .selected(self.selection_mode == mode)
                                            .toggled(self.selection_mode == mode)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_selection_mode(mode, window, cx);
                                            }))
                                            .map(|button| FocusScroll::new(("selection-mode", id), &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0())
                                    }),
                                ),
                        )
                        .child(inspector_details)
                        .child(
                            div()
                                .px_4()
                                .py_2()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Button::new("locate-selection")
                                        .primary()
                                        .flex_grow(1.0)
                                        .flex_shrink_0()
                                        .label(
                                            Message::new(Key::LocateSelected)
                                                .arg(
                                                    "kind",
                                                    text(
                                                        locale,
                                                        self.selected_target
                                                            .map(|target| {
                                                                selection_identity(target).0
                                                            })
                                                            .unwrap_or(Key::ModeObject),
                                                    ),
                                                )
                                                .display(locale),
                                        )
                                        .disabled(
                                            self.selected_target.is_none() || self.locate_pending,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if let Some(target) = this.selected_target {
                                                this.locate_target(target, window, cx);
                                            }
                                        }))
                                        .map(|button| FocusScroll::new("locate-selection", &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0()),
                                )
                                .child(
                                    Button::new("clear-selection")
                                        .outline()
                                        .flex_grow(1.0)
                                        .flex_shrink_0()
                                        .label(text(locale, Key::ClearSelection))
                                        .disabled(
                                            self.selected_target.is_none() && !self.locate_pending,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.clear_selection(&ClearSelection, window, cx)
                                        }))
                                        .map(|button| FocusScroll::new("clear-selection", &self.inspector_focus.scroll, button.w_full()).flex_grow(1.0).flex_shrink_0()),
                                ),
                        )
                        .child(crate::panels::picking::controls(
                            locale,
                            self.pick_filter,
                            self.selection_mode,
                            &self.inspector_focus.scroll,
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
                            })
                            .when(self.show_display, |body| {
                                body.when_some(appearance_controls, |body, controls| body.child(controls)).children(opacity_controls).children(crate::panels::display::controls(
                                    locale,
                                    &self.display,
                                    crate::panels::display::DisplayCommands {
                                        filled: Box::new(cx.listener(
                                            |this, filled: &bool, _, cx| {
                                                if this.display.filled != *filled {
                                                    Arc::make_mut(&mut this.display).filled = *filled;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        horizontal_pin_names: Box::new(cx.listener(
                                            |this, horizontal: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).horizontal_pin_names = *horizontal;
                                                this.invalidate_hover();
                                                cx.notify();
                                            },
                                        )),
                                        labels: Box::new(cx.listener(
                                            |this, (kind, enabled): &(pomelo_core::display::LabelKind, bool), _, cx| {
                                                Arc::make_mut(&mut this.display).label_options.set(*kind, *enabled);
                                                cx.notify();
                                            },
                                        )),
                                        drills: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                if this.display.show_drills != *visible {
                                                    Arc::make_mut(&mut this.display).show_drills =
                                                        *visible;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        backdrills: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                if this.display.show_backdrills != *visible {
                                                    Arc::make_mut(&mut this.display).show_backdrills = *visible;
                                                    this.invalidate_hover();
                                                    cx.notify();
                                                }
                                            },
                                        )),
                                        copper: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_copper =
                                                    *visible;
                                                this.invalidate_hover();
                                                cx.notify();
                                            },
                                        )),
                                        static_shapes_fill_solid: Box::new(cx.listener(
                                            |this, solid: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).static_shapes_fill_solid = *solid;
                                                cx.notify();
                                            },
                                        )),
                                        texts: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_texts =
                                                    *visible;
                                                cx.notify();
                                            },
                                        )),
                                        drawings: Box::new(cx.listener(
                                            |this, visible: &bool, _, cx| {
                                                Arc::make_mut(&mut this.display).show_drawings =
                                                    *visible;
                                                cx.notify();
                                            },
                                        )),
                                        reset_order: Box::new(cx.listener(|this, _, _, cx| {
                                            Arc::make_mut(&mut this.display).layer_order.clear();
                                            this.layer_scroll_target = Some(0);
                                            this.invalidate_hover();
                                            cx.notify();
                                        })),
                                    },
                                    &self.inspector_focus.scroll,
                                ))
                            })
                            .child(crate::panels::file_info::render(
                                crate::panels::file_info::FileInformation {
                                    locale,
                                    expanded: self.file_information_expanded,
                                    scene: &self.scene,
                                },
                                self.diagnostics_panel(window, cx),
                                &self.inspector_focus.file_information,
                                cx.listener(|this, expanded: &bool, _, cx| {
                                    this.file_information_expanded = *expanded;
                                    cx.notify();
                                }),
                                window,
                                cx,
                            )),
                    ),
            );

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
        let stats = self.telemetry.snapshot();
        let copper_stats = self.copper_telemetry.snapshot();
        let curve_frame = self.curve_fill.frame();
        let curve_revision = curve_frame.as_ref().map_or(0, |cache| cache.revision());
        let curve_bytes = curve_frame.as_ref().map_or(0, |cache| {
            cache
                .active
                .iter()
                .filter_map(|id| cache.entries.get(id))
                .map(|entry| entry.source.upload_bytes() as u64)
                .sum::<u64>()
        });
        let pad_stats = self.pad_telemetry.snapshot();
        let custom_pad_stats = self.custom_pad_telemetry.snapshot();
        let custom_outline_stats = self.custom_outline_telemetry.snapshot();
        let zone_outline_stats = self.zone_outline_telemetry.snapshot();
        let drill_stats = self.drill_telemetry.snapshot();
        let drawing_stats = self.drawing_telemetry.snapshot();
        let text_stats = self.text_telemetry.snapshot();
        let source_stroke_stats = self.source_stroke_telemetry.snapshot();
        let label_stats = self.label_telemetry.snapshot();
        let custom_bytes = self
            .pads
            .custom_mesh
            .as_ref()
            .map_or(0, |mesh| mesh.upload_bytes());
        let renderer = self.renderer.as_ref().ok().cloned();
        let status = renderer.as_ref().map(GpuPainterHandle::status);
        let failure = self
            .renderer
            .as_ref()
            .err()
            .cloned()
            .or_else(|| self.paint_error.clone())
            .or_else(|| status.as_ref().and_then(|status| status.last_error.clone()));
        let geometry_ready = stats.uploaded_instances == self.tracks.instances.len() as u64
            && copper_stats.uploaded_bytes == self.copper.upload_bytes() as u64
            && pad_stats.uploaded_instances == self.pads.analytic.len() as u64
            && custom_pad_stats.uploaded_bytes == custom_bytes as u64
            && custom_outline_stats.uploaded_instances
                == self
                    .pads
                    .custom_outlines
                    .as_ref()
                    .map_or(0, |source| source.instances.len()) as u64
            && drill_stats.uploaded_instances == self.drills.analytic.len() as u64
            && zone_outline_stats.uploaded_instances == self.zone_outlines.instances.len() as u64
            && drawing_stats.uploaded_instances == self.drawings.instances.len() as u64
            && text_stats.uploaded_instances
                == self
                    .texts
                    .as_ref()
                    .map_or(0, |source| source.instances.len()) as u64
            && source_stroke_stats.uploaded_instances
                == self
                    .source_strokes
                    .as_ref()
                    .map_or(0, |source| source.instances.len()) as u64;
        let ready = geometry_ready
            && !self.curve_fill.pending
            && !self.curve_fill.failed
            && copper_stats.curve_uploaded_bytes == curve_bytes
            && copper_stats.curve_revision == curve_revision
            && label_stats.uploaded_instances
                == self
                    .label_cache
                    .as_ref()
                    .map_or(0, |cache| cache.source.instances.len()) as u64;
        if failure.is_none()
            && !self.curve_fill.failed
            && (!ready || status.as_ref().is_some_and(|status| status.encoded == 0))
        {
            window.request_animation_frame();
        }
        if let Some(path) = std::env::var_os("POMELO_BOARD_GPU_TRACE") {
            let specs = window.gpu_specs();
            let pick_filter = serde_json::json!({
                "segment": self.pick_filter.contains(pomelo_core::picking::PickCategory::Segment),
                "pin": self.pick_filter.contains(pomelo_core::picking::PickCategory::Pin),
                "via": self.pick_filter.contains(pomelo_core::picking::PickCategory::Via),
                "zone": self.pick_filter.contains(pomelo_core::picking::PickCategory::Zone),
                "drawing": self.pick_filter.contains(pomelo_core::picking::PickCategory::Drawing),
            });
            let members_report = self.selected_members.as_ref().map(|members| {
                serde_json::json!({
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
                            SelectedObject::Drawing(id) => ("drawing", id),
                        };
                        serde_json::json!({"category":category,"source_id":id.0})
                    }).collect::<Vec<_>>(),
                })
            });
            let mut report = serde_json::json!({
                "backend": pomelo_render::backend::native::NAME, "shader_owner": "pomelo-render", "scope": "traces_outline_copper_pads_drills_drawings_msdf_text_labels",
                "cpu_pixel_readback": false, "expected_instances": self.tracks.instances.len(),
                "ready": ready, "status": {
                    "encoded": status.as_ref().map(|status| status.encoded),
                    "last_error": failure,
                },
                "statistics": stats,
                "copper_statistics": copper_stats,
                "curve_statistics": self.curve_fill.statistics(),
                "curve_pending": self.curve_fill.pending,
                "curve_failed": self.curve_fill.failed,
                "expected_curve_bytes": curve_bytes,
                "expected_curve_revision": curve_revision,
                "pad_statistics": pad_stats,
                "custom_pad_statistics": custom_pad_stats,
                "custom_outline_statistics": custom_outline_stats,
                "zone_outline_statistics": zone_outline_stats,
                "expected_zone_outline_instances": self.zone_outlines.instances.len(),
                "drill_statistics": drill_stats,
                "drawing_statistics": drawing_stats,
                "expected_drawing_instances": self.drawings.instances.len(),
                "text_statistics": text_stats,
                "source_stroke_statistics": source_stroke_stats,
                "label_statistics": label_stats,
            });
            let details = serde_json::json!({
                "expected_source_stroke_instances": self.source_strokes.as_ref().map_or(0, |source| source.instances.len()),
                "source_stroke_objects": self.source_strokes.as_ref().map_or(&[][..], |source| source.objects.as_slice()),
                "expected_text_instances": self.texts.as_ref().map_or(0, |source| source.instances.len()),
                "expected_drills": self.drills.analytic.len(),
                "adapter": specs.as_ref().map(|specs| &specs.device_name),
                "software_emulated": specs.as_ref().map(|specs| specs.is_software_emulated),
                "board_bounds": self.scene.bounds,
                "source_trace_count": self.scene.segments.len(),
                "color_mode": self.display.color_mode,
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
                "selected_anchor": self.selected_anchor,
                "panel_layout": panels,
                "canvas_size": self.navigation.size(),
                "device_scale_factor": window.scale_factor(),
                "selection_mode": format!("{:?}", self.selection_mode),
                "hovered_object": self.hovered.as_ref().filter(|(_, context)| context.matches_view(
                    self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                )).map(|(object, _)| format!("{object:?}")),
                "pick_filter": pick_filter,
                "selection_query": {
                    "search_pending": self.search_pending,
                    "selection_pending": self.locate_pending,
                    "candidate_position": self.last_pick.as_ref().filter(|context| context.matches_view(
                        self.navigation.camera(), self.navigation.size(), &self.display, self.selection_mode,
                    )).and(self.candidate_position),
                },
                "members": members_report,
                "search_entries": self.search.entries().len(),
                "pick_index_segments": self.picking.segment_count(),
                "display": {
                    "hidden_layers": self.display.hidden_layers.iter().map(|layer| layer.0).collect::<Vec<_>>(),
                    "show_drills": self.display.show_drills,
                    "show_backdrills": self.display.show_backdrills,
                    "show_copper": self.display.show_copper,
                    "static_shapes_fill_solid": self.display.static_shapes_fill_solid,
                    "filled": self.display.filled,
                    "horizontal_pin_names": self.display.horizontal_pin_names,
                    "label_options": self.display.label_options,
                    "appearance": self.display.appearance,
                    "show_texts": self.display.show_texts,
                    "show_drawings": self.display.show_drawings,
                    "layer_order": &*layer_order,
                    "copper_opacity": self.display.copper_opacity,
                    "global_opacity": self.display.global_opacity,
                },
                "camera": {
                    "center": self.navigation.camera().center,
                    "logical_pixels_per_mm": self.navigation.camera().pixels_per_mm,
                    "flipped": self.navigation.camera().flipped,
                    "logical_viewport_size": self.navigation.size(),
                },
            });
            if let (serde_json::Value::Object(report), serde_json::Value::Object(details)) =
                (&mut report, details)
            {
                report.extend(details);
            }
            if let Err(error) = std::fs::write(path, report.to_string()) {
                eprintln!("{}: {error}", text(locale, Key::FileIoFailed));
            }
        }
        let mut frame = TraceFrame {
            pass: pomelo_render::backend::native::OverlayPass::Base,
            filled: self.display.filled,
            hover_selection: self.hover_selection.clone().filter(|_| {
                self.hovered.as_ref().is_some_and(|(_, context)| {
                    context.matches_view(
                        self.navigation.camera(),
                        self.navigation.size(),
                        &self.display,
                        self.selection_mode,
                    )
                })
            }),
            color_mode: self.display.color_mode,
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
                        pomelo_render::backend::native::TraceSelection::Segment(id)
                    }
                    SelectionTarget::Track(id) => {
                        pomelo_render::backend::native::TraceSelection::Track(id)
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
            fallback_color: [0.6, 0.68, 0.73, 1.0],
            material_override: None,
            opacity: 1.0,
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
                        | pomelo_core::selection::SelectionTarget::ComponentGroup(_)
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
        let custom_outline_telemetry = Arc::clone(&self.custom_outline_telemetry);
        let drill_telemetry = Arc::clone(&self.drill_telemetry);
        let drawing_telemetry = Arc::clone(&self.drawing_telemetry);
        let text_telemetry = Arc::clone(&self.text_telemetry);
        let source_stroke_telemetry = Arc::clone(&self.source_stroke_telemetry);
        let zone_outline_telemetry = Arc::clone(&self.zone_outline_telemetry);
        let label_telemetry = Arc::clone(&self.label_telemetry);
        let texts = self.texts.clone();
        let source_strokes = self.source_strokes.clone();
        let drawings = Arc::clone(&self.drawings);
        let drills = Arc::clone(&self.drills);
        let display = Arc::clone(&self.display);
        let drill_color = [0.46, 0.49, 0.51, 1.0];
        let pads = Arc::clone(&self.pads);
        let copper = Arc::clone(&self.copper);
        let zone_outlines = Arc::clone(&self.zone_outlines);
        let copper_opacity = self.display.copper_opacity;
        let viewport = canvas(
            move |bounds, _, cx| {
                let mut resized = false;
                let mut labels = None;
                let mut curves = None;
                frame.camera = layout_view
                    .update(cx, |this, cx| {
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
                        let camera = this.navigation.camera();
                        let viewport_size = this.navigation.size();
                        let (width, height) = (viewport_size.x, viewport_size.y);
                        this.request_curves(
                            pomelo_render::scene::curves::CurveView::new(
                                camera,
                                width,
                                height,
                                f64::from(frame.scale_factor),
                            ),
                            cx,
                        );
                        curves = this.curve_fill.frame();
                        if let Some(index) = &this.label_index {
                            let key = [
                                camera.center.x.to_bits(),
                                camera.center.y.to_bits(),
                                camera.pixels_per_mm.to_bits(),
                                u64::from(camera.flipped),
                                width.to_bits(),
                                height.to_bits(),
                            ];
                            if this.label_cache.as_ref().is_none_or(|cache| {
                                cache.key != key || !Arc::ptr_eq(&cache.display, &this.display)
                            }) {
                                match index.layout(
                                    camera,
                                    width,
                                    height,
                                    &this.display,
                                    this.display.label_options,
                                    &pomelo_core::task::CancellationToken::default(),
                                ) {
                                    Ok(source) => {
                                        this.label_cache = Some(LabelCache {
                                            key,
                                            display: Arc::clone(&this.display),
                                            source: Arc::new(source),
                                        })
                                    }
                                    Err(diagnostic) => {
                                        this.label_cache = None;
                                        if !this
                                            .render_diagnostics
                                            .iter()
                                            .any(|item| item.code == diagnostic.code)
                                        {
                                            this.render_diagnostics.push(diagnostic);
                                        }
                                    }
                                }
                            }
                            labels = this
                                .label_cache
                                .as_ref()
                                .map(|cache| Arc::clone(&cache.source));
                        }
                        camera
                    })
                    .ok();
                if resized {
                    frame.hovered_object = None;
                }
                Arc::new(BoardFrame {
                    curves,
                    zone_outlines: Some(zone_outlines),
                    drawings: Some(drawings),
                    texts: source_strokes,
                    glyphs: texts,
                    labels,
                    traces: frame,
                    display,
                    pads: Some(pads),
                    drills: Some(drills),
                    drill_color,
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
                    let before_custom_outlines =
                        custom_outline_telemetry.snapshot().uploaded_instances;
                    let before_drills = drill_telemetry.snapshot().uploaded_instances;
                    let before_drawings = drawing_telemetry.snapshot().uploaded_instances;
                    let before_texts = text_telemetry.snapshot().uploaded_instances;
                    let before_source_strokes =
                        source_stroke_telemetry.snapshot().uploaded_instances;
                    let before_labels = label_telemetry.snapshot().uploaded_instances;
                    let before_zone_outlines = zone_outline_telemetry.snapshot().uploaded_instances;
                    window.on_next_frame(move |_, cx| {
                        let after = renderer.status();
                        let uploaded = telemetry.snapshot().uploaded_instances;
                        if before.last_error != after.last_error
                            || before.resets != after.resets
                            || before_uploaded != uploaded
                            || before_copper != copper_telemetry.snapshot().uploaded_bytes
                            || before_pads != pad_telemetry.snapshot().uploaded_instances
                            || before_custom != custom_pad_telemetry.snapshot().uploaded_bytes
                            || before_custom_outlines
                                != custom_outline_telemetry.snapshot().uploaded_instances
                            || before_drills != drill_telemetry.snapshot().uploaded_instances
                            || before_drawings != drawing_telemetry.snapshot().uploaded_instances
                            || before_texts != text_telemetry.snapshot().uploaded_instances
                            || before_source_strokes
                                != source_stroke_telemetry.snapshot().uploaded_instances
                            || before_labels != label_telemetry.snapshot().uploaded_instances
                            || before_zone_outlines
                                != zone_outline_telemetry.snapshot().uploaded_instances
                            || (before.encoded == 0 && after.encoded > 0)
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
            .child(toolbar::render(
                toolbar::State {
                    locale,
                    pan_tool: self.pan_tool,
                    color_mode: self.display.color_mode,
                    search: &self.search_input,
                },
                toolbar::Commands {
                    select: Box::new(cx.listener(|this, _, window, cx| {
                        this.pan_tool = false;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
                    pan: Box::new(cx.listener(|this, _, window, cx| {
                        this.pan_tool = true;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
                    colors: Box::new(cx.listener(
                        |this, mode: &pomelo_core::display::ColorMode, _, cx| {
                            Arc::make_mut(&mut this.display).color_mode = *mode;
                            this.invalidate_hover();
                            cx.notify();
                        },
                    )),
                    fit: Box::new(cx.listener(|this, _, window, cx| {
                        this.fit_board(&FitBoard, window, cx);
                    })),
                },
                cx,
            ))
            .child(
                div().flex().flex_1().min_h_0().min_w_0()
                .when(panels.left_collapsed, |row| row.child(crate::panels::sidebar::rail(Side::Left,
                    crate::panels::sidebar::toggle_button(Side::Left, true, locale,
                        |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleLeftPanel), cx)), cx)))
                .child(
                    h_resizable("board-panels")
                        .with_state(&self.panel_sizes)
                        .on_resize(cx.listener(|_, _, _, cx| cx.notify()))
                        .when(!panels.left_collapsed, |group| group.child(
                            resizable_panel()
                                .size(left_panel_width)
                                .size_range(px(200.0)..left_max_width)
                                .flex_none()
                                .child(layer_panel),
                        ))
                        .child(
                            resizable_panel().size_range(px(crate::workbench::panel_layout::MIN_CANVAS_WIDTH)..Pixels::MAX).child(
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
                                    .bg(self.canvas_background())
                                    .focus_visible(|style| style.border_color(theme.ring))
                                    .cursor(if self.drag_position.is_some() {
                                        CursorStyle::ClosedHand
                                    } else if self.pan_tool {
                                        CursorStyle::OpenHand
                                    } else {
                                        CursorStyle::Arrow
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event, window, cx| {
                                            if this.pan_tool {
                                                this.start_pan(event, window, cx);
                                            } else {
                                                this.pick_trace(event, window, cx);
                                            }
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Middle,
                                        cx.listener(Self::start_pan),
                                    )
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
                                    .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_pan))
                                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::stop_pan))
                                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::stop_pan))
                                    .on_mouse_up_out(
                                        MouseButton::Middle,
                                        cx.listener(Self::stop_pan),
                                    )
                                    .on_scroll_wheel(cx.listener(Self::scroll))
                                    .child(viewport)
                                    // Initial uploads must not resize the canvas after its first fit.
                                    .when(!geometry_ready && failure.is_none(), |canvas| {
                                        canvas.child(
                                            div()
                                                .absolute()
                                                .top_10()
                                                .left_4()
                                                .max_w(relative(0.85))
                                                .truncate()
                                                .bg(theme.popover)
                                                .text_color(theme.popover_foreground)
                                                .rounded_sm()
                                                .px_4()
                                                .py_1()
                                                .text_xs()
                                                .child(
                                                    Message::new(Key::TraceUpload)
                                                        .arg(
                                                            "completed",
                                                            stats.uploaded_instances * 128
                                                                + copper_stats.uploaded_bytes
                                                                + copper_stats.curve_uploaded_bytes
                                                                + pad_stats.uploaded_instances
                                                                    * 128
                                                                + custom_pad_stats.uploaded_bytes
                                                                + drill_stats.uploaded_instances
                                                                    * 128,
                                                        )
                                                        .arg(
                                                            "total",
                                                            self.tracks.instances.len() * 128
                                                                + self.copper.upload_bytes()
                                                                + curve_bytes as usize
                                                                + self.pads.analytic.len() * 128
                                                                + custom_bytes
                                                                + self.drills.analytic.len() * 128,
                                                        )
                                                        .display(locale),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .absolute()
                                            .top_3()
                                            .left_4()
                                            .max_w(relative(0.85))
                                            .truncate()
                                            .text_sm()
                                            .text_color(crate::theme::canvas_colors().1)
                                            .child(canvas_breadcrumb),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_4()
                                            .left_4()
                                            .w(rems(3.5))
                                            .h_16()
                                            .text_sm()
                                            .text_color(crate::theme::canvas_colors().1)
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .child(text(locale, Key::AxisY)),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left_1()
                                                    .top_6()
                                                    .w(px(2.0))
                                                    .h_8()
                                                    .bg(crate::theme::canvas_axes().1),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left_1()
                                                    .bottom_2()
                                                    .w_8()
                                                    .h(px(2.0))
                                                    .bg(crate::theme::canvas_axes().0),
                                            )
                                            .child(
                                                div().absolute().left(px(-3.0)).top_4().child(
                                                    Icon::new(IconName::ArrowUp)
                                                        .size_4()
                                                        .text_color(crate::theme::canvas_axes().1),
                                                ),
                                            )
                                            .child(
                                                div().absolute().left_6().bottom_0().child(
                                                    Icon::new(IconName::ArrowRight)
                                                        .size_4()
                                                        .text_color(crate::theme::canvas_axes().0),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .right_0()
                                                    .bottom_0()
                                                    .child(text(locale, Key::AxisX)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_4()
                                            .left_0()
                                            .right_0()
                                            .flex()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .p_1()
                                                    .border_1()
                                                    .border_color(crate::theme::canvas_colors().2)
                                                    .rounded_md()
                                                    .bg(crate::theme::canvas_toolbar_surface())
                                                    .text_color(crate::theme::canvas_colors().1)
                                                    // Commands overlay the board. Their pointer
                                                    // events must not also pick/pan the geometry.
                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                        cx.stop_propagation();
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Middle,
                                                        |_, _, cx| cx.stop_propagation(),
                                                    )
                                                    .on_scroll_wheel(|_, _, cx| {
                                                        cx.stop_propagation();
                                                    })
                                                    .child(
                                                        Button::new("canvas-select")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::MousePointer2)
                                                            .selected(!self.pan_tool)
                                                            .toggled(!self.pan_tool)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::SelectTool,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::SelectTool,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.pan_tool = false;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("canvas-pan")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Hand)
                                                            .selected(self.pan_tool)
                                                            .toggled(self.pan_tool)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::PanTool,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::PanTool,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.pan_tool = true;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(1.0))
                                                            .h_6()
                                                            .mx_1()
                                                            .bg(crate::theme::canvas_colors().2),
                                                    )
                                                    .child(
                                                        Button::new("zoom-out")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Minus)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::ZoomOut,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::ZoomOut,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.zoom(1.0 / 1.2, cx)
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div().px_2().text_sm().child(
                                                            Message::new(Key::ZoomPercent)
                                                                .arg(
                                                                    "percent",
                                                                    (self.navigation.zoom_percent())
                                                                        .round()
                                                                        as u64,
                                                                )
                                                                .display(locale),
                                                        ),
                                                    )
                                                    .child(
                                                        Button::new("zoom-in")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Plus)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::ZoomIn,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::ZoomIn,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| this.zoom(1.2, cx),
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(1.0))
                                                            .h_6()
                                                            .mx_1()
                                                            .bg(crate::theme::canvas_colors().2),
                                                    )
                                                    .child(
                                                        Button::new("canvas-fit")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::Scan)
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::FitBoard,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::FitBoard,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.fit_board(
                                                                        &FitBoard, window, cx,
                                                                    )
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("flip-board")
                                                            .custom(
                                                                crate::theme::canvas_toolbar_button(
                                                                    cx,
                                                                ),
                                                            )
                                                            .small()
                                                            .size_8()
                                                            .icon(IconName::RotateCcw)
                                                            .selected(
                                                                self.navigation.camera().flipped,
                                                            )
                                                            .accessibility_label(text(
                                                                locale,
                                                                Key::FlipBoard,
                                                            ))
                                                            .native_tooltip(text(
                                                                locale,
                                                                Key::FlipBoard,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.flip_board(
                                                                        &FlipBoard, window, cx,
                                                                    )
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    )
                                    .when_some(scale_bar, |canvas, scale| {
                                        canvas.child(
                                            div()
                                                .absolute()
                                                .right_4()
                                                .bottom_4()
                                                .when(self.navigation.size().x < 640.0, |label| {
                                                    label.bottom_16()
                                                })
                                                .text_sm()
                                                .text_color(crate::theme::canvas_colors().1)
                                                .child(
                                                    presentation::scale_message(
                                                        scale,
                                                        self.display.length_unit,
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
                                                        .border_color(
                                                            crate::theme::canvas_colors().1,
                                                        ),
                                                ),
                                        )
                                    }),
                            ),
                        )
                        .when(!panels.right_collapsed, |group| group.child(
                            resizable_panel()
                                .size(right_panel_width)
                                .size_range(px(240.0)..right_max_width)
                                .flex_none()
                                .child(inspector),
                        )),
                )
                .when(panels.right_collapsed, |row| row.child(crate::panels::sidebar::rail(Side::Right,
                    crate::panels::sidebar::toggle_button(Side::Right, true, locale,
                        |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleRightPanel), cx)), cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_shrink_0()
                    .px_4()
                    .h_9()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().size_2().rounded_full().bg(theme.primary))
                            .child(text(
                                locale,
                                if ready { Key::Ready } else { Key::PreparingGpu },
                            ))
                            .child(
                                Message::new(Key::WorkspaceSummary)
                                    .arg("layers", self.scene.layers.len())
                                    .arg("components", self.scene.components.len())
                                    .arg("nets", self.scene.nets.len())
                                    .display(locale),
                            ),
                    )
                    .when_some(pointer_coordinates, |bar, coordinates| {
                        bar.child(div().truncate().child(coordinates))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(text(locale, Key::LocalFile))
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
                            .child(text(locale, Key::GpuStatus)),
                    ),
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
        if !hover_text.is_empty() {
            view = view.child(
                div()
                    .absolute()
                    .bottom_24()
                    .left_72()
                    .rounded_md()
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .px_3()
                    .py_2()
                    .text_sm()
                    .child(hover_text),
            );
        }
        if let Some(details) = failure {
            // Error details must not resize the canvas and invalidate its curve view.
            view = view.child(
                div()
                    .absolute()
                    .bottom_12()
                    .left_4()
                    .max_w(relative(0.85))
                    .rounded_sm()
                    .bg(theme.popover)
                    .px_3()
                    .py_2()
                    .text_color(theme.danger)
                    .child(div().child(text(locale, Key::GpuFailed)))
                    .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                    .child(div().text_sm().child(details)),
            );
        }
        view
    }
}

impl Focusable for BoardViewport {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

struct LabelCache {
    key: [u64; 6],
    display: Arc<pomelo_core::display::BoardDisplay>,
    source: Arc<pomelo_render::text::msdf::PreparedGlyphs>,
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
