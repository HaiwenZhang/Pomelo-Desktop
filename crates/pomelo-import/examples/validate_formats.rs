//! Validate supplied boards through the same dispatcher used by the desktop app.
use pomelo_core::{search::SearchIndex, task::CancellationToken};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cancel = CancellationToken::default();
    let mut failed = false;
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let start = std::time::Instant::now();
        match FormatImporter.import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|p| eprintln!("{:?}: {}", p.stage, p.completed),
            },
        ) {
            Ok(board) => {
                let s = &board.scene;
                let search = SearchIndex::build(s, &cancel)?;
                println!(
                    "{}: {} layers={} nets={} tracks={} pins={} components={} vias={} zones={} texts={} drawings={} diagnostics={} search={:?} elapsed={:.2}s",
                    path.display(),
                    board.source.format,
                    s.layers.len(),
                    s.nets.len(),
                    s.segments.len(),
                    s.pins.len(),
                    s.components.len(),
                    s.vias.len(),
                    s.zones.len(),
                    s.texts.len(),
                    s.drawings.len(),
                    s.diagnostics.len(),
                    search.is_some(),
                    start.elapsed().as_secs_f64()
                );
            }
            Err(error) => {
                failed = true;
                eprintln!("{}: {error}", path.display());
            }
        }
    }
    if failed {
        return Err("One or more imports failed".into());
    }
    Ok(())
}
