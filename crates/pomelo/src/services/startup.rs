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
    pub source_font: Option<super::source_font::SourceFontConfig>,
}

impl Startup {
    /// Resolve an explicit locale even when another argument prevents startup.
    pub fn diagnostic_locale(args: &[OsString], fallback: Locale) -> Locale {
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if arg == "--encoding" || arg == "--source-font" || arg == "--source-font-block" {
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
        let mut source_font = None;
        let mut source_block = None;
        while let Some(arg) = args.next() {
            if arg == "--" {
                result.paths.extend(args.map(PathBuf::from));
                break;
            }
            if arg == "--encoding"
                || arg == "--locale"
                || arg == "--source-font"
                || arg == "--source-font-block"
            {
                let option = arg.to_string_lossy();
                let value = args.next().ok_or_else(|| {
                    let mut error = Diagnostic::error("CLI_MISSING_VALUE", Key::CliMissingValue);
                    error.message = error.message.arg("option", option.as_ref());
                    error
                })?;
                let tag = value.to_string_lossy();
                if arg == "--source-font" {
                    if source_font.is_some() || value.is_empty() {
                        return Err(invalid_option(option.as_ref()));
                    }
                    source_font = Some(PathBuf::from(value));
                } else if arg == "--source-font-block" {
                    if source_block.is_some() {
                        return Err(invalid_option(option.as_ref()));
                    }
                    source_block = Some(
                        tag.parse::<u8>()
                            .map_err(|_| invalid_option(option.as_ref()))?,
                    );
                } else if arg == "--encoding" {
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
        result.source_font = match (source_font, source_block) {
            (Some(path), Some(text_block)) => {
                Some(super::source_font::SourceFontConfig { path, text_block })
            }
            (None, None) => None,
            _ => {
                return Err(Diagnostic::error(
                    "CLI_SOURCE_FONT_OPTIONS",
                    Key::SourceFontOptions,
                ));
            }
        };
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
    fn explicit_source_font_and_block_do_not_become_board_paths() {
        let startup = parse(&[
            "--source-font",
            "字体 目录/ansi.dat",
            "--source-font-block",
            "3",
            "--locale",
            "ja",
            "board.brd",
        ])
        .unwrap();
        let config = startup.source_font.unwrap();
        assert_eq!(config.path, PathBuf::from("字体 目录/ansi.dat"));
        assert_eq!(config.text_block, 3);
        assert_eq!(startup.paths, [PathBuf::from("board.brd")]);
        assert_eq!(startup.locale, Some(Locale::Japanese));
        assert!(parse(&["board.brd"]).unwrap().source_font.is_none());
        assert_eq!(
            Startup::diagnostic_locale(
                &["--source-font", "--locale", "ja"].map(OsString::from),
                Locale::English
            ),
            Locale::English
        );
    }
    #[test]
    fn source_font_requires_a_single_valid_explicit_block_in_all_languages() {
        for args in [
            vec!["--source-font", "ansi.dat"],
            vec!["--source-font-block", "3"],
            vec!["--source-font"],
            vec!["--source-font-block", "-1"],
            vec!["--source-font-block", "256"],
            vec![
                "--source-font",
                "ansi.dat",
                "--source-font",
                "other.dat",
                "--source-font-block",
                "3",
            ],
            vec![
                "--source-font",
                "ansi.dat",
                "--source-font-block",
                "3",
                "--source-font-block",
                "4",
            ],
        ] {
            let error = parse(&args).unwrap_err();
            for locale in Locale::ALL {
                assert!(error.message.render(locale).is_ok());
            }
        }
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
