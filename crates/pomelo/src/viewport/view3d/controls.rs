//! Canvas-local 3D controls, sharing the existing canvas toolbar surface.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::{i18n::Locale, interaction::camera3d::CameraView3d};

impl BoardViewport {
    pub(in crate::viewport) fn render_3d_controls(
        &self,
        locale: Locale,
        cx: &Context<Self>,
    ) -> Div {
        let enabled = self.view3d.scene.is_some();
        let mut controls = Vec::new();
        for (id, transparent, key) in [
            ("3d-solid", false, Key::Preview3dSolid),
            ("3d-transparent", true, Key::Preview3dTransparent),
        ] {
            controls.push(
                Button::new(id)
                    .custom(crate::theme::canvas_toolbar_button(cx))
                    .small()
                    .label(text(locale, key))
                    .selected(self.view3d.transparent == transparent)
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.view3d.transparent = transparent;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let selected = self.view3d.z_scale;
        let view = cx.entity().downgrade();
        controls.push(
            Button::new("3d-z-scale")
                .custom(crate::theme::canvas_toolbar_button(cx))
                .small()
                .label(
                    Message::new(Key::Preview3dZScale)
                        .arg("scale", selected as u64)
                        .display(locale),
                )
                .native_tooltip(text(locale, Key::Preview3dDisplayOnly))
                .dropdown_caret(true)
                .disabled(!enabled)
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.label(text(locale, Key::Preview3dDisplayOnly));
                    for scale in [1_u64, 2, 5, 10, 20] {
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(
                                Message::new(Key::Preview3dZScale)
                                    .arg("scale", scale)
                                    .display(locale),
                            )
                            .checked(selected == scale as f64)
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.view3d.z_scale = scale as f64;
                                    this.fit_3d();
                                    cx.notify();
                                });
                            }),
                        );
                    }
                    menu
                })
                .into_any_element(),
        );
        div()
            .absolute()
            .top_12()
            .left_2()
            .right_2()
            .flex()
            .justify_center()
            .child(
                super::super::layout::canvas_bar(controls)
                    .flex_wrap()
                    .max_w_full(),
            )
    }

    pub(in crate::viewport) fn render_3d_navigation(
        &self,
        locale: Locale,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let enabled = self.view3d.scene.is_some();
        let mut controls = vec![
            Button::new("3d-fit")
                .custom(crate::theme::canvas_toolbar_button(cx))
                .small()
                .icon(IconName::Scan)
                .label(text(locale, Key::FitBoard))
                .disabled(!enabled)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.fit_3d();
                    cx.notify();
                }))
                .into_any_element(),
        ];
        for (id, view, key) in [
            ("3d-top", CameraView3d::Top, Key::View3dTop),
            ("3d-bottom", CameraView3d::Bottom, Key::View3dBottom),
            (
                "3d-isometric",
                CameraView3d::Isometric,
                Key::View3dIsometric,
            ),
        ] {
            controls.push(
                Button::new(id)
                    .custom(crate::theme::canvas_toolbar_button(cx))
                    .small()
                    .label(text(locale, key))
                    .selected(self.view3d.camera.is_view(view))
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.view3d.camera.set_view(view);
                        this.fit_3d();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        controls.push(
            div()
                .px_2()
                .text_xs()
                .child(text(locale, Key::View3dOrthographic))
                .into_any_element(),
        );
        controls
    }

    pub(in crate::viewport) fn render_3d_note(&self, locale: Locale) -> Div {
        let preview = self
            .view3d
            .scene
            .as_ref()
            .map_or_else(PreviewStackup::default, |s| s.preview);
        let warnings = self.view3d.scene.as_ref().map_or(0, |s| s.warning_count);
        div()
            .absolute()
            .top_24()
            .left_4()
            .right_4()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap_2()
            .text_xs()
            .text_color(crate::theme::canvas_colors().1)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(crate::theme::canvas_toolbar_surface())
                    .child(
                        Message::new(Key::Preview3dStackup)
                            .arg("board", preview.board_mm.to_string())
                            .arg("copper", preview.copper_mm.to_string())
                            .arg("plating", preview.plating_mm.to_string())
                            .display(locale),
                    ),
            )
            .when(warnings > 0, |note| {
                note.child(
                    Message::new(Key::Preview3dWarnings)
                        .arg("count", warnings)
                        .display(locale),
                )
            })
    }
}
