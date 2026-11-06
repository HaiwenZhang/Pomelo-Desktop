//! The same depth/OIT image contract as D3D11, on a wgpu adapter.
use super::*;
use crate::backend::{
    NativeGpuContext,
    solid_pipeline::{SolidPipeline, tests::verify},
};
use gpui::{Bounds, GpuPaintTarget, ScaledPixels, point, size};
#[test]
fn solid_wgpu_depth_oit_clipping_opacity_and_cache_pixels() {
    let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
    let adapter =
        futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
            .expect("wgpu solid adapter");
    let (raw, queue) =
        futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
    let extent = Extent3d {
        width: 128,
        height: 128,
        depth_or_array_layers: 1,
    };
    let texture = raw.create_texture(&TextureDescriptor {
        label: Some("Solid pixel output"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    verify(|pipeline, frame, clipped, opacity| {
        let mut encoder = raw.create_command_encoder(&Default::default());
        {
            let colors = [Some(RenderPassColorAttachment {
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
                color_attachments: &colors,
                ..Default::default()
            });
        }
        let bounds = Bounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(128.0), ScaledPixels(128.0)),
        );
        let clip = if clipped {
            Bounds::new(bounds.origin, size(ScaledPixels(72.0), ScaledPixels(128.0)))
        } else {
            bounds
        };
        {
            let mut paint = gpui::WgpuPaintContext {
                target: GpuPaintTarget {
                    size: [128, 128],
                    bounds,
                    clip,
                    scale_factor: 1.0,
                    sample_count: 1,
                    opacity,
                },
                device: &raw,
                encoder: &mut encoder,
                color_target: &view,
                color_format: TextureFormat::Rgba8Unorm,
            };
            let gpu = NativeGpuContext {
                device: Device::new(&mut paint),
                viewport: [128.0; 2],
                bounds,
                content_mask: gpui::ContentMask { bounds: clip },
            };
            if pipeline.is_none() {
                *pipeline = Some(SolidPipeline::new(&gpu.device, &frame.scene).unwrap());
            }
            pipeline.as_mut().unwrap().draw(&gpu, frame).unwrap();
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
            extent,
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
        output.slice(..).get_mapped_range().unwrap().to_vec()
    });
}
