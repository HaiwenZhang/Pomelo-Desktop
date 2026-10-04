//! Production D3D11 backdrill coverage against an independent Web formula oracle.
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{display::BoardDisplay, interaction::Camera, model::LayerId};

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_backdrill_patterns_match_web_and_follow_cut_layers_without_reupload() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs; only a physical D3D11 adapter is requested.
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
        point(ScaledPixels(12.0), ScaledPixels(8.0)),
        size(ScaledPixels(104.0), ScaledPixels(110.0)),
    );
    let mask = ViewBounds::new(
        point(ScaledPixels(20.0), ScaledPixels(14.0)),
        size(ScaledPixels(88.0), ScaledPixels(98.0)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds: mask },
    };
    let cancel = CancellationToken::default();
    let mut via = crate::drills::tests::backdrilled_via();
    via.at = Point::new(12345.6789, -9876.54321);
    let pads = Arc::new(
        crate::pads::PreparedPads::build(
            &[],
            std::slice::from_ref(&via),
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    let drills = Arc::new(
        crate::drills::PreparedDrills::build(
            &[],
            std::slice::from_ref(&via),
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap()
        .geometry,
    );
    let traces = TraceFrame {
        tracks: Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap()),
        bounds: Bounds {
            min: Point::new(via.at.x - 5.0, via.at.y - 5.0),
            max: Point::new(via.at.x + 5.0, via.at.y + 5.0),
        },
        camera: Some(Camera {
            center: Point::new(via.at.x + 0.137, via.at.y - 0.219),
            pixels_per_mm: 10.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(
            (0..4)
                .map(|layer| (LayerId(layer), [0.0, 0.0, 1.0, 1.0]))
                .collect(),
        ),
        fallback_color: [1.0; 4],
        material_override: None,
        opacity: 1.0,
        color_mode: pomelo_core::display::ColorMode::Net,
        pass: crate::backend::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    // Layer color mode makes the reference pad blue; backdrill stays green in either mode.
    let mut frame = BoardFrame {
        curves: None,
        traces,
        display: Arc::new(BoardDisplay::default()),
        pads: Some(pads),
        drills: Some(drills),
        copper: Arc::new(
            crate::copper::PreparedCopper::build(
                &[],
                crate::copper::CopperLimits::default(),
                &cancel,
            )
            .unwrap(),
        ),
        copper_opacity: 0.25,
        layer_order: Arc::new((0..4).map(LayerId).collect()),
        glyphs: None,
        labels: None,
        zone_outlines: None,
        drawings: None,
        texts: None,
        drill_color: [0.46, 0.49, 0.51, 1.0],
    };
    frame.traces.color_mode = pomelo_core::display::ColorMode::Layer;
    let pad_stats = Arc::new(TraceTelemetry::default());
    let drill_stats = Arc::new(TraceTelemetry::default());
    let mut renderer = BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::clone(&pad_stats),
        Arc::new(CopperTelemetry::default()),
        Arc::clone(&drill_stats),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    );
    for _ in 0..8 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let mut cases = Vec::new();
    let mut failures = Vec::new();
    for visible_layer in [Some(0), Some(1), Some(3), None] {
        for backdrills in [false, true] {
            for ordinary_drills in [false, true] {
                for filled in [false, true] {
                    for dpi in [1.0_f32, 2.0] {
                        for flipped in [false, true] {
                            let mut display = BoardDisplay {
                                show_drills: ordinary_drills,
                                show_backdrills: backdrills,
                                filled,
                                ..BoardDisplay::default()
                            };
                            display.hidden_layers.extend(
                                (0..4)
                                    .filter(|&layer| Some(layer) != visible_layer)
                                    .map(LayerId),
                            );
                            frame.display = Arc::new(display);
                            frame.traces.scale_factor = dpi;
                            frame.traces.camera.as_mut().unwrap().flipped = flipped;
                            target.bind(&context);
                            renderer.draw(&gpu, &frame).unwrap();
                            let pixels = target.read(&context);
                            let mut maximum = 0_u8;
                            let mut total = 0_u64;
                            for y in 0..SIDE {
                                for x in 0..SIDE {
                                    let sx = f64::from(x) + 0.5;
                                    let sy = f64::from(y) + 0.5;
                                    let mut rgb = [0.0; 3];
                                    if (20.0..108.0).contains(&sx) && (14.0..112.0).contains(&sy) {
                                        let zoom = 10.0 * f64::from(dpi);
                                        let mm_px = 0.1;
                                        let dx = (sx - 64.0) / zoom
                                            * if flipped { -1.0 } else { 1.0 }
                                            + 0.137;
                                        let dy = -(sy - 63.0) / zoom - 0.219;
                                        let r = dx.hypot(dy);
                                        let cut = matches!(visible_layer, Some(1 | 3));
                                        if visible_layer.is_some() && (!cut || !backdrills) {
                                            let outer = if cut { 4.0 } else { 3.0 };
                                            let distance = if filled {
                                                r - outer
                                            } else {
                                                (r - outer).abs() - mm_px * 0.65
                                            };
                                            rgb = blend(
                                                rgb,
                                                [0.0, 0.0, 1.0],
                                                1.0 - smooth(-mm_px * 0.65, mm_px * 0.65, distance),
                                            );
                                        }
                                        if visible_layer.is_some() && ordinary_drills {
                                            rgb = blend(
                                                rgb,
                                                [0.46, 0.49, 0.51],
                                                1.0 - smooth(-mm_px * 0.65, mm_px * 0.65, r - 1.0),
                                            );
                                        }
                                        if cut && backdrills {
                                            let sx = sx / f64::from(dpi);
                                            let sy = sy / f64::from(dpi);
                                            let cell = |v: f64| {
                                                ((v / 8.0 - (v / 8.0).floor()) - 0.5).abs() * 8.0
                                            };
                                            let hatch = 1.0
                                                - smooth(
                                                    0.35,
                                                    1.05,
                                                    cell(sx + sy).min(cell(sx - sy)),
                                                );
                                            let annulus =
                                                smooth(-mm_px * 0.65, mm_px * 0.65, r - 1.0);
                                            let pattern = hatch.max(annulus);
                                            let white = hatch / pattern.max(0.0001);
                                            let alpha = (1.0
                                                - smooth(-mm_px * 0.65, mm_px * 0.65, r - 4.0))
                                                * pattern;
                                            rgb = blend(rgb, [white, 1.0, white], alpha);
                                        }
                                    }
                                    for (channel, expected) in rgb.into_iter().enumerate() {
                                        let expected = (expected * 255.0).round() as u8;
                                        let diff = pixels[((y * SIDE + x) * 4) as usize + channel]
                                            .abs_diff(expected);
                                        maximum = maximum.max(diff);
                                        total += u64::from(diff);
                                    }
                                }
                            }
                            let mean = total as f64 / f64::from(SIDE * SIDE * 3);
                            cases.push(serde_json::json!({"visible_layer":visible_layer,"backdrills":backdrills,"drills":ordinary_drills,"filled":filled,"dpi":dpi,"flipped":flipped,"max_rgb_error":maximum,"mean_rgb_error":mean}));
                            if maximum > 4 || mean > 0.1 {
                                failures.push(format!("layer={visible_layer:?} back={backdrills} drills={ordinary_drills} filled={filled} dpi={dpi} flip={flipped}: max={maximum}, mean={mean}"));
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(pad_stats.snapshot().cache_builds, 1);
    assert_eq!(drill_stats.snapshot().cache_builds, 1);
    assert_eq!(drill_stats.snapshot().uploaded_bytes, 256);
    if let Some(dir) = std::env::var_os("POMELO_CANVAS_GPU_REPORT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("d3d11-backdrill-patterns.json"), serde_json::to_vec_pretty(&serde_json::json!({"hardware_only":true,"window_verified":false,"reference":"independent f64 Web circle/crosshatch formulas; blue pads then gray ordinary hole then green backdrill","cases":cases,"failures":failures})).unwrap()).unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} backdrill cases failed; first: {:?}",
        failures.len(),
        failures.first()
    );
}
fn smooth(lo: f64, hi: f64, value: f64) -> f64 {
    let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn blend(background: [f64; 3], color: [f64; 3], alpha: f64) -> [f64; 3] {
    std::array::from_fn(|i| background[i] * (1.0 - alpha) + color[i] * alpha)
}
