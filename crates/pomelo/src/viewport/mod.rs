//! Native desktop board canvas. Business GPU resources stay in pomelo-render.
use crate::panels::focus_scroll::FocusScroll;
use crate::tooltips::ButtonTooltipExt;
mod appearance;
mod curves;
mod diagnostics_panel;
mod frame;
mod gesture;
mod hover;
mod inspector_panel;
mod layer_panel;
mod layout;
mod navigation;
mod opacity;
mod picking;
mod scale_label;
mod search;
mod toolbar;

pub use crate::actions::FocusSearch;
use crate::i18n;
use crate::panels::inspector::{prepare_inspection, selection_identity};
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
    label_cache: Option<LabelCache>,
    curve_fill: curves::CurveState,
    copper: Arc<pomelo_render::copper::PreparedCopper>,
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
    pointer_gesture: Option<gesture::PointerGesture>,
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
                .with_label_telemetry(Arc::clone(&label_telemetry))
                .with_trace_residency(64 * 1024 * 1024),
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
            format_name: pomelo_import::formats::format_display_name(&prepared.board.source.format)
                .to_owned(),
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
            pointer_gesture: None,
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
