//! Small, versioned preferences, independent of window state.

use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use pomelo_core::{
    i18n::{LanguagePreference, MessageKey},
    model::Diagnostic,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LanguageStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageFile {
    schema_version: u32,
    language: LanguagePreference,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    Dark,
    Light,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    schema_version: u32,
    theme: ThemePreference,
}

#[derive(Clone)]
pub struct ThemeStore(LanguageStore);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPreferences {
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    pub left_width: f32,
    pub right_width: f32,
}
impl Default for PanelPreferences {
    fn default() -> Self {
        Self {
            left_collapsed: false,
            right_collapsed: false,
            left_width: 256.0,
            right_width: 288.0,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelFile {
    schema_version: u32,
    panels: PanelPreferences,
}
#[derive(Clone)]
pub struct PanelStore(LanguageStore);
impl PanelStore {
    pub fn platform_default() -> Option<Self> {
        LanguageStore::platform_default().map(|mut store| {
            store.path.set_file_name("panels.json");
            Self(store)
        })
    }
    pub fn load(&self) -> Result<PanelPreferences, Diagnostic> {
        let Some(parsed) = self.0.load_file::<PanelFile>()? else {
            return Ok(PanelPreferences::default());
        };
        self.validate(parsed.panels)?;
        if parsed.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.panels)
    }
    fn validate(&self, panels: PanelPreferences) -> Result<(), Diagnostic> {
        if !panels.left_width.is_finite()
            || !panels.right_width.is_finite()
            || !(200.0..=440.0).contains(&panels.left_width)
            || !(240.0..=440.0).contains(&panels.right_width)
        {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "PANEL_LAYOUT_INVALID"));
        }
        Ok(())
    }
    pub fn save(&self, panels: PanelPreferences) -> Result<(), Diagnostic> {
        self.validate(panels)?;
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&PanelFile {
            schema_version: 1,
            panels,
        })
    }
}

impl ThemeStore {
    pub fn platform_default() -> Option<Self> {
        LanguageStore::platform_default().map(|mut store| {
            store.path.set_file_name("theme.json");
            Self(store)
        })
    }

    pub fn load(&self) -> Result<ThemePreference, Diagnostic> {
        let Some(parsed) = self.0.load_file::<ThemeFile>()? else {
            return Ok(ThemePreference::default());
        };
        if parsed.schema_version != 1 {
            return Err(self
                .0
                .failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.theme)
    }

    pub fn save(&self, theme: ThemePreference) -> Result<(), Diagnostic> {
        self.0.preserve_invalid(self.load().is_err())?;
        self.0.save_file(&ThemeFile {
            schema_version: 1,
            theme,
        })
    }
}

impl LanguageStore {
    pub fn platform_default() -> Option<Self> {
        configuration_directory().map(|root| Self {
            path: root.join("language.json"),
        })
    }

    pub fn load(&self) -> Result<LanguagePreference, Diagnostic> {
        let Some(parsed) = self.load_file::<LanguageFile>()? else {
            return Ok(LanguagePreference::System);
        };
        if parsed.schema_version != 1 {
            return Err(self.failure(MessageKey::ConfigInvalid, "CONFIG_SCHEMA_UNSUPPORTED"));
        }
        Ok(parsed.language)
    }

    fn load_file<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, Diagnostic> {
        self.load_file_limited(4096)
    }

