//! Reveal focused commands without adding stops to the tab order.
use gpui_kit::base::StyledExt as _;
use gpui_kit::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct FocusReveal {
    scope: FocusHandle,
    previous: Rc<RefCell<Option<WeakFocusHandle>>>,
    viewport_size: Rc<Cell<Option<Size<Pixels>>>>,
    scroll: ScrollHandle,
    axis: Axis,
}

impl FocusReveal {
    pub fn new(scroll: &ScrollHandle, cx: &mut App) -> Self {
        Self::with_axis(scroll, Axis::Vertical, cx)
    }

    fn with_axis(scroll: &ScrollHandle, axis: Axis, cx: &mut App) -> Self {
        Self {
            scope: cx.focus_handle().tab_stop(false),
            previous: Default::default(),
            viewport_size: Default::default(),
            scroll: scroll.clone(),
            axis,
        }
    }

    pub fn wrap(&self, child: impl IntoElement) -> Div {
        let scope = self.scope.clone();
        let previous = self.previous.clone();
        let viewport_size = self.viewport_size.clone();
        let scroll = self.scroll.clone();
        let axis = self.axis;
        div()
            .relative()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .flex_shrink_0()
            .track_focus(&self.scope)
            .child(child)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let viewport = scroll.bounds();
                        let resized = viewport_size
                            .replace(Some(viewport.size))
                            .is_some_and(|previous| previous != viewport.size);
                        let focused = window
                            .focused(cx)
                            .filter(|focus| scope.contains(focus, window))
                            .map(|focus| focus.downgrade());
                        if *previous.borrow() == focused && !resized {
                            return;
                        }
                        *previous.borrow_mut() = focused.clone();
                        let Some(focused) = focused else { return };
                        let start = bounds.origin.along(axis);
                        let end = start + bounds.size.along(axis);
                        let viewport_start = viewport.origin.along(axis);
                        let viewport_end = viewport_start + viewport.size.along(axis);
                        let delta = if start < viewport_start {
                            viewport_start - start
                        } else if end > viewport_end {
                            viewport_end - end
                        } else {
                            return;
                        };
                        // Change the offset after prepaint so hitboxes and paint
                        // use the same coordinates for the current frame.
                        window.on_next_frame(move |window, cx| {
                            if window.focused(cx).is_some_and(|focus| focus == focused) {
                                let mut offset = scroll.offset();
                                match axis {
                                    Axis::Horizontal => offset.x += delta,
                                    Axis::Vertical => offset.y += delta,
                                }
                                scroll.set_offset(offset);
                                window.refresh();
                            }
                        });
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }

    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.scope.contains_focused(window, cx)
    }
}

/// A keyed focus scope for commands rendered inside a scroll region.
#[derive(IntoElement)]
pub struct FocusScroll {
    id: ElementId,
    scroll: ScrollHandle,
    child: AnyElement,
    style: StyleRefinement,
    axis: Axis,
}

impl FocusScroll {
    pub fn new(id: impl Into<ElementId>, scroll: &ScrollHandle, child: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            scroll: scroll.clone(),
            child: child.into_any_element(),
            style: Default::default(),
            axis: Axis::Vertical,
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.axis = Axis::Horizontal;
        self
    }
}

impl Styled for FocusScroll {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FocusScroll {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window.use_keyed_state((self.id, "focus-scroll"), cx, move |_, cx| {
            FocusReveal::with_axis(&self.scroll, self.axis, cx)
        });
        focus.read(cx).wrap(self.child).refine_style(&self.style)
    }
}

pub struct InspectorFocus {
    pub scroll: ScrollHandle,
    pub file_information: FocusReveal,
    pub diagnostics: FocusReveal,
    pub previous: FocusReveal,
    pub next: FocusReveal,
}

impl InspectorFocus {
    pub fn new(cx: &mut App) -> Self {
        let scroll = ScrollHandle::new();
        Self {
            file_information: FocusReveal::new(&scroll, cx),
            diagnostics: FocusReveal::new(&scroll, cx),
            previous: FocusReveal::new(&scroll, cx),
            next: FocusReveal::new(&scroll, cx),
            scroll,
        }
    }
}
