//! Successful import history, persisted independently of document lifetimes.

use crate::services::preferences::{RecentEntry, RecentStore};
use chrono::{DateTime, Datelike, TimeZone, Timelike};
use gpui_kit::{App, AppContext, BorrowAppContext, Global, RenderImage, Task};
use pomelo_core::{
    i18n::{Message, MessageKey},
    model::Diagnostic,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub struct RecentState {
    pub entries: Vec<RecentEntry>,
    pub warning: Option<Diagnostic>,
    pub previews: BTreeMap<PathBuf, Arc<RenderImage>>,
    store: Option<RecentStore>,
    save_task: Option<Task<()>>,
    revision: u64,
}
impl Global for RecentState {}

pub fn initialize(cx: &mut App) {
    let store = RecentStore::platform_default();
    let (entries, warning) = match &store {
        Some(store) => match store.load() {
            Ok(entries) => (entries, None),
            Err(error) => (Vec::new(), Some(error)),
        },
        None => (Vec::new(), Some(missing_directory())),
    };
    let pending_previews: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry
                .presentation
                .as_ref()
                .and_then(|value| value.thumbnail_png.as_ref())
                .is_some()
        })
        .cloned()
        .collect();
    cx.set_global(RecentState {
        entries,
        warning,
        previews: BTreeMap::new(),
        store,
        save_task: None,
        revision: 0,
    });
    cx.spawn(async move |cx| {
        let decoded = cx
            .background_spawn(async move {
                pending_previews
                    .into_iter()
                    .filter_map(|entry| {
                        let png = entry
                            .presentation
                            .as_ref()
                            .and_then(|value| value.thumbnail_png.as_deref())?;
                        let result = super::preview::decode(png)
                            .map_err(|diagnostic| diagnostic.with_path(&entry.path));
                        Some((entry, result))
                    })
                    .collect::<Vec<_>>()
            })
            .await;
        cx.update_global::<RecentState, _>(|state, cx| {
            for (entry, result) in decoded {
                if !cache_matches(&state.entries, &entry) {
                    continue;
                }
                match result {
                    Ok(image) => {
                        state.previews.entry(entry.path).or_insert(image);
                    }
                    Err(error) => {
                        if state.warning.is_none() {
                            state.warning = Some(error);
                        }
                    }
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    cx.on_app_quit(|cx| {
        let pending = cx.update_global::<RecentState, _>(|state, _| state.save_task.take());
        async move {
            if let Some(pending) = pending {
                pending.await;
            }
        }
    })
    .detach();
}

pub fn remember(
    entry: RecentEntry,
    replaced_path: Option<&std::path::Path>,
    preview: Option<Arc<RenderImage>>,
    cx: &mut App,
) {
    cx.update_global::<RecentState, _>(|state, _| {
        let path = entry.path.clone();
        crate::services::preferences::remember_relocated(&mut state.entries, entry, replaced_path);
        state.previews.remove(&path);
        if let Some(preview) = preview {
            state.previews.insert(path, preview);
        }
        state
            .previews
            .retain(|path, _| state.entries.iter().any(|entry| &entry.path == path));
    });
    persist(cx);
}

pub fn remove(path: &std::path::Path, cx: &mut App) {
    cx.update_global::<RecentState, _>(|state, _| {
        state.entries.retain(|entry| entry.path != path);
        state.previews.remove(path);
    });
    persist(cx);
}

fn persist(cx: &mut App) {
    let (entries, previous, store, revision) = cx.update_global::<RecentState, _>(|state, _| {
        state.warning = None;
        state.revision = state.revision.wrapping_add(1);
        (
            state.entries.clone(),
            state.save_task.take(),
            state.store.clone(),
            state.revision,
        )
    });
    cx.refresh_windows();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_spawn(async move {
                match store {
                    Some(store) => store.save(entries),
                    None => Err(missing_directory()),
                }
            })
            .await;
        cx.update_global::<RecentState, _>(|state, cx| {
            if state.revision == revision {
                state.warning = result.err();
                cx.refresh_windows();
            }
        });
    });
    cx.update_global::<RecentState, _>(|state, _| state.save_task = Some(task));
}

fn missing_directory() -> Diagnostic {
    Diagnostic::error(
        "CONFIG_DIRECTORY_MISSING",
        MessageKey::ConfigDirectoryMissing,
    )
}

fn cache_matches(entries: &[RecentEntry], snapshot: &RecentEntry) -> bool {
    entries.iter().any(|entry| {
        entry.path == snapshot.path
            && entry.opened_unix_seconds == snapshot.opened_unix_seconds
            && entry.presentation == snapshot.presentation
    })
}

/// Compare local calendar dates rather than assuming a day lasts 86,400 seconds.
pub fn opened_message<Tz: TimeZone>(seconds: u64, now: &DateTime<Tz>) -> Message {
    let opened = i64::try_from(seconds)
        .ok()
        .and_then(|seconds| now.timezone().timestamp_opt(seconds, 0).single());
    let Some(opened) = opened else {
        return Message::new(MessageKey::RecentTimeUnknown);
    };
    let days = now
        .date_naive()
        .signed_duration_since(opened.date_naive())
        .num_days();
    if days == 0 {
        Message::new(MessageKey::RecentToday).arg(
            "time",
            format!("{:02}:{:02}", opened.hour(), opened.minute()),
        )
    } else if days == 1 {
        Message::new(MessageKey::RecentYesterday)
    } else if now.year() == opened.year() {
        Message::new(MessageKey::RecentMonthDay)
            .arg("month", opened.month())
            .arg("day", opened.day())
    } else {
        Message::new(MessageKey::RecentYearMonthDay)
            .arg("year", opened.year().to_string())
            .arg("month", opened.month())
            .arg("day", opened.day())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::preferences::RecentPresentation;

    #[test]
    fn date_labels_use_local_midnight_and_retain_translation_parameters() {
        let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let now = zone.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let cases = [
            (2026, 1, 1, 0, 5, MessageKey::RecentToday),
            (2025, 12, 31, 23, 59, MessageKey::RecentYesterday),
            (2025, 12, 30, 12, 0, MessageKey::RecentYearMonthDay),
            (2026, 1, 3, 12, 0, MessageKey::RecentMonthDay),
        ];
        for (y, m, d, h, minute, key) in cases {
            let timestamp = zone
                .with_ymd_and_hms(y, m, d, h, minute, 0)
                .unwrap()
                .timestamp() as u64;
            let message = opened_message(timestamp, &now);
            assert_eq!(message.key, key);
            for locale in pomelo_core::i18n::Locale::ALL {
                assert!(message.render(locale).is_ok());
            }
        }
        assert_eq!(
            opened_message(u64::MAX, &now).key,
            MessageKey::RecentTimeUnknown
        );
    }

    #[test]
    fn cache_completion_cannot_restore_removed_or_reimported_history() {
        let snapshot = RecentEntry {
            path: std::path::absolute("source.brd").unwrap(),
            format: "allegro".into(),
            opened_unix_seconds: 123,
            encoding: pomelo_import::TextEncoding::Utf8,
            presentation: Some(RecentPresentation {
                layer_count: 4,
                thumbnail_png: Some("old".into()),
            }),
        };
        assert!(cache_matches(std::slice::from_ref(&snapshot), &snapshot));
        assert!(!cache_matches(&[], &snapshot));
        let mut updated = snapshot.clone();
        updated.presentation.as_mut().unwrap().thumbnail_png = Some("new".into());
        assert!(!cache_matches(std::slice::from_ref(&updated), &snapshot));
    }
}
