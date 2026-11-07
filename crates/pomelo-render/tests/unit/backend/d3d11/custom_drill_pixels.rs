//! WARP readback through the production compositor for an arbitrary polygon hole.
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    display::BoardDisplay,
    interaction::Camera,
    model::{CustomPadGeometry, DrillShape, Pad, Pin},
};

#[test]
fn warp_polygon_drill_preserves_copper_outside_the_polygon_and_reuses_its_mesh() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; the offscreen software device needs no window.
    unsafe {
        D3D11CreateDevice(
            None,
            windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP,
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
        size(ScaledPixels(128.0), ScaledPixels(128.0)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [128.0; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let cancel = CancellationToken::default();
    let pin = Pin {
        id: ObjectId(4),
        owner_id: ObjectId(0),
        net: NetId(0),
        name: String::new(),
        reference: String::new(),
        at: Point::default(),
        angle: 0.0,
        mirrored: false,
        drill: 4.0,
        drill_shape: DrillShape {
            width: 4.0,
            height: 4.0,
            rectangular: false,
            plated: false,
            custom: Some(Arc::new(CustomPadGeometry {
                contours: vec![vec![
                    Point::new(-2.0, -2.0),
                    Point::new(2.0, -2.0),
                    Point::new(-2.0, 2.0),
                ]],
                paths: vec![],
            })),
        },
        pads: vec![Pad::circle(LayerId(0), 6.0)],
        stackup_region: None,
        die: None,
    };
    let pins = [pin];
    let pads = crate::pads::PreparedPads::build(&pins, &[], Default::default(), &cancel).unwrap();
    let drills = crate::drills::PreparedDrills::build(&pins, &[], Default::default(), &cancel)
        .unwrap()
        .geometry;
    let tracks = Arc::new(PreparedTracks::build(&[], Default::default(), &cancel).unwrap());
    let traces = TraceFrame {
        tracks,
        bounds: Bounds {
            min: Point::new(-3.0, -3.0),
            max: Point::new(3.0, 3.0),
        },
        camera: Some(Camera {
            center: Point::default(),
            pixels_per_mm: 20.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])])),
        fallback_color: [1.0; 4],
        net_colors: None,
        pattern: None,
        object_filter: Default::default(),
        brightness: 1.0,
        object_colors: None,
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
    let display = Arc::new(BoardDisplay::default());
    let mut frame = BoardFrame {
        placement_outlines: None,
        placements: None,
        curves: None,
        glyphs: None,
        labels: None,
        zone_outlines: None,
        drawings: None,
        texts: None,
        traces,
        display,
        pads: Some(Arc::new(pads)),
        drills: Some(Arc::new(drills)),
        drill_color: [0.0, 0.0, 1.0, 1.0],
        copper_opacity: 1.0,
        layer_order: Arc::new(vec![LayerId(0)]),
        copper: Arc::new(
            crate::copper::PreparedCopper::build(&[], Default::default(), &cancel).unwrap(),
        ),
    };
    let mut renderer = BoardRenderer::new(
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(CopperTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
        Arc::new(TraceTelemetry::default()),
    );
    for _ in 0..12 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let baseline = target.read(&context);
    assert_eq!(
        rgb(&baseline, 44, 84),
        [0, 0, 255],
        "the triangle interior must be a hole"
    );
    assert_eq!(
        rgb(&baseline, 84, 44),
        [255, 0, 0],
        "copper inside the bounds but outside the triangle must remain"
    );
    assert!(renderer.geometry_is_uploaded(&frame));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(target.read(&context), baseline);
    Arc::make_mut(&mut frame.display).show_drills = false;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 44, 84), [255, 0, 0]);
    Arc::make_mut(&mut frame.display).show_drills = true;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(target.read(&context), baseline);
}
