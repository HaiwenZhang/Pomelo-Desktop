//! D3D11 FFI boundary for persistent, incrementally uploaded pad instances.

use super::{TraceFrame, pipeline::compile_shader};
use crate::{
    pads::{PadInstance, PreparedPads},
    split_position,
};
use anyhow::{Context as _, ensure};
use gpui::NativeGpuContext;
use pomelo_core::{interaction::Camera, model::LayerId};
use std::sync::Arc;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, D3D11_SRV_DIMENSION_BUFFER},
            Direct3D11::*,
            Dxgi::Common::DXGI_FORMAT_UNKNOWN,
        },
    },
    core::s,
};

const CHUNK_INSTANCES: usize = 16_384;
const CHUNKS_PER_FRAME: usize = 2;

struct Chunk {
    start: usize,
    count: usize,
    view: ID3D11ShaderResourceView,
}

pub(super) struct UploadedPads {
    pub source: Arc<PreparedPads>,
    chunks: Vec<Chunk>,
    uploaded: usize,
}

impl UploadedPads {
    pub fn new(source: Arc<PreparedPads>) -> anyhow::Result<Self> {
        ensure!(
            source.analytic.len() <= u32::MAX as usize,
            "GPU_PAD_INSTANCE_LIMIT"
        );
        let mut chunks = Vec::new();
        chunks.try_reserve_exact(source.analytic.len().div_ceil(CHUNK_INSTANCES))?;
        for batch in &source.batches {
            let end = batch
                .start
                .checked_add(batch.count)
                .context("GPU_PAD_BATCH_OVERFLOW")?;
            ensure!(end as usize <= source.analytic.len(), "GPU_PAD_BATCH_RANGE");
        }
        Ok(Self {
            source,
            chunks,
            uploaded: 0,
        })
    }

    pub fn uploaded(&self) -> usize {
        self.uploaded
    }

    pub fn upload_next(&mut self, device: &ID3D11Device) -> anyhow::Result<u64> {
        let mut bytes_uploaded = 0;
        for _ in 0..CHUNKS_PER_FRAME {
            let start = self.uploaded;
            if start == self.source.analytic.len() {
                break;
            }
            let count = CHUNK_INSTANCES.min(self.source.analytic.len() - start);
            let bytes = count * std::mem::size_of::<PadInstance>();
            let mut buffer = None;
            let mut view = None;
            // SAFETY: the padding-free instance slice remains alive through CreateBuffer;
            // D3D11 copies the initialization data. Output COM objects are checked before use.
            unsafe {
                device.CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: bytes as u32,
                        Usage: D3D11_USAGE_IMMUTABLE,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                        StructureByteStride: std::mem::size_of::<PadInstance>() as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: self.source.analytic[start..start + count].as_ptr().cast(),
                        ..Default::default()
                    }),
                    Some(&mut buffer),
                )?;
                let buffer = buffer.context("GPU_PAD_BUFFER_MISSING")?;
                device.CreateShaderResourceView(
                    &buffer,
                    Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_UNKNOWN,
                        ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
                        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                            Buffer: D3D11_BUFFER_SRV {
                                Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                                Anonymous2: D3D11_BUFFER_SRV_1 {
                                    NumElements: count as u32,
                                },
                            },
                        },
                    }),
                    Some(&mut view),
                )?;
            }
            // A shader resource view retains its buffer; no borrowed device/context escapes.
            self.chunks.push(Chunk {
                start,
                count,
                view: view.context("GPU_PAD_SRV_MISSING")?,
            });
            self.uploaded += count;
            bytes_uploaded += bytes as u64;
        }
        Ok(bytes_uploaded)
    }
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
    batch: [u32; 4],
    highlight: [f32; 4],
}
const _: () = assert!(std::mem::size_of::<Uniforms>() == 128);
const _: () = assert!(std::mem::offset_of!(Uniforms, batch) == 96);

pub(super) struct Pipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    uniforms: ID3D11Buffer,
    blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
}

