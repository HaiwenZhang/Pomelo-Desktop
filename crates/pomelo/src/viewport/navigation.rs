//! Camera commands and pan gestures.
use super::*;

impl BoardViewport {
    pub(crate) fn fit_board(&mut self, _: &FitBoard, _: &mut Window, cx: &mut Context<Self>) {
        if self.view3d.active {
            let previous = self.view3d.camera;
            if self.fit_3d() {
                if previous != self.view3d.camera {
                    self.view3d.camera = previous;
                    self.record_3d_view();
                    self.fit_3d();
                }
                cx.notify();
            }
            return;
        }
        self.wheel_history_at = None;
        let previous = self.navigation.camera();
        if self.navigation.fit(self.scene.bounds) {
            self.review
                .history
                .record(previous, self.navigation.camera());
            self.refresh_measure_preview(cx);
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn flip_board(&mut self, _: &FlipBoard, _: &mut Window, cx: &mut Context<Self>) {
        if self.view3d.active {
            self.record_3d_view();
            self.view3d.camera.orbit(0.0, std::f64::consts::PI);
            cx.notify();
            return;
        }
        self.wheel_history_at = None;
        let previous = self.navigation.camera();
        self.invalidate_hover();
        self.navigation.flip();
        self.review
            .history
            .record(previous, self.navigation.camera());
        self.refresh_measure_preview(cx);
        cx.notify();
    }

    pub(super) fn zoom(&mut self, factor: f64, cx: &mut Context<Self>) {
        if self.view3d.active {
            let size = self.view3d.size;
            self.view3d
                .camera
                .zoom_at([size[0] * 0.5, size[1] * 0.5], size, factor);
            cx.notify();
            return;
        }
        self.wheel_history_at = None;
        let previous = self.navigation.camera();
        let size = self.navigation.size();
        if self.navigation.zoom_at(
            pomelo_core::model::Point::new(size.x * 0.5, size.y * 0.5),
            factor,
        ) {
            self.review
                .history
                .record(previous, self.navigation.camera());
            self.refresh_measure_preview(cx);
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn pan(&mut self, x: f64, y: f64, cx: &mut Context<Self>) {
        if self.view3d.active {
            let size = self.view3d.size;
            self.view3d
                .camera
                .pan([x * size[0] * 0.1, y * size[1] * 0.1], size);
            cx.notify();
            return;
        }
        self.wheel_history_at = None;
        let previous = self.navigation.camera();
        let size = self.navigation.size();
        // Keyboard movement is proportional to the viewport, independent of DPI.
        if self.navigation.pan(pomelo_core::model::Point::new(
            x * size.x * 0.1,
            y * size.y * 0.1,
        )) {
            self.review
                .history
                .record(previous, self.navigation.camera());
            self.refresh_measure_preview(cx);
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        if self.view3d.active {
            self.view3d.camera.zoom_at(
                [f64::from(anchor.x), f64::from(anchor.y)],
                self.view3d.size,
                1.2_f64.powf(steps.clamp(-10.0, 10.0)),
            );
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let previous = self.navigation.camera();
        if self.navigation.zoom_at(
            pomelo_core::model::Point::new(
                f64::from(f32::from(anchor.x)),
                f64::from(f32::from(anchor.y)),
            ),
            1.2_f64.powf(steps.clamp(-10.0, 10.0)),
        ) {
            // A wheel burst is one history entry, not one entry per device event.
            let now = std::time::Instant::now();
            if self
                .wheel_history_at
                .is_none_or(|last| now.duration_since(last).as_millis() > 250)
            {
                self.review
                    .history
                    .record(previous, self.navigation.camera());
            }
            self.wheel_history_at = Some(now);
            self.refresh_measure_preview(cx);
            cx.stop_propagation();
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn start_pan(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            self.start_3d_drag(event, window, cx);
            return;
        }
        if self.pointer_gesture.is_some() {
            return;
        }
        self.invalidate_hover();
        window.focus(&self.focus, cx);
        if self.start_review_drag(event) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.pointer_gesture = Some(gesture::PointerGesture::new(
            event.position,
            event.button,
            self.pan_tool,
        ));
        self.wheel_history_at = None;
        self.pan_start = Some(self.navigation.camera());
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn move_pan(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            self.move_3d_drag(event, cx);
            return;
        }
        self.review.free_measure = event.modifiers.alt;
        self.review.orthogonal_measure = event.modifiers.shift;
        if self.pointer_position != Some(event.position) {
            self.pointer_position = Some(event.position);
            cx.notify();
        }
        if self
            .pointer_gesture
            .as_ref()
            .is_some_and(|gesture| event.pressed_button != Some(gesture.button))
        {
            self.record_pan_history();
            self.pointer_gesture = None;
            cx.notify();
        }
        if let Some(gesture) = &mut self.pointer_gesture {
            if let Some(delta) = gesture.advance(event.position) {
                self.navigation.pan(pomelo_core::model::Point::new(
                    f64::from(f32::from(delta.x)),
                    f64::from(f32::from(delta.y)),
                ));
                self.refresh_measure_preview(cx);
                cx.notify();
            }
        } else if self.review.drag.is_some() || self.review.tool.is_some() {
            self.move_review_pointer(event.position, cx);
        } else if event.pressed_button.is_none() {
            self.hover_at(event.position, window, cx);
        }
    }

    pub(super) fn stop_pan(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            self.stop_3d_drag(event, window, cx);
            return;
        }
        if event.button == MouseButton::Left && self.finish_review_drag(event, window, cx) {
            cx.stop_propagation();
            return;
        }
        if !self
            .pointer_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.button == event.button)
        {
            return;
        }
        if let Some(gesture) = self.pointer_gesture.take() {
            self.record_pan_history();
            self.refresh_measure_preview(cx);
            if gesture.is_click() && !self.review_click(event, window, cx) {
                self.pick_trace(event.position, window, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(super) fn cancel_pan(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .pointer_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.button == event.button)
        {
            self.record_pan_history();
            self.pointer_gesture = None;
            cx.notify();
        }
        if event.button == MouseButton::Left && self.review.drag.take().is_some() {
            cx.notify();
        }
    }

    fn record_pan_history(&mut self) {
        if let Some(previous) = self.pan_start.take() {
            self.review
                .history
                .record(previous, self.navigation.camera());
        }
    }

    /// Camera changes must move the live measurement endpoint under the cursor.
    pub(super) fn refresh_measure_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(position) = self.pointer_position
            && matches!(self.review.tool, Some(review::ReviewTool::Measure(_)))
        {
            self.move_review_pointer(position, cx);
        }
    }
}
