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
            ColorTarget::Grid => crate::theme::canvas_colors().2,
            ColorTarget::Drill => hsla([0.46, 0.49, 0.51, 1.0]),
            ColorTarget::Net(net, kind) => {
                let whole = self.display.appearance.nets.get(&net).and_then(|colors| {
                    (kind != pomelo_core::appearance::net::NetColorKind::All)
                        .then_some(colors.all)
                        .flatten()
                });
                hsla(
                    whole
                        .map(RgbColor::rgba)
                        .or_else(|| pomelo_render::scene::colors::net_color(net))
                        .unwrap_or([0.6, 0.68, 0.73, 1.0]),
                )
            }
            ColorTarget::Etch(layer) | ColorTarget::Pin(layer) | ColorTarget::Via(layer) => hsla(
                self.colors
                    .get(&layer)
                    .copied()
                    .unwrap_or([0.6, 0.68, 0.73, 1.0]),
            ),
        }
    }
    pub(super) fn display_color(&self, target: ColorTarget) -> Hsla {
        self.display
            .appearance
            .color(target)
            .map_or_else(|| self.source_color(target), |color| hsla(color.rgba()))
    }
    pub(super) fn canvas_background(&self) -> Hsla {
        self.display_color(ColorTarget::Background)
    }
    /// A labeled color row for settings lists.
    pub(super) fn color_control(
        &mut self,
        target: ColorTarget,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = text(locale, color_target_key(target));
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_w_0()
            .child(div().flex_1().min_w_0().text_sm().truncate().child(label))
            .child(self.color_swatch(target, locale, window, cx))
            .into_any_element()
    }

    /// The picker and its restore command, for rows that already name the target.
    pub(super) fn color_swatch(
        &mut self,
        target: ColorTarget,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = text(locale, color_target_key(target));
        let value = self.display_color(target);
        let picker = if let Some(picker) = self.color_pickers.get(&target) {
            picker.clone()
        } else {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
            cx.subscribe_in(
                &picker,
                window,
                move |this, _picker, event: &ColorPickerEvent, window, cx| {
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
                    this.sync_color_pickers(target, window, cx);
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
            .gap_1()
            .flex_shrink_0()
            .children(self.pattern_button(target, locale, cx))
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
                        this.sync_color_pickers(target, window, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn sync_color_pickers(
        &mut self,
        target: ColorTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets: Vec<_> = self
            .color_pickers
            .keys()
            .copied()
            .filter(|other| match (target, *other) {
                (ColorTarget::Net(net, _), ColorTarget::Net(candidate, _)) => net == candidate,
                _ => *other == target,
            })
            .collect();
        for target in targets {
            let value = self.display_color(target);
            if let Some(picker) = self.color_pickers.get(&target) {
                picker.update(cx, |picker, cx| picker.set_value(value, window, cx));
            }
        }
    }
}

fn color_target_key(target: ColorTarget) -> Key {
    match target {
        ColorTarget::Background => Key::CanvasBackgroundColor,
        ColorTarget::Grid => Key::GridColor,
        ColorTarget::Drill => Key::DrillColor,
        ColorTarget::Etch(_) => Key::TraceColor,
        ColorTarget::Pin(_) => Key::PadColor,
        ColorTarget::Via(_) => Key::ViaColor,
        ColorTarget::Net(_, kind) => match kind {
            pomelo_core::appearance::net::NetColorKind::All => Key::NetColorAll,
            pomelo_core::appearance::net::NetColorKind::Traces => Key::LayerTraces,
            pomelo_core::appearance::net::NetColorKind::Pins => Key::PadColor,
            pomelo_core::appearance::net::NetColorKind::Vias => Key::ViaColor,
            pomelo_core::appearance::net::NetColorKind::Shapes => Key::NetColorShapes,
        },
    }
}