impl Pipeline {
    pub fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        let source = include_str!("../../shaders/pad.hlsl");
        let vertex_blob = compile_shader(
            source,
            s!("pomelo-render/pad.hlsl"),
            s!("pad_vertex"),
            s!("vs_5_0"),
        )?;
        let fragment_blob = compile_shader(
            source,
            s!("pomelo-render/pad.hlsl"),
            s!("pad_fragment"),
            s!("ps_5_0"),
        )?;
        let (mut vertex, mut fragment, mut uniforms, mut blend, mut rasterizer) =
            (None, None, None, None, None);
        // SAFETY: initialized descriptors and live blobs; checked COM outputs. All resources
        // belong to the provided GPUI device and are dropped on renderer reset.
        unsafe {
            device.CreateVertexShader(
                std::slice::from_raw_parts(
                    vertex_blob.GetBufferPointer().cast(),
                    vertex_blob.GetBufferSize(),
                ),
                None,
                Some(&mut vertex),
            )?;
            device.CreatePixelShader(
                std::slice::from_raw_parts(
                    fragment_blob.GetBufferPointer().cast(),
                    fragment_blob.GetBufferSize(),
                ),
                None,
                Some(&mut fragment),
            )?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<Uniforms>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniforms),
            )?;
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            device.CreateBlendState(&blend_desc, Some(&mut blend))?;
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
            vertex: vertex.context("GPU_PAD_VERTEX_MISSING")?,
            fragment: fragment.context("GPU_PAD_FRAGMENT_MISSING")?,
            uniforms: uniforms.context("GPU_PAD_UNIFORMS_MISSING")?,
            blend: blend.context("GPU_PAD_BLEND_MISSING")?,
            rasterizer: rasterizer.context("GPU_PAD_RASTERIZER_MISSING")?,
        })
    }

    pub fn draw(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame,
        cache: &UploadedPads,
        layer: Option<LayerId>,
        visible: Option<&dyn Fn(&PadInstance) -> bool>,
    ) -> anyhow::Result<u64> {
        let bounds = context.bounds;
        let width = bounds.size.width.0;
        let height = bounds.size.height.0;
        if width <= 0.0 || height <= 0.0 {
            return Ok(0);
        }
        let camera = if let Some(mut camera) = frame.camera {
            camera.pixels_per_mm *= f64::from(frame.scale_factor);
            camera
        } else {
            let mut camera = Camera::default();
            ensure!(
                camera.fit(
                    frame.bounds,
                    f64::from(width),
                    f64::from(height),
                    f64::from(width.min(height)) * 0.04
                ),
                "GPU_PAD_CAMERA_FIT"
            );
            camera
        };
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_PAD_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, dx, y, dy].into_iter().all(f32::is_finite),
            "GPU_PAD_CAMERA_CENTER"
        );
        let clip = bounds.intersect(&context.content_mask.bounds);
        let rect = RECT {
            left: clip.origin.x.0.floor().max(0.0) as i32,
            top: clip.origin.y.0.floor().max(0.0) as i32,
            right: (clip.origin.x.0 + clip.size.width.0)
                .ceil()
                .min(context.viewport[0]) as i32,
            bottom: (clip.origin.y.0 + clip.size.height.0)
                .ceil()
                .min(context.viewport[1]) as i32,
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Ok(0);
        }
        let mut uniforms = Uniforms {
            viewport: [
                context.viewport[0],
                context.viewport[1],
                frame.scale_factor,
                f32::from(u8::from(frame.filled)),
            ],
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
                f32::from(u8::from(
                    frame.color_mode == pomelo_core::display::ColorMode::Net,
                )),
                frame.pass as u8 as f32,
            ],
            color: frame.fallback_color,
            batch: [
                0,
                frame.highlighted_net.map_or(0, |(net, _)| net.0),
                u32::from(
                    frame.pass == super::board::OverlayPass::Base
                        && frame.highlighted_net.is_some_and(|(net, _)| net.0 != 0),
                ),
                0,
            ],
            highlight: frame.highlighted_net.map_or([0.0; 4], |(_, color)| color),
        };
        let ctx = context.context;
        let mut draws = 0;
        // SAFETY: synchronous callback on GPUI's immediate context; GPUI's full D3D11.1
        // state swap restores all bindings after this callback. WRITE_DISCARD renames constants,
        // keeping earlier draws from observing a later batch's color/offset.
        unsafe {
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.VSSetShader(&self.vertex, None);
            ctx.PSSetShader(&self.fragment, None);
            ctx.RSSetState(&self.rasterizer);
            ctx.RSSetScissorRects(Some(&[rect]));
            ctx.OMSetBlendState(&self.blend, None, u32::MAX);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            for batch in &cache.source.batches {
                if layer.is_some_and(|layer| batch.layer != layer) {
                    continue;
                }
                uniforms.color = frame
                    .colors
                    .get(&batch.layer)
                    .copied()
                    .unwrap_or(frame.fallback_color);
                for chunk in &cache.chunks {
                    let start = (batch.start as usize).max(chunk.start);
                    let end = ((batch.start + batch.count) as usize).min(chunk.start + chunk.count);
                    if start >= end {
                        continue;
                    }
                    let selected = |index: usize| {
                        let instance = &cache.source.analytic[index];
                        let object = if instance.source[0] == 0 {
                            pomelo_core::selection::SelectedObject::Pin(
                                pomelo_core::model::ObjectId(instance.ids[0]),
                            )
                        } else {
                            pomelo_core::selection::SelectedObject::Via(
                                pomelo_core::model::ObjectId(instance.ids[0]),
                            )
                        };
                        frame.object_highlight(
                            object,
                            pomelo_core::model::NetId(instance.ids[2]),
                            None,
                        )
                    };
                    let mut cursor = start;
                    while cursor < end {
                        while cursor < end
                            && visible
                                .is_some_and(|predicate| !predicate(&cache.source.analytic[cursor]))
                        {
                            cursor += 1;
                        }
                        let start = cursor;
                        let highlighted = (cursor < end).then(|| selected(cursor)).flatten();
                        while cursor < end
                            && selected(cursor) == highlighted
                            && visible
                                .is_none_or(|predicate| predicate(&cache.source.analytic[cursor]))
                        {
                            cursor += 1;
                        }
                        let end = cursor;
                        if start == end {
                            continue;
                        }
                        if frame.pass != super::board::OverlayPass::Base && highlighted.is_none() {
                            continue;
                        }
                        uniforms.batch[3] = u32::from(highlighted.is_some());
                        uniforms.highlight = highlighted.unwrap_or_else(|| {
                            frame.highlighted_net.map_or([0.0; 4], |(_, color)| color)
                        });
                        uniforms.batch[0] = (start - chunk.start) as u32;
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
                            std::mem::size_of::<Uniforms>(),
                        );
                        ctx.Unmap(&self.uniforms, 0);
                        ctx.VSSetShaderResources(0, Some(&[Some(chunk.view.clone())]));
                        // Explicit shader offset avoids relying on StartInstanceLocation for an SRV.
                        ctx.DrawInstanced(4, (end - start) as u32, 0, 0);
                        draws += 1;
                    }
                }
            }
            ctx.VSSetShaderResources(0, Some(&[None]));
        }
        Ok(draws)
    }
}
