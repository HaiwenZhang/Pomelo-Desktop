//! Opt-in real-source subset proof. No GPUI window or preferences are opened.
use super::compositor_parity::{device, fixture, gpu_context};
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    appearance::{ColorTarget, RgbColor},
    interaction::Camera,
    model::{Pin, Via, Zone, ZoneKind},
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde_json::json;

const SHA: &str = "09c3e6f170ead979f4f8537ab27bc50f04021855b2c8cde117e5b4935aa393c7";
const TOP: LayerId = LayerId(0);
const SHAPE: f32 = 99.0 / 255.0;

struct SourceSubset {
    zones: Vec<Zone>,
    segments: Vec<Segment>,
    pins: Vec<Pin>,
    vias: Vec<Via>,
    source: serde_json::Value,
}

fn import_subset() -> SourceSubset {
    let path = std::env::var_os("POMELO_S5000C_U94_BOARD_PATH")
        .expect("explicit POMELO_S5000C_U94_BOARD_PATH required");
    if let Some(report) = std::env::var_os("POMELO_S5000C_U94_GPU_REPORT") {
        let original = std::fs::canonicalize(&path).unwrap();
        for output in [
            std::path::PathBuf::from(&report),
            std::path::PathBuf::from(&report).with_extension("png"),
        ] {
            assert_ne!(
                std::fs::canonicalize(output).ok().as_ref(),
                Some(&original),
                "evidence output must never overwrite the BRD source"
            );
        }
    }
    let cancel = CancellationToken::default();
    let board = AllegroImporter
        .import(
            std::path::Path::new(&path),
            &ImportOptions {
                text_encoding: TextEncoding::Windows1252,
                max_file_bytes: 512 * 1024 * 1024,
            },
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let sha: String = board
        .identity
        .sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(sha, SHA, "fixture must be the measured, unmodified source");
    let zones: Vec<_> = board
        .scene
        .zones
        .iter()
        .filter(|z| matches!(z.id.0, 933674 | 5598600))
        .cloned()
        .collect();
    assert_eq!(zones.len(), 2);
    let static_zone = zones.iter().find(|z| z.id.0 == 933674).unwrap();
    let dynamic_zone = zones.iter().find(|z| z.id.0 == 5598600).unwrap();
    assert_eq!(static_zone.kind, ZoneKind::Static);
    assert_eq!(dynamic_zone.kind, ZoneKind::Dynamic);
    assert_eq!(static_zone.paths.len(), 1);
    assert_eq!(
        dynamic_zone.paths.iter().map(Vec::len).collect::<Vec<_>>(),
        [23, 8]
    );
    assert_eq!(dynamic_zone.mesh.ring_bounds.len(), 2);
    assert!(zones.iter().all(|z| z.layer == TOP));
    SourceSubset {
        zones,
        segments: [4242052, 4241766]
            .map(|id| {
                board
                    .scene
                    .segments
                    .iter()
                    .find(|s| s.id.0 == id)
                    .unwrap()
                    .clone()
            })
            .into(),
        pins: [834575, 834394, 901969]
            .map(|id| {
                board
                    .scene
                    .pins
                    .iter()
                    .find(|p| p.id.0 == id)
                    .unwrap()
                    .clone()
            })
            .into(),
        vias: [1332220, 1656947]
            .map(|id| {
                board
                    .scene
                    .vias
                    .iter()
                    .find(|v| v.id.0 == id)
                    .unwrap()
                    .clone()
            })
            .into(),
        source: json!({"sha256":sha,"path":path.to_string_lossy(),
            "bytes":std::fs::metadata(&path).unwrap().len(),"encoding":board.identity.encoding,
            "layout_version":board.source.layout_version,
            "full_scene_counts":{"zones":board.scene.zones.len(),"segments":board.scene.segments.len(),
                "pins":board.scene.pins.len(),"vias":board.scene.vias.len()},
            "import_file_limit_bytes":512_u64*1024*1024,
            "source_indices":{
                "zones":board.scene.zones.iter().enumerate().filter(|(_,z)|matches!(z.id.0,933674|5598600))
                    .map(|(index,z)|json!({"id":z.id.0,"source_index":index})).collect::<Vec<_>>(),
                "segments":board.scene.segments.iter().enumerate().filter(|(_,s)|matches!(s.id.0,4242052|4241766))
                    .map(|(index,s)|json!({"id":s.id.0,"source_index":index})).collect::<Vec<_>>(),
                "pins":board.scene.pins.iter().enumerate().filter(|(_,p)|matches!(p.id.0,834575|834394|901969))
                    .map(|(index,p)|json!({"id":p.id.0,"source_index":index})).collect::<Vec<_>>(),
                "vias":board.scene.vias.iter().enumerate().filter(|(_,v)|matches!(v.id.0,1332220|1656947))
                    .map(|(index,v)|json!({"id":v.id.0,"source_index":index})).collect::<Vec<_>>()},
            "zone_order":"original whole-scene relative order preserved; preparation may group by layer"}),
    }
    // The whole imported scene drops here, before any GPU buffer allocation.
}

fn frame(source: &SourceSubset) -> BoardFrame {
    let cancel = CancellationToken::default();
    let trace_limits = TraceLimits {
        max_instances: 4096,
        max_bytes: 1024 * 1024,
    };
    let pad_limits = crate::pads::PadLimits {
        max_pads: 4096,
        max_bytes: 1024 * 1024,
    };
    let mut frame = fixture();
    frame.traces.tracks =
        Arc::new(PreparedTracks::build(&source.segments, trace_limits, &cancel).unwrap());
    frame.traces.bounds = Bounds {
        min: Point::new(14.5, 72.0),
        max: Point::new(18.5, 76.5),
    };
    frame.traces.camera = Some(Camera {
        center: Point::new(16.6, 74.2),
        pixels_per_mm: 31.0,
        flipped: false,
    });
    frame.traces.colors = Arc::new(BTreeMap::from([(TOP, [0.0, 1.0, 0.0, 1.0])]));
    frame.copper = Arc::new(
        crate::copper::PreparedCopper::build(
            &source.zones,
            crate::copper::CopperLimits {
                max_vertices: 100_000,
                max_indices: 300_000,
                max_zones: 2,
                max_bytes: 8 * 1024 * 1024,
            },
            &cancel,
        )
        .unwrap(),
    );
    frame.zone_outlines = Some(Arc::new(
        PreparedTracks::build_zone_outlines(&source.zones, trace_limits, &cancel).unwrap(),
    ));
    frame.pads = Some(Arc::new(
        crate::pads::PreparedPads::build(&source.pins, &source.vias, pad_limits, &cancel).unwrap(),
    ));
    frame.drills = Some(Arc::new(
        crate::drills::PreparedDrills::build(&source.pins, &source.vias, pad_limits, &cancel)
            .unwrap()
            .geometry,
    ));
    frame.layer_order = Arc::new(vec![TOP]);
    frame.copper_opacity = SHAPE;
    let display = Arc::make_mut(&mut frame.display);
    display.copper_opacity = SHAPE;
    display.hidden_layers.extend((1..14).map(LayerId));
    display.show_texts = false;
    display.show_drawings = false;
    display.show_backdrills = false;
    for (target, color) in [
        (ColorTarget::Etch(TOP), [0, 255, 0]),
        (ColorTarget::Pin(TOP), [0, 0, 255]),
        (ColorTarget::Via(TOP), [255, 255, 0]),
        (ColorTarget::Drill, [172; 3]),
    ] {
        display.appearance.set_color(target, Some(RgbColor(color)));
    }
    frame
}

fn over(dst: [u8; 3], src: [u8; 3], alpha: u8) -> [u8; 3] {
    std::array::from_fn(|i| {
        ((f64::from(src[i]) * f64::from(alpha) + f64::from(dst[i]) * f64::from(255 - alpha))
            / 255.0)
            .round() as u8
    })
}

fn close(actual: [u8; 3], expected: [u8; 3], message: &str) {
    assert!(
        actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2),
        "{message}: actual={actual:?} expected={expected:?}"
    );
}

