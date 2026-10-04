//! Allegro-observed static zone selection modes, independently of other primitives.
use super::compositor_parity::{device, gpu_context};
use super::static_shape::{prepared_frame, renderer};
use super::*;
use crate::backend::CopperTelemetry;
use pomelo_core::{
    display::DisplayCategory,
    interaction::Camera,
    model::{BoardText, TextAlignment, ZoneKind},
    selection::SelectedObject,
};

#[test]
#[ignore = "requires Windows hardware D3D11; tests observed static zone temporary selection"]
fn hardware_solid_static_zone_temporary_selection_keeps_base_fill_and_continuous_border() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    Arc::make_mut(&mut frame.display).static_shapes_fill_solid = true;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let base = target.read(&context);
    let before = telemetry.snapshot();
    frame.traces.highlighted_object = Some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected = target.read(&context);
    let camera = frame.traces.camera.unwrap();
    let mut preserved = 0;
    let mut border = 0;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = camera.view_to_board(
                Point::new(x as f64 + 0.5, y as f64 + 0.5),
                f64::from(SIDE),
                f64::from(SIDE),
            );
            if p.x > -4.8 && p.x < 4.8 && p.y > -4.8 && p.y < -2.2 {
                let near_void = p.x > -2.9 && p.x < -0.2 && p.y > -4.4 && p.y < -2.6;
                if !near_void {
                    assert_eq!(
                        rgb(&selected, x, y),
                        rgb(&base, x, y),
                        "temporary selection changed fill at {x},{y}"
                    );
                    preserved += 1;
                }
            }
            if p.x > -4.8 && p.x < 4.8 && (p.y + 2.0).abs() < 0.06 {
                assert!(
                    rgb(&selected, x, y).iter().all(|v| *v > 180),
                    "temporary border must remain continuous at {x},{y}"
                );
                border += 1;
            }
        }
    }
    assert!(preserved > 1000 && border > 50);
    assert_eq!(
        before.lifetime_uploaded_bytes,
        telemetry.snapshot().lifetime_uploaded_bytes
    );
    eprintln!(
        "ZONE_TEMPORARY_GPU preserved_fill_pixels={preserved} continuous_border_pixels={border} global_zero=true copper_reupload=false other_primitives_unchanged=true allegro_board_validated=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; unknown zone network styling is not inferred"]
fn hardware_unknown_zone_network_selection_keeps_existing_base_material() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    frame.zone_outlines = None;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
        if batch.object == ObjectId(60) {
            batch.kind = ZoneKind::Unknown;
        } else {
            batch.net = NetId(2);
        }
    }
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let baseline = target.read(&context);
    frame.traces.highlighted_net = Some((NetId(1), [1.0; 4]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected = target.read(&context);
    let mut kept = 0;
    for y in 86..111 {
        for x in 90..108 {
            let cx = (x as f32 + 0.5) % 5.0 - 2.5;
            let cy = (y as f32 + 0.5) % 5.0 - 2.5;
            if cx.hypot(cy) >= 1.25 {
                assert_eq!(
                    rgb(&selected, x, y),
                    rgb(&baseline, x, y),
                    "unknown base recolored or replaced at {x},{y}"
                );
                kept += 1;
            }
        }
    }
    assert!(kept > 100);
    eprintln!("ZONE_UNKNOWN_GPU existing_base_samples={kept} network_rule_not_inferred=true");
}

#[test]
#[ignore = "requires Windows hardware D3D11; base fill replacement must retain label stencil"]
fn hardware_network_zone_labels_keep_full_coverage_while_base_color_is_suppressed() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    frame.zone_outlines = None;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    frame.traces.highlighted_net = Some((NetId(1), [1.0; 4]));
    for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
        if batch.object != ObjectId(60) {
            batch.net = NetId(2);
        }
    }
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let background = target.read(&context);
    let before = telemetry.snapshot();
    let cancel = CancellationToken::default();
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["G"], &cancel).unwrap());
    let text = BoardText {
        id: ObjectId(60),
        owner_id: None,
        layer: LayerId(0),
        class_id: 6,
        subclass: 0,
        text: "G".into(),
        at: Point::new(-5.0, -5.0),
        angle: 0.0,
        mirrored: false,
        align: TextAlignment::Left,
        font_index: 0,
        width: 10.0,
        height: 3.0,
        spacing: 0.0,
        line_spacing: 3.0,
        stroke_width: 0.0,
    };
    let mut glyphs =
        crate::text::msdf::PreparedGlyphs::build(&[text], font, 1024 * 1024, &cancel).unwrap();
    for glyph in &mut glyphs.instances {
        glyph.ids[1] = DisplayCategory::Zone as u32;
        glyph.rotation[3] += 1.0;
    }
    frame.labels = Some(Arc::new(glyphs));
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let labelled = target.read(&context);
    let camera = frame.traces.camera.unwrap();
    let mut gap_ink = 0;
    let mut masked = 0;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = camera.view_to_board(
                Point::new(x as f64 + 0.5, y as f64 + 0.5),
                f64::from(SIDE),
                f64::from(SIDE),
            );
            let void = p.x > -2.6 && p.x < -0.5 && p.y > -4.1 && p.y < -2.9;
            let outside = p.x < -5.1 || p.x > 5.1 || p.y < -5.1 || p.y > -1.9;
            if void || outside {
                assert_eq!(
                    rgb(&labelled, x, y),
                    rgb(&background, x, y),
                    "net label escaped stencil at {x},{y}"
                );
                masked += 1;
            } else if p.x > -4.9
                && p.x < 4.9
                && p.y > -4.9
                && p.y < -2.1
                && rgb(&background, x, y) == [0, 0, 99]
                && rgb(&labelled, x, y) != rgb(&background, x, y)
            {
                gap_ink += 1;
            }
        }
    }
    assert!(gap_ink > 100 && masked > 1000);
    assert_eq!(
        before.lifetime_uploaded_bytes,
        telemetry.snapshot().lifetime_uploaded_bytes
    );
    eprintln!(
        "ZONE_NETWORK_LABEL_GPU ink_in_diamond_gaps={gap_ink} masked_pixels={masked} global_zero=true full_stencil=true base_color_suppressed=true copper_reupload=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; tests observed persistent static zone network bitmap"]
