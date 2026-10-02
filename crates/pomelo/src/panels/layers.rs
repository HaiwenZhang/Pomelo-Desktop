//! Layer row presentation. Visibility, ordering and scroll state stay in the viewport.
use gpui_kit::*;
use gpui_kit::{
    base::Disableable,
    component::{
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
    },
};
use pomelo_core::{
    i18n::{Locale, MessageKey, text},
    model::LayerId,
};

pub struct LayerRow {
    pub id: LayerId,
    pub label: String,
    pub visible: bool,
    pub at_bottom: bool,
    pub at_top: bool,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type VisibilityHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
pub struct LayerCommands {
    pub visibility: VisibilityHandler,
    pub to_bottom: ClickHandler,
    pub to_top: ClickHandler,
}

pub fn row(locale: Locale, state: LayerRow, commands: LayerCommands) -> AnyElement {
    div()
        .px_3()
        .py_1()
        .child(
            Checkbox::new(("layer-visible", state.id.0))
                .child(div().truncate().child(state.label.clone()))
                .accessibility_label(state.label.clone())
                .tooltip(state.label)
                .checked(state.visible)
                .on_change(commands.visibility),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new(("layer-to-bottom", state.id.0))
                        .ghost()
                        .label(text(locale, MessageKey::LayerToBottom))
                        .disabled(state.at_bottom)
                        .on_click(commands.to_bottom),
                )
                .child(
                    Button::new(("layer-to-top", state.id.0))
                        .ghost()
                        .label(text(locale, MessageKey::LayerToTop))
                        .disabled(state.at_top)
                        .on_click(commands.to_top),
                ),
        )
        .into_any_element()
}
