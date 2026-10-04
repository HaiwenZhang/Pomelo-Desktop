//! Native color controls; each document owns its picker state and saved overrides.
use super::*;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use pomelo_core::appearance::{ColorTarget, RgbColor};

fn hsla([r, g, b, a]: [f32; 4]) -> Hsla {
    Rgba { r, g, b, a }.into()
}
impl BoardViewport {
    fn source_color(&self, target: ColorTarget) -> Hsla {
        match target {
            ColorTarget::Background => crate::theme::canvas_colors().0,
            ColorTarget::Drill => hsla([0.46, 0.49, 0.51, 1.0]),
            ColorTarget::Etch(layer) | ColorTarget::Pin(layer) | ColorTarget::Via(layer) => hsla(
                self.colors
                    .get(&layer)
                    .copied()
                    .unwrap_or([0.6, 0.68, 0.73, 1.0]),
            ),
        }
    }
    fn display_color(&self, target: ColorTarget) -> Hsla {
        self.display
            .appearance
            .color(target)
            .map_or_else(|| self.source_color(target), |color| hsla(color.rgba()))
    }
    pub(super) fn canvas_background(&self) -> Hsla {
        self.display_color(ColorTarget::Background)
    }
    pub(super) fn color_control(
        &mut self,
        target: ColorTarget,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = match target {
            ColorTarget::Background => Key::CanvasBackgroundColor,
            ColorTarget::Drill => Key::DrillColor,
            ColorTarget::Etch(_) => Key::TraceColor,
            ColorTarget::Pin(_) => Key::PadColor,
            ColorTarget::Via(_) => Key::ViaColor,
        };
        let label = text(locale, key);
        let value = self.display_color(target);
        let picker = if let Some(picker) = self.color_pickers.get(&target) {
            picker.clone()
        } else {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
            cx.subscribe_in(
                &picker,
                window,
                move |this, picker, event: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(color) = event;
                    let color = color.map(|color| {
                        let color: Rgba = color.into();
                        RgbColor(
                            [color.r, color.g, color.b]
                                .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
                        )
                    });
                    Arc::make_mut(&mut this.display)
                        .appearance
                        .set_color(target, color);
                    // The picker supports alpha; PCB material alpha belongs to the
                    // independent opacity setting. Keep its swatch truthful as RGB8.
                    let committed = this.display_color(target);
                    picker.update(cx, |picker, cx| picker.set_value(committed, window, cx));
                    cx.notify();
                },
            )
            .detach();
            self.color_pickers.insert(target, picker.clone());
            picker
        };
        let restore_name = Message::new(Key::RestoreDisplayColor)
            .arg("target", label.clone())
            .display(locale);
        // The displayed swatch is PCB data, independent of the application's theme.
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(div().flex_1().min_w_0().text_sm().child(label.clone()))
            .child(ColorPicker::new(&picker).small().accessibility_label(label))
            .child(
                Button::new(("restore-color", picker.entity_id()))
                    .ghost()
                    .small()
                    .icon(IconName::RotateCcw)
                    .accessibility_label(restore_name.clone())
                    .native_tooltip(restore_name)
                    .disabled(self.display.appearance.color(target).is_none())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        Arc::make_mut(&mut this.display)
                            .appearance
                            .set_color(target, None);
                        let source = this.source_color(target);
                        if let Some(picker) = this.color_pickers.get(&target) {
                            picker.update(cx, |picker, cx| picker.set_value(source, window, cx));
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
