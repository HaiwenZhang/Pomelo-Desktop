//! Synthetic readback coverage: no external board fixtures or screenshot assets.
use super::*;
use crate::backend::{NativeGpuContext, OverlayPass, TraceFrame};
use gpui::{Bounds, GpuPaintTarget, ScaledPixels, point, size};
const SIDE: u64 = 128;
fn target(device: &DeviceRef) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_width(SIDE);
    desc.set_height(SIDE);
    desc.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    desc.set_usage(MTLTextureUsage::RenderTarget);
    desc.set_storage_mode(MTLStorageMode::Shared);
    device.new_texture(&desc)
}
fn clear(command: &CommandBufferRef, target: &TextureRef) {
    let pass = RenderPassDescriptor::new();
    let color = pass.color_attachments().object_at(0).unwrap();
    color.set_texture(Some(target));
    color.set_load_action(MTLLoadAction::Clear);
    color.set_store_action(MTLStoreAction::Store);
    color.set_clear_color(MTLClearColor::new(0.0, 1.0, 0.0, 1.0));
    command.new_render_command_encoder(pass).end_encoding();
}
fn pixels(command: &CommandBufferRef, texture: &TextureRef) -> Vec<u8> {
    command.commit();
    command.wait_until_completed();
    assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    texture.get_bytes(
        pixels.as_mut_ptr().cast(),
        SIDE * 4,
        MTLRegion::new_2d(0, 0, SIDE, SIDE),
        0,
    );
    pixels
}
fn gpu_target(opacity: f32) -> GpuPaintTarget {
    let bounds = Bounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    GpuPaintTarget {
        size: [SIDE as u32; 2],
        bounds,
        clip: bounds,
        scale_factor: 1.0,
        sample_count: 1,
        opacity,
    }
}
fn sample(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * SIDE as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn metal_trace_preserves_clip_opacity_and_existing_color() {
    trace_draws(1, None);
}
#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn metal_many_batches_complete_in_one_render_pass() {
    trace_draws(4096, None);
}
#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn metal_dynamic_zone_border_remains_visible() {
    trace_draws(1, Some(1));
    trace_draws(1, Some(2));
}
fn trace_draws(draw_count: usize, zone_mode: Option<u32>) {
    let raw = metal::Device::system_default().expect("Metal device");
    let queue = raw.new_command_queue();
    let command = queue.new_command_buffer();
    let color = target(&raw);
    clear(command, &color);
    let mut paint = gpui_apple::MetalPaintContext {
        target: gpu_target(0.5),
        device: &raw,
        command_buffer: command,
        color_target: &color,
        color_format: MTLPixelFormat::RGBA8Unorm,
    };
    paint.target.clip.size.width = ScaledPixels(72.0);
    let device = Device::new(&paint);
    let pipeline = device.pipeline(Shader::Trace).unwrap();
    let trace = crate::tracks::TraceInstance {
        a: [-40.0, 0.0, 0.0, 0.0],
        b: [40.0, 0.0, 0.0, 0.0],
        center: [0.0; 4],
        arc: [0.0; 4],
        bounds_min: [-42.0, -2.0, 0.0, 0.0],
        bounds_max: [42.0, 2.0, 0.0, 0.0],
        ids: [0; 4],
        flags: [
            0,
            0,
            4.0f32.to_bits(),
            if zone_mode.is_some() { 8 | 128 } else { 0 },
        ],
    };
    let buffer = device.buffer(bytemuck::bytes_of(&trace), false).unwrap();
    let mut uniforms: [f32; 32] = [
        128.0, 128.0, 1.0, 1.0, 0.0, 0.0, 128.0, 128.0, 0.0, 0.0, 72.0, 128.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];
    uniforms[27] = f32::from_bits(zone_mode.unwrap_or(0));
    for _ in 0..draw_count {
        device
            .draw(
                &pipeline,
                Draw {
                    uniforms: bytemuck::cast_slice(&uniforms),
                    vertices: &buffer,
                    indices: None,
                    start: 0,
                    count: 4,
                    instances: 1,
                    rect: Rect {
                        left: 0,
                        top: 0,
                        right: 72,
                        bottom: 128,
                    },
                    stencil: StencilMode::Inherited,
                    atlas: None,
                },
            )
            .unwrap();
    }
    device.finish();
    let pixels = pixels(command, &color);
    let middle = sample(&pixels, 64, 64);
    let expected_red = match zone_mode {
        Some(_) => 64,
        _ if draw_count == 1 => 127,
        _ => 255,
    };
    let expected_green = 255 - expected_red;
    assert!(middle[0].abs_diff(expected_red) <= 2, "{middle:?}");
    assert!(middle[1].abs_diff(expected_green) <= 2, "{middle:?}");
    assert_eq!(sample(&pixels, 80, 64), [0, 255, 0, 255]);
    assert_eq!(sample(&pixels, 64, 80), [0, 255, 0, 255]);
}
#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn metal_copper_subtracts_holes_and_preserves_background() {
    let raw = metal::Device::system_default().expect("Metal device");
    let queue = raw.new_command_queue();
    let command = queue.new_command_buffer();
    let color = target(&raw);
    clear(command, &color);
    let paint = gpui_apple::MetalPaintContext {
        target: gpu_target(1.0),
        device: &raw,
        command_buffer: command,
        color_target: &color,
        color_format: MTLPixelFormat::RGBA8Unorm,
    };
    let device = Device::new(&paint);
    let pipeline = device.pipeline(Shader::Copper).unwrap();
    let vertices: [[f32; 4]; 8] = [
        [-40.0, -40.0, 0.0, 0.0],
        [40.0, -40.0, 0.0, 0.0],
        [40.0, 40.0, 0.0, 0.0],
        [-40.0, 40.0, 0.0, 0.0],
        [-10.0, -10.0, 0.0, 0.0],
        [10.0, -10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0, 0.0],
        [-10.0, 10.0, 0.0, 0.0],
    ];
    let indices: [u32; 12] = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
    let vertices = device
        .buffer(bytemuck::cast_slice(&vertices), false)
        .unwrap();
    let indices = device.buffer(bytemuck::cast_slice(&indices), true).unwrap();
    let mut uniforms = vec![0f32; 44];
    uniforms[..4].copy_from_slice(&[128.0, 128.0, 1.0, 1.0]);
    uniforms[4..8].copy_from_slice(&[0.0, 0.0, 128.0, 128.0]);
    uniforms[8..12].copy_from_slice(&[0.0, 0.0, 128.0, 128.0]);
    uniforms[16..20].copy_from_slice(&[1.0, 1.0, 0.0, 0.0]);
    uniforms[20..24].copy_from_slice(&[1.0, 0.0, 0.0, 1.0]);
    uniforms[24..28].copy_from_slice(&[24.0, 24.0, 104.0, 104.0]);
    let _mask = device.stencil_scope([128.0; 2]).unwrap();
    for (mode, range) in [
        (StencilMode::Clear, None),
        (StencilMode::Toggle, Some(0..6)),
        (StencilMode::ApplyOuter, None),
        (StencilMode::ClearScratch, None),
        (StencilMode::Toggle, Some(6..12)),
        (StencilMode::ApplyHole, None),
        (StencilMode::ClearScratch, None),
        (StencilMode::Shade, None),
    ] {
        let (start, count, index) =
            range.map_or((0, 4, None), |r| (r.start, r.end - r.start, Some(&indices)));
        device
            .draw(
                &pipeline,
                Draw {
                    uniforms: bytemuck::cast_slice(&uniforms),
                    vertices: &vertices,
                    indices: index,
                    start,
                    count,
                    instances: 1,
                    rect: Rect {
                        left: 24,
                        top: 24,
                        right: 104,
                        bottom: 104,
                    },
                    stencil: mode,
                    atlas: None,
                },
            )
            .unwrap();
    }
    device.finish();
    let pixels = pixels(command, &color);
    assert_eq!(sample(&pixels, 40, 40), [255, 0, 0, 255]);
    assert_eq!(sample(&pixels, 64, 64), [0, 255, 0, 255]);
    assert_eq!(sample(&pixels, 10, 10), [0, 255, 0, 255]);
}