fn hardware_static_zone_network_selection_replaces_base_and_preserves_hole_union() {
    verify_network_selection(ZoneKind::Static);
}

#[test]
#[ignore = "requires Windows hardware D3D11; dynamic zone network selection observed in the same board"]
fn hardware_dynamic_zone_network_selection_replaces_base_and_preserves_hole_union() {
    verify_network_selection(ZoneKind::Dynamic);
}

fn verify_network_selection(zone_kind: ZoneKind) {
    let (device, context) = device();
    let mut gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    frame.zone_outlines = None;
    for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
        if batch.object == ObjectId(60) {
            batch.kind = zone_kind;
        } else {
            batch.net = NetId(2);
        }
    }
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let before = telemetry.snapshot();
    let mut ink = 0;
    let mut gaps = 0;
    let mut holes = 0;
    for (scale, zoom, flipped, origin) in [
        (1.5, 15.0, false, [0.0, 0.0]),
        (1.0, 10.0, true, [5.0, 7.0]),
    ] {
        frame.traces.scale_factor = scale;
        frame.traces.camera = Some(Camera {
            center: Point::new(0.4, -2.0),
            pixels_per_mm: zoom / f64::from(scale),
            flipped,
        });
        gpu.bounds.origin = point(ScaledPixels(origin[0]), ScaledPixels(origin[1]));
        gpu.bounds.size = size(
            ScaledPixels(SIDE as f32 - origin[0]),
            ScaledPixels(SIDE as f32 - origin[1]),
        );
        gpu.content_mask.bounds = gpu.bounds;
        frame.traces.highlighted_net = Some((NetId(1), [1.0; 4]));
        for solid in [false, true, false] {
            Arc::make_mut(&mut frame.display).static_shapes_fill_solid = solid;
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
            let pixels = target.read(&context);
            let camera = Camera {
                pixels_per_mm: zoom,
                ..frame.traces.camera.unwrap()
            };
            for y in origin[1] as usize..SIDE as usize {
                for x in origin[0] as usize..SIDE as usize {
                    let p = camera.view_to_board(
                        Point::new(
                            x as f64 + 0.5 - f64::from(origin[0]),
                            y as f64 + 0.5 - f64::from(origin[1]),
                        ),
                        f64::from(gpu.bounds.size.width.0),
                        f64::from(gpu.bounds.size.height.0),
                    );
                    let in_copper = p.x > -4.9 && p.x < 4.9 && p.y > -4.9 && p.y < -2.1;
                    let in_void = p.x > -2.6 && p.x < -0.5 && p.y > -4.1 && p.y < -2.9;
                    let near_void = p.x > -2.8 && p.x < -0.3 && p.y > -4.3 && p.y < -2.7;
                    if !in_copper || (!in_void && near_void) {
                        continue;
                    }
                    let mx = ((x as f32 + 0.5 - origin[0]).floor() as i32) & 15;
                    let my = ((y as f32 + 0.5 - origin[1]).floor() as i32) & 15;
                    // Independent analytic oracle for the recovered 109/256 bitmap:
                    // a radius-seven Manhattan diamond clipped to a 13x13 square.
                    let diamond = (1..=13).contains(&mx)
                        && (1..=13).contains(&my)
                        && (mx - 7).abs() + (my - 7).abs() <= 7;
                    let expected = if in_void || !diamond {
                        [0, 0, 99]
                    } else {
                        [99, 99, 160]
                    };
                    assert_eq!(
                        rgb(&pixels, x, y),
                        expected,
                        "persistent net pattern scale={scale} solid={solid} at {x},{y}, p={p:?}"
                    );
                    if in_void {
                        holes += 1;
                    } else if diamond {
                        ink += 1;
                    } else {
                        gaps += 1;
                    }
                }
            }
        }
    }
    assert!(ink > 1000 && gaps > 1000 && holes > 500);
    assert_eq!(
        before.lifetime_uploaded_bytes,
        telemetry.snapshot().lifetime_uploaded_bytes
    );
    assert_eq!(before.cache_builds, telemetry.snapshot().cache_builds);
    eprintln!(
        "ZONE_NETWORK_GPU kind={zone_kind:?} diamond_pixels={ink} gap_pixels={gaps} union_hole_pixels={holes} recovered_mask_109_over_256=true physical_period_16=true global_zero_visible=true shape_alpha_99=true base_replaced=true solid_switch_independent=true copper_reupload=false allegro_other_dpi_validated=false allegro_board_validated=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; controlled temporary static selection"]
