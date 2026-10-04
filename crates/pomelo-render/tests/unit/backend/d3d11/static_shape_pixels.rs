//! Hardware stipple proofs use real solid coverage for holes and annotations.
use super::compositor_parity::{device, fixture, gpu_context};
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    display::DisplayCategory,
    interaction::Camera,
    model::{
        BoardText, CustomPadGeometry, DrillShape, Pad, PadKind, Pin, TextAlignment, Zone, ZoneKind,
    },
};

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<Point> {
    vec![
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ]
}

fn zones() -> [Zone; 2] {
    let cancel = CancellationToken::default();
    [ZoneKind::Dynamic, ZoneKind::Static].map(|kind| {
        let mut rings = vec![rectangle(-5.0, -5.0, 10.0, 3.0)];
        if kind == ZoneKind::Static {
            // Two overlapping voids must form a union even though the fill is sparse.
            rings.extend([
                rectangle(-2.7, -4.2, 1.4, 1.4),
                rectangle(-1.8, -4.2, 1.4, 1.4),
            ]);
        }
        Zone {
            id: ObjectId(if kind == ZoneKind::Static { 60 } else { 62 }),
            kind,
            layer: LayerId(if kind == ZoneKind::Static { 0 } else { 2 }),
            net: NetId(1),
            paths: vec![],
            mesh: pomelo_core::copper::CopperMesh::build(
                &rings,
                &[],
                &pomelo_core::copper::MeshLimits::default(),
                &cancel,
            )
            .unwrap(),
        }
    })
}

