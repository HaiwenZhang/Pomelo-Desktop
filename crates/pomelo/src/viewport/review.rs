//! Review tools on the canvas: measurement, clearance, selection sets, highlight
//! groups, named views and navigation history. Geometry work runs off the UI thread;
//! results are published only if their request is still current.
use super::*;
use pomelo_core::{
    geometry::PathError,
    review::{
        ReviewState,
        clearance::{ClearanceError, object_clearance},
        export::ClearanceRecord,
        highlight::{HighlightGroup, MAX_HIGHLIGHT_GROUPS, ObjectCatalog, PALETTE, object_colors},
        measure::{MeasureKind, MeasurePoint, Measurement, SnapRequest, snap_point},
        selection_set::{
            BoxMode, MAX_SELECTION_OBJECTS, SelectionSet, objects_in_box, target_objects,
        },
        views::{MAX_NAMED_VIEWS, NamedView, NavigationHistory},
    },
    selection::SelectedObject,
    task::CancellationToken,
};

/// Canvas tools beyond select and pan. Only one is active at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewTool {
    Measure(MeasureKind),
    Clearance,
    BoxSelect,
    ZoomBox,
}

/// A rectangle being dragged by the box-select or zoom tool, in window pixels.
#[derive(Debug, Clone, Copy)]
pub(super) struct AreaDrag {
    pub start: Point<Pixels>,
    pub current: Point<Pixels>,
}

/// What a box selection does with the objects it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetEdit {
    Replace,
    Add,
    Remove,
    /// Remove all of them when every one is already selected, otherwise add them.
    Toggle,
}

/// Which list in the review panel is being renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenameTarget {
    Group(u32),
    View(u32),
}

#[derive(Default)]
pub(super) struct ReviewSession {
    pub tool: Option<ReviewTool>,
    /// Points placed so far by the active measure tool.
    pub active: Option<Measurement>,
    /// Snapped pointer position for the next point; never committed by itself.
    pub preview: Option<MeasurePoint>,
    pub measurements: Vec<Measurement>,
    pub clearance_first: Option<SelectedObject>,
    pub clearance: Option<ClearanceRecord>,
    pub clearance_pending: bool,
    pub clearance_notice: Option<Message>,
    pub drag: Option<AreaDrag>,
    pub zoom_anchor: Option<Point<Pixels>>,
    pub free_measure: bool,
    pub orthogonal_measure: bool,
    pub set: SelectionSet,
    pub set_offset: usize,
    pub set_pending: bool,
    pub set_notice: Option<Message>,
    /// Highlight groups and named views, saved with the document view.
    pub state: ReviewState,
    pub missing: BTreeMap<u32, usize>,
    pub object_colors: Option<Arc<pomelo_render::backend::native::ObjectColors>>,
    pub history: NavigationHistory,
    pub notice: Option<Message>,
    pub rename: Option<RenameTarget>,
    pub clearance_cancel: CancellationToken,
    pub set_cancel: CancellationToken,
    pub catalog_cancel: CancellationToken,
    pub clearance_task: Option<Task<()>>,
    pub set_task: Option<Task<()>>,
    pub catalog_task: Option<Task<()>>,
    pub export: super::review_export::ExportSession,
}

impl ReviewSession {
    /// Exit the current tool without removing measurements already completed.
    fn cancel_tool(&mut self) -> bool {
        let active = self.tool.is_some()
            || self.active.is_some()
            || self.drag.is_some()
            || self.clearance_first.is_some()
            || self.clearance_pending;
        self.clearance_cancel.cancel();
        self.clearance_task = None;
        self.clearance_pending = false;
        self.tool = None;
        self.active = None;
        self.preview = None;
        self.drag = None;
        self.zoom_anchor = None;
        self.clearance_first = None;
        self.clearance_notice = None;
        active
    }

    pub(super) fn clear_set(&mut self) {
        self.set_cancel.cancel();
        self.set_task = None;
        self.set_pending = false;
        self.set.clear();
        self.set_offset = 0;
        self.set_notice = None;
        if self.tool == Some(ReviewTool::BoxSelect) {
            self.cancel_tool();
        }
    }

