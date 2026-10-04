//! Caller-supplied ANSI font experiment; no font files are embedded or substituted.
use super::*;
use crate::text::{AnsiStrokeFont, FontLimits, PreparedTexts, instances::PreparedTextInstances};
use pomelo_core::{interaction::Camera, model::BoardText};

#[test]
#[ignore = "requires hardware D3D11, POMELO_ANSI_FONT_PATH and POMELO_ANSI_TEXT_PATH"]
fn hardware_ansi_source_text_keeps_photoplot_width_and_pen_lifts() {
    let font_path = std::env::var_os("POMELO_ANSI_FONT_PATH").expect("explicit font path required");
    let text_path =
        std::env::var_os("POMELO_ANSI_TEXT_PATH").expect("explicit source text required");
    let texts: Vec<BoardText> = serde_json::from_slice(&std::fs::read(text_path).unwrap()).unwrap();
    assert_eq!(texts.len(), 1);
    let text = &texts[0];
    let token = CancellationToken::default();
    let font = AnsiStrokeFont::load(
        &std::fs::read(font_path).unwrap(),
        FontLimits {
            glyphs: 128,
            encoded_bytes: 128 * 1024,
            points_per_glyph: 1024,
            strokes: 65536,
        },
        &token,
    )
    .unwrap();
    let source = PreparedTexts::build(&texts, &font, 1, 1024, 65536, &token).unwrap();
    assert!(!source.strokes.is_empty());
    assert!(source.strokes.iter().all(|s| s.width == text.stroke_width));
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots, with a hardware adapter explicitly required.
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
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let center = Point::new(
        text.at.x + 2.0 * (text.width + text.spacing) + text.width * 0.5,
        text.at.y + text.height * 0.5,
    );
    let camera = Camera {
        center,
        pixels_per_mm: 64.0,
        flipped: false,
    };
    let prepared = Arc::new(
        PreparedTextInstances::build(&texts, &font, 1, 1024, 4 * 1024 * 1024, &token).unwrap(),
    );
    let mut frame = TraceFrame {
        pass: crate::backend::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        tracks: prepared,
        bounds: Bounds {
            min: Point::new(center.x - 1.0, center.y - 1.0),
            max: Point::new(center.x + 1.0, center.y + 1.0),
        },
        camera: Some(camera),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::new()),
        fallback_color: [38.0 / 255.0, 1.0, 38.0 / 255.0, 1.0],
        material_override: None,
        opacity: 1.0,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let stats = Arc::new(TraceTelemetry::default());
    let mut renderer = TraceRenderer::<PreparedTextInstances>::new(Arc::clone(&stats));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    let mut inside = 0;
    let mut outside = 0;
    // The oracle is the source pen path, not a glyph bitmap or shader reimplementation.
    // Exclude AA edges so this check measures physical stroke width and empty pen lifts.
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = Point::new(
                center.x + (x as f64 + 0.5 - 64.0) / 64.0,
                center.y - (y as f64 + 0.5 - 64.0) / 64.0,
            );
            let distance = source
                .strokes
                .iter()
                .map(|s| {
                    let a = Point::new(s.a[0], s.a[1]);
                    let b = Point::new(s.b[0], s.b[1]);
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    let length2 = dx * dx + dy * dy;
                    let t = if length2 == 0.0 {
                        0.0
                    } else {
                        ((p.x - a.x) * dx + (p.y - a.y) * dy) / length2
                    }
                    .clamp(0.0, 1.0);
                    p.distance(Point::new(a.x + t * dx, a.y + t * dy)) - s.width * 0.5
                })
                .fold(f64::INFINITY, f64::min)
                * 64.0;
            if distance < -2.0 {
                assert_eq!(rgb(&pixels, x, y), [38, 255, 38], "source ink at {x},{y}");
                inside += 1;
            } else if distance > 2.0 {
                assert_eq!(rgb(&pixels, x, y), [0; 3], "pen lift at {x},{y}");
                outside += 1;
            }
        }
    }
    assert!(inside > 100 && outside > 100);
    let uploaded = stats.snapshot().uploaded_bytes;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(pixels, target.read(&context));
    assert_eq!(stats.snapshot().uploaded_bytes, uploaded);
    if let Some(output) = std::env::var_os("POMELO_ANSI_PIXELS_PATH") {
        image::save_buffer(output, &pixels, SIDE, SIDE, image::ColorType::Rgba8).unwrap();
    }
    frame.opacity = 0.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(
        target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3])
    );
    assert_eq!(stats.snapshot().uploaded_bytes, uploaded);
    // Use the production board compositor with simultaneous MSDF fallback.
    // Separate counters are essential: sharing uploaded_instances can otherwise
    // leave the application requesting frames forever after both sources upload.
    let msdf_font = Arc::new(crate::text::msdf::MsdfFont::bundled(["I"], &token).unwrap());
    let mut fallback = text.clone();
    fallback.id = ObjectId(9000);
    fallback.text = "I".into();
    fallback.at = Point::new(center.x - 0.9, center.y + 0.65);
    fallback.width = 0.15;
    fallback.height = 0.25;
    let glyphs = Arc::new(
        crate::text::msdf::PreparedGlyphs::build(&[fallback], msdf_font, 1024 * 1024, &token)
            .unwrap(),
    );
    let mut tracks = frame.with_source(Arc::new(
        crate::tracks::PreparedTracks::build_with_outline(
            &[],
            &[],
            crate::tracks::TraceLimits::default(),
            &token,
        )
        .unwrap(),
    ));
    tracks.opacity = 1.0;
    let display = pomelo_core::display::BoardDisplay {
        show_texts: true,
        ..Default::default()
    };
    let mut board = crate::backend::BoardFrame {
        traces: tracks,
        copper: Arc::new(
            crate::copper::PreparedCopper::build(
                &[],
                crate::copper::CopperLimits::default(),
                &token,
            )
            .unwrap(),
        ),
        glyphs: Some(Arc::clone(&glyphs)),
        texts: Some(Arc::clone(&frame.tracks)),
        labels: None,
        zone_outlines: None,
        drawings: None,
        curves: None,
        display: Arc::new(display),
        pads: None,
        drills: None,
        drill_color: [0.0; 4],
        copper_opacity: 1.0,
        layer_order: Arc::new(vec![text.layer]),
    };
    let glyph_stats = Arc::new(TraceTelemetry::default());
    let stroke_stats = Arc::new(TraceTelemetry::default());
    let mut compositor = crate::backend::BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(crate::backend::CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(crate::backend::CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::clone(&glyph_stats),
    )
    .with_source_stroke_telemetry(Arc::clone(&stroke_stats));
    for _ in 0..4 {
        target.bind(&context);
        compositor.draw(&gpu, &board).unwrap();
    }
    let combined = target.read(&context);
    assert_eq!(
        stroke_stats.snapshot().uploaded_instances,
        frame.tracks.instances.len() as u64
    );
    assert_eq!(
        glyph_stats.snapshot().uploaded_instances,
        glyphs.instances.len() as u64
    );
    assert!(stroke_stats.snapshot().draw_calls > 0 && glyph_stats.snapshot().draw_calls > 0);
    // The M's interior is unaffected by the fallback letter in the upper left.
    for y in 35..100 {
        for x in 35..100 {
            if rgb(&pixels, x, y) == [38, 255, 38] {
                assert_eq!(rgb(&combined, x, y), [38, 255, 38]);
            }
        }
    }
    let stroke_bytes = stroke_stats.snapshot().uploaded_bytes;
    let glyph_bytes = glyph_stats.snapshot().uploaded_bytes;
    Arc::make_mut(&mut board.display).global_opacity = 0.0;
    target.bind(&context);
    compositor.draw(&gpu, &board).unwrap();
    assert!(
        target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3])
    );
    Arc::make_mut(&mut board.display).global_opacity = 1.0;
    target.bind(&context);
    compositor.draw(&gpu, &board).unwrap();
    assert_eq!(combined, target.read(&context));
    assert_eq!(stroke_stats.snapshot().uploaded_bytes, stroke_bytes);
    assert_eq!(glyph_stats.snapshot().uploaded_bytes, glyph_bytes);
    println!(
        "ANSI_SOURCE_GPU object={} strokes={} width_mm={} inside={} outside={} uploaded_bytes={uploaded}",
        text.id.0,
        source.strokes.len(),
        text.stroke_width,
        inside,
        outside
    );
}
