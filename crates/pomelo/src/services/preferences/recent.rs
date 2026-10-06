//! Recent board history and relocation updates.
use super::json_file::JsonFileStore;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub const RECENT_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub format: String,
    pub opened_unix_seconds: u64,
    pub encoding: pomelo_import::TextEncoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<RecentPresentation>,
}

/// Last successful import information, independent of an open document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentPresentation {
    pub layer_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_png: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentFile {
    schema_version: u32,
    entries: Vec<RecentEntry>,
}

#[derive(Clone)]
pub struct RecentStore(pub(super) JsonFileStore);

impl RecentStore {
    pub fn platform_default() -> Option<Self> {
        JsonFileStore::in_configuration_directory("recent.json").map(Self)
    }

    pub fn load(&self) -> Result<Vec<RecentEntry>, Diagnostic> {
        let Some(file) = self.0.load_file_limited::<RecentFile>(4 * 1024 * 1024)? else {
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

    pub fn save(&self, entries: Vec<RecentEntry>) -> Result<(), Diagnostic> {
        self.validate(&entries)?;
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&RecentFile {
            schema_version: 1,
            entries,
        })
    }

    fn validate(&self, entries: &[RecentEntry]) -> Result<(), Diagnostic> {
        if entries.len() > RECENT_LIMIT
            || entries.iter().enumerate().any(|(index, entry)| {
                !entry.path.is_absolute()
                    || pomelo_import::formats::BoardFormat::from_tag(&entry.format).is_none()
                    || entry
                        .presentation
                        .as_ref()
                        .and_then(|value| value.thumbnail_png.as_ref())
                        .is_some_and(|png| png.len() > crate::services::preview::MAX_ENCODED_BYTES)
                    || entries[..index]
                        .iter()
                        .any(|other| other.path == entry.path)
            })
        {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "RECENT_ENTRIES_INVALID"));
        }
        Ok(())
    }
}

pub fn remember_recent(entries: &mut Vec<RecentEntry>, entry: RecentEntry) {
    entries.retain(|other| other.path != entry.path);
    entries.insert(0, entry);
    entries.truncate(RECENT_LIMIT);
}

pub fn remember_relocated(
    entries: &mut Vec<RecentEntry>,
    entry: RecentEntry,
    replaced_path: Option<&std::path::Path>,
) {
    if let Some(path) = replaced_path {
        entries.retain(|other| other.path != path);
    }
    remember_recent(entries, entry);
}
