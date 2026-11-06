//! Board-space grid display; no placement, snapping or source-grid mutation.
use super::*;
impl BoardViewport {
    pub(super) fn render_grid(&self) -> Option<AnyElement> {
        if self.view3d.active {
            return None;
        }
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        let points = self.display.grid.points(camera, size);
        if points.is_empty() {
            return None;
        }
        let color = self.display_color(pomelo_core::appearance::ColorTarget::Grid);
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    for point in &points {
                        let view = camera.board_to_view(*point, size.x, size.y);
                        let center =
                            bounds.origin + gpui_kit::point(px(view.x as f32), px(view.y as f32));
                        window.paint_quad(quad(
                            Bounds::centered_at(center, gpui_kit::size(px(1.5), px(1.5))),
                            px(0.75),
                            color,
                            px(0.0),
                            color,
                            BorderStyle::Solid,
                        ));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }
    pub(super) fn grid_spacing_control(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &Context<Self>,
    ) -> AnyElement {
        let spacing = self.display.grid.spacing_mm;
        let step = |id: &'static str, icon: IconName, factor: f64| {
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .accessibility_label(text(
                    locale,
                    if factor < 1.0 {
                        Key::FinerGrid
                    } else {
                        Key::CoarserGrid
                    },
                ))
                .disabled(if factor < 1.0 {
                    spacing <= 1e-6
                } else {
                    spacing >= 1e6
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let grid = &mut Arc::make_mut(&mut this.display).grid;
                    grid.spacing_mm = (grid.spacing_mm * factor).clamp(1e-6, 1e6);
                    cx.notify();
                }))
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_h_8()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .child(text(locale, Key::GridSpacing)),
            )
            .child(FocusScroll::new(
                "finer-grid",
                &self.inspector_focus.scroll,
                step("finer-grid", IconName::Minus, 0.5),
            ))
            .child(div().text_xs().child(review::format_length(
                spacing,
                self.display.length_unit,
                locale,
            )))
            .child(FocusScroll::new(
                "coarser-grid",
                &self.inspector_focus.scroll,
                step("coarser-grid", IconName::Plus, 2.0),
            ))
            .into_any_element()
    }
}
