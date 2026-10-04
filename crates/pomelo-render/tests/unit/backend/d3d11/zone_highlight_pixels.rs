//! Hardware proof of original zone fill plus white outlines and sparse white dots.
use super::compositor_parity::{device, gpu_context};
use super::static_shape::{prepared_frame, renderer};
use super::*;
use crate::backend::CopperTelemetry;
use pomelo_core::{interaction::Camera, model::ZoneKind, selection::SelectedObject};

#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_zone_highlight_preserves_fill_holes_and_adds_sparse_white_dots() {
    let (device, context) = device();
    let mut gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    frame.zone_outlines = None;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    let mut dots = 0;
    let mut gaps = 0;
    let mut holes = 0;
    for kind in [ZoneKind::Static, ZoneKind::Dynamic, ZoneKind::Unknown] {
        frame.copper = prepared_frame().copper;
        for batch in &mut Arc::get_mut(&mut frame.copper).unwrap().batches {
            if batch.object == ObjectId(60) {
                batch.kind = kind;
            } else {
                batch.net = NetId(2);
            }
        }
        for solid in [false, true] {
            Arc::make_mut(&mut frame.display).static_shapes_fill_solid = solid;
            for (scale, zoom, origin) in [(1.0, 10.0, [0.0, 0.0]), (1.5, 20.0, [5.0, 7.0])] {
                frame.traces.scale_factor = scale;
                frame.traces.camera = Some(Camera {
                    center: Point::new(0.4, -2.0),
                    pixels_per_mm: zoom / f64::from(scale),
                    flipped: scale > 1.0,
                });
                gpu.bounds.origin = point(ScaledPixels(origin[0]), ScaledPixels(origin[1]));
                gpu.bounds.size = size(
                    ScaledPixels(SIDE as f32 - origin[0]),
                    ScaledPixels(SIDE as f32 - origin[1]),
                );
                gpu.content_mask.bounds = gpu.bounds;
                frame.traces.highlighted_object = None;
                frame.traces.highlighted_net = None;
                frame.traces.hovered_object = None;
                for _ in 0..16 {
                    target.bind(&context);
                    renderer.draw(&gpu, &frame).unwrap();
                }
                let baseline = target.read(&context);
                let before = telemetry.snapshot();
                for mode in 0..3 {
                    frame.traces.highlighted_object =
                        (mode == 0).then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
                    frame.traces.hovered_object =
                        (mode == 1).then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
                    frame.traces.highlighted_net = (mode == 2).then_some((NetId(1), [1.0; 4]));
                    target.bind(&context);
                    renderer.draw(&gpu, &frame).unwrap();
                    let highlighted = target.read(&context);
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
                            let hole = p.x > -2.6 && p.x < -0.5 && p.y > -4.1 && p.y < -2.9;
                            let near_hole = p.x > -2.8 && p.x < -0.3 && p.y > -4.3 && p.y < -2.7;
                            if !inside || (!hole && near_hole) {
                                continue;
                            }
                            let cell = |coord: usize, offset: f32| {
                                ((coord as f64 + 0.5 - f64::from(offset)) / f64::from(scale))
                                    .rem_euclid(12.0)
                                    - 6.0
                            };
                            let distance = cell(x, origin[0]).hypot(cell(y, origin[1]));
                            let t = ((distance - 0.65) / 0.6).clamp(0.0, 1.0);
                            let alpha = if hole {
                                0.0
                            } else {
                                (1.0 - t * t * (3.0 - 2.0 * t)) * 0.9
                            };
                            let expected = rgb(&baseline, x, y).map(|v| {
                                (255.0 * alpha + f64::from(v) * (1.0 - alpha)).round() as u8
                            });
                            let actual = rgb(&highlighted, x, y);
                            assert!(
                                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                                "kind={kind:?} solid={solid} mode={mode} scale={scale} pixel={x},{y} actual={actual:?} expected={expected:?}"
                            );
                            if hole {
                                holes += 1;
                            } else if alpha > 0.0 {
                                dots += 1;
                            } else {
                                gaps += 1;
                            }
                        }
                    }
                }
                frame.traces.highlighted_object = None;
                frame.traces.highlighted_net = None;
                frame.traces.hovered_object = None;
                target.bind(&context);
                renderer.draw(&gpu, &frame).unwrap();
                assert_eq!(
                    target.read(&context),
                    baseline,
                    "clearing highlight restores fill"
                );
                assert_eq!(
                    before.lifetime_uploaded_bytes,
                    telemetry.snapshot().lifetime_uploaded_bytes
                );
            }
        }
    }
    assert!(dots > 100 && gaps > 1000 && holes > 500);
}

#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_zone_selection_and_hover_outlines_are_white() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = renderer(&telemetry);
    let mut frame = prepared_frame();
    Arc::make_mut(&mut frame.display).static_shapes_fill_solid = true;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let camera = frame.traces.camera.unwrap();
    for hover in [false, true] {
        frame.traces.highlighted_object =
            (!hover).then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        frame.traces.hovered_object =
            hover.then_some((SelectedObject::Zone(ObjectId(60)), [1.0; 4]));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let pixels = target.read(&context);
        let mut border = 0;
        for y in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                let p = camera.view_to_board(
                    Point::new(x as f64 + 0.5, y as f64 + 0.5),
                    f64::from(SIDE),
                    f64::from(SIDE),
                );
                if p.x > -4.8 && p.x < 4.8 && (p.y + 2.0).abs() < 0.06 {
                    assert!(
                        rgb(&pixels, x, y).iter().all(|v| *v > 180),
                        "hover={hover} border={x},{y}"
                    );
                    border += 1;
                }
            }
        }
        assert!(border > 50);
    }
}
