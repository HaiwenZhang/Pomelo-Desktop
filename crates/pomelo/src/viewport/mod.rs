//! Native desktop board canvas. Business GPU resources stay in pomelo-render.
use crate::panels::focus_scroll::FocusScroll;
use crate::tooltips::ButtonTooltipExt;
mod appearance;
mod canvas_toolbar;
mod curves;
mod diagnostics_panel;
mod frame;
mod gesture;
mod gpu_trace;
mod grid;
mod hover;
mod inspector_panel;
mod labels;
mod layer_panel;
mod layout;
mod motion;
mod navigation;
mod net_colors;
mod object_filter;
mod opacity;
mod patterns;
mod picking;
mod review;
mod review_export;
mod review_overlay;
mod review_panel;
mod scale_label;
mod search;
mod selection;
mod telemetry;
mod toolbar;
mod view3d;

pub use crate::actions::FocusSearch;
use crate::i18n;
use crate::panels::inspector::selection_identity;
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
    backend::native::{BoardFrame, TraceFrame},
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
        ToggleGrid,
        ZoomIn,
        ZoomOut,
        PanLeft,
        PanRight,
        PanUp,
        PanDown,
        CancelCanvasAction,
        MeasureDistance,
        MeasurePolyline,
        MeasureClearance,
        BoxSelect,
        ZoomToArea,
        FinishMeasurement,
        RemoveLastPoint,
        NavigateBack,
        NavigateForward
    ]
);

pub struct BoardViewport {
    view3d: view3d::View3d,
    preparation_budget: pomelo_core::memory::MemoryBudget,
    hover_selection: Option<(
        pomelo_core::selection::SelectionTarget,
        Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
    )>,
    hovered: Option<(pomelo_core::selection::SelectedObject, PickContext)>,
    hover_notice: Option<Message>,
    hover_tooltip_ready: bool,
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
    labels: labels::LabelState,
    curve_fill: curves::CurveState,
    copper: Arc<pomelo_render::copper::PreparedCopper>,
    placements: Arc<pomelo_render::copper::PreparedCopper>,
    placement_outlines: Arc<PreparedTracks>,
    pads: Arc<pomelo_render::pads::PreparedPads>,
    drills: Arc<pomelo_render::pads::PreparedPads>,
    scene: Arc<BoardScene>,
    format_name: String,
    render_diagnostics: Vec<pomelo_core::model::Diagnostic>,
    diagnostics_expanded: bool,
    diagnostics_page: usize,
    search: Arc<pomelo_core::search::SearchIndex>,
    picking: Arc<pomelo_core::picking_index::BoardPickingIndex>,
    search_input: Entity<InputState>,
    layer_input: Entity<InputState>,
    /// Narrows the net and component lists without changing the board search.
    entity_filter_input: Entity<InputState>,
    entity_filter: String,
    /// Keyboard-highlighted row in the net and component list.
    entity_highlight: usize,
    entity_scroll: UniformListScrollHandle,
    layer_query: String,
    input_locale: Option<pomelo_core::i18n::Locale>,
    left_panel: LeftPanelTab,
    right_panel: RightPanelTab,
    review: review::ReviewSession,
    rename_input: Entity<InputState>,
    /// Source file name, offered in exports unless the user anonymizes them.
    source_name: String,
    file_information_expanded: bool,
    file_information_scroll: ScrollHandle,
    pan_tool: bool,
    pan_start: Option<pomelo_core::interaction::Camera>,
    wheel_history_at: Option<std::time::Instant>,
    copper_slider: Entity<SliderState>,
    global_slider: Entity<SliderState>,
    shadow_slider: Entity<SliderState>,
    object_filter_slider: Entity<SliderState>,
    last_visibility: Option<pomelo_core::display::LayerVisibility>,
    net_color_target: Option<pomelo_core::model::NetId>,
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
    selected_related_objects: Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
    search_cancel: pomelo_core::task::CancellationToken,
    search_task: Option<Task<()>>,
    search_pending: bool,
    search_notice: Option<Message>,
    locate_pending: bool,
    pending_message: Key,
    locate_details: Option<pomelo_core::geometry::PathError>,
    search_query: String,
    layer_scroll: ListState,
    layer_list_ids: Vec<layer_panel::LayerListKey>,
    collapsed_layer_groups: BTreeSet<crate::panels::layers::LayerGroup>,
    expanded_layer: Option<pomelo_core::model::LayerId>,
    entity_sort: layer_panel::EntitySort,
    entity_rows: Option<layer_panel::EntityRows>,
    layer_scroll_target: Option<LayerScrollTarget>,
    display: Arc<pomelo_core::display::BoardDisplay>,
    colors: Arc<BTreeMap<pomelo_core::model::LayerId, [f32; 4]>>,
    renderer: Result<GpuPainterHandle<BoardFrame>, String>,
    telemetry: telemetry::RendererTelemetry,
    paint_error: Option<String>,
    navigation: ViewportNavigation,
    bounds: Option<Bounds<Pixels>>,
    pointer_gesture: Option<gesture::PointerGesture>,
    pointer_position: Option<Point<Pixels>>,
    camera_motion: motion::CameraMotion,
    focus: FocusHandle,
    diagnostics_navigation: crate::panels::diagnostics::NavigationFocus,
    inspector_focus: crate::panels::focus_scroll::InspectorFocus,
}

