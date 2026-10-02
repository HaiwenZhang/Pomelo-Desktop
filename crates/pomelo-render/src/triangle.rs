//! Hardware-only wgpu/WGSL smoke probe. Readback is an experiment, not the PCB backend.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use serde::Serialize;

/// Native graphics API requested for the experiment; no implicit backend fallback.
#[derive(Clone, Copy, Debug)]
pub enum ProbeBackend {
    Dx12,
    Vulkan,
    Metal,
}

impl ProbeBackend {
    pub const fn native() -> Self {
        if cfg!(target_os = "windows") {
            Self::Dx12
        } else if cfg!(target_os = "macos") {
            Self::Metal
        } else {
            Self::Vulkan
        }
    }

    fn flags(self) -> wgpu::Backends {
        match self {
            Self::Dx12 => wgpu::Backends::DX12,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Metal => wgpu::Backends::METAL,
        }
    }
}

/// Probe failures retain technical details separately from the translated summary.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("GPU_PROBE_SIZE_INVALID {0}x{1}")]
    InvalidSize(u32, u32),
    #[error("GPU_ADAPTER_FAILED {0}")]
    Adapter(String),
    #[error("GPU_SOFTWARE_ADAPTER {0}")]
    SoftwareAdapter(String),
    #[error("GPU_DEVICE_FAILED {0}")]
    Device(String),
    #[error("GPU_VALIDATION_FAILED {0}")]
    Validation(String),
    #[error("GPU_READBACK_FAILED {0}")]
    Readback(String),
    #[error("GPU_TRIANGLE_PIXELS_INVALID")]
    PixelsInvalid,
}

impl ProbeError {
    pub fn diagnostic(&self) -> Diagnostic {
        let code = match self {
            Self::InvalidSize(..) => "GPU_PROBE_SIZE_INVALID",
            Self::Adapter(_) => "GPU_ADAPTER_FAILED",
            Self::SoftwareAdapter(_) => "GPU_SOFTWARE_ADAPTER",
            Self::Device(_) => "GPU_DEVICE_FAILED",
            Self::Validation(_) => "GPU_VALIDATION_FAILED",
            Self::Readback(_) => "GPU_READBACK_FAILED",
            Self::PixelsInvalid => "GPU_TRIANGLE_PIXELS_INVALID",
        };
        Diagnostic::error(code, MessageKey::GpuFailed).with_details(self.to_string())
    }
}

/// Machine-readable evidence; timing includes CPU waiting and readback, not GPU timestamps.
#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub wgpu_version: &'static str,
    pub shader_language: &'static str,
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub width: u32,
    pub height: u32,
    pub initialization_ms: f64,
    pub draw_and_readback_ms: f64,
    pub triangle_pixels: usize,
    pub pixels_verified: bool,
    pub presentation: &'static str,
}

/// Unpadded RGBA8 pixels produced by the GPU; no CPU triangle rasterization is used.
pub struct TriangleFrame {
    pub rgba: Vec<u8>,
    pub report: ProbeReport,
}