    /// Rebuild the per-object color map after a group edit.
    pub fn refresh_colors(&mut self) {
        let colors = object_colors(&self.state.highlight_groups);
        self.object_colors = (!colors.is_empty()).then(|| {
            Arc::new(
                colors
                    .into_iter()
                    .map(|(object, color)| {
                        let [r, g, b, _] = color.rgba();
                        (object, [r, g, b])
                    })
                    .collect(),
            )
        });
    }

    fn cancel_work(&mut self) {
        self.clearance_cancel.cancel();
        self.set_cancel.cancel();
        self.catalog_cancel.cancel();
    }
}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.cancel_work();
        self.export.cancel.cancel();
    }
}

/// Length in the display unit with a fixed precision, followed by the unit label.
pub(super) fn format_length(
    millimeters: f64,
    unit: pomelo_core::units::LengthUnit,
    locale: pomelo_core::i18n::Locale,
) -> String {
    use pomelo_core::units::LengthUnit;
    let value = unit.from_millimeters(millimeters);
    let (number, key) = match unit {
        LengthUnit::Millimeters => (format!("{value:.4}"), Key::UnitMillimeters),
        LengthUnit::Mils => (format!("{value:.2}"), Key::UnitMils),
    };
    // Avoid "-0.0000" for values that round to zero.
    let number = if number
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        number.trim_start_matches('-').to_owned()
    } else {
        number
    };
    format!("{number} {}", text(locale, key))
}

pub(super) fn format_angle(degrees: f64) -> String {
    let value = format!("{degrees:.2}");
    if value == "-0.00" {
        "0.00".into()
    } else {
        value
    }
}

impl BoardViewport {
    fn board_point(&self, position: Point<Pixels>) -> Option<pomelo_core::model::Point> {
        let bounds = self.bounds?;
        let local = position - bounds.origin;
        let size = self.navigation.size();
        let camera = self.navigation.camera();
        let point = camera.view_to_board(
            pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            size.x,
            size.y,
        );
        point.is_finite().then_some(point)
    }

    pub(super) fn set_review_tool(
        &mut self,
        tool: Option<ReviewTool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            return;
        }
        let review = &mut self.review;
        // Selecting the active tool again turns it off.
        let tool = if review.tool == tool { None } else { tool };
        review.cancel_tool();
        review.tool = tool;
        review.active = match tool {
            Some(ReviewTool::Measure(kind)) => Some(Measurement::new(kind)),
            _ => None,
        };
        review.preview = None;
        review.drag = None;
        review.clearance_first = None;
        review.clearance_notice = None;
        if tool.is_some() {
            self.pan_tool = false;
            self.pointer_gesture = None;
            self.invalidate_hover();
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Cancel the active gesture and leave the tool in a single operation.
    pub(super) fn cancel_review_tool(&mut self, cx: &mut Context<Self>) -> bool {
        let cancelled = self.review.cancel_tool();
        cx.notify();
        cancelled
    }

    pub(super) fn cancel_canvas_action(
        &mut self,
        _: &CancelCanvasAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review.tool == Some(ReviewTool::BoxSelect) || !self.cancel_review_tool(cx) {
            self.clear_selection(&ClearSelection, window, cx);
        }
        window.focus(&self.focus, cx);
    }

    pub(super) fn finish_measurement(
        &mut self,
        _: &FinishMeasurement,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let review = &mut self.review;
        if let Some(active) = review.active.take() {
            let kind = active.kind;
            if active.is_complete_shape() {
                review.measurements.push(active);
            }
            review.active = review.tool.map(|_| Measurement::new(kind));
            review.preview = None;
            cx.notify();
        }
    }

    pub(super) fn remove_last_point(
        &mut self,
        _: &RemoveLastPoint,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active) = &mut self.review.active
            && active.pop().is_some()
        {
            cx.notify();
        }
    }

