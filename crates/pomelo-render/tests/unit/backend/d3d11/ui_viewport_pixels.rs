//! Regression for the native UI callback's element-local initial viewport.
use super::*;
use crate::backend::{OverlayPass, TraceFrame};
use pomelo_core::{interaction::Camera, selection::SelectedObject};

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_ui_callback_keeps_canvas_coordinates_and_review_colors() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized outputs; the test requires a real hardware adapter.
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
    target.bind(&context);
    let bounds = ViewBounds::new(
        point(ScaledPixels(32.0), ScaledPixels(16.0)),
        size(ScaledPixels(64.0), ScaledPixels(64.0)),
    );
    // GPUI 0.8 enters the callback with the element's viewport, not the full target.
    // SAFETY: the viewport fits this test's target texture.
    unsafe {
        context.RSSetViewports(Some(&[D3D11_VIEWPORT {
            TopLeftX: 32.0,
            TopLeftY: 16.0,
            Width: 64.0,
            Height: 64.0,
            MaxDepth: 1.0,
            ..Default::default()
        }]));
    }
    let paint = gpui::D3D11PaintContext {
        target: gpui::GpuPaintTarget {
            size: [SIDE, SIDE],
            bounds,
            clip: bounds,
            scale_factor: 1.0,
            sample_count: 1,
            opacity: 1.0,
        },
        device: &device,
        context: &context,
        color_target: &target.view,
        color_format: DXGI_FORMAT_R8G8B8A8_UNORM,
    };
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::from_paint(&paint),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let tracks = Arc::new(
        PreparedTracks::build(
            &[Segment {
                id: ObjectId(1),
                track_id: ObjectId(1),
                layer: LayerId(0),
                net: NetId(0),
                a: Point::new(-5.0, 0.0),
                b: Point::new(5.0, 0.0),
                width: 1.0,
                arc: None,
                bond_wire: None,
            }],
            TraceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    );
    let mut frame = TraceFrame {
        net_colors: None,
        pattern: None,
        object_filter: Default::default(),
        brightness: 1.0,
        object_colors: None,
        pass: OverlayPass::Base,
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        tracks,
        bounds: Bounds {
            min: Point::new(-8.0, -8.0),
            max: Point::new(8.0, 8.0),
        },
        camera: Some(Camera {
            center: Point::default(),
            pixels_per_mm: 4.0,
            flipped: false,
        }),
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
    let telemetry = Arc::new(TraceTelemetry::default());
    let mut renderer = TraceRenderer::new(Arc::clone(&telemetry));
    renderer.draw(&gpu, &frame).unwrap();
    let pixels = target.read(&context);
    assert_eq!(rgb(&pixels, 48, 48), [255, 0, 0]);
    assert_eq!(rgb(&pixels, 64, 40), [0; 3]);

    let uploads = telemetry.snapshot().uploaded_bytes;
    frame.brightness = 0.4;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(
        rgb(&target.read(&context), 48, 48),
        [102, 0, 0],
        "brightness modulates RGB independently of coverage"
    );
    assert_eq!(telemetry.snapshot().uploaded_bytes, uploads);
    frame.brightness = 1.0;

    frame.object_colors = Some(Arc::new(BTreeMap::from([(
        SelectedObject::Segment(ObjectId(1)),
        [0.0, 1.0, 0.0],
    )])));
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(rgb(&target.read(&context), 48, 48), [0, 255, 0]);
    assert_eq!(telemetry.snapshot().uploaded_bytes, uploads);
}
