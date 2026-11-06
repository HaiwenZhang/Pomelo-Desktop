//! Execute the complete solid pipeline using WARP and a hardware adapter.
use super::*;
use crate::backend::solid_pipeline::{SolidPipeline, tests::verify};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_WARP};
fn run(kind: D3D_DRIVER_TYPE) {
    let (mut raw, mut context) = (None, None);
    // SAFETY: initialized outputs; the offscreen test needs no window or swapchain.
    unsafe {
        D3D11CreateDevice(
            None,
            kind,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut raw),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let raw = raw.unwrap();
    let context = context.unwrap();
    let target = Target::new(&raw);
    verify(|pipeline, frame, clipped, opacity| {
        target.bind(&context);
        // SAFETY: test-owned target belongs to this immediate context.
        unsafe {
            context.ClearRenderTargetView(&target.view, &[0.0, 1.0, 0.0, 1.0]);
        }
        let bounds = ViewBounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(128.0), ScaledPixels(128.0)),
        );
        let clip = if clipped {
            ViewBounds::new(bounds.origin, size(ScaledPixels(72.0), ScaledPixels(128.0)))
        } else {
            bounds
        };
        let gpu = NativeGpuContext {
            device: crate::backend::d3d11::Device::new(&raw, &context, opacity),
            viewport: [128.0; 2],
            bounds,
            content_mask: ContentMask { bounds: clip },
        };
        if pipeline.is_none() {
            *pipeline = Some(SolidPipeline::new(&gpu.device, &frame.scene).unwrap());
        }
        pipeline.as_mut().unwrap().draw(&gpu, frame).unwrap();
        target.read(&context)
    });
}
#[test]
fn solid_d3d11_warp_depth_oit_clipping_opacity_and_cache_pixels() {
    run(D3D_DRIVER_TYPE_WARP);
}
#[test]
#[ignore = "requires a Windows hardware D3D11 adapter"]
fn solid_d3d11_hardware_depth_oit_clipping_opacity_and_cache_pixels() {
    run(D3D_DRIVER_TYPE_HARDWARE);
}
