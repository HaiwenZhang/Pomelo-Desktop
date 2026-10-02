//! Audited D3D11 boundary: staged uploads and two-bit stencil coverage union.

use super::{TraceFrame, pipeline::compile_shader};
use crate::{
    copper::{CopperVertex, PreparedCopper},
    split_position,
};
use anyhow::{Context as _, ensure};
use gpui::NativeGpuContext;
use std::sync::Arc;
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
                DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R32_UINT, DXGI_FORMAT_UNKNOWN,
                DXGI_SAMPLE_DESC,
            },
        },
    },
    core::s,
};

pub(super) const UPLOAD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copper_mesh_and_stencil_clear_shaders_compile() {
        let source = include_str!("../../shaders/copper.hlsl");
        compile_shader(source, s!("copper.hlsl"), s!("copper_vertex"), s!("vs_5_0")).unwrap();
        compile_shader(source, s!("copper.hlsl"), s!("clear_vertex"), s!("vs_5_0")).unwrap();
        compile_shader(
            source,
            s!("copper.hlsl"),
            s!("copper_fragment"),
            s!("ps_5_0"),
        )
        .unwrap();
    }
}

pub(super) struct UploadedCopper {
    pub source: Arc<PreparedCopper>,
    vertex_buffer: Option<ID3D11Buffer>,
    vertex_view: Option<ID3D11ShaderResourceView>,
    index_buffer: Option<ID3D11Buffer>,
    vertex_uploaded: usize,
    index_uploaded: usize,
}

fn buffer(
    device: &ID3D11Device,
    count: usize,
    stride: usize,
    structured: bool,
) -> anyhow::Result<Option<ID3D11Buffer>> {
    if count == 0 {
        return Ok(None);
    }
    let bytes = count
        .checked_mul(stride)
        .context("GPU_COPPER_BUFFER_SIZE")?;
    let mut output = None;
    // SAFETY: initialized descriptor, checked 32-bit size; no initial source pointer.
    unsafe {
        device.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: u32::try_from(bytes)?,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: if structured {
                    D3D11_BIND_SHADER_RESOURCE.0
                } else {
                    D3D11_BIND_INDEX_BUFFER.0
                } as u32,
                MiscFlags: if structured {
                    D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
                } else {
                    0
                },
                StructureByteStride: if structured { stride as u32 } else { 0 },
                ..Default::default()
            },
            None,
            Some(&mut output),
        )?;
    }
    Ok(Some(output.context("GPU_COPPER_BUFFER_MISSING")?))
}

impl UploadedCopper {
    pub fn new(source: Arc<PreparedCopper>, device: &ID3D11Device) -> anyhow::Result<Self> {
        let vertex_buffer = buffer(
            device,
            source.vertices.len(),
            size_of::<CopperVertex>(),
            true,
        )?;
        let index_buffer = buffer(device, source.indices.len(), size_of::<u32>(), false)?;
        let mut vertex_view = None;
        if let Some(buffer) = &vertex_buffer {
            // SAFETY: structured view spans the checked live buffer, on the same device.
            unsafe {
                device.CreateShaderResourceView(
                    buffer,
                    Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_UNKNOWN,
                        ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
                        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                            Buffer: D3D11_BUFFER_SRV {
                                Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                                Anonymous2: D3D11_BUFFER_SRV_1 {
                                    NumElements: u32::try_from(source.vertices.len())?,
                                },
                            },
                        },
                    }),
                    Some(&mut vertex_view),
                )?;
            }
            ensure!(vertex_view.is_some(), "GPU_COPPER_SRV_MISSING");
        }
        Ok(Self {
            source,
            vertex_buffer,
            vertex_view,
            index_buffer,
            vertex_uploaded: 0,
            index_uploaded: 0,
        })
    }
    pub fn uploaded_bytes(&self) -> usize {
        self.vertex_uploaded * size_of::<CopperVertex>() + self.index_uploaded * size_of::<u32>()
    }
    pub fn upload_next(&mut self, context: &NativeGpuContext<'_>) -> anyhow::Result<()> {
        let mut budget = UPLOAD_BYTES_PER_FRAME;
        if let Some(buffer) = &self.vertex_buffer {
            upload_slice(
                context.context,
                buffer,
                &self.source.vertices,
                &mut self.vertex_uploaded,
                &mut budget,
            );
        }
        if let Some(buffer) = &self.index_buffer {
            upload_slice(
                context.context,
                buffer,
                &self.source.indices,
                &mut self.index_uploaded,
                &mut budget,
            );
        }
        // SAFETY: status queried on the callback's live device, without retaining it.
        unsafe {
            context.device.GetDeviceRemovedReason()?;
        }
        Ok(())
    }
}