/// Render one triangle on a hardware adapter and validate its color/orientation/coverage.
/// Run on a worker thread: device initialization and mapping can block.
pub fn render_triangle(
    backend: ProbeBackend,
    width: u32,
    height: u32,
) -> Result<TriangleFrame, ProbeError> {
    let (row_bytes, padded_row_bytes) = readback_layout(width, height)?;
    let initialization = Instant::now();
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backend.flags();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|error| ProbeError::Adapter(error.to_string()))?;
    let info = adapter.get_info();
    if matches!(
        info.device_type,
        wgpu::DeviceType::Cpu | wgpu::DeviceType::Other
    ) {
        return Err(ProbeError::SoftwareAdapter(info.name));
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("pomelo-triangle-probe"),
        ..Default::default()
    }))
    .map_err(|error| ProbeError::Device(error.to_string()))?;

    let out_of_memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("pomelo-triangle-wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/triangle.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pomelo-triangle-pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pomelo-triangle-target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pomelo-triangle-readback"),
        size: u64::from(padded_row_bytes) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let initialization_ms = initialization.elapsed().as_secs_f64() * 1000.0;
    let draw = Instant::now();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pomelo-triangle-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    let submission = queue.submit([encoder.finish()]);
    let validation_future = validation.pop();
    let internal_future = internal.pop();
    let memory_future = out_of_memory.pop();
    let (sender, receiver) = mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(10)),
        })
        .map_err(|error| ProbeError::Readback(error.to_string()))?;
    if let Some(error) = [
        pollster::block_on(validation_future),
        pollster::block_on(internal_future),
        pollster::block_on(memory_future),
    ]
    .into_iter()
    .flatten()
    .next()
    {
        return Err(ProbeError::Validation(error.to_string()));
    }
    receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|error| ProbeError::Readback(error.to_string()))?
        .map_err(|error| ProbeError::Readback(error.to_string()))?;
    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .map_err(|error| ProbeError::Readback(error.to_string()))?;
    let mut rgba = Vec::with_capacity((u64::from(row_bytes) * u64::from(height)) as usize);
    for row in mapped.chunks_exact(padded_row_bytes as usize) {
        rgba.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    readback.unmap();
    let draw_and_readback_ms = draw.elapsed().as_secs_f64() * 1000.0;
    let triangle_pixels = verify_pixels(&rgba, width, height)?;
    Ok(TriangleFrame {
        rgba,
        report: ProbeReport {
            schema_version: 1,
            wgpu_version: "30.0.1",
            shader_language: "WGSL",
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: info.driver,
            driver_info: info.driver_info,
            width,
            height,
            initialization_ms,
            draw_and_readback_ms,
            triangle_pixels,
            pixels_verified: true,
            presentation: "cpu_readback_to_gpui_image",
        },
    })
}

fn readback_layout(width: u32, height: u32) -> Result<(u32, u32), ProbeError> {
    if !(16..=4096).contains(&width) || !(16..=4096).contains(&height) {
        return Err(ProbeError::InvalidSize(width, height));
    }
    let row = width * 4;
    Ok((
        row,
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
    ))
}

fn verify_pixels(rgba: &[u8], width: u32, height: u32) -> Result<usize, ProbeError> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(ProbeError::PixelsInvalid);
    }
    let pixel = |x: u32, y: u32| {
        let offset = (y as usize * width as usize + x as usize) * 4;
        &rgba[offset..offset + 4]
    };
    let colored = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 255)
        .count();
    // Known interior points verify the vertex colors and image's vertical orientation.
    let samples = [
        (width / 2, height / 4, 0),
        (width * 3 / 10, height * 7 / 10, 1),
        (width * 7 / 10, height * 7 / 10, 2),
    ];
    let valid_samples = samples.into_iter().all(|(x, y, channel)| {
        let value = pixel(x, y);
        value[3] == 255
            && value[channel] > 140
            && (0..3)
                .filter(|other| *other != channel)
                .all(|other| value[channel].saturating_sub(value[other]) > 50)
    });
    let total = width as usize * height as usize;
    if pixel(0, 0) != [0, 0, 0, 0] || colored < total / 5 || colored > total / 3 || !valid_samples {
        return Err(ProbeError::PixelsInvalid);
    }
    Ok(colored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_rows_align_for_non_multiple_widths_and_reject_unbounded_sizes() {
        assert_eq!(readback_layout(257, 129).unwrap(), (1028, 1280));
        assert!(readback_layout(0, 100).is_err());
        assert!(readback_layout(u32::MAX, 100).is_err());
    }

    #[test]
    fn blank_or_wrong_sized_images_cannot_pass_the_gpu_probe() {
        assert!(verify_pixels(&vec![0; 64 * 64 * 4], 64, 64).is_err());
        assert!(verify_pixels(&[], 64, 64).is_err());
    }

    #[test]
    fn probe_errors_have_translated_summaries_in_all_five_languages() {
        let error = ProbeError::Readback("driver details".into()).diagnostic();
        for locale in pomelo_core::i18n::Locale::ALL {
            let summary = error.message.render(locale).unwrap();
            assert!(!summary.contains("render.failed"));
            assert!(!summary.contains("driver details"));
        }
        assert_eq!(error.code.as_ref(), "GPU_READBACK_FAILED");
    }
}
