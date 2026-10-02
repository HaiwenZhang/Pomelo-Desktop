//! Cancellation and request identity, independent of any executor or UI.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId {
    pub document: DocumentId,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStage {
    Reading,
    Indexing,
    Decoding,
    BuildingGeometry,
    PreparingGpu,
    PreparingTracks,
    PreparingCopper,
    PreparingPads,
    PreparingDrills,
    PreparingTexts,
    BuildingSearch,
    BuildingPicking,
}

impl ImportStage {
    pub const fn message_key(self) -> crate::i18n::MessageKey {
        use crate::i18n::MessageKey;
        match self {
            Self::Reading => MessageKey::Reading,
            Self::Indexing => MessageKey::Indexing,
            Self::Decoding => MessageKey::Decoding,
            Self::BuildingGeometry => MessageKey::BuildingGeometry,
            Self::PreparingGpu => MessageKey::PreparingGpu,
            Self::PreparingTracks => MessageKey::PreparingTracks,
            Self::PreparingCopper => MessageKey::PreparingCopper,
            Self::PreparingPads => MessageKey::PreparingPads,
            Self::PreparingDrills => MessageKey::PreparingDrills,
            Self::PreparingTexts => MessageKey::PreparingTexts,
            Self::BuildingSearch => MessageKey::BuildingSearch,
            Self::BuildingPicking => MessageKey::BuildingPicking,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportProgress {
    pub stage: ImportStage,
    pub completed: u64,
    pub total: Option<u64>,
}

/// A bounded worker-to-UI mailbox. Publishing replaces any unread progress;
/// the UI controls its polling rate rather than scheduling one notification per record.
/// Create one mailbox per request so old workers cannot overwrite a new import.
#[derive(Debug, Clone, Default)]
pub struct LatestImportProgress(Arc<Mutex<Option<ImportProgress>>>);

impl LatestImportProgress {
    pub fn publish(&self, progress: ImportProgress) {
        // The critical section only replaces a plain value. Recovering a poisoned
        // lock keeps optional progress reporting from aborting the actual import.
        let mut latest = self.0.lock().unwrap_or_else(|error| error.into_inner());
        *latest = Some(progress);
    }

    /// Consume the latest update, returning None until the worker publishes again.
    pub fn take(&self) -> Option<ImportProgress> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_reaches_worker_clone() {
        let token = CancellationToken::default();
        let worker = token.clone();
        token.cancel();
        assert!(worker.is_cancelled());
    }

    #[test]
    fn progress_burst_keeps_only_latest_update() {
        let mailbox = LatestImportProgress::default();
        let worker = mailbox.clone();
        std::thread::spawn(move || {
            for completed in 0..100_000 {
                worker.publish(ImportProgress {
                    stage: ImportStage::Indexing,
                    completed,
                    total: Some(100_000),
                });
            }
        })
        .join()
        .unwrap();
        assert_eq!(mailbox.take().unwrap().completed, 99_999);
        assert!(mailbox.take().is_none());
    }

    #[test]
    fn separate_requests_do_not_share_progress() {
        let previous = LatestImportProgress::default();
        let current = LatestImportProgress::default();
        previous.publish(ImportProgress {
            stage: ImportStage::BuildingGeometry,
            completed: 42,
            total: None,
        });
        assert!(current.take().is_none());
    }
}