    fn load_file_limited<T: serde::de::DeserializeOwned>(
        &self,
        limit: u64,
    ) -> Result<Option<T>, Diagnostic> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(self.failure(MessageKey::ConfigInvalid, &error)),
        };
        // Preferences have no reason to consume an unbounded amount of memory.
        let metadata = file
            .metadata()
            .map_err(|error| self.failure(MessageKey::ConfigInvalid, &error))?;
        if metadata.len() > limit {
            return Err(self.failure(MessageKey::ConfigInvalid, "CONFIG_FILE_TOO_LARGE"));
        }
        use std::io::Read;
        let parsed = serde_json::from_reader(file.take(limit + 1))
            .map_err(|error| self.failure(MessageKey::ConfigInvalid, &error))?;
        Ok(Some(parsed))
    }

    pub fn save(&self, language: LanguagePreference) -> Result<(), Diagnostic> {
        self.preserve_invalid(self.load().is_err())?;
        self.save_file(&LanguageFile {
            schema_version: 1,
            language,
        })
    }

    fn save_file(&self, value: &impl Serialize) -> Result<(), Diagnostic> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| self.failure(MessageKey::ConfigSaveFailed, "CONFIG_PARENT_MISSING"))?;
        fs::create_dir_all(parent).map_err(|error| self.save_failure("directory", error))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| self.save_failure("temporary_create", error))?;
        serde_json::to_writer_pretty(&mut temporary, value)
            .map_err(|error| self.save_failure("serialize", error))?;
        temporary
            .flush()
            .map_err(|error| self.save_failure("flush", error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| self.save_failure("sync_all", error))?;
        // tempfile uses MOVEFILE_REPLACE_EXISTING on Windows, unlike std::rename.
        temporary
            .persist(&self.path)
            .map_err(|error| self.save_failure("persist", &error.error))?;
        Ok(())
    }

    fn save_failure(&self, stage: &'static str, details: impl std::fmt::Display) -> Diagnostic {
        self.failure(
            MessageKey::ConfigSaveFailed,
            format!("stage={stage}; {details}"),
        )
    }

    fn preserve_invalid(&self, invalid: bool) -> Result<(), Diagnostic> {
        if !invalid {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| self.failure(MessageKey::ConfigSaveFailed, "CONFIG_PARENT_MISSING"))?;
        let prefix = format!(
            "{}.recovery-",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        );
        let mut backup = tempfile::Builder::new()
            .prefix(&prefix)
            .tempfile_in(parent)
            .map_err(|error| self.failure(MessageKey::ConfigSaveFailed, &error))?;
        let mut source = fs::File::open(&self.path)
            .map_err(|error| self.failure(MessageKey::ConfigSaveFailed, &error))?;
        // Stream the original, including oversized/unrecognized files; do not
        // replace it unless an independent, durable recovery copy exists.
        io::copy(&mut source, &mut backup)
            .and_then(|_| backup.flush())
            .and_then(|()| backup.as_file().sync_all())
            .map_err(|error| self.failure(MessageKey::ConfigSaveFailed, &error))?;
        backup
            .keep()
            .map_err(|error| self.failure(MessageKey::ConfigSaveFailed, &error.error))?;
        Ok(())
    }

    fn failure(&self, key: MessageKey, details: impl std::fmt::Display) -> Diagnostic {
        Diagnostic::error(
            match key {
                MessageKey::ConfigInvalid => "CONFIG_INVALID",
                _ => "CONFIG_SAVE_FAILED",
            },
            key,
        )
        .with_path(&self.path)
        .with_details(details.to_string())
    }
}

/// An explicit profile must never fall back to another profile's writable files.
pub(crate) fn configuration_directory() -> Option<PathBuf> {
    configuration_root(
        std::env::var_os("POMELO_CONFIG_DIR").map(PathBuf::from),
        dirs::config_dir(),
    )
}

fn configuration_root(
    override_directory: Option<PathBuf>,
    platform_directory: Option<PathBuf>,
) -> Option<PathBuf> {
    override_directory
        .or_else(|| platform_directory.map(|root| root.join("Pomelo")))
        .filter(|root| root.is_absolute())
}

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
pub struct RecentStore(LanguageStore);

