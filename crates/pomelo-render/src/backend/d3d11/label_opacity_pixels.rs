//! Existing per-glyph opacity metadata must survive mixed drill label batches.
use super::compositor_parity::{device, fixture, gpu_context};
use super::*;
use crate::backend::d3d11::{BoardRenderer, CopperTelemetry};
use pomelo_core::{
    display::DisplayCategory,
    model::{BoardText, TextAlignment},
};

#[test]
#[ignore = "requires Windows hardware D3D11; validates opacity metadata without changing font/layout"]
fn hardware_label_opacity_keeps_span_and_zone_independent_in_mixed_batches() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut frame = fixture();
    let shape_alpha = 99.0 / 255.0;
    frame.copper_opacity = shape_alpha;
    let label = |id, layer, text: &str, x, y| BoardText {
        id: ObjectId(id),
        owner_id: None,
        layer,
        class_id: 6,
        subclass: 0,
        text: text.into(),
        at: Point::new(x, y),
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
    let texts = [
        label(51, LayerId::UNASSIGNED, "1:8", -5.0, 3.0),
        label(52, LayerId::UNASSIGNED, "GND", 1.0, 3.0),
        label(60, LayerId(0), "GND", -4.0, -4.8),
    ];
    let cancel = CancellationToken::default();
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["1:8", "GND"], &cancel).unwrap());
    let mut glyphs =
        crate::text::msdf::PreparedGlyphs::build(&texts, font, 1024 * 1024, &cancel).unwrap();
    for glyph in &mut glyphs.instances {
        let zone = glyph.ids[0] == 60;
        let independent = zone || glyph.ids[0] == 51;
        glyph.ids[1] = if zone {
            DisplayCategory::Zone
        } else {
            DisplayCategory::Drill
        } as u32;
        glyph.rotation[3] += f32::from(u8::from(independent));
        glyph.color = if zone {
            [0.88, 0.91, 0.94, 0.62 * shape_alpha]
        } else {
            [1.0; 4]
        };
    }
    assert!(
        glyphs
            .batches
            .iter()
            .any(|batch| batch.layer == LayerId::UNASSIGNED && batch.count == 6),
        "span and via name must share one actual drill batch"
    );
    let labels = Arc::new(glyphs);
    frame.labels = Some(Arc::clone(&labels));
    let telemetry = Arc::new(TraceTelemetry::default());
    let mut renderer = BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    )
    .with_label_telemetry(Arc::clone(&telemetry));
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let full = target.read(&context);
    let uploaded = telemetry.snapshot().uploaded_bytes;
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let zero = target.read(&context);
    let mut span_ink = 0;
    for y in 18..40 {
        for x in 8..46 {
            let before = rgb(&full, x, y);
            if before[0] > 150 {
                assert_eq!(
                    rgb(&zero, x, y),
                    before,
                    "through span independent bit lost at {x},{y}"
                );
                span_ink += 1;
            }
        }
    }
    assert!(
        span_ink > 10,
        "span fixture must contain full-opacity white ink"
    );
    let mut via_ink = 0;
    for y in 18..40 {
        for x in 70..112 {
            if rgb(&full, x, y)[0] > 150 {
                via_ink += 1;
            }
            assert_eq!(
                rgb(&zero, x, y),
                [0; 3],
                "via net name must still follow global opacity"
            );
        }
    }
    assert!(
        via_ink > 10,
        "via name fixture must contain ordinary label ink"
    );
    for y in 100..113 {
        for x in 20..50 {
            let before = rgb(&full, x, y);
            assert_eq!(
                rgb(&zero, x, y),
                before,
                "zone name must ignore global alpha and retain its shape alpha"
            );
        }
    }
    Arc::make_mut(&mut frame.display).global_opacity = 128.0 / 255.0;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let half = target.read(&context);
    for y in 18..40 {
        for x in 70..112 {
            let before = rgb(&full, x, y);
            let after = rgb(&half, x, y);
            assert!(
                before
                    .iter()
                    .zip(after)
                    .all(|(a, b)| (f32::from(b) - f32::from(*a) * 128.0 / 255.0).abs() <= 1.0),
                "via label alpha must apply once: {before:?}/{after:?}"
            );
        }
    }
    assert_eq!(
        telemetry.snapshot().uploaded_bytes,
        uploaded,
        "opacity changes must not rebuild or upload label packets"
    );
    // Removing labels intentionally clears their cache. Perform this separate
    // visibility control only after verifying opacity changes require no upload.
    // This region otherwise contains independently translucent copper only.
    frame.labels = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let without_zone_label = target.read(&context);
    let mut zone_ink = 0;
    for y in 100..113 {
        for x in 20..50 {
            if rgb(&full, x, y) != rgb(&without_zone_label, x, y) {
                zone_ink += 1;
            }
        }
    }
    assert!(
        zone_ink > 10,
        "zone name must make a visible shape-opacity contribution"
    );
    eprintln!(
        "LABEL_OPACITY_GPU white_span_pixels={span_ink} ordinary_via_pixels={via_ink} independent_zone_pixels={zone_ink} zone_alpha=0p62_times_99_over_255=true mixed_drill_batch=true global_0_128_255=true unchanged_atlas_layout=true no_label_reupload=true allegro_board_validated=false"
    );
}
