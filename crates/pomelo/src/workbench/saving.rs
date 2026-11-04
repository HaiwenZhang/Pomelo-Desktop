//! View persistence and exit flushing outside GPUI's shutdown budget.

use futures::future::{FutureExt as _, Shared};
use gpui_kit::*;
use pomelo_core::{
    i18n::{Locale, MessageKey as Key},
    model::Diagnostic,
};

use super::Workbench;
use crate::{document::DocumentStatus, i18n};

#[derive(serde::Serialize)]
pub(super) struct ViewSaveSnapshot {
    pub(super) entries: Vec<crate::services::preferences::ViewEntry>,
    pub(super) panels: crate::services::preferences::PanelPreferences,
}

impl ViewSaveSnapshot {
    fn encoded(&self) -> Result<Vec<u8>, Diagnostic> {
        serde_json::to_vec(self).map_err(|error| {
            Diagnostic::error("CONFIG_SAVE_FAILED", Key::ConfigSaveFailed)
                .with_details(format!("stage=snapshot_encode; {error}"))
        })
    }
}

impl Workbench {
    pub(super) fn save_views(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let pending = self.queue_view_save(index, cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = pending.await {
                let _ = this.update(cx, |this, cx| {
                    report_save_error(&error, i18n::current(cx));
                    this.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn queue_view_save(
        &mut self,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Shared<Task<Result<(), Diagnostic>>> {
        let snapshot = self.view_save_snapshot(index, cx);
        self.enqueue_view_save(snapshot, cx)
    }

    fn view_save_snapshot(&self, index: Option<usize>, cx: &Context<Self>) -> ViewSaveSnapshot {
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let entries = {
            let mut entries: Vec<_> = self
                .documents
                .iter()
                .enumerate()
                .filter(|(position, _)| index.is_none_or(|index| index == *position))
                .filter_map(|(position, document)| {
                    let DocumentStatus::Imported(prepared) = &document.status else {
                        return None;
                    };
                    let viewport = document.viewport.as_ref()?;
                    Some((
                        position,
                        crate::services::preferences::ViewEntry {
                            path: document.path.clone(),
                            state: viewport.read(cx).snapshot(&prepared.board.identity)?,
                        },
                    ))
                })
                .collect();
            // Updates are most-recent first; keep the active document if the store truncates.
            entries.sort_by_key(|(position, _)| Some(*position) != self.active);
            entries.into_iter().map(|(_, entry)| entry).collect()
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let entries: Vec<crate::services::preferences::ViewEntry> = {
            let _ = index;
            Vec::new()
        };
        let panels = self.panel_layout.read(cx).snapshot(cx);
        ViewSaveSnapshot { entries, panels }
    }

    fn enqueue_view_save(
        &mut self,
        snapshot: ViewSaveSnapshot,
        cx: &mut Context<Self>,
    ) -> Shared<Task<Result<(), Diagnostic>>> {
        let ViewSaveSnapshot { entries, panels } = snapshot;
        self.view_save_generation = self.view_save_generation.wrapping_add(1);
        let panel_store = crate::services::preferences::PanelStore::platform_default();
        let previous = self.view_save_task.take();
        let store = crate::services::preferences::ViewStore::platform_default();
        // GPUI blocks the main thread during shutdown; disk writes must not depend on it.
        let pending = cx
            .background_executor()
            .spawn(async move {
                if let Some(previous) = previous {
                    let _ = previous.await;
                }
                let panel_store = panel_store.ok_or_else(|| {
                    Diagnostic::error("CONFIG_DIRECTORY_MISSING", Key::ConfigDirectoryMissing)
                })?;
                panel_store.save(panels)?;
                if !entries.is_empty() {
                    let store = store.ok_or_else(|| {
                        Diagnostic::error("CONFIG_DIRECTORY_MISSING", Key::ConfigDirectoryMissing)
                    })?;
                    store.save_updates(entries)?;
                }
                Ok(())
            })
            .shared();
        self.view_save_task = Some(pending.clone());
        pending
    }

    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
        if self.quit_flushed || self.quit_task.is_some() {
            return;
        }
        self.quit_snapshot = None;
        self.advance_quit(cx);
    }

    fn advance_quit(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.view_save_snapshot(None, cx);
        let encoded = match snapshot.encoded() {
            Ok(encoded) => encoded,
            Err(error) => {
                self.quit_failed(error, cx);
                return;
            }
        };
        // Recheck after every write: edits and other queued saves can happen while awaiting IO.
        if self
            .quit_snapshot
            .as_ref()
            .is_some_and(|(saved, generation)| {
                *saved == encoded && *generation == self.view_save_generation
            })
        {
            self.quit_flushed = true;
            cx.quit();
            return;
        }
        let pending = self.enqueue_view_save(snapshot, cx);
        self.quit_snapshot = Some((encoded, self.view_save_generation));
        self.quit_task = Some(cx.spawn(async move |this, cx| {
            let result = pending.await;
            let _ = this.update(cx, |this, cx| {
                this.quit_task.take();
                match result {
                    Ok(()) => this.advance_quit(cx),
                    Err(error) => this.quit_failed(error, cx),
                }
            });
        }));
    }

    fn quit_failed(&mut self, error: Diagnostic, cx: &mut Context<Self>) {
        self.quit_task.take();
        self.quit_snapshot = None;
        report_save_error(&error, i18n::current(cx));
        self.error = Some(error);
        cx.notify();
    }
}

pub(super) fn report_save_error(error: &Diagnostic, locale: Locale) {
    eprintln!(
        "{}; code={}; path={}; {}",
        error.message.display(locale),
        error.code,
        error
            .path
            .as_deref()
            .map_or_else(String::new, |path| path.display().to_string()),
        error.technical_details.as_deref().unwrap_or_default(),
    );
}
