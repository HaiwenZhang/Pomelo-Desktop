//! Bounded JSON reads, atomic writes and recovery copies for configuration files.
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub(super) struct JsonFileStore {
    pub(super) path: PathBuf,
}
impl JsonFileStore {
    pub(super) fn in_configuration_directory(file_name: &str) -> Option<Self> {
        super::configuration_directory().map(|root| Self {
            path: root.join(file_name),
        })
    }
    pub(super) fn load_file<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<Option<T>, Diagnostic> {
        self.load_file_limited(4096)
    }

    pub(super) fn load_file_limited<T: serde::de::DeserializeOwned>(
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

    pub(super) fn save_file(&self, value: &impl Serialize) -> Result<(), Diagnostic> {
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

    pub(super) fn save_failure(
        &self,
        stage: &'static str,
        details: impl std::fmt::Display,
    ) -> Diagnostic {
        self.failure(
            MessageKey::ConfigSaveFailed,
            format!("stage={stage}; {details}"),
        )
    }

    pub(super) fn preserve_invalid(&self, invalid: bool) -> Result<(), Diagnostic> {
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

    pub(super) fn failure(&self, key: MessageKey, details: impl std::fmt::Display) -> Diagnostic {
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