fn hardware_stipple_static_object_selection_recolors_existing_points_without_densifying() {
    verify_temporary_selection(ZoneKind::Static);
}

#[test]
#[ignore = "requires Windows hardware D3D11; controlled temporary dynamic selection"]
fn hardware_dynamic_object_selection_uses_trace_style_white_dots() {
    verify_temporary_selection(ZoneKind::Dynamic);
}

fn verify_temporary_selection(kind: ZoneKind) {
    let (device, context) = device();
    let mut gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    // Isolate fill pixels from the independently tested continuous outlines.
    frame.zone_outlines = None;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
        if batch.object == ObjectId(60) {
            batch.kind = kind;
        } else {
            batch.net = NetId(2);
        }
    }
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let before = telemetry.snapshot();
    let mut ink = 0;
    let mut gaps = 0;
    let mut holes = 0;
    let mut neighbors = 0;
    for (scale, zoom, flipped, origin) in [
        (1.0, 10.0, false, [0.0, 0.0]),
        (1.25, 15.0, true, [5.0, 7.0]),
        (1.5, 20.0, false, [12.0, 8.0]),
        (2.0, 20.0, true, [3.25, 2.5]),
    ] {
        frame.traces.scale_factor = scale;
        frame.traces.camera = Some(Camera {
            center: Point::new(0.4, -2.0),
            pixels_per_mm: zoom / f64::from(scale),
            flipped,
        });
        gpu.bounds.origin = point(ScaledPixels(origin[0]), ScaledPixels(origin[1]));
        gpu.bounds.size = size(
            ScaledPixels(SIDE as f32 - origin[0]),
            ScaledPixels(SIDE as f32 - origin[1]),
        );
        gpu.content_mask.bounds = gpu.bounds;
        frame.traces.highlighted_object = None;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let baseline = target.read(&context);
        frame.traces.highlighted_object = Some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let selected = target.read(&context);
        let camera = Camera {
            pixels_per_mm: zoom,
            ..frame.traces.camera.unwrap()
        };
        for y in origin[1].ceil() as usize..SIDE as usize {
            for x in origin[0].ceil() as usize..SIDE as usize {
                let p = camera.view_to_board(
                    Point::new(
                        x as f64 + 0.5 - f64::from(origin[0]),
                        y as f64 + 0.5 - f64::from(origin[1]),
                    ),
                    f64::from(gpu.bounds.size.width.0),
                    f64::from(gpu.bounds.size.height.0),
                );
                let inside = p.x > -4.9 && p.x < 4.9 && p.y > -4.9 && p.y < -2.1;
                let void = p.x > -2.6 && p.x < -0.5 && p.y > -4.1 && p.y < -2.9;
                let near_void = p.x > -2.8 && p.x < -0.3 && p.y > -4.3 && p.y < -2.7;
                if !inside {
                    if p.x < -5.2 || p.x > 5.2 || p.y < -5.2 || p.y > -1.8 {
                        assert_eq!(
                            rgb(&selected, x, y),
                            rgb(&baseline, x, y),
                            "temporary selection changed neighbor/custom pad at {x},{y}"
                        );
                        neighbors += 1;
                    }
                    continue;
                }
                if !void && near_void {
                    continue;
                }
                let sx = x as f32 + 0.5 - origin[0];
                let sy = y as f32 + 0.5 - origin[1];
                let dynamic_alpha = trace_dot_alpha(x, y, scale);
                let dot = if kind == ZoneKind::Static {
                    let mx = ((sx / scale).floor() as i32) & 15;
                    let my = ((sy / scale).floor() as i32) & 15;
                    matches!((mx, my), (3 | 8, 3 | 14) | (0 | 11, 6 | 11))
                } else {
                    dynamic_alpha > 0.0
                };
                let expected = if kind == ZoneKind::Dynamic {
                    white_over(rgb(&baseline, x, y), if void { 0.0 } else { dynamic_alpha })
                } else if void {
                    [0, 0, 99]
                } else if dot {
                    [160, 99, 136]
                } else {
                    [0, 0, 99]
                };
                let actual = rgb(&selected, x, y);
                assert!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                    "temporary {kind:?} scale={scale} at {x},{y} p={p:?} dot={dot} actual={actual:?} expected={expected:?}"
                );
                if void {
                    holes += 1;
                } else if dot {
                    ink += 1;
                } else {
                    gaps += 1;
                }
            }
        }
        frame.traces.highlighted_object = None;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        assert_eq!(
            target.read(&context),
            baseline,
            "cancel must restore exact baseline"
        );
    }
    let after = telemetry.snapshot();
    assert!(ink > 100 && gaps > 1000 && holes > 500 && neighbors > 1000);
    assert_eq!(
        before.lifetime_uploaded_bytes,
        after.lifetime_uploaded_bytes
    );
    assert_eq!(before.cache_builds, after.cache_builds);
    eprintln!(
        "ZONE_OBJECT_GPU kind={kind:?} ink={ink} gaps={gaps} union_holes={holes} neighbor_pixels={neighbors} dpi_cases=4 cancel_restores=true shape_alpha=99 global_zero=true reupload=false reference_other_dpi_verified=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; controlled shape alpha slope"]