#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn metal_empty_curve_override_preserves_background_without_missing_buffers() {
    use crate::backend::common::copper::{CopperDrawOptions, Pipeline, UploadedCopper};
    use pomelo_core::{
        interaction::Camera,
        model::{LayerId, NetId, ObjectId, Point, Zone, ZoneKind},
        task::CancellationToken,
    };
    use std::sync::Arc;
    let raw = metal::Device::system_default().unwrap();
    let queue = raw.new_command_queue();
    let command = queue.new_command_buffer();
    let color = target(&raw);
    clear(command, &color);
    let paint = gpui_apple::MetalPaintContext {
        target: gpu_target(1.0),
        device: &raw,
        command_buffer: command,
        color_target: &color,
        color_format: MTLPixelFormat::RGBA8Unorm,
    };
    let device = Device::new(&paint);
    let context = NativeGpuContext {
        device,
        viewport: [128.0; 2],
        bounds: paint.target.bounds,
        content_mask: gpui::ContentMask {
            bounds: paint.target.clip,
        },
    };
    let device = &context.device;
    let cancel = CancellationToken::default();
    let zone = Zone {
        id: ObjectId(1),
        layer: LayerId(0),
        net: NetId(1),
        kind: ZoneKind::Dynamic,
        paths: vec![],
        mesh: pomelo_core::copper::CopperMesh::build(
            &[vec![
                Point::new(-4.0, -4.0),
                Point::new(4.0, -4.0),
                Point::new(4.0, 4.0),
                Point::new(-4.0, 4.0),
            ]],
            &[],
            &Default::default(),
            &cancel,
        )
        .unwrap(),
    };
    let source = Arc::new(
        crate::copper::PreparedCopper::build(&[zone], Default::default(), &cancel).unwrap(),
    );
    let mut original = UploadedCopper::new(source, device).unwrap();
    original.upload_next(&context).unwrap();
    let mut empty = UploadedCopper::new(
        Arc::new(crate::copper::PreparedCopper {
            memory_reservation: None,
            source: None,
            vertices: vec![],
            indices: vec![],
            batches: vec![],
        }),
        device,
    )
    .unwrap();
    empty.active = true;
    let overrides = BTreeMap::from([(ObjectId(1), empty)]);
    let frame = TraceFrame {
        tracks: Arc::new(
            crate::tracks::PreparedTracks::build(&[], Default::default(), &cancel).unwrap(),
        ),
        bounds: pomelo_core::model::Bounds {
            min: Point::new(-6.0, -6.0),
            max: Point::new(6.0, 6.0),
        },
        camera: Some(Camera {
            center: Point::default(),
            pixels_per_mm: 10.0,
            flipped: false,
        }),
        scale_factor: 1.0,
        colors: Arc::new(BTreeMap::new()),
        fallback_color: [1.0, 0.0, 0.0, 1.0],
        material_override: None,
        opacity: 1.0,
        color_mode: Default::default(),
        pass: OverlayPass::Base,
        filled: true,
        hover_selection: None,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    let mut pipeline = Pipeline::new(device).unwrap();
    let drawn = pipeline
        .draw(
            &context,
            &frame,
            &original,
            CopperDrawOptions {
                opacity: 1.0,
                layer: None,
                visible: None,
                annotations: None,
                annotation_owners: None,
                overrides: Some(&overrides),
                static_shapes_fill_solid: false,
                network_selection: None,
            },
        )
        .unwrap();
    assert_eq!(drawn, (0, 0));
    device.finish();
    assert_eq!(sample(&pixels(command, &color), 64, 64), [0, 255, 0, 255]);
}
