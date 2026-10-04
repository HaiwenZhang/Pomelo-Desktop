//! Dynamic Base boundary policy derived from controlled RDIMM alpha25/26 captures.
//! Hardware coverage is not a claim of pixel-perfect Allegro raster equivalence.
use super::compositor_parity::{device, fixture, gpu_context};
use super::static_shape::renderer;
use super::*;
use crate::backend::{BoardFrame, CopperTelemetry};
use pomelo_core::{
    interaction::Camera,
    model::{Zone, ZoneKind},
    selection::SelectedObject,
};

fn frame() -> BoardFrame {
    let cancel = CancellationToken::default();
    let rectangle = |x: f64, y: f64, w: f64, h: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + w, y),
            Point::new(x + w, y + h),
            Point::new(x, y + h),
        ]
    };
    let zones = [
        (ZoneKind::Dynamic, 60, -5.0, 3.0),
        (ZoneKind::Static, 61, -1.0, 2.0),
        (ZoneKind::Unknown, 62, 2.0, 3.0),
    ]
    .map(|(kind, id, x, width)| {
        let mut rings = vec![rectangle(x, -4.0, width, 6.0)];
        if kind == ZoneKind::Dynamic {
            rings.push(rectangle(-4.0, -1.0, 1.0, 2.0));
        }
        Zone {
            id: ObjectId(id),
            layer: LayerId(0),
            net: NetId(1),
            kind,
            paths: vec![],
            mesh: pomelo_core::copper::CopperMesh::build(
                &rings,
                &[],
                &pomelo_core::copper::MeshLimits::default(),
                &cancel,
            )
            .unwrap(),
        }
    });
    let mut frame = fixture();
    frame.traces.tracks =
        Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap());
    frame.traces.camera = Some(Camera {
        center: Point::default(),
        pixels_per_mm: 5.0,
        flipped: false,
    });
    frame.copper = Arc::new(
        crate::copper::PreparedCopper::build(
            &zones,
            crate::copper::CopperLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    frame.zone_outlines = Some(Arc::new(
        PreparedTracks::build_zone_outlines(&zones, TraceLimits::default(), &cancel).unwrap(),
    ));
    frame.pads = None;
    frame.drills = None;
    Arc::make_mut(&mut frame.display).static_shapes_fill_solid = true;
    frame
}

#[test]
#[ignore = "requires Windows hardware D3D11; actual Allegro raster phase remains a separate visual check"]
fn hardware_dynamic_base_outline_switches_at_shape25_26_without_fill_or_hole_changes() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let copper = Arc::new(CopperTelemetry::default());
    let edges = Arc::new(TraceTelemetry::default());
    let mut renderer = renderer(&copper).with_zone_outline_telemetry(Arc::clone(&edges));
    let mut reference_renderer =
        super::static_shape::renderer(&Arc::new(CopperTelemetry::default()));
    let mut reference = frame();
    let mut frame = frame();
    reference.copper = Arc::clone(&frame.copper);
    reference.zone_outlines = None;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        target.bind(&context);
        reference_renderer.draw(&gpu, &reference).unwrap();
    }
    let before_copper = copper.snapshot().lifetime_uploaded_bytes;
    let before_edges = edges.snapshot();
    let mut suppressed = 0;
    let mut low_border = 0;
    let mut preserved_fill = 0;
    let mut preserved_void = 0;
    let mut legacy_border = 0;
    for scale in [1.0, 1.25, 1.5, 2.0] {
        frame.traces.scale_factor = scale;
        for flipped in [false, true] {
            frame.traces.camera.as_mut().unwrap().flipped = flipped;
            let mut camera = frame.traces.camera.unwrap();
            camera.pixels_per_mm *= f64::from(scale);
            for alpha in [0_u8, 1, 8, 25, 26, 99, 255] {
                frame.copper_opacity = f32::from(alpha) / 255.0;
                let mut global255 = None;
                for global in [1.0, 0.0] {
                    Arc::make_mut(&mut frame.display).global_opacity = global;
                    target.bind(&context);
                    renderer.draw(&gpu, &frame).unwrap();
                    let actual = target.read(&context);
                    if let Some(baseline) = &global255 {
                        assert_eq!(
                            &actual, baseline,
                            "shape{alpha} changed under Global0, DPI{scale}"
                        );
                    } else {
                        global255 = Some(actual.clone());
                    }
                    reference.traces = frame.traces.clone();
                    reference.display = Arc::clone(&frame.display);
                    reference.copper_opacity = frame.copper_opacity;
                    target.bind(&context);
                    reference_renderer.draw(&gpu, &reference).unwrap();
                    let filled_only = target.read(&context);
                    let margin = 2.2 / camera.pixels_per_mm;
                    for y in 0..SIDE as usize {
                        for x in 0..SIDE as usize {
                            let p = camera.view_to_board(
                                Point::new(x as f64 + 0.5, y as f64 + 0.5),
                                f64::from(SIDE),
                                f64::from(SIDE),
                            );
                            if p.x > -5.0 - margin
                                && p.x < -2.0 + margin
                                && p.y > -4.0 - margin
                                && p.y < 2.0 + margin
                            {
                                if alpha >= 26 {
                                    assert_eq!(
                                        rgb(&actual, x, y),
                                        rgb(&filled_only, x, y),
                                        "independent bright Dynamic edge at shape{alpha} {x},{y} DPI{scale}"
                                    );
                                    suppressed += 1;
                                } else if rgb(&actual, x, y) != rgb(&filled_only, x, y) {
                                    low_border += 1;
                                }
                                if p.x > -4.0 + margin
                                    && p.x < -3.0 - margin
                                    && p.y > -1.0 + margin
                                    && p.y < 1.0 - margin
                                {
                                    assert_eq!(
                                        rgb(&actual, x, y),
                                        [0; 3],
                                        "true void filled at shape{alpha} {x},{y}"
                                    );
                                    preserved_void += 1;
                                }
                                if p.x > -5.0 + margin
                                    && p.x < -4.0 - margin
                                    && p.y > -3.5
                                    && p.y < -1.5
                                {
                                    assert_eq!(
                                        rgb(&actual, x, y),
                                        rgb(&filled_only, x, y),
                                        "low-alpha fill modified at {x},{y}"
                                    );
                                    preserved_fill += 1;
                                }
                            }
                            if alpha == 0
                                && p.y.abs() < 1.0
                                && ((p.x - 1.0).abs() * camera.pixels_per_mm < 0.51
                                    || (p.x - 5.0).abs() * camera.pixels_per_mm < 0.51)
                            {
                                assert!(
                                    rgb(&actual, x, y)[0] > 100,
                                    "Static/Unknown legacy border changed at {x},{y}"
                                );
                                legacy_border += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        suppressed > 5000
            && low_border > 500
            && preserved_fill > 100
            && preserved_void > 100
            && legacy_border > 100
    );
    assert_eq!(before_copper, copper.snapshot().lifetime_uploaded_bytes);
    assert_eq!(before_edges.cache_builds, edges.snapshot().cache_builds);
    assert_eq!(before_edges.uploaded_bytes, edges.snapshot().uploaded_bytes);
    eprintln!(
        "DYNAMIC_OUTLINE_GPU threshold25_26_samples={suppressed} low_border={low_border} fill={preserved_fill} void={preserved_void} static_unknown={legacy_border} dpi=1,1.25,1.5,2 global_independent=true copper_edge_reupload=false actual_board_pixel_perfect=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; validates physical line policy, not Allegro raster phase"]
fn hardware_dynamic_low_alpha_hairline_has_one_physical_pixel_coverage_at_multiple_dpi() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer(&Arc::new(CopperTelemetry::default()));
    let mut frame = frame();
    frame.copper_opacity = 0.0;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let mut profiles = 0;
    for scale in [1.0, 1.25, 1.5, 2.0] {
        frame.traces.scale_factor = scale;
        for zoom in [4.0, 5.0] {
            for phase in [0.0, 0.031] {
                frame.traces.camera = Some(Camera {
                    center: Point::new(phase, 0.0),
                    pixels_per_mm: zoom,
                    flipped: false,
                });
                target.bind(&context);
                renderer.draw(&gpu, &frame).unwrap();
                let pixels = target.read(&context);
                let edge_x = f64::from(SIDE) * 0.5 + (-5.0 - phase) * zoom * f64::from(scale);
                let profile = (0..SIDE as usize)
                    .filter(|x| (*x as f64 + 0.5 - edge_x).abs() < 3.0)
                    .map(|x| f64::from(rgb(&pixels, x, 64)[0]) / 255.0)
                    .sum::<f64>();
                assert!(
                    (profile - 1.0).abs() < 0.012,
                    "physical line grew at DPI{scale} zoom{zoom} phase{phase}: integrated coverage{profile}"
                );
                profiles += 1;
            }
        }
    }
    eprintln!(
        "DYNAMIC_OUTLINE_WIDTH_GPU profiles={profiles} integrated_physical_coverage=1 tolerance=.012 actual_allegro_phase_unverified=true"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; Base suppression must not remove selected or hovered boundaries"]
fn hardware_dynamic_high_alpha_preserves_object_hover_and_net_overlay_boundaries() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let copper = Arc::new(CopperTelemetry::default());
    let edges = Arc::new(TraceTelemetry::default());
    let mut renderer = renderer(&copper).with_zone_outline_telemetry(Arc::clone(&edges));
    let mut frame = frame();
    frame.traces.camera.as_mut().unwrap().pixels_per_mm = 10.0;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let base = target.read(&context);
    let before_copper = copper.snapshot().lifetime_uploaded_bytes;
    let before_edges = edges.snapshot();
    let mut selected = 0;
    let camera = frame.traces.camera.unwrap();
    for mode in 0..3 {
        frame.traces.highlighted_object =
            (mode == 0).then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        frame.traces.hovered_object =
            (mode == 1).then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        frame.traces.highlighted_net = (mode == 2).then_some((NetId(1), [1.0; 4]));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let pixels = target.read(&context);
        for y in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                let p = camera.view_to_board(
                    Point::new(x as f64 + 0.5, y as f64 + 0.5),
                    f64::from(SIDE),
                    f64::from(SIDE),
                );
                if p.y.abs() < 1.5 && (p.x + 5.0).abs() < 0.11 {
                    assert!(
                        rgb(&pixels, x, y)[1] > 100,
                        "overlay{mode} boundary suppressed at{x},{y}"
                    );
                    selected += 1;
                }
                if mode < 2 && p.x > 2.5 && p.x < 4.5 && p.y > -3.5 && p.y < 1.5 {
                    assert_eq!(
                        rgb(&pixels, x, y),
                        rgb(&base, x, y),
                        "object/hover changed Unknown neighbor at{x},{y}"
                    );
                }
                if p.x > -3.65 && p.x < -3.35 && p.y.abs() < 0.7 {
                    assert_eq!(
                        rgb(&pixels, x, y),
                        [0; 3],
                        "overlay{mode} filled real void at{x},{y}"
                    );
                }
            }
        }
    }
    assert!(selected > 50);
    assert_eq!(before_copper, copper.snapshot().lifetime_uploaded_bytes);
    assert_eq!(before_edges.cache_builds, edges.snapshot().cache_builds);
    assert_eq!(before_edges.uploaded_bytes, edges.snapshot().uploaded_bytes);
    eprintln!(
        "DYNAMIC_OUTLINE_OVERLAY_GPU border_samples={selected} object_hover_net=true neighbor_void_preserved=true reupload=false global0=true"
    );
}
