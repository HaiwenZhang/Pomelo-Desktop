//! Cancellable hover picking and delayed tooltips.
use super::*;

impl BoardViewport {
    pub(super) fn invalidate_hover(&mut self) {
        self.hover_selection = None;
        self.hover_cancel.cancel();
        self.hover_task = None;
        self.hovered = None;
        self.hover_notice = None;
        self.hover_tooltip_ready = false;
    }

    pub(super) fn hover_at(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pan_tool || self.pointer_gesture.is_some() {
            return;
        }
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
        self.hover_tooltip_ready = false;
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
        let tooltip_context = context.clone();
        let hover_started = pomelo_render::frame_timing::begin();
        // Coalesce rapid pointer events for one frame; detailed information
        // retains its longer delay without holding back the geometry preview.
        let tooltip_delay = cx
            .background_executor()
            .timer(std::time::Duration::from_secs(1));
        let delay = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(16));
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
            let published = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled()
                    || this.pan_tool
                    || this.pointer_gesture.is_some()
                    || this.pick_filter != filter
                    || !context.matches_view(
                        this.navigation.camera(),
                        this.navigation.size(),
                        &this.display,
                        this.selection_mode,
                    )
                {
                    return false;
                }
                this.hover_notice = None;
                match result {
                    Ok(hit) => {
                        this.hover_selection = hit
                            .as_ref()
                            .map(|(_, target, members)| (*target, Arc::clone(members)));
                        this.hovered = hit.map(|(object, _, _)| (object, context));
                    }
                    Err(pomelo_core::geometry::PathError::Cancelled) => return false,
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
                pomelo_render::frame_timing::record("hover_applied", hover_started, || {
                    serde_json::json!({
                        "object":this.hovered.as_ref().map(|(object, _)| format!("{object:?}")),
                        "camera":this.navigation.camera(),
                    })
                });
                cx.notify();
                this.hovered.is_some() || this.hover_notice.is_some()
            });
            if !matches!(published, Ok(true)) {
                return;
            }
            tooltip_delay.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled()
                    || this.pan_tool
                    || this.pointer_gesture.is_some()
                    || this.pick_filter != filter
                    || !tooltip_context.matches_view(
                        this.navigation.camera(),
                        this.navigation.size(),
                        &this.display,
                        this.selection_mode,
                    )
                {
                    return;
                }
                this.hover_tooltip_ready = true;
                pomelo_render::frame_timing::record("hover_tooltip_ready", hover_started, || {
                    serde_json::json!({
                        "object":this.hovered.as_ref().map(|(object, _)| format!("{object:?}")),
                    })
                });
                cx.notify();
            });
        }));
    }
}
