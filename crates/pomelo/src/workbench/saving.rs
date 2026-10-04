//! Ordinary exits flush outside GPUI's 200ms shutdown budget.

use super::*;

#[derive(serde::Serialize)]
pub(super) struct ViewSaveSnapshot {
    pub(super) entries: Vec<crate::prefs::ViewEntry>,
    pub(super) panels: crate::prefs::PanelPreferences,
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
        self.enqueue_view_save(snapshot, cx);
        self.quit_snapshot = Some((encoded, self.view_save_generation));
        let Some(pending) = self.view_save_task.clone() else {
            return;
        };
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
