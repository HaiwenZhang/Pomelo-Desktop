//! Hardware-only diagnostic readback. Never compiled into the production renderer.

use super::*;
use crate::{
    backend::d3d11::{TraceRenderer, TraceTelemetry},
    tracks::TraceLimits,
};
use gpui::NativeGpuRenderer;
use gpui::{Bounds as ViewBounds, ContentMask, ScaledPixels, point, size};
use pomelo_core::{
    model::{Arc as BoardArc, Bounds, NetId, ObjectId, Point, Segment},
    task::CancellationToken,
};
use std::collections::BTreeMap;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Dxgi::{
                Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
                IDXGIAdapter, IDXGIDevice,
            },
        },
    },
    core::Interface,
};

const SIDE: u32 = 128;

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_copper_overlapping_holes_preserve_underlying_trace() {
    use crate::backend::d3d11::{CopperRenderer, CopperTelemetry};
    use crate::copper::{CopperLimits, PreparedCopper};
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::Zone,
    };
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs; hardware device only, with no software fallback.
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let device = device.unwrap();
    let context = context.unwrap();
    let square = |x: f64, y: f64, side: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + side, y),
            Point::new(x + side, y + side),
            Point::new(x, y + side),
        ]
    };
    let cancellation = CancellationToken::default();
    let zone = Zone {
        id: ObjectId(2),
        layer: LayerId(2),
        net: NetId(1),
        paths: vec![],
        mesh: CopperMesh::build(
            &[
                square(1.0, 1.0, 18.0),
                square(4.0, 4.0, 8.0),
                square(8.0, 8.0, 8.0),
            ],
            &[],
            &MeshLimits::default(),
            &cancellation,
        )
        .unwrap(),
    };
    let copper = Arc::new(
        PreparedCopper::build(
            std::slice::from_ref(&zone),
            CopperLimits::default(),
            &cancellation,
        )
        .unwrap(),
    );
    let line = Segment {
        id: ObjectId(1),
        track_id: ObjectId(1),
        layer: LayerId(1),
        net: NetId(1),
        a: Point::new(2.0, 10.0),
        b: Point::new(18.0, 10.0),
        width: 1.0,
        arc: None,
        bond_wire: None,
    };
    let mut frame = TraceFrame {
        tracks: Arc::new(
            PreparedTracks::build(&[line], TraceLimits::default(), &cancellation).unwrap(),
        ),
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(20.0, 20.0),
        },
        camera: None,
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::from([
            (LayerId(1), [1.0, 0.0, 0.0, 1.0]),
            (LayerId(2), [0.0, 1.0, 0.0, 1.0]),
            (LayerId(3), [0.0, 0.0, 1.0, 1.0]),
        ])),
        fallback_color: [1.0; 4],
        highlighted_net: None,
        highlighted_objects: None,
        highlighted_related_objects: None,
        hovered_object: None,
        highlighted_trace: None,
        highlighted_object: None,
    };
    let bounds = ViewBounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    };
    let mut gpu = NativeGpuContext {
        device: &device,
        context: &context,
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let target = Target::new(&device);
    let mut traces = TraceRenderer::<PreparedTracks>::new(Arc::new(TraceTelemetry::default()));
    let telemetry = Arc::new(CopperTelemetry::default());
    let mut renderer = CopperRenderer::new(Arc::clone(&telemetry));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 0.5, true).unwrap();
    let pixels = target.read(&context);
    assert_eq!(
        rgb(&pixels, 64, 64),
        [255, 0, 0],
        "overlapping holes must retain the underlying trace"
    );
    assert_eq!(
        rgb(&pixels, 40, 87),
        [0, 0, 0],
        "single-hole coverage must stay transparent"
    );
    let fill = rgb(&pixels, 23, 105);
    assert!(
        fill[0] == 0 && (126..=129).contains(&fill[1]) && fill[2] == 0,
        "half-opacity copper: {fill:?}"
    );
    let copper_before_highlight = telemetry.snapshot();
    frame.highlighted_net = Some((NetId(1), [1.0, 0.0, 1.0, 1.0]));
    target.bind(&context);
    renderer.draw(&gpu, &frame, &copper, 0.5, false).unwrap();
    let copper_highlight = target.read(&context);
    let tinted_fill = rgb(&copper_highlight, 23, 105);
    assert!(
        (126..=129).contains(&tinted_fill[0])
            && tinted_fill[1] == 0
            && (126..=129).contains(&tinted_fill[2])
    );
    assert_eq!(rgb(&copper_highlight, 64, 64), [0; 3]);
    frame.highlighted_net = None;
    frame.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Zone(ObjectId(2)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 1.0, false).unwrap();
    let selected_zone = target.read(&context);
    assert_eq!(rgb(&selected_zone, 23, 105), [0, 255, 255]);
    assert_eq!(rgb(&selected_zone, 64, 64), [255, 0, 0]);
    assert_eq!(rgb(&selected_zone, 40, 87), [0; 3]);
    frame.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(2)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 0.5, false).unwrap();
    assert_eq!(target.read(&context), pixels);
    frame.highlighted_object = None;
    frame.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Zone(ObjectId(2)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 1.0, false).unwrap();
    let hovered_zone = target.read(&context);
    assert_eq!(rgb(&hovered_zone, 23, 105), [255, 0, 255]);
    assert_eq!(rgb(&hovered_zone, 64, 64), [255, 0, 0]);
    assert_eq!(rgb(&hovered_zone, 40, 87), [0; 3]);
    frame.highlighted_net = Some((NetId(1), [0.0, 1.0, 1.0, 1.0]));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 1.0, false).unwrap();
    assert_eq!(rgb(&target.read(&context), 23, 105), [0, 255, 255]);
    frame.highlighted_net = None;
    frame.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(2)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 0.5, false).unwrap();
    assert_eq!(target.read(&context), pixels);
    frame.hovered_object = None;
    assert_eq!(
        copper_before_highlight.lifetime_uploaded_bytes,
        telemetry.snapshot().lifetime_uploaded_bytes
    );
    assert_eq!(
        copper_before_highlight.cache_builds,
        telemetry.snapshot().cache_builds
    );
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 0.5, false).unwrap();
    assert_eq!(
        target.read(&context),
        pixels,
        "cached draw must preserve all pixels"
    );
    assert_eq!(telemetry.snapshot().cache_builds, 1);
    assert_eq!(
        telemetry.snapshot().uploaded_bytes,
        copper.upload_bytes() as u64
    );
    renderer.reset();
    assert_eq!(telemetry.snapshot().uploaded_bytes, 0);
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &copper, 0.5, true).unwrap();
    assert_eq!(
        target.read(&context),
        pixels,
        "resource reset must reconstruct the same coverage"
    );
    assert_eq!(telemetry.snapshot().pipeline_builds, 2);
    assert_eq!(telemetry.snapshot().cache_builds, 2);
    assert_eq!(telemetry.snapshot().resets, 1);

    let overlay = Zone {
        id: ObjectId(3),
        layer: LayerId(3),
        net: NetId(1),
        paths: vec![],
        mesh: CopperMesh::build(
            &[square(9.0, 9.0, 2.0)],
            &[],
            &MeshLimits::default(),
            &cancellation,
        )
        .unwrap(),
    };
    let layered = Arc::new(
        PreparedCopper::build(&[zone, overlay], CopperLimits::default(), &cancellation).unwrap(),
    );
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    renderer.draw(&gpu, &frame, &layered, 0.5, true).unwrap();
    let overlay_pixels = target.read(&context);
    let center = rgb(&overlay_pixels, 64, 64);
    assert!(
        (126..=129).contains(&center[0]) && center[1] == 0 && (126..=129).contains(&center[2]),
        "later zone must clear earlier hole coverage without erasing underlying color: {center:?}"
    );
    assert_eq!(rgb(&overlay_pixels, 40, 87), [0, 0, 0]);
    assert_eq!(telemetry.snapshot().visible_zones, 2);
    gpu.content_mask.bounds = ViewBounds {
        origin: point(ScaledPixels(64.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(64.0), ScaledPixels(128.0)),
    };
    target.bind(&context);
    renderer.draw(&gpu, &frame, &layered, 0.5, false).unwrap();
    let clipped = target.read(&context);
    assert_eq!(
        rgb(&clipped, 23, 105),
        [0, 0, 0],
        "parent clip must exclude copper to its left"
    );
    assert!(
        (126..=129).contains(&rgb(&clipped, 105, 105)[1]),
        "copper within the parent clip must remain visible"
    );
    assert_eq!(
        telemetry.snapshot().cache_builds,
        3,
        "clip changes must reuse uploaded geometry"
    );
    // Exercise the same composite payload and renderer used by the desktop viewport.
    gpu.content_mask.bounds = bounds;
    let trace_stats = Arc::new(TraceTelemetry::default());
    let copper_stats = Arc::new(CopperTelemetry::default());
    let composite_pad_stats = Arc::new(TraceTelemetry::default());
    let composite_custom_stats = Arc::new(CopperTelemetry::default());
    let composite_drill_stats = Arc::new(TraceTelemetry::default());
    let composite_drawing_stats = Arc::new(TraceTelemetry::default());
    let composite_text_stats = Arc::new(TraceTelemetry::default());
    let mut board = crate::backend::d3d11::BoardRenderer::new(
        Arc::clone(&trace_stats),
        Arc::clone(&copper_stats),
        Arc::clone(&composite_pad_stats),
        Arc::clone(&composite_custom_stats),
        Arc::clone(&composite_drill_stats),
        Arc::clone(&composite_drawing_stats),
        Arc::clone(&composite_text_stats),
    );
    let mut board_frame = crate::backend::d3d11::BoardFrame {
        drawings: None,
        texts: None,
        traces: frame,
        display: Arc::new(pomelo_core::display::BoardDisplay::default()),
        pads: None,
        drills: None,
        drill_color: [1.0; 4],
        copper: layered,
        copper_opacity: 0.5,
        layer_order: Arc::new(vec![LayerId(2), LayerId(3), LayerId(1)]),
    };
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        copper_stats.snapshot().uploaded_bytes,
        0,
        "trace upload frame must defer copper traffic"
    );
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let composed = target.read(&context);
    assert_eq!(
        rgb(&composed, 64, 64),
        [255, 0, 0],
        "desktop composition draws traces above copper"
    );
    assert!((126..=129).contains(&rgb(&composed, 23, 105)[1]));
    assert_eq!(
        copper_stats.snapshot().uploaded_bytes,
        board_frame.copper.upload_bytes() as u64
    );
    assert_eq!(trace_stats.snapshot().cache_builds, 1);
    assert_eq!(copper_stats.snapshot().cache_builds, 1);
    let trace_bytes = trace_stats.snapshot().uploaded_bytes;
    let copper_bytes = copper_stats.snapshot().uploaded_bytes;
    for unit in [
        pomelo_core::units::LengthUnit::Mils,
        pomelo_core::units::LengthUnit::Millimeters,
    ] {
        Arc::make_mut(&mut board_frame.display).length_unit = unit;
        target.bind(&context);
        board.draw(&gpu, &board_frame).unwrap();
        assert_eq!(
            target.read(&context),
            composed,
            "display units must not alter PCB geometry"
        );
        assert_eq!(trace_stats.snapshot().uploaded_bytes, trace_bytes);
        assert_eq!(copper_stats.snapshot().uploaded_bytes, copper_bytes);
        assert_eq!(trace_stats.snapshot().cache_builds, 1);
        assert_eq!(copper_stats.snapshot().cache_builds, 1);
    }
    let original_order = Arc::clone(&board_frame.layer_order);
    let mut display_order = (*board_frame.display).clone();
    assert!(display_order.move_layer_to_edge(LayerId(3), true, &original_order));
    board_frame.layer_order =
        Arc::new(display_order.ordered_layers(original_order.iter().copied()));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let reordered = rgb(&target.read(&context), 64, 64);
    assert!(
        (126..=129).contains(&reordered[0]),
        "upper copper must blend over trace: {reordered:?}"
    );
    assert_ne!(reordered, [255, 0, 0]);
    assert_eq!(trace_stats.snapshot().uploaded_bytes, trace_bytes);
    assert_eq!(copper_stats.snapshot().uploaded_bytes, copper_bytes);
    assert_eq!(trace_stats.snapshot().cache_builds, 1);
    assert_eq!(copper_stats.snapshot().cache_builds, 1);
    board_frame.layer_order = original_order;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), composed);
    for (opacity, expected_green) in [(0.0, 0), (1.0, 255)] {
        board_frame.copper_opacity = opacity;
        target.bind(&context);
        board.draw(&gpu, &board_frame).unwrap();
        let pixels = target.read(&context);
        assert_eq!(rgb(&pixels, 23, 105)[1], expected_green);
        assert_eq!(rgb(&pixels, 64, 64), [255, 0, 0]);
        assert_eq!(trace_stats.snapshot().uploaded_bytes, trace_bytes);
        assert_eq!(copper_stats.snapshot().uploaded_bytes, copper_bytes);
        assert_eq!(trace_stats.snapshot().cache_builds, 1);
        assert_eq!(copper_stats.snapshot().cache_builds, 1);
    }
    board_frame.copper_opacity = 0.5;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), composed);
    let mut hidden_copper = (*board_frame.display).clone();
    hidden_copper.show_copper = false;
    board_frame.display = Arc::new(hidden_copper);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let hidden_pixels = target.read(&context);
    assert_eq!(rgb(&hidden_pixels, 23, 105), [0, 0, 0]);
    assert_eq!(rgb(&hidden_pixels, 64, 64), [255, 0, 0]);
    assert_eq!(copper_stats.snapshot().visible_zones, 0);
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), composed);
    assert_eq!(copper_stats.snapshot().uploaded_bytes, copper_bytes);
    assert_eq!(copper_stats.snapshot().cache_builds, 1);
    let dimension = pomelo_core::model::BoardDrawing {
        id: ObjectId(90),
        owner_id: None,
        layer: LayerId(99),
        net: NetId(1),
        graphic_ids: vec![],
        text_ids: vec![],
        segments: vec![Segment {
            id: ObjectId(1),
            track_id: ObjectId(1),
            layer: LayerId(99),
            net: NetId(1),
            a: Point::new(2.0, 10.0),
            b: Point::new(18.0, 10.0),
            width: 1.0,
            arc: None,
            bond_wire: None,
        }],
    };
    let dimensions = Arc::new(
        PreparedTracks::build_drawings(&[dimension], TraceLimits::default(), &cancellation)
            .unwrap(),
    );
    let original_colors = Arc::clone(&board_frame.traces.colors);
    let mut drawing_colors = (*original_colors).clone();
    drawing_colors.insert(LayerId(99), [0.0, 0.0, 1.0, 1.0]);
    board_frame.traces.colors = Arc::new(drawing_colors);
    board_frame.traces.highlighted_net = Some((NetId(1), [0.0, 1.0, 1.0, 1.0]));
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Segment(ObjectId(1)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let without_dimension = rgb(&target.read(&context), 64, 64);
    board_frame.drawings = Some(Arc::clone(&dimensions));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 64, 64),
        [0, 0, 255],
        "dimension must keep its color despite matching PCB IDs"
    );
    let drawing_uploaded = composite_drawing_stats.snapshot();
    let drawing_pixels = target.read(&context);
    let mut hidden_drawings = (*board_frame.display).clone();
    hidden_drawings.show_drawings = false;
    board_frame.display = Arc::new(hidden_drawings);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 64), without_dimension);
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), drawing_pixels);
    assert_eq!(drawing_uploaded.cache_builds, 1);
    assert_eq!(drawing_uploaded.uploaded_instances, 1);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        composite_drawing_stats.snapshot().uploaded_bytes,
        drawing_uploaded.uploaded_bytes
    );
    assert_eq!(composite_drawing_stats.snapshot().cache_builds, 1);
    let recovery_drawings = Arc::new(TraceTelemetry::default());
    let mut recovery_board = crate::backend::d3d11::BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::clone(&recovery_drawings),
        Arc::new(TraceTelemetry::default()),
    );
    for _ in 0..3 {
        target.bind(&context);
        recovery_board.draw(&gpu, &board_frame).unwrap();
    }
    assert_eq!(rgb(&target.read(&context), 64, 64), [0, 0, 255]);
    assert_eq!(recovery_drawings.snapshot().uploaded_instances, 1);
    assert_eq!(recovery_drawings.snapshot().cache_builds, 1);
    let before_reset_bytes = recovery_drawings.snapshot().uploaded_bytes;
    recovery_board.reset();
    assert_eq!(recovery_drawings.snapshot().uploaded_instances, 0);
    assert_eq!(recovery_drawings.snapshot().resets, 1);
    for _ in 0..3 {
        target.bind(&context);
        recovery_board.draw(&gpu, &board_frame).unwrap();
    }
    assert_eq!(
        rgb(&target.read(&context), 64, 64),
        [0, 0, 255],
        "dimension pixels must return after resource reset"
    );
    assert_eq!(recovery_drawings.snapshot().cache_builds, 2);
    assert_eq!(recovery_drawings.snapshot().pipeline_builds, 2);
    assert_eq!(recovery_drawings.snapshot().uploaded_instances, 1);
    assert_eq!(
        recovery_drawings.snapshot().uploaded_bytes,
        before_reset_bytes * 2
    );
    board_frame.drawings = Some(Arc::new(
        PreparedTracks::build_drawings(&[], TraceLimits::default(), &cancellation).unwrap(),
    ));
    target.bind(&context);
    recovery_board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 64, 64),
        without_dimension,
        "empty source must remove the old dimension geometry"
    );
    assert_eq!(recovery_drawings.snapshot().uploaded_instances, 0);
    let cleared_resets = recovery_drawings.snapshot().resets;
    assert_eq!(cleared_resets, 2);
    target.bind(&context);
    recovery_board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        recovery_drawings.snapshot().resets,
        cleared_resets,
        "empty frames must not reset repeatedly"
    );
    board_frame.drawings = Some(Arc::clone(&dimensions));
    let mut hidden_dimensions = pomelo_core::display::BoardDisplay::default();
    hidden_dimensions.hidden_layers.insert(LayerId(99));
    board_frame.display = Arc::new(hidden_dimensions);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 64, 64),
        without_dimension,
        "hidden drawing layer must restore the PCB beneath"
    );
    board_frame.drawings = None;
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    let font = crate::text::StrokeFont::bundled_core(
        crate::text::FontLimits {
            glyphs: 1024,
            encoded_bytes: 65536,
            points_per_glyph: 1024,
            strokes: 65536,
        },
        &cancellation,
    )
    .unwrap();
    let text = pomelo_core::model::BoardText {
        id: ObjectId(1),
        owner_id: None,
        layer: LayerId(99),
        class_id: 0,
        subclass: 0,
        text: "A".into(),
        at: Point::new(2.0, 2.0),
        angle: 0.0,
        mirrored: false,
        align: pomelo_core::model::TextAlignment::Left,
        font_index: 0,
        width: 16.0,
        height: 16.0,
        spacing: 0.0,
        line_spacing: 16.0,
        stroke_width: 1.0,
    };
    let mut missing_text = text.clone();
    missing_text.id = ObjectId(999);
    missing_text.text = "한".into();
    let (prepared_text, text_warnings) =
        crate::text::PreparedTexts::build_recovering_missing_glyphs(
            &[missing_text.clone(), text.clone()],
            &font,
            2,
            2,
            1024,
            &cancellation,
        )
        .unwrap();
    assert_eq!(text_warnings.len(), 1);
    assert_eq!(text_warnings[0].object, Some(ObjectId(999)));
    assert_eq!(prepared_text.batches.len(), 1);
    assert_eq!(prepared_text.batches[0].object, ObjectId(1));
    board_frame.texts = Some(Arc::new(
        crate::text_instances::PreparedTextInstances::build(
            &[missing_text, text],
            &font,
            2,
            2,
            65536,
            &cancellation,
        )
        .unwrap(),
    ));
    for _ in 0..2 {
        target.bind(&context);
        board.draw(&gpu, &board_frame).unwrap();
    }
    let text_pixels = target.read(&context);
    assert!(
        text_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [0, 0, 255]),
        "bundled glyph must retain its layer color despite overlapping PCB IDs"
    );
    let uploaded_text = composite_text_stats.snapshot();
    let mut text_hidden = (*board_frame.display).clone();
    text_hidden.show_texts = false;
    board_frame.display = Arc::new(text_hidden);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert!(
        !target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [0, 0, 255])
    );
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), text_pixels);
    assert_eq!(
        composite_text_stats.snapshot().uploaded_bytes,
        uploaded_text.uploaded_bytes
    );
    let mut hidden_text = pomelo_core::display::BoardDisplay::default();
    hidden_text.hidden_layers.insert(LayerId(99));
    board_frame.display = Arc::new(hidden_text);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 64), without_dimension);
    assert!(
        !target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [0, 0, 255])
    );
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), text_pixels);
    assert_eq!(
        composite_text_stats.snapshot().uploaded_bytes,
        uploaded_text.uploaded_bytes
    );
    let recovery_text_stats = Arc::new(TraceTelemetry::default());
    let mut text_recovery_board = crate::backend::d3d11::BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::clone(&recovery_text_stats),
    );
    for _ in 0..3 {
        target.bind(&context);
        text_recovery_board.draw(&gpu, &board_frame).unwrap();
    }
    assert_eq!(target.read(&context), text_pixels);
    text_recovery_board.reset();
    assert_eq!(recovery_text_stats.snapshot().uploaded_instances, 0);
    assert_eq!(recovery_text_stats.snapshot().resets, 1);
    for _ in 0..3 {
        target.bind(&context);
        text_recovery_board.draw(&gpu, &board_frame).unwrap();
    }
    assert_eq!(
        target.read(&context),
        text_pixels,
        "glyph composite must return after resource reset"
    );
    let rebuilt_text = recovery_text_stats.snapshot();
    assert_eq!(
        rebuilt_text.uploaded_instances,
        uploaded_text.uploaded_instances
    );
    assert_eq!(rebuilt_text.cache_builds, 2);
    assert_eq!(rebuilt_text.pipeline_builds, 2);
    assert_eq!(
        rebuilt_text.uploaded_bytes,
        uploaded_text.uploaded_bytes * 2
    );
    board_frame.texts = None;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(composite_text_stats.snapshot().uploaded_instances, 0);
    assert_eq!(composite_text_stats.snapshot().resets, 1);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        composite_text_stats.snapshot().resets,
        1,
        "absent text source must not repeatedly reset its cache"
    );
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    board_frame.traces.highlighted_net = None;
    board_frame.traces.highlighted_object = None;
    board_frame.traces.colors = original_colors;
    use pomelo_core::model::{DrillShape, Pad, PadKind, Pin};
    let mut disk = Pad::circle(LayerId(2), 4.0);
    disk.offset = Point::new(-5.0, -5.0);
    let mut ring = Pad::circle(LayerId(3), 4.0);
    ring.offset = Point::new(5.0, 5.0);
    ring.kind = PadKind::DONUT;
    ring.inner_diameter = Some(2.0);
    let mut custom = Pad::circle(LayerId(3), 4.0);
    custom.kind = PadKind::CUSTOM;
    custom.custom = Some(Arc::new(pomelo_core::model::CustomPadGeometry {
        contours: vec![square(-2.0, -2.0, 4.0), square(-0.5, -0.5, 1.0)],
        paths: vec![],
    }));
    let pin = Pin {
        id: ObjectId(50),
        owner_id: ObjectId(49),
        net: NetId(1),
        name: String::new(),
        reference: String::new(),
        at: Point::new(10.0, 10.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: false,
        },
        pads: vec![disk, ring, custom],
        stackup_region: None,
        die: None,
    };
    let mut drill_pin = pin.clone();
    drill_pin.at = Point::new(15.0, 15.0);
    drill_pin.angle = std::f64::consts::FRAC_PI_2;
    drill_pin.drill_shape = DrillShape {
        width: 2.0,
        height: 1.0,
        plated: true,
    };
    let drill_geometry = Arc::new(
        crate::drills::PreparedDrills::build(
            &[],
            &[pomelo_core::model::Via {
                id: drill_pin.id,
                net: drill_pin.net,
                at: drill_pin.at,
                drill: 2.0,
                drill_shape: drill_pin.drill_shape,
                padstack: ObjectId(51),
                padstack_name: String::new(),
                start_layer: Some(LayerId(2)),
                end_layer: Some(LayerId(3)),
                pads: Arc::from(drill_pin.pads),
                backdrill: None,
                stackup_region: None,
                angle: drill_pin.angle,
                mirrored: false,
                finger: None,
            }],
            crate::pads::PadLimits::default(),
            &cancellation,
        )
        .unwrap()
        .geometry,
    );
    let mut prepared_pads = crate::pads::PreparedPads::build(
        std::slice::from_ref(&pin),
        &[],
        crate::pads::PadLimits::default(),
        &cancellation,
    )
    .unwrap();
    prepared_pads.custom_mesh = Some(Arc::new(
        prepared_pads
            .build_custom_meshes(
                crate::copper::CopperLimits::default(),
                &MeshLimits::default(),
                &cancellation,
            )
            .unwrap(),
    ));
    let pads = Arc::new(prepared_pads);
    let pad_stats = Arc::new(TraceTelemetry::default());
    let mut pad_renderer = crate::backend::d3d11::PadRenderer::new(Arc::clone(&pad_stats));
    target.bind(&context);
    pad_renderer.prepare(&gpu, &pads).unwrap();
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let pad_pixels = target.read(&context);
    assert_eq!(
        rgb(&pad_pixels, 35, 93),
        [0, 255, 0],
        "analytic disk center must be filled"
    );
    assert_eq!(
        rgb(&pad_pixels, 93, 35),
        [0, 0, 0],
        "donut opening must stay transparent"
    );
    assert_eq!(
        rgb(&pad_pixels, 103, 35),
        [0, 0, 255],
        "donut rim must be filled"
    );
    let pad_upload_before = pad_stats.snapshot();
    board_frame.traces.highlighted_net = Some((NetId(1), [1.0, 0.0, 1.0, 1.0]));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let highlighted_pads = target.read(&context);
    assert_eq!(rgb(&highlighted_pads, 35, 93), [255, 0, 255]);
    assert_eq!(rgb(&highlighted_pads, 103, 35), [255, 0, 255]);
    assert_eq!(rgb(&highlighted_pads, 93, 35), [0; 3]);
    board_frame.traces.highlighted_net = Some((NetId(999), [1.0, 0.0, 1.0, 1.0]));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    board_frame.traces.highlighted_net = None;
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(50)])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [0, 255, 255]);
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(999)])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    board_frame.traces.highlighted_objects = None;
    for (object, expected) in [
        (
            pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
            [0, 255, 255],
        ),
        (
            pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
            rgb(&pad_pixels, 35, 93),
        ),
    ] {
        board_frame.traces.highlighted_related_objects = Some((
            Arc::new(std::collections::BTreeSet::from([object])),
            [0.0, 1.0, 1.0, 1.0],
        ));
        target.bind(&context);
        pad_renderer
            .draw_prepared(&gpu, &board_frame.traces, None)
            .unwrap();
        assert_eq!(rgb(&target.read(&context), 35, 93), expected);
    }
    board_frame.traces.highlighted_related_objects = None;
    board_frame.traces.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [255, 0, 255]);
    board_frame.traces.highlighted_net = Some((NetId(1), [0.0, 1.0, 1.0, 1.0]));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [0, 255, 255]);
    board_frame.traces.highlighted_net = None;
    board_frame.traces.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    board_frame.traces.hovered_object = None;

    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [0, 255, 255]);
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(50)])),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [255, 0, 255]);
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 35, 93), [0, 255, 255]);
    board_frame.traces.highlighted_objects = None;
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    board_frame.traces.highlighted_object = None;
    assert_eq!(
        pad_upload_before.uploaded_bytes,
        pad_stats.snapshot().uploaded_bytes
    );
    assert_eq!(
        pad_upload_before.cache_builds,
        pad_stats.snapshot().cache_builds
    );
    target.bind(&context);
    pad_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    assert_eq!(pad_stats.snapshot().uploaded_instances, 2);
    assert_eq!(pad_stats.snapshot().uploaded_bytes, 256);
    assert_eq!(pad_stats.snapshot().cache_builds, 1);
    // A real via uses the same source ID and geometry as the pin fixture.
    // Selection must distinguish their categories without altering their holes.
    let via = pomelo_core::model::Via {
        id: pin.id,
        net: pin.net,
        at: pin.at,
        drill: 0.0,
        drill_shape: pin.drill_shape,
        padstack: ObjectId(51),
        padstack_name: String::new(),
        start_layer: Some(LayerId(2)),
        end_layer: Some(LayerId(3)),
        pads: Arc::from(pin.pads.clone()),
        backdrill: None,
        stackup_region: None,
        angle: pin.angle,
        mirrored: pin.mirrored,
        finger: None,
    };
    let mut via_pads = crate::pads::PreparedPads::build(
        &[],
        &[via],
        crate::pads::PadLimits::default(),
        &cancellation,
    )
    .unwrap();
    via_pads.custom_mesh = Some(Arc::new(
        via_pads
            .build_custom_meshes(
                crate::copper::CopperLimits::default(),
                &MeshLimits::default(),
                &cancellation,
            )
            .unwrap(),
    ));
    assert!(
        via_pads
            .custom_mesh
            .as_ref()
            .unwrap()
            .batches
            .iter()
            .all(|batch| batch.selected_object
                == pomelo_core::selection::SelectedObject::Via(pin.id))
    );
    let via_pads = Arc::new(via_pads);
    let via_stats = Arc::new(TraceTelemetry::default());
    let mut via_renderer = crate::backend::d3d11::PadRenderer::new(Arc::clone(&via_stats));
    via_renderer.prepare(&gpu, &via_pads).unwrap();
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(pin.id),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    via_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let via_selected = target.read(&context);
    assert_eq!(rgb(&via_selected, 35, 93), [0, 255, 255]);
    assert_eq!(rgb(&via_selected, 103, 35), [0, 255, 255]);
    assert_eq!(rgb(&via_selected, 93, 35), [0; 3]);
    let via_before = via_stats.snapshot();
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Pin(pin.id),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    via_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(target.read(&context), pad_pixels);
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(pin.id),
        [0.0, 1.0, 1.0, 1.0],
    ));
    let via_copper_stats = Arc::new(CopperTelemetry::default());
    let mut via_copper = CopperRenderer::new(Arc::clone(&via_copper_stats));
    target.bind(&context);
    via_copper
        .draw(
            &gpu,
            &board_frame.traces,
            via_pads.custom_mesh.as_ref().unwrap(),
            1.0,
            true,
        )
        .unwrap();
    let via_custom = target.read(&context);
    assert_eq!(rgb(&via_custom, 73, 64), [0, 255, 255]);
    assert_eq!(rgb(&via_custom, 64, 64), [0; 3]);
    let via_custom_before = via_copper_stats.snapshot();
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Zone(pin.id),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    via_copper
        .draw(
            &gpu,
            &board_frame.traces,
            via_pads.custom_mesh.as_ref().unwrap(),
            1.0,
            false,
        )
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 73, 64), [0, 0, 255]);
    assert_eq!(
        via_custom_before.lifetime_uploaded_bytes,
        via_copper_stats.snapshot().lifetime_uploaded_bytes
    );
    assert_eq!(
        via_custom_before.cache_builds,
        via_copper_stats.snapshot().cache_builds
    );
    assert_eq!(
        via_before.uploaded_bytes,
        via_stats.snapshot().uploaded_bytes
    );
    assert_eq!(via_before.cache_builds, via_stats.snapshot().cache_builds);
    board_frame.traces.highlighted_object = None;
    let mut first_owner = pin.clone();
    board_frame.traces.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    via_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let hovered_ring = target.read(&context);
    assert_eq!(rgb(&hovered_ring, 103, 35), [255, 0, 255]);
    assert_eq!(rgb(&hovered_ring, 93, 35), [0; 3]);
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    via_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 103, 35), [0, 255, 255]);
    board_frame.traces.highlighted_object = None;
    board_frame.traces.hovered_object = None;
    first_owner.pads.truncate(1);
    let mut second_owner = first_owner.clone();
    second_owner.id = ObjectId(51);
    second_owner.owner_id = ObjectId(52);
    second_owner.at.x += 4.0;
    let mixed_pads = Arc::new(
        crate::pads::PreparedPads::build(
            &[first_owner, second_owner],
            &[],
            crate::pads::PadLimits::default(),
            &cancellation,
        )
        .unwrap(),
    );
    let mixed_stats = Arc::new(TraceTelemetry::default());
    let mut mixed_renderer = crate::backend::d3d11::PadRenderer::new(Arc::clone(&mixed_stats));
    mixed_renderer.prepare(&gpu, &mixed_pads).unwrap();
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(50)])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    mixed_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let mixed_first = target.read(&context);
    assert_eq!(rgb(&mixed_first, 35, 93), [0, 255, 255]);
    assert_eq!(rgb(&mixed_first, 58, 93), [0, 255, 0]);
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(51)])),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    mixed_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let mixed_second = target.read(&context);
    assert_eq!(rgb(&mixed_second, 35, 93), [0, 255, 0]);
    assert_eq!(rgb(&mixed_second, 58, 93), [255, 0, 255]);
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    mixed_renderer
        .draw_prepared(&gpu, &board_frame.traces, None)
        .unwrap();
    let mixed_colors = target.read(&context);
    assert_eq!(rgb(&mixed_colors, 35, 93), [0, 255, 255]);
    assert_eq!(rgb(&mixed_colors, 58, 93), [255, 0, 255]);
    board_frame.traces.highlighted_object = None;
    assert_eq!(mixed_stats.snapshot().uploaded_bytes, 256);
    assert_eq!(mixed_stats.snapshot().cache_builds, 1);
    board_frame.traces.highlighted_objects = None;
    board_frame.pads = Some(Arc::clone(&pads));
    let uploaded_before = copper_stats.snapshot().lifetime_uploaded_bytes;
    board_frame.layer_order = Arc::new(vec![LayerId(1), LayerId(2), LayerId(3), LayerId(3)]);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let reordered = target.read(&context);
    assert_eq!(composite_pad_stats.snapshot().uploaded_instances, 2);
    assert_eq!(
        rgb(&reordered, 35, 93),
        [0, 255, 0],
        "composite renderer must draw analytic pads on their layer"
    );
    let center = rgb(&reordered, 64, 64);
    assert!(
        (126..=129).contains(&center[0]) && center[1] == 0 && (126..=129).contains(&center[2]),
        "higher copper layer must blend above lower trace layer once: {center:?}"
    );
    assert_eq!(copper_stats.snapshot().visible_zones, 2);
    assert_eq!(
        copper_stats.snapshot().lifetime_uploaded_bytes,
        uploaded_before
    );
    assert_eq!(trace_stats.snapshot().cache_builds, 1);
    assert_eq!(copper_stats.snapshot().cache_builds, 1);
    assert_eq!(
        composite_custom_stats.snapshot().uploaded_bytes,
        0,
        "custom upload must wait for analytic upload completion"
    );
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let custom_pixels = target.read(&context);
    assert_eq!(
        rgb(&custom_pixels, 73, 64),
        [0, 0, 255],
        "custom exterior must fill its own layer"
    );
    assert_eq!(
        rgb(&custom_pixels, 64, 64),
        center,
        "custom hole must preserve lower-layer composition"
    );
    assert_eq!(composite_custom_stats.snapshot().visible_zones, 1);
    assert_eq!(composite_custom_stats.snapshot().cache_builds, 1);
    let custom_probe_stats = Arc::new(CopperTelemetry::default());
    let mut custom_probe = CopperRenderer::new(Arc::clone(&custom_probe_stats));
    board_frame.traces.highlighted_net = Some((NetId(1), [1.0, 0.0, 1.0, 1.0]));
    target.bind(&context);
    custom_probe
        .draw(
            &gpu,
            &board_frame.traces,
            pads.custom_mesh.as_ref().unwrap(),
            1.0,
            true,
        )
        .unwrap();
    let custom_highlight = target.read(&context);
    assert_eq!(rgb(&custom_highlight, 73, 64), [255, 0, 255]);
    assert_eq!(rgb(&custom_highlight, 64, 64), [0; 3]);
    let custom_uploaded = custom_probe_stats.snapshot().lifetime_uploaded_bytes;
    board_frame.traces.highlighted_net = None;
    for (object, expected) in [
        (
            pomelo_core::selection::SelectedObject::Pin(ObjectId(50)),
            [255, 0, 255],
        ),
        (
            pomelo_core::selection::SelectedObject::Via(ObjectId(50)),
            [0, 0, 255],
        ),
    ] {
        board_frame.traces.hovered_object = Some((object, [1.0, 0.0, 1.0, 1.0]));
        target.bind(&context);
        custom_probe
            .draw(
                &gpu,
                &board_frame.traces,
                pads.custom_mesh.as_ref().unwrap(),
                1.0,
                false,
            )
            .unwrap();
        let hovered_custom = target.read(&context);
        assert_eq!(rgb(&hovered_custom, 73, 64), expected);
        assert_eq!(rgb(&hovered_custom, 64, 64), [0; 3]);
    }
    board_frame.traces.hovered_object = None;
    board_frame.traces.highlighted_objects = Some((
        Arc::new(std::collections::BTreeSet::from([ObjectId(50)])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    custom_probe
        .draw(
            &gpu,
            &board_frame.traces,
            pads.custom_mesh.as_ref().unwrap(),
            1.0,
            false,
        )
        .unwrap();
    let custom_selected = target.read(&context);
    assert_eq!(rgb(&custom_selected, 73, 64), [0, 255, 255]);
    assert_eq!(rgb(&custom_selected, 64, 64), [0; 3]);
    board_frame.traces.highlighted_objects = None;
    target.bind(&context);
    custom_probe
        .draw(
            &gpu,
            &board_frame.traces,
            pads.custom_mesh.as_ref().unwrap(),
            1.0,
            false,
        )
        .unwrap();
    assert_eq!(rgb(&target.read(&context), 73, 64), [0, 0, 255]);
    assert_eq!(
        custom_uploaded,
        custom_probe_stats.snapshot().lifetime_uploaded_bytes
    );
    board_frame.drills = Some(drill_geometry);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let drill_pixels = target.read(&context);
    assert_eq!(
        rgb(&drill_pixels, 93, 35),
        [255, 255, 255],
        "drill display must fill its center independently of the copper opening"
    );
    assert!(
        rgb(&drill_pixels, 93, 30)[1] > 200,
        "rotated slot must extend along its rotated axis"
    );
    assert_eq!(
        rgb(&drill_pixels, 98, 35),
        rgb(&custom_pixels, 98, 35),
        "outside the rotated slot must preserve the lower-layer pixel"
    );
    assert_eq!(composite_drill_stats.snapshot().uploaded_instances, 1);
    assert_eq!(composite_drill_stats.snapshot().cache_builds, 1);
    let before_pad_bytes = composite_pad_stats.snapshot().uploaded_bytes;
    let before_custom_bytes = composite_custom_stats.snapshot().lifetime_uploaded_bytes;
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay {
        hidden_layers: std::collections::BTreeSet::from([LayerId(2), LayerId(3)]),
        show_drills: false,
        ..Default::default()
    });
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let hidden = target.read(&context);
    assert_eq!(
        rgb(&hidden, 35, 93),
        [0, 0, 0],
        "hidden layer must remove its analytic pad"
    );
    assert_eq!(
        rgb(&hidden, 73, 64),
        [255, 0, 0],
        "hidden custom pad must expose the lower trace"
    );
    assert_eq!(
        rgb(&hidden, 93, 35),
        [0, 0, 0],
        "disabled drills must not cover hidden pad layers"
    );
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        target.read(&context),
        drill_pixels,
        "restoring visibility must restore all pixels"
    );
    assert_eq!(
        composite_pad_stats.snapshot().uploaded_bytes,
        before_pad_bytes
    );
    assert_eq!(
        composite_custom_stats.snapshot().lifetime_uploaded_bytes,
        before_custom_bytes
    );
    assert_eq!(composite_pad_stats.snapshot().cache_builds, 1);
    assert_eq!(composite_drill_stats.snapshot().cache_builds, 1);
    let before_drill_bytes = composite_drill_stats.snapshot().uploaded_bytes;
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay {
        hidden_layers: std::collections::BTreeSet::from([LayerId(2), LayerId(3)]),
        show_drills: true,
        ..Default::default()
    });
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 93, 35),
        [0, 0, 0],
        "via drill must disappear when all of its pad layers are hidden"
    );
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay {
        hidden_layers: std::collections::BTreeSet::from([LayerId(2)]),
        show_drills: true,
        ..Default::default()
    });
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 93, 35),
        [255, 255, 255],
        "one visible pad layer must restore the via drill"
    );
    assert_eq!(
        composite_drill_stats.snapshot().uploaded_bytes,
        before_drill_bytes
    );
    assert_eq!(composite_drill_stats.snapshot().cache_builds, 1);
}

