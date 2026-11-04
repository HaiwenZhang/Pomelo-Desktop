//! Coalesce navigation layouts on a worker; only the latest request may publish.
use super::*;
use pomelo_core::{display::BoardDisplay, interaction::Camera, task::CancellationToken};
use pomelo_render::text::msdf::{LabelIndex, PreparedGlyphs};
use std::time::Duration;

#[derive(Clone, Copy)]
struct View {
    camera: Camera,
    width: f64,
    height: f64,
}
impl PartialEq for View {
    fn eq(&self, next: &Self) -> bool {
        self.camera.center == next.camera.center
            && self.camera.pixels_per_mm == next.camera.pixels_per_mm
            && self.camera.flipped == next.camera.flipped
            && self.width == next.width
            && self.height == next.height
    }
}
impl View {
    fn can_preview(self, next: Self) -> bool {
        self.camera.flipped == next.camera.flipped
            && self.width == next.width
            && self.height == next.height
            && (0.5..=2.0).contains(&(next.camera.pixels_per_mm / self.camera.pixels_per_mm))
    }
}
#[derive(Clone)]
struct Request {
    view: View,
    index: Arc<LabelIndex>,
    display: Arc<BoardDisplay>,
}
impl Request {
    fn same_rules(&self, next: &Self) -> bool {
        Arc::ptr_eq(&self.index, &next.index)
            && (Arc::ptr_eq(&self.display, &next.display)
                || self.display.same_label_layout(&next.display))
    }
    fn matches(&self, next: &Self) -> bool {
        self.view == next.view && self.same_rules(next)
    }
    fn can_preview(&self, next: &Self) -> bool {
        self.same_rules(next) && self.view.can_preview(next.view)
    }
}
#[derive(Default)]
pub(super) struct LabelState {
    request: Option<Request>,
    complete: Option<(Request, Arc<PreparedGlyphs>)>,
    placeholder: Option<Arc<PreparedGlyphs>>,
    cancel: CancellationToken,
    task: Option<Task<()>>,
    pub pending: bool,
    pub failed: bool,
}
impl LabelState {
    fn accepts(&self, request: &Request, cancel: &CancellationToken) -> bool {
        !cancel.is_cancelled()
            && self
                .request
                .as_ref()
                .is_some_and(|current| current.matches(request))
    }

