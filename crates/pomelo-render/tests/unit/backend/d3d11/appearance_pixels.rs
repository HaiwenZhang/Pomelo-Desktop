//! Exercise the production compositor with separate Etch/Pin/Via/drill materials.
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    appearance::{ColorTarget, RgbColor},
    display::BoardDisplay,
    interaction::Camera,
    model::{
        BoardText, CustomPadGeometry, DrillShape, Pad, PadKind, Pin, TextAlignment, Via, Zone,
    },
};

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_category_colors_cover_analytic_custom_copper_text_and_drills_without_reupload() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; a hardware adapter is required.
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
    let cancel = CancellationToken::default();
    let square = |r: f64| {
        vec![
            Point::new(-r, -r),
            Point::new(r, -r),
            Point::new(r, r),
            Point::new(-r, r),
        ]
    };
    let layer = LayerId(0);
    let texts = [BoardText {
        id: ObjectId(70),
        owner_id: None,
        layer,
        class_id: 6,
        subclass: 0,
        text: "GND".into(),
        at: Point::new(-5.0, 4.0),
        angle: 0.0,
        mirrored: false,
        align: TextAlignment::Left,
        font_index: 0,
        width: 0.75,
        height: 1.0,
        spacing: 0.0,
        line_spacing: 1.0,
        stroke_width: 0.1,
    }];
    let font = crate::text::msdf::MsdfFont::bundled(texts.iter().map(|t| t.text.as_str()), &cancel)
        .unwrap();
    let glyphs = Arc::new(
        crate::text::msdf::PreparedGlyphs::build(&texts, Arc::new(font), 1024 * 1024, &cancel)
            .unwrap(),
    );
    let mut automatic_text = texts[0].clone();
    automatic_text.id = ObjectId(50);
    automatic_text.at = Point::new(1.0, 4.0);
    let mut automatic = crate::text::msdf::PreparedGlyphs::build(
        &[automatic_text],
        Arc::clone(&glyphs.font),
        1024 * 1024,
        &cancel,
    )
    .unwrap();
    for glyph in &mut automatic.instances {
        glyph.ids[1] = pomelo_core::display::DisplayCategory::Pin as u32;
        glyph.low[2] = 0.0;
        glyph.color = [0.7, 0.7, 0.7, 1.0];
    }
    let automatic = Arc::new(automatic);
    let mut checked = 0;
    for custom in [false, true] {
        let mut pad = Pad::circle(layer, 3.0);
        if custom {
            pad.kind = PadKind::CUSTOM;
            pad.custom = Some(Arc::new(CustomPadGeometry {
                contours: vec![square(1.5), square(0.4)],
                paths: vec![],
            }));
        }
        let drill_shape = DrillShape {
            width: 0.8,
            height: 0.8,
            plated: true,
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
            drill: 0.8,
            drill_shape,
            pads: vec![pad.clone()],
            stackup_region: None,
            die: None,
        };
        let via = Via {
            id: ObjectId(51),
            net: NetId(1),
            at: Point::new(3.0, 0.0),
            drill: 0.8,
            drill_shape,
            padstack: ObjectId(52),
            padstack_name: String::new(),
            start_layer: Some(layer),
            end_layer: Some(layer),
            pads: Arc::from([pad]),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        };
        let pins = [pin];
        let vias = [via];
        let mut pads = crate::pads::PreparedPads::build(
            &pins,
            &vias,
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap();
        if custom {
            pads.custom_mesh = Some(Arc::new(
                pads.build_custom_meshes(
                    crate::copper::CopperLimits::default(),
                    &pomelo_core::copper::MeshLimits::default(),
                    &cancel,
                )
                .unwrap(),
            ));
        }
        let drills = crate::drills::PreparedDrills::build(
            &pins,
            &vias,
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap();
        let tracks = Arc::new(
            PreparedTracks::build(
                &[Segment {
                    id: ObjectId(10),
                    track_id: ObjectId(10),
                    layer,
                    net: NetId(1),
                    a: Point::new(-5.0, 2.0),
                    b: Point::new(5.0, 2.0),
                    width: 0.8,
                    arc: None,
                    bond_wire: None,
                }],
                TraceLimits::default(),
                &cancel,
            )
            .unwrap(),
        );
        let zone = Zone {
            kind: pomelo_core::model::ZoneKind::Unknown,
            id: ObjectId(60),
            layer,
            net: NetId(1),
            paths: vec![],
            mesh: pomelo_core::copper::CopperMesh::build(
                &[vec![
                    Point::new(-5.0, -5.0),
                    Point::new(5.0, -5.0),
                    Point::new(5.0, -2.0),
                    Point::new(-5.0, -2.0),
                ]],
                &[],
                &pomelo_core::copper::MeshLimits::default(),
                &cancel,
            )
            .unwrap(),
        };
        let mut frame = BoardFrame {
            traces: TraceFrame {
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
                colors: Arc::new(BTreeMap::from([(layer, [1.0, 1.0, 0.0, 1.0])])),
                fallback_color: [1.0; 4],
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
            },
            display: Arc::new(BoardDisplay {
                show_texts: true,
                ..Default::default()
            }),
            pads: Some(Arc::new(pads)),
            drills: Some(Arc::new(drills.geometry)),
            copper: Arc::new(
                crate::copper::PreparedCopper::build(
                    &[zone],
                    crate::copper::CopperLimits::default(),
                    &cancel,
                )
                .unwrap(),
            ),
            copper_opacity: 0.4,
            layer_order: Arc::new(vec![layer]),
            glyphs: Some(Arc::clone(&glyphs)),
            labels: Some(Arc::clone(&automatic)),
            curves: None,
            zone_outlines: None,
            drawings: None,
            texts: None,
            drill_color: [0.2, 0.2, 0.2, 1.0],
        };
        let trace_stats = Arc::new(TraceTelemetry::default());
        let copper_stats = Arc::new(CopperTelemetry::default());
        let pad_stats = Arc::new(TraceTelemetry::default());
        let custom_stats = Arc::new(CopperTelemetry::default());
        let drill_stats = Arc::new(TraceTelemetry::default());
        let text_stats = Arc::new(TraceTelemetry::default());
        let label_stats = Arc::new(TraceTelemetry::default());
        let mut renderer = BoardRenderer::new(
            Arc::clone(&trace_stats),
            Arc::clone(&copper_stats),
            Arc::clone(&pad_stats),
            Arc::clone(&custom_stats),
            Arc::clone(&drill_stats),
            Arc::new(TraceTelemetry::default()),
            Arc::clone(&text_stats),
        )
        .with_label_telemetry(Arc::clone(&label_stats));
        for _ in 0..16 {
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
        }
        let baseline = target.read(&context);
        assert_eq!(
            rgb(&baseline, 34, 64),
            [51; 3],
            "compositor must honor BoardFrame drill color"
        );
        let uploads = [
            trace_stats.snapshot().uploaded_bytes,
            copper_stats.snapshot().uploaded_bytes,
            pad_stats.snapshot().uploaded_bytes,
            custom_stats.snapshot().uploaded_bytes,
            drill_stats.snapshot().uploaded_bytes,
            text_stats.snapshot().uploaded_bytes,
            label_stats.snapshot().uploaded_bytes,
        ];
        let appearance = &mut Arc::make_mut(&mut frame.display).appearance;
        appearance.set_color(ColorTarget::Etch(layer), Some(RgbColor([38, 255, 38])));
        appearance.set_color(ColorTarget::Pin(layer), Some(RgbColor([255, 0, 0])));
        appearance.set_color(ColorTarget::Via(layer), Some(RgbColor([0, 0, 255])));
        appearance.set_color(ColorTarget::Drill, Some(RgbColor([172; 3])));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let colored = target.read(&context);
        for (x, y, expected) in [
            (64, 44, [38, 255, 38]),
            (44, 64, [255, 0, 0]),
            (104, 64, [0, 0, 255]),
            (34, 64, [172; 3]),
            (94, 64, [172; 3]),
            (64, 104, [15, 102, 15]),
        ] {
            let actual = rgb(&colored, x, y);
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (i32::from(*a) - b).abs() <= 1),
                "custom={custom} at ({x},{y}): {actual:?} expected {expected:?}"
            );
            checked += 1;
        }
        let mut text_pixels = 0;
        for y in 4..35 {
            for x in 10..50 {
                let before = rgb(&baseline, x, y);
                let after = rgb(&colored, x, y);
                if before[0] > 150 && before[1] > 150 {
                    assert!(
                        after[1] > 150 && after[0] < 60 && after[2] < 60,
                        "source copper text must inherit Etch RGB: {after:?}"
                    );
                    text_pixels += 1;
                }
            }
        }
        Arc::make_mut(&mut frame.display).global_opacity = 128.0 / 255.0;
        frame.copper_opacity = 99.0 / 255.0;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let translucent = target.read(&context);
        for (x, y, expected) in [
            (64, 44, [19, 128, 19]),
            (44, 64, [128, 0, 0]),
            (104, 64, [0, 0, 128]),
            // Filled analytic pads lie under the translucent drill. Custom pads
            // have an actual central void; verify ordinary source-over, not a
            // post-composition fade or an opaque background-colored fake hole.
            (34, 64, if custom { [86; 3] } else { [150, 86, 86] }),
            (94, 64, if custom { [86; 3] } else { [86, 86, 150] }),
            (64, 104, [15, 99, 15]),
        ] {
            let actual = rgb(&translucent, x, y);
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (i32::from(*a) - b).abs() <= 1),
                "global opacity custom={custom} ({x},{y}): {actual:?}, expected {expected:?}"
            );
        }
        for y in 4..35 {
            for x in 10..50 {
                let full = rgb(&colored, x, y);
                if full[1] > 150 {
                    let half = rgb(&translucent, x, y);
                    assert!(
                        (f32::from(half[1]) - f32::from(full[1]) * 128.0 / 255.0).abs() <= 1.0,
                        "source text global alpha must apply once: full={full:?}, half={half:?}"
                    );
                }
            }
        }
        let mut label_pixels = 0;
        let mut label_peak = 0;
        for y in 4..35 {
            for x in 70..110 {
                let full = rgb(&colored, x, y);
                label_peak = label_peak.max(full[0]);
                if full[0] > 150 {
                    let half = rgb(&translucent, x, y);
                    assert!(
                        full.iter().zip(half).all(|(a, b)| (f32::from(b)
                            - f32::from(*a) * 128.0 / 255.0)
                            .abs()
                            <= 1.0),
                        "automatic label alpha must apply once: full={full:?}, half={half:?}"
                    );
                    label_pixels += 1;
                }
            }
        }
        assert!(
            label_pixels > 10,
            "automatic label must contain visible ink: peak={label_peak}, uploaded={}, drawn={}",
            label_stats.snapshot().uploaded_bytes,
            label_stats.snapshot().draw_calls
        );
        assert!(text_pixels > 10, "source text fixture must have ink pixels");
        Arc::make_mut(&mut frame.display).global_opacity = 0.0;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let transparent = target.read(&context);
        for (x, y) in [(64, 44), (44, 64), (104, 64), (34, 64), (94, 64)] {
            assert_eq!(rgb(&transparent, x, y), [0; 3]);
        }
        for y in 4..35 {
            for x in (10..50).chain(70..110) {
                assert_eq!(rgb(&transparent, x, y), [0; 3]);
            }
        }
        assert_eq!(rgb(&transparent, 64, 104), [15, 99, 15]);
        frame.traces.highlighted_object = Some((
            pomelo_core::selection::SelectedObject::Segment(ObjectId(10)),
            [1.0; 4],
        ));
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let selected = target.read(&context);
        assert_eq!(rgb(&selected, 64, 40), [229; 3], "selection stays visible");
        frame.traces.highlighted_object = None;
        Arc::make_mut(&mut frame.display).global_opacity = 1.0;
        frame.copper_opacity = 0.4;
        frame.traces.color_mode = pomelo_core::display::ColorMode::Net;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let net_pixels = target.read(&context);
        let net = crate::scene::colors::net_color(NetId(1))
            .unwrap()
            .map(|c| (c * 255.0).round() as u8);
        for (x, y) in [(64, 44), (44, 64), (104, 64)] {
            assert_eq!(rgb(&net_pixels, x, y), [net[0], net[1], net[2]]);
        }
        assert_eq!(
            rgb(&net_pixels, 34, 64),
            [172; 3],
            "drills ignore net color mode"
        );
        Arc::make_mut(&mut frame.display).global_opacity = 128.0 / 255.0;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let net_translucent = target.read(&context);
        for (x, y) in [(64, 44), (44, 64), (104, 64)] {
            let actual = rgb(&net_translucent, x, y);
            assert!(
                actual
                    .iter()
                    .zip(net)
                    .all(|(a, b)| (f32::from(*a) - f32::from(b) * 128.0 / 255.0).abs() <= 1.0)
            );
        }
        Arc::make_mut(&mut frame.display).global_opacity = 1.0;
        frame.traces.color_mode = pomelo_core::display::ColorMode::Layer;
        Arc::make_mut(&mut frame.display).appearance = Default::default();
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        assert_eq!(
            target.read(&context),
            baseline,
            "restoring colors must restore all production materials"
        );
        assert_eq!(
            [
                trace_stats.snapshot().uploaded_bytes,
                copper_stats.snapshot().uploaded_bytes,
                pad_stats.snapshot().uploaded_bytes,
                custom_stats.snapshot().uploaded_bytes,
                drill_stats.snapshot().uploaded_bytes,
                text_stats.snapshot().uploaded_bytes,
                label_stats.snapshot().uploaded_bytes
            ],
            uploads
        );
    }
    eprintln!(
        "CATEGORY_COLOR_GPU checked={checked} analytic_custom=true source_text=true automatic_labels=true global_alpha_once=true independent_shape_alpha=true zero_alpha=true selection_visible=true net_mode=true restore=true geometry_reupload=false"
    );
}
