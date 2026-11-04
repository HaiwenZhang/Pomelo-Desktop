//! Display-filter proof on the real native command path, including logical DPI.
use super::compositor_parity::{device, fixture, gpu_context, renderer};
use super::*;
use pomelo_core::display::filter::ObjectFilter;
#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_object_filter_keeps_categories_independent_and_uses_logical_pixels() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer();
    let mut frame = fixture();
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    assert_eq!(rgb(&target.read(&context), 44, 70), [0, 255, 0]);
    let filter = ObjectFilter {
        minimum_pixels: 21,
        vias: false,
        copper: false,
        ..Default::default()
    };
    Arc::make_mut(&mut frame.display).object_filter = filter;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 44, 70),
        [0; 3],
        "20px pin must be filtered"
    );
    assert_eq!(
        rgb(&target.read(&context), 44, 60),
        [255, 255, 0],
        "via filtering is disabled"
    );
    frame.traces.camera.as_mut().unwrap().pixels_per_mm = 20.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 24, 76),
        [0, 255, 0],
        "zoom makes the pin visible again"
    );
    frame.traces.camera.as_mut().unwrap().pixels_per_mm = 10.0;
    frame.traces.scale_factor = 2.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 24, 76),
        [0; 3],
        "DPI alone must not pass a logical-pixel filter"
    );
    frame.traces.scale_factor = 1.0;
    Arc::make_mut(&mut frame.display).object_filter = ObjectFilter {
        minimum_pixels: 31,
        pins: false,
        vias: false,
        ..Default::default()
    };
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 90, 100),
        [0; 3],
        "30px copper region must be filtered"
    );
    Arc::make_mut(&mut frame.display).object_filter = ObjectFilter::default();
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_ne!(
        rgb(&target.read(&context), 90, 100),
        [0; 3],
        "turning the filter off restores original copper"
    );
}
#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_source_text_filters_by_whole_em_height_and_restores_without_upload() {
    use pomelo_core::model::{BoardText, TextAlignment};
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let cancel = CancellationToken::default();
    let text = BoardText {
        id: ObjectId(99),
        owner_id: None,
        layer: LayerId(0),
        class_id: 6,
        subclass: 0,
        text: "GND".into(),
        at: Point::new(-2.0, 0.0),
        angle: 0.0,
        mirrored: false,
        align: TextAlignment::Left,
        font_index: 0,
        width: 1.0,
        height: 1.0,
        spacing: 0.0,
        line_spacing: 1.0,
        stroke_width: 0.1,
    };
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["GND"], &cancel).unwrap());
    let source = Arc::new(
        crate::text::msdf::PreparedGlyphs::build([&text], font, 1024 * 1024, &cancel).unwrap(),
    );
    let mut frame = fixture().traces.with_source(source);
    let telemetry = Arc::new(TraceTelemetry::default());
    let mut renderer = TraceRenderer::new(telemetry.clone());
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let original = target.read(&context);
    assert!(
        original
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0; 3])
    );
    let uploads = telemetry.snapshot().uploaded_bytes;
    frame.object_filter.minimum_pixels = 11;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(
        target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0; 3])
    );
    frame.object_filter.text = false;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(target.read(&context), original);
    assert_eq!(telemetry.snapshot().uploaded_bytes, uploads);
}

#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_void_filter_changes_masks_and_restores_holes_in_static_and_curve_sources() {
    use crate::{
        copper::{CopperLimits, PreparedCopper},
        scene::curves::{
            CurveCacheLimits, CurveFillCache, CurveView,
            tests::{board, circle_path, circle_ring, lines, zone},
        },
    };
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let cancel = CancellationToken::default();
    for curved in [false, true] {
        let mut frame = fixture();
        let mut renderer = renderer();
        let outer = vec![
            Point::new(-5.0, -5.0),
            Point::new(5.0, -5.0),
            Point::new(5.0, -2.0),
            Point::new(-5.0, -2.0),
        ];
        let at = Point::new(-2.0, -3.5);
        let hole = circle_ring(at, 0.7);
        let source_zone = zone(
            60,
            vec![outer.clone(), hole.clone()],
            vec![
                lines(&outer),
                if curved {
                    circle_path(at, 0.7)
                } else {
                    lines(&hole)
                },
            ],
        );
        let scene = board(vec![source_zone]);
        frame.layer_order = Arc::new(vec![LayerId(1)]);
        frame.zone_outlines = Some(Arc::new(
            PreparedTracks::build_zone_outlines(&scene.zones, TraceLimits::default(), &cancel)
                .unwrap(),
        ));
        frame.copper = Arc::new(
            PreparedCopper::build(&scene.zones, CopperLimits::default(), &cancel).unwrap(),
        );
        if curved {
            // Explicit low-scale refinement isolates the curved mask branch of the filter.
            let request = CurveView {
                bounds: Bounds {
                    min: Point::new(-6.0, -6.0),
                    max: Point::new(6.0, 6.0),
                },
                tolerance: 0.005,
            };
            frame.curves = Some(Arc::new(
                CurveFillCache::prepare(
                    &scene,
                    request,
                    &frame.display,
                    &CurveFillCache::default(),
                    CurveCacheLimits::default(),
                    &cancel,
                )
                .unwrap(),
            ));
        }
        for _ in 0..16 {
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
        }
        assert_eq!(
            rgb(&target.read(&context), 44, 99),
            [0; 3],
            "curved={curved}: original hole"
        );
        Arc::make_mut(&mut frame.display).object_filter = ObjectFilter {
            minimum_pixels: 15,
            ..Default::default()
        };
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        assert_ne!(
            rgb(&target.read(&context), 44, 99),
            [0; 3],
            "curved={curved}: sub-threshold hole must be filled"
        );
        Arc::make_mut(&mut frame.display).object_filter.voids = false;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        assert_eq!(
            rgb(&target.read(&context), 44, 99),
            [0; 3],
            "curved={curved}: hole category restored"
        );
    }
}
