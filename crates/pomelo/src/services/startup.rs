//! Parse desktop options without treating option values as board paths.

use pomelo_core::{
    i18n::{Locale, MessageKey as Key},
    model::Diagnostic,
};
use pomelo_import::{ImportOptions, TextEncoding};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Default)]
pub struct Startup {
    pub paths: Vec<PathBuf>,
    pub options: ImportOptions,
    pub locale: Option<Locale>,
}

impl Startup {
    /// Resolve an explicit locale even when another argument prevents startup.
    pub fn diagnostic_locale(args: &[OsString], fallback: Locale) -> Locale {
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if arg == "--encoding" {
                args.next();
            } else if arg == "--locale"
                && let Some(tag) = args.next()
                && let Some(locale) = Locale::ALL
                    .into_iter()
                    .find(|locale| locale.tag() == tag.to_string_lossy())
            {
                return locale;
            }
        }
        fallback
    }

    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, Diagnostic> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        let mut encoding_seen = false;
        while let Some(arg) = args.next() {
            if arg == "--" {
                result.paths.extend(args.map(PathBuf::from));
                break;
            }
            if arg == "--encoding" || arg == "--locale" {
                let option = arg.to_string_lossy();
                let value = args.next().ok_or_else(|| {
                    let mut error = Diagnostic::error("CLI_MISSING_VALUE", Key::CliMissingValue);
                    error.message = error.message.arg("option", option.as_ref());
                    error
                })?;
                let tag = value.to_string_lossy();
                if arg == "--encoding" {
                    if encoding_seen {
                        return Err(invalid_option(option.as_ref()));
                    }
                    encoding_seen = true;
                    result.options.text_encoding =
                        TextEncoding::from_tag(&tag).ok_or_else(|| {
                            let mut error = Diagnostic::error(
                                "CLI_UNSUPPORTED_ENCODING",
                                Key::CliUnsupportedEncoding,
                            );
                            error.message = error.message.arg("encoding", tag.as_ref());
                            error
                        })?;
                } else {
                    if result.locale.is_some() {
                        return Err(invalid_option(option.as_ref()));
                    }
                    result.locale = Some(
                        Locale::ALL
                            .into_iter()
                            .find(|locale| locale.tag() == tag)
                            .ok_or_else(|| {
                                let mut error = Diagnostic::error(
                                    "CLI_UNSUPPORTED_LOCALE",
                                    Key::CliUnsupportedLocale,
                                );
                                error.message = error.message.arg("locale", tag.as_ref());
                                error
                            })?,
                    );
                }
            } else if arg.to_string_lossy().starts_with('-') {
                return Err(invalid_option(&arg.to_string_lossy()));
            } else {
                result.paths.push(arg.into());
            }
        }
        Ok(result)
    }
}

fn invalid_option(option: &str) -> Diagnostic {
    let mut error = Diagnostic::error("APP_INVALID_OPTION", Key::StartupInvalidOption);
    error.message = error.message.arg("option", option);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Startup, Diagnostic> {
        Startup::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn startup_error_locale_respects_option_values_and_separator() {
        for locale in Locale::ALL {
            let args = ["--unknown", "--locale", locale.tag()].map(OsString::from);
            assert_eq!(Startup::diagnostic_locale(&args, Locale::English), locale);
        }
        for args in [
            vec!["--", "--locale", "ja"],
            vec!["--encoding", "--locale", "ja"],
            vec!["--locale", "unknown"],
        ] {
            assert_eq!(
                Startup::diagnostic_locale(
                    &args.into_iter().map(OsString::from).collect::<Vec<_>>(),
                    Locale::English,
                ),
                Locale::English
            );
        }
    }
    #[test]
    fn startup_error_uses_saved_or_system_locale_without_valid_override() {
        for fallback in Locale::ALL {
            for args in [
                vec!["--unknown"],
                vec!["--locale", "unknown"],
                vec!["--", "--locale", "ja"],
            ] {
                let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
                assert_eq!(Startup::diagnostic_locale(&args, fallback), fallback);
            }
        }
    }
    #[test]
    fn language_and_encoding_are_independent_of_paths() {
        let startup = parse(&[
            "first.brd",
            "--locale",
            "ja",
            "--encoding",
            "windows-1252",
            "second.brd",
        ])
        .unwrap();
        assert_eq!(
            startup.paths,
            [PathBuf::from("first.brd"), PathBuf::from("second.brd")]
        );
        assert_eq!(startup.locale, Some(Locale::Japanese));
        assert_eq!(startup.options.text_encoding.tag(), "windows-1252");
    }
    #[test]
    fn separator_preserves_option_like_paths() {
        assert_eq!(
            parse(&["--", "--locale", "ja"]).unwrap().paths,
            [PathBuf::from("--locale"), PathBuf::from("ja")]
        );
    }
    #[test]
    fn invalid_arguments_are_localized_in_all_languages() {
        for args in [
            vec!["--encoding"],
            vec!["--encoding", "unknown"],
            vec!["--locale", "unknown"],
            vec!["--locale", "en", "--locale", "ja"],
            vec!["--encoding", "gbk", "--encoding", "big5"],
            vec!["--unknown"],
        ] {
            let error = parse(&args).unwrap_err();
            for locale in Locale::ALL {
                assert!(error.message.render(locale).is_ok());
            }
        }
    }
}
