use gpui_kit::gpui::{MouseButton, Pixels, Point};

pub(super) struct PointerGesture {
    pub button: MouseButton,
    pub dragging: bool,
    start: Point<Pixels>,
    previous: Point<Pixels>,
}

impl PointerGesture {
    pub fn new(position: Point<Pixels>, button: MouseButton, pan_tool: bool) -> Self {
        Self {
            button,
            dragging: button == MouseButton::Middle || pan_tool,
            start: position,
            previous: position,
        }
    }

    pub fn advance(&mut self, position: Point<Pixels>) -> Option<Point<Pixels>> {
        let distance = position - self.start;
        if f32::from(distance.x).hypot(f32::from(distance.y)) > 4.0 {
            self.dragging = true;
        }
        if !self.dragging {
            return None;
        }
        let delta = position - self.previous;
        self.previous = position;
        Some(delta)
    }

    pub fn is_click(&self) -> bool {
        self.button == MouseButton::Left && !self.dragging
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::gpui::{point, px};

    #[test]
    fn select_click_tolerates_four_pixels_of_movement() {
        let mut gesture = PointerGesture::new(point(px(0.), px(0.)), MouseButton::Left, false);
        assert!(gesture.advance(point(px(4.), px(0.))).is_none());
        assert!(gesture.is_click());
    }

    #[test]
    fn select_drag_pans_from_press_and_stays_a_drag_after_returning() {
        let mut gesture = PointerGesture::new(point(px(0.), px(0.)), MouseButton::Left, false);
        gesture.advance(point(px(2.), px(0.)));
        assert_eq!(
            gesture.advance(point(px(3.), px(3.))),
            Some(point(px(3.), px(3.)))
        );
        assert_eq!(
            gesture.advance(point(px(0.), px(0.))),
            Some(point(px(-3.), px(-3.)))
        );
        assert!(!gesture.is_click());
    }

    #[test]
    fn middle_button_pans_immediately_in_select() {
        let mut gesture = PointerGesture::new(point(px(0.), px(0.)), MouseButton::Middle, false);
        assert_eq!(
            gesture.advance(point(px(1.), px(0.))),
            Some(point(px(1.), px(0.)))
        );
        assert!(!gesture.is_click());
    }

    #[test]
    fn pan_tool_never_selects_even_without_movement() {
        let gesture = PointerGesture::new(point(px(0.), px(0.)), MouseButton::Left, true);
        assert!(gesture.dragging);
        assert!(!gesture.is_click());
    }
}
