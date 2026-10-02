//! Successful import history, persisted independently of document lifetimes.

use crate::prefs::{RecentEntry, RecentStore};
use gpui_kit::{App, AppContext, BorrowAppContext, Global, Task};
use pomelo_core::{i18n::MessageKey, model::Diagnostic};

pub struct RecentState {
    pub entries: Vec<RecentEntry>,
    pub warning: Option<Diagnostic>,
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
    cx.set_global(RecentState {
        entries,
        warning,
        store,
        save_task: None,
        revision: 0,
    });
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

pub fn remember(entry: RecentEntry, replaced_path: Option<&std::path::Path>, cx: &mut App) {
    cx.update_global::<RecentState, _>(|state, _| {
        crate::prefs::remember_relocated(&mut state.entries, entry, replaced_path);
    });
    persist(cx);
}

pub fn remove(path: &std::path::Path, cx: &mut App) {
    cx.update_global::<RecentState, _>(|state, _| state.entries.retain(|entry| entry.path != path));
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
