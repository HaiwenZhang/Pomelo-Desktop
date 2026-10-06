//! Linux driver resource validation and command execution on a headless adapter.
use super::*;
use gpui::{Bounds, GpuPaintTarget, ScaledPixels, point, size};

struct GpuFixture {
    raw: gpui_wgpu::wgpu::Device,
    queue: Queue,
    view: TextureView,
}

impl GpuFixture {
    fn new() -> Self {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
        let adapter = futures::executor::block_on(instance.request_adapter(&Default::default()))
            .expect("headless wgpu adapter");
        let mut limits = Limits::downlevel_defaults()
            .using_resolution(adapter.limits())
            .using_alignment(adapter.limits());
        limits.max_buffer_size = 1024;
        limits.max_storage_buffer_binding_size = 256;
        let (raw, queue) = futures::executor::block_on(adapter.request_device(&DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .unwrap();
        let texture = raw.create_texture(&TextureDescriptor {
            label: Some("Linux resource test target"),
            size: Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            raw,
            queue,
            view: texture.create_view(&Default::default()),
        }
    }

    fn encode<T>(&self, run: impl FnOnce(&Device<'_>) -> T) -> T {
        let mut encoder = self.raw.create_command_encoder(&Default::default());
        let bounds = Bounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(16.0), ScaledPixels(16.0)),
        );
        let mut context = gpui_wgpu::WgpuPaintContext {
            target: GpuPaintTarget {
                size: [16, 16],
                bounds,
                clip: bounds,
                scale_factor: 1.0,
                sample_count: 1,
                opacity: 1.0,
            },
            device: &self.raw,
            encoder: &mut encoder,
            color_target: &self.view,
            color_format: TextureFormat::Rgba8Unorm,
        };
        let device = Device::new(&mut context);
        let result = run(&device);
        drop(device);
        self.queue.submit([encoder.finish()]);
        self.wait();
        result
    }

    fn wait(&self) {
        self.raw
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
    }

    fn read(&self, buffer: &gpui_wgpu::wgpu::Buffer) -> Vec<u8> {
        let (send, receive) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
        self.wait();
        receive.recv().unwrap().unwrap();
        let bytes = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }
}

// Read resources through their production STORAGE/TEXTURE_BINDING usage rather
// than adding COPY_SRC to the driver just for tests.
fn readback(
    device: &Device<'_>,
    input: BindingResource<'_>,
    shader: &str,
    bytes: u64,
) -> gpui_wgpu::wgpu::Buffer {
    let storage = device.raw.create_buffer(&BufferDescriptor {
        label: Some("Linux resource test compute output"),
        size: bytes,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let output = device.raw.create_buffer(&BufferDescriptor {
        label: Some("Linux resource test readback"),
        size: bytes,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let module = device.raw.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device
        .raw
        .create_compute_pipeline(&ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bindings = device.raw.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: input,
            },
            BindGroupEntry {
                binding: 1,
                resource: storage.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.encoder.borrow_mut();
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &output, 0, bytes);
    output
}

const COPY_WORDS: &str = "
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(1) fn main() {
    for (var i = 0u; i < arrayLength(&output); i++) { output[i] = input[i]; }
}";

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn storage_buffers_respect_binding_limit_below_allocation_limit() {
    GpuFixture::new().encode(|device| {
        let error = device
            .buffer_empty(512, false)
            .err()
            .expect("storage binding limit");
        assert!(error.to_string().contains("limit 256 bytes"), "{error}");
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn index_buffers_can_exceed_storage_binding_limit() {
    GpuFixture::new().encode(|device| {
        let buffer = device.buffer_empty(512, true).unwrap();
        assert_eq!(buffer.raw.size(), 512);
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn index_buffers_reject_sizes_above_allocation_limit() {
    GpuFixture::new().encode(|device| {
        let error = device
            .buffer_empty(1028, true)
            .err()
            .expect("allocation limit");
        assert!(error.to_string().contains("limit 1024 bytes"), "{error}");
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn mutable_buffers_reject_zero_size() {
    GpuFixture::new().encode(|device| {
        let error = device
            .buffer_empty(0, false)
            .err()
            .expect("empty allocation");
        assert!(error.to_string().contains("GPU_BUFFER_SIZE"), "{error}");
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn immutable_buffers_reject_empty_data() {
    GpuFixture::new().encode(|device| {
        let error = device.buffer(&[], false).err().expect("empty geometry");
        assert_eq!(error.to_string(), "GPU_BUFFER_EMPTY");
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer limits"]
fn immutable_buffers_reject_oversized_storage_data_before_gpu_validation() {
    GpuFixture::new().encode(|device| {
        let error = device
            .structured_buffer(&[0; 512], 16)
            .err()
            .expect("binding limit");
        assert!(error.to_string().contains("limit 256 bytes"), "{error}");
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux buffer readback"]
fn immutable_storage_buffers_preserve_initial_bytes() {
    let fixture = GpuFixture::new();
    let expected: [u32; 4] = [1, 0x12345678, u32::MAX, 9];
    let output = fixture.encode(|device| {
        let buffer = device
            .structured_buffer(bytemuck::cast_slice(&expected), 4)
            .unwrap();
        readback(device, buffer.raw.as_entire_binding(), COPY_WORDS, 16)
    });
    assert_eq!(
        fixture.read(&output),
        bytemuck::cast_slice::<_, u8>(&expected)
    );
}

#[test]
#[ignore = "requires a GPU adapter; Linux staged upload readback"]
fn staged_uploads_preserve_untouched_bytes_and_write_the_final_word() {
    let fixture = GpuFixture::new();
    let initial: [u32; 4] = [11, 22, 33, 44];
    let output = fixture.encode(|device| {
        let buffer = device.buffer_empty(16, false).unwrap();
        device
            .write_buffer(&buffer, 0, bytemuck::cast_slice(&initial))
            .unwrap();
        device
            .write_buffer(&buffer, 4, bytemuck::bytes_of(&55u32))
            .unwrap();
        device
            .write_buffer(&buffer, 12, bytemuck::bytes_of(&66u32))
            .unwrap();
        readback(device, buffer.raw.as_entire_binding(), COPY_WORDS, 16)
    });
    assert_eq!(
        fixture.read(&output),
        bytemuck::cast_slice::<_, u8>(&[11u32, 55, 33, 66])
    );
}

#[test]
#[ignore = "requires a GPU adapter; Linux upload validation"]
fn staged_uploads_reject_unaligned_offsets_and_lengths() {
    GpuFixture::new().encode(|device| {
        let buffer = device.buffer_empty(16, false).unwrap();
        for (offset, data) in [(1, &[0u8; 4][..]), (0, &[0u8; 3][..])] {
            let error = device.write_buffer(&buffer, offset, data).unwrap_err();
            assert_eq!(error.to_string(), "GPU_UPLOAD_RANGE");
        }
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux upload validation"]
fn staged_uploads_reject_out_of_bounds_and_overflowing_ranges() {
    GpuFixture::new().encode(|device| {
        let buffer = device.buffer_empty(16, false).unwrap();
        for offset in [16, usize::MAX - 3] {
            let error = device.write_buffer(&buffer, offset, &[0; 4]).unwrap_err();
            assert_eq!(error.to_string(), "GPU_UPLOAD_RANGE");
        }
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux atlas validation"]
fn atlases_reject_missing_pixels_and_zero_dimensions() {
    GpuFixture::new().encode(|device| {
        for (width, height, data) in [(1, 1, &[0u8; 3][..]), (0, 1, &[][..]), (1, 0, &[][..])] {
            let error = device
                .atlas(width, height, data)
                .err()
                .expect("invalid atlas");
            assert_eq!(error.to_string(), "GPU_ATLAS_SIZE");
        }
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux atlas pixel readback"]
fn atlas_uploads_preserve_pixels_across_padded_rows() {
    let fixture = GpuFixture::new();
    let expected = [
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 0, 32, 255, 0, 255, 16, 0, 255,
        255, 0,
    ];
    let output = fixture.encode(|device| {
        let atlas = device.atlas(3, 2, &expected).unwrap();
        readback(
            device,
            BindingResource::TextureView(&atlas.view),
            "
@group(0) @binding(0) var input: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(1) fn main() {
    for (var i = 0u; i < 6u; i++) {
        output[i] = pack4x8unorm(textureLoad(input, vec2i(vec2u(i % 3u, i / 3u)), 0));
    }
}",
            24,
        )
    });
    assert_eq!(fixture.read(&output), expected);
}

#[test]
#[ignore = "requires a GPU adapter; Linux stencil scopes"]
fn nested_stencil_scopes_restore_the_previous_active_state() {
    GpuFixture::new().encode(|device| {
        assert!(!device.active.get());
        {
            let _outer = device.stencil_scope([16.0; 2]).unwrap();
            {
                let _inner = device.stencil_scope([16.0; 2]).unwrap();
                assert!(device.active.get());
            }
            assert!(device.active.get());
        }
        assert!(!device.active.get());
    });
}

#[test]
#[ignore = "requires a GPU adapter; Linux draw validation"]
fn draws_reject_empty_or_negative_scissors_before_encoding() {
    GpuFixture::new().encode(|device| {
        let pipeline = device.pipeline(Shader::Trace).unwrap();
        let vertices = device.buffer_empty(128, false).unwrap();
        for rect in [
            Rect {
                left: -1,
                top: 0,
                right: 16,
                bottom: 16,
            },
            Rect {
                left: 0,
                top: -1,
                right: 16,
                bottom: 16,
            },
            Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 16,
            },
            Rect {
                left: 0,
                top: 0,
                right: 16,
                bottom: 0,
            },
        ] {
            let error = device
                .draw(
                    &pipeline,
                    Draw {
                        uniforms: &[],
                        vertices: &vertices,
                        indices: None,
                        start: 0,
                        count: 4,
                        instances: 1,
                        rect,
                        stencil: StencilMode::Inherited,
                        atlas: None,
                    },
                )
                .unwrap_err();
            assert_eq!(error.to_string(), "GPU_SCISSOR_INVALID");
        }
    });
}
