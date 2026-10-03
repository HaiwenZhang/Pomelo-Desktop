//! Startup feedback that does not depend on a GPUI window or GPU device.

use std::{
    fs,
    io::{self, Write},
    panic::{UnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use pomelo_core::{
    i18n::{LanguagePreference, Locale, Message, MessageKey as Key, text},
    model::Diagnostic,
};

use super::prefs;

const LOG_LIMIT: usize = 16 * 1024;

pub fn preferred_locale() -> Locale {
    let preference = prefs::LanguageStore::platform_default()
        .and_then(|store| store.load().ok())
        .unwrap_or_default();
    resolve_locale(preference, sys_locale::get_locale().as_deref())
}

fn resolve_locale(preference: LanguagePreference, system_tag: Option<&str>) -> Locale {
    preference.resolve(system_tag)
}

/// GPUI Kit's Windows constructor currently calls `expect` on platform errors.
/// Catch only that constructor's unwind; never resume a failed application.
pub fn try_initialize<T>(initialize: impl FnOnce() -> T + UnwindSafe) -> Result<T, Diagnostic> {
    catch_unwind(initialize).map_err(|payload| {
        let details = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("PLATFORM_INITIALIZATION_PANIC");
        Diagnostic::error("APP_PLATFORM_FAILED", Key::StartupPlatformFailed)
            .with_details(bounded(details, LOG_LIMIT / 2))
    })
}

pub fn report(locale: Locale, diagnostic: &Diagnostic) {
    let presentation = prepare(
        locale,
        diagnostic,
        prefs::configuration_directory().as_deref(),
    );
    #[cfg(target_os = "windows")]
    {
        let open_folder = text(locale, Key::StartupFailureOpenLogFolder);
        let buttons = match &presentation.log_folder {
            Some(_) => rfd::MessageButtons::OkCancelCustom(presentation.close, open_folder.clone()),
            None => rfd::MessageButtons::OkCustom(presentation.close),
        };
        let result = rfd::MessageDialog::new()
            .set_title(presentation.title)
            .set_description(presentation.description)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(buttons)
            .show();
        if result == rfd::MessageDialogResult::Custom(open_folder)
            && let Some(folder) = presentation.log_folder
            && let Err(diagnostic) = open_log_folder(&folder, |folder| {
                std::process::Command::new("explorer.exe")
                    .arg(folder)
                    .spawn()
                    .map(|_| ())
            })
        {
            // Preserve the original log if launching Explorer itself fails.
            let description = format!(
                "{}\n\n{}\n{}",
                diagnostic.message.display(locale),
                text(locale, Key::TechnicalDetails),
                technical_details(&diagnostic, 384)
            );
            rfd::MessageDialog::new()
                .set_title(text(locale, Key::StartupFailureTitle))
                .set_description(description)
                .set_level(rfd::MessageLevel::Error)
                .set_buttons(rfd::MessageButtons::OkCustom(text(
                    locale,
                    Key::StartupFailureClose,
                )))
                .show();
        }
    }
    #[cfg(not(target_os = "windows"))]
    eprintln!("{}\n{}", presentation.title, presentation.description);
}

struct Presentation {
    title: String,
    description: String,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    close: String,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    log_folder: Option<PathBuf>,
}

fn prepare(locale: Locale, diagnostic: &Diagnostic, root: Option<&Path>) -> Presentation {
    let log = root.and_then(|root| write_log(root, locale, diagnostic).ok());
    let primary = bounded(&diagnostic.message.display(locale), 768);
    let technical = technical_details(diagnostic, 384);
    let log_message = match &log {
        Some(path) => Message::new(Key::StartupFailureLog)
            .arg("path", bounded(&path.to_string_lossy(), 1024))
            .display(locale),
        None => text(locale, Key::StartupFailureLogUnavailable),
    };
    Presentation {
        title: text(locale, Key::StartupFailureTitle),
        description: format!(
            "{primary}\n\n{}\n{technical}\n\n{log_message}",
            text(locale, Key::TechnicalDetails)
        ),
        close: text(locale, Key::StartupFailureClose),
        log_folder: log.and_then(|path| path.parent().map(Path::to_path_buf)),
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn open_log_folder(
    folder: &Path,
    launch: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), Diagnostic> {
    let result = if folder.is_absolute() {
        launch(folder)
    } else {
        Err(io::Error::from(io::ErrorKind::InvalidInput))
    };
    result.map_err(|error| {
        let mut diagnostic = Diagnostic::error(
            "APP_LOG_FOLDER_OPEN_FAILED",
            Key::StartupFailureLogOpenFailed,
        )
        .with_details(error.to_string());
        diagnostic.message = diagnostic
            .message
            .arg("path", bounded(&folder.to_string_lossy(), 1024));
        diagnostic
    })
}

fn technical_details(diagnostic: &Diagnostic, limit: usize) -> String {
    let mut result = bounded(&diagnostic.code, 128);
    if let Some(details) = &diagnostic.technical_details {
        result.push('\n');
        result.push_str(&bounded(details, limit));
    }
    result
}

fn write_log(root: &Path, locale: Locale, diagnostic: &Diagnostic) -> io::Result<PathBuf> {
    if !root.is_absolute() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let directory = root.join("logs");
    fs::create_dir_all(&directory)?;
    let path = directory.join("startup-error.log");
    let report = format!(
        "{}\nlocale={}\ntime={}\n\n{}\n\n{}\n{}\n",
        text(locale, Key::StartupFailureTitle),
        locale.tag(),
        chrono::Utc::now().to_rfc3339(),
        bounded(&diagnostic.message.display(locale), LOG_LIMIT / 3),
        text(locale, Key::TechnicalDetails),
        technical_details(diagnostic, LOG_LIMIT / 2),
    );
    // Keep only the latest startup failure; startup never reads a previous log.
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    temporary.write_all(bounded(&report, LOG_LIMIT).as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).map_err(|error| error.error)?;
    Ok(path)
}

/// Cap UTF-8 bytes and remove controls that would truncate Win32 dialog strings.
fn bounded(value: &str, limit: usize) -> String {
    let mut result = String::new();
    for character in value.chars() {
        let character = if character.is_control() && character != '\n' && character != '\t' {
            ' '
        } else {
            character
        };
        if result.len() + character.len_utf8() > limit.saturating_sub(3) {
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid_option(option: &str) -> Diagnostic {
        let mut diagnostic = Diagnostic::error("APP_INVALID_OPTION", Key::StartupInvalidOption);
        diagnostic.message = diagnostic.message.arg("option", option);
        diagnostic
    }

    #[test]
    fn early_locale_honors_explicit_preferences_and_normalized_system_tags() {
        for locale in Locale::ALL {
            assert_eq!(
                resolve_locale(LanguagePreference::Explicit(locale), Some("unknown")),
                locale
            );
        }
        for (tag, expected) in [
            ("zh-Hant-HK", Locale::TraditionalChinese),
            ("zh_CN", Locale::SimplifiedChinese),
            ("ja-JP", Locale::Japanese),
            ("ko-KR", Locale::Korean),
            ("en-US", Locale::English),
            ("fr-FR", Locale::English),
        ] {
            assert_eq!(
                resolve_locale(LanguagePreference::System, Some(tag)),
                expected
            );
        }
    }

    #[test]
    fn native_presentations_use_all_five_catalogs_and_log_locations() {
        let root = tempfile::tempdir().unwrap();
        for locale in Locale::ALL {
            let diagnostic = invalid_option("--incorrect");
            let presentation = prepare(locale, &diagnostic, Some(root.path()));
            assert_eq!(presentation.title, text(locale, Key::StartupFailureTitle));
            assert_eq!(presentation.close, text(locale, Key::StartupFailureClose));
            assert!(
                presentation
                    .description
                    .contains(&diagnostic.message.display(locale))
            );
            assert!(presentation.description.contains("--incorrect"));
            assert!(presentation.description.contains("APP_INVALID_OPTION"));
            assert!(presentation.description.contains("startup-error.log"));
            assert_eq!(presentation.log_folder, Some(root.path().join("logs")));
            assert!(!presentation.description.contains("%{"));
            if locale != Locale::English {
                assert!(!presentation.description.contains("Technical details"));
            }
        }
    }

    #[test]
    fn unavailable_log_never_hides_the_original_failure() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("not-a-directory");
        fs::write(&blocked, "preserve").unwrap();
        for locale in Locale::ALL {
            for root in [Some(blocked.as_path()), None, Some(Path::new("relative"))] {
                let diagnostic = invalid_option("--incorrect");
                let presentation = prepare(locale, &diagnostic, root);
                assert!(
                    presentation
                        .description
                        .contains(&diagnostic.message.display(locale))
                );
                assert!(
                    presentation
                        .description
                        .contains(&text(locale, Key::StartupFailureLogUnavailable))
                );
                assert!(!presentation.description.contains("startup-error.log"));
                assert!(presentation.log_folder.is_none());
            }
        }
        assert_eq!(fs::read_to_string(blocked).unwrap(), "preserve");
    }

    #[test]
    fn bounded_log_replaces_previous_report_and_sanitizes_multibyte_controls() {
        let root = tempfile::tempdir().unwrap();
        let diagnostic = invalid_option("--incorrect");
        let path = write_log(root.path(), Locale::English, &diagnostic).unwrap();
        fs::write(&path, vec![b'x'; LOG_LIMIT * 2]).unwrap();
        let huge =
            invalid_option(&"中文\0\u{1b}".repeat(10000)).with_details("日本語\0".repeat(10000));
        write_log(root.path(), Locale::Japanese, &huge).unwrap();
        let written = fs::read_to_string(path).unwrap();
        assert!(written.len() <= LOG_LIMIT);
        assert!(written.contains("APP_INVALID_OPTION"));
        assert!(written.contains("locale=ja"));
        assert!(!written.contains('\0'));
        assert!(!written.contains('\u{1b}'));
        assert!(written.contains('…'));
        assert_eq!(fs::read_dir(root.path().join("logs")).unwrap().count(), 1);
    }

    #[test]
    fn platform_constructor_failure_becomes_a_localizable_diagnostic() {
        let result = try_initialize(|| panic!("D3D11_TEST_FAILURE"));
        let diagnostic = result.unwrap_err();
        assert_eq!(&*diagnostic.code, "APP_PLATFORM_FAILED");
        assert_eq!(
            diagnostic.technical_details.as_deref(),
            Some("D3D11_TEST_FAILURE")
        );
        for locale in Locale::ALL {
            assert!(diagnostic.message.render(locale).is_ok());
        }
        assert_eq!(try_initialize(|| 42).unwrap(), 42);
    }

    #[test]
    fn log_folder_action_uses_written_directory_and_preserves_log_on_launch_failure() {
        let root = tempfile::tempdir().unwrap();
        let presentation = prepare(
            Locale::English,
            &invalid_option("--incorrect"),
            Some(root.path()),
        );
        let folder = presentation.log_folder.unwrap();
        let original = fs::read(folder.join("startup-error.log")).unwrap();
        open_log_folder(&folder, |received| {
            assert_eq!(received, root.path().join("logs"));
            Ok(())
        })
        .unwrap();
        let diagnostic = open_log_folder(&folder, |_| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        assert_eq!(&*diagnostic.code, "APP_LOG_FOLDER_OPEN_FAILED");
        for locale in Locale::ALL {
            assert!(
                diagnostic
                    .message
                    .render(locale)
                    .unwrap()
                    .contains(&folder.to_string_lossy().to_string())
            );
            assert_ne!(
                text(locale, Key::StartupFailureOpenLogFolder),
                text(locale, Key::StartupFailureClose)
            );
        }
        assert_eq!(
            fs::read(folder.join("startup-error.log")).unwrap(),
            original
        );
        assert!(
            open_log_folder(Path::new("relative"), |_| panic!(
                "must not launch relative paths"
            ))
            .is_err()
        );
    }
}
