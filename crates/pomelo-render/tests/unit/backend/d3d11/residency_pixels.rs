//! Hardware proof of visible chunk residency with original source draw order.
use super::*;

fn real_case_copper_source(name: &str) {
    use crate::{
        backend::{CopperRenderer, CopperTelemetry},
        copper::PreparedCopper,
        preparation_memory::PreparationMemory,
    };
    use pomelo_import::{
        BoardImporter, ImportContext, ImportOptions, TextEncoding,
        formats::allegro::AllegroImporter,
    };
    let root = std::env::var_os("POMELO_LARGE_BOARD_CASES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/PCB/brd_cases".into());
    let path = if name == "ARM_Test.mcm" {
        std::env::var_os("POMELO_LARGE_MCM_CASE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/PCB/mcm/ARM_Test.mcm".into())
    } else {
        root.join(name)
    };
    let cancel = CancellationToken::default();
    let board = AllegroImporter
        .import(
            &path,
            &ImportOptions {
                text_encoding: TextEncoding::Windows1252,
                ..Default::default()
            },
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let mut memory = PreparationMemory::new(pomelo_core::memory::MemoryBudget::new(0));
    let mut frame = compositor_parity::fixture().traces;
    frame.bounds = board.scene.bounds;
    frame.colors = Arc::new(BTreeMap::new());
    frame.fallback_color = [1.0, 0.0, 0.0, 1.0];
    let bounds = board.scene.bounds;
    let fit = Camera {
        center: Point::new(
            (bounds.min.x + bounds.max.x) / 2.0,
            (bounds.min.y + bounds.max.y) / 2.0,
        ),
        pixels_per_mm: 100.0 / (bounds.max.x - bounds.min.x).max(bounds.max.y - bounds.min.y),
        flipped: false,
    };
    let mut views = vec![fit];
    for zone in board
        .scene
        .zones
        .iter()
        .filter_map(|zone| zone.mesh.ring_bounds.first())
        .take(2)
    {
        views.push(Camera {
            center: Point::new(
                (zone.min.x + zone.max.x) / 2.0,
                (zone.min.y + zone.max.y) / 2.0,
            ),
            pixels_per_mm: 20.0,
            flipped: views.len() == 2,
        });
    }
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut expected = Vec::new();
    for source_backed in [false, true] {
        let source = Arc::new(
            memory
                .prepare_copper(|limits| {
                    if source_backed {
                        PreparedCopper::build_scene(Arc::clone(&board.scene), limits, &cancel)
                    } else {
                        PreparedCopper::build(&board.scene.zones, limits, &cancel)
                    }
                })
                .unwrap(),
        );
        let stats = Arc::new(CopperTelemetry::default());
        let mut renderer = CopperRenderer::new(Arc::clone(&stats));
        let started = std::time::Instant::now();
        let mut callbacks = 0;
        for _ in 0..2500 {
            let before = stats.snapshot().uploaded_bytes;
            renderer.prepare(&gpu, &source, true).unwrap();
            callbacks += 1;
            assert!(stats.snapshot().uploaded_bytes - before <= 4 * 1024 * 1024);
            if stats.snapshot().uploaded_bytes == source.upload_bytes() as u64 {
                break;
            }
        }
        assert_eq!(
            stats.snapshot().uploaded_bytes,
            source.upload_bytes() as u64
        );
        let upload_ms = started.elapsed().as_millis();
        for (index, camera) in views.iter().enumerate() {
            frame.camera = Some(*camera);
            target.bind(&context);
            renderer.draw(&gpu, &frame, &source, 0.35, false).unwrap();
            let pixels = target.read(&context);
            if source_backed {
                assert_eq!(pixels, expected[index], "{name} copper view={index}");
            } else {
                if index == 0 {
                    assert!(
                        pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|pixel| pixel[..3].iter().any(|value| *value != 0))
                    );
                }
                expected.push(pixels);
            }
        }
        let uploaded = stats.snapshot().uploaded_bytes;
        target.bind(&context);
        renderer.draw(&gpu, &frame, &source, 0.35, true).unwrap();
        assert_eq!(stats.snapshot().uploaded_bytes, uploaded);
        assert_eq!(target.read(&context), *expected.last().unwrap());
        eprintln!(
            "case={name} source_backed={source_backed} cpu_resident_bytes={} uploaded_bytes={uploaded} upload_ms={upload_ms} callbacks={callbacks} eager_pixels_equal=true stable_reupload=false",
            source.allocation_bytes()
        );
    }
}