    pub fn frame(&self) -> Option<Arc<PreparedGlyphs>> {
        let request = self.request.as_ref()?;
        if self.failed {
            return self.placeholder.clone();
        }
        self.complete
            .as_ref()
            .filter(|(complete, _)| complete.can_preview(request))
            .map(|(_, source)| Arc::clone(source))
            .or_else(|| self.placeholder.clone())
    }
}
impl Drop for LabelState {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl BoardViewport {
    pub(super) fn request_labels(
        &mut self,
        camera: Camera,
        width: f64,
        height: f64,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = &self.label_index else {
            return;
        };
        let request = Request {
            view: View {
                camera,
                width,
                height,
            },
            index: Arc::clone(index),
            display: Arc::clone(&self.display),
        };
        if self
            .labels
            .request
            .as_ref()
            .is_some_and(|old| old.matches(&request))
        {
            return;
        }
        let navigation = self.labels.request.as_ref().is_some_and(|old| {
            old.same_rules(&request) && old.view.camera.flipped == camera.flipped
        });
        if self
            .labels
            .request
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(&old.index, index))
        {
            self.labels.placeholder = Some(Arc::new(index.empty()));
            self.labels.complete = None;
        }
        self.labels.cancel.cancel();
        self.labels.cancel = CancellationToken::default();
        self.labels.request = Some(request.clone());
        self.labels.failed = false;
        if self
            .labels
            .complete
            .as_ref()
            .is_some_and(|(complete, _)| complete.matches(&request))
        {
            self.labels.pending = false;
            self.labels.task = None;
            return;
        }
        self.labels.pending = true;
        let cancel = self.labels.cancel.clone();
        let worker_cancel = cancel.clone();
        let worker_request = request.clone();
        let delay = cx.background_executor().timer(if navigation {
            Duration::from_millis(120)
        } else {
            Duration::ZERO
        });
        let worker = cx.background_spawn(async move {
            delay.await;
            if worker_cancel.is_cancelled() {
                return None;
            }
            let started = pomelo_render::frame_timing::begin();
            let result = worker_request.index.layout(
                camera,
                width,
                height,
                &worker_request.display,
                worker_request.display.label_options,
                &worker_cancel,
            );
            pomelo_render::frame_timing::record("label_layout", started, || {
                serde_json::json!({
                    "camera":camera,"cancelled":worker_cancel.is_cancelled(),
                    "glyphs":result.as_ref().map_or(0, |source| source.instances.len()),
                })
            });
            Some(result)
        });
        self.labels.task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                // Token identity also rejects A -> B -> A completions from the first A.
                if !this.labels.accepts(&request, &cancel) {
                    return;
                }
                this.labels.pending = false;
                match result {
                    Some(Ok(source)) => this.labels.complete = Some((request, Arc::new(source))),
                    Some(Err(diagnostic)) => {
                        this.labels.failed = true;
                        if !this
                            .render_diagnostics
                            .iter()
                            .any(|item| item.code == diagnostic.code)
                        {
                            this.render_diagnostics.push(diagnostic);
                        }
                    }
                    None => return,
                }
                cx.notify();
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn navigation_preview_rejects_flip_resize_and_large_scale_changes() {
        let view = View {
            camera: Camera::default(),
            width: 800.,
            height: 600.,
        };
        let mut next = view;
        next.camera.center.x += 1.;
        assert!(view.can_preview(next));
        next.camera.pixels_per_mm *= 1.5;
        assert!(view.can_preview(next));
        next.camera.pixels_per_mm *= 2.;
        assert!(!view.can_preview(next));
        next = view;
        next.camera.flipped = !next.camera.flipped;
        assert!(!view.can_preview(next));
        next = view;
        next.width += 1.;
        assert!(!view.can_preview(next));
    }
    #[::core::prelude::v1::test]
    fn closing_the_viewport_cancels_label_work() {
        let state = LabelState::default();
        let token = state.cancel.clone();
        drop(state);
        assert!(token.is_cancelled());
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;
    fn request() -> Request {
        let scene = BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            diagnostics: vec![],
            bounds: pomelo_core::model::Bounds {
                min: Default::default(),
                max: Default::default(),
            },
        };
        let font = Arc::new(pomelo_render::text::msdf::MsdfFont {
            glyphs: Default::default(),
            pages: vec![],
            cap_height: 1.,
        });
        Request {
            view: View {
                camera: Camera::default(),
                width: 800.,
                height: 600.,
            },
            index: Arc::new(
                LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap(),
            ),
            display: Arc::new(BoardDisplay::default()),
        }
    }
    #[::core::prelude::v1::test]
    fn preview_preserves_font_but_hides_stale_labels_when_rules_or_source_change() {
        let first = request();
        let source = Arc::new(first.index.empty());
        let placeholder = Arc::new(first.index.empty());
        let mut state = LabelState {
            request: Some(first.clone()),
            complete: Some((first.clone(), source.clone())),
            placeholder: Some(placeholder.clone()),
            cancel: CancellationToken::default(),
            task: None,
            pending: false,
            failed: false,
        };
        let mut next = first.clone();
        next.view.camera.center.x += 1.;
        state.request = Some(next.clone());
        assert!(Arc::ptr_eq(&state.frame().unwrap(), &source));
        Arc::make_mut(&mut next.display).label_options.pin_names = false;
        state.request = Some(next);
        assert!(Arc::ptr_eq(&state.frame().unwrap(), &placeholder));
        state.request = Some(first.clone());
        state.failed = true;
        assert!(
            Arc::ptr_eq(&state.frame().unwrap(), &placeholder),
            "a failed layout cannot leave stale preview labels visible"
        );
        let other = request();
        assert!(!first.can_preview(&other));
        let mut mode = first.clone();
        Arc::make_mut(&mut mode.display).trace_render_mode =
            pomelo_core::display::TraceRenderMode::Sdf;
        assert!(
            first.matches(&mode),
            "renderer mode does not invalidate labels"
        );
    }
    #[::core::prelude::v1::test]
    fn out_of_order_and_cancelled_a_b_a_results_cannot_publish() {
        let first = request();
        let old_token = CancellationToken::default();
        let mut state = LabelState {
            request: Some(first.clone()),
            complete: None,
            placeholder: None,
            cancel: CancellationToken::default(),
            task: None,
            pending: false,
            failed: false,
        };
        assert!(state.accepts(&first, &old_token));
        let mut second = first.clone();
        second.view.camera.center.x += 1.;
        state.request = Some(second);
        assert!(!state.accepts(&first, &old_token));
        old_token.cancel();
        state.request = Some(first.clone());
        assert!(!state.accepts(&first, &old_token));
        assert!(state.accepts(&first, &CancellationToken::default()));
    }
}