/// Left sidebar tabs, in display order; the tab bar addresses them by index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftPanelTab {
    Layers,
    Nets,
    Components,
}

impl LeftPanelTab {
    const ALL: [Self; 3] = [Self::Layers, Self::Nets, Self::Components];

    fn index(self) -> usize {
        self as usize
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Layers)
    }

    /// Search result group listed by this tab, if it lists search results.
    fn search_category(self) -> Option<pomelo_core::search::SearchCategory> {
        match self {
            Self::Layers => None,
            Self::Nets => Some(pomelo_core::search::SearchCategory::Net),
            Self::Components => Some(pomelo_core::search::SearchCategory::Component),
        }
    }
}

/// Right sidebar tabs, in display order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RightPanelTab {
    Inspector,
    Display,
    Review,
}

impl RightPanelTab {
    const ALL: [Self; 3] = [Self::Inspector, Self::Display, Self::Review];

    fn index(self) -> usize {
        self as usize
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Inspector)
    }
}

/// What the layer list should reveal on its next render. A layer is stored by
/// identity, not index, because reordering and name filtering change positions.
#[derive(Clone, Copy)]
enum LayerScrollTarget {
    Start,
    Layer(pomelo_core::model::LayerId),
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

#[derive(Clone)]
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
            review: self.review.state.clone(),
            view3d: pomelo_core::display3d::View3dState {
                active: self.view3d.active,
                camera: (self.view3d.initialized || self.view3d.camera_restored)
                    .then(|| pomelo_core::display3d::Camera3dState::capture(self.view3d.camera)),
                display: self.view3d.settings,
            },
        })
    }

    pub fn new(
        prepared: &crate::document::PreparedDocument,
        source_name: String,
        panel_layout: Entity<crate::workbench::panel_layout::PanelLayout>,
        preparation_budget: pomelo_core::memory::MemoryBudget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&panel_layout, |_, _, cx| cx.notify()).detach();
        let tracks = Arc::clone(&prepared.tracks);
        let drawings = Arc::clone(&prepared.drawings);
        let texts = prepared.texts.clone();
        let source_strokes = prepared.source_strokes.clone();
        let copper = Arc::clone(&prepared.copper);
        let placements = Arc::clone(&prepared.placements);
        let placement_outlines = Arc::clone(&prepared.placement_outlines);
        let pads = Arc::clone(&prepared.pads);
        let drills = Arc::clone(&prepared.drills);
        let scene = Arc::clone(&prepared.board.scene);
        let search = Arc::clone(&prepared.search);
        let telemetry = telemetry::RendererTelemetry::default();
        let renderer = window
            .register_gpu_painter(pomelo_render::backend::native::painter(
                telemetry.renderer().with_trace_residency(0),
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
        let top_layer = scene.layers.first().map(|layer| layer.id);
        let initial_display = pomelo_core::display::BoardDisplay {
            trace_render_mode: if std::env::var("POMELO_TRACE_RENDERER").is_ok_and(|v| v == "sdf") {
                pomelo_core::display::TraceRenderMode::Sdf
            } else {
                pomelo_core::display::TraceRenderMode::Stencil
            },
            active_layer: top_layer,
            static_shapes_fill_solid: true,
            color_mode: pomelo_core::display::ColorMode::Net,
            hidden_layers: scene
                .layers
                .iter()
                .map(|layer| layer.id)
                .chain(scene.special_layers.iter().map(|layer| layer.id))
                .chain(scene.drawing_layers.iter().map(|layer| layer.id))
                .filter(|id| Some(*id) != top_layer)
                .collect(),
            ..pomelo_core::display::BoardDisplay::default()
        };
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
                let category = this.left_panel.search_category();
                let first = this
                    .search_results
                    .iter()
                    .find(|entry| Some(entry.target.category()) == category);
                if let Some(entry) = first {
                    this.locate_search(entry.target, window, cx);
                }
            }
        })
        .detach();
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(
            &rename_input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    if let Some(target) = this.review.rename.take() {
                        let name = input.read(cx).value().to_string();
                        this.rename_review_item(target, &name, cx);
                    }
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                InputEvent::Blur if this.review.rename.take().is_some() => cx.notify(),
                _ => {}
            },
        )
        .detach();
        let layer_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&layer_input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.layer_query = input.read(cx).value().to_lowercase();
                this.layer_scroll_target = Some(LayerScrollTarget::Start);
                cx.notify();
            }
        })
        .detach();
        let entity_filter_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(
            &entity_filter_input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    this.entity_filter = input.read(cx).value().to_lowercase();
                    this.entity_highlight = 0;
                    this.entity_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.locate_highlighted_entity(window, cx),
                _ => {}
            },
        )
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
        let shadow_brightness = prepared
            .restored_view
            .as_ref()
            .map_or(0.6, |state| state.display.shadow.brightness);
        let shadow_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(shadow_brightness * 100.0)
        });
        cx.subscribe(&shadow_slider, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                Arc::make_mut(&mut this.display).shadow.brightness = value.end() / 100.0;
                cx.notify();
            }
        })
        .detach();
        let filter_pixels = prepared
            .restored_view
            .as_ref()
            .map_or(0, |state| state.display.object_filter.minimum_pixels);
        let object_filter_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(f32::from(filter_pixels))
        });
        cx.subscribe(&object_filter_slider, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                Arc::make_mut(&mut this.display)
                    .object_filter
                    .minimum_pixels = value.end().round() as u16;
                this.invalidate_hover();
                cx.notify();
            }
        })
        .detach();
        let mut viewport = Self {
            view3d: view3d::View3d::default(),
            preparation_budget,
            format_name: pomelo_import::formats::format_display_name(&prepared.board.source.format)
                .to_owned(),
            diagnostics_navigation: crate::panels::diagnostics::NavigationFocus::new(cx),
            inspector_focus: crate::panels::focus_scroll::InspectorFocus::new(cx),
            search_input,
            layer_input,
            entity_filter_input,
            entity_filter: String::new(),
            entity_highlight: 0,
            entity_scroll: UniformListScrollHandle::new(),
            layer_query: String::new(),
            input_locale: None,
            left_panel: LeftPanelTab::Layers,
            right_panel: RightPanelTab::Inspector,
            review: review::ReviewSession::default(),
            rename_input,
            source_name,
            file_information_expanded: false,
            file_information_scroll: ScrollHandle::new(),
            pan_tool: false,
            pan_start: None,
            wheel_history_at: None,
            panel_sizes: panel_layout.read(cx).sizes.clone(),
            panel_layout,
            copper_slider,
            global_slider,
            shadow_slider,
            object_filter_slider,
            last_visibility: None,
            net_color_target: None,
            color_pickers: BTreeMap::new(),
            search_results: Vec::new(),
            selected_target: None,
            selected_anchor: None,
            restoring_selection: None,
            selected_source_labels: Vec::new(),
            selected_related_net: None,
            selected_related_component: None,
            selection_mode: pomelo_core::interaction::SelectionMode::Object,
            last_pick: None,
            candidate_position: None,
            selected_summary: None,
            selected_members: None,
            hovered: None,
            hover_selection: None,
            hover_notice: None,
            hover_tooltip_ready: false,
            hover_cancel: pomelo_core::task::CancellationToken::default(),
            hover_task: None,
            pick_filter: pomelo_core::picking::PickFilter::all(),
            members_offset: 0,
            members_page_revision: 0,
            members_scroll: ScrollHandle::new(),
            members_navigation: crate::panels::diagnostics::NavigationFocus::new(cx),
            selected_pins: Arc::new(BTreeSet::new()),
            selected_related_objects: Arc::new(BTreeSet::new()),
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
            // Drawing families are long and rarely needed first.
            collapsed_layer_groups: BTreeSet::from([crate::panels::layers::LayerGroup::Drawings]),
            expanded_layer: None,
            entity_sort: layer_panel::EntitySort::default(),
            entity_rows: None,
            layer_scroll_target: None,
            tracks,
            zone_outlines: Arc::clone(&prepared.zone_outlines),
            drawings,
            copper,
            placements,
            placement_outlines,
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
            texts,
            source_strokes,
            label_index: prepared.label_index.clone(),
            labels: labels::LabelState::default(),
            curve_fill: curves::CurveState::default(),
            paint_error: None,
            navigation: ViewportNavigation::default(),
            bounds: None,
            pointer_gesture: None,
            pointer_position: None,
            camera_motion: Default::default(),
            focus: cx.focus_handle(),
        };
        if let Some(state) = &prepared.restored_view
            && viewport.navigation.restore_camera(state.camera)
        {
            viewport.display = Arc::new(state.display.clone());
            viewport.review.state = state.review.clone();
            viewport.review.refresh_colors();
            viewport.validate_review_state(window, cx);
            viewport.selection_mode = state.selection_mode;
            viewport.pick_filter = state.pick_filter;
            viewport.view3d.settings = state.view3d.display;
            if let Some(camera) = state.view3d.camera.and_then(|camera| camera.camera()) {
                viewport.view3d.camera = camera;
                viewport.view3d.camera_restored = true;
            }
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
        let reopen_3d = prepared
            .restored_view
            .as_ref()
            .is_some_and(|state| state.view3d.active);
        if (reopen_3d || std::env::var_os("POMELO_3D_PREVIEW").is_some())
            && pomelo_render::backend::native::SolidRenderer::supported()
        {
            viewport.toggle_3d(window, cx);
        }
        viewport
    }
}

impl Drop for BoardViewport {
    fn drop(&mut self) {
        self.search_cancel.cancel();
        self.hover_cancel.cancel();
    }
}

impl Focusable for BoardViewport {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
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
