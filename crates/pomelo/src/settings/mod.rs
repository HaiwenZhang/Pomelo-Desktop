//! Immediate application preferences. Document state remains owned by the workbench.

use gpui_kit::component::{
    ActiveTheme, WindowExt,
    button::Button,
    radio::{Radio, RadioGroup},
};
use gpui_kit::*;
use pomelo_core::i18n::{LanguagePreference, Locale, MessageKey as Key, text};

use crate::{i18n, prefs::ThemePreference, theme};

pub fn open(window: &mut Window, cx: &mut App) {
    // Repeated menu/shortcut activation must not stack settings surfaces.
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_dialog(cx, |dialog, _, cx| {
        // Read on each render so the open dialog also follows language/theme changes.
        let locale = i18n::current(cx);
        let language = cx.global::<i18n::LanguageState>();
        let preference = language.preference;
        let language_warning = language.warning.clone();
        let appearance = cx.global::<theme::ThemeState>();
        let selected_theme = appearance.preference;
        let theme_warning = appearance.warning.clone();
        let languages = std::iter::once(LanguagePreference::System)
            .chain(Locale::ALL.map(LanguagePreference::Explicit))
            .collect::<Vec<_>>();
        let selected_language = languages
            .iter()
            .position(|candidate| *candidate == preference);
        let language_controls = RadioGroup::new("settings-language")
            .selected_index(selected_language)
            .children(languages.iter().enumerate().map(|(index, candidate)| {
                let label = match candidate {
                    LanguagePreference::System => text(locale, Key::FollowSystem),
                    LanguagePreference::Explicit(locale) => locale.native_name().to_owned(),
                };
                Radio::new(("settings-language-option", index)).label(label)
            }))
            .on_change(move |index, _, cx| {
                if let Some(preference) = languages.get(*index) {
                    i18n::select(*preference, cx);
                }
            });
        let theme_controls = RadioGroup::new("settings-theme")
            .layout(Axis::Horizontal)
            .selected_index(Some(match selected_theme {
                ThemePreference::Dark => 0,
                ThemePreference::Light => 1,
            }))
            .child(Radio::new("dark").label(text(locale, Key::DarkTheme)))
            .child(Radio::new("light").label(text(locale, Key::LightTheme)))
            .on_change(|index, window, cx| {
                let preference = match index {
                    0 => ThemePreference::Dark,
                    1 => ThemePreference::Light,
                    _ => return,
                };
                theme::select(preference, window, cx);
            });
        let mut language_section = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(text(locale, Key::SettingsLanguage)),
            )
            .child(language_controls)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::SettingsLanguageHint)),
            );
        if let Some(warning) = language_warning {
            language_section = language_section.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(warning.message.display(locale)),
            );
        }
        let mut theme_section = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(text(locale, Key::SettingsTheme)),
            )
            .child(theme_controls);
        if let Some(warning) = theme_warning {
            theme_section = theme_section.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(warning.message.display(locale)),
            );
        }
        dialog
            // The component's built-in X has an English-only accessible name.
            // Our translated Close command and Escape provide the same dismissal.
            .close_button(false)
            .title(text(locale, Key::SettingsTitle))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(language_section)
                    .child(theme_section),
            )
            .footer(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text(locale, Key::SettingsImmediate)),
                    )
                    .child(
                        div().flex().justify_end().child(
                            Button::new("settings-close")
                                .outline()
                                .label(text(locale, Key::SettingsClose))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                    ),
            )
    });
}

/// Native Help menu destination; version is the running application package.
pub fn open_about(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_dialog(cx, |dialog, _, cx| {
        let locale = i18n::current(cx);
        dialog
            .close_button(false)
            .title("Pomelo")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(text(locale, Key::ProductViewer))
                    .child(env!("CARGO_PKG_VERSION"))
                    .child(text(locale, Key::LocalReadOnly)),
            )
            .footer(
                Button::new("about-close")
                    .label(text(locale, Key::SettingsClose))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    });
}
