//! Welcome surface composition; native file commands belong to the workbench.
pub mod recent;
use gpui_kit::component::{ActiveTheme, button::Button};
use gpui_kit::*;
use pomelo_core::i18n::{Locale, Message, MessageKey as Key, text};

pub fn render(
    locale: Locale,
    recent: AnyElement,
    cx: &App,
    on_open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .p_8()
        .gap_6()
        .child(div().text_2xl().child(text(locale, Key::WelcomeTitle)))
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child(text(locale, Key::WelcomeDescription)),
        )
        .child(
            div()
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .p_8()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    Message::new(Key::DropHint)
                        .arg(
                            "shortcut",
                            if cfg!(target_os = "macos") {
                                "⌘O"
                            } else {
                                "Ctrl+O"
                            },
                        )
                        .display(locale),
                )
                .child(
                    Button::new("welcome-open")
                        .label(text(locale, Key::OpenFile))
                        .on_click(on_open),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text(locale, Key::DevelopmentStage)),
        )
        .child(recent)
        .into_any_element()
}
