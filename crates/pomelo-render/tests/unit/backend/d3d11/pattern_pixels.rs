//! Hardware proof of screen-space material masks, independent of source/stencil geometry.
use super::compositor_parity::{device, fixture, gpu_context};
use super::*;
use crate::{
    backend::{BoardRenderer, CopperTelemetry},
    copper::{CopperLimits, PreparedCopper},
};
use pomelo_core::{
    appearance::{ColorTarget, pattern::FillPattern},
    copper::{CopperMesh, MeshLimits},
    model::{Zone, ZoneKind},
};
#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_material_patterns_keep_logical_pixel_phase_holes_and_geometry_residency() {
    let (device, context) = device();
    let mut gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let cancel = CancellationToken::default();
    for hole in [false, true] {
        let mut rings = vec![vec![
            Point::new(-5.0, -5.0),
            Point::new(5.0, -5.0),
            Point::new(5.0, -2.0),
            Point::new(-5.0, -2.0),
        ]];
        if hole {
            rings.push(vec![
                Point::new(-3.0, -4.0),
                Point::new(-1.0, -4.0),
                Point::new(-1.0, -3.0),
                Point::new(-3.0, -3.0),
            ]);
        }
        let zone = Zone {
            id: ObjectId(91),
            layer: LayerId(0),
            net: NetId(0),
            kind: ZoneKind::Static,
            paths: vec![],
            mesh: CopperMesh::build(&rings, &[], &MeshLimits::default(), &cancel).unwrap(),
        };
        let mut frame = fixture();
        frame.copper =
            Arc::new(PreparedCopper::build(&[zone], CopperLimits::default(), &cancel).unwrap());
        frame.layer_order = Arc::new(vec![LayerId(0)]);
        Arc::make_mut(&mut frame.display).global_opacity = 0.0;
        let telemetry = Arc::new(CopperTelemetry::default());
        let trace = || Arc::new(TraceTelemetry::default());
        let mut renderer = BoardRenderer::new(
            trace(),
            telemetry.clone(),
            trace(),
            Arc::new(CopperTelemetry::default()),
            trace(),
            trace(),
            trace(),
        );
        for _ in 0..16 {
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
        }
        let uploaded = telemetry.snapshot().lifetime_uploaded_bytes;
        for (dpi, origin) in [(1.0, [0.0, 0.0]), (1.5, [5.0, 7.0])] {
            gpu.bounds.origin = point(ScaledPixels(origin[0]), ScaledPixels(origin[1]));
            gpu.bounds.size = size(
                ScaledPixels(SIDE as f32 - origin[0]),
                ScaledPixels(SIDE as f32 - origin[1]),
            );
            gpu.content_mask.bounds = gpu.bounds;
            frame.traces.scale_factor = dpi;
            for flipped in [false, true] {
                frame.traces.camera.as_mut().unwrap().flipped = flipped;
                for pattern in FillPattern::ALL {
                    Arc::make_mut(&mut frame.display)
                        .appearance
                        .set_pattern(ColorTarget::Etch(LayerId(0)), Some(pattern));
                    target.bind(&context);
                    renderer.draw(&gpu, &frame).unwrap();
                    let pixels = target.read(&context);
                    for y in if dpi == 1.0 { 90..112 } else { 104..124 } {
                        for x in if flipped { 40..56 } else { 90..110 } {
                            let cell =
                                |v: usize, o: f32| ((v as f32 + 0.5 - o) / dpi).floor() as u32;
                            let expected = if pattern.ink(cell(x, origin[0]), cell(y, origin[1])) {
                                [99, 0, 0]
                            } else {
                                [0; 3]
                            };
                            assert_eq!(
                                rgb(&pixels, x, y),
                                expected,
                                "hole={hole} dpi={dpi} flip={flipped} pattern={pattern:?} x={x} y={y}"
                            );
                        }
                    }
                    if hole {
                        let camera = frame.traces.camera.unwrap();
                        let mut physical = camera;
                        physical.pixels_per_mm *= f64::from(dpi);
                        let p = physical.board_to_view(
                            Point::new(-2.0, -3.5),
                            f64::from(gpu.bounds.size.width.0),
                            f64::from(gpu.bounds.size.height.0),
                        );
                        let x = (p.x + f64::from(origin[0])).floor() as usize;
                        let y = (p.y + f64::from(origin[1])).floor() as usize;
                        if x < SIDE as usize && y < SIDE as usize {
                            assert_eq!(
                                rgb(&pixels, x, y),
                                [0; 3],
                                "pattern must not fill actual holes"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(
            telemetry.snapshot().lifetime_uploaded_bytes,
            uploaded,
            "material changes must not re-upload source geometry"
        );
    }
}
#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_pin_and_via_patterns_are_independent_of_etch_and_drill_centers() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut frame = fixture();
    let mut renderer = super::compositor_parity::renderer();
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    Arc::make_mut(&mut frame.display)
        .appearance
        .set_pattern(ColorTarget::Pin(LayerId(2)), Some(FillPattern::Horizontal));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    assert_eq!(rgb(&pixels, 44, 70), [0; 3]);
    assert_eq!(rgb(&pixels, 44, 72), [0, 255, 0]);
    assert_eq!(rgb(&pixels, 44, 60), [255, 255, 0]);
    Arc::make_mut(&mut frame.display)
        .appearance
        .set_pattern(ColorTarget::Via(LayerId(0)), Some(FillPattern::Vertical));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    assert_eq!(rgb(&pixels, 44, 60), [255, 255, 0]);
    assert_eq!(rgb(&pixels, 45, 60), [0, 255, 0]);
    assert_eq!(
        rgb(&pixels, 44, 64),
        [51; 3],
        "physical drill center remains readable"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_embedded_copper_labels_remain_continuous_over_material_gaps() {
    use pomelo_core::{
        display::DisplayCategory,
        model::{BoardText, TextAlignment},
    };
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut frame = fixture();
    frame.layer_order = Arc::new(vec![LayerId(0)]);
    Arc::make_mut(&mut frame.display)
        .hidden_layers
        .insert(LayerId(2));
    let cancel = CancellationToken::default();
    let source = BoardText {
        id: ObjectId(60),
        owner_id: None,
        layer: LayerId(0),
        class_id: 6,
        subclass: 0,
        text: "GND".into(),
        at: Point::new(-4.0, -4.8),
        angle: 0.0,
        mirrored: false,
        align: TextAlignment::Left,
        font_index: 0,
        width: 0.65,
        height: 1.0,
        spacing: 0.0,
        line_spacing: 1.0,
        stroke_width: 0.0,
    };
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["GND"], &cancel).unwrap());
    let mut glyphs =
        crate::text::msdf::PreparedGlyphs::build(&[source], font, 1024 * 1024, &cancel).unwrap();
    // The fixture zone is object 60: label ownership must still select its stencil.
    for glyph in &mut glyphs.instances {
        glyph.ids[1] = DisplayCategory::Zone as u32;
        glyph.rotation[3] += 1.0;
        glyph.color = [1.0; 4];
    }
    frame.labels = Some(Arc::new(glyphs));
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    let mut renderer = super::compositor_parity::renderer();
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let solid = target.read(&context);
    let pattern = FillPattern::Horizontal;
    Arc::make_mut(&mut frame.display)
        .appearance
        .set_pattern(ColorTarget::Etch(LayerId(0)), Some(pattern));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let patterned = target.read(&context);
    let mut gap_ink = 0;
    for y in 100..113 {
        for x in 20..50 {
            let before = rgb(&solid, x, y);
            if !pattern.ink(x as u32, y as u32) && before[1] > 30 {
                let after = rgb(&patterned, x, y);
                assert_eq!(
                    after[1..],
                    before[1..],
                    "embedded label was stippled at {x},{y}"
                );
                gap_ink += 1;
            }
        }
    }
    assert!(
        gap_ink > 10,
        "fixture must contain readable label pixels in material gaps"
    );
}
