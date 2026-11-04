//! Document-local material patterns, composed with native menus and a bitmap swatch.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::appearance::{ColorTarget, pattern::FillPattern};
impl BoardViewport {
    pub(super) fn pattern_button(
        &self,
        target: ColorTarget,
        locale: pomelo_core::i18n::Locale,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let (layer, kind, key) = match target {
            ColorTarget::Etch(l) => (l, 0, Key::LayerTraces),
            ColorTarget::Pin(l) => (l, 1, Key::LayerPads),
            ColorTarget::Via(l) => (l, 2, Key::LayerVias),
            _ => return None,
        };
        let selected = self.display.appearance.pattern(target);
        let name = Message::new(Key::MaterialPattern)
            .arg("target", text(locale, key))
            .arg("pattern", pattern_name(selected, locale))
            .display(locale);
        let view = cx.entity().downgrade();
        let color = cx.theme().foreground;
        Some(
            Button::new(("material-pattern", u64::from(layer.0) * 3 + kind))
                .ghost()
                .small()
                .size_6()
                .accessibility_label(name.clone())
                .native_tooltip(name)
                .child(pattern_swatch(selected.unwrap_or_default(), color))
                .dropdown_menu(move |mut menu, _, _| {
                    for pattern in
                        std::iter::once(None).chain(FillPattern::ALL.into_iter().map(Some))
                    {
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(pattern_name(pattern, locale))
                                .checked(pattern == selected)
                                .on_click(move |_, _, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        Arc::make_mut(&mut this.display)
                                            .appearance
                                            .set_pattern(target, pattern);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element(),
        )
    }
}
fn pattern_swatch(pattern: FillPattern, color: Hsla) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let step = bounds.size.width / 16.0;
            for y in 0..16 {
                for x in 0..16 {
                    if pattern.ink(x, y) {
                        let origin =
                            bounds.origin + gpui_kit::point(step * x as f32, step * y as f32);
                        window.paint_quad(quad(
                            Bounds::new(origin, gpui_kit::size(step, step)),
                            px(0.0),
                            color,
                            px(0.0),
                            color,
                            BorderStyle::Solid,
                        ));
                    }
                }
            }
        },
    )
    .w_4()
    .h_4()
    .into_any_element()
}
fn pattern_name(pattern: Option<FillPattern>, locale: pomelo_core::i18n::Locale) -> String {
    let key = match pattern {
        None => Key::PatternDefault,
        Some(FillPattern::Solid) => Key::PatternSolid,
        Some(FillPattern::DiagonalDown) => Key::PatternDiagonalDown,
        Some(FillPattern::DiagonalUp) => Key::PatternDiagonalUp,
        Some(FillPattern::Horizontal) => Key::PatternHorizontal,
        Some(FillPattern::Vertical) => Key::PatternVertical,
        Some(FillPattern::SparseDots) => Key::PatternSparseDots,
        Some(FillPattern::Dots) => Key::PatternDots,
        Some(FillPattern::DenseDots) => Key::PatternDenseDots,
        Some(FillPattern::VerticalDashes) => Key::PatternVerticalDashes,
        Some(FillPattern::Crosshatch) => Key::PatternCrosshatch,
        Some(FillPattern::SquareGrid) => Key::PatternSquareGrid,
        Some(FillPattern::Checkerboard) => Key::PatternCheckerboard,
        Some(FillPattern::Rings) => Key::PatternRings,
        Some(FillPattern::Diamonds) => Key::PatternDiamonds,
        Some(FillPattern::Crosses) => Key::PatternCrosses,
        Some(FillPattern::DenseCheckerboard) => Key::PatternDenseCheckerboard,
    };
    text(locale, key)
}
