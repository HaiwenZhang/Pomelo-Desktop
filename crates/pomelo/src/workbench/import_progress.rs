//! Centered import indicator with a bounded, live stage log.
use crate::document::{DocumentSession, DocumentStatus};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{ActiveTheme, progress::ProgressCircle},
    *,
};
use pomelo_core::{
    i18n::{Locale, Message, MessageKey as Key, text},
    task::{ImportProgress, ImportStage},
};

fn percent(progress: &ImportProgress) -> Option<f32> {
    progress
        .total
        .filter(|total| *total > 0)
        .map(|total| (progress.completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32)
}

fn detail(progress: &ImportProgress, locale: Locale) -> String {
    let bytes = matches!(progress.stage, ImportStage::Reading | ImportStage::Indexing);
    let key = match (bytes, progress.total) {
        (true, Some(_)) => Key::ImportBytesKnown,
        (true, None) => Key::ImportBytesUnknown,
        (false, Some(_)) => Key::ImportItemsKnown,
        (false, None) => Key::ImportItemsUnknown,
    };
    let mut message = Message::new(key).arg("completed", progress.completed);
    if let Some(total) = progress.total {
        message = message.arg("total", total);
    }
    message.display(locale)
}

pub(super) fn render(document: &DocumentSession, locale: Locale, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let queued = matches!(document.status, DocumentStatus::Queued);
    let stage = text(
        locale,
        document.progress.as_ref().map_or(
            if queued {
                Key::ImportQueued
            } else {
                Key::Reading
            },
            |progress| progress.stage.message_key(),
        ),
    );
    let value = document.progress.as_ref().and_then(percent);
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .p_6()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(560.0))
                .p_6()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(
                            document
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_6()
                        .child(
                            ProgressCircle::new((
                                "pcb-import-progress",
                                document.request.document.0,
                            ))
                            .size(px(112.0))
                            .flex_shrink_0()
                            .color(theme.primary)
                            .accessibility_label(stage.clone())
                            .loading(value.is_none())
                            .value(value.unwrap_or(0.0))
                            .when_some(value, |circle, value| {
                                circle.child(
                                    div().text_lg().font_weight(FontWeight::SEMIBOLD).child(
                                        Message::new(Key::UiPercent)
                                            .arg("value", value.round() as u32)
                                            .display(locale),
                                    ),
                                )
                            }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(stage),
                                )
                                .children(document.progress_log.iter().enumerate().map(
                                    |(index, progress)| {
                                        let current = index + 1 == document.progress_log.len();
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .text_sm()
                                            .text_color(if current {
                                                theme.foreground
                                            } else {
                                                theme.muted_foreground
                                            })
                                            .child(text(locale, progress.stage.message_key()))
                                            .when(
                                                progress.completed != 0 || progress.total.is_some(),
                                                |row| {
                                                    row.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(theme.muted_foreground)
                                                            .child(detail(progress, locale)),
                                                    )
                                                },
                                            )
                                    },
                                )),
                        ),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::percent;
    use pomelo_core::task::{ImportProgress, ImportStage};

    #[test]
    fn stage_percentage_handles_unknown_empty_and_overrun_totals() {
        let progress = |completed, total| ImportProgress {
            stage: ImportStage::Reading,
            completed,
            total,
        };
        assert_eq!(percent(&progress(1, None)), None);
        assert_eq!(percent(&progress(0, Some(0))), None);
        assert_eq!(percent(&progress(25, Some(100))), Some(25.0));
        assert_eq!(percent(&progress(101, Some(100))), Some(100.0));
    }
}
