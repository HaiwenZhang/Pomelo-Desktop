//! Camera commands and pan gestures.
use super::*;

impl BoardViewport {
    pub(crate) fn fit_board(&mut self, _: &FitBoard, _: &mut Window, cx: &mut Context<Self>) {
        if self.navigation.fit(self.scene.bounds) {
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn flip_board(&mut self, _: &FlipBoard, _: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_hover();
        self.navigation.flip();
        cx.notify();
    }

    pub(super) fn zoom(&mut self, factor: f64, cx: &mut Context<Self>) {
        let size = self.navigation.size();
        if self.navigation.zoom_at(
            pomelo_core::model::Point::new(size.x * 0.5, size.y * 0.5),
            factor,
        ) {
            self.invalidate_hover();
            cx.notify();
        }
    }

    pub(super) fn pan(&mut self, x: f64, y: f64, cx: &mut Context<Self>) {
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

    pub(super) fn start_pan(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pointer_gesture.is_some() {
            return;
        }
        self.invalidate_hover();
        window.focus(&self.focus, cx);
        self.pointer_gesture = Some(gesture::PointerGesture::new(
            event.position,
            event.button,
            self.pan_tool,
        ));
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn move_pan(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pointer_position != Some(event.position) {
            self.pointer_position = Some(event.position);
            cx.notify();
        }
        if self
            .pointer_gesture
            .as_ref()
            .is_some_and(|gesture| event.pressed_button != Some(gesture.button))
        {
            self.pointer_gesture = None;
            cx.notify();
        }
        if let Some(gesture) = &mut self.pointer_gesture {
            if let Some(delta) = gesture.advance(event.position) {
                self.navigation.pan(pomelo_core::model::Point::new(
                    f64::from(f32::from(delta.x)),
                    f64::from(f32::from(delta.y)),
                ));
                cx.notify();
            }
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
        if !self
            .pointer_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.button == event.button)
        {
            return;
        }
        if let Some(gesture) = self.pointer_gesture.take() {
            if gesture.is_click() {
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
            self.pointer_gesture = None;
            cx.notify();
        }
    }
}
