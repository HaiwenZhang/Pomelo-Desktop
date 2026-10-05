//! Direct3D resources and commands. PCB batching and rendering policy live in common.
use super::super::{
    Shader,
    common::{
        Rect,
        driver::{Draw, StencilMode, StencilScope},
    },
};
use super::shaders::{compile_shader, source};
use anyhow::{Context as _, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
                D3D11_SRV_DIMENSION_BUFFER,
            },
            Direct3D11::*,
            Dxgi::Common::{
                DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R32_UINT,
                DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
            },
        },
    },
    core::{Interface, s},
};

pub(crate) struct Buffer {
    raw: ID3D11Buffer,
    view: Option<ID3D11ShaderResourceView>,
    size: usize,
}
pub(crate) struct Atlas {
    view: ID3D11ShaderResourceView,
    sampler: ID3D11SamplerState,
}
pub(crate) struct Pipeline {
    vertex: ID3D11VertexShader,
    clear_vertex: Option<ID3D11VertexShader>,
    fragment: ID3D11PixelShader,
    uniforms: ID3D11Buffer,
    uniform_size: usize,
    blend: ID3D11BlendState,
    no_color: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    states: BTreeMap<(StencilMode, bool), ID3D11DepthStencilState>,
}
struct StencilTarget {
    size: [u32; 2],
    view: ID3D11DepthStencilView,
}
struct OutputState {
    targets: [Option<ID3D11RenderTargetView>; 1],
    depth: Option<ID3D11DepthStencilView>,
    state: Option<ID3D11DepthStencilState>,
    reference: u32,
}
type BindingKey = (usize, bool, [i32; 4], StencilMode, bool, usize, usize);
struct DrawState {
    output: OutputState,
    bindings: Option<BindingKey>,
    vertices: Option<usize>,
}
pub(crate) struct DrawScope<'d, 'a> {
    device: &'d Device<'a>,
    owns: bool,
}
impl Drop for DrawScope<'_, '_> {
    fn drop(&mut self) {
        if self.owns {
            self.device.batched.set(false);
            if let Some(state) = self.device.draw_state.borrow_mut().take() {
                self.device.restore_output(&state.output);
            }
        }
    }
}
pub(crate) struct Device<'a> {
    raw: &'a ID3D11Device,
    pub(crate) context: &'a ID3D11DeviceContext,
    opacity: f32,
    sample_count: u32,
    stencil: RefCell<Option<StencilTarget>>,
    active: Cell<bool>,
    batched: Cell<bool>,
    draw_state: RefCell<Option<DrawState>>,
}
impl<'a> Device<'a> {
    pub(crate) fn new(
        raw: &'a ID3D11Device,
        context: &'a ID3D11DeviceContext,
        opacity: f32,
    ) -> Self {
        Self {
            raw,
            context,
            opacity,
            sample_count: 1,
            stencil: RefCell::new(None),
            active: Cell::new(false),
            batched: Cell::new(false),
            draw_state: RefCell::new(None),
        }
    }
    pub(super) fn from_paint(context: &gpui_windows::D3D11PaintContext<'a>) -> Self {
        let mut result = Self::new(context.device, context.context, context.target.opacity);
        result.sample_count = context.target.sample_count;
        result
    }
    pub fn structured_buffer(&self, data: &[u8], stride: usize) -> anyhow::Result<Buffer> {
        self.create_buffer(data.len(), Some(data), false, stride)
    }
    pub fn buffer(&self, data: &[u8], index: bool) -> anyhow::Result<Buffer> {
        self.create_buffer(data.len(), Some(data), index, 16)
    }
    pub fn buffer_empty(&self, size: usize, index: bool) -> anyhow::Result<Buffer> {
        self.create_buffer(size, None, index, 16)
    }
    fn create_buffer(
        &self,
        size: usize,
        data: Option<&[u8]>,
        index: bool,
        stride: usize,
    ) -> anyhow::Result<Buffer> {
        ensure!(size > 0 && size <= u32::MAX as usize, "GPU_BUFFER_SIZE");
        ensure!(
            index || (stride > 0 && size.is_multiple_of(stride)),
            "GPU_BUFFER_STRIDE"
        );
        let initial = data.map(|bytes| D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr().cast(),
            ..Default::default()
        });
        let mut raw = None;
        let mut view = None;
        // SAFETY: initialization bytes cover the allocation; descriptors and COM outputs are checked.
        unsafe {
            self.raw.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size as u32,
                    Usage: if data.is_some() {
                        D3D11_USAGE_IMMUTABLE
                    } else {
                        D3D11_USAGE_DEFAULT
                    },
                    BindFlags: if index {
                        D3D11_BIND_INDEX_BUFFER.0 as u32
                    } else {
                        D3D11_BIND_SHADER_RESOURCE.0 as u32
                    },
                    MiscFlags: if index {
                        0
                    } else {
                        D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
                    },
                    StructureByteStride: if index { 0 } else { stride as u32 },
                    ..Default::default()
                },
                initial.as_ref().map(|value| value as *const _),
                Some(&mut raw),
            )?;
            let raw = raw.context("GPU_BUFFER_MISSING")?;
            if !index {
                self.raw.CreateShaderResourceView(
                    &raw,
                    Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_UNKNOWN,
                        ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
                        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                            Buffer: D3D11_BUFFER_SRV {
                                Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                                Anonymous2: D3D11_BUFFER_SRV_1 {
                                    NumElements: (size / stride) as u32,
                                },
                            },
                        },
                    }),
                    Some(&mut view),
                )?;
            }
            Ok(Buffer { raw, view, size })
        }
    }
    pub fn write_buffer(&self, buffer: &Buffer, offset: usize, data: &[u8]) -> anyhow::Result<()> {
        ensure!(
            offset
                .checked_add(data.len())
                .is_some_and(|end| end <= buffer.size),
            "GPU_UPLOAD_RANGE"
        );
        let range = D3D11_BOX {
            left: offset as u32,
            right: (offset + data.len()) as u32,
            top: 0,
            bottom: 1,
            front: 0,
            back: 1,
        };
        // SAFETY: checked source and destination range; the immediate context snapshots upload bytes.
        unsafe {
            self.context.UpdateSubresource(
                &buffer.raw,
                0,
                Some(&range),
                data.as_ptr().cast(),
                0,
                0,
            );
            self.raw.GetDeviceRemovedReason()?;
        }
        Ok(())
    }
    pub fn atlas(&self, width: u32, height: u32, data: &[u8]) -> anyhow::Result<Atlas> {
        ensure!(
            data.len() == width as usize * height as usize * 4,
            "GPU_ATLAS_SIZE"
        );
        let (mut texture, mut view, mut sampler) = (None, None, None);
        // SAFETY: decoded RGBA bytes cover the immutable image; all output handles are checked.
        unsafe {
            self.raw.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: data.as_ptr().cast(),
                    SysMemPitch: width * 4,
                    ..Default::default()
                }),
                Some(&mut texture),
            )?;
            self.raw.CreateShaderResourceView(
                texture.as_ref().context("GPU_ATLAS_TEXTURE_MISSING")?,
                None,
                Some(&mut view),
            )?;
            self.raw.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: 0.0,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
        }
        Ok(Atlas {
            view: view.context("GPU_ATLAS_VIEW_MISSING")?,
            sampler: sampler.context("GPU_ATLAS_SAMPLER_MISSING")?,
        })
    }
    pub fn pipeline(&self, shader: Shader) -> anyhow::Result<Pipeline> {
        let (source, vertex_entry, fragment_entry) = source(shader);
        let vertex_blob = compile_shader(&source, s!("pomelo-render"), vertex_entry, s!("vs_5_0"))?;
        let fragment_blob =
            compile_shader(&source, s!("pomelo-render"), fragment_entry, s!("ps_5_0"))?;
        let (
            mut vertex,
            mut fragment,
            mut clear_vertex,
            mut uniforms,
            mut blend,
            mut no_color,
            mut rasterizer,
        ) = (None, None, None, None, None, None, None);
        let uniform_size = match shader {
            Shader::Copper => 192,
            Shader::Probe => 128,
            _ => 144,
        };
        // SAFETY: live compiler blobs and initialized descriptors; validated COM outputs stay on this device.
        unsafe {
            self.raw.CreateVertexShader(
                std::slice::from_raw_parts(
                    vertex_blob.GetBufferPointer().cast(),
                    vertex_blob.GetBufferSize(),
                ),
                None,
                Some(&mut vertex),
            )?;
            self.raw.CreatePixelShader(
                std::slice::from_raw_parts(
                    fragment_blob.GetBufferPointer().cast(),
                    fragment_blob.GetBufferSize(),
                ),
                None,
                Some(&mut fragment),
            )?;
            if shader == Shader::Copper {
                let blob =
                    compile_shader(&source, s!("copper.hlsl"), s!("clear_vertex"), s!("vs_5_0"))?;
                self.raw.CreateVertexShader(
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast(),
                        blob.GetBufferSize(),
                    ),
                    None,
                    Some(&mut clear_vertex),
                )?;
            }
            self.raw.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: uniform_size as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniforms),
            )?;
            let mut desc = D3D11_BLEND_DESC::default();
            desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            self.raw.CreateBlendState(&desc, Some(&mut blend))?;
            desc.RenderTarget[0].RenderTargetWriteMask = 0;
            self.raw.CreateBlendState(&desc, Some(&mut no_color))?;
            self.raw.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ScissorEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )?;
        }
        let mut states = BTreeMap::new();
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
                let (read, write, _, equal, invert, zero) = mode.state(inherited);
                let face = D3D11_DEPTH_STENCILOP_DESC {
                    StencilFailOp: D3D11_STENCIL_OP_KEEP,
                    StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
                    StencilPassOp: if invert {
                        D3D11_STENCIL_OP_INVERT
                    } else if zero {
                        D3D11_STENCIL_OP_ZERO
                    } else if write != 0 {
                        D3D11_STENCIL_OP_REPLACE
                    } else {
                        D3D11_STENCIL_OP_KEEP
                    },
                    StencilFunc: if equal {
                        D3D11_COMPARISON_EQUAL
                    } else {
                        D3D11_COMPARISON_ALWAYS
                    },
                };
                let mut state = None;
                // SAFETY: common stencil masks fit eight bits and apply identical front/back operations.
                unsafe {
                    self.raw.CreateDepthStencilState(
                        &D3D11_DEPTH_STENCIL_DESC {
                            DepthEnable: false.into(),
                            DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                            DepthFunc: D3D11_COMPARISON_ALWAYS,
                            StencilEnable: true.into(),
                            StencilReadMask: read as u8,
                            StencilWriteMask: write as u8,
                            FrontFace: face,
                            BackFace: face,
                        },
                        Some(&mut state),
                    )?;
                }
                states.insert(
                    (mode, inherited),
                    state.context("GPU_STENCIL_STATE_MISSING")?,
                );
            }
        }
        Ok(Pipeline {
            vertex: vertex.context("GPU_VERTEX_SHADER_MISSING")?,
            clear_vertex,
            fragment: fragment.context("GPU_FRAGMENT_SHADER_MISSING")?,
            uniforms: uniforms.context("GPU_UNIFORM_MISSING")?,
            uniform_size,
            blend: blend.context("GPU_BLEND_MISSING")?,
            no_color: no_color.context("GPU_BLEND_MISSING")?,
            rasterizer: rasterizer.context("GPU_RASTERIZER_MISSING")?,
            states,
        })
    }
    fn ensure_stencil(&self, size: [f32; 2]) -> anyhow::Result<()> {
        ensure!(
            size.into_iter()
                .all(|v| v.is_finite() && v > 0.0 && v <= 16384.0),
            "GPU_STENCIL_TARGET_SIZE"
        );
        let size = size.map(|v| v.ceil() as u32);
        ensure!(
            u64::from(size[0]) * u64::from(size[1]) * 4 <= 256 * 1024 * 1024,
            "GPU_STENCIL_BUDGET"
        );
        if self
            .stencil
            .borrow()
            .as_ref()
            .is_some_and(|s| s.size == size)
        {
            return Ok(());
        }
        let (mut texture, mut view) = (None, None);
        // SAFETY: private same-device attachment matching the frame's dimensions and samples.
        unsafe {
            self.raw.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: size[0],
                    Height: size[1],
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: self.sample_count,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )?;
            self.raw.CreateDepthStencilView(
                texture.as_ref().context("GPU_STENCIL_TEXTURE_MISSING")?,
                None,
                Some(&mut view),
            )?;
        }
        *self.stencil.borrow_mut() = Some(StencilTarget {
            size,
            view: view.context("GPU_STENCIL_VIEW_MISSING")?,
        });
        Ok(())
    }
    pub fn stencil_scope(&self, size: [f32; 2]) -> anyhow::Result<StencilScope<'_>> {
        self.ensure_stencil(size)?;
        let previous = self.active.replace(true);
        Ok(StencilScope {
            active: &self.active,
            previous,
        })
    }
    /// Reuse bindings only until this native pipeline invocation returns to its caller.
    /// Callers keep resources alive and issue all context draws through this device in the scope.
    pub(crate) fn draw_scope(&self) -> DrawScope<'_, 'a> {
        DrawScope {
            device: self,
            owns: !self.batched.replace(true),
        }
    }
    fn capture_output(&self) -> OutputState {
        let mut output = OutputState {
            targets: [None],
            depth: None,
            state: None,
            reference: 0,
        };
        // SAFETY: COM outputs retain the borrowed context's current attachments and depth state.
        unsafe {
            self.context
                .OMGetRenderTargets(Some(&mut output.targets), Some(&mut output.depth));
            self.context
                .OMGetDepthStencilState(Some(&mut output.state), Some(&mut output.reference));
        }
        output
    }
    fn restore_output(&self, output: &OutputState) {
        // SAFETY: retained COM resources remain alive through restoration, including error unwinding.
        unsafe {
            self.context.VSSetShaderResources(0, Some(&[None]));
            self.context.PSSetShaderResources(0, Some(&[None]));
            self.context
                .OMSetRenderTargets(Some(&output.targets), output.depth.as_ref());
            self.context
                .OMSetDepthStencilState(output.state.as_ref(), output.reference);
        }
    }
    pub fn draw(&self, pipeline: &Pipeline, draw: Draw<'_>) -> anyhow::Result<()> {
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
        ensure!(
            draw.uniforms.len() + 16 == pipeline.uniform_size,
            "GPU_UNIFORM_SIZE"
        );
        let inherited = self.active.get();
        let stencil = self.stencil.borrow();
        let stencil = if inherited || draw.stencil != StencilMode::Inherited {
            Some(&stencil.as_ref().context("GPU_STENCIL_MISSING")?.view)
        } else {
            None
        };
        let mut state = self.draw_state.borrow_mut();
        if self.batched.get() && state.is_none() {
            *state = Some(DrawState {
                output: self.capture_output(),
                bindings: None,
                vertices: None,
            });
        }
        let direct_output = state.is_none().then(|| self.capture_output());
        let output = state
            .as_ref()
            .map(|state| &state.output)
            .or(direct_output.as_ref())
            .context("GPU_OUTPUT_STATE_MISSING")?;
        let bindings = (
            pipeline.uniforms.as_raw() as usize,
            draw.indices.is_some(),
            [left, top, right, bottom],
            draw.stencil,
            inherited,
            draw.atlas.map_or(0, |atlas| atlas.view.as_raw() as usize),
            draw.atlas
                .map_or(0, |atlas| atlas.sampler.as_raw() as usize),
        );
        let bind = state
            .as_ref()
            .is_none_or(|state| state.bindings != Some(bindings));
        let vertices = draw
            .vertices
            .view
            .as_ref()
            .map_or(0, |view| view.as_raw() as usize);
        let bind_vertices = state
            .as_ref()
            .is_none_or(|state| state.vertices != Some(vertices));
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: synchronous callback on the borrowed immediate context, checked uniform ABI and resources.
        unsafe {
            self.context.Map(
                &pipeline.uniforms,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )?;
            std::ptr::copy_nonoverlapping(
                draw.uniforms.as_ptr(),
                mapped.pData.cast(),
                draw.uniforms.len(),
            );
            std::ptr::copy_nonoverlapping(
                bytemuck::cast_slice::<f32, u8>(&[self.opacity, 0.0, 0.0, 0.0]).as_ptr(),
                mapped.pData.cast::<u8>().add(draw.uniforms.len()),
                16,
            );
            self.context.Unmap(&pipeline.uniforms, 0);
            if bind {
                self.context
                    .OMSetRenderTargets(Some(&output.targets), stencil);
                self.context.OMSetDepthStencilState(
                    &pipeline.states[&(draw.stencil, inherited)],
                    draw.stencil.state(inherited).2,
                );
                self.context.OMSetBlendState(
                    if draw.stencil.color() {
                        &pipeline.blend
                    } else {
                        &pipeline.no_color
                    },
                    None,
                    u32::MAX,
                );
                self.context.RSSetState(&pipeline.rasterizer);
                self.context.RSSetScissorRects(Some(&[RECT {
                    left,
                    top,
                    right,
                    bottom,
                }]));
                self.context.IASetInputLayout(None);
                self.context
                    .IASetPrimitiveTopology(if draw.indices.is_some() {
                        D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
                    } else {
                        D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP
                    });
                self.context.VSSetShader(
                    if draw.indices.is_some() {
                        &pipeline.vertex
                    } else {
                        pipeline.clear_vertex.as_ref().unwrap_or(&pipeline.vertex)
                    },
                    None,
                );
                self.context.PSSetShader(&pipeline.fragment, None);
                self.context
                    .VSSetConstantBuffers(0, Some(&[Some(pipeline.uniforms.clone())]));
                self.context
                    .PSSetConstantBuffers(0, Some(&[Some(pipeline.uniforms.clone())]));
                self.context
                    .PSSetShaderResources(0, Some(&[draw.atlas.map(|atlas| atlas.view.clone())]));
                self.context
                    .PSSetSamplers(0, Some(&[draw.atlas.map(|atlas| atlas.sampler.clone())]));
            }
            if bind_vertices {
                self.context
                    .VSSetShaderResources(0, Some(std::slice::from_ref(&draw.vertices.view)));
            }
            if let Some(indices) = draw.indices {
                self.context
                    .IASetIndexBuffer(&indices.raw, DXGI_FORMAT_R32_UINT, 0);
                self.context.DrawIndexed(draw.count, draw.start, 0);
            } else {
                self.context
                    .DrawInstanced(draw.count, draw.instances, draw.start, 0);
            }
            if let Some(output) = direct_output.as_ref() {
                self.restore_output(output);
            }
            self.raw.GetDeviceRemovedReason()?;
        }
        if let Some(state) = state.as_mut() {
            state.bindings = Some(bindings);
            state.vertices = Some(vertices);
        }
        Ok(())
    }
}
