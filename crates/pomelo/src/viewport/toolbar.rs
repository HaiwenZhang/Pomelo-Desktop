//! Board commands grouped by file access, navigation, search and coloring.
use std::rc::Rc;

use gpui_kit::{
    assets::IconName,
    base::Selectable,
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonGroup, ButtonVariants},
        input::{Input, InputState},
        separator::Separator,
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::{
    display::ColorMode,
    i18n::{Locale, MessageKey as Key, text},
};

use crate::tooltips::ButtonTooltipExt;

pub(super) struct State<'a> {
    pub locale: Locale,
    pub pan_tool: bool,
    pub color_mode: ColorMode,
    pub search: &'a Entity<InputState>,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ColorHandler = Box<dyn Fn(&ColorMode, &mut Window, &mut App)>;

pub(super) struct Commands {
    pub select: ClickHandler,
    pub pan: ClickHandler,
    pub colors: ColorHandler,
    pub fit: ClickHandler,
}

pub(super) fn render(state: State<'_>, commands: Commands, cx: &App) -> AnyElement {
    let locale = state.locale;
    let colors = Rc::new(commands.colors);
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("board-open")
                .ghost()
                .large()
                .icon(IconName::FolderOpen)
                .label(text(locale, Key::OpenFile))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, Key::ShortcutOpen)),
                )
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::OpenFile), cx)
                }),
        )
        .child(Separator::vertical().h_8().flex_shrink_0())
        .child(
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap_2()
                .child(
                    Button::new("select-tool")
                        .outline()
                        .large()
                        .when(!state.pan_tool, |button| button.primary())
                        .icon(IconName::MousePointer2)
                        .selected(!state.pan_tool)
                        .toggled(!state.pan_tool)
                        .accessibility_label(text(locale, Key::SelectTool))
                        .native_tooltip(text(locale, Key::SelectTool))
                        .on_click(commands.select),
                )
                .child(
                    Button::new("pan-tool")
                        .outline()
                        .large()
                        .when(state.pan_tool, |button| button.primary())
                        .icon(IconName::Hand)
                        .selected(state.pan_tool)
                        .toggled(state.pan_tool)
                        .accessibility_label(text(locale, Key::PanTool))
                        .native_tooltip(text(locale, Key::PanTool))
                        .on_click(commands.pan),
                ),
        )
        .child(Separator::vertical().h_8().flex_shrink_0())
        .child(
            div().flex_1().min_w_0().child(
                Input::new(state.search)
                    .h_9()
                    .prefix(Icon::new(IconName::Search).large())
                    .suffix(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text(locale, Key::ShortcutSearch)),
                    )
                    .aria_label(text(locale, Key::SearchBoard)),
            ),
        )
        .child(
            ButtonGroup::new("board-color-mode")
                .flex_shrink_0()
                .children(
                    [
                        ("layer-colors", ColorMode::Layer, Key::LayerColor),
                        ("net-colors", ColorMode::Net, Key::NetColor),
                    ]
                    .into_iter()
                    .map(|(id, mode, key)| {
                        let command = Rc::clone(&colors);
                        Button::new(id)
                            .large()
                            .min_w(rems(7.0))
                            .map(|button| {
                                if state.color_mode == mode {
                                    button.primary()
                                } else {
                                    button.outline()
                                }
                            })
                            .selected(state.color_mode == mode)
                            .label(text(locale, key))
                            .native_tooltip(text(locale, key))
                            .on_click(move |_, window, cx| command(&mode, window, cx))
                    }),
                ),
        )
        .child(Separator::vertical().h_8().flex_shrink_0())
        .child(
            Button::new("fit-board")
                .outline()
                .large()
                .icon(IconName::Scan)
                .label(text(locale, Key::FitBoard))
                .accessibility_label(text(locale, Key::FitBoard))
                .native_tooltip(text(locale, Key::FitBoard))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, Key::ShortcutFit)),
                )
                .on_click(commands.fit),
        )
        .into_any_element()
}
