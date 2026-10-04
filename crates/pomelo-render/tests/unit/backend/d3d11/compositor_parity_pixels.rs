//! Diagnostic proofs of the current compositor policy, not Allegro acceptance.
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry, OverlayPass};
use pomelo_core::{
    appearance::{ColorTarget, RgbColor},
    display::{BoardDisplay, DisplayCategory, LayerPriority},
    interaction::Camera,
    model::{DrillShape, Pad, Pin, Via, Zone},
    selection::SelectedObject,
};

pub(super) fn device() -> (ID3D11Device, ID3D11DeviceContext) {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; the test requests a hardware adapter only.
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
    (device.unwrap(), context.unwrap())
}

pub(super) fn renderer() -> BoardRenderer {
    BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    )
}

pub(super) fn fixture() -> BoardFrame {
    let cancel = CancellationToken::default();
    let top = LayerId(0);
    let bottom = LayerId(2);
    let tracks = [bottom, top].map(|layer| Segment {
        id: ObjectId(10 + layer.0),
        track_id: ObjectId(10 + layer.0),
        layer,
        net: NetId(1),
        a: Point::new(-5.0, 0.0),
        b: Point::new(5.0, 0.0),
        width: 1.0,
        arc: None,
        bond_wire: None,
    });
    let pin = Pin {
        id: ObjectId(50),
        owner_id: ObjectId(49),
        net: NetId(1),
        name: String::new(),
        reference: String::new(),
        at: Point::new(-2.0, 0.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: false,
        },
        pads: vec![Pad::circle(bottom, 2.0)],
        stackup_region: None,
        die: None,
    };
    let via = Via {
        id: ObjectId(51),
        net: NetId(1),
        at: pin.at,
        drill: 0.4,
        drill_shape: DrillShape {
            width: 0.4,
            height: 0.4,
            plated: true,
        },
        padstack: ObjectId(52),
        padstack_name: String::new(),
        start_layer: Some(top),
        end_layer: Some(top),
        pads: Arc::from([Pad::circle(top, 1.0)]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    };
    let pins = [pin];
    let vias = [via];
    let rect = |x: f64, y: f64, w: f64, h: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + w, y),
            Point::new(x + w, y + h),
            Point::new(x, y + h),
        ]
    };
    let zones = [bottom, top].map(|layer| {
        let mut contours = vec![rect(-5.0, -5.0, 10.0, 3.0), rect(-2.7, -4.2, 1.4, 1.4)];
        if layer == top {
            contours.push(rect(1.3, -4.2, 1.4, 1.4));
        }
        Zone {
            kind: pomelo_core::model::ZoneKind::Unknown,
            id: ObjectId(60 + layer.0),
            layer,
            net: NetId(1),
            paths: vec![],
            mesh: pomelo_core::copper::CopperMesh::build(
                &contours,
                &[],
                &pomelo_core::copper::MeshLimits::default(),
                &cancel,
            )
            .unwrap(),
        }
    });
    let mut display = BoardDisplay::default();
    display
        .appearance
        .set_color(ColorTarget::Pin(bottom), Some(RgbColor([0, 255, 0])));
    display
        .appearance
        .set_color(ColorTarget::Via(top), Some(RgbColor([255, 255, 0])));
    BoardFrame {
        traces: TraceFrame {
            tracks: Arc::new(
                PreparedTracks::build(&tracks, TraceLimits::default(), &cancel).unwrap(),
            ),
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
                (top, [1.0, 0.0, 0.0, 1.0]),
                (bottom, [0.0, 0.0, 1.0, 1.0]),
            ])),
            fallback_color: [1.0; 4],
            material_override: None,
            opacity: 1.0,
            color_mode: pomelo_core::display::ColorMode::Layer,
            pass: OverlayPass::Base,
            filled: true,
            hover_selection: None,
            highlighted_objects: None,
            highlighted_net: None,
            highlighted_trace: None,
            highlighted_object: None,
            hovered_object: None,
            highlighted_related_objects: None,
        },
        display: Arc::new(display),
        pads: Some(Arc::new(
            crate::pads::PreparedPads::build(
                &pins,
                &vias,
                crate::pads::PadLimits::default(),
                &cancel,
            )
            .unwrap(),
        )),
        drills: Some(Arc::new(
            crate::drills::PreparedDrills::build(
                &pins,
                &vias,
                crate::pads::PadLimits::default(),
                &cancel,
            )
            .unwrap()
            .geometry,
        )),
        copper: Arc::new(
            crate::copper::PreparedCopper::build(
                &zones,
                crate::copper::CopperLimits::default(),
                &cancel,
            )
            .unwrap(),
        ),
        copper_opacity: 99.0 / 255.0,
        layer_order: Arc::new(vec![bottom, top]),
        glyphs: None,
        labels: None,
        curves: None,
        zone_outlines: None,
        drawings: None,
        texts: None,
        drill_color: [0.2, 0.2, 0.2, 1.0],
    }
}

