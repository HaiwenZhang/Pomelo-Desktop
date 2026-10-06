//! wgpu command encoding on GPUI's device; uploads use staging copies without a queue.
use super::super::Shader;
use super::super::common::Rect;
use super::super::common::driver::{Draw, StencilMode, StencilScope};
use super::shaders;
use anyhow::{Context as _, ensure};
use gpui_wgpu::wgpu::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
pub(crate) struct Buffer {
    raw: gpui_wgpu::wgpu::Buffer,
    size: usize,
}
pub(crate) struct Atlas {
    view: TextureView,
    sampler: Sampler,
}
pub(crate) struct Pipeline {
    layout: BindGroupLayout,
    variants: BTreeMap<(StencilMode, bool, bool), RenderPipeline>,
}
pub(crate) struct Device<'a> {
    raw: &'a gpui_wgpu::wgpu::Device,
    encoder: RefCell<&'a mut CommandEncoder>,
    color: &'a TextureView,
    format: TextureFormat,
    sample_count: u32,
    size: [u32; 2],
    opacity: f32,
    stencil: RefCell<Option<TextureView>>,
    initialized: Cell<bool>,
    active: Cell<bool>,
}
impl<'a> Device<'a> {
    pub fn new(context: &'a mut gpui_wgpu::WgpuPaintContext<'_>) -> Self {
        Self {
            raw: context.device,
            encoder: RefCell::new(context.encoder),
            color: context.color_target,
            format: context.color_format,
            sample_count: context.target.sample_count,
            size: context.target.size,
            opacity: context.target.opacity,
            stencil: RefCell::new(None),
            initialized: Cell::new(false),
            active: Cell::new(false),
        }
    }
    /// Maximum geometry allocation that can also be bound for drawing.
    pub fn buffer_limit(&self, indices: bool) -> usize {
        let limits = self.raw.limits();
        let limit = if indices {
            limits.max_buffer_size
        } else {
            limits
                .max_buffer_size
                .min(limits.max_storage_buffer_binding_size)
        };
        usize::try_from(limit).unwrap_or(usize::MAX)
    }
    pub fn buffer_empty(&self, size: usize, indices: bool) -> anyhow::Result<Buffer> {
        ensure!(
            size > 0 && size <= self.buffer_limit(indices),
            "GPU_BUFFER_SIZE: requested {size} bytes, limit {} bytes (indices={indices})",
            self.buffer_limit(indices)
        );
        let raw = self.raw.create_buffer(&BufferDescriptor {
            label: Some("PCB geometry"),
            size: size as u64,
            usage: BufferUsages::COPY_DST
                | if indices {
                    BufferUsages::INDEX
                } else {
                    BufferUsages::STORAGE
                },
            mapped_at_creation: false,
        });
        Ok(Buffer { raw, size })
    }
    pub fn structured_buffer(&self, data: &[u8], _: usize) -> anyhow::Result<Buffer> {
        self.buffer(data, false)
    }
    pub fn buffer(&self, data: &[u8], indices: bool) -> anyhow::Result<Buffer> {
        ensure!(!data.is_empty(), "GPU_BUFFER_EMPTY");
        ensure!(
            data.len() <= self.buffer_limit(indices),
            "GPU_BUFFER_SIZE: requested {} bytes, limit {} bytes (indices={indices})",
            data.len(),
            self.buffer_limit(indices)
        );
        let raw = self.raw.create_buffer(&BufferDescriptor {
            label: Some("PCB immutable geometry"),
            size: data.len() as u64,
            usage: if indices {
                BufferUsages::INDEX
            } else {
                BufferUsages::STORAGE
            },
            mapped_at_creation: true,
        });
        raw.slice(..).get_mapped_range_mut().copy_from_slice(data);
        raw.unmap();
        Ok(Buffer {
            raw,
            size: data.len(),
        })
    }
    pub fn write_buffer(&self, buffer: &Buffer, offset: usize, data: &[u8]) -> anyhow::Result<()> {
        ensure!(
            offset
                .checked_add(data.len())
                .is_some_and(|end| end <= buffer.size)
                && offset.is_multiple_of(4)
                && data.len().is_multiple_of(4),
            "GPU_UPLOAD_RANGE"
        );
        let staging = self.raw.create_buffer(&BufferDescriptor {
            label: Some("PCB upload staging"),
            size: data.len() as u64,
            usage: BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        staging
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(data);
        staging.unmap();
        self.encoder.borrow_mut().copy_buffer_to_buffer(
            &staging,
            0,
            &buffer.raw,
            offset as u64,
            data.len() as u64,
        );
        Ok(())
    }
    pub fn atlas(&self, width: u32, height: u32, data: &[u8]) -> anyhow::Result<Atlas> {
        ensure!(
            width > 0 && height > 0 && data.len() == width as usize * height as usize * 4,
            "GPU_ATLAS_SIZE"
        );
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.raw.create_texture(&TextureDescriptor {
            label: Some("PCB MSDF atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let stride = (width as usize * 4).div_ceil(256) * 256;
        let mut padded = vec![0; stride * height as usize];
        for (dst, src) in padded
            .chunks_exact_mut(stride)
            .zip(data.chunks_exact(width as usize * 4))
        {
            dst[..src.len()].copy_from_slice(src);
        }
        let staging = self.raw.create_buffer(&BufferDescriptor {
            label: Some("PCB atlas staging"),
            size: padded.len() as u64,
            usage: BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        staging
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(&padded);
        staging.unmap();
        self.encoder.borrow_mut().copy_buffer_to_texture(
            TexelCopyBufferInfo {
                buffer: &staging,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(height),
                },
            },
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            size,
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        let sampler = self.raw.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Ok(Atlas { view, sampler })
    }
    pub fn pipeline(&self, shader: Shader) -> anyhow::Result<Pipeline> {
        let module = self.raw.create_shader_module(ShaderModuleDescriptor {
            label: Some("PCB native shader"),
            source: ShaderSource::Wgsl(shaders::source(shader).into()),
        });
        let mut entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        if shader == Shader::Label {
            entries.extend([
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ]);
        }
        let layout = self
            .raw
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("PCB bindings"),
                entries: &entries,
            });
        let pipeline_layout = self.raw.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("PCB pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let mut variants = BTreeMap::new();
        for mode in [
            StencilMode::Inherited,
            StencilMode::Clear,
            StencilMode::Exterior,
            StencilMode::Holes,
            StencilMode::Toggle,
            StencilMode::ApplyOuter,
            StencilMode::ApplyHole,
            StencilMode::ClearScratch,
            StencilMode::Shade,
        ] {
            for inherited in [false, true] {
                for indexed in [false, true] {
                    if shader != Shader::Copper && (mode != StencilMode::Inherited || indexed) {
                        continue;
                    }
                    let (read, write, _, equal, invert, zero) = mode.state(inherited);
                    let face = StencilFaceState {
                        compare: if equal {
                            CompareFunction::Equal
                        } else {
                            CompareFunction::Always
                        },
                        fail_op: StencilOperation::Keep,
                        depth_fail_op: StencilOperation::Keep,
                        pass_op: if invert {
                            StencilOperation::Invert
                        } else if zero {
                            StencilOperation::Zero
                        } else if write != 0 {
                            StencilOperation::Replace
                        } else {
                            StencilOperation::Keep
                        },
                    };
                    let pipeline = self.raw.create_render_pipeline(&RenderPipelineDescriptor {
                        label: Some("PCB pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: VertexState {
                            module: &module,
                            entry_point: Some(if shader == Shader::Copper && !indexed {
                                "clear_main"
                            } else {
                                "vertex_main"
                            }),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(FragmentState {
                            module: &module,
                            entry_point: Some("fragment_main"),
                            compilation_options: Default::default(),
                            targets: &[Some(ColorTargetState {
                                format: self.format,
                                blend: Some(BlendState::ALPHA_BLENDING),
                                write_mask: if mode.color() {
                                    ColorWrites::ALL
                                } else {
                                    ColorWrites::empty()
                                },
                            })],
                        }),
                        primitive: PrimitiveState {
                            topology: if indexed {
                                PrimitiveTopology::TriangleList
                            } else {
                                PrimitiveTopology::TriangleStrip
                            },
                            cull_mode: None,
                            ..Default::default()
                        },
                        depth_stencil: Some(DepthStencilState {
                            format: TextureFormat::Depth24PlusStencil8,
                            depth_write_enabled: Some(false),
                            depth_compare: Some(CompareFunction::Always),
                            stencil: StencilState {
                                front: face,
                                back: face,
                                read_mask: read,
                                write_mask: write,
                            },
                            bias: Default::default(),
                        }),
                        multisample: MultisampleState {
                            count: self.sample_count,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    });
                    variants.insert((mode, inherited, indexed), pipeline);
                }
            }
        }
        Ok(Pipeline { layout, variants })
    }
    fn ensure_stencil(&self) {
        if self.stencil.borrow().is_some() {
            return;
        }
        let texture = self.raw.create_texture(&TextureDescriptor {
            label: Some("PCB private stencil"),
            size: Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: self.sample_count,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth24PlusStencil8,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        *self.stencil.borrow_mut() = Some(texture.create_view(&Default::default()));
    }
    pub fn stencil_scope(&self, _: [f32; 2]) -> anyhow::Result<StencilScope<'_>> {
        self.ensure_stencil();
        let previous = self.active.replace(true);
        Ok(StencilScope {
            active: &self.active,
            previous,
        })
    }
    pub fn draw(&self, pipeline: &Pipeline, draw: Draw<'_, Buffer, Atlas>) -> anyhow::Result<()> {
        let Rect {
            left,
            top,
            right,
            bottom,
        } = draw.rect;
        ensure!(
            left >= 0 && top >= 0 && right > left && bottom > top,
            "GPU_SCISSOR_INVALID"
        );
        let mut bytes = draw.uniforms.to_vec();
        bytes.extend_from_slice(bytemuck::cast_slice(&[self.opacity, 0.0, 0.0, 0.0]));
        let uniforms = self.raw.create_buffer(&BufferDescriptor {
            label: Some("PCB draw uniforms"),
            size: bytes.len() as u64,
            usage: BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        uniforms
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(&bytes);
        uniforms.unmap();
        let mut entries = vec![
            BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: draw.vertices.raw.as_entire_binding(),
            },
        ];
        if let Some(atlas) = draw.atlas {
            entries.extend([
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&atlas.view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&atlas.sampler),
                },
            ]);
        }
        let bindings = self.raw.create_bind_group(&BindGroupDescriptor {
            label: Some("PCB draw bindings"),
            layout: &pipeline.layout,
            entries: &entries,
        });
        self.ensure_stencil();
        let stencil = self.stencil.borrow();
        let stencil = stencil.as_ref().context("GPU_STENCIL_MISSING")?;
        let mut encoder = self.encoder.borrow_mut();
        let attachments = [Some(RenderPassColorAttachment {
            view: self.color,
            resolve_target: None,
            depth_slice: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("PCB draw"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: stencil,
                depth_ops: None,
                stencil_ops: Some(Operations {
                    load: if self.initialized.replace(true) {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(0)
                    },
                    store: StoreOp::Store,
                }),
            }),
            ..Default::default()
        });
        pass.set_pipeline(
            &pipeline.variants[&(draw.stencil, self.active.get(), draw.indices.is_some())],
        );
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_stencil_reference(draw.stencil.state(self.active.get()).2);
        pass.set_scissor_rect(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        );
        if let Some(indices) = draw.indices {
            pass.set_index_buffer(indices.raw.slice(..), IndexFormat::Uint32);
            pass.draw_indexed(draw.start..draw.start + draw.count, 0, 0..draw.instances);
        } else {
            pass.draw(draw.start..draw.start + draw.count, 0..draw.instances);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/backend/wgpu/pixels.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/backend/wgpu/resources.rs"]
mod resource_tests;
