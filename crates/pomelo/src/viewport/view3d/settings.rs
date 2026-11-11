//! 3D section of the right panel's display page: the full controls behind the
//! toolbar's display mode and Z stretch, plus selection feedback.
use super::*;
use crate::panels::display::{section, setting_switch, switch_grid};
use gpui_kit::component::tab::{Tab, TabBar};
use pomelo_core::{
    display3d::{Display3d, RenderMode3d, Z_SCALE_PRESETS, ZStretch},
    i18n::Locale,
};

/// Opacity kept by unselected objects, as offered in the panel.
const DIM_STEPS: [f32; 4] = [1.0, 0.7, 0.3, 0.1];

impl BoardViewport {
    fn update_3d_settings(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Display3d)) {
        let before = self.view3d.settings;
        change(&mut self.view3d.settings);
        if before != self.view3d.settings {
            if before.z_scale != self.view3d.settings.z_scale
                || before.stretch != self.view3d.settings.stretch
            {
                self.fit_3d();
            }
            cx.notify();
        }
    }

    pub(in crate::viewport) fn render_3d_settings(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.view3d.settings;
        let scroll = &self.inspector_focus.scroll;
        let theme = cx.theme();
        let row = |label: Key, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .min_h_8()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, label)),
                )
                .child(control)
                .into_any_element()
        };
        let modes = controls::RENDER_MODES;
        let mode = TabBar::new("3d-mode-tabs")
            .segmented()
            .xsmall()
            .selected_index(
                modes
                    .iter()
                    .position(|(m, _, _)| *m == settings.mode)
                    .unwrap_or(0),
            )
            .children(
                modes
                    .iter()
                    .map(|(_, key, _)| Tab::new().label(text(locale, *key))),
            )
            .on_click(cx.listener(move |this, index: &usize, _, cx| {
                let mode = modes
                    .get(*index)
                    .map_or(RenderMode3d::Default, |(m, _, _)| *m);
                this.update_3d_settings(cx, |s| s.mode = mode);
            }))
            .into_any_element();
        let scale = TabBar::new("3d-z-tabs")
            .segmented()
            .xsmall()
            .selected_index(
                Z_SCALE_PRESETS
                    .iter()
                    .position(|v| *v == settings.z_scale)
                    .unwrap_or(usize::MAX),
            )
            .children(Z_SCALE_PRESETS.iter().map(|v| {
                Tab::new().label(
                    Message::new(Key::Preview3dZScale)
                        .arg("scale", controls::format_scale(*v))
                        .display(locale),
                )
            }))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(scale) = Z_SCALE_PRESETS.get(*index).copied() {
                    this.update_3d_settings(cx, |s| s.z_scale = scale);
                }
            }))
            .into_any_element();
        let stretch = TabBar::new("3d-stretch-tabs")
            .segmented()
            .xsmall()
            .selected_index(usize::from(settings.stretch == ZStretch::All))
            .child(Tab::new().label(text(locale, Key::StretchDielectric)))
            .child(Tab::new().label(text(locale, Key::StretchAll)))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                let stretch = if *index == 0 {
                    ZStretch::Dielectric
                } else {
                    ZStretch::All
                };
                this.update_3d_settings(cx, |s| s.stretch = stretch);
            }))
            .into_any_element();
        let dim = TabBar::new("3d-dim-tabs")
            .segmented()
            .xsmall()
            .selected_index(
                DIM_STEPS
                    .iter()
                    .position(|v| (*v - settings.unselected_opacity).abs() < 0.005)
                    .unwrap_or(usize::MAX),
            )
            .children(DIM_STEPS.iter().map(|v| {
                Tab::new().label(
                    Message::new(Key::UiPercent)
                        .arg("value", (v * 100.0).round() as u32)
                        .display(locale),
                )
            }))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(value) = DIM_STEPS.get(*index).copied() {
                    this.update_3d_settings(cx, |s| s.unselected_opacity = value);
                }
            }))
            .into_any_element();
        let toggle = |id: &'static str, key: Key, checked: bool, set: fn(&mut Display3d, bool)| {
            setting_switch(
                id,
                text(locale, key),
                text(locale, key),
                checked,
                Box::new(cx.listener(move |this, value: &bool, _, cx| {
                    let value = *value;
                    this.update_3d_settings(cx, |s| set(s, value));
                })),
                scroll,
            )
        };
        let switches = vec![
            toggle(
                "3d-hover-preview",
                Key::HoverPreview,
                settings.hover_preview,
                |s, v| s.hover_preview = v,
            ),
            toggle(
                "3d-selected-solid",
                Key::SelectedSolid,
                settings.selected_solid,
                |s, v| s.selected_solid = v,
            ),
            toggle(
                "3d-show-dielectric",
                Key::ShowDielectric,
                settings.show_dielectric,
                |s, v| s.show_dielectric = v,
            ),
            toggle(
                "3d-show-stubs",
                Key::ShowStubs,
                settings.show_stubs,
                |s, v| s.show_stubs = v,
            ),
        ];
        div()
            .flex()
            .flex_col()
            .child(section(
                text(locale, Key::Display3dSection),
                vec![
                    row(Key::ToolbarDisplayMode, mode),
                    row(Key::ZStretchFactor, scale),
                    row(Key::StretchTarget, stretch),
                    div()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::StretchHint))
                        .into_any_element(),
                ],
                cx,
            ))
            .child(section(
                text(locale, Key::SelectionFeedback),
                vec![row(Key::UnselectedOpacity, dim), switch_grid(switches)],
                cx,
            ))
            .into_any_element()
    }
}
