//! Theme restoration and serialized preference writes.

use crate::prefs::{ThemePreference, ThemeStore};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, AppContext, BorrowAppContext, Global, Task, Window};
use pomelo_core::{i18n::MessageKey, model::Diagnostic};

pub struct ThemeState {
    pub preference: ThemePreference,
    pub warning: Option<Diagnostic>,
    store: Option<ThemeStore>,
    save_task: Option<Task<()>>,
    revision: u64,
}

impl Global for ThemeState {}

pub fn initialize(cx: &mut App) {
    let store = ThemeStore::platform_default();
    let (preference, warning) = match &store {
        Some(store) => match store.load() {
            Ok(preference) => (preference, None),
            Err(error) => (ThemePreference::default(), Some(error)),
        },
        None => (ThemePreference::default(), Some(missing_directory())),
    };
    Theme::change(mode(preference), None, cx);
    cx.set_global(ThemeState {
        preference,
        warning,
        store,
        save_task: None,
        revision: 0,
    });
    cx.on_app_quit(|cx| {
        let pending = cx.update_global::<ThemeState, _>(|state, _| state.save_task.take());
        async move {
            if let Some(pending) = pending {
                pending.await;
            }
        }
    })
    .detach();
}

pub fn toggle(window: &mut Window, cx: &mut App) {
    let preference = match cx.global::<ThemeState>().preference {
        ThemePreference::Dark => ThemePreference::Light,
        ThemePreference::Light => ThemePreference::Dark,
    };
    select(preference, window, cx);
}

/// Apply an explicit choice; serialize saves with the existing toolbar toggle.
pub fn select(preference: ThemePreference, window: &mut Window, cx: &mut App) {
    let (previous, store, revision) = cx.update_global::<ThemeState, _>(|state, _| {
        state.preference = preference;
        state.warning = None;
        state.revision = state.revision.wrapping_add(1);
        (state.save_task.take(), state.store.clone(), state.revision)
    });
    Theme::change(mode(preference), Some(window), cx);
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_spawn(async move {
                match store {
                    Some(store) => store.save(preference),
                    None => Err(missing_directory()),
                }
            })
            .await;
        cx.update_global::<ThemeState, _>(|state, cx| {
            if state.revision == revision {
                state.warning = result.err();
                cx.refresh_windows();
            }
        });
    });
    cx.update_global::<ThemeState, _>(|state, _| state.save_task = Some(task));
}

fn mode(preference: ThemePreference) -> ThemeMode {
    match preference {
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::Light => ThemeMode::Light,
    }
}

fn missing_directory() -> Diagnostic {
    Diagnostic::error(
        "CONFIG_DIRECTORY_MISSING",
        MessageKey::ConfigDirectoryMissing,
    )
}
