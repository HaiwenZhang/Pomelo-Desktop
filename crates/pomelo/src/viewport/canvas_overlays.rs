//! Optional surfaces floating over the canvas, the menu that shows or hides
//! them, and the canvas display popup. The canvas toolbar itself is not
//! optional: it always carries the way back to every hidden overlay.
use super::canvas_toolbar::icon_button;
use super::*;
use crate::panels::display::{setting_switch, switch_grid};
use crate::services::preferences::CanvasOverlays;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    tab::{Tab, TabBar},
};
use pomelo_core::{display::ColorMode, i18n::Locale};

/// One optional canvas surface and the view it belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Overlay {
    Location,
    Probe,
    Scale,
    ReviewMarks,
    ViewCube,
}

pub(super) const OVERLAYS: [Overlay; 5] = [
    Overlay::Location,
    Overlay::Probe,
    Overlay::Scale,
    Overlay::ReviewMarks,
    Overlay::ViewCube,
];

impl Overlay {
    pub(super) fn key(self) -> Key {
        match self {
            Self::Location => Key::OverlayLocation,
            Self::Probe => Key::ProbeHeading,
            Self::Scale => Key::OverlayScale,
            Self::ReviewMarks => Key::OverlayReviewMarks,
            Self::ViewCube => Key::OverlayViewCube,
        }
    }

    pub(super) fn shown(self, overlays: &CanvasOverlays) -> bool {
        match self {
            Self::Location => overlays.location,
            Self::Probe => overlays.probe,
            Self::Scale => overlays.scale,
            Self::ReviewMarks => overlays.review_marks,
            Self::ViewCube => overlays.view_cube,
        }
    }

    pub(super) fn set(self, overlays: &mut CanvasOverlays, shown: bool) {
        match self {
            Self::Location => overlays.location = shown,
            Self::Probe => overlays.probe = shown,
            Self::Scale => overlays.scale = shown,
            Self::ReviewMarks => overlays.review_marks = shown,
            Self::ViewCube => overlays.view_cube = shown,
        }
    }

    /// The probe and scale exist only in 2D; the view cube only in 3D.
    pub(super) fn applies(self, in_3d: bool) -> bool {
        match self {
            Self::Probe | Self::Scale => !in_3d,
            Self::ViewCube => in_3d,
            Self::Location | Self::ReviewMarks => true,
        }
    }
}

impl BoardViewport {
    pub(super) fn overlays(&self, cx: &App) -> CanvasOverlays {
        self.panel_layout.read(cx).preferences.overlays
    }

    pub(super) fn overlay_shown(&self, overlay: Overlay, cx: &App) -> bool {
        overlay.shown(&self.overlays(cx))
    }

    /// Overlay visibility is a window preference shared by every document.
    pub(super) fn set_overlay(&mut self, overlay: Overlay, shown: bool, cx: &mut Context<Self>) {
        let mut overlays = self.overlays(cx);
        overlay.set(&mut overlays, shown);
        self.set_overlays(overlays, cx);
    }

    pub(super) fn set_overlays(&mut self, overlays: CanvasOverlays, cx: &mut Context<Self>) {
        if !overlays.probe {
            self.probe = Default::default();
        }
        self.panel_layout
            .update(cx, |layout, cx| layout.set_overlays(overlays, cx));
        cx.notify();
    }