struct Target {
    texture: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
}

impl Target {
    fn new(device: &ID3D11Device) -> Self {
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: SIDE,
            Height: SIDE,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let (mut texture, mut staging, mut view) = (None, None, None);
        // SAFETY: descriptors are initialized; test-owned resources remain on this device.
        unsafe {
            device
                .CreateTexture2D(&descriptor, None, Some(&mut texture))
                .unwrap();
            device
                .CreateRenderTargetView(texture.as_ref().unwrap(), None, Some(&mut view))
                .unwrap();
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..descriptor
                    },
                    None,
                    Some(&mut staging),
                )
                .unwrap();
        }
        Self {
            texture: texture.unwrap(),
            staging: staging.unwrap(),
            view: view.unwrap(),
        }
    }

    fn bind(&self, context: &ID3D11DeviceContext) {
        // SAFETY: live target and context from the same device; no borrowed handles escape.
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(self.view.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: SIDE as f32,
                Height: SIDE as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.ClearRenderTargetView(&self.view, &[0.0, 0.0, 0.0, 1.0]);
        }
    }

    fn read(&self, context: &ID3D11DeviceContext) -> Vec<u8> {
        let mut pixels = vec![0; (SIDE * SIDE * 4) as usize];
        // SAFETY: Map READ synchronizes the copy. Only valid row bytes are read; row pitch
        // is honored, and staging remains mapped until all rows have been copied.
        unsafe {
            context.CopyResource(&self.staging, &self.texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
            for row in 0..SIDE as usize {
                let source = std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    SIDE as usize * 4,
                );
                pixels[row * SIDE as usize * 4..(row + 1) * SIDE as usize * 4]
                    .copy_from_slice(source);
            }
            context.Unmap(&self.staging, 0);
        }
        pixels
    }
}

