//! Shared diagnostic page presentation; document owners retain paging and expansion.
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{component::ActiveTheme, component::scroll::ScrollableElement, *};
use pomelo_core::{
    i18n::{Locale, Message, MessageKey as Key, text},
    model::{Diagnostic, Severity},
};

/// Stable pagination focus handles owned by the surrounding document view.
pub struct NavigationFocus {
    pub previous: FocusHandle,
    pub next: FocusHandle,
}

impl NavigationFocus {
    pub fn new(cx: &mut App) -> Self {
        Self {
            previous: cx.focus_handle(),
            next: cx.focus_handle(),
        }
    }

    /// Keep focus on an enabled command when a page change disables its trigger.
    pub fn focus_boundary(&self, page: usize, last: usize, window: &mut Window, cx: &mut App) {
        if last == 0 {
            return;
        }
        if page == 0 {
            window.focus(&self.next, cx);
        } else if page == last {
            window.focus(&self.previous, cx);
        }
    }
}

/// Theme a standard Base Button while preserving caller-owned keyboard focus.
pub fn page_button(
    id: &'static str,
    label: String,
    focus: &FocusHandle,
    disabled: bool,
    window: &Window,
    cx: &App,
) -> gpui_kit::base::Button {
    gpui_kit::base::Button::new(id)
        .track_focus(focus)
        .disabled(disabled)
        .h_8()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(transparent_black())
        .text_sm()
        .text_color(cx.theme().foreground)
        .when(!disabled && focus.is_focused(window), |button| {
            button.border_color(cx.theme().ring)
        })
        .when(!disabled, |button| {
            button
                .hover(|style| style.bg(cx.theme().button_hover))
                .active(|style| style.bg(cx.theme().button_active))
                .focus(|style| style.border_color(cx.theme().ring))
        })
        .styles(|styles| styles.disabled(|style| style.opacity(0.5)))
        .accessibility_label(label.clone())
        .child(label)
}

/// Render an already bounded page without limiting the height of its content.
pub fn page_rows<'a>(
    locale: Locale,
    page: usize,
    diagnostics: impl IntoIterator<Item = &'a Diagnostic>,
    cx: &App,
) -> AnyElement {
    let mut rows = div()
        .h_48()
        .flex_shrink_0()
        .overflow_y_scrollbar()
        // The page identity belongs to the scrollbar wrapper, which owns the
        // scroll offset; changing only the content ID retains the old offset.
        .id(("diagnostic-page", page))
        .flex()
        .flex_col()
        .gap_2();
    for diagnostic in diagnostics {
        let (severity, color) = match diagnostic.severity {
            Severity::Info => (Key::DiagnosticInfo, cx.theme().info),
            Severity::Warning => (Key::DiagnosticWarning, cx.theme().warning),
            Severity::Error => (Key::DiagnosticError, cx.theme().danger),
        };
        let mut row = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_0()
            .gap_1()
            .text_sm()
            .whitespace_normal()
            .child(div().text_color(color).child(text(locale, severity)))
            .child(diagnostic.message.display(locale))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(diagnostic.code.as_ref())),
            );
        if let Some(offset) = diagnostic.offset {
            row = row.child(
                Message::new(Key::DiagnosticOffset)
                    .arg("offset", offset)
                    .display(locale),
            );
        }
        if let Some(object) = diagnostic.object {
            row = row.child(
                Message::new(Key::DiagnosticObject)
                    .arg("id", object.0)
                    .display(locale),
            );
        }
        if let Some(path) = &diagnostic.path {
            row = row.child(path.to_string_lossy().into_owned());
        }
        if let Some(details) = &diagnostic.technical_details {
            row = row
                .child(text(locale, Key::TechnicalDetails))
                .child(SharedString::from(details.as_ref()));
        }
        rows = rows.child(row);
    }
    // The inner scrollbar handles the wheel first; the boundary keeps it from
    // also moving the surrounding inspector or import-status scroll surface.
    div()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(rows)
        .into_any_element()
}
