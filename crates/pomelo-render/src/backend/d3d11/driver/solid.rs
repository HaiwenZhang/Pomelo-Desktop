//! Native D3D11 depth, weighted transparency, and window composition.
use super::*;
use crate::backend::{
    solid_cache::upload_memory,
    solid_pipeline::{BufferKind, SolidDraw},
    solid_renderer::{SolidUniforms, hlsl_source},
};
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_LINELIST, ID3DBlob};
use windows::Win32::Graphics::Dxgi::Common::*;

struct ColorTarget {
    view: ID3D11RenderTargetView,
    sampled: ID3D11ShaderResourceView,
}
struct Targets {
    size: [u32; 2],
    color: ColorTarget,
    accum: ColorTarget,
    reveal: ColorTarget,
    depth: ID3D11DepthStencilView,
}
pub(crate) struct SolidDriver {
    vertex: ID3D11VertexShader,
    opaque: ID3D11PixelShader,
    transparent: ID3D11PixelShader,
    composite_vertex: ID3D11VertexShader,
    composite: ID3D11PixelShader,
    edge_vertex: ID3D11VertexShader,
    edge: ID3D11PixelShader,
    input: ID3D11InputLayout,
    uniforms: ID3D11Buffer,
    opaque_blend: ID3D11BlendState,
    transparent_blend: ID3D11BlendState,
    composite_blend: ID3D11BlendState,
    depth_write: ID3D11DepthStencilState,
    depth_read: ID3D11DepthStencilState,
    rasterizer: ID3D11RasterizerState,
    targets: Option<Targets>,
}
impl Device<'_> {
    pub(crate) fn solid_opacity(&self) -> f32 {
        self.opacity
    }
    pub(crate) fn solid_buffer(&self, data: &[u8], kind: BufferKind) -> anyhow::Result<Buffer> {
        if matches!(kind, BufferKind::Material) {
            return self.buffer(data, false);
        }
        ensure!(
            !data.is_empty() && data.len() <= self.buffer_limit(true),
            "SOLID_GPU_BUFFER_LIMIT"
        );
        let mut raw = None;
        // SAFETY: immutable initialization covers the checked allocation; COM output is checked.
        unsafe {
            self.raw.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: data.len() as u32,
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: if matches!(kind, BufferKind::Index) {
                        D3D11_BIND_INDEX_BUFFER.0
                    } else {
                        D3D11_BIND_VERTEX_BUFFER.0
                    } as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: data.as_ptr().cast(),
                    ..Default::default()
                } as *const _),
                Some(&mut raw),
            )?;
        }
        Ok(Buffer {
            raw: raw.context("SOLID_BUFFER")?,
            view: None,
            size: data.len(),
        })
    }
}
fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: the returned slice borrows the compiler blob that owns its bytes.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}
impl SolidDriver {
    pub(crate) fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        let (source, entries) = hlsl_source()?;
        let compile = |index: usize, target| {
            let entry = std::ffi::CString::new(entries[index].as_str())?;
            compile_shader(
                &source,
                s!("pomelo solid"),
                PCSTR(entry.as_ptr().cast()),
                target,
            )
        };
        let vb = compile(0, s!("vs_5_0"))?;
        let ob = compile(1, s!("ps_5_0"))?;
        let tb = compile(2, s!("ps_5_0"))?;
        let cvb = compile(3, s!("vs_5_0"))?;
        let cb = compile(4, s!("ps_5_0"))?;
        let evb = compile(5, s!("vs_5_0"))?;
        let eb = compile(6, s!("ps_5_0"))?;
        let (mut edge_vertex, mut edge) = (None, None);
        let (
            mut vertex,
            mut opaque,
            mut transparent,
            mut composite_vertex,
            mut composite,
            mut input,
            mut uniforms,
        ) = (None, None, None, None, None, None, None);
        let attributes = [
            (0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0),
            (1, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16),
            (2, DXGI_FORMAT_R32G32B32A32_UINT, 0, 32),
            (3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0),
            (4, DXGI_FORMAT_R32G32B32A32_UINT, 1, 16),
        ]
        .map(
            |(location, format, slot, offset)| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("LOC"),
                SemanticIndex: location,
                Format: format,
                InputSlot: slot,
                AlignedByteOffset: offset,
                InputSlotClass: if slot == 0 {
                    D3D11_INPUT_PER_VERTEX_DATA
                } else {
                    D3D11_INPUT_PER_INSTANCE_DATA
                },
                InstanceDataStepRate: u32::from(slot == 1),
            },
        );
        let (mut depth_write, mut depth_read, mut rasterizer) = (None, None, None);
        let (mut opaque_blend, mut transparent_blend, mut composite_blend) = (None, None, None);
        // SAFETY: shader blobs and descriptor slices remain live throughout synchronous COM calls.
        unsafe {
            device
                .raw
                .CreateVertexShader(blob_bytes(&vb), None, Some(&mut vertex))?;
            device
                .raw
                .CreatePixelShader(blob_bytes(&ob), None, Some(&mut opaque))?;
            device
                .raw
                .CreatePixelShader(blob_bytes(&tb), None, Some(&mut transparent))?;
            device
                .raw
                .CreateVertexShader(blob_bytes(&cvb), None, Some(&mut composite_vertex))?;
            device
                .raw
                .CreatePixelShader(blob_bytes(&cb), None, Some(&mut composite))?;
            device
                .raw
                .CreateVertexShader(blob_bytes(&evb), None, Some(&mut edge_vertex))?;
            device
                .raw
                .CreatePixelShader(blob_bytes(&eb), None, Some(&mut edge))?;
            device
                .raw
                .CreateInputLayout(&attributes, blob_bytes(&vb), Some(&mut input))?;
            device.raw.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<SolidUniforms>() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniforms),
            )?;
            let mut depth = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D11_COMPARISON_LESS_EQUAL,
                ..Default::default()
            };
            device
                .raw
                .CreateDepthStencilState(&depth, Some(&mut depth_write))?;
            depth.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
            device
                .raw
                .CreateDepthStencilState(&depth, Some(&mut depth_read))?;
            device.raw.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ScissorEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )?;
            let mut blend = D3D11_BLEND_DESC::default();
            let component = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: false.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_ZERO,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            blend.RenderTarget[0] = component;
            device
                .raw
                .CreateBlendState(&blend, Some(&mut opaque_blend))?;
            blend.IndependentBlendEnable = true.into();
            blend.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                DestBlend: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ONE,
                ..component
            };
            blend.RenderTarget[1] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_ZERO,
                DestBlend: D3D11_BLEND_INV_SRC_COLOR,
                ..component
            };
            device
                .raw
                .CreateBlendState(&blend, Some(&mut transparent_blend))?;
            blend.IndependentBlendEnable = false.into();
            blend.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                ..component
            };
            device
                .raw
                .CreateBlendState(&blend, Some(&mut composite_blend))?;
        }
        Ok(Self {
            vertex: vertex.context("SOLID_VERTEX")?,
            opaque: opaque.context("SOLID_OPAQUE")?,
            transparent: transparent.context("SOLID_TRANSPARENT")?,
            composite_vertex: composite_vertex.context("SOLID_COMPOSITE_VERTEX")?,
            composite: composite.context("SOLID_COMPOSITE")?,
            edge_vertex: edge_vertex.context("SOLID_EDGE_VERTEX")?,
            edge: edge.context("SOLID_EDGE")?,
            input: input.context("SOLID_INPUT")?,
            uniforms: uniforms.context("SOLID_UNIFORMS")?,
            depth_write: depth_write.context("SOLID_DEPTH")?,
            depth_read: depth_read.context("SOLID_DEPTH")?,
            rasterizer: rasterizer.context("SOLID_RASTERIZER")?,
            opaque_blend: opaque_blend.context("SOLID_BLEND")?,
            transparent_blend: transparent_blend.context("SOLID_BLEND")?,
            composite_blend: composite_blend.context("SOLID_BLEND")?,
            targets: None,
        })
    }
    fn ensure_targets(&mut self, device: &Device<'_>, size: [u32; 2]) -> anyhow::Result<()> {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return Ok(());
        }
        let bytes = u64::from(size[0]) * u64::from(size[1]) * 22;
        ensure!(
            size.into_iter().all(|v| v > 0 && v <= 16384) && bytes <= 512 * 1024 * 1024,
            "SOLID_TARGET_MEMORY_LIMIT"
        );
        let _memory = upload_memory(bytes as usize)?;
        let texture = |format, flags| -> anyhow::Result<ID3D11Texture2D> {
            let mut texture = None;
            // SAFETY: dimensions and budget are checked; attachment output is checked before use.
            unsafe {
                device.raw.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: size[0],
                        Height: size[1],
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: format,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: flags,
                        ..Default::default()
                    },
                    None,
                    Some(&mut texture),
                )?;
            }
            texture.context("SOLID_TEXTURE")
        };
        let color = |format| -> anyhow::Result<ColorTarget> {
            let texture = texture(
                format,
                (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            )?;
            let (mut view, mut sampled) = (None, None);
            // SAFETY: default views match the typed same-device texture.
            unsafe {
                device
                    .raw
                    .CreateRenderTargetView(&texture, None, Some(&mut view))?;
                device
                    .raw
                    .CreateShaderResourceView(&texture, None, Some(&mut sampled))?;
            }
            Ok(ColorTarget {
                view: view.context("SOLID_TARGET_VIEW")?,
                sampled: sampled.context("SOLID_SAMPLED_VIEW")?,
            })
        };
        let depth_texture = texture(DXGI_FORMAT_D32_FLOAT, D3D11_BIND_DEPTH_STENCIL.0 as u32)?;
        let mut depth = None;
        // SAFETY: typed depth view matches the same-device single-sample texture.
        unsafe {
            device
                .raw
                .CreateDepthStencilView(&depth_texture, None, Some(&mut depth))?;
        }
        self.targets = Some(Targets {
            size,
            color: color(DXGI_FORMAT_R16G16B16A16_FLOAT)?,
            accum: color(DXGI_FORMAT_R16G16B16A16_FLOAT)?,
            reveal: color(DXGI_FORMAT_R16_FLOAT)?,
            depth: depth.context("SOLID_DEPTH_TARGET")?,
        });
        Ok(())
    }
    pub(crate) fn draw(&mut self, device: &Device<'_>, draw: SolidDraw<'_>) -> anyhow::Result<u64> {
        self.ensure_targets(device, draw.size)?;
        let t = self.targets.as_ref().context("SOLID_TARGETS")?;
        let previous = device.capture_output();
        let ctx = device.context;
        let mut viewports = [D3D11_VIEWPORT::default(); 16];
        let mut count = viewports.len() as u32;
        // SAFETY: context belongs to the current paint callback; descriptors, buffers and views are live.
        // GPUI isolates native callbacks and restores the remaining shader/input/rasterizer state.
        unsafe {
            ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
            ctx.PSSetShaderResources(3, Some(&[None, None, None, None]));
            ctx.UpdateSubresource(
                &self.uniforms,
                0,
                None,
                bytemuck::bytes_of(&draw.uniforms).as_ptr().cast(),
                0,
                0,
            );
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            ctx.PSSetShaderResources(3, Some(std::slice::from_ref(&draw.material.view)));
            ctx.RSSetState(&self.rasterizer);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: draw.size[0] as f32,
                Height: draw.size[1] as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            ctx.RSSetScissorRects(Some(&[RECT {
                left: 0,
                top: 0,
                right: draw.size[0] as i32,
                bottom: draw.size[1] as i32,
            }]));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(&self.input);
            ctx.VSSetShader(&self.vertex, None);
            ctx.ClearRenderTargetView(&t.color.view, &[0.0; 4]);
            ctx.ClearDepthStencilView(&t.depth, D3D11_CLEAR_DEPTH.0, 1.0, 0);
            ctx.OMSetRenderTargets(Some(&[Some(t.color.view.clone())]), &t.depth);
            ctx.OMSetDepthStencilState(&self.depth_write, 0);
            ctx.OMSetBlendState(&self.opaque_blend, None, u32::MAX);
            ctx.PSSetShader(&self.opaque, None);
        }
        let geometry = |batches: &[usize], edges: bool| {
            let mut calls = 0;
            for &id in batches {
                if !draw.visible[id] {
                    continue;
                }
                let page = &draw.scene.pages[id];
                let (Some(mesh), Some(instances)) = (&draw.meshes[page.mesh], &draw.instances[id])
                else {
                    continue;
                };
                let buffers = [Some(mesh.vertices.raw.clone()), Some(instances.raw.clone())];
                let strides = [48u32, 32];
                let offsets = [0u32; 2];
                // SAFETY: mesh/page sizes are checked during upload, layouts match Vertex3d/Instance3d.
                unsafe {
                    ctx.IASetVertexBuffers(
                        0,
                        2,
                        Some(buffers.as_ptr()),
                        Some(strides.as_ptr()),
                        Some(offsets.as_ptr()),
                    );
                    ctx.IASetIndexBuffer(&mesh.indices.raw, DXGI_FORMAT_R32_UINT, 0);
                    let indices = if edges {
                        draw.scene.meshes[page.mesh].edges.clone()
                    } else {
                        draw.scene.meshes[page.mesh].draw_indices(draw.uniforms.parameters[3])
                    };
                    ctx.DrawIndexedInstanced(
                        indices.end - indices.start,
                        page.instances.len() as u32,
                        indices.start,
                        0,
                        0,
                    );
                }
                calls += 1;
            }
            calls
        };
        let mut calls = geometry(draw.opaque, false);
        if !draw.edges.is_empty() {
            // SAFETY: same targets and input layout as the opaque pass; lines only test depth.
            unsafe {
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_LINELIST);
                ctx.VSSetShader(&self.edge_vertex, None);
                ctx.PSSetShader(&self.edge, None);
                ctx.OMSetDepthStencilState(&self.depth_read, 0);
            }
            calls += geometry(draw.edges, true);
            // SAFETY: restore the triangle state for the transparent pass.
            unsafe {
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                ctx.VSSetShader(&self.vertex, None);
            }
        }
        // SAFETY: transparent MRTs share the opaque depth size; composition unbinds MRTs before sampling.
        unsafe {
            if !draw.transparent.is_empty() {
                ctx.ClearRenderTargetView(&t.accum.view, &[0.0; 4]);
                ctx.ClearRenderTargetView(&t.reveal.view, &[1.0; 4]);
                ctx.OMSetRenderTargets(
                    Some(&[Some(t.accum.view.clone()), Some(t.reveal.view.clone())]),
                    &t.depth,
                );
                ctx.OMSetDepthStencilState(&self.depth_read, 0);
                ctx.OMSetBlendState(&self.transparent_blend, None, u32::MAX);
                ctx.PSSetShader(&self.transparent, None);
                calls += geometry(draw.transparent, false);
            }
            ctx.OMSetRenderTargets(Some(&previous.targets), None);
            ctx.OMSetDepthStencilState(None, 0);
            ctx.OMSetBlendState(&self.composite_blend, None, u32::MAX);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: draw.uniforms.viewport[0],
                Height: draw.uniforms.viewport[1],
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            ctx.RSSetScissorRects(Some(&[RECT {
                left: draw.rect.left,
                top: draw.rect.top,
                right: draw.rect.right,
                bottom: draw.rect.bottom,
            }]));
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&self.composite_vertex, None);
            ctx.PSSetShader(&self.composite, None);
            ctx.PSSetShaderResources(
                4,
                Some(&[
                    Some(t.color.sampled.clone()),
                    Some(t.accum.sampled.clone()),
                    Some(t.reveal.sampled.clone()),
                ]),
            );
            ctx.Draw(6, 0);
            ctx.PSSetShaderResources(3, Some(&[None, None, None, None]));
            device.restore_output(&previous);
            ctx.RSSetViewports(Some(&viewports[..count as usize]));
            device.raw.GetDeviceRemovedReason()?;
        }
        Ok(calls + 1)
    }
}