    /// Snap a pointer position for measurement on the UI thread; the query is indexed
    /// and limited to a handful of candidates under the pointer.
    fn snap_at(&self, position: Point<Pixels>) -> Option<MeasurePoint> {
        let mut point = self.board_point(position)?;
        if self.review.orthogonal_measure
            && let Some(last) = self.review.active.as_ref().and_then(|m| m.points.last())
        {
            let dx = (point.x - last.position.x).abs();
            let dy = (point.y - last.position.y).abs();
            if dx >= dy {
                point.y = last.position.y;
            } else {
                point.x = last.position.x;
            }
            return Some(MeasurePoint::free(point));
        }
        if self.review.free_measure {
            return Some(MeasurePoint::free(point));
        }
        let camera = self.navigation.camera();
        let request = SnapRequest {
            point,
            radius_mm: 8.0 / camera.pixels_per_mm,
            pixels_per_mm: camera.pixels_per_mm,
            filter: self.pick_filter,
        };
        Some(
            snap_point(
                &self.scene,
                &self.picking,
                request,
                &self.display,
                &CancellationToken::default(),
            )
            .unwrap_or_else(|_| MeasurePoint::free(point)),
        )
    }

    /// Pointer movement with a review tool: preview snapping or rectangle dragging.
    pub(super) fn move_review_pointer(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.review.drag {
            drag.current = position;
            cx.notify();
            return;
        }
        if matches!(self.review.tool, Some(ReviewTool::Measure(_))) {
            let preview = self.snap_at(position);
            if preview != self.review.preview {
                self.review.preview = preview;
                cx.notify();
            }
        }
    }

    /// Start a rectangle for the box-select and zoom tools; returns true when handled.
    pub(super) fn start_review_drag(&mut self, event: &MouseDownEvent) -> bool {
        if event.button != MouseButton::Left
            || !matches!(
                self.review.tool,
                Some(ReviewTool::BoxSelect | ReviewTool::ZoomBox)
            )
        {
            return false;
        }
        self.review.drag = Some(AreaDrag {
            start: self.review.zoom_anchor.unwrap_or(event.position),
            current: event.position,
        });
        true
    }

