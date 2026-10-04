//! Independent analytic coverage oracle on a hardware D3D11 target.
use super::*;
use crate::{
    backend::{CopperRenderer, CopperTelemetry},
    copper::{CopperLimits, PreparedCopper},
    scene::curves::{
        CurveCacheLimits, CurveFillCache, CurveView,
        tests::{board, circle_path, circle_ring, lines, zone},
    },
};
use pomelo_core::{
    display::BoardDisplay,
    interaction::Camera,
    model::{LayerId, Zone},
    selection::SelectedObject,
};

fn fixture(kind: usize) -> (Zone, Point) {
    let polygon = |coordinates: &[[f64; 2]]| {
        coordinates
            .iter()
            .map(|p| Point::new(p[0], p[1]))
            .collect::<Vec<_>>()
    };
    match kind {
        0 => (
            zone(
                7,
                vec![circle_ring(Point::default(), 0.05)],
                vec![circle_path(Point::default(), 0.05)],
            ),
            Point::new(0.05 / 2_f64.sqrt(), 0.05 / 2_f64.sqrt()),
        ),
        1 => {
            let square = polygon(&[[-0.09, -0.09], [0.09, -0.09], [0.09, 0.09], [-0.09, 0.09]]);
            let mut rings = vec![square.clone()];
            let mut paths = vec![lines(&square)];
            for (x, radius) in [(-0.005, 0.02), (0.005, 0.02), (-0.005, 0.008)] {
                let p = Point::new(x, 0.0);
                rings.push(circle_ring(p, radius));
                paths.push(circle_path(p, radius));
            }
            (zone(7, rings, paths), Point::new(0.0, 0.019))
        }
        _ => {
            let u = polygon(&[
                [-0.03, -0.03],
                [0.03, -0.03],
                [0.03, 0.03],
                [0.0004, 0.03],
                [0.0004, 0.0],
                [-0.0004, 0.0],
                [-0.0004, 0.03],
                [-0.03, 0.03],
            ]);
            let hole = Point::new(0.0, -0.02);
            (
                zone(
                    7,
                    vec![u.clone(), circle_ring(hole, 0.006)],
                    vec![lines(&u), circle_path(hole, 0.006)],
                ),
                Point::new(0.0, 0.022),
            )
        }
    }
}
fn analytic(kind: usize, p: Point) -> (bool, f64) {
    match kind {
        0 => {
            let distance = p.x.hypot(p.y) - 0.05;
            (distance < 0.0, distance.abs())
        }
        1 => {
            let exterior = 0.09 - p.x.abs().max(p.y.abs());
            let holes: Vec<_> = [(-0.005, 0.02), (0.005, 0.02), (-0.005, 0.008)]
                .into_iter()
                .map(|(x, r)| (p.x - x).hypot(p.y) - r)
                .collect();
            (
                exterior > 0.0 && holes.iter().all(|d| *d > 0.0),
                holes.iter().fold(exterior.abs(), |a, d| a.min(d.abs())),
            )
        }
        _ => {
            let outer = p.x.abs() < 0.03 && p.y > -0.03 && p.y < 0.03;
            let covered =
                outer && (p.y < 0.0 || p.x.abs() > 0.0004) && p.x.hypot(p.y + 0.02) > 0.006;
            let distances = [
                (p.x.abs() - 0.03).abs(),
                (p.y + 0.03).abs(),
                (p.y - 0.03).abs(),
                (p.x.abs() - 0.0004).abs(),
                p.y.abs(),
                (p.x.hypot(p.y + 0.02) - 0.006).abs(),
            ];
            (covered, distances.into_iter().fold(f64::INFINITY, f64::min))
        }
    }
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_curve_fill_preserves_disjoint_islands_hole_union_and_cache() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; hardware only, no WARP fallback.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let (device, context) = (device.unwrap(), context.unwrap());
    let target = Target::new(&device);
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    let cancel = CancellationToken::default();
    let empty = Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap());
    let mut cases = 0;
    let mut checked = 0;
    for kind in 0..3 {
        let (zone, at) = fixture(kind);
        let static_source = Arc::new(
            PreparedCopper::build(
                std::slice::from_ref(&zone),
                CopperLimits::default(),
                &cancel,
            )
            .unwrap(),
        );
        let scene = board(vec![zone]);
        for scale in [1001.0, 8192.0, 50000.0] {
            for dpi in [1.0, 2.0] {
                for flipped in [false, true] {
                    for masked in [false, true] {
                        let camera = Camera {
                            center: at,
                            pixels_per_mm: scale,
                            flipped,
                        };
                        let logical = f64::from(SIDE) / dpi;
                        let request = CurveView::new(camera, logical, logical, dpi).unwrap();
                        let curves = CurveFillCache::prepare(
                            &scene,
                            request,
                            &BoardDisplay::default(),
                            &CurveFillCache::default(),
                            CurveCacheLimits::default(),
                            &cancel,
                        )
                        .unwrap();
                        let telemetry = Arc::new(CopperTelemetry::default());
                        let mut renderer = CopperRenderer::new(Arc::clone(&telemetry));
                        let mask = if masked {
                            ViewBounds::new(
                                point(ScaledPixels(12.0), ScaledPixels(17.0)),
                                size(ScaledPixels(99.0), ScaledPixels(96.0)),
                            )
                        } else {
                            bounds
                        };
                        let gpu = NativeGpuContext {
                            device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
                            viewport: [SIDE as f32; 2],
                            bounds,
                            content_mask: ContentMask { bounds: mask },
                        };
                        renderer.prepare(&gpu, &static_source, true).unwrap();
                        renderer.prepare_curves(&gpu, Some(&curves), true).unwrap();
                        let uploaded = telemetry.snapshot().curve_lifetime_uploaded_bytes;
                        assert!(
                            uploaded > 0,
                            "empty curve fixture kind={kind} scale={scale} dpi={dpi} flip={flipped} active={:?}",
                            curves.active
                        );
                        for highlight in 0..3 {
                            let mut frame = TraceFrame {
                                pass: crate::backend::OverlayPass::Base,
                                filled: true,
                                hover_selection: None,
                                color_mode: pomelo_core::display::ColorMode::Layer,
                                tracks: Arc::clone(&empty),
                                bounds: scene.bounds,
                                camera: Some(camera),
                                scale_factor: dpi as f32,
                                colors: Arc::new(BTreeMap::from([(
                                    LayerId(1),
                                    [0.0, 1.0, 0.0, 1.0],
                                )])),
                                fallback_color: [1.0; 4],
                                material_override: None,
                                opacity: 1.0,
                                highlighted_net: None,
                                highlighted_objects: None,
                                highlighted_related_objects: None,
                                hovered_object: None,
                                highlighted_trace: None,
                                highlighted_object: None,
                            };
                            match highlight {
                                1 => {
                                    frame.highlighted_object = Some((
                                        SelectedObject::Zone(ObjectId(7)),
                                        [0.0, 0.0, 1.0, 1.0],
                                    ))
                                }
                                2 => {
                                    frame.hovered_object = Some((
                                        SelectedObject::Zone(ObjectId(7)),
                                        [1.0, 0.0, 1.0, 1.0],
                                    ))
                                }
                                _ => {}
                            }
                            target.bind(&context);
                            // SAFETY: this test owns the same-device target and context.
                            unsafe {
                                context.ClearRenderTargetView(&target.view, &[1.0, 0.0, 0.0, 1.0]);
                            }
                            renderer
                                .draw(&gpu, &frame, &static_source, 0.5, false)
                                .unwrap();
                            let pixels = target.read(&context);
                            let mut physical_camera = camera;
                            physical_camera.pixels_per_mm *= dpi;
                            for y in 0..SIDE as usize {
                                for x in 0..SIDE as usize {
                                    let p = physical_camera.view_to_board(
                                        Point::new(x as f64 + 0.5, y as f64 + 0.5),
                                        f64::from(SIDE),
                                        f64::from(SIDE),
                                    );
                                    let (inside, distance) = analytic(kind, p);
                                    // Tessellation has a quarter-pixel sagitta budget; test away
                                    // from that boundary band against exact analytic coverage.
                                    if distance * scale * dpi <= 0.35 {
                                        continue;
                                    }
                                    let in_mask = !masked
                                        || ((12..111).contains(&x) && (17..113).contains(&y));
                                    let expected: [u8; 3] = if !inside || !in_mask {
                                        [255, 0, 0]
                                    } else {
                                        match highlight {
                                            1 => [128, 0, 128],
                                            2 => [255, 0, 128],
                                            _ => [128, 128, 0],
                                        }
                                    };
                                    let actual = rgb(&pixels, x, y);
                                    assert!(actual.iter().zip(expected).all(|(&a,b)| (i16::from(a)-i16::from(b)).abs() <= 1), "curve kind={kind} scale={scale} dpi={dpi} flip={flipped} mask={masked} highlight={highlight} pixel={x},{y} expected={expected:?} actual={actual:?}");
                                    checked += 1;
                                }
                            }
                            renderer.prepare_curves(&gpu, Some(&curves), true).unwrap();
                            assert_eq!(
                                telemetry.snapshot().curve_lifetime_uploaded_bytes,
                                uploaded,
                                "repeated frames/highlights must not re-upload curves"
                            );
                            cases += 1;
                        }
                        let before = telemetry.snapshot();
                        renderer.reset();
                        renderer.prepare(&gpu, &static_source, true).unwrap();
                        renderer.prepare_curves(&gpu, Some(&curves), true).unwrap();
                        assert_eq!(
                            telemetry.snapshot().curve_lifetime_uploaded_bytes,
                            before.curve_lifetime_uploaded_bytes + uploaded
                        );
                    }
                }
            }
        }
    }
    assert_eq!(cases, 216);
    assert!(checked > 3_000_000);
    eprintln!("CURVE_HARDWARE_CASES={cases} ANALYTIC_PIXELS={checked}");
}

