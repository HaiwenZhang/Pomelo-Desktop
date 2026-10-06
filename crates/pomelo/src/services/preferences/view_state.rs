//! Per-document view snapshots and source identity matching.
use super::RECENT_LIMIT;
use super::json_file::JsonFileStore;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEntry {
    pub path: PathBuf,
    pub state: pomelo_core::view_state::ViewState,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ViewFile {
    pub(super) schema_version: u32,
    pub(super) entries: Vec<ViewEntry>,
}

#[derive(Clone)]
pub struct ViewStore(pub(super) JsonFileStore);

impl ViewStore {
    /// Caller serializes jobs; each job merges against the preceding disk result.
    pub fn save_updates(&self, entries: Vec<ViewEntry>) -> Result<(), Diagnostic> {
        self.validate(&entries)?;
        let mut saved = self.load().unwrap_or_default();
        for entry in entries {
            saved.retain(|previous| previous.path != entry.path);
            saved.insert(0, entry);
        }
        saved.truncate(RECENT_LIMIT);
        self.save(saved)
    }
    pub fn load_matching(
        &self,
        path: &std::path::Path,
        source: &pomelo_core::view_state::SourceIdentity,
    ) -> Result<Option<pomelo_core::view_state::ViewState>, Diagnostic> {
        let entries = self.load()?;
        let Some(entry) = entries.into_iter().find(|entry| entry.path == path) else {
            return Ok(None);
        };
        if entry.state.matching(source)?.is_some() {
            Ok(Some(entry.state))
        } else {
            Ok(None)
        }
    }
    pub fn platform_default() -> Option<Self> {
        JsonFileStore::in_configuration_directory("views.json").map(Self)
    }

    pub fn load(&self) -> Result<Vec<ViewEntry>, Diagnostic> {
        let Some(file) = self.0.load_file_limited::<ViewFile>(4 * 1024 * 1024)? else {
            return Ok(Vec::new());
        };
        if file.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        self.validate(&file.entries)?;
        Ok(file.entries)
    }

    pub fn save(&self, entries: Vec<ViewEntry>) -> Result<(), Diagnostic> {
        self.validate(&entries)?;
        let file = ViewFile {
            schema_version: 1,
            entries,
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| self.0.failure(MessageKey::ConfigSaveFailed, error))?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(self
                .0
                .failure(MessageKey::ConfigSaveFailed, "CONFIG_FILE_TOO_LARGE"));
        }
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&file)
    }

    fn validate(&self, entries: &[ViewEntry]) -> Result<(), Diagnostic> {
        if entries.len() > RECENT_LIMIT
            || entries.iter().enumerate().any(|(index, entry)| {
                !entry.path.is_absolute()
                    || entry.path.as_os_str().len() > 32768
                    || entries[..index]
                        .iter()
                        .any(|other| other.path == entry.path)
            })
        {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "VIEW_ENTRIES_INVALID"));
        }
        for entry in entries {
            entry
                .state
                .validate()
                .map_err(|error| error.with_path(&self.0.path))?;
        }
        Ok(())
    }
}
