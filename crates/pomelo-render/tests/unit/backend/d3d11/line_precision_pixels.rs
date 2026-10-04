//! Independent f64 capsule oracle for the production HLSL line pipeline.

use super::*;
use pomelo_core::{interaction::Camera, model::LayerId};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_long_lines_match_f64_capsules_at_deep_zoom() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; a real hardware adapter is required.
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
    // SAFETY: adapter interfaces belong to the live hardware device.
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
            .position(|v| *v == 0)
            .unwrap_or(adapter.Description.len())],
    );
    let target = Target::new(&device);
    // A displaced canvas and inset content mask exercise screen/world conversion.
    let bounds = ViewBounds::new(
        point(ScaledPixels(12.0), ScaledPixels(8.0)),
        size(ScaledPixels(104.0), ScaledPixels(110.0)),
    );
    let cancel = CancellationToken::default();
    let mut comparisons = Vec::new();
    let mut failures = Vec::new();
    for (kind, flag, width_pixels) in [
        ("trace", 0, 2.75),
        ("zone_edge", 8, 0.0),
        ("custom_pin_edge", 16, 0.0),
        ("drawing", 64, 1.5),
    ] {
        for angle in [0.0_f64, 0.371, -1.117, 2.137] {
            let (sin, cos) = angle.sin_cos();
            for zoom in [200.0, 20_000.0, 10_000_000.0] {
                let segment = Segment {
                    id: ObjectId(1),
                    track_id: ObjectId(2),
                    layer: LayerId(1),
                    net: NetId(0),
                    a: Point::new(12345.6789, -9876.54321),
                    b: Point::new(12345.6789 + 80.0 * cos, -9876.54321 + 80.0 * sin),
                    width: width_pixels / zoom,
                    arc: None,
                    bond_wire: None,
                };
                let mut tracks = PreparedTracks::build(
                    std::slice::from_ref(&segment),
                    TraceLimits::default(),
                    &cancel,
                )
                .unwrap();
                tracks.instances[0].flags[3] = flag;
                let mut frame = TraceFrame {
                    tracks: Arc::new(tracks),
                    bounds: segment.bounds().unwrap(),
                    camera: None,
                    scale_factor: 1.0,
                    colors: Arc::new(BTreeMap::new()),
                    fallback_color: [0.3, 0.7, 0.4, 1.0],
                    material_override: None,
                    opacity: 1.0,
                    color_mode: pomelo_core::display::ColorMode::Layer,
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
                let telemetry = Arc::new(TraceTelemetry::default());
                let mut renderer = TraceRenderer::<PreparedTracks>::new(Arc::clone(&telemetry));
                for (location, fraction, along_pixels) in [
                    ("middle", 0.173, 0.0),
                    ("start", 0.0, 0.0),
                    ("end", 1.0, 0.0),
                    ("offscreen_before", 0.0, -200.0),
                    ("offscreen_after", 1.0, 200.0),
                ] {
                    for dpi in [1.0_f32, 2.0] {
                        for flipped in [false, true] {
                            let center = Point::new(
                                segment.a.x
                                    + (segment.b.x - segment.a.x) * fraction
                                    + (cos * along_pixels - sin * 0.37) / zoom,
                                segment.a.y
                                    + (segment.b.y - segment.a.y) * fraction
                                    + (sin * along_pixels + cos * 0.37) / zoom,
                            );
                            frame.camera = Some(Camera {
                                center,
                                pixels_per_mm: zoom,
                                flipped,
                            });
                            frame.scale_factor = dpi;
                            let mask = if flipped {
                                ViewBounds::new(
                                    point(ScaledPixels(27.0), ScaledPixels(19.0)),
                                    size(ScaledPixels(72.0), ScaledPixels(85.0)),
                                )
                            } else {
                                bounds
                            };
                            let gpu = NativeGpuContext {
                                device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
                                viewport: [SIDE as f32; 2],
                                bounds,
                                content_mask: ContentMask { bounds: mask },
                            };
                            target.bind(&context);
                            renderer.draw(&gpu, &frame).unwrap();
                            let pixels = target.read(&context);
                            let mut maximum = 0_u8;
                            let mut total = 0_u64;
                            let mut visible = 0_usize;
                            let physical_zoom = f64::from((zoom * f64::from(dpi)) as f32);
                            let pixel_mm = f64::from(dpi) / physical_zoom;
                            let dx = segment.b.x - segment.a.x;
                            let dy = segment.b.y - segment.a.y;
                            let half_width = if flag & 8 != 0 {
                                pixel_mm * 0.65
                            } else {
                                (f64::from(segment.width as f32) * 0.5).max(pixel_mm * 0.5)
                            };
                            for y in 0..SIDE {
                                for x in 0..SIDE {
                                    let sx = f64::from(x) + 0.5;
                                    let sy = f64::from(y) + 0.5;
                                    let inside = sx >= f64::from(mask.origin.x.0)
                                        && sx < f64::from(mask.origin.x.0 + mask.size.width.0)
                                        && sy >= f64::from(mask.origin.y.0)
                                        && sy < f64::from(mask.origin.y.0 + mask.size.height.0);
                                    // Ordinary f64 closest-point capsule; no compensated
                                    // shader arithmetic or normal-frame construction here.
                                    let px = (sx - 64.0) / physical_zoom
                                        * if flipped { -1.0 } else { 1.0 }
                                        + (center.x - segment.a.x);
                                    let py =
                                        -(sy - 63.0) / physical_zoom + (center.y - segment.a.y);
                                    let t =
                                        ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                                    let distance = (px - t * dx).hypot(py - t * dy) - half_width;
                                    let u = ((distance + pixel_mm * 0.65) / (pixel_mm * 1.3))
                                        .clamp(0.0, 1.0);
                                    let alpha = if inside {
                                        1.0 - u * u * (3.0 - 2.0 * u)
                                    } else {
                                        0.0
                                    };
                                    visible += usize::from(alpha > 0.01);
                                    for (channel, color) in [0.3, 0.7, 0.4].into_iter().enumerate()
                                    {
                                        let expected = (color * alpha * 255.0).round() as u8;
                                        let actual =
                                            pixels[((y * SIDE + x) * 4) as usize + channel];
                                        let difference = actual.abs_diff(expected);
                                        maximum = maximum.max(difference);
                                        total += u64::from(difference);
                                    }
                                }
                            }
                            let mean = total as f64 / f64::from(SIDE * SIDE * 3);
                            comparisons.push(serde_json::json!({
                                "kind":kind,"angle":angle,"logical_pixels_per_mm":zoom,
                                "dpi":dpi,"flipped":flipped,"location":location,
                                "max_rgb_error":maximum,"mean_rgb_error":mean,
                                "reference_visible_pixels":visible
                            }));
                            if maximum > 4 || mean > 0.05 || (visible == 0 && along_pixels == 0.0) {
                                failures.push(format!(
                                    "{kind} angle={angle} zoom={zoom} dpi={dpi} flip={flipped} {location}: max={maximum}, mean={mean}"
                                ));
                            }
                        }
                    }
                }
                assert_eq!(telemetry.snapshot().cache_builds, 1);
                assert_eq!(telemetry.snapshot().uploaded_bytes, 128);
            }
        }
    }
    if let Some(dir) = std::env::var_os("POMELO_CANVAS_GPU_REPORT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("d3d11-long-line-precision.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "adapter":adapter_name,"hardware_only":true,"window_verified":false,
                "reference":"independent f64 closest-point capsule and Web smoothstep coverage",
                "shader_sha256":format!("{:x}",Sha256::digest(include_bytes!("../../../../src/shaders/d3d11/trace.hlsl"))),
                "cases":comparisons,"failures":failures
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} line precision cases failed; first: {:?}",
        failures.len(),
        failures.first()
    );
}