    /// Toolbar entry listing the optional overlays with their visibility.
    /// The trigger keeps a neutral fill while any overlay is hidden, so the
    /// reduced canvas is explained without opening the menu.
    pub(super) fn render_overlay_menu(&self, locale: Locale, cx: &mut Context<Self>) -> AnyElement {
        let in_3d = self.view3d.active;
        let current = self.overlays(cx);
        let any_hidden = OVERLAYS
            .iter()
            .any(|overlay| overlay.applies(in_3d) && !overlay.shown(&current));
        let label = text(locale, Key::CanvasOverlays);
        let view = cx.entity().downgrade();
        icon_button(
            "canvas-overlays",
            IconName::PanelsTopLeft,
            label.clone(),
            cx,
        )
        .selected(any_hidden)
        // The toolbar sits at the canvas bottom: the menu opens upward.
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, window, _| {
            menu = menu.min_w(window.rem_size() * 15.0).label(label.clone());
            for overlay in OVERLAYS.into_iter().filter(|o| o.applies(in_3d)) {
                let view = view.clone();
                let shown = overlay.shown(&current);
                menu = menu.item(
                    PopupMenuItem::new(text(locale, overlay.key()))
                        .checked(shown)
                        .on_click(move |_, _, cx| {
                            let _ =
                                view.update(cx, |this, cx| this.set_overlay(overlay, !shown, cx));
                        }),
                );
            }
            let show_all = view.clone();
            let toolbar_only = view.clone();
            menu.separator()
                .item(
                    PopupMenuItem::new(text(locale, Key::OverlayShowAll)).on_click(
                        move |_, _, cx| {
                            let _ = show_all.update(cx, |this, cx| {
                                this.set_overlays(CanvasOverlays::all(true), cx)
                            });
                        },
                    ),
                )
                .item(
                    PopupMenuItem::new(text(locale, Key::OverlayToolbarOnly)).on_click(
                        move |_, _, cx| {
                            let _ = toolbar_only.update(cx, |this, cx| {
                                this.set_overlays(CanvasOverlays::all(false), cx)
                            });
                        },
                    ),
                )
        })
        .into_any_element()
    }

    /// Toolbar trigger for the display popup; it stays pressed while open.
    pub(super) fn render_display_button(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.display_quick_open;
        icon_button(
            "canvas-display",
            IconName::SlidersHorizontal,
            text(locale, Key::DisplayQuick),
            cx,
        )
        .selected(open)
        .on_click(cx.listener(|this, _, _, cx| {
            this.display_quick_open = !this.display_quick_open;
            cx.notify();
        }))
        .into_any_element()
    }

    /// Everyday display settings above the canvas toolbar. Everything else
    /// stays on the full display page, one click away.
    pub(super) fn render_display_quick(
        &mut self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.display_quick_open {
            return None;
        }
        let body = if self.view3d.active {
            self.render_3d_settings(locale, true, cx)
        } else {
            self.render_2d_quick(locale, cx)
        };
        let theme = cx.theme();
        Some(
            div()
                .absolute()
                .bottom(rems(4.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("display-quick")
                        .occlude()
                        .w(rems(23.0))
                        .max_w(relative(0.92))
                        // The anchor has no definite height, so the cap is in rems.
                        .max_h(rems(30.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(div().flex_shrink_0().child(body))
                        .child(div().h(px(1.0)).flex_shrink_0().bg(theme.border))
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(text(locale, Key::DisplayOnlyNote)),
                                )
                                .child(
                                    Button::new("all-display-settings")
                                        .outline()
                                        .xsmall()
                                        .flex_shrink_0()
                                        .label(text(locale, Key::AllDisplaySettings))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_display_settings(cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Open the full display page in the right panel, showing the panel if needed.
    pub(super) fn open_display_settings(&mut self, cx: &mut Context<Self>) {
        self.display_quick_open = false;
        self.right_panel = super::RightPanelTab::Display;
        if self.panel_layout.read(cx).preferences.right_collapsed {
            self.panel_layout.update(cx, |layout, cx| {
                layout.toggle(crate::workbench::panel_layout::Side::Right, cx)
            });
        }
        cx.notify();
    }

    fn render_2d_quick(&mut self, locale: Locale, cx: &mut Context<Self>) -> AnyElement {
        let display = Arc::clone(&self.display);
        let commands = self.display_commands(cx);
        let scroll = ScrollHandle::new();
        let switch = |id: &'static str,
                      short: Key,
                      full: Key,
                      checked: bool,
                      handler: crate::panels::display::VisibilityHandler| {
            setting_switch(
                id,
                text(locale, short),
                text(locale, full),
                checked,
                handler,
                &scroll,
            )
        };
        let colors = TabBar::new("quick-color-mode")
            .segmented()
            .xsmall()
            .selected_index(match display.color_mode {
                ColorMode::Layer => 0,
                ColorMode::Net => 1,
            })
            .child(Tab::new().label(text(locale, Key::LayerColor)))
            .child(Tab::new().label(text(locale, Key::NetColor)))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                Arc::make_mut(&mut this.display).color_mode = if *index == 0 {
                    ColorMode::Layer
                } else {
                    ColorMode::Net
                };
                this.invalidate_hover();
                cx.notify();
            }));
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_x_3()
                    .gap_y_1()
                    .min_h_8()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(text(locale, Key::ColorBy)),
                    )
                    .child(colors),
            )
            .child(switch_grid(vec![
                switch(
                    "quick-fill-pads",
                    Key::FillPads,
                    Key::FillPads,
                    display.filled,
                    commands.filled,
                ),
                switch(
                    "quick-copper",
                    Key::PickCopper,
                    Key::ShowCopper,
                    display.show_copper,
                    commands.copper,
                ),
                switch(
                    "quick-drills",
                    Key::ShortDrills,
                    Key::ShowDrills,
                    display.show_drills,
                    commands.drills,
                ),
                switch(
                    "quick-texts",
                    Key::ShortBoardText,
                    Key::ShowTexts,
                    display.show_texts,
                    commands.texts,
                ),
                switch(
                    "quick-drawings",
                    Key::ShortAuxGraphics,
                    Key::ShowDrawings,
                    display.show_drawings,
                    commands.drawings,
                ),
                switch(
                    "quick-grid",
                    Key::ShowGrid,
                    Key::ShowGrid,
                    display.grid.visible,
                    commands.grid,
                ),
            ]))
            .into_any_element()
    }

    /// Location chip in the canvas's top-left corner.
    pub(super) fn render_location_chip(
        &self,
        locale: Locale,
        label: String,
        cx: &App,
    ) -> Option<AnyElement> {
        if !self.overlay_shown(Overlay::Location, cx) {
            return None;
        }
        let (_, ink, hairline) = crate::theme::canvas_colors();
        Some(
            div()
                .absolute()
                .top_3()
                .left_3()
                .max_w(relative(0.6))
                .h_7()
                .px_2p5()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(hairline)
                .bg(crate::theme::canvas_toolbar_surface())
                .text_sm()
                .text_color(ink)
                .child(
                    Icon::new(IconName::Layers)
                        .small()
                        .flex_shrink_0()
                        .text_color(crate::theme::canvas_muted()),
                )
                .child(div().min_w_0().truncate().child(label))
                .when(self.isolation_active(), |chip| {
                    chip.child(
                        div()
                            .flex_shrink_0()
                            .text_color(crate::theme::canvas_muted())
                            .child(text(locale, Key::IsolateEnd)),
                    )
                })
                .into_any_element(),
        )
    }

    /// The per-layer stack under the pointer, as a closable card in the
    /// canvas's top-right corner. It is hidden while nothing is under the
    /// pointer so it never covers the board without information.
    pub(super) fn render_probe_card(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.overlay_shown(Overlay::Probe, cx) {
            return None;
        }
        let content = self.render_probe(locale, cx)?;
        let (_, _, hairline) = crate::theme::canvas_colors();
        Some(
            div()
                .absolute()
                .top_3()
                .right_3()
                .w(rems(16.0))
                .max_w(relative(0.45))
                .occlude()
                .flex()
                .flex_col()
                .rounded_lg()
                .border_1()
                .border_color(hairline)
                .bg(crate::theme::canvas_toolbar_surface())
                .shadow_md()
                // A board with dozens of layers scrolls inside the card
                // instead of covering the canvas.
                .child(
                    div()
                        .id("probe-card-body")
                        .max_h(rems(22.0))
                        .overflow_y_scroll()
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(content),
                )
                .child(
                    div().absolute().top_1().right_1().child(
                        Button::new("hide-probe")
                            .custom(crate::theme::canvas_toolbar_button(cx))
                            .xsmall()
                            .icon(IconName::X)
                            .accessibility_label(
                                Message::new(Key::HideOverlay)
                                    .arg("name", text(locale, Key::ProbeHeading))
                                    .display(locale),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_overlay(Overlay::Probe, false, cx)
                            })),
                    ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{OVERLAYS, Overlay};
    use crate::services::preferences::CanvasOverlays;

    #[test]
    fn every_overlay_round_trips_through_its_preference_field() {
        for overlay in OVERLAYS {
            let mut overlays = CanvasOverlays::all(true);
            overlay.set(&mut overlays, false);
            assert!(!overlay.shown(&overlays), "{overlay:?}");
            let others = OVERLAYS.iter().filter(|other| **other != overlay);
            assert!(
                others.clone().all(|other| other.shown(&overlays)),
                "{overlay:?}"
            );
        }
    }

    #[test]
    fn each_view_offers_only_the_overlays_it_draws() {
        let in_2d: Vec<_> = OVERLAYS.into_iter().filter(|o| o.applies(false)).collect();
        let in_3d: Vec<_> = OVERLAYS.into_iter().filter(|o| o.applies(true)).collect();
        assert!(in_2d.contains(&Overlay::Probe) && !in_2d.contains(&Overlay::ViewCube));
        assert!(in_3d.contains(&Overlay::ViewCube) && !in_3d.contains(&Overlay::Scale));
    }
}