fn rectangle_distance(p: Point, center: Point, half: Point) -> f64 {
    let x = (p.x - center.x).abs() - half.x;
    let y = (p.y - center.y).abs() - half.y;
    x.max(0.0).hypot(y.max(0.0)) + x.max(y).min(0.0)
}

fn segment_distance(p: Point, s: &Segment) -> f64 {
    let dx = s.b.x - s.a.x;
    let dy = s.b.y - s.a.y;
    let t = (((p.x - s.a.x) * dx + (p.y - s.a.y) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (p.x - s.a.x - t * dx).hypot(p.y - s.a.y - t * dy) - s.width * 0.5
}

fn dot(x: u32, y: u32) -> bool {
    matches!((x & 15, y & 15), (3 | 8, 3 | 14) | (0 | 11, 6 | 11))
}

fn rgb(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    super::rgb(pixels, x as usize, y as usize)
}

#[test]
#[ignore = "requires explicit S5000C source path and Windows hardware D3D11; local source subset only"]
fn hardware_s5000c_u94_real_void_and_same_layer_composition_without_reupload() {
    let source = import_subset();
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let (traces, copper, pads, drills, outlines) = (
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    );
    let mut renderer = BoardRenderer::new(
        Arc::clone(&traces),
        Arc::clone(&copper),
        Arc::clone(&pads),
        Arc::new(CopperTelemetry::default()),
        Arc::clone(&drills),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    )
    .with_zone_outline_telemetry(Arc::clone(&outlines));
    let mut frame = frame(&source);
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let initial_uploads = [
        copper.snapshot().lifetime_uploaded_bytes,
        traces.snapshot().uploaded_bytes,
        pads.snapshot().uploaded_bytes,
        drills.snapshot().uploaded_bytes,
        outlines.snapshot().uploaded_bytes,
    ];
    assert_eq!(initial_uploads[0] as usize, frame.copper.upload_bytes());
    let mut results = Vec::new();
    for solid in [false, true] {
        Arc::make_mut(&mut frame.display).static_shapes_fill_solid = solid;
        Arc::make_mut(&mut frame.display).global_opacity = 0.0;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let base = target.read(&context);
        let mut overlap = 0;
        let camera = frame.traces.camera.unwrap();
        for y in 0..SIDE {
            for x in 0..SIDE {
                let p = camera.view_to_board(
                    Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5),
                    128.0,
                    128.0,
                );
                // Interior rectangle is independently measured from the real contours;
                // stay away from every outer edge, pad and short connection.
                if (15.65..16.45).contains(&p.x) && (73.2..74.0).contains(&p.y) {
                    let expected = if solid || dot(x, y) {
                        over([0, 99, 0], [0, 255, 0], 99)
                    } else {
                        [0, 99, 0]
                    };
                    close(rgb(&base, x, y), expected, "two actual zone interiors");
                    overlap += 1;
                }
            }
        }
        assert!(overlap > 500);
        for global in [128_u8, 255] {
            Arc::make_mut(&mut frame.display).global_opacity = f32::from(global) / 255.0;
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
            let pixels = target.read(&context);
            let (mut pin_count, mut trace_count, mut via_count, mut drill_count) = (0, 0, 0, 0);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let p = camera.view_to_board(
                        Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5),
                        128.0,
                        128.0,
                    );
                    let pin =
                        rectangle_distance(p, source.pins[1].at, Point::new(0.225044, 0.149987));
                    let via = source
                        .vias
                        .iter()
                        .map(|v| (p.x - v.at.x).hypot(p.y - v.at.y))
                        .fold(f64::INFINITY, f64::min);
                    let trace = source
                        .segments
                        .iter()
                        .map(|s| segment_distance(p, s))
                        .fold(f64::INFINITY, f64::min);
                    let underlying = rgb(&base, x, y);
                    // 1.25 physical pixels from AA boundaries; the short stub has
                    // few valid interior samples, so counts are recorded explicitly.
                    let margin = 1.25 / 31.0;
                    if pin < -margin && via > 0.2286 + margin && trace > margin {
                        close(
                            rgb(&pixels, x, y),
                            over(underlying, [0, 0, 255], global),
                            "actual U94.40 pad over copper",
                        );
                        pin_count += 1;
                    }
                    if trace < -margin && via > 0.2286 + margin && pin > margin {
                        close(
                            rgb(&pixels, x, y),
                            over(underlying, [0, 255, 0], global),
                            "actual trace over copper",
                        );
                        trace_count += 1;
                    }
                    if via < 0.2286 - margin && via > 0.127 + margin && pin > margin {
                        let underneath = if trace < -margin {
                            over(underlying, [0, 255, 0], global)
                        } else if trace > margin {
                            underlying
                        } else {
                            continue;
                        };
                        close(
                            rgb(&pixels, x, y),
                            over(underneath, [255, 255, 0], global),
                            "actual via annulus over trace/copper",
                        );
                        via_count += 1;
                    }
                    if via < 0.127 - margin && pin > margin && trace < -margin {
                        let expected = over(
                            over(over(underlying, [0, 255, 0], global), [255, 255, 0], global),
                            [172; 3],
                            global,
                        );
                        close(
                            rgb(&pixels, x, y),
                            expected,
                            "actual plated drill over via/trace/copper",
                        );
                        drill_count += 1;
                    }
                }
            }
            assert!(
                pin_count > 20 && trace_count > 0 && via_count > 0 && drill_count > 10,
                "insufficient independent interiors: pin={pin_count} trace={trace_count} via={via_count} drill={drill_count}"
            );
            results.push(
                json!({"camera":"overview","static_solid":solid,"global":global,
                "zone_overlap_samples":overlap,"pin_samples":pin_count,"trace_samples":trace_count,
                "via_annulus_samples":via_count,"drill_samples":drill_count}),
            );
        }
    }
    frame.traces.camera = Some(Camera {
        center: source.pins[2].at,
        pixels_per_mm: 100.0,
        flipped: false,
    });
    Arc::make_mut(&mut frame.display).static_shapes_fill_solid = false;
    let mut hole_counts = Vec::new();
    for global in [0_u8, 128, 255] {
        Arc::make_mut(&mut frame.display).global_opacity = f32::from(global) / 255.0;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let pixels = target.read(&context);
        let camera = frame.traces.camera.unwrap();
        let (mut gaps, mut pin) = (0, 0);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let p = camera.view_to_board(
                    Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5),
                    128.0,
                    128.0,
                );
                let relative = Point::new(p.x - 16.9801794, p.y - 75.1774214);
                // Independent analytic rounded rectangular void, from the source's
                // four arcs and four lines. This does not consult GPU triangulation.
                let hole = rectangle_distance(
                    relative,
                    Point::default(),
                    Point::new(0.3520948 - 0.1270508, 0.2770378 - 0.1270508),
                ) - 0.1270508;
                let pad = rectangle_distance(p, source.pins[2].at, Point::new(0.225044, 0.149987));
                if hole < -0.035 && pad > 0.035 {
                    assert_eq!(
                        rgb(&pixels, x, y),
                        [0; 3],
                        "actual closed void gap ({x},{y}) global={global}"
                    );
                    gaps += 1;
                }
                if pad < -0.035 {
                    close(
                        rgb(&pixels, x, y),
                        over([0; 3], [0, 0, 255], global),
                        "actual U94.41 inside copper void",
                    );
                    pin += 1;
                }
            }
        }
        assert!(gaps > 500 && pin > 500);
        hole_counts.push(
            json!({"global":global,"black_void_gap_samples":gaps,"pin_inside_void_samples":pin}),
        );
        if global == 255
            && let Some(path) = std::env::var_os("POMELO_S5000C_U94_GPU_REPORT")
        {
            image::save_buffer(
                std::path::Path::new(&path).with_extension("png"),
                &pixels,
                SIDE,
                SIDE,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    // A non-black prior target distinguishes actual void coverage from painting
    // a black polygon over solid copper. No new source geometry is introduced.
    let sentinel = [51_u8, 77, 179];
    target.bind(&context);
    // SAFETY: the offscreen RTV and immediate context belong to the live device.
    unsafe {
        context.ClearRenderTargetView(
            &target.view,
            &[
                f32::from(sentinel[0]) / 255.0,
                f32::from(sentinel[1]) / 255.0,
                f32::from(sentinel[2]) / 255.0,
                1.0,
            ],
        );
    }
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    let camera = frame.traces.camera.unwrap();
    let (mut sentinel_gaps, mut sentinel_pin) = (0, 0);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let p = camera.view_to_board(
                Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5),
                128.0,
                128.0,
            );
            let relative = Point::new(p.x - 16.9801794, p.y - 75.1774214);
            let hole = rectangle_distance(
                relative,
                Point::default(),
                Point::new(0.3520948 - 0.1270508, 0.2770378 - 0.1270508),
            ) - 0.1270508;
            let pad = rectangle_distance(p, source.pins[2].at, Point::new(0.225044, 0.149987));
            if hole < -0.035 && pad > 0.035 {
                assert_eq!(
                    rgb(&pixels, x, y),
                    sentinel,
                    "real void must preserve pre-existing RTV color ({x},{y})"
                );
                sentinel_gaps += 1;
            }
            if pad < -0.035 {
                assert_eq!(
                    rgb(&pixels, x, y),
                    [0, 0, 255],
                    "opaque Pin41 over prior target color"
                );
                sentinel_pin += 1;
            }
        }
    }
    assert_eq!((sentinel_gaps, sentinel_pin), (1132, 836));
    if let Some(path) = std::env::var_os("POMELO_S5000C_U94_GPU_REPORT") {
        let path = std::path::Path::new(&path);
        let output = path.with_file_name(format!(
            "{}-sentinel.png",
            path.file_stem().unwrap().to_string_lossy()
        ));
        image::save_buffer(output, &pixels, SIDE, SIDE, image::ColorType::Rgba8).unwrap();
    }
    let final_uploads = [
        copper.snapshot().lifetime_uploaded_bytes,
        traces.snapshot().uploaded_bytes,
        pads.snapshot().uploaded_bytes,
        drills.snapshot().uploaded_bytes,
        outlines.snapshot().uploaded_bytes,
    ];
    assert_eq!(
        initial_uploads, final_uploads,
        "alpha, static-fill switch and camera must not upload geometry again"
    );
    let report = json!({"source":source.source,"scope":"full CPU import followed by nine original objects only on GPU",
        "whole_board_gpu_validated":false,"allegro_visual_parity_validated":false,
        "source_ids":{"zones":[933674,5598600],"segments":[4242052,4241766],"pins":[834575,834394,901969],"vias":[1332220,1656947]},
        "contours":source.zones.iter().map(|z|json!({"id":z.id.0,"kind":z.kind,"path_edges":z.paths.iter().map(Vec::len).collect::<Vec<_>>(),"curved":z.mesh.curved,"vertices":z.mesh.vertices.len(),"indices":z.mesh.indices.len()})).collect::<Vec<_>>(),
        "parameters":{"viewport_physical_px":[128,128],"dpi":1,"filled":true,"shape":99,
            "top_only":true,"etch":true,"pin":true,"via":true,"drill":true,"texts":false,"drawings":false,
            "labels_prepared":false,"camera_overview":{"center":[16.6,74.2],"px_per_mm":31},
            "camera_detail":{"center":[16.9801794,75.1774214],"px_per_mm":100},
            "colors":{"etch":[0,255,0],"pin":[0,0,255],"via":[255,255,0],"drill":[172,172,172]},
            "rgb_matching_allegro_required":false},"overview":results,"closed_void":hole_counts,
        "sentinel_void":{"prior_rtv_rgb":sentinel,"global":255,"retained_prior_color_samples":sentinel_gaps,
            "opaque_pin41_samples":sentinel_pin,"proves_void_is_not_black_overlay":true},
        "uploads_before":initial_uploads,"uploads_after":final_uploads,"copper_statistics":copper.snapshot()});
    if let Some(path) = std::env::var_os("POMELO_S5000C_U94_GPU_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    eprintln!(
        "S5000C_U94_REAL_SOURCE {}",
        serde_json::to_string(&report).unwrap()
    );
}
