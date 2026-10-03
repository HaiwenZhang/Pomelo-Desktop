//! Host scheduling only; contour math/cache preparation remain in pomelo-render.
use super::*;
use pomelo_core::{display::BoardDisplay, task::CancellationToken};
use pomelo_render::scene::curves::{CurveCacheLimits, CurveFillCache, CurveView};

struct Request {
    view: CurveView,
    display: Arc<BoardDisplay>,
}
impl Request {
    fn matches(&self, view: CurveView, display: &Arc<BoardDisplay>) -> bool {
        self.view == view && Arc::ptr_eq(&self.display, display)
    }
}
#[derive(Default)]
pub(super) struct CurveState {
    cache: Arc<CurveFillCache>,
    request: Option<Request>,
    complete: Option<Request>,
    cancel: CancellationToken,
    task: Option<Task<()>>,
    pub pending: bool,
    pub failed: bool,
}
impl CurveState {
    pub fn frame(&self) -> Option<Arc<CurveFillCache>> {
        if self.request.is_none() {
            return Some(Arc::clone(&self.cache));
        }
        self.complete
            .as_ref()
            .filter(|complete| {
                self.request
                    .as_ref()
                    .is_some_and(|request| complete.matches(request.view, &request.display))
            })
            .map(|_| Arc::clone(&self.cache))
    }
    pub fn statistics(&self) -> pomelo_render::scene::curves::CurveStatistics {
        self.cache.statistics
    }
}
impl Drop for CurveState {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl BoardViewport {
    pub(super) fn request_curves(&mut self, view: Option<CurveView>, cx: &mut Context<Self>) {
        let view = view.filter(|_| self.display.filled && self.display.show_copper);
        let Some(view) = view else {
            if self.curve_fill.request.is_some() {
                self.curve_fill.cache = Arc::new(
                    self.curve_fill
                        .cache
                        .inactive(CurveCacheLimits::default().soft_bytes),
                );
                self.curve_fill.complete = None;
            }
            self.curve_fill.cancel.cancel();
            self.curve_fill.task = None;
            self.curve_fill.request = None;
            self.curve_fill.pending = false;
            self.curve_fill.failed = false;
            return;
        };
        if self
            .curve_fill
            .request
            .as_ref()
            .is_some_and(|request| request.matches(view, &self.display))
        {
            return;
        }
        self.curve_fill.cancel.cancel();
        self.curve_fill.cancel = CancellationToken::default();
        self.curve_fill.request = Some(Request {
            view,
            display: Arc::clone(&self.display),
        });
        self.curve_fill.pending = true;
        self.curve_fill.failed = false;
        let cancel = self.curve_fill.cancel.clone();
        let worker_cancel = cancel.clone();
        let scene = Arc::clone(&self.scene);
        let display = Arc::clone(&self.display);
        let previous = Arc::clone(&self.curve_fill.cache);
        let worker_display = Arc::clone(&display);
        let worker = cx.background_spawn(async move {
            CurveFillCache::prepare(
                &scene,
                view,
                &worker_display,
                &previous,
                CurveCacheLimits::default(),
                &worker_cancel,
            )
        });
        self.curve_fill.task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if cancel.is_cancelled()
                    || this
                        .curve_fill
                        .request
                        .as_ref()
                        .is_none_or(|request| !request.matches(view, &display))
                {
                    return;
                }
                this.curve_fill.pending = false;
                match result {
                    Ok(cache) => {
                        this.curve_fill.cache = Arc::new(cache);
                        this.curve_fill.complete = Some(Request { view, display });
                    }
                    Err(error) => {
                        this.curve_fill.failed = true;
                        let diagnostic = error.diagnostic();
                        if !this
                            .render_diagnostics
                            .iter()
                            .any(|d| d.code == diagnostic.code && d.object == diagnostic.object)
                        {
                            this.render_diagnostics.push(diagnostic);
                        }
                    }
                }
                cx.notify();
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::{Arc, BoardDisplay, CurveState, CurveView, Request};
    use pomelo_core::model::{Bounds as BoardBounds, Point as BoardPoint};
    fn view(x: f64) -> CurveView {
        CurveView {
            bounds: BoardBounds {
                min: BoardPoint::new(x, 0.0),
                max: BoardPoint::new(x + 1.0, 1.0),
            },
            tolerance: 0.001,
        }
    }
    #[test]
    fn a_completed_snapshot_cannot_paint_a_new_view_or_display_generation() {
        let display = Arc::new(BoardDisplay::default());
        let mut state = CurveState {
            request: Some(Request {
                view: view(1.0),
                display: Arc::clone(&display),
            }),
            complete: Some(Request {
                view: view(0.0),
                display: Arc::clone(&display),
            }),
            cache: Arc::default(),
            cancel: Default::default(),
            task: None,
            pending: false,
            failed: false,
        };
        assert!(state.frame().is_none());
        state.request = Some(Request {
            view: view(0.0),
            display: Arc::new(BoardDisplay::default()),
        });
        assert!(state.frame().is_none());
        state.request = Some(Request {
            view: view(0.0),
            display,
        });
        assert!(state.frame().is_some());
    }
    #[test]
    fn closing_the_viewport_cancels_its_curve_worker() {
        let state = CurveState::default();
        let token = state.cancel.clone();
        drop(state);
        assert!(token.is_cancelled());
    }
}
