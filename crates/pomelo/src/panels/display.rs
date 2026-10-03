//! Display controls; mutations and renderer invalidation belong to the viewport.
use super::focus_scroll::FocusScroll;
use gpui_kit::*;
use gpui_kit::{
    base::Disableable,
    component::{
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
    },
};
use pomelo_core::{
    display::BoardDisplay,
    i18n::{Locale, Message, MessageKey as Key, text},
};
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type VisibilityHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
pub struct DisplayCommands {
    pub filled: VisibilityHandler,
    pub drills: VisibilityHandler,
    pub backdrills: VisibilityHandler,
    pub copper: VisibilityHandler,
    pub texts: VisibilityHandler,
    pub drawings: VisibilityHandler,
    pub decrease_opacity: ClickHandler,
    pub increase_opacity: ClickHandler,
    pub reset_order: ClickHandler,
}
pub fn controls(
    locale: Locale,
    state: &BoardDisplay,
    commands: DisplayCommands,
    scroll: &ScrollHandle,
) -> Vec<AnyElement> {
    vec![
        div()
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "fill-pads",
                scroll,
                Checkbox::new("fill-pads")
                    .label(text(locale, Key::FillPads))
                    .checked(state.filled)
                    .on_change(commands.filled),
            ))
            .into_any_element(),
        div()
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "show-drills",
                scroll,
                Checkbox::new("show-drills")
                    .label(text(locale, Key::ShowDrills))
                    .checked(state.show_drills)
                    .on_change(commands.drills),
            ))
            .into_any_element(),
        div()
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "show-backdrills",
                scroll,
                Checkbox::new("show-backdrills")
                    .label(text(locale, Key::ShowBackdrills))
                    .checked(state.show_backdrills)
                    .on_change(commands.backdrills),
            ))
            .into_any_element(),
        div()
            .id("copper-opacity")
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(FocusScroll::new(
                "show-copper",
                scroll,
                Checkbox::new("show-copper")
                    .label(text(locale, Key::ShowCopper))
                    .checked(state.show_copper)
                    .on_change(commands.copper),
            ))
            .child(
                Message::new(Key::CopperOpacity)
                    .arg("percent", (state.copper_opacity * 100.0).round() as u32)
                    .display(locale),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(FocusScroll::new(
                        "decrease-copper-opacity",
                        scroll,
                        Button::new("decrease-copper-opacity")
                            .outline()
                            .label(text(locale, Key::DecreaseOpacity))
                            .disabled(state.copper_opacity <= 0.0)
                            .on_click(commands.decrease_opacity),
                    ))
                    .child(FocusScroll::new(
                        "increase-copper-opacity",
                        scroll,
                        Button::new("increase-copper-opacity")
                            .outline()
                            .label(text(locale, Key::IncreaseOpacity))
                            .disabled(state.copper_opacity >= 1.0)
                            .on_click(commands.increase_opacity),
                    )),
            )
            .into_any_element(),
        div()
            .id("text-visibility")
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "show-texts",
                scroll,
                Checkbox::new("show-texts")
                    .label(text(locale, Key::ShowTexts))
                    .checked(state.show_texts)
                    .on_change(commands.texts),
            ))
            .into_any_element(),
        div()
            .id("drawing-visibility")
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "show-drawings",
                scroll,
                Checkbox::new("show-drawings")
                    .label(text(locale, Key::ShowDrawings))
                    .checked(state.show_drawings)
                    .on_change(commands.drawings),
            ))
            .into_any_element(),
        div()
            .px_4()
            .py_2()
            .child(FocusScroll::new(
                "reset-layer-order",
                scroll,
                Button::new("reset-layer-order")
                    .ghost()
                    .label(text(locale, Key::ResetLayerOrder))
                    .disabled(state.layer_order.is_empty())
                    .on_click(commands.reset_order),
            ))
            .into_any_element(),
    ]
}
