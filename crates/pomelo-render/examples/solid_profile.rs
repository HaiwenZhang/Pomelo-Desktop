//! CPU import-to-shared-mesh measurement; all heights here are explicit preview assumptions.
use pomelo_core::{geometry3d::solid::SolidLimits, task::CancellationToken};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use pomelo_render::{
    preparation_memory::PreparationMemory,
    scene::board3d::{PreparedBoard3d, PreviewStackup},
};
use std::{path::PathBuf, time::Instant};
fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("usage: solid_profile <board> [max-workers]"))?,
    );
    let cancel = CancellationToken::default();
    let started = Instant::now();
    let imported = FormatImporter.import(
        &path,
        &ImportOptions::default(),
        &ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        },
    )?;
    let import_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let workers = std::env::args()
        .nth(2)
        .map(|v| v.parse::<usize>())
        .transpose()?;
    let board = if let Some(workers) = workers {
        PreparedBoard3d::preview_with_workers(
            &imported.scene,
            PreviewStackup::default(),
            SolidLimits {
                max_allocation_bytes: 8 * 1024 * 1024 * 1024usize,
                ..Default::default()
            },
            &cancel,
            workers,
        )?
    } else {
        PreparationMemory::new(pomelo_core::memory::MemoryBudget::new(0)).prepare_board3d(
            &imported.scene,
            PreviewStackup::default(),
            &cancel,
        )?
    };
    let report = serde_json::json!({"path":path,"max_workers":workers,"available_parallelism":std::thread::available_parallelism().map_or(1,usize::from),"preview_stackup":true,"gpu_validated":false,"import_ms":import_ms,"prepare_ms":started.elapsed().as_millis(),
        "vertices":board.vertex_count(),"indices":board.index_count(),"triangles":board.index_count()/3,"instances":board.instance_count(),
        "batches":board.pages.len(),"pages":board.pages.len(),"meshes":board.meshes.len(),"max_page_vertices":board.meshes.iter().map(|m|m.vertices.len()).max(),"max_page_indices":board.meshes.iter().map(|m|m.indices.len()).max(),"max_instance_count":board.pages.iter().map(|p|p.instances.len()).max(),"sources":board.sources.len(),"upload_bytes":board.upload_bytes(),"allocation_bytes":board.allocation_bytes(),"warnings":board.warning_count,
        "warning_samples":board.warnings.iter().take(8).map(ToString::to_string).collect::<Vec<_>>()});
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
