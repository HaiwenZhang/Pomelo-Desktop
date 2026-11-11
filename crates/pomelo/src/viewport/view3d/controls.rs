//! 3D-only entries of the canvas toolbar and the stack assumption notice.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::{
    display3d::{RenderMode3d, Z_SCALE_PRESETS},
    i18n::Locale,
};

/// Display modes in menu order, with their name and one-line purpose.
pub(in crate::viewport) const RENDER_MODES: [(RenderMode3d, Key, Key); 3] = [
    (
        RenderMode3d::Default,
        Key::RenderModeDefault,
        Key::RenderModeDefaultHint,
    ),
    (
        RenderMode3d::Wireframe,
        Key::RenderModeWireframe,
        Key::RenderModeWireframeHint,
    ),
    (
        RenderMode3d::Solid,
        Key::RenderModeSolid,
        Key::RenderModeSolidHint,
    ),
];

impl BoardViewport {
    pub(in crate::viewport) fn set_render_mode_3d(
        &mut self,
        mode: RenderMode3d,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.settings.mode != mode {
            self.view3d.settings.mode = mode;
            cx.notify();
        }
    }

    /// Change the Z display factor around the current focus, keeping the view framed.
    pub(in crate::viewport) fn set_z_scale_3d(&mut self, scale: f64, cx: &mut Context<Self>) {
        let scale = scale.clamp(
            *pomelo_core::display3d::Z_SCALE_RANGE.start(),
            *pomelo_core::display3d::Z_SCALE_RANGE.end(),
        );
        if self.view3d.settings.z_scale != scale {
            self.view3d.settings.z_scale = scale;
            cx.notify();
        }
    }

    /// Display mode and Z stretch, the 3D entries of the toolbar display group.
    pub(in crate::viewport) fn render_3d_display_tools(
        &self,
        locale: Locale,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let enabled = self.view3d.scene.is_some();
        let view = cx.entity().downgrade();
        let current = self.view3d.settings.mode;
        let mode_label = text(locale, Key::ToolbarDisplayMode);
        let mode = super::super::canvas_toolbar::icon_button(
            "3d-display-mode",
            IconName::Box,
            mode_label.clone(),
            cx,
        )
        .disabled(!enabled)
        .dropdown_menu(move |mut menu, window, _| {
            menu = menu
                .min_w(window.rem_size() * 15.0)
                .label(mode_label.clone());
            for (mode, name, hint) in RENDER_MODES {
                let view = view.clone();
                menu = menu.item(
                    PopupMenuItem::new(
                        Message::new(Key::RenderModeItem)
                            .arg("name", text(locale, name))
                            .arg("hint", text(locale, hint))
                            .display(locale),
                    )
                    .checked(current == mode)
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| this.set_render_mode_3d(mode, cx));
                    }),
                );
            }
            menu
        })
        .into_any_element();
        let view = cx.entity().downgrade();
        let selected = self.view3d.settings.z_scale;
        let stretch_label = Message::new(Key::ToolbarZStretch)
            .arg("scale", format_scale(selected))
            .display(locale);
        let stretch = super::super::canvas_toolbar::icon_button(
            "3d-z-stretch",
            IconName::MoveVertical,
            stretch_label.clone(),
            cx,
        )
        .selected(selected != 1.0)
        .disabled(!enabled)
        .dropdown_menu(move |mut menu, window, _| {
            menu = menu
                .min_w(window.rem_size() * 14.0)
                .label(stretch_label.clone());
            for scale in Z_SCALE_PRESETS {
                let view = view.clone();
                let label = if scale == 1.0 {
                    text(locale, Key::ZScaleTrue)
                } else {
                    Message::new(Key::Preview3dZScale)
                        .arg("scale", format_scale(scale))
                        .display(locale)
                };
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(selected == scale)
                        .on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.set_z_scale_3d(scale, cx));
                        }),
                );
            }
            menu.separator()
                .label(text(locale, Key::Preview3dDisplayOnly))
        })
        .into_any_element();
        vec![mode, stretch]
    }

    /// Thickness assumption shown with the layer stack instead of over the board.
    pub(in crate::viewport) fn render_stackup_notice(&self, locale: Locale, cx: &App) -> Div {
        let preview = self
            .view3d
            .scene
            .as_ref()
            .map_or_else(PreviewStackup::default, |s| s.preview);
        let warnings = self.view3d.scene.as_ref().map_or(0, |s| s.warning_count);
        let theme = cx.theme();
        let details = Message::new(Key::Preview3dStackup)
            .arg("board", preview.board_mm.to_string())
            .arg("copper", preview.copper_mm.to_string())
            .arg("plating", preview.plating_mm.to_string())
            .display(locale);
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap_1()
            .text_xs()
            .child(
                div()
                    .id("stackup-assumed")
                    .px_1p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.warning)
                    .text_color(theme.warning)
                    .child(text(locale, Key::StackupAssumed)),
            )
            .child(div().text_color(theme.muted_foreground).child(details))
            .when(self.view3d.scene.is_some(), |note| {
                let total = self
                    .scene
                    .layers
                    .iter()
                    .filter(|l| {
                        l.id.is_copper()
                            && l.function != pomelo_core::model::LayerFunction::Dielectric
                    })
                    .count();
                note.child(
                    div().text_color(theme.muted_foreground).child(
                        Message::new(Key::Prepared3dLayers)
                            .arg("prepared", self.view3d.prepared_layers.len())
                            .arg("total", total)
                            .display(locale),
                    ),
                )
            })
            .when(
                self.view3d.detail != pomelo_render::scene::board3d::Detail3d::Full,
                |note| {
                    note.child(div().text_color(theme.warning).child(text(
                        locale,
                        if self.view3d.detail == pomelo_render::scene::board3d::Detail3d::OuterOnly
                        {
                            Key::Reduced3dOuterOnly
                        } else {
                            Key::Reduced3dCoarse
                        },
                    )))
                },
            )
            .when(warnings > 0, |note| {
                note.child(
                    div().text_color(theme.warning).child(
                        Message::new(Key::Preview3dWarnings)
                            .arg("count", warnings)
                            .display(locale),
                    ),
                )
            })
    }
}

/// Z factors are whole numbers in menus; keep a fraction if the slider set one.
pub(in crate::viewport) fn format_scale(scale: f64) -> String {
    if scale.fract() == 0.0 {
        format!("{scale:.0}")
    } else {
        format!("{scale:.1}")
    }
}
