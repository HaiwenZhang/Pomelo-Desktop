//! Window-wide side panel layout, shared by every PCB document.
use crate::prefs::PanelPreferences;
use gpui_kit::{App, AppContext, Context, Entity, component::ResizableState};

// Keep the existing canvas command bar usable when both sidebars are wide.
pub(crate) const MIN_CANVAS_WIDTH: f32 = 360.0;
pub(crate) const PANEL_FRAME_WIDTH: f32 = 4.0;

pub(crate) struct PanelLayout {
    pub preferences: PanelPreferences,
    pub sizes: Entity<ResizableState>,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum Side {
    Left,
    Right,
}
/// Clamp restored widths without allowing fixed sidebars to push the canvas offscreen.
pub(crate) fn fit_to_window(
    mut panels: PanelPreferences,
    width: f32,
    rail: f32,
) -> PanelPreferences {
    let budget = width
        - MIN_CANVAS_WIDTH
        - PANEL_FRAME_WIDTH
        - rail * (u8::from(panels.left_collapsed) + u8::from(panels.right_collapsed)) as f32;
    let occupied = if panels.left_collapsed {
        0.0
    } else {
        panels.left_width
    } + if panels.right_collapsed {
        0.0
    } else {
        panels.right_width
    };
    let mut excess = (occupied - budget).max(0.0);
    if !panels.right_collapsed {
        let reduction = excess.min(panels.right_width - 240.0);
        panels.right_width -= reduction;
        excess -= reduction;
    }
    if !panels.left_collapsed {
        panels.left_width = (panels.left_width - excess).max(200.0);
    }
    panels
}
impl PanelLayout {
    pub fn new(preferences: PanelPreferences, cx: &mut Context<Self>) -> Self {
        Self {
            preferences,
            sizes: cx.new(|_| ResizableState::default()),
        }
    }
    pub fn snapshot(&self, cx: &App) -> PanelPreferences {
        let sizes = self.sizes.read(cx).sizes();
        let mut preferences = self.preferences;
        if !preferences.left_collapsed
            && let Some(size) = sizes.first()
        {
            preferences.left_width = size.as_f32().clamp(200.0, 440.0);
        }
        let right_index = if preferences.left_collapsed { 1 } else { 2 };
        if !preferences.right_collapsed
            && let Some(size) = sizes.get(right_index)
        {
            preferences.right_width = size.as_f32().clamp(240.0, 440.0);
        }
        preferences
    }
    pub fn toggle(&mut self, side: Side, cx: &mut Context<Self>) {
        self.preferences = self.snapshot(cx);
        match side {
            Side::Left => self.preferences.left_collapsed = !self.preferences.left_collapsed,
            Side::Right => self.preferences.right_collapsed = !self.preferences.right_collapsed,
        }
        // The remaining children have different indexes after a collapse.
        // Rebuild only Kit's layout state using the remembered expanded widths.
        self.sizes.update(cx, |sizes, _| sizes.clear());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{PanelPreferences, fit_to_window};

    #[test]
    fn maximum_restored_sidebars_leave_space_for_the_canvas_in_a_small_window() {
        let panels = fit_to_window(
            PanelPreferences {
                left_width: 440.0,
                right_width: 440.0,
                ..PanelPreferences::default()
            },
            1000.0,
            40.0,
        );
        assert_eq!((panels.left_width, panels.right_width), (396.0, 240.0));
    }

    #[test]
    fn collapsing_both_sides_preserves_the_remembered_expanded_widths() {
        let panels = PanelPreferences {
            left_collapsed: true,
            right_collapsed: true,
            left_width: 440.0,
            right_width: 440.0,
        };
        assert_eq!(fit_to_window(panels, 1000.0, 40.0), panels);
    }
}