fn hardware_device() -> (ID3D11Device, ID3D11DeviceContext) {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs; this diagnostic requests hardware only.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    (device.unwrap(), context.unwrap())
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_curve_uploads_share_one_four_mib_budget() {
    use crate::copper::CopperVertex;
    let (device, context) = hardware_device();
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let cancel = CancellationToken::default();
    let (a, _) = fixture(0);
    let mut b = a.clone();
    b.id = ObjectId(8);
    let scene = board(vec![a, b]);
    let mut cache = CurveFillCache::prepare(
        &scene,
        CurveView {
            bounds: scene.bounds,
            tolerance: 0.001,
        },
        &BoardDisplay::default(),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
        &cancel,
    )
    .unwrap();
    // Stress the real D3D11 uploader with two independent nine-MiB sources.
    // Drawing is irrelevant here; no batches reference these diagnostic buffers.
    for entry in cache.entries.values_mut() {
        entry.source = Arc::new(PreparedCopper {
            memory_reservation: None,
            source: None,
            vertices: vec![CopperVertex { position: [0.0; 4] }; 600_000],
            indices: vec![0, 0, 0],
            batches: vec![],
        });
    }
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = CopperRenderer::new(Arc::clone(&telemetry));
    renderer.prepare_curves(&gpu, Some(&cache), false).unwrap();
    assert_eq!(telemetry.snapshot().curve_cache_builds, 0);
    let expected = cache
        .entries
        .values()
        .map(|e| e.source.upload_bytes() as u64)
        .sum::<u64>();
    let mut frames = 0;
    while telemetry.snapshot().curve_uploaded_bytes < expected {
        let before = telemetry.snapshot().curve_lifetime_uploaded_bytes;
        renderer.prepare_curves(&gpu, Some(&cache), true).unwrap();
        let after = telemetry.snapshot();
        assert!(after.curve_lifetime_uploaded_bytes - before <= 4 * 1024 * 1024);
        frames += 1;
        assert!(frames <= 5);
    }
    assert_eq!(frames, 5);
    assert_eq!(telemetry.snapshot().curve_cache_builds, 2);
    assert_eq!(telemetry.snapshot().curve_lifetime_uploaded_bytes, expected);
    eprintln!("CURVE_UPLOAD_BYTES={expected} FRAMES={frames} MAX_BYTES_PER_FRAME=4194304");
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_curve_zone_labels_share_the_final_hole_mask() {
    use crate::backend::board::TraceScope;
    use pomelo_core::model::{BoardText, TextAlignment};
    let (device, context) = hardware_device();
    let target = Target::new(&device);
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let cancel = CancellationToken::default();
    let (zone, at) = fixture(1);
    let source = Arc::new(
        PreparedCopper::build(
            std::slice::from_ref(&zone),
            CopperLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    let scene = board(vec![zone]);
    let camera = Camera {
        center: at,
        pixels_per_mm: 1001.0,
        flipped: false,
    };
    let curves = CurveFillCache::prepare(
        &scene,
        CurveView::new(camera, f64::from(SIDE), f64::from(SIDE), 1.0).unwrap(),
        &BoardDisplay::default(),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
        &cancel,
    )
    .unwrap();
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["G"], &cancel).unwrap());
    let mut glyphs = crate::text::msdf::PreparedGlyphs::build(
        &[BoardText {
            id: ObjectId(7),
            owner_id: None,
            layer: LayerId(1),
            class_id: 0,
            subclass: 0,
            text: "G".into(),
            at: Point::new(-0.045, -0.035),
            angle: 0.0,
            mirrored: false,
            align: TextAlignment::Left,
            font_index: 0,
            width: 0.1,
            height: 0.08,
            spacing: 0.0,
            line_spacing: 0.08,
            stroke_width: 0.0,
        }],
        font,
        1024 * 1024,
        &cancel,
    )
    .unwrap();
    for glyph in &mut glyphs.instances {
        glyph.ids[1] = pomelo_core::display::DisplayCategory::Zone as u32;
    }
    let frame = TraceFrame {
        pass: crate::backend::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        tracks: Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap()),
        bounds: scene.bounds,
        camera: Some(camera),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::from([(LayerId(1), [0.0, 1.0, 0.0, 1.0])])),
        fallback_color: [1.0; 4],
        material_override: None,
        opacity: 1.0,
        highlighted_net: None,
        highlighted_objects: None,
        highlighted_related_objects: None,
        hovered_object: None,
        highlighted_trace: None,
        highlighted_object: None,
    };
    let glyph_frame = frame.base().with_source(Arc::new(glyphs));
    let mut glyph_renderer = TraceRenderer::<crate::text::msdf::PreparedGlyphs>::new(Arc::new(
        TraceTelemetry::default(),
    ));
    let mut copper = CopperRenderer::new(Arc::new(CopperTelemetry::default()));
    copper.prepare(&gpu, &source, true).unwrap();
    copper.prepare_curves(&gpu, Some(&curves), true).unwrap();
    target.bind(&context);
    copper.draw(&gpu, &frame, &source, 0.5, false).unwrap();
    let baseline = target.read(&context);
    target.bind(&context);
    glyph_renderer.draw(&gpu, &glyph_frame).unwrap();
    let unmasked = target.read(&context);
    target.bind(&context);
    copper
        .draw_annotated(&gpu, &frame, 0.5, LayerId(1), None, &mut |_| {
            glyph_renderer.draw_prepared(
                &gpu,
                &glyph_frame,
                TraceScope::Labels(
                    LayerId(1),
                    pomelo_core::display::DisplayCategory::Zone,
                    Some(ObjectId(7)),
                ),
            )
        })
        .unwrap();
    let masked = target.read(&context);
    let (mut excluded, mut visible) = (0, 0);
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = camera.view_to_board(
                Point::new(x as f64 + 0.5, y as f64 + 0.5),
                f64::from(SIDE),
                f64::from(SIDE),
            );
            let (inside, distance) = analytic(1, p);
            if distance * camera.pixels_per_mm <= 0.35 {
                continue;
            }
            if !inside {
                assert_eq!(
                    rgb(&masked, x, y),
                    rgb(&baseline, x, y),
                    "label escaped curve mask {x},{y}"
                );
                if rgb(&unmasked, x, y) != [0; 3] {
                    excluded += 1;
                }
            } else if rgb(&masked, x, y) != rgb(&baseline, x, y) {
                visible += 1;
            }
        }
    }
    assert!(
        excluded > 10 && visible > 50,
        "need glyph pixels inside and outside holes excluded={excluded} visible={visible}"
    );
    eprintln!("CURVE_LABEL_EXCLUDED_PIXELS={excluded} VISIBLE_PIXELS={visible}");
}