impl RecentStore {
    pub fn platform_default() -> Option<Self> {
        LanguageStore::platform_default().map(|mut store| {
            store.path.set_file_name("recent.json");
            Self(store)
        })
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEntry {
    pub path: PathBuf,
    pub state: pomelo_core::view_state::ViewState,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewFile {
    schema_version: u32,
    entries: Vec<ViewEntry>,
}

#[derive(Clone)]
pub struct ViewStore(LanguageStore);

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
        LanguageStore::platform_default().map(|mut store| {
            store.path.set_file_name("views.json");
            Self(store)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_save_failure_identifies_directory_stage_in_all_languages() {
        let directory = tempfile::tempdir().unwrap();
        let blocked_parent = directory.path().join("parent-is-a-file");
        fs::write(&blocked_parent, b"preserved").unwrap();
        let store = LanguageStore {
            path: blocked_parent.join("language.json"),
        };
        let error = store
            .save_file(&LanguageFile {
                schema_version: 1,
                language: LanguagePreference::System,
            })
            .unwrap_err();
        assert!(
            error.code.as_ref() == "CONFIG_SAVE_FAILED"
                && error.path.as_deref() == Some(store.path.as_path())
                && error
                    .technical_details
                    .as_deref()
                    .is_some_and(|details| details.starts_with("stage=directory; "))
                && Locale::ALL
                    .into_iter()
                    .all(|locale| error.message.render(locale).is_ok())
                && fs::read(blocked_parent).unwrap() == b"preserved",
            "{error:?}"
        );
    }

    #[test]
    fn failed_atomic_replace_preserves_destination_and_cleans_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("language.json");
        fs::create_dir(&destination).unwrap();
        let sentinel = destination.join("preserved");
        fs::write(&sentinel, b"original").unwrap();
        let store = LanguageStore { path: destination };
        let error = store
            .save_file(&LanguageFile {
                schema_version: 1,
                language: LanguagePreference::System,
            })
            .unwrap_err();
        assert!(
            error
                .technical_details
                .as_deref()
                .is_some_and(|details| details.starts_with("stage=persist; "))
                && fs::read(sentinel).unwrap() == b"original"
                && fs::read_dir(directory.path()).unwrap().count() == 1,
            "{error:?}"
        );
    }

    #[test]
    fn panel_layout_restores_each_side_and_remembered_expanded_width() {
        let directory = tempfile::tempdir().unwrap();
        let store = PanelStore(LanguageStore {
            path: directory.path().join("panels.json"),
        });
        let panels = PanelPreferences {
            left_collapsed: true,
            right_collapsed: false,
            left_width: 312.0,
            right_width: 360.0,
        };
        store.save(panels).unwrap();
        let reloaded = PanelStore(LanguageStore {
            path: store.0.path.clone(),
        });
        assert_eq!(reloaded.load().unwrap(), panels);
    }
    #[test]
    fn invalid_panel_width_preserves_the_previous_usable_layout() {
        let directory = tempfile::tempdir().unwrap();
        let store = PanelStore(LanguageStore {
            path: directory.path().join("panels.json"),
        });
        store.save(PanelPreferences::default()).unwrap();
        let original = fs::read(&store.0.path).unwrap();
        for width in [f32::NAN, f32::INFINITY, 0.0, 441.0] {
            let panels = PanelPreferences {
                left_width: width,
                ..PanelPreferences::default()
            };
            let error = store.save(panels).unwrap_err();
            for locale in pomelo_core::i18n::Locale::ALL {
                assert!(error.message.render(locale).is_ok());
            }
        }
        assert_eq!(fs::read(&store.0.path).unwrap(), original);
    }
    #[test]
    fn corrupt_panel_file_is_recoverable_without_discarding_the_source() {
        let directory = tempfile::tempdir().unwrap();
        let store = PanelStore(LanguageStore {
            path: directory.path().join("panels.json"),
        });
        let original = b"{\"schema_version\":99,\"panels\":{}}";
        fs::write(&store.0.path, original).unwrap();
        assert!(store.load().is_err());
        store.save(PanelPreferences::default()).unwrap();
        let recovery = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("panels.json.recovery-")
            })
            .unwrap();
        assert_eq!(fs::read(recovery.path()).unwrap(), original);
    }
    use pomelo_core::i18n::Locale;

    #[test]
    fn explicit_configuration_root_is_used_without_the_platform_subdirectory() {
        let directory = tempfile::tempdir().unwrap();
        let explicit = directory.path().join("isolated-profile");
        assert_eq!(
            configuration_root(Some(explicit.clone()), Some(directory.path().into())),
            Some(explicit)
        );
        assert_eq!(
            configuration_root(None, Some(directory.path().into())),
            Some(directory.path().join("Pomelo"))
        );
    }

    #[test]
    fn invalid_explicit_configuration_root_cannot_write_the_platform_profile() {
        let directory = tempfile::tempdir().unwrap();
        for invalid in [PathBuf::new(), PathBuf::from("relative-profile")] {
            assert_eq!(
                configuration_root(Some(invalid), Some(directory.path().into())),
                None
            );
        }
        assert_eq!(configuration_root(None, None), None);
    }

    fn view_entry(path: PathBuf) -> ViewEntry {
        ViewEntry {
            path,
            state: pomelo_core::view_state::ViewState {
                schema_version: 1,
                source: pomelo_core::view_state::SourceIdentity {
                    sha256: [7; 32],
                    format: "allegro".into(),
                    encoding: "utf-8".into(),
                },
                camera: Default::default(),
                display: Default::default(),
                selection_mode: pomelo_core::interaction::SelectionMode::Net,
                pick_filter: pomelo_core::picking::PickFilter::default(),
                selection: Some(pomelo_core::selection::SelectionTarget::Net(
                    pomelo_core::model::NetId(7),
                )),
                selection_anchor: None,
            },
        }
    }
    #[test]
    fn sequential_view_updates_keep_other_documents_and_replace_latest_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let store = ViewStore(LanguageStore {
            path: directory.path().join("views.json"),
        });
        let first = view_entry(directory.path().join("first.brd"));
        let mut second = view_entry(directory.path().join("second.brd"));
        second.state.display.length_unit = pomelo_core::units::LengthUnit::Mils;
        second.state.pick_filter = pomelo_core::picking::PickFilter::none();
        store.save_updates(vec![first.clone()]).unwrap();
        store.save_updates(vec![second.clone()]).unwrap();
        second.state.camera.flipped = true;
        second.state.display.layer_order = vec![
            pomelo_core::model::LayerId(3),
            pomelo_core::model::LayerId(1),
        ];
        second.state.display.show_copper = false;
        second.state.display.show_texts = false;
        second.state.display.show_drawings = false;
        second.state.display.copper_opacity = 0.75;
        second.state.display.color_mode = pomelo_core::display::ColorMode::Net;
        second.state.display.set_primitive(
            pomelo_core::model::LayerId(3),
            pomelo_core::display::LayerPrimitive::Vias,
            false,
        );
        store.save_updates(vec![second.clone()]).unwrap();
        let saved = store.load().unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].path, second.path);
        assert!(saved[0].state.camera.flipped);
        let restored = store
            .load_matching(&second.path, &second.state.source)
            .unwrap()
            .unwrap();
        assert_eq!(
            restored.display.layer_order,
            second.state.display.layer_order
        );
        assert!(!restored.display.show_copper);
        assert!(!restored.display.show_texts);
        assert!(!restored.display.show_drawings);
        assert_eq!(restored.display.copper_opacity, 0.75);
        assert_eq!(
            restored.display.color_mode,
            pomelo_core::display::ColorMode::Net
        );
        assert_eq!(
            saved[1].state.display.color_mode,
            pomelo_core::display::ColorMode::Layer
        );
        assert_eq!(
            restored.display.layer_primitives,
            second.state.display.layer_primitives
        );
        assert!(saved[1].state.display.layer_primitives.is_empty());
        assert_eq!(
            restored.display.length_unit,
            pomelo_core::units::LengthUnit::Mils
        );
        assert_eq!(
            saved[1].state.display.length_unit,
            pomelo_core::units::LengthUnit::Millimeters
        );
        assert_eq!(saved[0].state.pick_filter, second.state.pick_filter);
        assert_eq!(saved[1].state.pick_filter, first.state.pick_filter);
        assert_eq!(saved[1].path, first.path);
        let mut invalid = first;
        invalid.state.camera.pixels_per_mm = 0.0;
        let original = fs::read(&store.0.path).unwrap();
        assert!(store.save_updates(vec![invalid]).is_err());
        assert_eq!(fs::read(&store.0.path).unwrap(), original);
        second
            .state
            .display
            .layer_order
            .push(pomelo_core::model::LayerId(3));
        let error = store.save_updates(vec![second]).unwrap_err();
        for locale in Locale::ALL {
            assert!(!error.message.display(locale).to_string().is_empty());
        }
        assert_eq!(fs::read(&store.0.path).unwrap(), original);
    }
    #[test]
    fn view_store_replaces_existing_and_preserves_invalid_source_before_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let store = ViewStore(LanguageStore {
            path: directory.path().join("views.json"),
        });
        let mut entry = view_entry(directory.path().join("不存在的板.brd"));
        store.save(vec![entry.clone()]).unwrap();
        assert!(
            store
                .load_matching(&entry.path, &entry.state.source)
                .unwrap()
                .is_some()
        );
        let mut changed = entry.state.source.clone();
        changed.sha256[0] ^= 1;
        assert!(
            store
                .load_matching(&entry.path, &changed)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .load_matching(&directory.path().join("other.brd"), &entry.state.source)
                .unwrap()
                .is_none()
        );
        entry.state.camera.flipped = true;
        store.save(vec![entry.clone()]).unwrap();
        assert!(store.load().unwrap()[0].state.camera.flipped);
        fs::write(&store.0.path, b"broken-view-state").unwrap();
        assert!(store.load().is_err());
        store.save(vec![entry]).unwrap();
        let backup = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|file| {
                file.file_name()
                    .to_string_lossy()
                    .starts_with("views.json.recovery-")
            })
            .unwrap();
        assert_eq!(fs::read(backup.path()).unwrap(), b"broken-view-state");
        assert_eq!(store.load().unwrap().len(), 1);
    }
    #[test]
    fn view_store_rejects_duplicate_or_invalid_state_without_changing_file() {
        let directory = tempfile::tempdir().unwrap();
        let store = ViewStore(LanguageStore {
            path: directory.path().join("views.json"),
        });
        let mut entry = view_entry(directory.path().join("board.brd"));
        store.save(vec![entry.clone()]).unwrap();
        let before = fs::read(&store.0.path).unwrap();
        assert!(store.save(vec![entry.clone(), entry.clone()]).is_err());
        entry.state.camera.pixels_per_mm = 0.0;
        let error = store.save(vec![entry]).unwrap_err();
        for locale in Locale::ALL {
            assert!(!error.message.display(locale).to_string().is_empty());
        }
        assert_eq!(fs::read(&store.0.path).unwrap(), before);
        let _ = ViewStore::platform_default();
    }

    #[test]
    fn view_save_failure_preserves_existing_directory_and_reports_five_languages() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("views.json");
        fs::create_dir(&config_path).unwrap();
        let marker = config_path.join("keep.txt");
        fs::write(&marker, b"existing-data").unwrap();
        let store = ViewStore(LanguageStore {
            path: config_path.clone(),
        });
        let error = store
            .save_updates(vec![view_entry(directory.path().join("board.brd"))])
            .unwrap_err();
        for locale in Locale::ALL {
            assert!(!error.message.display(locale).to_string().is_empty());
        }
        assert!(config_path.is_dir());
        assert_eq!(fs::read(marker).unwrap(), b"existing-data");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_filter_on_disk_reports_localized_error_without_changing_source() {
        let directory = tempfile::tempdir().unwrap();
        let store = ViewStore(LanguageStore {
            path: directory.path().join("views.json"),
        });
        let entry = view_entry(directory.path().join("board.brd"));
        store.save(vec![entry.clone()]).unwrap();
        let mut file: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.0.path).unwrap()).unwrap();
        file["entries"][0]["state"]["pick_filter"] = serde_json::json!(32);
        let original = serde_json::to_vec(&file).unwrap();
        fs::write(&store.0.path, &original).unwrap();
        let error = store
            .load_matching(&entry.path, &entry.state.source)
            .unwrap_err();
        for locale in Locale::ALL {
            assert!(!error.message.display(locale).to_string().is_empty());
        }
        assert_eq!(fs::read(&store.0.path).unwrap(), original);
    }

    #[test]
    fn disk_unit_defaults_for_legacy_files_and_rejects_unknown_units_without_rewriting() {
        use pomelo_core::units::LengthUnit;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("views.json");
        let store = ViewStore(LanguageStore { path: path.clone() });
        let entry = view_entry(directory.path().join("board.brd"));
        store.save(vec![entry.clone()]).unwrap();
        let mut file: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        file["entries"][0]["state"]["display"]
            .as_object_mut()
            .unwrap()
            .remove("length_unit");
        fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let reopened = ViewStore(LanguageStore { path: path.clone() });
        assert_eq!(
            reopened
                .load_matching(&entry.path, &entry.state.source)
                .unwrap()
                .unwrap()
                .display
                .length_unit,
            LengthUnit::Millimeters
        );

        file["entries"][0]["state"]["display"]["length_unit"] = serde_json::json!("unknown-unit");
        let original = serde_json::to_vec(&file).unwrap();
        fs::write(&path, &original).unwrap();
        let error = reopened
            .load_matching(&entry.path, &entry.state.source)
            .unwrap_err();
        for locale in Locale::ALL {
            assert!(error.message.render(locale).is_ok());
        }
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn corrupt_preferences_are_backed_up_before_recovery_and_valid_saves_do_not_repeat_backup() {
        let directory = tempfile::tempdir().unwrap();
        let language = LanguageStore {
            path: directory.path().join("language.json"),
        };
        let theme = ThemeStore(LanguageStore {
            path: directory.path().join("theme.json"),
        });
        let recent = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        let originals = [
            (
                "language.json",
                br#"{"schema_version":1,"language":"unknown"}"#.to_vec(),
            ),
            ("theme.json", vec![b'x'; 4097]),
            (
                "recent.json",
                br#"{"schema_version":9,"entries":[]}"#.to_vec(),
            ),
        ];
        for (name, bytes) in &originals {
            fs::write(directory.path().join(name), bytes).unwrap();
        }
        language
            .save(LanguagePreference::Explicit(Locale::Japanese))
            .unwrap();
        theme.save(ThemePreference::Light).unwrap();
        recent.save(Vec::new()).unwrap();
        language
            .save(LanguagePreference::Explicit(Locale::Korean))
            .unwrap();
        theme.save(ThemePreference::Dark).unwrap();
        recent.save(Vec::new()).unwrap();
        for (name, original) in originals {
            let backups = fs::read_dir(directory.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(&format!("{name}.recovery-"))
                })
                .collect::<Vec<_>>();
            assert_eq!(backups.len(), 1);
            assert_eq!(fs::read(&backups[0]).unwrap(), original);
        }
        assert_eq!(
            language.load().unwrap(),
            LanguagePreference::Explicit(Locale::Korean)
        );
        assert_eq!(theme.load().unwrap(), ThemePreference::Dark);
        assert!(recent.load().unwrap().is_empty());
    }

    #[test]
    fn recovery_failure_preserves_original_and_returns_localized_save_diagnostic() {
        let directory = tempfile::tempdir().unwrap();
        let store = LanguageStore {
            path: directory.path().join("language.json"),
        };
        fs::create_dir(&store.path).unwrap();
        let sentinel = store.path.join("preserved");
        fs::write(&sentinel, b"original").unwrap();
        let error = store.save(LanguagePreference::System).unwrap_err();
        assert_eq!(error.code.as_ref(), "CONFIG_SAVE_FAILED");
        assert!(
            Locale::ALL
                .into_iter()
                .all(|locale| error.message.render(locale).is_ok())
        );
        assert_eq!(fs::read(sentinel).unwrap(), b"original");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    fn recent_entry(path: PathBuf, opened_unix_seconds: u64) -> RecentEntry {
        RecentEntry {
            path,
            opened_unix_seconds,
            format: "allegro".into(),
            encoding: pomelo_import::TextEncoding::Windows1252,
            presentation: None,
        }
    }

    #[test]
    fn recent_history_moves_reopened_board_to_front_and_bounds_the_list() {
        let directory = tempfile::tempdir().unwrap();
        let mut entries = Vec::new();
        for index in 0..25 {
            remember_recent(
                &mut entries,
                recent_entry(directory.path().join(format!("{index}.brd")), index),
            );
        }
        assert_eq!(entries.len(), RECENT_LIMIT);
        assert_eq!(entries.last().unwrap().opened_unix_seconds, 5);
        let reopened = directory.path().join("10.brd");
        remember_recent(&mut entries, recent_entry(reopened.clone(), 100));
        assert_eq!(entries[0].path, reopened);
        assert_eq!(entries[0].opened_unix_seconds, 100);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.path == reopened)
                .count(),
            1
        );
    }

    #[test]
    fn recent_history_roundtrip_preserves_encoding_unicode_and_missing_sources() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        assert!(store.load().unwrap().is_empty());
        let source = directory.path().join("缺失 空格.brd");
        store.save(vec![recent_entry(source.clone(), 123)]).unwrap();
        let entries = store.load().unwrap();
        assert_eq!(entries[0].path, source);
        assert_eq!(entries[0].format, "allegro");
        assert_eq!(entries[0].opened_unix_seconds, 123);
        assert_eq!(entries[0].encoding.tag(), "windows-1252");
        assert!(!source.exists());
        store.save(Vec::new()).unwrap();
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn legacy_history_accepts_new_metadata_without_rewriting_on_read() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        let legacy = recent_entry(directory.path().join("legacy.brd"), 123);
        store.save(vec![legacy]).unwrap();
        let bytes = fs::read(&store.0.path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("presentation"));
        assert!(store.load().unwrap()[0].presentation.is_none());
        assert_eq!(fs::read(&store.0.path).unwrap(), bytes);

        // A damaged optional image remains an ordinary, openable history entry.
        let mut enriched = recent_entry(directory.path().join("board.brd"), 124);
        enriched.presentation = Some(RecentPresentation {
            layer_count: 4,
            thumbnail_png: Some("invalid preview".into()),
        });
        let mut entries = store.load().unwrap();
        remember_recent(&mut entries, enriched.clone());
        store.save(entries).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].presentation, enriched.presentation);
        assert!(
            crate::services::preview::decode(
                loaded[0]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .thumbnail_png
                    .as_ref()
                    .unwrap()
            )
            .is_err()
        );
        assert!(loaded[1].presentation.is_none());
        let original = fs::read(&store.0.path).unwrap();
        enriched.presentation.as_mut().unwrap().thumbnail_png =
            Some("A".repeat(crate::services::preview::MAX_ENCODED_BYTES + 1));
        assert!(store.save(vec![enriched]).is_err());
        assert_eq!(fs::read(&store.0.path).unwrap(), original);
    }

    #[test]
    fn recent_history_preserves_all_supported_board_formats() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        let entries: Vec<_> = ["allegro", "altium", "odb", "pads", "kicad", "hfss"]
            .into_iter()
            .enumerate()
            .map(|(i, format)| {
                let mut entry = recent_entry(directory.path().join(format!("board-{i}")), i as u64);
                entry.format = format.into();
                entry
            })
            .collect();
        store.save(entries).unwrap();
        assert_eq!(
            store
                .load()
                .unwrap()
                .into_iter()
                .map(|entry| entry.format)
                .collect::<Vec<_>>(),
            ["allegro", "altium", "odb", "pads", "kicad", "hfss"]
        );
    }

    #[test]
    fn bounded_preview_history_fits_the_load_limit_and_roundtrips_pixels() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use image::ImageEncoder as _;
        let directory = tempfile::tempdir().unwrap();
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        let mut png = Vec::new();
        let pixels = image::RgbaImage::from_pixel(192, 128, image::Rgba([13, 27, 181, 255]));
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(pixels.as_raw(), 192, 128, image::ExtendedColorType::Rgba8)
            .unwrap();
        let encoded = STANDARD.encode(png);
        let mut entries: Vec<_> = (0..RECENT_LIMIT)
            .map(|index| {
                let mut entry =
                    recent_entry(directory.path().join(format!("{index}.brd")), index as u64);
                entry.presentation = Some(RecentPresentation {
                    layer_count: 4,
                    thumbnail_png: Some("A".repeat(crate::services::preview::MAX_ENCODED_BYTES)),
                });
                entry
            })
            .collect();
        entries[0].presentation.as_mut().unwrap().thumbnail_png = Some(encoded.clone());
        store.save(entries).unwrap();
        assert!(fs::metadata(&store.0.path).unwrap().len() < 4 * 1024 * 1024);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), RECENT_LIMIT);
        assert_eq!(
            loaded[0]
                .presentation
                .as_ref()
                .unwrap()
                .thumbnail_png
                .as_ref()
                .unwrap(),
            &encoded
        );
        let image = crate::services::preview::decode(&encoded).unwrap();
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[181, 27, 13, 255]);
    }

    #[test]
    fn successful_relocation_replaces_old_and_existing_new_history_without_touching_files() {
        let directory = tempfile::tempdir().unwrap();
        let old_path = directory.path().join("old.brd");
        let new_path = directory.path().join("new.brd");
        let other_path = directory.path().join("other.brd");
        fs::write(&old_path, b"old-source").unwrap();
        fs::write(&new_path, b"new-source").unwrap();
        let mut entries = vec![
            recent_entry(other_path.clone(), 3),
            recent_entry(new_path.clone(), 2),
            recent_entry(old_path.clone(), 1),
        ];
        remember_relocated(
            &mut entries,
            recent_entry(new_path.clone(), 100),
            Some(&old_path),
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, new_path);
        assert_eq!(entries[0].opened_unix_seconds, 100);
        assert_eq!(entries[1].path, other_path);
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        store.save(entries).unwrap();
        assert_eq!(store.load().unwrap().len(), 2);
        assert_eq!(fs::read(old_path).unwrap(), b"old-source");
        assert_eq!(fs::read(new_path).unwrap(), b"new-source");
    }

    #[test]
    fn invalid_recent_history_is_rejected_without_replacing_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let store = RecentStore(LanguageStore {
            path: directory.path().join("recent.json"),
        });
        let entry = recent_entry(directory.path().join("board.brd"), 1);
        store.save(vec![entry.clone()]).unwrap();
        let original = fs::read(&store.0.path).unwrap();
        for entries in [
            vec![entry.clone(), entry.clone()],
            vec![recent_entry("relative.brd".into(), 1)],
            vec![entry; RECENT_LIMIT + 1],
        ] {
            assert!(store.save(entries).is_err());
            assert_eq!(fs::read(&store.0.path).unwrap(), original);
        }
        let invalid = br#"{"schema_version":2,"entries":[]}"#;
        fs::write(&store.0.path, invalid).unwrap();
        let error = store.load().unwrap_err();
        assert!(
            Locale::ALL
                .into_iter()
                .all(|locale| error.message.render(locale).is_ok())
        );
        assert_eq!(fs::read(&store.0.path).unwrap(), invalid);
    }

    #[test]
    fn theme_replaces_existing_file_without_changing_language() {
        let directory = tempfile::tempdir().unwrap();
        let language = LanguageStore {
            path: directory.path().join("language.json"),
        };
        language
            .save(LanguagePreference::Explicit(Locale::Korean))
            .unwrap();
        let theme = ThemeStore(LanguageStore {
            path: directory.path().join("theme.json"),
        });
        assert_eq!(theme.load().unwrap(), ThemePreference::Dark);
        theme.save(ThemePreference::Light).unwrap();
        assert_eq!(theme.load().unwrap(), ThemePreference::Light);
        theme.save(ThemePreference::Dark).unwrap();
        assert_eq!(theme.load().unwrap(), ThemePreference::Dark);
        assert_eq!(
            language.load().unwrap(),
            LanguagePreference::Explicit(Locale::Korean)
        );
    }

    #[test]
    fn invalid_theme_schema_value_and_oversized_file_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let theme = ThemeStore(LanguageStore {
            path: directory.path().join("theme.json"),
        });
        for source in [
            br#"{"schema_version":2,"theme":"dark"}"#.to_vec(),
            br#"{"schema_version":1,"theme":"unknown"}"#.to_vec(),
            vec![b' '; 4097],
        ] {
            fs::write(&theme.0.path, &source).unwrap();
            let error = theme.load().unwrap_err();
            assert!(
                Locale::ALL
                    .into_iter()
                    .all(|locale| error.message.render(locale).is_ok())
            );
            assert_eq!(fs::read(&theme.0.path).unwrap(), source);
        }
    }

    #[test]
    fn preference_replaces_existing_file_and_restores_exact_language() {
        let directory = tempfile::tempdir().unwrap();
        let store = LanguageStore {
            path: directory.path().join("language.json"),
        };
        store
            .save(LanguagePreference::Explicit(Locale::Japanese))
            .unwrap();
        store
            .save(LanguagePreference::Explicit(Locale::TraditionalChinese))
            .unwrap();
        assert_eq!(
            store.load().unwrap(),
            LanguagePreference::Explicit(Locale::TraditionalChinese)
        );
    }

    #[test]
    fn invalid_preference_returns_a_translatable_diagnostic_without_overwriting_source() {
        let directory = tempfile::tempdir().unwrap();
        let store = LanguageStore {
            path: directory.path().join("language.json"),
        };
        let original = br#"{"schema_version":1,"language":"unknown"}"#;
        fs::write(&store.path, original).unwrap();
        let error = store.load().unwrap_err();
        assert!(
            Locale::ALL
                .into_iter()
                .all(|locale| error.message.render(locale).is_ok())
        );
        assert_eq!(fs::read(&store.path).unwrap(), original);
    }
}
