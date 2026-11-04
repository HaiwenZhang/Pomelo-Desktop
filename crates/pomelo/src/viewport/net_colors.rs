//! Network material controls reuse the document color pickers and Display panel.
use super::*;
use pomelo_core::{
    appearance::{ColorTarget, net::NetColorKind},
    model::NetId,
};
impl BoardViewport {
    pub(super) fn open_net_colors(&mut self, net: NetId, cx: &mut Context<Self>) {
        self.net_color_target = Some(net);
        self.right_panel = RightPanelTab::Display;
        self.inspector_focus
            .scroll
            .set_offset(gpui_kit::point(px(0.0), px(0.0)));
        self.panel_layout.update(cx, |layout, cx| {
            if layout.preferences.right_collapsed {
                layout.toggle(crate::workbench::panel_layout::Side::Right, cx);
            }
        });
        cx.notify();
    }
    pub(super) fn net_color_controls(
        &mut self,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let net = self.net_color_target?;
        let name = self
            .scene
            .nets
            .get(&net)
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| net.0.to_string());
        let title = Message::new(Key::NetColorsFor)
            .arg("name", name)
            .display(locale);
        let mut rows = vec![
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_1()
                .child(
                    Button::new("reset-net-colors")
                        .ghost()
                        .small()
                        .icon(IconName::RotateCcw)
                        .label(text(locale, Key::ResetNetColors))
                        .accessibility_label(text(locale, Key::ResetNetColors))
                        .disabled(!self.display.appearance.nets.contains_key(&net))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let target = ColorTarget::Net(net, NetColorKind::All);
                            Arc::make_mut(&mut this.display)
                                .appearance
                                .set_color(target, None);
                            this.sync_color_pickers(target, window, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("close-net-colors")
                        .ghost()
                        .small()
                        .icon(IconName::X)
                        .accessibility_label(text(locale, Key::CloseNetColors))
                        .native_tooltip(text(locale, Key::CloseNetColors))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.net_color_target = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        ];
        for (index, kind) in NetColorKind::ALL.into_iter().enumerate() {
            let control = self.color_control(ColorTarget::Net(net, kind), locale, window, cx);
            rows.push(
                FocusScroll::new(
                    ("net-color-control", index as u64),
                    &self.inspector_focus.scroll,
                    control,
                )
                .w_full()
                .min_h_8()
                .into_any_element(),
            );
        }
        rows.push(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text(locale, Key::NetColorFallback))
                .into_any_element(),
        );
        if self.display.appearance.hide_net_colors {
            rows.push(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::NetColorsHidden))
                    .into_any_element(),
            );
        }
        Some(crate::panels::display::section(title, rows, cx))
    }
}
