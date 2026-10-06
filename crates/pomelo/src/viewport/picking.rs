//! Pointer picking and candidate cycling.
use super::*;

impl BoardViewport {
    pub(super) fn set_selection_mode(
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

    pub(super) fn pick_trace(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let local = position - bounds.origin;
        // A normal click keeps selecting Web's topmost hit. Candidate cycling
        // is an explicit keyboard command, not a side effect of repeated clicks.
        self.last_pick = None;
        self.pick_at(
            pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y)),
            window,
            cx,
        );
    }

    pub(super) fn next_candidate(
        &mut self,
        _: &NextCandidate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn pick_at(
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
}