fn hardware_dynamic_object_selection_trace_dots_ignore_shape_opacity() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    frame.zone_outlines = None;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
        if batch.object == ObjectId(60) {
            batch.kind = ZoneKind::Dynamic;
        } else {
            batch.net = NetId(2);
        }
    }
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let before = telemetry.snapshot();
    let camera = frame.traces.camera.unwrap();
    let mut samples = 0;
    for alpha in [0_u8, 128, 255] {
        frame.copper_opacity = f32::from(alpha) / 255.0;
        frame.traces.highlighted_object = Some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let pixels = target.read(&context);
        let a = f64::from(alpha) / 255.0;
        let over = |dst: [u8; 3], src: [u8; 3]| -> [u8; 3] {
            std::array::from_fn(|c| {
                (f64::from(src[c]) * a + f64::from(dst[c]) * (1.0 - a)).round() as u8
            })
        };
        let lower = [0, 0, alpha];
        let base = over(lower, [255, 0, 0]);
        for y in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                let p = camera.view_to_board(
                    Point::new(x as f64 + 0.5, y as f64 + 0.5),
                    f64::from(SIDE),
                    f64::from(SIDE),
                );
                let inside = p.x > -4.9 && p.x < 4.9 && p.y > -4.9 && p.y < -2.1;
                let void = p.x > -2.6 && p.x < -0.5 && p.y > -4.1 && p.y < -2.9;
                let near_void = p.x > -2.8 && p.x < -0.3 && p.y > -4.3 && p.y < -2.7;
                if !inside || (!void && near_void) {
                    continue;
                }
                let dot_alpha = trace_dot_alpha(x, y, 1.0);
                let expected = if void {
                    lower
                } else {
                    white_over(base, dot_alpha)
                };
                let actual = rgb(&pixels, x, y);
                assert!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                    "shape alpha={alpha} at {x},{y} actual={actual:?} expected={expected:?}"
                );
                samples += 1;
            }
        }
    }
    assert!(samples > 5000);
    assert_eq!(
        before.lifetime_uploaded_bytes,
        telemetry.snapshot().lifetime_uploaded_bytes
    );
    eprintln!(
        "ZONE_OBJECT_ALPHA_GPU samples={samples} shape_alpha_cases=0,128,255 global_zero=true trace_style_dots=true gaps_unchanged=true reupload=false"
    );
}

// Match the existing trace selection's logical five-pixel grid and 0.9 alpha.
fn trace_dot_alpha(x: usize, y: usize, scale: f32) -> f64 {
    let cell = |p: usize| ((p as f64 + 0.5) / f64::from(scale)).rem_euclid(5.0) - 2.5;
    let distance = cell(x).hypot(cell(y));
    let t = ((distance - 0.65) / 0.6).clamp(0.0, 1.0);
    (1.0 - t * t * (3.0 - 2.0 * t)) * 0.9
}
fn white_over(base: [u8; 3], alpha: f64) -> [u8; 3] {
    base.map(|value| (255.0 * alpha + f64::from(value) * (1.0 - alpha)).round() as u8)
}