fn rgb(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let offset = (y * SIDE as usize + x) * 4;
    pixels[offset..offset + 3].try_into().unwrap()
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; run explicitly for GPU validation"]
fn hardware_trace_pixels_preserve_caps_arc_hole_clip_and_cache() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs. HARDWARE has no WARP fallback; an unavailable GPU fails.
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let device = device.unwrap();
    let context = context.unwrap();
    let compact_text_pipeline = super::Pipeline::new_text(&device).unwrap();
    drop(compact_text_pipeline);
    println!("COMPACT_TEXT_SHADER_PIPELINE_CREATED rendered=false");

    // SAFETY: interface queried from the live device; returned adapter description is initialized.
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
            .position(|value| *value == 0)
            .unwrap_or(adapter.Description.len())],
    );
    assert!(!adapter_name.to_ascii_lowercase().contains("software"));
    let line = Segment {
        id: ObjectId(1),
        track_id: ObjectId(1),
        layer: LayerId(1),
        net: NetId(7),
        a: Point::new(2.0, 5.0),
        b: Point::new(18.0, 5.0),
        width: 1.0,
        arc: None,
        bond_wire: None,
    };
    let circle = Segment {
        id: ObjectId(2),
        track_id: ObjectId(1),
        layer: LayerId(2),
        net: NetId(7),
        a: Point::new(14.0, 12.0),
        b: Point::new(14.0, 12.0),
        width: 1.0,
        arc: Some(BoardArc {
            center: Point::new(10.0, 12.0),
            radius: 4.0,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        }),
        bond_wire: None,
    };
    let quarter = Segment {
        id: ObjectId(3),
        track_id: ObjectId(3),
        layer: LayerId(3),
        net: NetId(8),
        a: Point::new(8.0, 12.0),
        b: Point::new(6.0, 10.0),
        width: 1.0,
        arc: Some(BoardArc {
            center: Point::new(6.0, 12.0),
            radius: 2.0,
            start: 0.0,
            sweep: -std::f64::consts::FRAC_PI_2,
        }),
        bond_wire: None,
    };
    let segments = [line, circle, quarter];
    let tracks = Arc::new(
        PreparedTracks::build(
            &segments,
            TraceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    );
    let colors = Arc::new(BTreeMap::from([
        (LayerId(1), [1.0, 0.0, 0.0, 1.0]),
        (LayerId(2), [0.0, 1.0, 0.0, 1.0]),
        (LayerId(3), [0.0, 0.0, 1.0, 1.0]),
    ]));
    let mut frame = TraceFrame {
        tracks: Arc::clone(&tracks),
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(20.0, 20.0),
        },
        camera: None,
        scale_factor: 1.0,
        colors,
        fallback_color: [1.0; 4],
        highlighted_net: None,
        highlighted_objects: None,
        highlighted_related_objects: None,
        hovered_object: None,
        highlighted_trace: None,
        highlighted_object: None,
    };
    let telemetry = Arc::new(TraceTelemetry::default());
    let mut renderer = TraceRenderer::<PreparedTracks>::new(Arc::clone(&telemetry));
    let target = Target::new(&device);
    let bounds = ViewBounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    };
    let mut gpu = NativeGpuContext {
        device: &device,
        context: &context,
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    assert!(rgb(&pixels, 64, 93)[0] > 240, "trace center must be red");
    assert_eq!(
        rgb(&pixels, 64, 52),
        [0; 3],
        "circle center must stay transparent"
    );
    assert!(
        rgb(&pixels, 64, 29)[1] > 220,
        "full-circle upper rim must be green"
    );
    assert!(
        rgb(&pixels, 41, 52)[1] > 180,
        "full-circle left rim must be green"
    );
    assert_eq!(
        rgb(&pixels, 9, 93),
        [0; 3],
        "outside round cap must stay clear"
    );
    assert!(
        rgb(&pixels, 16, 93)[0] > 220,
        "round cap must cover its endpoint"
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0 && pixel[0] < 240),
        "AA needs fractional coverage"
    );

    assert!(
        rgb(&pixels, 49, 61)[2] > 200,
        "negative quarter arc must cover its sweep"
    );
    assert_eq!(
        // The left rim of this quarter arc is excluded; keep the sample clear
        // of the separate green circle centred at (10, 12).
        rgb(&pixels, 29, 52),
        [0; 3],
        "excluded arc quadrant must stay clear"
    );

    // Same geometry, with DPI and camera changed: no second geometry upload.
    let before_highlight = telemetry.snapshot();
    frame.highlighted_net = Some((NetId(7), [0.0, 1.0, 1.0, 1.0]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let highlighted = target.read(&context);
    assert_eq!(rgb(&highlighted, 64, 93), [0, 255, 255]);
    assert_eq!(rgb(&highlighted, 64, 52), [0; 3]);
    assert_eq!(
        rgb(&highlighted, 64, 29),
        [0, 255, 255],
        "same network on another layer must highlight"
    );
    assert_eq!(
        rgb(&highlighted, 49, 61),
        rgb(&pixels, 49, 61),
        "another network must retain its layer color"
    );
    frame.highlighted_net = Some((NetId(3), [1.0, 0.0, 1.0, 1.0]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        pixels,
        target.read(&context),
        "layer ID 3 must not alias network ID 3"
    );
    frame.highlighted_net = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    frame.highlighted_trace = Some((
        super::super::board::TraceSelection::Segment(ObjectId(2)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected_segment = target.read(&context);
    assert_eq!(rgb(&selected_segment, 64, 29), [255, 0, 255]);
    assert_eq!(rgb(&selected_segment, 64, 93), rgb(&pixels, 64, 93));
    assert_eq!(rgb(&selected_segment, 49, 61), rgb(&pixels, 49, 61));
    frame.highlighted_trace = Some((
        super::super::board::TraceSelection::Track(ObjectId(1)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected_track = target.read(&context);
    assert_eq!(rgb(&selected_track, 64, 29), [0, 255, 255]);
    assert_eq!(rgb(&selected_track, 64, 93), [0, 255, 255]);
    assert_eq!(rgb(&selected_track, 49, 61), rgb(&pixels, 49, 61));
    assert_eq!(rgb(&selected_track, 64, 52), [0; 3]);
    frame.highlighted_trace = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    frame.highlighted_related_objects = Some((
        Arc::new(std::collections::BTreeSet::from([
            pomelo_core::selection::SelectedObject::Segment(ObjectId(1)),
            pomelo_core::selection::SelectedObject::Segment(ObjectId(2)),
        ])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let related = target.read(&context);
    assert_eq!(rgb(&related, 64, 93), [0, 255, 255]);
    assert_eq!(rgb(&related, 64, 29), [0, 255, 255]);
    assert_eq!(rgb(&related, 49, 61), rgb(&pixels, 49, 61));
    assert_eq!(rgb(&related, 64, 52), [0; 3]);
    frame.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Segment(ObjectId(2)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let priority = target.read(&context);
    assert_eq!(rgb(&priority, 64, 29), [255, 0, 255]);
    assert_eq!(rgb(&priority, 64, 93), [0, 255, 255]);
    frame.highlighted_object = None;
    frame.highlighted_related_objects = Some((
        Arc::new(std::collections::BTreeSet::from([
            pomelo_core::selection::SelectedObject::Via(ObjectId(1)),
            pomelo_core::selection::SelectedObject::Zone(ObjectId(2)),
        ])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    frame.highlighted_related_objects = None;
    frame.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Segment(ObjectId(1)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let hovered = target.read(&context);
    assert_eq!(rgb(&hovered, 64, 93), [255, 0, 255]);
    assert_eq!(rgb(&hovered, 64, 29), rgb(&pixels, 64, 29));
    frame.highlighted_net = Some((NetId(7), [0.0, 1.0, 1.0, 1.0]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 93), [0, 255, 255]);
    frame.highlighted_net = None;
    frame.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Via(ObjectId(1)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    frame.hovered_object = None;
    let after_highlight = telemetry.snapshot();
    assert_eq!(
        before_highlight.uploaded_bytes,
        after_highlight.uploaded_bytes
    );
    assert_eq!(before_highlight.cache_builds, after_highlight.cache_builds);

    frame.camera = Some(Camera {
        center: Point::new(10.0, 10.0),
        pixels_per_mm: 2.944,
        flipped: false,
    });
    frame.scale_factor = 2.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        pixels,
        target.read(&context),
        "DPI conversion must preserve physical geometry"
    );
    let stats = telemetry.snapshot();
    assert_eq!(
        (
            stats.cache_builds,
            stats.pipeline_builds,
            stats.uploaded_bytes
        ),
        (1, 1, 384)
    );

    gpu.content_mask.bounds.origin.x = ScaledPixels(64.0);
    gpu.content_mask.bounds.size.width = ScaledPixels(64.0);
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let clipped = target.read(&context);
    assert_eq!(rgb(&clipped, 32, 93), [0; 3]);
    assert!(rgb(&clipped, 96, 93)[0] > 240);
    renderer.reset();
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        clipped,
        target.read(&context),
        "reset must reconstruct the same image"
    );
    let rebuilt = telemetry.snapshot();
    assert_eq!(
        (
            rebuilt.pipeline_builds,
            rebuilt.cache_builds,
            rebuilt.resets,
            rebuilt.uploaded_bytes
        ),
        (2, 2, 1, 768)
    );
    // The last chunk straddles layers, exercising a non-zero SRV batch offset.
    let mut bulk = vec![segments[0].clone(); CHUNK_INSTANCES * 2 + 1];
    for (index, segment) in bulk.iter_mut().enumerate() {
        segment.id = ObjectId(1000 + index as u32);
    }
    bulk.extend_from_slice(&segments[1..]);
    let bulk_count = bulk.len();
    frame.tracks = Arc::new(
        PreparedTracks::build(&bulk, TraceLimits::default(), &CancellationToken::default())
            .unwrap(),
    );
    gpu.content_mask.bounds = bounds;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        telemetry.snapshot().uploaded_instances,
        (CHUNK_INSTANCES * 2) as u64
    );
    assert_eq!(
        rgb(&target.read(&context), 64, 29),
        [0; 3],
        "not-yet-uploaded circle cannot appear"
    );
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let bulk_pixels = target.read(&context);
    assert!(
        rgb(&bulk_pixels, 64, 29)[1] > 220,
        "last layer must use its offset within the chunk"
    );
    let bulk_stats = telemetry.snapshot();
    assert_eq!(
        (
            bulk_stats.cache_builds,
            bulk_stats.pipeline_builds,
            bulk_stats.uploaded_instances,
            bulk_stats.uploaded_bytes
        ),
        (3, 2, bulk_count as u64, 768 + (bulk_count * 128) as u64)
    );
    // The first chunk ends in a highlighted span. Later overlapping unselected
    // instances must overwrite it with the base color, not inherit that span.
    frame.highlighted_related_objects = Some((
        Arc::new(std::collections::BTreeSet::from([
            pomelo_core::selection::SelectedObject::Segment(ObjectId(
                1000 + CHUNK_INSTANCES as u32 - 1,
            )),
        ])),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let bulk_highlighted = target.read(&context);
    assert_eq!(rgb(&bulk_highlighted, 64, 93), [255, 0, 0]);
    assert_eq!(rgb(&bulk_highlighted, 64, 29), rgb(&bulk_pixels, 64, 29));
    frame.highlighted_related_objects = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(bulk_pixels, target.read(&context));
    let bulk_after_selection = telemetry.snapshot();
    assert_eq!(bulk_stats.cache_builds, bulk_after_selection.cache_builds);
    assert_eq!(
        bulk_stats.uploaded_bytes,
        bulk_after_selection.uploaded_bytes
    );
    assert_eq!(
        bulk_stats.uploaded_instances,
        bulk_after_selection.uploaded_instances
    );
    // Diagnostic synthetic glyph: exercise the complete text-to-HLSL path.
    let cancellation = CancellationToken::default();
    let font = crate::text::StrokeFont::load(
        [('A', "RRK[Y[")],
        crate::text::FontLimits {
            glyphs: 1,
            encoded_bytes: 6,
            points_per_glyph: 2,
            strokes: 1,
        },
        &cancellation,
    )
    .unwrap();
    let mut source_text = pomelo_core::model::BoardText {
        id: ObjectId(777),
        owner_id: None,
        layer: LayerId(1),
        class_id: 0,
        subclass: 0,
        text: "A".into(),
        at: Point::new(2.0, 10.0),
        angle: 0.0,
        mirrored: false,
        align: pomelo_core::model::TextAlignment::Left,
        font_index: 0,
        width: 16.0,
        height: 1.0,
        spacing: 0.0,
        line_spacing: 1.0,
        stroke_width: 1.0,
    };
    let prepared_text = crate::text::PreparedTexts::build(
        std::slice::from_ref(&source_text),
        &font,
        1,
        1,
        1,
        &cancellation,
    )
    .unwrap();
    let mut text_frame = TraceFrame {
        tracks: Arc::new(
            PreparedTracks::build_texts(&prepared_text, TraceLimits::default(), &cancellation)
                .unwrap(),
        ),
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(20.0, 20.0),
        },
        camera: None,
        scale_factor: 1.0,
        colors: Arc::clone(&frame.colors),
        fallback_color: [1.0; 4],
        highlighted_net: None,
        highlighted_objects: None,
        highlighted_related_objects: None,
        hovered_object: None,
        highlighted_trace: None,
        highlighted_object: None,
    };
    let text_stats = Arc::new(TraceTelemetry::default());
    let mut text_renderer = TraceRenderer::<PreparedTracks>::new(Arc::clone(&text_stats));
    target.bind(&context);
    text_renderer.draw(&gpu, &text_frame).unwrap();
    let text_pixels = target.read(&context);
    assert_eq!(rgb(&text_pixels, 64, 64), [255, 0, 0]);
    assert_eq!(rgb(&text_pixels, 64, 80), [0; 3]);
    assert_eq!(text_stats.snapshot().uploaded_instances, 1);
    let compact = Arc::new(
        crate::text_instances::PreparedTextInstances::build(
            std::slice::from_ref(&source_text),
            &font,
            1,
            1,
            1024,
            &cancellation,
        )
        .unwrap(),
    );
    let mut compact_cache = UploadedTracks::new(compact).unwrap();
    assert_eq!(compact_cache.upload_next(&device).unwrap(), 64);
    let compact_pipeline = Pipeline::new_text(&device).unwrap();
    target.bind(&context);
    compact_pipeline
        .draw(
            &gpu,
            &text_frame,
            &compact_cache,
            crate::backend::d3d11::board::TraceScope::All,
        )
        .unwrap();
    assert_eq!(text_pixels, target.read(&context));
    assert_eq!(compact_cache.upload_next(&device).unwrap(), 0);
    println!("COMPACT_TEXT_PIXELS_MATCH_LEGACY uploaded_bytes=64 instances=1");

    target.bind(&context);
    text_renderer.draw(&gpu, &text_frame).unwrap();
    assert_eq!(text_pixels, target.read(&context));
    assert_eq!(text_stats.snapshot().cache_builds, 1);
    assert_eq!(text_stats.snapshot().uploaded_bytes, 128);
    source_text.at = Point::new(10.0, 2.0);
    source_text.angle = std::f64::consts::FRAC_PI_2;
    let rotated_text = crate::text::PreparedTexts::build(
        std::slice::from_ref(&source_text),
        &font,
        1,
        1,
        1,
        &cancellation,
    )
    .unwrap();
    text_frame.tracks = Arc::new(
        PreparedTracks::build_texts(&rotated_text, TraceLimits::default(), &cancellation).unwrap(),
    );
    target.bind(&context);
    text_renderer.draw(&gpu, &text_frame).unwrap();
    let rotated_pixels = target.read(&context);
    assert_eq!(rgb(&rotated_pixels, 64, 64), [255, 0, 0]);
    assert_eq!(rgb(&rotated_pixels, 64, 80), [255, 0, 0]);
    assert_eq!(rgb(&rotated_pixels, 80, 64), [0; 3]);
    assert_eq!(text_stats.snapshot().cache_builds, 2);
    assert_eq!(text_stats.snapshot().uploaded_bytes, 256);
    text_renderer.reset();
    assert_eq!(text_stats.snapshot().uploaded_instances, 0);
    target.bind(&context);
    text_renderer.draw(&gpu, &text_frame).unwrap();
    assert_eq!(rotated_pixels, target.read(&context));
    let restored_text = text_stats.snapshot();
    assert_eq!(restored_text.cache_builds, 3);
    assert_eq!(restored_text.pipeline_builds, 2);
    assert_eq!(restored_text.resets, 1);
    assert_eq!(restored_text.uploaded_bytes, 384);
    if let Some(directory) = std::env::var_os("POMELO_STROKE_FONT_DIR") {
        use std::io::Read;
        let path = std::path::PathBuf::from(directory).join("4e.json");
        let mut data = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .take(65_537)
            .read_to_end(&mut data)
            .unwrap();
        let real_font = crate::text::StrokeFont::load_json(
            &data,
            65_536,
            crate::text::FontLimits {
                glyphs: 256,
                encoded_bytes: 65_536,
                points_per_glyph: 1024,
                strokes: 65_536,
            },
            &cancellation,
        )
        .unwrap();
        source_text.text = "中".into();
        source_text.at = Point::new(2.0, 2.0);
        source_text.angle = 0.0;
        source_text.width = 16.0;
        source_text.height = 16.0;
        source_text.stroke_width = 0.5;
        let real_text = crate::text::PreparedTexts::build(
            std::slice::from_ref(&source_text),
            &real_font,
            1,
            1,
            1024,
            &cancellation,
        )
        .unwrap();
        let stroke_count = real_text.strokes.len();
        assert!(stroke_count > 1);
        text_frame.tracks = Arc::new(
            PreparedTracks::build_texts(&real_text, TraceLimits::default(), &cancellation).unwrap(),
        );
        target.bind(&context);
        text_renderer.draw(&gpu, &text_frame).unwrap();
        let real_pixels = target.read(&context);
        let red_pixels = real_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > 220 && pixel[1] == 0 && pixel[2] == 0)
            .count();
        assert!(
            red_pixels > 100,
            "real CJK glyph must produce visible strokes"
        );
        let uploaded = text_stats.snapshot();
        assert_eq!(uploaded.uploaded_instances, stroke_count as u64);
        target.bind(&context);
        text_renderer.draw(&gpu, &text_frame).unwrap();
        assert_eq!(real_pixels, target.read(&context));
        assert_eq!(
            uploaded.uploaded_bytes,
            text_stats.snapshot().uploaded_bytes
        );
        assert_eq!(uploaded.cache_builds, text_stats.snapshot().cache_builds);
        println!(
            "EXTERNAL_CJK_GPU character=U+4E2D strokes={stroke_count} red_pixels={red_pixels}"
        );
    }
    if let Some(path) = std::env::var_os("POMELO_TEXT_BOARD_PATH") {
        use pomelo_import::{
            BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter,
        };
        let mut options = ImportOptions::default();
        if let Ok(encoding) = std::env::var("POMELO_TEXT_BOARD_ENCODING") {
            options.text_encoding = pomelo_import::TextEncoding::from_tag(&encoding)
                .expect("diagnostic board encoding must be a supported tag");
        }
        let board = AllegroImporter
            .import(
                std::path::Path::new(&path),
                &options,
                &ImportContext {
                    cancellation: &cancellation,
                    progress: &|_| {},
                },
            )
            .unwrap();
        assert!(!board.scene.texts.is_empty());
        let font = crate::text::StrokeFont::bundled_for_recovering_texts(
            &board.scene.texts,
            2_000_000,
            4 * 1024 * 1024,
            crate::text::FontLimits {
                glyphs: 65_536,
                encoded_bytes: 4 * 1024 * 1024,
                points_per_glyph: 1024,
                strokes: 1_000_000,
            },
            &cancellation,
        )
        .unwrap();
        let (text, diagnostics) = crate::text::PreparedTexts::build_recovering_missing_glyphs(
            &board.scene.texts,
            &font,
            1_000_000,
            2_000_000,
            TraceLimits::default().max_instances,
            &cancellation,
        )
        .unwrap();
        assert!(
            diagnostics.is_empty(),
            "this fixture must prepare every source text"
        );
        let legacy =
            PreparedTracks::build_texts(&text, TraceLimits::default(), &cancellation).unwrap();
        let (streamed, ids, summary) = PreparedTracks::build_source_texts(
            &board.scene.texts,
            &font,
            1_000_000,
            2_000_000,
            TraceLimits::default(),
            &cancellation,
        )
        .unwrap();
        assert_eq!(streamed.instances, legacy.instances);
        assert_eq!(streamed.batches, legacy.batches);
        assert_eq!(ids.len(), text.batches.len());
        assert!(summary.diagnostics.is_empty());
        drop(legacy);
        drop(text);
        text_frame.tracks = Arc::new(streamed);
        text_frame.bounds = board.scene.bounds;
        text_frame.colors = Arc::new(BTreeMap::new());
        text_frame.fallback_color = [1.0, 0.0, 0.0, 1.0];
        let board_text_stats = Arc::new(TraceTelemetry::default());
        let mut board_text_renderer =
            TraceRenderer::<PreparedTracks>::new(Arc::clone(&board_text_stats));
        let mut frames = 0;
        loop {
            assert!(
                frames < 256,
                "board text upload must finish within bounded frames"
            );
            target.bind(&context);
            board_text_renderer.draw(&gpu, &text_frame).unwrap();
            frames += 1;
            if board_text_stats.snapshot().uploaded_instances
                == text_frame.tracks.instances.len() as u64
            {
                break;
            }
        }
        let pixels = target.read(&context);
        let visible = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > 0)
            .count();
        assert!(visible > 0, "real BRD text must produce visible pixels");
        let compact = Arc::new(
            crate::text_instances::PreparedTextInstances::build(
                &board.scene.texts,
                &font,
                1_000_000,
                2_000_000,
                512 * 1024 * 1024,
                &cancellation,
            )
            .unwrap(),
        );
        let mut compact_cache = UploadedTracks::new(compact).unwrap();
        let compact_pipeline = Pipeline::new_text(&device).unwrap();
        let mut compact_bytes = 0;
        let mut compact_frames = 0;
        while compact_cache.uploaded() < compact_cache.source.instances.len() {
            assert!(compact_frames < 512);
            compact_bytes += compact_cache.upload_next(&device).unwrap();
            compact_frames += 1;
        }
        target.bind(&context);
        compact_pipeline
            .draw(
                &gpu,
                &text_frame,
                &compact_cache,
                crate::backend::d3d11::board::TraceScope::All,
            )
            .unwrap();
        assert_eq!(pixels, target.read(&context));
        assert_eq!(
            compact_bytes,
            compact_cache.source.instances.len() as u64 * 64
        );
        assert_eq!(compact_cache.upload_next(&device).unwrap(), 0);
        println!(
            "REAL_BOARD_COMPACT_TEXT_PIXELS_MATCH_LEGACY instances={} uploaded_bytes={} upload_frames={}",
            compact_cache.source.instances.len(),
            compact_bytes,
            compact_frames
        );

        let uploaded = board_text_stats.snapshot();
        target.bind(&context);
        board_text_renderer.draw(&gpu, &text_frame).unwrap();
        assert_eq!(pixels, target.read(&context));
        assert_eq!(
            uploaded.uploaded_bytes,
            board_text_stats.snapshot().uploaded_bytes
        );
        assert_eq!(uploaded.cache_builds, 1);
        println!(
            "REAL_BOARD_TEXT_GPU objects={} instances={} uploaded_bytes={} frames={frames} visible_pixels={visible}",
            board.scene.texts.len(),
            uploaded.uploaded_instances,
            uploaded.uploaded_bytes
        );
    }
    // A large coordinate translation must not erase the sub-millimetre geometry.
    let translation = Point::new(100_000.000_123, -100_000.000_456);
    let shifted = segments
        .iter()
        .cloned()
        .map(|mut segment| {
            for point in [&mut segment.a, &mut segment.b] {
                point.x += translation.x;
                point.y += translation.y;
            }
            if let Some(arc) = &mut segment.arc {
                arc.center.x += translation.x;
                arc.center.y += translation.y;
            }
            segment
        })
        .collect::<Vec<_>>();
    frame.tracks = Arc::new(
        PreparedTracks::build(
            &shifted,
            TraceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    );
    frame.camera.as_mut().unwrap().center = Point::new(translation.x + 10.0, translation.y + 10.0);
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let translated = target.read(&context);
    assert!(
        pixels
            .iter()
            .zip(&translated)
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "high/low coordinates must preserve the image"
    );
    frame.camera.as_mut().unwrap().flipped = true;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let flipped = target.read(&context);
    assert!(
        rgb(&flipped, 79, 61)[2] > 200,
        "flipped quarter arc must move to the opposite side"
    );
    assert_eq!(rgb(&flipped, 49, 61), [0; 3]);
    let final_stats = telemetry.snapshot();
    assert_eq!(
        (
            final_stats.cache_builds,
            final_stats.pipeline_builds,
            final_stats.uploaded_bytes
        ),
        (4, 2, 768 + ((bulk_count + 3) * 128) as u64)
    );
    // Exercise the same logical camera controller used by the desktop viewport.
    let mut navigation = pomelo_core::interaction::ViewportNavigation::default();
    assert!(navigation.resize(frame.bounds, f64::from(SIDE), f64::from(SIDE)));
    frame.tracks = Arc::clone(&tracks);
    frame.scale_factor = 1.0;
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(target.read(&context), pixels);
    let navigation_baseline = telemetry.snapshot();
    assert!(navigation.zoom_at(Point::new(64.0, 93.0), 2.0));
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(rgb(&target.read(&context), 64, 93)[0] > 240);
    assert!(navigation.pan(Point::new(0.0, -20.0)));
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let moved = target.read(&context);
    assert!(rgb(&moved, 64, 73)[0] > 240);
    assert_eq!(rgb(&moved, 64, 93), [0; 3]);
    let navigation_stats = telemetry.snapshot();
    assert_eq!(
        navigation_stats.uploaded_bytes,
        navigation_baseline.uploaded_bytes
    );
    assert_eq!(
        navigation_stats.cache_builds,
        navigation_baseline.cache_builds
    );
    assert_eq!(
        navigation_stats.pipeline_builds,
        navigation_baseline.pipeline_builds
    );
    let corners = [
        Point::new(1.0, 1.0),
        Point::new(19.0, 1.0),
        Point::new(19.0, 19.0),
        Point::new(1.0, 19.0),
    ];
    let outline = (0..4)
        .map(|index| Segment {
            id: ObjectId(100 + index as u32),
            track_id: ObjectId(100 + index as u32),
            layer: LayerId(1),
            net: NetId(0),
            a: corners[index],
            b: corners[(index + 1) % 4],
            width: 0.0,
            arc: None,
            bond_wire: None,
        })
        .collect::<Vec<_>>();
    frame.tracks = Arc::new(
        PreparedTracks::build_with_outline(
            &segments,
            &outline,
            TraceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    );
    assert!(navigation.fit(frame.bounds));
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let outline_pixels = target.read(&context);
    let outline_pixel = rgb(&outline_pixels, 11, 64);
    assert!(
        outline_pixel.into_iter().all(|channel| channel > 90),
        "outline uses neutral color, not the red trace layer color"
    );
    let outline_stats = telemetry.snapshot();
    assert_eq!(outline_stats.uploaded_instances, 7);
    if let Some(path) = std::env::var_os("POMELO_TRACE_PIXEL_OUTPUT") {
        image::save_buffer(&path, &pixels, SIDE, SIDE, image::ColorType::Rgba8).unwrap();
        image::save_buffer(
            std::path::Path::new(&path).with_extension("outline.png"),
            &outline_pixels,
            SIDE,
            SIDE,
            image::ColorType::Rgba8,
        )
        .unwrap();
        let report = serde_json::json!({ "adapter": adapter_name, "hardware_only": true,
            "scope": "synthetic_trace_pixels", "diagnostic_cpu_readback": true,
            "production_cpu_readback": false, "dimensions": [SIDE, SIDE], "statistics": rebuilt, "bulk_instances": bulk_count, "bulk_statistics": bulk_stats, "final_statistics": final_stats, "navigation_statistics": navigation_stats, "outline_statistics": outline_stats,
            "checks": ["round_caps", "full_circle_rims", "transparent_center", "antialiasing", "clip", "dpi", "cached_upload", "renderer_reset", "incremental_upload", "chunk_batch_offset", "source_revision", "negative_quarter_arc", "large_coordinates", "flipped_camera", "navigation_pointer_anchor", "navigation_pan", "navigation_reuses_geometry", "neutral_outline_hairline"] });
        std::fs::write(
            std::path::Path::new(&path).with_extension("json"),
            report.to_string(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "requires Windows hardware D3D11 and POMELO_TEXT_BOARD_PATH"]
fn hardware_real_board_compact_text_upload_draw_and_reset() {
    use crate::text_instances::PreparedTextInstances;
    use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
    let path = std::env::var_os("POMELO_TEXT_BOARD_PATH").expect("explicit board path required");
    let options = ImportOptions {
        text_encoding: pomelo_import::TextEncoding::from_tag(
            &std::env::var("POMELO_TEXT_BOARD_ENCODING").unwrap_or_else(|_| "windows-1252".into()),
        )
        .expect("supported source encoding required"),
        ..Default::default()
    };
    let cancellation = CancellationToken::default();
    let board = AllegroImporter
        .import(
            std::path::Path::new(&path),
            &options,
            &ImportContext {
                cancellation: &cancellation,
                progress: &|_| {},
            },
        )
        .unwrap();
    let font = crate::text::StrokeFont::bundled_for_recovering_texts(
        &board.scene.texts,
        2_000_000,
        4 * 1024 * 1024,
        crate::text::FontLimits {
            glyphs: 65_536,
            encoded_bytes: 4 * 1024 * 1024,
            points_per_glyph: 1024,
            strokes: 1_000_000,
        },
        &cancellation,
    )
    .unwrap();
    let texts = Arc::new(
        PreparedTextInstances::build(
            &board.scene.texts,
            &font,
            1_000_000,
            2_000_000,
            512 * 1024 * 1024,
            &cancellation,
        )
        .unwrap(),
    );
    assert!(
        texts.summary.diagnostics.is_empty(),
        "fixture must have complete text coverage"
    );
    assert_eq!(texts.objects.len(), board.scene.texts.len());
    assert!(!texts.instances.is_empty());
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs; HARDWARE cannot silently use a software adapter.
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let device = device.unwrap();
    let context = context.unwrap();
    let target = Target::new(&device);
    let bounds = ViewBounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    };
    let mut gpu = NativeGpuContext {
        device: &device,
        context: &context,
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let mut frame = TraceFrame {
        tracks: Arc::clone(&texts),
        bounds: board.scene.bounds,
        camera: None,
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::new()),
        fallback_color: [1.0, 0.0, 0.0, 1.0],
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let stats = Arc::new(TraceTelemetry::default());
    let mut renderer = TraceRenderer::<PreparedTextInstances>::new(Arc::clone(&stats));
    let mut frames = 0;
    while stats.snapshot().uploaded_instances != texts.instances.len() as u64 {
        assert!(
            frames
                < texts
                    .instances
                    .len()
                    .div_ceil(CHUNK_INSTANCES * CHUNKS_PER_FRAME)
                    + 1
        );
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        frames += 1;
    }
    let pixels = target.read(&context);
    let red_pixels = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 0)
        .count();
    assert!(red_pixels > 0);
    let uploaded = stats.snapshot();
    assert_eq!(uploaded.uploaded_bytes, texts.instances.len() as u64 * 64);
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    assert_eq!(stats.snapshot().uploaded_bytes, uploaded.uploaded_bytes);
    assert_eq!(stats.snapshot().cache_builds, 1);
    frame.fallback_color = [0.0, 1.0, 0.0, 1.0];
    gpu.content_mask.bounds.size.width = ScaledPixels(SIDE as f32 * 0.5);
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let clipped = target.read(&context);
    let green_pixels = clipped
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[1] > 0)
        .count();
    assert!(green_pixels > 0);
    for (index, pixel) in clipped.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(pixel[0], 0);
        if index % SIDE as usize >= SIDE as usize / 2 {
            assert_eq!(pixel[1], 0);
        }
    }
    assert_eq!(stats.snapshot().uploaded_bytes, uploaded.uploaded_bytes);
    renderer.reset();
    assert_eq!(stats.snapshot().uploaded_instances, 0);
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(stats.snapshot().cache_builds, 2);
    assert_eq!(stats.snapshot().pipeline_builds, 2);
    assert_eq!(
        stats.snapshot().uploaded_instances,
        texts
            .instances
            .len()
            .min(CHUNK_INSTANCES * CHUNKS_PER_FRAME) as u64
    );
    let mut reset_frames = 1;
    while stats.snapshot().uploaded_instances != texts.instances.len() as u64 {
        assert!(reset_frames <= frames);
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        reset_frames += 1;
    }
    assert_eq!(clipped, target.read(&context));
    assert_eq!(stats.snapshot().uploaded_bytes, uploaded.uploaded_bytes * 2);
    assert_eq!(reset_frames, frames);
    println!(
        "REAL_BOARD_COMPACT_TEXT_GPU objects={} instances={} uploaded_bytes={} upload_frames={} red_pixels={} clipped_green_pixels={} cache_rebuilt=true full_reset_reupload_verified=true window_verified=false",
        texts.objects.len(),
        texts.instances.len(),
        uploaded.uploaded_bytes,
        frames,
        red_pixels,
        green_pixels
    );
}
