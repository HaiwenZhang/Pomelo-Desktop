//! Opt-in entire source geometry upload; no window, fonts or presentation proof.
use super::compositor_parity::{device, fixture, gpu_context};
use super::*;
use crate::backend::d3d11::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    appearance::{ColorTarget, RgbColor},
    interaction::Camera,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

const SHA: &str = "09c3e6f170ead979f4f8537ab27bc50f04021855b2c8cde117e5b4935aa393c7";
const BUDGET: u64 = 4 * 1024 * 1024;

#[derive(Default)]
struct Telemetry {
    traces: Arc<TraceTelemetry>,
    copper: Arc<CopperTelemetry>,
    pads: Arc<TraceTelemetry>,
    custom: Arc<CopperTelemetry>,
    custom_edges: Arc<TraceTelemetry>,
    drills: Arc<TraceTelemetry>,
    zone_edges: Arc<TraceTelemetry>,
    drawings: Arc<TraceTelemetry>,
}
impl Telemetry {
    fn renderer(&self) -> BoardRenderer {
        BoardRenderer::new(
            Arc::clone(&self.traces),
            Arc::clone(&self.copper),
            Arc::clone(&self.pads),
            Arc::clone(&self.custom),
            Arc::clone(&self.drills),
            Arc::clone(&self.drawings),
            Arc::new(TraceTelemetry::default()),
        )
        .with_custom_outline_telemetry(Arc::clone(&self.custom_edges))
        .with_zone_outline_telemetry(Arc::clone(&self.zone_edges))
    }
    fn bytes(&self) -> [u64; 8] {
        [
            self.traces.snapshot().uploaded_bytes,
            self.copper.snapshot().lifetime_uploaded_bytes,
            self.pads.snapshot().uploaded_bytes,
            self.custom.snapshot().lifetime_uploaded_bytes,
            self.custom_edges.snapshot().uploaded_bytes,
            self.drills.snapshot().uploaded_bytes,
            self.zone_edges.snapshot().uploaded_bytes,
            self.drawings.snapshot().uploaded_bytes,
        ]
    }
}

fn draw_checked(
    renderer: &mut BoardRenderer,
    gpu: &NativeGpuContext<'_>,
    frame: &BoardFrame,
    target: &Target,
    telemetry: &Telemetry,
) -> u64 {
    let before = telemetry.bytes();
    target.bind(gpu.context);
    renderer.draw(gpu, frame).unwrap();
    let after = telemetry.bytes();
    let bytes = after.iter().zip(before).map(|(a, b)| a - b).sum();
    assert!(
        bytes <= BUDGET,
        "combined geometry upload exceeds4MiB: {before:?} -> {after:?}"
    );
    bytes
}

fn hole_distance(p: Point) -> f64 {
    let x = (p.x - 16.9801794).abs() - (0.3520948 - 0.1270508);
    let y = (p.y - 75.1774214).abs() - (0.2770378 - 0.1270508);
    x.max(0.0).hypot(y.max(0.0)) + x.max(y).min(0.0) - 0.1270508
}
fn pin_distance(p: Point) -> f64 {
    let x = (p.x - 16.9801794).abs() - 0.225044;
    let y = (p.y - 75.1774214).abs() - 0.149987;
    x.max(0.0).hypot(y.max(0.0)) + x.max(y).min(0.0)
}

