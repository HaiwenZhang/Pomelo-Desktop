//! Global and copper opacity rows for the Display panel.
use super::*;
use gpui_kit::component::slider::Slider;

impl BoardViewport {
    pub(super) fn opacity_controls(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &App,
    ) -> Vec<AnyElement> {
        [
            (
                "shadow-brightness-slider",
                Key::ShadowBrightness,
                &self.shadow_slider,
                self.display.shadow.brightness,
            ),
            (
                "global-opacity-slider",
                Key::GlobalOpacityLabel,
                &self.global_slider,
                self.display.global_opacity,
            ),
            (
                "copper-opacity-slider",
                Key::CopperOpacityLabel,
                &self.copper_slider,
                self.display.copper_opacity,
            ),
        ]
        .into_iter()
        .map(|(id, key, slider, value)| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_h_8()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .truncate()
                        .child(text(locale, key)),
                )
                .child(
                    div()
                        .w(relative(0.42))
                        .flex_shrink_0()
                        .child(FocusScroll::new(
                            id,
                            &self.inspector_focus.scroll,
                            Slider::new(slider)
                                .accessibility_label(text(locale, key))
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary),
                        )),
                )
                .child(
                    div()
                        .w_10()
                        .flex_shrink_0()
                        .text_xs()
                        .text_right()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            Message::new(Key::UiPercent)
                                .arg("value", (value * 100.0).round() as u32)
                                .display(locale),
                        ),
                )
                .into_any_element()
        })
        .collect()
    }
}
