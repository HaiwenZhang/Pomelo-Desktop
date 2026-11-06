//! WGSL solid rendering on GPUI's borrowed wgpu encoder.
use super::*;
use crate::backend::{
    solid_cache::upload_memory,
    solid_pipeline::{BufferKind, SolidDraw},
};

struct Targets {
    size: [u32; 2],
    color: TextureView,
    accum: TextureView,
    reveal: TextureView,
    depth: TextureView,
}
pub(crate) struct SolidDriver {
    geometry_layout: BindGroupLayout,
    composite_layout: BindGroupLayout,
    opaque: RenderPipeline,
    transparent: RenderPipeline,
    composite: RenderPipeline,
    targets: Option<Targets>,
}
impl Device<'_> {
    pub(crate) fn solid_opacity(&self) -> f32 {
        self.opacity
    }
    pub(crate) fn solid_buffer(&self, data: &[u8], kind: BufferKind) -> anyhow::Result<Buffer> {
        let limit = match kind {
            BufferKind::Material => self.buffer_limit(false),
            _ => self.buffer_limit(true),
        };
        ensure!(
            !data.is_empty() && data.len() <= limit,
            "SOLID_GPU_BUFFER_LIMIT"
        );
        let raw = self.raw.create_buffer(&BufferDescriptor {
            label: Some("Solid geometry"),
            size: data.len() as u64,
            usage: match kind {
                BufferKind::Vertex => BufferUsages::VERTEX,
                BufferKind::Index => BufferUsages::INDEX,
                BufferKind::Material => BufferUsages::STORAGE,
            },
            mapped_at_creation: true,
        });
        raw.slice(..).get_mapped_range_mut()?.copy_from_slice(data);
        raw.unmap();
        Ok(Buffer {
            raw,
            size: data.len(),
        })
    }
}
impl SolidDriver {
    pub(crate) fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        let raw = device.raw;
        let module = raw.create_shader_module(ShaderModuleDescriptor {
            label: Some("Solid WGSL"),
            source: ShaderSource::Wgsl(include_str!("../../../shaders/solid.wgsl").into()),
        });
        let uniform = BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let geometry_layout = raw.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Solid materials"),
            entries: &[
                uniform,
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let geometry_pipeline = raw.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Solid geometry layout"),
            bind_group_layouts: &[Some(&geometry_layout)],
            immediate_size: 0,
        });
        let texture = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let composite_layout = raw.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Solid composition"),
            entries: &[uniform, texture(4), texture(5), texture(6)],
        });
        let composite_pipeline = raw.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Solid composite layout"),
            bind_group_layouts: &[Some(&composite_layout)],
            immediate_size: 0,
        });
        let vertices = [
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 16,
                shader_location: 1,
            },
            VertexAttribute {
                format: VertexFormat::Uint32x4,
                offset: 32,
                shader_location: 2,
            },
        ];
        let instances = [
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 3,
            },
            VertexAttribute {
                format: VertexFormat::Uint32x4,
                offset: 16,
                shader_location: 4,
            },
        ];
        let buffers = [
            VertexBufferLayout {
                array_stride: 48,
                step_mode: VertexStepMode::Vertex,
                attributes: &vertices,
            },
            VertexBufferLayout {
                array_stride: 32,
                step_mode: VertexStepMode::Instance,
                attributes: &instances,
            },
        ];
        let additive = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        let opaque_targets = [Some(ColorTargetState {
            format: TextureFormat::Rgba16Float,
            blend: None,
            write_mask: ColorWrites::ALL,
        })];
        let transparent_targets = [
            Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: Some(BlendState {
                    color: additive,
                    alpha: additive,
                }),
                write_mask: ColorWrites::ALL,
            }),
            Some(ColorTargetState {
                format: TextureFormat::R16Float,
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::OneMinusSrc,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::REPLACE,
                }),
                write_mask: ColorWrites::ALL,
            }),
        ];
        let geometry = |transparent| {
            raw.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("Solid geometry"),
                layout: Some(&geometry_pipeline),
                vertex: VertexState {
                    module: &module,
                    entry_point: Some("solid_vertex"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(FragmentState {
                    module: &module,
                    entry_point: Some(if transparent {
                        "solid_transparent"
                    } else {
                        "solid_opaque"
                    }),
                    compilation_options: Default::default(),
                    targets: if transparent {
                        &transparent_targets
                    } else {
                        &opaque_targets
                    },
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: Some(DepthStencilState {
                    format: TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!transparent),
                    depth_compare: Some(CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = geometry(false);
        let transparent = geometry(true);
        let composite = raw.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Solid composite"),
            layout: Some(&composite_pipeline),
            vertex: VertexState {
                module: &module,
                entry_point: Some("composite_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("composite_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: device.format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState {
                count: device.sample_count,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            geometry_layout,
            composite_layout,
            opaque,
            transparent,
            composite,
            targets: None,
        })
    }
    fn ensure_targets(&mut self, device: &Device<'_>, size: [u32; 2]) -> anyhow::Result<()> {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return Ok(());
        }
        let bytes = u64::from(size[0]) * u64::from(size[1]) * 22;
        ensure!(
            size.into_iter()
                .all(|v| v > 0 && v <= device.raw.limits().max_texture_dimension_2d)
                && bytes <= 512 * 1024 * 1024,
            "SOLID_TARGET_MEMORY_LIMIT"
        );
        let _memory = upload_memory(bytes as usize)?;
        let texture = |format, sampled| {
            device
                .raw
                .create_texture(&TextureDescriptor {
                    label: Some("Solid target"),
                    size: Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT
                        | if sampled {
                            TextureUsages::TEXTURE_BINDING
                        } else {
                            TextureUsages::empty()
                        },
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        self.targets = Some(Targets {
            size,
            color: texture(TextureFormat::Rgba16Float, true),
            accum: texture(TextureFormat::Rgba16Float, true),
            reveal: texture(TextureFormat::R16Float, true),
            depth: texture(TextureFormat::Depth32Float, false),
        });
        Ok(())
    }
    pub(crate) fn draw(
        &mut self,
        device: &Device<'_>,
        draw: SolidDraw<'_, Buffer>,
    ) -> anyhow::Result<u64> {
        self.ensure_targets(device, draw.size)?;
        let t = self.targets.as_ref().context("SOLID_TARGETS")?;
        let uniforms = device.raw.create_buffer(&BufferDescriptor {
            label: Some("Solid frame"),
            size: size_of_val(&draw.uniforms) as u64,
            usage: BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        uniforms
            .slice(..)
            .get_mapped_range_mut()?
            .copy_from_slice(bytemuck::bytes_of(&draw.uniforms));
        uniforms.unmap();
        let geometry = device.raw.create_bind_group(&BindGroupDescriptor {
            label: Some("Solid materials"),
            layout: &self.geometry_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: draw.material.raw.as_entire_binding(),
                },
            ],
        });
        let composite = device.raw.create_bind_group(&BindGroupDescriptor {
            label: Some("Solid composition"),
            layout: &self.composite_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(&t.color),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(&t.accum),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(&t.reveal),
                },
            ],
        });
        let attachment = |view, clear| {
            Some(RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: Operations {
                    load: LoadOp::Clear(clear),
                    store: StoreOp::Store,
                },
            })
        };
        let encode = |encoder: &mut CommandEncoder, transparent: bool| {
            let colors = if transparent {
                vec![
                    attachment(&t.accum, Color::TRANSPARENT),
                    attachment(&t.reveal, Color::WHITE),
                ]
            } else {
                vec![attachment(&t.color, Color::TRANSPARENT)]
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Solid geometry"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(Operations {
                        load: if transparent {
                            LoadOp::Load
                        } else {
                            LoadOp::Clear(1.0)
                        },
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(if transparent {
                &self.transparent
            } else {
                &self.opaque
            });
            pass.set_bind_group(0, &geometry, &[]);
            let mut calls = 0;
            for &id in if transparent {
                draw.transparent
            } else {
                draw.opaque
            } {
                if !draw.visible[id] {
                    continue;
                }
                let page = &draw.scene.pages[id];
                let (Some(mesh), Some(instances)) = (&draw.meshes[page.mesh], &draw.instances[id])
                else {
                    continue;
                };
                pass.set_vertex_buffer(0, mesh.vertices.raw.slice(..));
                pass.set_vertex_buffer(1, instances.raw.slice(..));
                pass.set_index_buffer(mesh.indices.raw.slice(..), IndexFormat::Uint32);
                pass.draw_indexed(
                    0..draw.scene.meshes[page.mesh].indices.len() as u32,
                    0,
                    0..page.instances.len() as u32,
                );
                calls += 1;
            }
            calls
        };
        let mut encoder = device.encoder.borrow_mut();
        let mut calls = encode(&mut encoder, false);
        if !draw.transparent.is_empty() {
            calls += encode(&mut encoder, true);
        }
        let colors = [Some(RenderPassColorAttachment {
            view: device.color,
            resolve_target: None,
            depth_slice: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("Solid window composition"),
            color_attachments: &colors,
            ..Default::default()
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &composite, &[]);
        pass.set_viewport(
            0.0,
            0.0,
            draw.uniforms.viewport[0],
            draw.uniforms.viewport[1],
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            draw.rect.left as u32,
            draw.rect.top as u32,
            (draw.rect.right - draw.rect.left) as u32,
            (draw.rect.bottom - draw.rect.top) as u32,
        );
        pass.draw(0..6, 0..1);
        Ok(calls + 1)
    }
}
