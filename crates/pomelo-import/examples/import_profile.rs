//! Profile the production Allegro index and scene stages on caller-supplied inputs.
use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportOptions, TextEncoding,
    formats::allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        index::IndexLimits,
        semantics::scene::{SceneBuilder, SceneLimits},
    },
};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mut paths = Vec::<PathBuf>::new();
    let mut workers = 0usize;
    while let Some(arg) = args.next() {
        if arg == "--mesh-workers" {
            workers = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing --mesh-workers value"))?
                .to_string_lossy()
                .parse()?;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    anyhow::ensure!(
        !paths.is_empty(),
        "usage: import_profile [--mesh-workers N] <board> ..."
    );
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
            .with_mesh_workers(workers)
            .build_with_stage_observer(&context, &|stage, elapsed| {
                println!(
                    "case={} stage={stage} elapsed_us={}",
                    path.display(),
                    elapsed.as_micros()
                );
            })?;
        let complete_ms = started.elapsed().as_millis();
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        for zone in &scene.zones {
            for value in [zone.id.0, zone.layer.0, zone.net.0, zone.mesh.outer_count] {
                digest.update(value.to_le_bytes());
            }
            for point in &zone.mesh.vertices {
                digest.update(point.x.to_bits().to_le_bytes());
                digest.update(point.y.to_bits().to_le_bytes());
            }
            for values in [
                &zone.mesh.indices,
                &zone.mesh.ring_offsets,
                &zone.mesh.ring_order,
            ] {
                digest.update((values.len() as u64).to_le_bytes());
                for value in values {
                    digest.update(value.to_le_bytes());
                }
            }
            for bounds in &zone.mesh.ring_bounds {
                for point in [bounds.min, bounds.max] {
                    digest.update(point.x.to_bits().to_le_bytes());
                    digest.update(point.y.to_bits().to_le_bytes());
                }
            }
            for chunk in &zone.mesh.hole_chunks {
                for value in [chunk.start, chunk.count, chunk.ring_start, chunk.ring_count] {
                    digest.update(value.to_le_bytes());
                }
                for point in [chunk.bounds.min, chunk.bounds.max] {
                    digest.update(point.x.to_bits().to_le_bytes());
                    digest.update(point.y.to_bits().to_le_bytes());
                }
            }
            digest.update([u8::from(zone.mesh.curved)]);
        }
        println!(
            "case={} mesh_sha256={} requested_workers={workers}",
            path.display(),
            hex::encode(digest.finalize())
        );
        println!(
            "case={} elapsed_ms={} segments={} vias={} pins={} zones={} texts={} drawings={}",
            path.display(),
            complete_ms,
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
