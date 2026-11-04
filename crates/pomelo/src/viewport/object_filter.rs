//! Screen-size filtering controls; the viewport remains the state owner.
use super::*;
use gpui_kit::component::{checkbox::Checkbox, slider::Slider};
use pomelo_core::display::filter::FilterKind;
impl BoardViewport {
    pub(super) fn object_filter_controls(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows = vec![
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_h_8()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .child(text(locale, Key::ObjectFilterThreshold)),
                )
                .child(
                    div().w(relative(0.42)).child(FocusScroll::new(
                        "object-filter-slider",
                        &self.inspector_focus.scroll,
                        Slider::new(&self.object_filter_slider)
                            .accessibility_label(text(locale, Key::ObjectFilterThreshold))
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary),
                    )),
                )
                .child(
                    div().w_12().text_xs().text_right().child(
                        Message::new(Key::ObjectFilterPixels)
                            .arg("pixels", self.display.object_filter.minimum_pixels)
                            .display(locale),
                    ),
                )
                .into_any_element(),
        ];
        let switches = [
            (FilterKind::Text, Key::ShortBoardText, "filter-text"),
            (FilterKind::Via, Key::LayerVias, "filter-vias"),
            (FilterKind::Pin, Key::LayerPads, "filter-pins"),
            (FilterKind::Copper, Key::PickCopper, "filter-copper"),
            (FilterKind::Void, Key::FilterVoids, "filter-voids"),
            (
                FilterKind::EmbeddedName,
                Key::DisplayNetLabels,
                "filter-net-names",
            ),
        ];
        rows.push(
            div()
                .grid()
                .grid_cols(2)
                .gap_2()
                .children(switches.into_iter().map(|(kind, key, id)| {
                    FocusScroll::new(
                        id,
                        &self.inspector_focus.scroll,
                        Checkbox::new(id)
                            .small()
                            .label(text(locale, key))
                            .checked(self.display.object_filter.enabled(kind))
                            .disabled(self.display.object_filter.minimum_pixels == 0)
                            .on_change(cx.listener(move |this, enabled: &bool, _, cx| {
                                Arc::make_mut(&mut this.display)
                                    .object_filter
                                    .set(kind, *enabled);
                                this.invalidate_hover();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
                }))
                .into_any_element(),
        );
        rows
    }
}
