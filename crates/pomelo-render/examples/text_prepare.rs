//! Explicit real-board CPU preparation probe; no GPU/window success claim.
use pomelo_core::task::CancellationToken;
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
use pomelo_render::{
    text::{FontLimits, StrokeFont},
    tracks::{PreparedTracks, TraceLimits},
};

fn main() {
    if let Err(error) = prepare() {
        eprintln!(
            "{} {:?}: {}",
            error.code,
            error.object,
            error.message.display(pomelo_core::i18n::Locale::English)
        );
        std::process::exit(1);
    }
}

fn prepare() -> Result<(), pomelo_core::model::Diagnostic> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments.next().ok_or_else(|| {
        let mut diagnostic = pomelo_core::model::Diagnostic::error(
            "CLI_MISSING_VALUE",
            pomelo_core::i18n::MessageKey::CliMissingValue,
        );
        diagnostic.message = diagnostic.message.arg("option", "BOARD_PATH");
        diagnostic
    })?;
    let mut options = ImportOptions::default();
    if let Some(encoding) = arguments.next() {
        let encoding = encoding.to_string_lossy();
        options.text_encoding =
            pomelo_import::TextEncoding::from_tag(&encoding).ok_or_else(|| {
                let mut diagnostic = pomelo_core::model::Diagnostic::error(
                    "CLI_UNSUPPORTED_ENCODING",
                    pomelo_core::i18n::MessageKey::CliUnsupportedEncoding,
                );
                diagnostic.message = diagnostic.message.arg("encoding", encoding.as_ref());
                diagnostic
            })?;
    }
    let token = CancellationToken::default();
    let mut compact = false;
    let count_only = match arguments.next() {
        None => false,
        Some(argument) if argument == "--count-strokes" => true,
        Some(argument) if argument == "--compact" => {
            compact = true;
            false
        }
        Some(argument) => {
            let mut error = pomelo_core::model::Diagnostic::error(
                "APP_INVALID_OPTION",
                pomelo_core::i18n::MessageKey::StartupInvalidOption,
            );
            error.message = error
                .message
                .arg("option", argument.to_string_lossy().as_ref());
            return Err(error);
        }
    };
    if let Some(argument) = arguments.next() {
        let mut error = pomelo_core::model::Diagnostic::error(
            "APP_INVALID_OPTION",
            pomelo_core::i18n::MessageKey::StartupInvalidOption,
        );
        error.message = error
            .message
            .arg("option", argument.to_string_lossy().as_ref());
        return Err(error);
    }
    let board = AllegroImporter
        .import(
            std::path::Path::new(&path),
            &options,
            &ImportContext {
                cancellation: &token,
                progress: &|_| {},
            },
        )
        .map_err(|error| error.diagnostic())?;
    let font = StrokeFont::bundled_for_recovering_texts(
        &board.scene.texts,
        2_000_000,
        4 * 1024 * 1024,
        FontLimits {
            glyphs: 65_536,
            encoded_bytes: 4 * 1024 * 1024,
            points_per_glyph: 1024,
            strokes: 1_000_000,
        },
        &token,
    );
    if count_only {
        let font = font?;
        let (strokes, missing) =
            pomelo_render::text::count_text_strokes(&board.scene.texts, &font, 2_000_000, &token)?;
        println!(
            "TEXT_STROKE_COUNT objects={} strokes={} missing_objects={} geometry_verified=false",
            board.scene.texts.len(),
            strokes,
            missing
        );
        return Ok(());
    }
    if compact {
        let font = font?;
        let prepared = pomelo_render::text_instances::PreparedTextInstances::build(
            &board.scene.texts,
            &font,
            1_000_000,
            2_000_000,
            512 * 1024 * 1024,
            &token,
        )?;
        println!(
            "TEXT_COMPACT objects={} prepared_objects={} skipped_objects={} instances={} instance_bytes={} gpu_verified=false",
            board.scene.texts.len(),
            prepared.summary.objects,
            prepared.summary.diagnostics.len(),
            prepared.instances.len(),
            prepared.instances.len()
                * std::mem::size_of::<pomelo_render::text_instances::TextInstance>()
        );
        return Ok(());
    }
    let result = font.and_then(|font| {
        let (tracks, _, summary) = PreparedTracks::build_source_texts(
            &board.scene.texts,
            &font,
            1_000_000,
            2_000_000,
            TraceLimits::default(),
            &token,
        )?;
        Ok((tracks, summary.objects, summary.diagnostics))
    });
    match result {
        Ok((tracks, prepared_objects, diagnostics)) => {
            for diagnostic in &diagnostics {
                eprintln!(
                    "{} {:?}: {}",
                    diagnostic.code,
                    diagnostic.object,
                    diagnostic
                        .message
                        .display(pomelo_core::i18n::Locale::English)
                );
            }
            println!(
                "TEXT_PREPARED objects={} prepared_objects={} skipped_objects={} instances={} layers={}",
                board.scene.texts.len(),
                prepared_objects,
                diagnostics.len(),
                tracks.instances.len(),
                tracks.batches.len()
            );
        }
        Err(error) => {
            return Err(error);
        }
    }
    Ok(())
}