pub(super) fn prepared_frame() -> BoardFrame {
    let cancel = CancellationToken::default();
    let source = zones();
    let mut frame = fixture();
    frame.copper = Arc::new(
        crate::copper::PreparedCopper::build(
            &source,
            crate::copper::CopperLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    frame.zone_outlines = Some(Arc::new(
        PreparedTracks::build_zone_outlines(&source, TraceLimits::default(), &cancel).unwrap(),
    ));
    // A custom pin uses the same polygon pipeline; its fill must stay solid.
    let pin = Pin {
        id: ObjectId(70),
        owner_id: ObjectId(69),
        net: NetId(1),
        name: String::new(),
        reference: String::new(),
        at: Point::new(0.0, 2.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        pads: vec![Pad {
            kind: PadKind::CUSTOM,
            custom: Some(Arc::new(CustomPadGeometry {
                contours: vec![rectangle(-1.0, -1.0, 2.0, 2.0)],
                paths: vec![],
            })),
            ..Pad::circle(LayerId(0), 2.0)
        }],
        stackup_region: None,
        die: None,
    };
    let mut pads =
        crate::pads::PreparedPads::build(&[pin], &[], crate::pads::PadLimits::default(), &cancel)
            .unwrap();
    let mut custom = pads
        .build_custom_meshes(
            crate::copper::CopperLimits::default(),
            &pomelo_core::copper::MeshLimits::default(),
            &cancel,
        )
        .unwrap();
    // Protect custom geometry even if its tag is accidentally set Static.
    for batch in &mut custom.batches {
        batch.kind = ZoneKind::Static;
    }
    pads.custom_mesh = Some(Arc::new(custom));
    frame.pads = Some(Arc::new(pads));
    frame.drills = None;
    frame
}

pub(super) fn renderer(telemetry: &Arc<CopperTelemetry>) -> BoardRenderer {
    BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::clone(telemetry),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    )
}

fn is_dot(x: usize, y: usize) -> bool {
    matches!((x & 15, y & 15), (3 | 8, 3 | 14) | (0 | 11, 6 | 11))
}

#[test]
#[ignore = "requires Windows hardware D3D11; synthetic coverage does not establish Allegro board parity"]
fn hardware_static_shape_stipple_keeps_solid_geometry_and_custom_pads_without_reupload() {
    let (device, context) = device();
    let mut gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    let outlines = frame.zone_outlines.take();
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let before = telemetry.snapshot();
    let mut points = 0;
    let mut gaps = 0;
    let mut voids = 0;
    let mut borders = 0;
    for (scale, zoom, flipped, origin) in [
        (1.0, 10.0, false, [0.0, 0.0]),
        (2.0, 10.0, true, [0.0, 0.0]),
        (1.5, 20.0, false, [12.0, 8.0]),
        (1.25, 15.0, true, [3.25, 2.5]),
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
        for solid in [false, true, false] {
            Arc::make_mut(&mut frame.display).static_shapes_fill_solid = solid;
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
            let pixels = target.read(&context);
            let camera = Camera {
                pixels_per_mm: zoom,
                ..frame.traces.camera.unwrap()
            };
            for y in 0..SIDE as usize {
                for x in 0..SIDE as usize {
                    if (x as f32) < origin[0] || (y as f32) < origin[1] {
                        continue;
                    }
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
                    if in_copper && (in_void || !near_void) {
                        // D3D11 samples physical pixel centers; the Allegro
                        // stipple is anchored in logical canvas pixels at any DPI.
                        let pattern_x = ((x as f32 + 0.5 - origin[0]) / scale).floor();
                        let pattern_y = ((y as f32 + 0.5 - origin[1]) / scale).floor();
                        let dot = is_dot(pattern_x as usize, pattern_y as usize);
                        let expected = if in_void || (!solid && !dot) {
                            [0, 0, 99]
                        } else {
                            [99, 0, 61]
                        };
                        assert_eq!(
                            rgb(&pixels, x, y),
                            expected,
                            "scale={scale}, zoom={zoom}, flipped={flipped}, origin={origin:?}, solid={solid}, p={p:?}, pixel={x},{y}"
                        );
                        if in_void {
                            voids += 1;
                        } else if dot {
                            points += 1;
                        } else {
                            gaps += 1;
                        }
                    }
                    if p.x.abs() < 0.8 && (p.y - 2.0).abs() < 0.8 {
                        assert_eq!(
                            rgb(&pixels, x, y),
                            [255, 0, 0],
                            "custom pin must remain solid regardless of static fill switch"
                        );
                    }
                }
            }
        }
        // Verify the independent continuous boundary after the fill-only oracle,
        // avoiding its antialias band and overlapping contour edges in fill samples.
        frame.zone_outlines = outlines.clone();
        for _ in 0..2 {
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
        }
        let boundary_pixels = target.read(&context);
        let camera = Camera {
            pixels_per_mm: zoom,
            ..frame.traces.camera.unwrap()
        };
        for y in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                if (x as f32) < origin[0] || (y as f32) < origin[1] {
                    continue;
                }
                let p = camera.view_to_board(
                    Point::new(
                        x as f64 + 0.5 - f64::from(origin[0]),
                        y as f64 + 0.5 - f64::from(origin[1]),
                    ),
                    f64::from(gpu.bounds.size.width.0),
                    f64::from(gpu.bounds.size.height.0),
                );
                if p.x > -4.8 && p.x < 4.8 && (p.y + 2.0).abs() < 0.06 {
                    assert!(
                        rgb(&boundary_pixels, x, y)[0] > 90,
                        "continuous outline disappeared at {x},{y}"
                    );
                    borders += 1;
                }
            }
        }
        frame.zone_outlines = None;
    }
    assert!(
        points > 30 && gaps > 1000 && voids > 100 && borders > 0,
        "fixture did not exercise dots/gaps/voids/borders: {points}/{gaps}/{voids}/{borders}"
    );
    let after = telemetry.snapshot();
    assert_eq!(
        (
            after.pipeline_builds,
            after.cache_builds,
            after.lifetime_uploaded_bytes
        ),
        (
            before.pipeline_builds,
            before.cache_builds,
            before.lifetime_uploaded_bytes
        )
    );
    eprintln!(
        "STATIC_STIPPLE_GPU dots={points} gaps={gaps} union_void={voids} border={borders} custom_pin_solid=true fixed_logical_pattern=true flipped=true scale_1_1p25_1p5_2=true fractional_canvas_origin=true toggle_geometry_reupload=false allegro_board_validated=false"
    );
}

#[test]
#[ignore = "requires Windows hardware D3D11; verifies label stencil independently of sparse fill"]
fn hardware_static_shape_stipple_keeps_labels_in_full_zone_and_out_of_voids() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    // Omit outlines so unchanged void pixels only measure the zone mask.
    frame.zone_outlines = None;
    let cancel = CancellationToken::default();
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["G"], &cancel).unwrap());
    let mut glyphs = crate::text::msdf::PreparedGlyphs::build(
        &[BoardText {
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
        }],
        font,
        1024 * 1024,
        &cancel,
    )
    .unwrap();
    for glyph in &mut glyphs.instances {
        glyph.ids[1] = DisplayCategory::Zone as u32;
    }
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let background = target.read(&context);
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
                    "zone label escaped its full geometry stencil at {x},{y}"
                );
                masked += 1;
            } else if p.x > -4.9
                && p.x < 4.9
                && p.y > -4.9
                && p.y < -2.1
                && !is_dot(x, y)
                && rgb(&labelled, x, y) != rgb(&background, x, y)
            {
                gap_ink += 1;
            }
        }
    }
    assert!(
        gap_ink > 30 && masked > 1000,
        "full clipped label must appear between stipple pixels: {gap_ink}/{masked}"
    );
    eprintln!(
        "STATIC_STIPPLE_LABEL_GPU ink_in_gaps={gap_ink} masked_pixels={masked} stencil_solid=true union_void_preserved=true allegro_board_validated=false"
    );
}