#[test]
#[ignore = "explicit S5000C source and Windows hardware D3D11; all geometry only, not GPUI/text/Allegro acceptance"]
fn hardware_s5000c_whole_scene_geometry_upload_budget_cache_and_real_void() {
    let path = PathBuf::from(
        std::env::var_os("POMELO_S5000C_FULL_BOARD_PATH").expect("explicit source required"),
    );
    let report_path = PathBuf::from(
        std::env::var_os("POMELO_S5000C_FULL_GPU_REPORT")
            .expect("explicit absolute report path required"),
    );
    assert!(report_path.is_absolute());
    let png = report_path.with_extension("png");
    let fit_png = report_path.with_file_name(format!(
        "{}-fit.png",
        report_path.file_stem().unwrap().to_string_lossy()
    ));
    let original = std::fs::canonicalize(&path).unwrap();
    for output in [&report_path, &png, &fit_png] {
        assert_ne!(
            std::fs::canonicalize(output).ok().as_ref(),
            Some(&original),
            "never overwrite source board"
        );
    }
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
    let sha: String = board
        .identity
        .sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(sha, SHA);
    let scene = board.scene;
    let mut frame = fixture();
    frame.traces.tracks = Arc::new(
        PreparedTracks::build_with_outline(
            &scene.segments,
            &scene.outline,
            TraceLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    frame.drawings = Some(Arc::new(
        PreparedTracks::build_drawings(&scene.drawings, TraceLimits::default(), &cancel).unwrap(),
    ));
    frame.zone_outlines = Some(Arc::new(
        PreparedTracks::build_zone_outlines(&scene.zones, TraceLimits::default(), &cancel).unwrap(),
    ));
    frame.copper = Arc::new(
        crate::copper::PreparedCopper::build(
            &scene.zones,
            crate::copper::CopperLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    let mut pads = crate::pads::PreparedPads::build(
        &scene.pins,
        &scene.vias,
        crate::pads::PadLimits::default(),
        &cancel,
    )
    .unwrap();
    pads.custom_mesh = Some(Arc::new(
        pads.build_custom_meshes(
            crate::copper::CopperLimits::default(),
            &pomelo_core::copper::MeshLimits::default(),
            &cancel,
        )
        .unwrap(),
    ));
    frame.pads = Some(Arc::new(pads));
    frame.drills = Some(Arc::new(
        crate::drills::PreparedDrills::build(
            &scene.pins,
            &scene.vias,
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap()
        .geometry,
    ));
    let pads = frame.pads.as_ref().unwrap();
    let custom = pads.custom_mesh.as_ref().unwrap();
    let drills = frame.drills.as_ref().unwrap();
    let outlines = frame.zone_outlines.as_ref().unwrap();
    let drawings = frame.drawings.as_ref().unwrap();
    let expected = [
        frame.traces.tracks.instances.len() as u64 * 128,
        frame.copper.upload_bytes() as u64,
        pads.analytic.len() as u64 * 128,
        custom.upload_bytes() as u64,
        pads.custom_outlines
            .as_ref()
            .map_or(0, |p| p.instances.len() as u64 * 128),
        drills.analytic.len() as u64 * 128,
        outlines.instances.len() as u64 * 128,
        drawings.instances.len() as u64 * 128,
    ];
    assert_eq!(
        frame.traces.tracks.instances.len(),
        scene.segments.len() + scene.outline.len()
    );
    assert_eq!(frame.copper.batches.len(), scene.zones.len());
    assert_eq!(scene.zones.len(), 1017);
    let backdrill_count = drills
        .analytic
        .iter()
        .filter(|p| p.source[3] & 2 != 0)
        .count();
    let all_layers: BTreeSet<_> = frame
        .traces
        .tracks
        .batches
        .iter()
        .map(|b| b.layer)
        .chain(frame.copper.batches.iter().map(|b| b.layer))
        .chain(pads.batches.iter().map(|b| b.layer))
        .chain(custom.batches.iter().map(|b| b.layer))
        .chain(
            pads.custom_outlines
                .iter()
                .flat_map(|source| source.batches.iter().map(|b| b.layer)),
        )
        .chain(outlines.batches.iter().map(|b| b.layer))
        .chain(drawings.batches.iter().map(|b| b.layer))
        .collect();
    frame.layer_order = Arc::new(
        frame.display.ordered_layers(
            scene
                .layers
                .iter()
                .map(|l| l.id)
                .chain(scene.special_layers.iter().map(|l| l.id))
                .chain(scene.drawing_layers.iter().map(|l| l.id))
                .chain(all_layers.iter().copied())
                .filter(|layer| *layer != LayerId::UNASSIGNED),
        ),
    );
    frame.traces.bounds = scene.bounds;
    let detail = Camera {
        center: Point::new(16.9801794, 75.1774214),
        pixels_per_mm: 100.0,
        flipped: false,
    };
    frame.traces.camera = Some(detail);
    frame.traces.colors = Arc::new(
        scene
            .layers
            .iter()
            .map(|l| (l.id, [0.0, 1.0, 0.0, 1.0]))
            .collect(),
    );
    frame.copper_opacity = 99.0 / 255.0;
    let display = Arc::make_mut(&mut frame.display);
    display.layer_order = frame.layer_order.as_ref().clone();
    display.appearance = Default::default();
    display.hidden_layers = all_layers.clone();
    display.show_copper = false;
    display.show_drawings = false;
    display.show_drills = false;
    display.show_backdrills = false;
    display.copper_opacity = frame.copper_opacity;
    display.show_texts = false;
    display
        .appearance
        .set_color(ColorTarget::Etch(LayerId(0)), Some(RgbColor([0, 255, 0])));
    display
        .appearance
        .set_color(ColorTarget::Pin(LayerId(0)), Some(RgbColor([0, 0, 255])));
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Telemetry::default();
    let mut renderer = telemetry.renderer();
    let mut upload_bytes_by_draw = Vec::new();
    let max_draws = expected
        .iter()
        .map(|bytes| bytes.div_ceil(BUDGET))
        .sum::<u64>()
        + 24;
    for index in 0..max_draws {
        upload_bytes_by_draw.push(draw_checked(
            &mut renderer,
            &gpu,
            &frame,
            &target,
            &telemetry,
        ));
        if index % 16 == 0 {
            eprintln!(
                "S5000C_FULL_UPLOAD draw={} uploaded={:?} expected={expected:?}",
                index + 1,
                telemetry.bytes()
            );
        }
        if renderer.geometry_is_uploaded(&frame) {
            break;
        }
    }
    assert!(
        renderer.geometry_is_uploaded(&frame),
        "production readiness incomplete after bounded draws={max_draws}"
    );
    assert_eq!(
        telemetry.bytes(),
        expected,
        "every expected whole-source geometry slot must complete"
    );
    let uploaded = telemetry.bytes();
    let top = LayerId(0);
    let display = Arc::make_mut(&mut frame.display);
    display.hidden_layers = all_layers
        .iter()
        .copied()
        .filter(|layer| *layer != top)
        .collect();
    display.show_copper = true;
    display.show_drills = true;
    display.show_backdrills = true;
    display.global_opacity = 0.0;
    assert_eq!(
        draw_checked(&mut renderer, &gpu, &frame, &target, &telemetry),
        0
    );
    let pixels = target.read(&context);
    let dynamic = scene.zones.iter().find(|z| z.id.0 == 5598600).unwrap();
    assert_eq!(
        dynamic.paths.iter().map(Vec::len).collect::<Vec<_>>(),
        [23, 8]
    );
    // Classify neighboring copper from all original TOP source contours, not
    // the two-zone subset, then test the genuinely empty band at global0.
    let hole_bounds = dynamic.mesh.ring_bounds[1];
    let neighboring: Vec<_> = scene
        .zones
        .iter()
        .filter(|z| {
            z.layer == top
                && z.mesh.ring_bounds.first().is_some_and(|b| {
                    b.min.x <= hole_bounds.max.x
                        && b.max.x >= hole_bounds.min.x
                        && b.min.y <= hole_bounds.max.y
                        && b.max.y >= hole_bounds.min.y
                })
        })
        .collect();
    let (mut gap_count, mut occupied_by_neighbor) = (0, 0);
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = detail.view_to_board(Point::new(x as f64 + 0.5, y as f64 + 0.5), 128.0, 128.0);
            if hole_distance(p) < -0.035 && pin_distance(p) > 0.035 {
                let occupied = neighboring
                    .iter()
                    .any(|zone| zone.mesh.covers_fill(p, &cancel).unwrap());
                if occupied {
                    occupied_by_neighbor += 1;
                    continue;
                }
                assert_eq!(
                    rgb(&pixels, x, y),
                    [0; 3],
                    "whole-scene real void gap ({x},{y})"
                );
                gap_count += 1;
            }
        }
    }
    assert!(
        gap_count > 500,
        "whole-scene true void gap has insufficient independent interior samples"
    );
    Arc::make_mut(&mut frame.display).global_opacity = 1.0;
    assert_eq!(
        draw_checked(&mut renderer, &gpu, &frame, &target, &telemetry),
        0
    );
    let detail_pixels = target.read(&context);
    image::save_buffer(&png, &detail_pixels, SIDE, SIDE, image::ColorType::Rgba8).unwrap();
    let mut opaque_pin = 0;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = detail.view_to_board(Point::new(x as f64 + 0.5, y as f64 + 0.5), 128.0, 128.0);
            if pin_distance(p) < -0.035 {
                assert_eq!(
                    rgb(&detail_pixels, x, y),
                    [0, 0, 255],
                    "whole-scene opaque Pin41"
                );
                opaque_pin += 1;
            }
        }
    }
    assert!(opaque_pin > 500);
    let display = Arc::make_mut(&mut frame.display);
    display.hidden_layers.clear();
    display.show_drawings = true;
    display.static_shapes_fill_solid = true;
    display.global_opacity = 128.0 / 255.0;
    frame.copper_opacity = 128.0 / 255.0;
    display.copper_opacity = frame.copper_opacity;
    let mut fit = detail;
    assert!(fit.fit(scene.bounds, 128.0, 128.0, 8.0));
    frame.traces.camera = Some(fit);
    assert_eq!(
        draw_checked(&mut renderer, &gpu, &frame, &target, &telemetry),
        0
    );
    let fit_pixels = target.read(&context);
    let nonblack = fit_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3] != [0, 0, 0])
        .count();
    assert!(
        nonblack > 100,
        "whole geometry fit must produce actual framebuffer content"
    );
    image::save_buffer(&fit_png, &fit_pixels, SIDE, SIDE, image::ColorType::Rgba8).unwrap();
    assert_eq!(telemetry.bytes(), uploaded);
    assert!(renderer.geometry_is_uploaded(&frame));
    // SAFETY: query from the live hardware device into an initialized adapter description.
    let adapter = unsafe {
        device
            .cast::<IDXGIDevice>()
            .unwrap()
            .GetAdapter()
            .unwrap()
            .GetDesc()
            .unwrap()
    };
    let adapter_name = String::from_utf16_lossy(
        &adapter.Description[..adapter
            .Description
            .iter()
            .position(|x| *x == 0)
            .unwrap_or(adapter.Description.len())],
    );
    let report = json!({"source":{"path":path,"sha256":sha,"bytes":std::fs::metadata(&path).unwrap().len(),"identity":board.identity},
        "whole_scene_geometry_uploaded":true,"gpui_presented":false,"text_labels_validated":false,"allegro_visual_parity":false,
        "adapter":adapter_name,"hardware_device_requested":true,"source_subset_used":false,
        "counts":{"source_segments":scene.segments.len(),"source_outline":scene.outline.len(),"tracks":frame.traces.tracks.instances.len(),
            "zones":frame.copper.batches.len(),"copper_vertices":frame.copper.vertices.len(),"copper_indices":frame.copper.indices.len(),
            "pads":pads.analytic.len(),"custom_pad_placements":pads.custom.len(),"custom_vertices":custom.vertices.len(),"custom_indices":custom.indices.len(),
            "drills":drills.analytic.len(),"backdrill_patterns":backdrill_count,"drawings":drawings.instances.len(),"zone_outlines":outlines.instances.len()},
        "default_limits":{"copper_vertices":crate::copper::CopperLimits::default().max_vertices,
            "copper_indices":crate::copper::CopperLimits::default().max_indices,"copper_bytes":crate::copper::CopperLimits::default().max_bytes,
            "trace_instances":TraceLimits::default().max_instances,"trace_bytes":TraceLimits::default().max_bytes,
            "pad_count":crate::pads::PadLimits::default().max_pads,"pad_bytes":crate::pads::PadLimits::default().max_bytes},
        "resource_order":["traces_with_board_outline","copper","analytic_pads","custom_pad_copper","custom_pad_outlines","drills_backdrill","zone_outlines","drawings"],
        "layer_order":frame.layer_order.as_ref(),
        "expected_uploaded_bytes":expected,"actual_uploaded_bytes":uploaded,
        "combined_upload_budget_per_draw":BUDGET,"upload_bytes_by_draw":upload_bytes_by_draw,"bounded_max_upload_draws":max_draws,
        "u94_full_scene":{"camera":detail,"dpi":1,"shape":99,"filled":true,"top_only":true,"static_solid":false,
            "global0_true_void_gap_samples":gap_count,"source_top_neighbor_candidates":neighboring.iter().map(|z|z.id.0).collect::<Vec<_>>(),
            "excluded_gap_samples_occupied_by_source_neighbor":occupied_by_neighbor,"global255_opaque_pin41_samples":opaque_pin},
        "whole_board_fit":{"camera":fit,"all_geometry_layers_visible":true,"shape":128,"global":128,"static_solid":true,
            "filled":true,"drills_backdrill_drawings":true,"nonblack_readback_pixels":nonblack},
        "after_camera_alpha_uploaded_bytes":telemetry.bytes(),"copper_statistics":telemetry.copper.snapshot(),
        "custom_copper_statistics":telemetry.custom.snapshot(),"traces_statistics":telemetry.traces.snapshot(),
        "pads_statistics":telemetry.pads.snapshot(),"custom_outline_statistics":telemetry.custom_edges.snapshot(),
        "drills_statistics":telemetry.drills.snapshot(),"zone_outline_statistics":telemetry.zone_edges.snapshot(),"drawings_statistics":telemetry.drawings.snapshot(),
        "limits":["No GPUI window or surface presentation","No font/ANSI changes; no text/labels/atlas resources prepared",
            "No optional asynchronous curve LOD/refinement cache supplied","Whole geometry upload and bounded local source void/readback proof, not Allegro visual acceptance",
            "4MiB checks measure geometry payload uploads; constant updates and empty pipeline allocations excluded"]});
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!(
        "S5000C_FULL_GEOMETRY {}",
        serde_json::to_string(&report).unwrap()
    );
}
