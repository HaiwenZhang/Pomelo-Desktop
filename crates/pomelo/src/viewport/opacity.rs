//! Global opacity uses the same percentage slider as the layer controls.
use super::*;
use gpui_kit::component::slider::Slider;

impl BoardViewport {
    pub(super) fn opacity_control(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &App,
    ) -> AnyElement {
        div()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::GlobalOpacityLabel)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div().flex_1().min_w_0().child(FocusScroll::new(
                            "global-opacity-slider",
                            &self.inspector_focus.scroll,
                            Slider::new(&self.global_slider)
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary),
                        )),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_sm()
                            .min_w_8()
                            .text_right()
                            .child(
                                Message::new(Key::UiPercent)
                                    .arg(
                                        "value",
                                        (self.display.global_opacity * 100.0).round() as u32,
                                    )
                                    .display(locale),
                            ),
                    ),
            )
            .into_any_element()
    }
}