pub(super) fn gpu_context<'a>(
    device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
) -> NativeGpuContext<'a> {
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    NativeGpuContext {
        device: crate::backend::d3d11::Device::new(device, context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    }
}

fn over(destination: [u8; 3], source: [u8; 3], opacity: f64) -> [u8; 3] {
    std::array::from_fn(|index| {
        (f64::from(source[index]) * opacity + f64::from(destination[index]) * (1.0 - opacity))
            .round() as u8
    })
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; tests viewer policy, not Allegro acceptance"]
fn hardware_compositor_translucent_overlap_applies_global_per_object() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer();
    let mut frame = fixture();
    let alpha = 128.0 / 255.0;
    Arc::make_mut(&mut frame.display).global_opacity = alpha as f32;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    for filled in [true, false] {
        Arc::make_mut(&mut frame.display).filled = filled;
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let pixels = target.read(&context);
        let mut expected = over(over([0; 3], [0, 0, 255], alpha), [255, 0, 0], alpha);
        if filled {
            expected = over(over(expected, [0, 255, 0], alpha), [255, 255, 0], alpha);
        }
        expected = over(expected, [51; 3], alpha);
        let actual = rgb(&pixels, 44, 64);
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (i32::from(*actual) - i32::from(expected)).abs() <= 1),
            "filled={filled}: per-object composition {actual:?}, expected {expected:?}"
        );
        assert_eq!(
            rgb(&pixels, 64, 99),
            [99, 0, 61],
            "global alpha must not scale either independent shape fill"
        );
        eprintln!(
            "COMPOSITOR_ALPHA filled={filled} alpha=128/255 center={actual:?} independent_oracle={expected:?} shape_alpha=99/255 shape_overlap=[99,0,61] allegro_validated=false"
        );
    }
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; tests viewer policy, not Allegro acceptance"]
fn hardware_compositor_zero_global_keeps_selection_and_hover_independent() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer();
    let mut frame = fixture();
    Arc::make_mut(&mut frame.display).global_opacity = 0.0;
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let hidden = target.read(&context);
    assert_eq!(
        rgb(&hidden, 64, 59),
        [0; 3],
        "ordinary trace is transparent"
    );
    for hovered in [false, true] {
        if hovered {
            frame.traces.hovered_object = Some((SelectedObject::Segment(ObjectId(10)), [1.0; 4]));
        } else {
            frame.traces.highlighted_object =
                Some((SelectedObject::Segment(ObjectId(10)), [1.0; 4]));
        }
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        let shown = target.read(&context);
        let expected = if hovered { [145, 229, 195] } else { [229; 3] };
        assert_eq!(
            rgb(&shown, 64, 59),
            expected,
            "hovered={hovered}: overlay remains visible at global=0"
        );
        frame.traces.highlighted_object = None;
        frame.traces.hovered_object = None;
    }
    eprintln!(
        "COMPOSITOR_POLICY global_zero=true base_trace_hidden=true selection_visible=true hover_visible=true web_overlay_zero_skipped=true allegro_validated=false"
    );
}

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; tests viewer policy, not Allegro acceptance"]
fn hardware_compositor_cross_layer_class_priority_and_zone_holes() {
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer();
    let mut frame = fixture();
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let baseline = target.read(&context);
    for (x, y, expected) in [
        (64, 64, [255, 0, 0]),
        (44, 57, [0, 255, 0]),
        (46, 61, [255, 255, 0]),
        (44, 64, [51; 3]),
        (64, 99, [99, 0, 61]),
        (44, 99, [0; 3]),
        (84, 99, [0, 0, 99]),
    ] {
        let actual = rgb(&baseline, x, y);
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (i32::from(*a) - b).abs() <= 1),
            "default stack ({x},{y}): {actual:?}, expected {expected:?}"
        );
    }
    Arc::make_mut(&mut frame.display).active_layer = Some(LayerId(0));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 44, 64),
        [255, 0, 0],
        "active TOP etch currently outranks the drill"
    );
    let display = Arc::make_mut(&mut frame.display);
    display.active_layer = None;
    display.priorities = vec![LayerPriority {
        layer: LayerId(0),
        category: DisplayCategory::Trace,
    }];
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 44, 64),
        [255, 0, 0],
        "promoted TOP etch currently outranks the drill"
    );
    let display = Arc::make_mut(&mut frame.display);
    display.priorities.clear();
    display.hidden_layers.insert(LayerId(0));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 44, 64),
        [0, 255, 0],
        "hiding the only via pad layer hides its drill and exposes the bottom pin"
    );
    eprintln!(
        "COMPOSITOR_POLICY default_class_order=true top_same_class=true active_etch_above_drill=true promoted_etch_above_drill=true two_zone_source_over=true zone_holes_expose_lower_fill=true via_drill_scope=true allegro_validated=false"
    );
}
