//! Hardware-only diagnostic readback. Never compiled into the production renderer.

use super::*;
use crate::{
    backend::d3d11::{TraceRenderer, TraceTelemetry},
    tracks::TraceLimits,
};
use gpui::NativeGpuRenderer;
use gpui::{Bounds as ViewBounds, ContentMask, ScaledPixels, point, size};
use pomelo_core::{
    model::{Arc as BoardArc, Bounds, LayerId, NetId, ObjectId, Point, Segment},
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

#[path = "appearance_pixels.rs"]
mod appearance;

#[path = "compositor_parity_pixels.rs"]
mod compositor_parity;

#[path = "static_shape_pixels.rs"]
mod static_shape;

#[path = "label_opacity_pixels.rs"]
mod label_opacity;
#[path = "zone_highlight_pixels.rs"]
mod zone_highlight;

#[path = "dynamic_outline_pixels.rs"]
mod dynamic_outline;

#[path = "ansi_text_pixels.rs"]
mod ansi_text;

#[path = "line_precision_pixels.rs"]
mod line_precision;

#[path = "backdrill_pixels.rs"]
mod backdrill;

#[path = "die_pad_pixels.rs"]
mod die_pad;

#[path = "curve_fill_pixels.rs"]
mod curve_fill;

#[path = "s5000c_u94_pixels.rs"]
mod s5000c_u94;

#[path = "s5000c_full_geometry_pixels.rs"]
mod s5000c_full_geometry;

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_custom_pad_outlines_preserve_holes_owner_kind_and_cache() {
    use crate::backend::d3d11::{BoardFrame, BoardRenderer, CopperTelemetry};
    use pomelo_core::{
        display::BoardDisplay,
        interaction::Camera,
        model::{CustomPadGeometry, DrillShape, LayerId, Pad, PadKind, Pin, Via},
        selection::SelectedObject,
    };
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; only a real hardware adapter is requested.
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
        device: &device,
        context: &context,
        viewport: [SIDE as f32, SIDE as f32],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let cancel = CancellationToken::default();
    let square = |r: f64| {
        vec![
            Point::new(-r, -r),
            Point::new(r, -r),
            Point::new(r, r),
            Point::new(-r, r),
        ]
    };
    let mut pad = Pad::circle(LayerId(1), 4.0);
    pad.kind = PadKind::CUSTOM;
    pad.custom = Some(Arc::new(CustomPadGeometry {
        contours: vec![square(2.0), square(0.5)],
        paths: vec![],
    }));
    let drill = DrillShape {
        width: 0.0,
        height: 0.0,
        plated: false,
    };
    let pin = Pin {
        id: ObjectId(50),
        owner_id: ObjectId(49),
        net: NetId(1),
        name: String::new(),
        reference: String::new(),
        at: Point::new(-3.0, 0.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: drill,
        pads: vec![pad],
        stackup_region: None,
        die: None,
    };
    let mut disk = Pad::circle(LayerId(1), 3.0);
    disk.kind = PadKind::CUSTOM;
    disk.custom = Some(Arc::new(CustomPadGeometry {
        contours: vec![
            (0..32)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 32.0;
                    Point::new(a.cos() * 1.5, a.sin() * 1.5)
                })
                .collect(),
        ],
        paths: vec![vec![Segment {
            id: ObjectId(0),
            track_id: ObjectId(0),
            layer: LayerId(0),
            net: NetId(0),
            a: Point::new(1.5, 0.0),
            b: Point::new(1.5, 0.0),
            width: 0.0,
            arc: Some(BoardArc {
                center: Point::default(),
                radius: 1.5,
                start: 0.0,
                sweep: std::f64::consts::TAU,
            }),
            bond_wire: None,
        }]],
    }));
    // Equal numeric IDs must retain separate pin/via identity in the overlay pass.
    let via = Via {
        id: pin.id,
        net: pin.net,
        at: Point::new(3.0, 0.0),
        drill: 0.0,
        drill_shape: drill,
        padstack: ObjectId(51),
        padstack_name: String::new(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(1)),
        pads: Arc::from([disk]),
        backdrill: None,
        stackup_region: None,
        angle: 0.47,
        mirrored: true,
        finger: None,
    };
    let mut pads = crate::pads::PreparedPads::build(
        &[pin],
        &[via],
        crate::pads::PadLimits::default(),
        &cancel,
    )
    .unwrap();
    pads.custom_mesh = Some(Arc::new(
        pads.build_custom_meshes(
            crate::copper::CopperLimits::default(),
            &pomelo_core::copper::MeshLimits::default(),
            &cancel,
        )
        .unwrap(),
    ));
    let tracks = Arc::new(
        PreparedTracks::build(
            &[Segment {
                id: ObjectId(10),
                track_id: ObjectId(10),
                layer: LayerId(0),
                net: NetId(2),
                a: Point::new(-5.0, 0.0),
                b: Point::new(-1.0, 0.0),
                width: 0.2,
                arc: None,
                bond_wire: None,
            }],
            TraceLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    let traces = TraceFrame {
        tracks,
        bounds: Bounds {
            min: Point::new(-6.0, -6.0),
            max: Point::new(6.0, 6.0),
        },
        camera: Some(Camera {
            center: Point::default(),
            pixels_per_mm: 10.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::from([
            (LayerId(0), [1.0, 0.0, 0.0, 1.0]),
            (LayerId(1), [0.0, 0.0, 1.0, 1.0]),
        ])),
        fallback_color: [1.0; 4],
        material_override: None,
        opacity: 1.0,
        color_mode: pomelo_core::display::ColorMode::Layer,
        pass: super::super::OverlayPass::Base,
        filled: false,
        hover_selection: None,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let display = BoardDisplay {
        filled: false,
        layer_order: vec![LayerId(0), LayerId(1)],
        ..BoardDisplay::default()
    };
    let mut frame = BoardFrame {
        curves: None,
        traces,
        display: Arc::new(display),
        pads: Some(Arc::new(pads)),
        copper: Arc::new(
            crate::copper::PreparedCopper::build(
                &[],
                crate::copper::CopperLimits::default(),
                &cancel,
            )
            .unwrap(),
        ),
        copper_opacity: 0.25,
        layer_order: Arc::new(vec![LayerId(0), LayerId(1)]),
        glyphs: None,
        labels: None,
        zone_outlines: None,
        drawings: None,
        texts: None,
        drills: None,
        drill_color: [0.5; 4],
    };
    let outlines = Arc::new(TraceTelemetry::default());
    let mut renderer = BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    )
    .with_custom_outline_telemetry(Arc::clone(&outlines));
    for _ in 0..12 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let baseline = target.read(&context);
    assert!(
        rgb(&baseline, 14, 54)[2] > 70,
        "exterior outline must be visible"
    );
    assert!(
        rgb(&baseline, 29, 62)[2] > 70,
        "hole outline must be visible"
    );
    assert!(
        rgb(&baseline, 109, 64)[2] > 70,
        "mirrored/rotated analytic via circle must be visible"
    );
    assert!(
        rgb(&baseline, 34, 64)[0] > 180 && rgb(&baseline, 34, 64)[2] == 0,
        "hole must retain the underlying trace"
    );
    assert!(
        rgb(&baseline, 24, 64)[0] > 180 && rgb(&baseline, 24, 64)[2] == 0,
        "non-filled custom pad interior must retain the underlying trace"
    );
    let uploaded = outlines.snapshot();
    assert_eq!(uploaded.uploaded_instances, 9);
    frame.traces.highlighted_object = Some((SelectedObject::Pin(ObjectId(50)), [1.0; 4]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected = target.read(&context);
    assert!(
        rgb(&selected, 14, 54)[0] > 70,
        "pin selection must highlight its exterior"
    );
    assert_eq!(
        rgb(&selected, 109, 64),
        rgb(&baseline, 109, 64),
        "same-ID via must remain unselected"
    );
    assert_eq!(
        rgb(&selected, 24, 64),
        rgb(&baseline, 24, 64),
        "selection must not fill the empty pad interior"
    );
    frame.traces.highlighted_object = None;
    frame.traces.hovered_object = Some((SelectedObject::Via(ObjectId(50)), [1.0; 4]));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let hovered = target.read(&context);
    assert!(
        rgb(&hovered, 109, 64)[1] > 70,
        "via hover must use its own analytic boundary"
    );
    assert_eq!(rgb(&hovered, 14, 54), rgb(&baseline, 14, 54));
    frame.traces.hovered_object = None;
    Arc::make_mut(&mut frame.display)
        .hidden_layers
        .insert(LayerId(1));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let hidden = target.read(&context);
    assert_eq!(rgb(&hidden, 14, 54), [0, 0, 0]);
    assert_eq!(rgb(&hidden, 109, 64), [0, 0, 0]);
    let stats = outlines.snapshot();
    assert_eq!(stats.uploaded_bytes, uploaded.uploaded_bytes);
    assert_eq!(
        stats.cache_builds, 1,
        "display and overlays must not rebuild immutable edges"
    );
    renderer.reset();
    assert_eq!(outlines.snapshot().resets, 1);
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_msdf_atlas_matches_web_sampling_rotation_mirroring_and_dpi() {
    use crate::text::msdf::{MsdfFont, PreparedGlyphs};
    use pomelo_core::{
        interaction::Camera,
        model::{BoardText, LayerId, TextAlignment},
    };
    let (mut device, mut context) = (None, None);
    // SAFETY: valid initialized outputs, hardware device only, checked returned handles.
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
    let device = device.unwrap();
    let context = context.unwrap();
    let target = Target::new(&device);
    let gpu = gpui::NativeGpuContext {
        device: &device,
        context: &context,
        viewport: [SIDE as f32, SIDE as f32],
        bounds: ViewBounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
        ),
        content_mask: ContentMask {
            bounds: ViewBounds::new(
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
            ),
        },
    };
    let cancel = CancellationToken::default();
    let font = Arc::new(MsdfFont::bundled(["G铜"], &cancel).unwrap());
    let tracks = Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap());
    let mut colors = BTreeMap::new();
    colors.insert(LayerId(0), [0.3, 0.7, 0.4, 1.0]);
    let base = TraceFrame {
        tracks,
        bounds: Bounds {
            min: Point::new(-6.0, -6.0),
            max: Point::new(6.0, 6.0),
        },
        camera: Some(Camera {
            center: Point::default(),
            pixels_per_mm: 10.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(colors),
        fallback_color: [1.0; 4],
        material_override: None,
        opacity: 1.0,
        color_mode: pomelo_core::display::ColorMode::Layer,
        pass: super::super::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let mut comparisons = Vec::new();
    for ch in ["G", "铜"] {
        for mirrored in [false, true] {
            for flipped in [false, true] {
                for angle in [0.0, 0.47] {
                    for dpi in [1.0f32, 2.0] {
                        let text = BoardText {
                            id: ObjectId(71),
                            owner_id: None,
                            layer: LayerId(0),
                            class_id: 0,
                            subclass: 0,
                            text: ch.into(),
                            at: Point::new(-1.0, -1.0),
                            angle,
                            mirrored,
                            align: TextAlignment::Center,
                            font_index: 0,
                            width: 3.0,
                            height: 3.0,
                            spacing: 0.0,
                            line_spacing: 4.0,
                            stroke_width: 0.1,
                        };
                        let source = Arc::new(
                            PreparedGlyphs::build(&[text], Arc::clone(&font), 1024 * 1024, &cancel)
                                .unwrap(),
                        );
                        let mut frame = base.with_source(Arc::clone(&source));
                        frame.scale_factor = dpi;
                        frame.camera = Some(Camera {
                            pixels_per_mm: 10.0 / f64::from(dpi),
                            flipped,
                            ..Camera::default()
                        });
                        let mut renderer: TraceRenderer<PreparedGlyphs> =
                            TraceRenderer::new(Arc::new(TraceTelemetry::default()));
                        target.bind(&context);
                        renderer.draw(&gpu, &frame).unwrap();
                        let actual = target.read(&context);
                        let glyph = &source.instances[0];
                        let page = font.pages.iter().find(|p| p.page == glyph.page()).unwrap();
                        let xy = [
                            f64::from(glyph.xywh[0]) + f64::from(glyph.low[0]),
                            f64::from(glyph.xywh[1]) + f64::from(glyph.low[1]),
                        ];
                        let (w, h) = (f64::from(glyph.xywh[2]), f64::from(glyph.xywh[3]));
                        let (cos, sin) =
                            (f64::from(glyph.rotation[0]), f64::from(glyph.rotation[1]));
                        let uv = glyph.uv.map(f64::from);
                        let du = uv[2] - uv[0];
                        let dv = uv[1] - uv[3];
                        let dx = ((cos / 10.0 / w * du * f64::from(page.width)).powi(2)
                            + (-sin / 10.0 / h * dv * f64::from(page.height)).powi(2))
                        .sqrt();
                        let dy = ((-sin / 10.0 / w * du * f64::from(page.width)).powi(2)
                            + (-cos / 10.0 / h * dv * f64::from(page.height)).powi(2))
                        .sqrt();
                        let softness = 0.5 / (4.0 / dx.max(dy).max(0.0001)).max(1.0);
                        let mut max_error = 0u8;
                        let mut total = 0u64;
                        for y in 0..SIDE {
                            for x in 0..SIDE {
                                let px = (f64::from(x) + 0.5 - 64.0) / 10.0
                                    * if flipped { -1.0 } else { 1.0 }
                                    - xy[0];
                                let py = -(f64::from(y) + 0.5 - 64.0) / 10.0 - xy[1];
                                let u = (px * cos + py * sin) / w;
                                let v = (-px * sin + py * cos) / h;
                                let alpha = if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
                                {
                                    let tx = (uv[0] + u * du) * f64::from(page.width) - 0.5;
                                    let ty = (uv[3] + v * dv) * f64::from(page.height) - 0.5;
                                    let sample = |channel: usize| {
                                        let ix = tx.floor() as i64;
                                        let iy = ty.floor() as i64;
                                        let fx = tx - tx.floor();
                                        let fy = ty - ty.floor();
                                        let pixel = |x: i64, y: i64| {
                                            f64::from(
                                                page.rgba[(y.clamp(0, i64::from(page.height) - 1)
                                                    as usize
                                                    * page.width as usize
                                                    + x.clamp(0, i64::from(page.width) - 1)
                                                        as usize)
                                                    * 4
                                                    + channel],
                                            ) / 255.0
                                        };
                                        (pixel(ix, iy) * (1.0 - fx) + pixel(ix + 1, iy) * fx)
                                            * (1.0 - fy)
                                            + (pixel(ix, iy + 1) * (1.0 - fx)
                                                + pixel(ix + 1, iy + 1) * fx)
                                                * fy
                                    };
                                    let r = sample(0);
                                    let g = sample(1);
                                    let b = sample(2);
                                    let median = r.min(g).max(r.max(g).min(b));
                                    let t = ((median - (0.5 - softness)) / (softness * 2.0))
                                        .clamp(0.0, 1.0);
                                    t * t * (3.0 - 2.0 * t)
                                } else {
                                    0.0
                                };
                                for (c, color) in [0.3, 0.7, 0.4].into_iter().enumerate() {
                                    let expected = (color * alpha * 255.0).round() as u8;
                                    let error = actual[((y * SIDE + x) * 4) as usize + c]
                                        .abs_diff(expected);
                                    max_error = max_error.max(error);
                                    total += u64::from(error);
                                }
                            }
                        }
                        let mean = total as f64 / f64::from(SIDE * SIDE * 3);
                        comparisons.push(serde_json::json!({"text":ch,"mirrored":mirrored,"flipped":flipped,"angle":angle,"dpi":dpi,"max_rgb_error":max_error,"mean_rgb_error":mean}));
                        assert!(
                            max_error <= 4 && mean <= 0.05,
                            "MSDF sampling mismatch {ch} mirror={mirrored} flip={flipped} angle={angle} DPI={dpi}: max={max_error}, mean={mean}"
                        );
                    }
                }
            }
        }
    }
    if let Some(dir) = std::env::var_os("POMELO_CANVAS_GPU_REPORT_DIR") {
        let path = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("d3d11-msdf-sampling.json"),
            serde_json::to_vec_pretty(&comparisons).unwrap(),
        )
        .unwrap();
    }
}

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
        kind: pomelo_core::model::ZoneKind::Unknown,
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
        pass: crate::backend::d3d11::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
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
        material_override: None,
        opacity: 1.0,
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
    // Exercise the actual MSDF callback inside the zone stencil, including overlapping holes.
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["G"], &cancellation).unwrap());
    let mut glyphs = crate::text::msdf::PreparedGlyphs::build(
        &[pomelo_core::model::BoardText {
            id: zone.id,
            owner_id: None,
            layer: zone.layer,
            class_id: 0,
            subclass: 0,
            text: "G".into(),
            at: Point::new(0.0, 0.0),
            angle: 0.0,
            mirrored: false,
            align: pomelo_core::model::TextAlignment::Left,
            font_index: 0,
            width: 20.0,
            height: 20.0,
            spacing: 0.0,
            line_spacing: 20.0,
            stroke_width: 0.0,
        }],
        font,
        1024 * 1024,
        &cancellation,
    )
    .unwrap();
    for glyph in &mut glyphs.instances {
        glyph.ids[1] = pomelo_core::display::DisplayCategory::Zone as u32;
    }
    let glyph_frame = frame.base().with_source(Arc::new(glyphs));
    let mut glyph_renderer = TraceRenderer::<crate::text::msdf::PreparedGlyphs>::new(Arc::new(
        TraceTelemetry::default(),
    ));
    target.bind(&context);
    glyph_renderer.draw(&gpu, &glyph_frame).unwrap();
    let unmasked = target.read(&context);
    let mut annotated = CopperRenderer::new(Arc::new(CopperTelemetry::default()));
    annotated.prepare(&gpu, &copper, true).unwrap();
    target.bind(&context);
    traces.draw(&gpu, &frame).unwrap();
    annotated
        .draw_annotated(&gpu, &frame, 0.5, zone.layer, &mut |_| {
            glyph_renderer.draw_prepared(
                &gpu,
                &glyph_frame,
                crate::backend::d3d11::board::TraceScope::Labels(
                    zone.layer,
                    pomelo_core::display::DisplayCategory::Zone,
                    Some(zone.id),
                ),
            )
        })
        .unwrap();
    let masked = target.read(&context);
    let mut camera = pomelo_core::interaction::Camera::default();
    assert!(camera.fit(
        frame.bounds,
        f64::from(SIDE),
        f64::from(SIDE),
        f64::from(SIDE) * 0.04
    ));
    let mut shown = 0;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = camera.view_to_board(
                Point::new(x as f64 + 0.5, y as f64 + 0.5),
                f64::from(SIDE),
                f64::from(SIDE),
            );
            let in_hole = (p.x > 4.1 && p.x < 11.9 && p.y > 4.1 && p.y < 11.9)
                || (p.x > 8.1 && p.x < 15.9 && p.y > 8.1 && p.y < 15.9);
            let outside = p.x < 0.9 || p.x > 19.1 || p.y < 0.9 || p.y > 19.1;
            if in_hole || outside {
                assert_eq!(
                    rgb(&masked, x, y),
                    rgb(&pixels, x, y),
                    "zone label escaped its stencil at {x},{y}"
                );
            } else if p.x > 1.1
                && p.x < 18.9
                && p.y > 1.1
                && p.y < 18.9
                && rgb(&unmasked, x, y)[0] > 100
                && rgb(&masked, x, y) != rgb(&pixels, x, y)
            {
                shown += 1;
            }
        }
    }
    assert!(
        shown > 50,
        "visible MSDF label must survive outside the holes: {shown}"
    );
    assert_eq!(rgb(&masked, 64, 64), [255, 0, 0]);
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
        kind: pomelo_core::model::ZoneKind::Unknown,
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
        curves: None,
        glyphs: None,
        labels: None,
        zone_outlines: None,
        drawings: None,
        texts: None,
        traces: frame,
        display: Arc::new(pomelo_core::display::BoardDisplay::default()),
        pads: None,
        drills: None,
        drill_color: [0.46, 0.49, 0.51, 1.0],
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
    let original_display = Arc::clone(&board_frame.display);
    let mut display_order = (*board_frame.display).clone();
    assert!(display_order.move_layer_to_edge(LayerId(3), true, &original_order));
    board_frame.layer_order =
        Arc::new(display_order.ordered_layers(original_order.iter().copied()));
    board_frame.display = Arc::new(display_order);
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
    board_frame.display = original_display;
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
    Arc::make_mut(&mut board_frame.display).priorities =
        vec![pomelo_core::display::LayerPriority {
            layer: LayerId(99),
            category: pomelo_core::display::DisplayCategory::Text,
        }];
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
    board_frame.traces.color_mode = pomelo_core::display::ColorMode::Net;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let net_pixels = target.read(&context);
    let mut solid_drawing_pixels = 0;
    for (before, after) in drawing_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(net_pixels.as_chunks::<4>().0)
    {
        if before[..3] == [0, 0, 255] {
            assert_eq!(before, after, "drawing ink ignores network material mode");
            solid_drawing_pixels += 1;
        }
    }
    assert!(solid_drawing_pixels > 10);
    board_frame.traces.color_mode = pomelo_core::display::ColorMode::Layer;
    let visible_drawing_display = Arc::clone(&board_frame.display);
    let mut hidden_drawings = (*board_frame.display).clone();
    hidden_drawings.show_drawings = false;
    board_frame.display = Arc::new(hidden_drawings);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 64), without_dimension);
    board_frame.display = visible_drawing_display;
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
    Arc::make_mut(&mut board_frame.display).show_texts = true;
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
    board_frame.traces.color_mode = pomelo_core::display::ColorMode::Net;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let net_pixels = target.read(&context);
    let mut solid_text_pixels = 0;
    for (before, after) in text_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(net_pixels.as_chunks::<4>().0)
    {
        if before[..3] == [0, 0, 255] {
            assert_eq!(
                before, after,
                "board text ink ignores network material mode"
            );
            solid_text_pixels += 1;
        }
    }
    assert!(solid_text_pixels > 0);
    board_frame.traces.color_mode = pomelo_core::display::ColorMode::Layer;
    let visible_text_display = Arc::clone(&board_frame.display);
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
    board_frame.display = Arc::clone(&visible_text_display);
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
    board_frame.display = Arc::clone(&visible_text_display);
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
    board_frame.display = Arc::new(pomelo_core::display::BoardDisplay {
        layer_order: vec![LayerId(1), LayerId(2), LayerId(3)],
        ..Default::default()
    });
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
    // A replacement pad source uploads its immutable edges before drilling resources.
    for _ in 0..4 {
        target.bind(&context);
        board.draw(&gpu, &board_frame).unwrap();
        if composite_drill_stats.snapshot().uploaded_instances == 1 {
            break;
        }
    }
    let drill_pixels = target.read(&context);
    let visible_drill_display = Arc::clone(&board_frame.display);
    assert_eq!(
        rgb(&drill_pixels, 93, 35),
        [117, 125, 130],
        "drill display must fill its center independently of the copper opening"
    );
    assert!(
        rgb(&drill_pixels, 93, 30)[1] > 120,
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
    board_frame.display = Arc::clone(&visible_drill_display);
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
        [117, 125, 130],
        "one visible pad layer must restore the via drill"
    );
    assert_eq!(
        composite_drill_stats.snapshot().uploaded_bytes,
        before_drill_bytes
    );
    assert_eq!(composite_drill_stats.snapshot().cache_builds, 1);
    // Per-layer categories use the production composite renderer and retained GPU data.
    use pomelo_core::display::LayerPrimitive;
    board_frame.display = Arc::clone(&visible_drill_display);
    let pad_upload = composite_pad_stats.snapshot().uploaded_bytes;
    let custom_upload = composite_custom_stats.snapshot().lifetime_uploaded_bytes;
    let trace_upload = trace_stats.snapshot().uploaded_bytes;
    let mut categories = (*visible_drill_display).clone();
    categories.set_primitive(LayerId(2), LayerPrimitive::Pads, false);
    board_frame.display = Arc::new(categories.clone());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let without_disk = target.read(&context);
    assert_eq!(
        rgb(&without_disk, 35, 93),
        [0; 3],
        "pin pad filter removes its analytic disk"
    );
    assert_eq!(
        rgb(&without_disk, 73, 64),
        [0, 0, 255],
        "another layer's custom pad remains"
    );
    assert_eq!(
        rgb(&without_disk, 93, 35),
        [117, 125, 130],
        "pad filter does not hide a via drill"
    );
    assert_eq!(
        copper_stats.snapshot().visible_zones,
        2,
        "pad filter must preserve zones on the same layer"
    );
    categories.set_primitive(LayerId(3), LayerPrimitive::Pads, false);
    board_frame.display = Arc::new(categories);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 73, 64),
        rgb(&reordered, 73, 64),
        "custom pad filter restores the underlying composition"
    );
    let mut categories = (*visible_drill_display).clone();
    categories.set_primitive(LayerId(1), LayerPrimitive::Traces, false);
    board_frame.display = Arc::new(categories);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let no_trace = rgb(&target.read(&context), 64, 64);
    assert!(
        no_trace[0] == 0 && no_trace[1] == 0 && (126..=129).contains(&no_trace[2]),
        "trace filter leaves upper copper intact: {no_trace:?}"
    );
    board_frame.display = Arc::clone(&visible_drill_display);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), drill_pixels);
    assert_eq!(composite_pad_stats.snapshot().uploaded_bytes, pad_upload);
    assert_eq!(
        composite_custom_stats.snapshot().lifetime_uploaded_bytes,
        custom_upload
    );
    assert_eq!(trace_stats.snapshot().uploaded_bytes, trace_upload);

    // Identical geometry with via ownership proves pads/vias are separate categories.
    board_frame.pads = Some(via_pads);
    for _ in 0..3 {
        target.bind(&context);
        board.draw(&gpu, &board_frame).unwrap();
    }
    let all_vias = target.read(&context);
    assert_eq!(all_vias, drill_pixels);
    let via_upload = composite_pad_stats.snapshot().uploaded_bytes;
    let custom_via_upload = composite_custom_stats.snapshot().lifetime_uploaded_bytes;
    let mut categories = (*visible_drill_display).clone();
    for layer in [LayerId(2), LayerId(3)] {
        categories.set_primitive(layer, LayerPrimitive::Pads, false);
    }
    board_frame.display = Arc::new(categories.clone());
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        target.read(&context),
        all_vias,
        "pad switches cannot remove via copper"
    );
    for layer in [LayerId(2), LayerId(3)] {
        categories.set_primitive(layer, LayerPrimitive::Vias, false);
    }
    board_frame.display = Arc::new(categories);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let hidden_vias = target.read(&context);
    assert_eq!(rgb(&hidden_vias, 35, 93), [0; 3]);
    assert_eq!(
        rgb(&hidden_vias, 93, 35),
        [0; 3],
        "via drill disappears when all occupied layers disable vias"
    );
    assert_eq!(rgb(&hidden_vias, 73, 64), rgb(&reordered, 73, 64));
    board_frame.display = Arc::clone(&visible_drill_display);
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(target.read(&context), all_vias);
    assert_eq!(composite_pad_stats.snapshot().uploaded_bytes, via_upload);
    assert_eq!(
        composite_custom_stats.snapshot().lifetime_uploaded_bytes,
        custom_via_upload
    );
    assert_eq!(
        composite_drill_stats.snapshot().uploaded_bytes,
        before_drill_bytes
    );
    println!("LAYER_PRIMITIVE_FILTERS_PIXELS_VERIFIED uploads_unchanged=true");

    // The same net must tint trace, zone, analytic/custom via copper consistently.
    use pomelo_core::display::ColorMode;
    board_frame.traces.color_mode = ColorMode::Net;
    Arc::make_mut(&mut board_frame.display).color_mode = ColorMode::Net;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let net_pixels = target.read(&context);
    for (x, y) in [(35, 93), (73, 64), (103, 35), (64, 64)] {
        assert_eq!(rgb(&net_pixels, x, y), [26, 58, 95]);
    }
    assert_eq!(
        rgb(&net_pixels, 93, 35),
        [117, 125, 130],
        "physical drills retain their material"
    );
    let zone = rgb(&net_pixels, 23, 105);
    for (actual, expected) in zone.into_iter().zip([13i16, 29, 48]) {
        assert!(
            (i16::from(actual) - expected).abs() <= 1,
            "zone opacity: {zone:?}"
        );
    }
    board_frame.traces.highlighted_net = Some((NetId(1), [0.0, 1.0, 1.0, 1.0]));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let selected_network = target.read(&context);
    assert_ne!(
        selected_network, net_pixels,
        "selection must add visible overlays"
    );
    assert!(
        selected_network
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3].iter().all(|channel| *channel > 220)),
        "selection contains white Web overlay pixels"
    );
    for (x, y) in [(35, 93), (103, 35)] {
        assert_eq!(
            rgb(&selected_network, x, y),
            rgb(&net_pixels, x, y),
            "analytic pad interiors preserve net color"
        );
    }
    board_frame.traces.highlighted_net = None;
    board_frame.traces.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Via(pin.id),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let hovered_via = target.read(&context);
    assert_ne!(hovered_via, net_pixels, "hover adds a visible mint overlay");
    assert_eq!(
        rgb(&hovered_via, 35, 93),
        rgb(&net_pixels, 35, 93),
        "hover outline preserves the analytic pad interior"
    );
    assert_eq!(
        rgb(&hovered_via, 103, 35),
        rgb(&net_pixels, 103, 35),
        "hover identity does not affect another via"
    );
    board_frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Via(pin.id),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    let selected_via = target.read(&context);
    assert_ne!(
        selected_via, hovered_via,
        "selection and hover have distinct overlays"
    );
    assert_eq!(rgb(&selected_via, 35, 93), rgb(&net_pixels, 35, 93));
    assert_eq!(rgb(&selected_via, 103, 35), rgb(&net_pixels, 103, 35));
    board_frame.traces.highlighted_object = None;
    board_frame.traces.hovered_object = Some((
        pomelo_core::selection::SelectedObject::Segment(ObjectId(1)),
        [1.0, 0.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &board_frame.traces).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 64), [255, 0, 255]);
    board_frame.traces.highlighted_trace = Some((
        crate::backend::d3d11::TraceSelection::Segment(ObjectId(1)),
        [0.0, 1.0, 1.0, 1.0],
    ));
    target.bind(&context);
    traces.draw(&gpu, &board_frame.traces).unwrap();
    assert_eq!(rgb(&target.read(&context), 64, 64), [0, 255, 255]);
    board_frame.traces.highlighted_trace = None;
    board_frame.traces.hovered_object = None;
    board_frame.traces.color_mode = ColorMode::Layer;
    Arc::make_mut(&mut board_frame.display).color_mode = ColorMode::Layer;
    target.bind(&context);
    board.draw(&gpu, &board_frame).unwrap();
    assert_eq!(
        target.read(&context),
        all_vias,
        "returning to layer colors restores all pixels"
    );
    assert_eq!(trace_stats.snapshot().uploaded_bytes, trace_upload);
    assert_eq!(
        copper_stats.snapshot().lifetime_uploaded_bytes,
        uploaded_before
    );
    assert_eq!(composite_pad_stats.snapshot().uploaded_bytes, via_upload);
    assert_eq!(
        composite_custom_stats.snapshot().lifetime_uploaded_bytes,
        custom_via_upload
    );
    assert_eq!(
        composite_drill_stats.snapshot().uploaded_bytes,
        before_drill_bytes
    );
    println!("NETWORK_COMPOSITE_PIXELS_VERIFIED highlights_preserved=true uploads_unchanged=true");

    // Exercise every Web palette index and a high-bit NetId in both instanced shaders.
    // Grid samples lie well inside each trace/disk, independent of antialiasing.
    let network_ids: Vec<_> = (0..=203).chain([u32::MAX]).collect();
    let mut palette_segments = Vec::new();
    let mut palette_pins = Vec::new();
    for (index, net) in network_ids.iter().enumerate() {
        let at = Point::new((index % 16) as f64 + 0.5, (index / 16) as f64 + 0.5);
        palette_segments.push(Segment {
            id: ObjectId(index as u32 + 1),
            track_id: ObjectId(index as u32 + 1),
            layer: LayerId(1),
            net: NetId(*net),
            a: Point::new(at.x - 0.35, at.y),
            b: Point::new(at.x + 0.35, at.y),
            width: 0.4,
            arc: None,
            bond_wire: None,
        });
        let mut value = pin.clone();
        value.id = ObjectId(index as u32 + 1);
        value.net = NetId(*net);
        value.at = at;
        value.pads = vec![Pad::circle(LayerId(1), 0.6)];
        palette_pins.push(value);
    }
    let mut palette_frame = TraceFrame {
        pass: crate::backend::d3d11::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: ColorMode::Net,
        tracks: Arc::new(
            PreparedTracks::build(&palette_segments, TraceLimits::default(), &cancellation)
                .unwrap(),
        ),
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(16.0, 16.0),
        },
        camera: Some(pomelo_core::interaction::Camera {
            center: Point::new(8.0, 8.0),
            pixels_per_mm: 8.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::from([(LayerId(1), [1.0, 0.0, 0.0, 1.0])])),
        fallback_color: [1.0, 0.0, 0.0, 1.0],
        material_override: None,
        opacity: 1.0,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let palette_pad_source = Arc::new(
        crate::pads::PreparedPads::build(
            &palette_pins,
            &[],
            crate::pads::PadLimits::default(),
            &cancellation,
        )
        .unwrap(),
    );
    let palette_trace_stats = Arc::new(TraceTelemetry::default());
    let palette_pad_stats = Arc::new(TraceTelemetry::default());
    let mut palette_traces = TraceRenderer::<PreparedTracks>::new(Arc::clone(&palette_trace_stats));
    let mut palette_pads = crate::backend::d3d11::PadRenderer::new(Arc::clone(&palette_pad_stats));
    palette_pads.prepare(&gpu, &palette_pad_source).unwrap();
    for analytic_pads in [false, true] {
        target.bind(&context);
        if analytic_pads {
            palette_pads
                .draw_prepared(&gpu, &palette_frame, None)
                .unwrap();
        } else {
            palette_traces.draw(&gpu, &palette_frame).unwrap();
        }
        let pixels = target.read(&context);
        for (index, net) in network_ids.iter().enumerate() {
            let expected =
                crate::scene::colors::net_color(NetId(*net)).unwrap_or([1.0, 0.0, 0.0, 1.0]);
            let expected = [expected[0], expected[1], expected[2]]
                .map(|channel| (channel * 255.0).round() as u8);
            assert_eq!(
                rgb(&pixels, index % 16 * 8 + 4, 123 - index / 16 * 8),
                expected,
                "net {net}, pads={analytic_pads}"
            );
        }
    }
    let trace_upload = palette_trace_stats.snapshot().uploaded_bytes;
    let pad_upload = palette_pad_stats.snapshot().uploaded_bytes;
    palette_frame.color_mode = ColorMode::Layer;
    for analytic_pads in [false, true] {
        target.bind(&context);
        if analytic_pads {
            palette_pads
                .draw_prepared(&gpu, &palette_frame, None)
                .unwrap();
        } else {
            palette_traces.draw(&gpu, &palette_frame).unwrap();
        }
        let pixels = target.read(&context);
        for index in 0..network_ids.len() {
            assert_eq!(
                rgb(&pixels, index % 16 * 8 + 4, 123 - index / 16 * 8),
                [255, 0, 0]
            );
        }
    }
    assert_eq!(palette_trace_stats.snapshot().uploaded_bytes, trace_upload);
    assert_eq!(palette_pad_stats.snapshot().uploaded_bytes, pad_upload);
    println!(
        "NETWORK_PALETTE_PIXELS_VERIFIED samples={} trace_and_pad=true net_zero_fallback=true",
        network_ids.len()
    );
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
        pass: crate::backend::d3d11::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        tracks: Arc::clone(&tracks),
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(20.0, 20.0),
        },
        camera: None,
        scale_factor: 1.0,
        colors,
        fallback_color: [1.0; 4],
        material_override: None,
        opacity: 1.0,
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
    let dpi_pixels = target.read(&context);
    // Web AA and minimum stroke widths are measured in CSS/logical pixels.
    // Doubling DPI keeps the camera's physical centerline but widens its AA band.
    for (x, y) in [(32, 93), (64, 93), (96, 93), (64, 64), (12, 12)] {
        assert_eq!(
            rgb(&pixels, x, y),
            rgb(&dpi_pixels, x, y),
            "DPI preserves solid interiors and empty holes"
        );
    }
    assert_ne!(
        pixels, dpi_pixels,
        "logical-pixel AA changes the physical edge coverage"
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
        pass: crate::backend::d3d11::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
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
        material_override: None,
        opacity: 1.0,
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
        dpi_pixels
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
    // The legacy renderer fallback uses pixel padding; the desktop uses Web's
    // 86% occupancy. Compare against an independently specified camera instead.
    frame.camera = Some(pomelo_core::interaction::Camera {
        center: Point::new(10.0, 10.0),
        pixels_per_mm: 5.504,
        flipped: false,
    });
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let navigation_expected = target.read(&context);
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(target.read(&context) == navigation_expected);
    let navigation_baseline = telemetry.snapshot();
    assert!(navigation.zoom_at(Point::new(64.0, 92.0), 2.0));
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(rgb(&target.read(&context), 64, 92)[0] > 240);
    assert!(navigation.pan(Point::new(0.0, -20.0)));
    frame.camera = Some(navigation.camera());
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let moved = target.read(&context);
    assert!(rgb(&moved, 64, 72)[0] > 240);
    assert_eq!(rgb(&moved, 64, 92), [0; 3]);
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
    let outline_pixel = rgb(&outline_pixels, 14, 64);
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
        pass: crate::backend::d3d11::OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        tracks: Arc::clone(&texts),
        bounds: board.scene.bounds,
        camera: None,
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::new()),
        fallback_color: [1.0, 0.0, 0.0, 1.0],
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
