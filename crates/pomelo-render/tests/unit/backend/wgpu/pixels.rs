//! Synthetic readback coverage for the Linux wgpu command driver.
use super::*;

use gpui::{Bounds, GpuPaintTarget, ScaledPixels, point, size};
#[test]
#[ignore = "requires a GPU adapter; synthetic pixel readback"]
fn wgpu_pipelines_and_staged_upload_preserve_clip_and_opacity() {
    let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
    let adapter =
        futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
            .expect("wgpu adapter");
    let (raw, queue) =
        futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
    let size = Extent3d {
        width: 128,
        height: 128,
        depth_or_array_layers: 1,
    };
    let texture = raw.create_texture(&TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = raw.create_command_encoder(&Default::default());
    {
        let attachments = [Some(RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            depth_slice: None,
            ops: Operations {
                load: LoadOp::Clear(Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                }),
                store: StoreOp::Store,
            },
        })];
        let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &attachments,
            ..Default::default()
        });
    }
    let bounds = Bounds::new(point(ScaledPixels(0.0), ScaledPixels(0.0)), size_fn());
    {
        let mut context = gpui_wgpu::WgpuPaintContext {
            target: GpuPaintTarget {
                size: [128, 128],
                bounds,
                clip: bounds,
                scale_factor: 1.0,
                sample_count: 1,
                opacity: 0.5,
            },
            device: &raw,
            encoder: &mut encoder,
            color_target: &view,
            color_format: TextureFormat::Rgba8Unorm,
        };
        let device = Device::new(&mut context);
        for shader in [
            Shader::Pad,
            Shader::Text,
            Shader::Label,
            Shader::Copper,
            Shader::Probe,
        ] {
            device.pipeline(shader).unwrap();
        }
        let pipeline = device.pipeline(Shader::Trace).unwrap();
        let trace = crate::tracks::TraceInstance {
            a: [-40.0, 0.0, 0.0, 0.0],
            b: [40.0, 0.0, 0.0, 0.0],
            center: [0.0; 4],
            arc: [0.0; 4],
            bounds_min: [-42.0, -2.0, 0.0, 0.0],
            bounds_max: [42.0, 2.0, 0.0, 0.0],
            ids: [0; 4],
            flags: [0, 0, 4.0f32.to_bits(), 0],
        };
        let buffer = device
            .buffer_empty(size_of::<crate::tracks::TraceInstance>(), false)
            .unwrap();
        device
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&trace))
            .unwrap();
        let uniforms: [f32; 32] = [
            128.0, 128.0, 1.0, 1.0, 0.0, 0.0, 128.0, 128.0, 0.0, 0.0, 72.0, 128.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
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
    let output = raw.create_buffer(&BufferDescriptor {
        label: None,
        size: 128 * 128 * 4,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &output,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(128),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    output.slice(..).map_async(MapMode::Read, move |result| {
        send.send(result).unwrap();
    });
    raw.poll(PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    })
    .unwrap();
    receive.recv().unwrap().unwrap();
    let pixels = output.slice(..).get_mapped_range();
    let sample =
        |x: usize, y: usize| -> [u8; 4] { pixels[(y * 128 + x) * 4..][..4].try_into().unwrap() };
    let middle = sample(64, 64);
    assert!(
        (126..=129).contains(&middle[0]) && (126..=129).contains(&middle[1]),
        "{middle:?}"
    );
    assert_eq!(sample(80, 64), [0, 255, 0, 255]);
    assert_eq!(sample(64, 80), [0, 255, 0, 255]);
}
fn size_fn() -> gpui::Size<ScaledPixels> {
    size(ScaledPixels(128.0), ScaledPixels(128.0))
}