fn upload_slice<T: Copy>(
    context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    source: &[T],
    uploaded: &mut usize,
    budget: &mut usize,
) {
    // Private callers use only the padding-free CopperVertex and u32 ABIs.
    let count = (*budget / size_of::<T>()).min(source.len() - *uploaded);
    if count == 0 {
        return;
    }
    let bytes = count * size_of::<T>();
    let start = *uploaded * size_of::<T>();
    let range = D3D11_BOX {
        left: start as u32,
        right: (start + bytes) as u32,
        top: 0,
        bottom: 1,
        front: 0,
        back: 1,
    };
    // SAFETY: source range and byte box fit the buffer created from the same slice length.
    // UpdateSubresource snapshots these bytes before returning; the Arc remains alive.
    unsafe {
        context.UpdateSubresource(
            buffer,
            0,
            Some(&range),
            source[*uploaded..].as_ptr().cast(),
            0,
            0,
        );
    }
    *uploaded += count;
    *budget -= bytes;
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Uniforms {
    viewport: [f32; 4],
    canvas: [f32; 4],
    clip: [f32; 4],
    camera: [f32; 4],
    view: [f32; 4],
    color: [f32; 4],
    rectangle: [f32; 4],
}
const _: () = assert!(size_of::<Uniforms>() == 112);

struct StencilTarget {
    width: u32,
    height: u32,
    view: ID3D11DepthStencilView,
}

pub(super) struct Pipeline {
    vertex: ID3D11VertexShader,
    clear_vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    uniforms: ID3D11Buffer,
    blend: ID3D11BlendState,
    no_color: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    exterior: ID3D11DepthStencilState,
    holes: ID3D11DepthStencilState,
    shade: ID3D11DepthStencilState,
    clear: ID3D11DepthStencilState,
    stencil: Option<StencilTarget>,
}

fn stencil_state(
    device: &ID3D11Device,
    mask: u8,
    compare: D3D11_COMPARISON_FUNC,
    op: D3D11_STENCIL_OP,
) -> anyhow::Result<ID3D11DepthStencilState> {
    let face = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: op,
        StencilFunc: compare,
    };
    let mut state = None;
    // SAFETY: initialized state applies identical semantics to both triangle windings.
    unsafe {
        device.CreateDepthStencilState(
            &D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: false.into(),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                DepthFunc: D3D11_COMPARISON_ALWAYS,
                StencilEnable: true.into(),
                StencilReadMask: 3,
                StencilWriteMask: mask,
                FrontFace: face,
                BackFace: face,
            },
            Some(&mut state),
        )?;
    }
    state.context("GPU_COPPER_STENCIL_STATE_MISSING")
}

impl Pipeline {
    pub fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        let source = include_str!("../../shaders/copper.hlsl");
        let vb = compile_shader(source, s!("copper.hlsl"), s!("copper_vertex"), s!("vs_5_0"))?;
        let cb = compile_shader(source, s!("copper.hlsl"), s!("clear_vertex"), s!("vs_5_0"))?;
        let pb = compile_shader(
            source,
            s!("copper.hlsl"),
            s!("copper_fragment"),
            s!("ps_5_0"),
        )?;
        let (
            mut vertex,
            mut clear_vertex,
            mut fragment,
            mut uniforms,
            mut blend,
            mut no_color,
            mut rasterizer,
        ) = (None, None, None, None, None, None, None);
        // SAFETY: live compiler blobs, initialized descriptors, checked COM outputs.
        unsafe {
            device.CreateVertexShader(
                std::slice::from_raw_parts(vb.GetBufferPointer().cast(), vb.GetBufferSize()),
                None,
                Some(&mut vertex),
            )?;
            device.CreateVertexShader(
                std::slice::from_raw_parts(cb.GetBufferPointer().cast(), cb.GetBufferSize()),
                None,
                Some(&mut clear_vertex),
            )?;
            device.CreatePixelShader(
                std::slice::from_raw_parts(pb.GetBufferPointer().cast(), pb.GetBufferSize()),
                None,
                Some(&mut fragment),
            )?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<Uniforms>() as u32,
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
            device.CreateBlendState(&desc, Some(&mut blend))?;
            desc.RenderTarget[0].RenderTargetWriteMask = 0;
            device.CreateBlendState(&desc, Some(&mut no_color))?;
            device.CreateRasterizerState(
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
        Ok(Self {
            vertex: vertex.context("GPU_COPPER_VERTEX_MISSING")?,
            clear_vertex: clear_vertex.context("GPU_COPPER_CLEAR_VERTEX_MISSING")?,
            fragment: fragment.context("GPU_COPPER_FRAGMENT_MISSING")?,
            uniforms: uniforms.context("GPU_COPPER_UNIFORMS_MISSING")?,
            blend: blend.context("GPU_COPPER_BLEND_MISSING")?,
            no_color: no_color.context("GPU_COPPER_MASK_BLEND_MISSING")?,
            rasterizer: rasterizer.context("GPU_COPPER_RASTERIZER_MISSING")?,
            exterior: stencil_state(device, 1, D3D11_COMPARISON_ALWAYS, D3D11_STENCIL_OP_REPLACE)?,
            holes: stencil_state(device, 2, D3D11_COMPARISON_ALWAYS, D3D11_STENCIL_OP_REPLACE)?,
            shade: stencil_state(device, 0, D3D11_COMPARISON_EQUAL, D3D11_STENCIL_OP_KEEP)?,
            clear: stencil_state(device, 3, D3D11_COMPARISON_ALWAYS, D3D11_STENCIL_OP_ZERO)?,
            stencil: None,
        })
    }

    fn stencil(&mut self, context: &NativeGpuContext<'_>) -> anyhow::Result<()> {
        let [width, height] = context.viewport;
        ensure!(
            width.is_finite()
                && height.is_finite()
                && width > 0.0
                && height > 0.0
                && width <= 16384.0
                && height <= 16384.0,
            "GPU_COPPER_TARGET_SIZE"
        );
        let (width, height) = (width.ceil() as u32, height.ceil() as u32);
        if self
            .stencil
            .as_ref()
            .is_some_and(|target| target.width == width && target.height == height)
        {
            return Ok(());
        }
        // Limit scratch separately from prepared buffers: 4 bytes per physical target pixel.
        ensure!(
            u64::from(width) * u64::from(height) * 4 <= 256 * 1024 * 1024,
            "GPU_COPPER_STENCIL_BUDGET"
        );
        let (mut texture, mut view) = (None, None);
        // SAFETY: same-device single-sample scratch. It never replaces/owns GPUI's color target.
        unsafe {
            context.device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )?;
            context.device.CreateDepthStencilView(
                texture
                    .as_ref()
                    .context("GPU_COPPER_STENCIL_TEXTURE_MISSING")?,
                None,
                Some(&mut view),
            )?;
        }
        self.stencil = Some(StencilTarget {
            width,
            height,
            view: view.context("GPU_COPPER_STENCIL_VIEW_MISSING")?,
        });
        Ok(())
    }

