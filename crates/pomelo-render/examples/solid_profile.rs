//! CPU import-to-shared-mesh measurement; all heights here are explicit preview assumptions.
//!
//! ```text
//! solid_profile <board> [max-workers]
//!     [--backend direct|kernel-ops|kernel-thread]   (needs --features kernel-3d for kernel-*)
//!     [--workers N] [--layer NAME] [--window x0,y0,x1,y1] [--timeout SECONDS]
//! ```
//!
//! `--layer` and `--window` (source millimetres) crop the board before preparation, for the
//! solidcore comparison's small inputs (docs/pcb-3d-solidcore-experiment-plan.md §7.1).
use pomelo_core::{
    geometry3d::solid::SolidLimits,
    model::{BoardScene, Bounds, Point},
    task::CancellationToken,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use pomelo_render::{
    preparation_memory::PreparationMemory,
    scene::board3d::{PreparedBoard3d, PreviewStackup, layer_subset},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

#[derive(Default)]
struct Args {
    path: Option<PathBuf>,
    workers: Option<usize>,
    backend: String,
    layer: Option<String>,
    window: Option<Bounds>,
    timeout: Option<f64>,
}

fn parse() -> anyhow::Result<Args> {
    let mut args = Args {
        backend: "direct".into(),
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .ok_or_else(|| anyhow::anyhow!("{a} needs a value"))
        };
        match a.as_str() {
            "--backend" => args.backend = value()?,
            "--workers" => args.workers = Some(value()?.parse()?),
            "--layer" => args.layer = Some(value()?),
            "--timeout" => args.timeout = Some(value()?.parse()?),
            "--window" => {
                let v: Vec<f64> = value()?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                anyhow::ensure!(v.len() == 4, "--window x0,y0,x1,y1");
                args.window = Some(Bounds {
                    min: Point::new(v[0].min(v[2]), v[1].min(v[3])),
                    max: Point::new(v[0].max(v[2]), v[1].max(v[3])),
                });
            }
            _ if args.path.is_none() => args.path = Some(PathBuf::from(a)),
            _ if args.workers.is_none() => args.workers = Some(a.parse()?),
            _ => anyhow::bail!("unexpected argument {a}"),
        }
    }
    Ok(args)
}

fn overlaps(b: &Bounds, w: &Bounds) -> bool {
    b.min.x <= w.max.x && b.max.x >= w.min.x && b.min.y <= w.max.y && b.max.y >= w.min.y
}

fn inside(p: Point, w: &Bounds) -> bool {
    p.x >= w.min.x && p.x <= w.max.x && p.y >= w.min.y && p.y <= w.max.y
}

fn crop(mut board: BoardScene, window: &Bounds) -> BoardScene {
    board
        .segments
        .retain(|s| s.bounds().is_some_and(|b| overlaps(&b, window)));
    board.zones.retain(|z| {
        z.mesh
            .ring_bounds
            .first()
            .is_some_and(|b| overlaps(b, window))
    });
    board.pins.retain(|p| inside(p.at, window));
    board.vias.retain(|v| inside(v.at, window));
    board
}

fn main() -> anyhow::Result<()> {
    let args = parse()?;
    let path = args.path.clone().ok_or_else(|| {
        anyhow::anyhow!("usage: solid_profile <board> [max-workers] [--backend ...] [--layer NAME] [--window x0,y0,x1,y1] [--timeout S]")
    })?;
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

    let mut scene = imported.scene;
    if let Some(name) = &args.layer {
        let id = scene
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
            .map(|l| l.id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no layer {name}; layers: {:?}",
                    scene.layers.iter().map(|l| &l.name).collect::<Vec<_>>()
                )
            })?;
        scene = Arc::new(layer_subset(&scene, &[id].into_iter().collect()));
    }
    if let Some(window) = &args.window {
        let all = scene.layers.iter().map(|l| l.id).collect();
        scene = Arc::new(crop(layer_subset(&scene, &all), window));
    }
    let objects = serde_json::json!({
        "segments": scene.segments.len(),
        "zones": scene.zones.len(),
        "pins": scene.pins.len(),
        "pads": scene.pins.iter().map(|p| p.pads.len()).sum::<usize>(),
        "vias": scene.vias.len(),
    });

    let preview = PreviewStackup::default();
    #[cfg(feature = "kernel-3d")]
    let kernel_stats = {
        use pomelo_render::scene::kernel3d::Backend3d;
        let backend = Backend3d::parse(&args.backend)
            .ok_or_else(|| anyhow::anyhow!("unknown backend {}", args.backend))?;
        backend.install(preview.chord_mm)
    };
    #[cfg(not(feature = "kernel-3d"))]
    anyhow::ensure!(
        args.backend == "direct",
        "backend {} needs --features kernel-3d",
        args.backend
    );

    if let Some(seconds) = args.timeout {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            cancel.cancel();
        });
    }
    let started = Instant::now();
    let result = if let Some(workers) = args.workers {
        PreparedBoard3d::preview_with_workers(
            &scene,
            preview,
            SolidLimits {
                max_allocation_bytes: 8 * 1024 * 1024 * 1024usize,
                ..Default::default()
            },
            &cancel,
            workers,
        )
    } else {
        PreparationMemory::new(pomelo_core::memory::MemoryBudget::new(0))
            .prepare_board3d(&scene, preview, &cancel)
    };
    let prepare_ms = started.elapsed().as_millis();
    let mut report = serde_json::json!({
        "path": path,
        "backend": args.backend,
        "layer": args.layer,
        "window": args.window.map(|w| [w.min.x, w.min.y, w.max.x, w.max.y]),
        "max_workers": args.workers,
        "available_parallelism": std::thread::available_parallelism().map_or(1, usize::from),
        "preview_stackup": true,
        "gpu_validated": false,
        "import_ms": import_ms,
        "prepare_ms": prepare_ms,
        "objects": objects,
    });
    match result {
        Ok(board) => {
            let extra = serde_json::json!({
                "completed": true,
                "vertices":board.vertex_count(),"indices":board.index_count(),"triangles":board.index_count()/3,"instances":board.instance_count(),
                "batches":board.pages.len(),"pages":board.pages.len(),"meshes":board.meshes.len(),"max_page_vertices":board.meshes.iter().map(|m|m.vertices.len()).max(),"max_page_indices":board.meshes.iter().map(|m|m.indices.len()).max(),"max_instance_count":board.pages.iter().map(|p|p.instances.len()).max(),"sources":board.sources.len(),"upload_bytes":board.upload_bytes(),"allocation_bytes":board.allocation_bytes(),"warnings":board.warning_count,
                "warning_samples":board.warnings.iter().take(8).map(ToString::to_string).collect::<Vec<_>>(),
            });
            report
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
        }
        Err(e) => {
            report["completed"] = false.into();
            report["error"] = e.to_string().into();
            report["timed_out"] = cancel.is_cancelled().into();
        }
    }
    #[cfg(feature = "kernel-3d")]
    if let Some(stats) = kernel_stats {
        report["kernel"] = stats.to_json();
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