#[test]
#[ignore = "requires local 15061 board and hardware D3D11"]
fn hardware_15061_copper_source_matches_eager() {
    real_case_copper_source("15061-1b.brd");
}

#[test]
#[ignore = "requires local ntpcb board and hardware D3D11"]
fn hardware_ntpcb_copper_source_matches_eager() {
    real_case_copper_source("ntpcb_320mb.brd");
}

#[test]
#[ignore = "requires local ARM_Test.mcm and hardware D3D11"]
fn hardware_arm_mcm_copper_source_matches_eager() {
    real_case_copper_source("ARM_Test.mcm");
}

fn real_case_trace_residency(name: &str) {
    use crate::preparation_memory::PreparationMemory;
    use pomelo_import::{
        BoardImporter, ImportContext, ImportOptions, TextEncoding,
        formats::allegro::AllegroImporter,
    };
    let root = std::env::var_os("POMELO_LARGE_BOARD_CASES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/PCB/brd_cases".into());
    let cancel = CancellationToken::default();
    let started = std::time::Instant::now();
    let board = AllegroImporter
        .import(
            &root.join(name),
            &ImportOptions {
                text_encoding: TextEncoding::Windows1252,
                ..Default::default()
            },
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let mut memory = PreparationMemory::new(pomelo_core::memory::MemoryBudget::new(0));
    let sources = [
        (
            "tracks",
            memory
                .prepare_tracks(|limits| {
                    PreparedTracks::build_with_outline(
                        &board.scene.segments,
                        &board.scene.outline,
                        limits,
                        &cancel,
                    )
                })
                .unwrap(),
        ),
        (
            "zone_outlines",
            memory
                .prepare_tracks(|limits| {
                    PreparedTracks::build_zone_outlines(&board.scene.zones, limits, &cancel)
                })
                .unwrap(),
        ),
    ];
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    for (kind, source) in sources {
        assert!(!source.instances.is_empty());
        let points = [
            source.instances.first().unwrap().a,
            source.instances.last().unwrap().a,
        ];
        let mut frame = compositor_parity::fixture().traces;
        frame.bounds = board.scene.bounds;
        frame.tracks = Arc::new(source);
        frame.colors = Arc::new(BTreeMap::new());
        frame.fallback_color = [1.0, 0.0, 0.0, 1.0];
        frame.opacity = 0.35;
        let stats = Arc::new(TraceTelemetry::default());
        let baseline_stats = Arc::new(TraceTelemetry::default());
        let mut cached = TraceRenderer::<PreparedTracks>::new(Arc::clone(&stats))
            .with_residency(64 * 1024 * 1024);
        let mut eager = TraceRenderer::<PreparedTracks>::new(Arc::clone(&baseline_stats));
        for (view_index, point) in [points[0], points[1], points[0]].into_iter().enumerate() {
            frame.camera = Some(Camera {
                center: Point::new(
                    f64::from(point[0]) + f64::from(point[2]),
                    f64::from(point[1]) + f64::from(point[3]),
                ),
                pixels_per_mm: 20.0,
                flipped: view_index == 1,
            });
            let eager_started = std::time::Instant::now();
            for _ in 0..1000 {
                target.bind(&context);
                eager.draw(&gpu, &frame).unwrap();
                if baseline_stats.snapshot().visible_ready {
                    break;
                }
            }
            assert!(baseline_stats.snapshot().visible_ready);
            let expected = target.read(&context);
            let eager_ms = eager_started.elapsed().as_millis();
            let cached_started = std::time::Instant::now();
            let mut callbacks = 0;
            for _ in 0..1000 {
                let before = stats.snapshot().uploaded_bytes;
                target.bind(&context);
                cached.draw(&gpu, &frame).unwrap();
                callbacks += 1;
                assert!(stats.snapshot().uploaded_bytes - before <= 4 * 1024 * 1024);
                if stats.snapshot().visible_ready {
                    break;
                }
            }
            assert!(stats.snapshot().visible_ready);
            assert_eq!(
                target.read(&context),
                expected,
                "{name} {kind} view={view_index}"
            );
            let cached_ms = cached_started.elapsed().as_millis();
            let before = stats.snapshot().uploaded_bytes;
            target.bind(&context);
            cached.draw(&gpu, &frame).unwrap();
            assert_eq!(stats.snapshot().uploaded_bytes, before);
            assert_eq!(target.read(&context), expected);
            eprintln!(
                "case={name} source={kind} view={view_index} source_instances={} resident_instances={} lifetime_upload_bytes={} full_upload_bytes={} cached_ready_ms={cached_ms} eager_ready_ms={eager_ms} callbacks={callbacks} eager_pixels_equal=true stable_upload=false",
                frame.tracks.instances.len(),
                stats.snapshot().uploaded_instances,
                stats.snapshot().uploaded_bytes,
                baseline_stats.snapshot().uploaded_bytes
            );
        }
    }
    eprintln!(
        "case={name} trace_residency_total_ms={} gpu=hardware_d3d11 copper_fill_verified=false",
        started.elapsed().as_millis()
    );
}

#[test]
#[ignore = "requires local 15061-1b.brd and hardware D3D11"]
fn hardware_15061_trace_residency_matches_eager() {
    real_case_trace_residency("15061-1b.brd");
}

#[test]
#[ignore = "requires local ntpcb_320mb.brd and hardware D3D11"]
fn hardware_ntpcb_trace_residency_matches_eager() {
    real_case_trace_residency("ntpcb_320mb.brd");
}

#[test]
#[ignore = "requires hardware D3D11"]
fn hardware_visible_trace_chunks_pan_evict_and_match_eager_pixels() {
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let cancel = CancellationToken::default();
    let count = 4096;
    let segments: Vec<_> = (0..count * 3)
        .map(|index| {
            let group = index / count;
            let x = if group == 1 { 100.0 } else { 0.0 };
            Segment {
                id: ObjectId(index as u32),
                track_id: ObjectId(group as u32 + 1),
                layer: LayerId(0),
                net: NetId(group as u32 + 1),
                a: Point::new(x - 3.0, 0.0),
                b: Point::new(x + 3.0, 0.0),
                width: 0.2,
                arc: None,
                bond_wire: None,
            }
        })
        .collect();
    let source =
        Arc::new(PreparedTracks::build(&segments, TraceLimits::default(), &cancel).unwrap());
    let mut frame = compositor_parity::fixture().traces;
    frame.tracks = source;
    frame.camera = Some(Camera {
        center: Point::default(),
        pixels_per_mm: 10.0,
        flipped: false,
    });
    frame.opacity = 0.35;
    frame.color_mode = pomelo_core::display::ColorMode::Net;
    let stats = Arc::new(TraceTelemetry::default());
    let mut cached = TraceRenderer::<PreparedTracks>::new(Arc::clone(&stats))
        .with_residency(count * size_of::<crate::tracks::TraceInstance>());
    let mut eager = TraceRenderer::<PreparedTracks>::new(Arc::new(TraceTelemetry::default()));

    for center in [0.0, 100.0, 0.0] {
        frame.camera.as_mut().unwrap().center.x = center;
        target.bind(&context);
        eager.draw(&gpu, &frame).unwrap();
        let expected = target.read(&context);
        target.bind(&context);
        cached.draw(&gpu, &frame).unwrap();
        assert_eq!(
            target.read(&context),
            expected,
            "source order/coverage changed at center={center}"
        );
        let snapshot = stats.snapshot();
        assert!(snapshot.visible_ready);
        assert_eq!(
            snapshot.uploaded_instances,
            if center == 100.0 { count } else { count * 2 } as u64
        );
        let uploaded = snapshot.uploaded_bytes;
        target.bind(&context);
        cached.draw(&gpu, &frame).unwrap();
        assert_eq!(
            stats.snapshot().uploaded_bytes,
            uploaded,
            "stable view uploaded again"
        );
        assert_eq!(target.read(&context), expected);
    }
    assert_eq!(
        stats.snapshot().uploaded_bytes,
        (count * 5 * size_of::<crate::tracks::TraceInstance>()) as u64
    );
    cached.reset();
    target.bind(&context);
    cached.draw(&gpu, &frame).unwrap();
    assert!(stats.snapshot().visible_ready);
    assert_eq!(stats.snapshot().uploaded_instances, (count * 2) as u64);
    println!(
        "VISIBLE_TRACE_RESIDENCY_HARDWARE source_instances={} first_visible={} far_visible={} return_reuploads=true stable_reuploads=false eager_pixels_equal=true reset_verified=true",
        count * 3,
        count * 2,
        count
    );
}
