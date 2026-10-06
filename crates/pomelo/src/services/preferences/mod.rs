//! Versioned preferences, recent history and document view persistence.
mod json_file;
mod language;
mod panels;
mod recent;
mod theme;
mod view_state;

pub use language::LanguageStore;
pub use panels::{PanelPreferences, PanelStore};
pub use recent::{RECENT_LIMIT, RecentEntry, RecentPresentation, RecentStore, remember_relocated};
pub use theme::{ThemePreference, ThemeStore};
pub use view_state::{ViewEntry, ViewStore};

use std::path::PathBuf;
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

#[cfg(test)]
mod tests;
