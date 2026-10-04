//! Exact opacity entry. Each document retains its input focus and display values.
use super::*;
use gpui_kit::component::input::NumberInput;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum OpacityTarget {
    Global,
    Copper,
}

impl BoardViewport {
    fn opacity_value(&self, target: OpacityTarget) -> f32 {
        match target {
            OpacityTarget::Global => self.display.global_opacity,
            OpacityTarget::Copper => self.display.copper_opacity,
        }
    }

    pub(super) fn opacity_control(
        &mut self,
        target: OpacityTarget,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = text(
            locale,
            match target {
                OpacityTarget::Global => Key::GlobalOpacityLabel,
                OpacityTarget::Copper => Key::CopperOpacityLabel,
            },
        );
        let value = self.opacity_value(target);
        let byte = (value * 255.0).round() as u8;
        let input = if let Some(input) = self.opacity_inputs.get(&target) {
            input.clone()
        } else {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .min(0.0)
                    .max(255.0)
                    .step(1.0)
                    .default_value(byte.to_string())
            });
            cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Ok(byte) = input.read(cx).value().trim().parse::<u8>() {
                        let value = f32::from(byte) / 255.0;
                        let display = Arc::make_mut(&mut this.display);
                        match target {
                            OpacityTarget::Global => display.global_opacity = value,
                            OpacityTarget::Copper => {
                                display.copper_opacity = value;
                                this.copper_slider.update(cx, |slider, cx| {
                                    slider.set_value(value * 100.0, window, cx)
                                });
                            }
                        }
                    }
                    cx.notify();
                } else if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    // Empty/invalid edits never alter the saved material. Return to
                    // the last applied byte on submission or leaving the control.
                    let byte = (this.opacity_value(target) * 255.0).round() as u8;
                    input.update(cx, |input, cx| {
                        input.set_value(byte.to_string(), window, cx)
                    });
                    cx.notify();
                }
            })
            .detach();
            self.opacity_inputs.insert(target, input.clone());
            input
        };
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        input.update(cx, |input, cx| {
            input.set_placeholder(label.clone(), window, cx);
            // Keep the existing percentage slider/buttons and exact entry in sync,
            // without rewriting a partial edit while the user is typing.
            if !focused && input.value().as_ref() != byte.to_string() {
                input.set_value(byte.to_string(), window, cx);
            }
        });
        let invalid = input.read(cx).value().trim().parse::<u8>().is_err();
        div()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().text_sm().child(label))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                Message::new(Key::UiPercent)
                                    .arg("value", (value * 100.0).round() as u32)
                                    .display(locale),
                            ),
                    ),
            )
            .child(FocusScroll::new(
                ("opacity", target as u64),
                &self.inspector_focus.scroll,
                NumberInput::new(&input)
                    .small()
                    .suffix(text(locale, Key::OpacityByteScale)),
            ))
            .when(invalid, |row| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(text(locale, Key::OpacityInvalid)),
                )
            })
            .into_any_element()
    }
}
