//! Search requests, selection preparation and member paging.
use super::*;

impl BoardViewport {
    pub(super) fn query_search(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn locate_search(
        &mut self,
        target: pomelo_core::search::SearchTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locate_target(target.into(), window, cx);
    }

    pub(super) fn locate_target(
        &mut self,
        target: pomelo_core::selection::SelectionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_target(target, true, window, cx);
    }

    pub(super) fn prepare_target(
        &mut self,
        target: pomelo_core::selection::SelectionTarget,
        move_camera: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_anchored_target(target, move_camera, None, None, window, cx);
    }

    pub(super) fn prepare_anchored_target(
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

    pub(super) fn page_members(
        &mut self,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn leave_search(
        &mut self,
        _: &LeaveSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_cancel.cancel();
        self.search_pending = false;
        self.locate_pending = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn clear_selection(
        &mut self,
        _: &ClearSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
}