    pub fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedCopper,
        opacity: f32,
        layer: Option<pomelo_core::model::LayerId>,
    ) -> anyhow::Result<(u64, u64)> {
        if cache.index_uploaded == 0 {
            return Ok((0, 0));
        }
        let bounds = context.bounds;
        let (width, height) = (bounds.size.width.0, bounds.size.height.0);
        if width <= 0.0 || height <= 0.0 {
            return Ok((0, 0));
        }
        let mut camera = frame.camera.unwrap_or_default();
        if frame.camera.is_some() {
            camera.pixels_per_mm *= f64::from(frame.scale_factor);
        } else {
            ensure!(
                camera.fit(
                    frame.bounds,
                    f64::from(width),
                    f64::from(height),
                    f64::from(width.min(height)) * 0.04
                ),
                "GPU_COPPER_CAMERA_FIT"
            );
        }
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_COPPER_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, y, dx, dy].into_iter().all(f32::is_finite)
                && opacity.is_finite()
                && (0.0..=1.0).contains(&opacity),
            "GPU_COPPER_FRAME_INVALID"
        );
        self.stencil(context)?;
        let stencil = &self
            .stencil
            .as_ref()
            .context("GPU_COPPER_STENCIL_MISSING")?
            .view;
        let clip = bounds.intersect(&context.content_mask.bounds);
        let mut uniforms = Uniforms {
            viewport: [context.viewport[0], context.viewport[1], 0.0, 0.0],
            canvas: [bounds.origin.x.0, bounds.origin.y.0, width, height],
            clip: [
                clip.origin.x.0,
                clip.origin.y.0,
                clip.size.width.0,
                clip.size.height.0,
            ],
            camera: [x, y, dx, dy],
            view: [
                camera.pixels_per_mm as f32,
                if camera.flipped { -1.0 } else { 1.0 },
                0.0,
                0.0,
            ],
            color: frame.fallback_color,
            rectangle: [0.0; 4],
        };
        let ctx = context.context;
        let (mut targets, mut old_target, mut old_state, mut old_ref) = ([None], None, None, 0);
        let (mut draws, mut zones) = (0, 0);
        // SAFETY: synchronous use of borrowed immediate context. All buffers are ready before
        // drawing. Local OM state is restored before the trace subrenderer; GPUI restores all
        // outer callback state. Coverage writes never modify the borrowed color target.
        unsafe {
            ctx.OMGetRenderTargets(Some(&mut targets), Some(&mut old_target));
            ensure!(targets[0].is_some(), "GPU_COPPER_COLOR_TARGET_MISSING");
            ctx.OMGetDepthStencilState(Some(&mut old_state), Some(&mut old_ref));
            ctx.OMSetRenderTargets(Some(&targets), Some(stencil));
            ctx.ClearDepthStencilView(stencil, D3D11_CLEAR_STENCIL.0, 1.0, 0);
            ctx.IASetInputLayout(None);
            ctx.IASetIndexBuffer(cache.index_buffer.as_ref(), DXGI_FORMAT_R32_UINT, 0);
            ctx.PSSetShader(&self.fragment, None);
            ctx.RSSetState(&self.rasterizer);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            ctx.VSSetShaderResources(0, Some(std::slice::from_ref(&cache.vertex_view)));
            for batch in &cache.source.batches {
                if layer.is_some_and(|layer| batch.layer != layer) {
                    continue;
                }
                let outer = batch.outer_indices();
                let holes = batch.hole_indices();
                if outer.is_empty()
                    || holes.end as usize > cache.index_uploaded
                    || (batch.vertex_start as usize + batch.vertex_count as usize)
                        > cache.vertex_uploaded
                {
                    continue;
                }
                let Some(zone_bounds) = batch.bounds else {
                    continue;
                };
                let a = camera.board_to_view(zone_bounds.min, f64::from(width), f64::from(height));
                let b = camera.board_to_view(zone_bounds.max, f64::from(width), f64::from(height));
                let left = (a.x.min(b.x) + f64::from(bounds.origin.x.0))
                    .floor()
                    .max(f64::from(clip.origin.x.0))
                    .max(0.0);
                let top = (a.y.min(b.y) + f64::from(bounds.origin.y.0))
                    .floor()
                    .max(f64::from(clip.origin.y.0))
                    .max(0.0);
                let right = (a.x.max(b.x) + f64::from(bounds.origin.x.0))
                    .ceil()
                    .min(f64::from(clip.origin.x.0 + clip.size.width.0))
                    .min(f64::from(context.viewport[0]));
                let bottom = (a.y.max(b.y) + f64::from(bounds.origin.y.0))
                    .ceil()
                    .min(f64::from(clip.origin.y.0 + clip.size.height.0))
                    .min(f64::from(context.viewport[1]));
                if right <= left || bottom <= top {
                    continue;
                }
                let rect = RECT {
                    left: left.floor() as i32,
                    top: top.floor() as i32,
                    right: right.ceil() as i32,
                    bottom: bottom.ceil() as i32,
                };
                uniforms.rectangle = [
                    rect.left as f32,
                    rect.top as f32,
                    rect.right as f32,
                    rect.bottom as f32,
                ];
                uniforms.color = frame
                    .highlighted_net
                    .filter(|(net, _)| net.0 != 0 && *net == batch.net)
                    .map(|(_, color)| color)
                    .unwrap_or_else(|| {
                        frame
                            .colors
                            .get(&batch.layer)
                            .copied()
                            .unwrap_or(frame.fallback_color)
                    });
                if let Some(color) = frame.object_highlight(batch.selected_object, batch.net, None)
                {
                    uniforms.color = color;
                }
                uniforms.color[3] *= opacity;
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                ctx.Map(
                    &self.uniforms,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )?;
                std::ptr::copy_nonoverlapping(
                    (&uniforms as *const Uniforms).cast::<u8>(),
                    mapped.pData.cast::<u8>(),
                    size_of::<Uniforms>(),
                );
                ctx.Unmap(&self.uniforms, 0);
                ctx.RSSetScissorRects(Some(&[rect]));
                ctx.OMSetBlendState(&self.no_color, None, u32::MAX);
                // Clear only this zone's bounded area, avoiding a full target clear per zone.
                ctx.VSSetShader(&self.clear_vertex, None);
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                ctx.OMSetDepthStencilState(&self.clear, 0);
                ctx.Draw(4, 0);
                draws += 1;
                ctx.VSSetShader(&self.vertex, None);
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                ctx.OMSetDepthStencilState(&self.exterior, 1);
                ctx.DrawIndexed(outer.end - outer.start, outer.start, 0);
                draws += 1;
                if !holes.is_empty() {
                    ctx.OMSetDepthStencilState(&self.holes, 2);
                    ctx.DrawIndexed(holes.end - holes.start, holes.start, 0);
                    draws += 1;
                }
                // Both hole-only (2) and exterior+hole (3) fail EQUAL 1. Overlap stays excluded.
                ctx.OMSetBlendState(&self.blend, None, u32::MAX);
                ctx.OMSetDepthStencilState(&self.shade, 1);
                ctx.DrawIndexed(outer.end - outer.start, outer.start, 0);
                draws += 1;
                zones += 1;
            }
            ctx.VSSetShaderResources(0, Some(&[None]));
            ctx.OMSetRenderTargets(Some(&targets), old_target.as_ref());
            ctx.OMSetDepthStencilState(old_state.as_ref(), old_ref);
        }
        Ok((draws, zones))
    }
}
