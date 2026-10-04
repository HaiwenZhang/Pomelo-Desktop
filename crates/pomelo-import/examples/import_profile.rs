//! Profile the production Allegro index and scene stages on explicit local inputs.
use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportOptions, TextEncoding,
    allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::IndexLimits,
        semantics::scene::{SceneBuilder, SceneLimits},
    },
};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(!paths.is_empty(), "usage: import_profile <board.brd> ...");
    for path in paths {
        let cancel = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        };
        let options = ImportOptions {
            text_encoding: TextEncoding::Windows1252,
            ..Default::default()
        };
        let started = Instant::now();
        let database = BrdDatabase::read_path(
            &path,
            &options,
            &IndexLimits::default(),
            DecodeLimits::default(),
            &context,
        )?;
        println!(
            "case={} stage=source_index elapsed_us={}",
            path.display(),
            started.elapsed().as_micros()
        );
        let hash_started = Instant::now();
        let hash = pomelo_import::source::content_sha256(database.source_bytes(), &context)?;
        let hash: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        println!(
            "case={} stage=hash elapsed_us={} sha256={hash}",
            path.display(),
            hash_started.elapsed().as_micros()
        );
        let scene = SceneBuilder::new(&database, SceneLimits::default())
            .build_with_stage_observer(&context, &|stage, elapsed| {
                println!(
                    "case={} stage={stage} elapsed_us={}",
                    path.display(),
                    elapsed.as_micros()
                );
            })?;
        println!(
            "case={} elapsed_ms={} segments={} vias={} pins={} zones={} texts={} drawings={}",
            path.display(),
            started.elapsed().as_millis(),
            scene.segments.len(),
            scene.vias.len(),
            scene.pins.len(),
            scene.zones.len(),
            scene.texts.len(),
            scene.drawings.len()
        );
    }
    Ok(())
}