    pub(super) fn finish_review_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.review.drag.take() else {
            return false;
        };
        cx.notify();
        let distance = event.position - drag.start;
        if self.review.tool == Some(ReviewTool::ZoomBox)
            && self.review.zoom_anchor.is_none()
            && f32::from(distance.x).abs() < 4.0
            && f32::from(distance.y).abs() < 4.0
        {
            self.review.zoom_anchor = Some(event.position);
            // Waiting for the second click is a preview, not a captured drag.
            // Toolbar buttons must remain clickable between the two points.
            return true;
        }
        let small = f32::from(distance.x).abs() < 4.0 && f32::from(distance.y).abs() < 4.0;
        let (Some(a), Some(b)) = (
            self.board_point(drag.start),
            self.board_point(event.position),
        ) else {
            return true;
        };
        let area = pomelo_core::model::Bounds {
            min: pomelo_core::model::Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: pomelo_core::model::Point::new(a.x.max(b.x), a.y.max(b.y)),
        };
        match self.review.tool {
            Some(ReviewTool::ZoomBox) if !small => {
                self.review.zoom_anchor = None;
                self.review.tool = None;
                self.wheel_history_at = None;
                let previous = self.navigation.camera();
                if self.navigation.zoom_to(area) {
                    self.review
                        .history
                        .record(previous, self.navigation.camera());
                    self.invalidate_hover();
                }
            }
            Some(ReviewTool::BoxSelect) if small => {
                let edit = if event.modifiers.alt {
                    SetEdit::Remove
                } else {
                    SetEdit::Toggle
                };
                self.edit_set_at(event.position, edit, window, cx);
            }
            Some(ReviewTool::BoxSelect) => {
                // Drag direction on screen decides the mode, independent of a flipped view.
                let mode = BoxMode::from_drag(
                    pomelo_core::model::Point::new(f64::from(drag.start.x), 0.0),
                    pomelo_core::model::Point::new(f64::from(event.position.x), 0.0),
                );
                let edit = if event.modifiers.alt {
                    SetEdit::Remove
                } else if event.modifiers.shift {
                    SetEdit::Add
                } else {
                    SetEdit::Replace
                };
                self.select_box(area, mode, edit, window, cx);
            }
            _ => {}
        }
        true
    }

    /// A click with a measure or clearance tool; returns true when the tool consumed it.
    pub(super) fn review_click(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.review.tool {
            Some(ReviewTool::Measure(kind)) => {
                self.review.free_measure = event.modifiers.alt;
                self.review.orthogonal_measure = event.modifiers.shift;
                let Some(point) = self.snap_at(event.position) else {
                    return true;
                };
                let review = &mut self.review;
                let active = review.active.get_or_insert_with(|| Measurement::new(kind));
                if active
                    .points
                    .last()
                    .is_none_or(|last| last.position != point.position)
                {
                    active.push(point);
                }
                let double = event.click_count >= 2 && kind == MeasureKind::Polyline;
                if (kind == MeasureKind::Distance && active.points.len() == 2) || double {
                    self.finish_measurement(&FinishMeasurement, window, cx);
                }
                cx.notify();
                true
            }
            Some(ReviewTool::Clearance) => {
                self.pick_clearance_object(event.position, window, cx);
                true
            }
            Some(ReviewTool::BoxSelect | ReviewTool::ZoomBox) => true,
            None if event.modifiers.shift => {
                self.edit_set_at(event.position, SetEdit::Toggle, window, cx);
                true
            }
            None => false,
        }
    }

    fn object_at(&self, position: Point<Pixels>) -> Result<Option<SelectedObject>, PathError> {
        let Some(bounds) = self.bounds else {
            return Ok(None);
        };
        let local = position - bounds.origin;
        let camera = self.navigation.camera();
        let Some(query) = pomelo_core::picking::PickQuery::from_viewport(
            camera,
            pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            self.navigation.size(),
            5.0,
        ) else {
            return Ok(None);
        };
        Ok(self
            .picking
            .query_visible_objects(
                query,
                camera.pixels_per_mm,
                self.pick_filter,
                &self.display,
                1,
                &CancellationToken::default(),
            )?
            .first()
            .map(|hit| hit.object))
    }

    fn pick_clearance_object(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let review = &mut self.review;
        review.clearance_notice = None;
        let object = match self.object_at(position) {
            Ok(Some(object)) => object,
            Ok(None) => {
                self.review.clearance_notice = Some(Message::new(Key::ClearanceNoObject));
                cx.notify();
                return;
            }
            Err(_) => {
                self.review.clearance_notice = Some(Message::new(Key::PickFailed));
                cx.notify();
                return;
            }
        };
        let review = &mut self.review;
        let Some(first) = review.clearance_first.take() else {
            review.clearance_first = Some(object);
            cx.notify();
            return;
        };
        review.clearance_cancel.cancel();
        review.clearance_cancel = CancellationToken::default();
        review.clearance_pending = true;
        let cancel = review.clearance_cancel.clone();
        let worker_cancel = cancel.clone();
        let scene = Arc::clone(&self.scene);
        let display = Arc::clone(&self.display);
        let worker = cx.background_spawn(async move {
            object_clearance(&scene, first, object, &display, &worker_cancel)
        });
        self.review.clearance_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                let review = &mut this.review;
                review.clearance_pending = false;
                let describe = |object| {
                    let (kind, id) =
                        selection_identity(pomelo_core::selection::SelectionTarget::Object(object));
                    Message::new(Key::ReviewObjectLabel)
                        .arg("kind", Message::new(kind).display(i18n::current(cx)))
                        .arg("id", id)
                };
                match result {
                    Ok(result) => {
                        review.clearance = Some(ClearanceRecord {
                            first,
                            second: object,
                            result,
                        });
                    }
                    Err(ClearanceError::Cancelled) => return,
                    Err(ClearanceError::SameObject) => {
                        review.clearance_notice = Some(Message::new(Key::ClearanceSameObject));
                    }
                    Err(ClearanceError::Limit) => {
                        review.clearance_notice = Some(Message::new(Key::ClearanceLimit));
                    }
                    Err(ClearanceError::Missing(object)) => {
                        review.clearance_notice = Some(
                            Message::new(Key::ClearanceMissing)
                                .arg("object", describe(object).display(i18n::current(cx))),
                        );
                    }
                    Err(ClearanceError::Unsupported(object)) => {
                        review.clearance_notice = Some(
                            Message::new(Key::ClearanceUnsupported)
                                .arg("object", describe(object).display(i18n::current(cx))),
                        );
                    }
                    Err(ClearanceError::Invalid(object)) => {
                        review.clearance_notice = Some(
                            Message::new(Key::ClearanceInvalid)
                                .arg("object", describe(object).display(i18n::current(cx))),
                        );
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn apply_set_edit(&mut self, objects: Vec<SelectedObject>, truncated: bool, edit: SetEdit) {
        let review = &mut self.review;
        let mut truncated = truncated;
        match edit {
            SetEdit::Replace => {
                review.set.clear();
                truncated |= review.set.extend(objects).truncated;
            }
            SetEdit::Add => truncated |= review.set.extend(objects).truncated,
            SetEdit::Remove => {
                review.set.remove_all(objects);
            }
            SetEdit::Toggle => {
                if !objects.is_empty() && objects.iter().all(|object| review.set.contains(*object))
                {
                    review.set.remove_all(objects);
                } else {
                    truncated |= review.set.extend(objects).truncated;
                }
            }
        }
        review.set_offset = review
            .set_offset
            .min(review.set.len().saturating_sub(1) / SET_PAGE * SET_PAGE);
        review.set_notice = truncated
            .then(|| Message::new(Key::SelectionSetTruncated).arg("limit", MAX_SELECTION_OBJECTS));
    }

    /// Run a set edit in the background with a fresh cancellation token.
    fn run_set_task(
        &mut self,
        edit: SetEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&CancellationToken) -> Result<(Vec<SelectedObject>, bool), PathError>
        + Send
        + 'static,
    ) {
        let review = &mut self.review;
        review.set_cancel.cancel();
        review.set_cancel = CancellationToken::default();
        review.set_pending = true;
        review.set_notice = None;
        let cancel = review.set_cancel.clone();
        let worker_cancel = cancel.clone();
        let worker = cx.background_spawn(async move { work(&worker_cancel) });
        self.review.set_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                this.review.set_pending = false;
                match result {
                    Ok((objects, truncated)) => this.apply_set_edit(objects, truncated, edit),
                    Err(PathError::Cancelled) => return,
                    Err(_) => this.review.set_notice = Some(Message::new(Key::PickFailed)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_box(
        &mut self,
        area: pomelo_core::model::Bounds,
        mode: BoxMode,
        edit: SetEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scene = Arc::clone(&self.scene);
        let display = Arc::clone(&self.display);
        let filter = self.pick_filter;
        self.run_set_task(edit, window, cx, move |cancel| {
            let selection = objects_in_box(
                &scene,
                area,
                mode,
                filter,
                &display,
                MAX_SELECTION_OBJECTS,
                cancel,
            )?;
            Ok((selection.objects, selection.truncated))
        });
    }

    /// Toggle the target under the pointer, expanded by the current selection mode.
    fn edit_set_at(
        &mut self,
        position: Point<Pixels>,
        edit: SetEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(Some(object)) = self.object_at(position) else {
            return;
        };
        let scene = Arc::clone(&self.scene);
        let mode = self.selection_mode;
        self.run_set_task(edit, window, cx, move |cancel| {
            let target = pomelo_core::selection::canvas_target(&scene, object, mode, cancel)?;
            target_objects(&scene, target, MAX_SELECTION_OBJECTS, cancel)
        });
    }

    pub(super) fn add_current_selection_to_set(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.selected_target else {
            self.review.set_notice = Some(Message::new(Key::HighlightNothing));
            cx.notify();
            return;
        };
        let scene = Arc::clone(&self.scene);
        self.run_set_task(SetEdit::Add, window, cx, move |cancel| {
            target_objects(&scene, target, MAX_SELECTION_OBJECTS, cancel)
        });
    }

    pub(super) fn clear_selection_set(&mut self, cx: &mut Context<Self>) {
        self.review.clear_set();
        cx.notify();
    }

    pub(super) fn remove_from_set(&mut self, object: SelectedObject, cx: &mut Context<Self>) {
        if self.review.set.remove(object) {
            let review = &mut self.review;
            review.set_offset = review
                .set_offset
                .min(review.set.len().saturating_sub(1) / SET_PAGE * SET_PAGE);
            cx.notify();
        }
    }

    pub(super) fn page_set(&mut self, offset: usize, cx: &mut Context<Self>) {
        let last = self.review.set.len().saturating_sub(1) / SET_PAGE * SET_PAGE;
        self.review.set_offset = offset.min(last);
        cx.notify();
    }

    /// Selection snapshot rendered with the selection overlay: source bonds plus the set.
    pub(super) fn related_highlight(&self) -> Arc<BTreeSet<SelectedObject>> {
        let set = self.review.set.objects();
        if set.is_empty() {
            Arc::clone(&self.selected_related_objects)
        } else if self.selected_related_objects.is_empty() {
            Arc::clone(set)
        } else {
            Arc::new(set.union(&self.selected_related_objects).copied().collect())
        }
    }

    pub(super) fn create_highlight_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.state.highlight_groups.len() >= MAX_HIGHLIGHT_GROUPS {
            self.review.notice =
                Some(Message::new(Key::HighlightLimit).arg("limit", MAX_HIGHLIGHT_GROUPS));
            cx.notify();
            return;
        }
        if !self.review.set.is_empty() {
            let members: Vec<_> = self.review.set.objects().iter().copied().collect();
            self.push_highlight_group(members, cx);
            return;
        }
        let Some(target) = self.selected_target else {
            self.review.notice = Some(Message::new(Key::HighlightNothing));
            cx.notify();
            return;
        };
        let scene = Arc::clone(&self.scene);
        let cancel = self.review.set_cancel.clone();
        let worker_cancel = cancel.clone();
        let worker = cx.background_spawn(async move {
            target_objects(&scene, target, MAX_SELECTION_OBJECTS, &worker_cancel)
        });
        self.review.set_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                if let Ok((members, _)) = result {
                    this.push_highlight_group(members, cx);
                }
            });
        }));
    }

    fn push_highlight_group(&mut self, members: Vec<SelectedObject>, cx: &mut Context<Self>) {
        use pomelo_core::review::highlight::MAX_DOCUMENT_MEMBERS;
        // Groups are saved with the view, so the document-wide budget is checked first.
        if self.review.state.member_count() + members.len() > MAX_DOCUMENT_MEMBERS {
            self.review.notice =
                Some(Message::new(Key::HighlightTooLarge).arg("limit", MAX_DOCUMENT_MEMBERS));
            cx.notify();
            return;
        }
        let locale = i18n::current(cx);
        let state = &mut self.review.state;
        let index = state.highlight_groups.len();
        let id = state.allocate_id();
        let name = Message::new(Key::HighlightDefaultName)
            .arg("index", id as u64 + 1)
            .display(locale);
        let group = HighlightGroup::new(id, name, PALETTE[index % PALETTE.len()], members);
        state.highlight_groups.insert(0, group);
        self.review.missing.insert(id, 0);
        self.review.notice = None;
        self.review.refresh_colors();
        cx.notify();
    }

    pub(super) fn edit_group(
        &mut self,
        id: u32,
        edit: impl FnOnce(&mut HighlightGroup),
        cx: &mut Context<Self>,
    ) {
        if let Some(group) = self
            .review
            .state
            .highlight_groups
            .iter_mut()
            .find(|group| group.id == id)
        {
            edit(group);
            self.review.refresh_colors();
            cx.notify();
        }
    }

    pub(super) fn delete_group(&mut self, id: u32, cx: &mut Context<Self>) {
        self.review
            .state
            .highlight_groups
            .retain(|group| group.id != id);
        self.review.missing.remove(&id);
        self.review.refresh_colors();
        cx.notify();
    }

    pub(super) fn select_group_members(&mut self, id: u32, cx: &mut Context<Self>) {
        let Some(group) = self
            .review
            .state
            .highlight_groups
            .iter()
            .find(|group| group.id == id)
        else {
            return;
        };
        let members = group.members.clone();
        let truncated = self.review.set.extend(members).truncated;
        self.review.set_notice = truncated
            .then(|| Message::new(Key::SelectionSetTruncated).arg("limit", MAX_SELECTION_OBJECTS));
        cx.notify();
    }

    /// Count saved references that no longer exist, off the UI thread.
    pub(super) fn validate_review_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.state.highlight_groups.is_empty() {
            return;
        }
        self.review.catalog_cancel.cancel();
        self.review.catalog_cancel = CancellationToken::default();
        let cancel = self.review.catalog_cancel.clone();
        let worker_cancel = cancel.clone();
        let scene = Arc::clone(&self.scene);
        let groups = self.review.state.highlight_groups.clone();
        let worker = cx.background_spawn(async move {
            let catalog = ObjectCatalog::build(&scene, &worker_cancel)?;
            Ok::<_, PathError>(
                groups
                    .iter()
                    .map(|group| (group.id, catalog.missing_members(group)))
                    .collect::<BTreeMap<_, _>>(),
            )
        });
        self.review.catalog_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if let (false, Ok(missing)) = (cancel.is_cancelled(), result) {
                    this.review.missing = missing;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn save_named_view(&mut self, cx: &mut Context<Self>) {
        if self.review.state.named_views.len() >= MAX_NAMED_VIEWS {
            self.review.notice =
                Some(Message::new(Key::NamedViewLimit).arg("limit", MAX_NAMED_VIEWS));
            cx.notify();
            return;
        }
        let locale = i18n::current(cx);
        let state = &mut self.review.state;
        let id = state.allocate_id();
        let name = Message::new(Key::NamedViewDefaultName)
            .arg("index", state.named_views.len() + 1)
            .display(locale);
        let view = NamedView::capture(id, &name, self.navigation.camera(), &self.display);
        if view.valid() {
            state.named_views.push(view);
        }
        self.review.notice = None;
        cx.notify();
    }

    pub(super) fn update_named_view(&mut self, id: u32, cx: &mut Context<Self>) {
        let camera = self.navigation.camera();
        if let Some(view) = self
            .review
            .state
            .named_views
            .iter_mut()
            .find(|view| view.id == id)
        {
            let name = view.name.clone();
            *view = NamedView::capture(id, &name, camera, &self.display);
            cx.notify();
        }
    }

    pub(super) fn restore_named_view(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .review
            .state
            .named_views
            .iter()
            .find(|view| view.id == id)
            .cloned()
        else {
            return;
        };
        let previous = self.navigation.camera();
        self.wheel_history_at = None;
        if self.navigation.restore_camera(view.camera) {
            self.review.history.record(previous, view.camera);
        }
        if view.apply_display(Arc::make_mut(&mut self.display)) {
            // Picker entities retain their own values; rebuild them from the restored materials.
            self.color_pickers.clear();
            for (slider, value) in [
                (&self.copper_slider, self.display.copper_opacity * 100.0),
                (&self.global_slider, self.display.global_opacity * 100.0),
                (&self.shadow_slider, self.display.shadow.brightness * 100.0),
                (
                    &self.object_filter_slider,
                    f32::from(self.display.object_filter.minimum_pixels),
                ),
            ] {
                slider.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
        self.refresh_measure_preview(cx);
        self.invalidate_hover();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn delete_named_view(&mut self, id: u32, cx: &mut Context<Self>) {
        self.review.state.named_views.retain(|view| view.id != id);
        cx.notify();
    }

    pub(super) fn rename_review_item(
        &mut self,
        target: RenameTarget,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let name = pomelo_core::review::highlight::normalized_name(name);
        if name.is_empty() {
            return;
        }
        match target {
            RenameTarget::Group(id) => self.edit_group(
                id,
                |group| {
                    group.rename(&name);
                },
                cx,
            ),
            RenameTarget::View(id) => {
                if let Some(view) = self
                    .review
                    .state
                    .named_views
                    .iter_mut()
                    .find(|view| view.id == id)
                {
                    view.name = name;
                    cx.notify();
                }
            }
        }
    }

    pub(super) fn navigate_back(
        &mut self,
        _: &NavigateBack,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            return;
        }
        self.wheel_history_at = None;
        let current = self.navigation.camera();
        if let Some(camera) = self.review.history.back(current)
            && self.navigation.restore_camera(camera)
        {
            self.refresh_measure_preview(cx);
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn navigate_forward(
        &mut self,
        _: &NavigateForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            return;
        }
        self.wheel_history_at = None;
        let current = self.navigation.camera();
        if let Some(camera) = self.review.history.forward(current)
            && self.navigation.restore_camera(camera)
        {
            self.refresh_measure_preview(cx);
            self.invalidate_hover();
            cx.notify();
        }
    }

    /// Locate a target and record the move as one history step.
    pub(super) fn locate_with_history(&mut self, bounds: pomelo_core::model::Bounds) -> bool {
        self.wheel_history_at = None;
        let previous = self.navigation.camera();
        let moved = self.navigation.locate(bounds);
        if moved {
            self.review
                .history
                .record(previous, self.navigation.camera());
        }
        moved
    }

    pub(super) fn clear_measurements(&mut self, cx: &mut Context<Self>) {
        let review = &mut self.review;
        if matches!(
            review.tool,
            Some(ReviewTool::Measure(_) | ReviewTool::Clearance)
        ) {
            review.cancel_tool();
        }
        review.measurements.clear();
        review.clearance = None;
        review.clearance_notice = None;
        cx.notify();
    }
}

/// Rows per page of the selection-set member list.
pub(super) const SET_PAGE: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::{i18n::Locale, units::LengthUnit};

    #[::core::prelude::v1::test]
    fn cancelling_measurement_exits_in_one_step_and_keeps_completed_results() {
        let mut measurement = Measurement::new(MeasureKind::Distance);
        measurement.push(MeasurePoint::free(pomelo_core::model::Point::new(0.0, 0.0)));
        measurement.push(MeasurePoint::free(pomelo_core::model::Point::new(3.0, 4.0)));
        let mut active = Measurement::new(MeasureKind::Polyline);
        active.push(MeasurePoint::free(pomelo_core::model::Point::new(1.0, 2.0)));
        let mut review = ReviewSession::default();
        review.tool = Some(ReviewTool::Measure(MeasureKind::Polyline));
        review.active = Some(active);
        review.preview = Some(MeasurePoint::free(pomelo_core::model::Point::new(4.0, 5.0)));
        review.measurements.push(measurement);
        assert!(review.cancel_tool());
        assert!(review.tool.is_none());
        assert!(review.active.is_none());
        assert!(review.preview.is_none());
        assert_eq!(review.measurements[0].total_length(), 5.0);
        assert!(!review.cancel_tool());
    }

    #[::core::prelude::v1::test]
    fn cancelling_clearance_invalidates_pending_background_results() {
        let mut review = ReviewSession::default();
        review.tool = Some(ReviewTool::Clearance);
        review.clearance_pending = true;
        review.clearance_first = Some(SelectedObject::Pin(pomelo_core::model::ObjectId(7)));
        let pending = review.clearance_cancel.clone();
        assert!(review.cancel_tool());
        assert!(pending.is_cancelled());
        assert!(!review.clearance_pending);
        assert!(review.clearance_first.is_none());
        assert!(review.tool.is_none());
    }

    #[::core::prelude::v1::test]
    fn clearing_box_selection_also_cancels_pending_selection_and_leaves_box_mode() {
        let object = SelectedObject::Pin(pomelo_core::model::ObjectId(7));
        let mut review = ReviewSession::default();
        review.tool = Some(ReviewTool::BoxSelect);
        review.set_pending = true;
        review.set_offset = SET_PAGE;
        review.drag = Some(AreaDrag {
            start: point(px(0.0), px(0.0)),
            current: point(px(20.0), px(20.0)),
        });
        review.set.insert(object);
        review.state.highlight_groups.push(HighlightGroup {
            id: 1,
            name: "Saved group".into(),
            color: PALETTE[0],
            visible: true,
            members: vec![object],
        });
        let pending = review.set_cancel.clone();
        review.clear_set();
        assert!(pending.is_cancelled());
        assert!(!review.set_pending);
        assert!(review.set.is_empty());
        assert_eq!(review.set_offset, 0);
        assert!(review.tool.is_none());
        assert!(review.drag.is_none());
        assert_eq!(review.state.highlight_groups[0].members, [object]);
    }

    #[::core::prelude::v1::test]
    fn lengths_use_fixed_precision_and_never_show_negative_zero() {
        assert_eq!(
            format_length(1.23456, LengthUnit::Millimeters, Locale::English),
            "1.2346 mm"
        );
        assert_eq!(
            format_length(25.4, LengthUnit::Mils, Locale::English),
            "1000.00 Mils"
        );
        assert_eq!(
            format_length(-0.00001, LengthUnit::Millimeters, Locale::English),
            "0.0000 mm"
        );
        assert_eq!(format_angle(-0.001), "0.00");
        assert_eq!(format_angle(90.0), "90.00");
    }
}
