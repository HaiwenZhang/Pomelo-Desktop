//! D3D11 FFI boundary for persistent, incrementally uploaded trace instances.

use super::{TraceFrame, pipeline::compile_shader};
use crate::{
    split_position,
    tracks::{PreparedTracks, TraceInstance},
};
use anyhow::{Context as _, ensure};
use gpui::NativeGpuContext;
use pomelo_core::interaction::Camera;
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

// Implementations below expose only padding-free, initialized GPU ABI structs.
pub trait InstanceSource: Send + Sync + 'static {
    const COMPACT_TEXT: bool;
    const MSDF: bool = false;
    type Instance: Send + Sync;
    fn instances(&self) -> &[Self::Instance];
    fn batches(&self) -> &[crate::tracks::TraceBatch];
    fn selection_ids(&self, index: usize) -> [u32; 4];
    fn font(&self) -> Option<&crate::text::msdf::MsdfFont> {
        None
    }
    fn atlas_page(&self, _: usize) -> u16 {
        0
    }
    fn label_category(&self, _: usize) -> u32 {
        u32::MAX
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        pomelo_core::selection::SelectedObject::Segment(pomelo_core::model::ObjectId(
            self.selection_ids(index)[0],
        ))
    }
}
impl InstanceSource for PreparedTracks {
    const COMPACT_TEXT: bool = false;
    type Instance = TraceInstance;
    fn instances(&self) -> &[TraceInstance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        self.instances[index].ids
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        let instance = &self.instances[index];
        let id = pomelo_core::model::ObjectId(instance.ids[0]);
        if instance.flags[3] & 64 != 0 {
            pomelo_core::selection::SelectedObject::Drawing(id)
        } else if instance.flags[3] & 8 != 0 {
            pomelo_core::selection::SelectedObject::Zone(id)
        } else if instance.flags[3] & 16 != 0 {
            pomelo_core::selection::SelectedObject::Pin(id)
        } else if instance.flags[3] & 32 != 0 {
            pomelo_core::selection::SelectedObject::Via(id)
        } else {
            pomelo_core::selection::SelectedObject::Segment(id)
        }
    }
}
impl InstanceSource for crate::text_instances::PreparedTextInstances {
    const COMPACT_TEXT: bool = true;
    type Instance = crate::text_instances::TextInstance;
    fn instances(&self) -> &[Self::Instance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        [
            if self.instances[index].flags[1] == 1 {
                self.instances[index].flags[0]
            } else {
                self.instances[index].ids[0]
            },
            0,
            self.instances[index].ids[1],
            0,
        ]
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        let stroke = &self.instances[index];
        pomelo_core::selection::SelectedObject::Drawing(pomelo_core::model::ObjectId(
            if stroke.flags[1] == 1 {
                stroke.flags[0]
            } else {
                u32::MAX
            },
        ))
    }
}

impl InstanceSource for crate::text::msdf::PreparedGlyphs {
    const COMPACT_TEXT: bool = false;
    const MSDF: bool = true;
    type Instance = crate::text::msdf::GlyphInstance;
    fn instances(&self) -> &[Self::Instance] {
        &self.instances
    }
    fn batches(&self) -> &[crate::tracks::TraceBatch] {
        &self.batches
    }
    fn selection_ids(&self, index: usize) -> [u32; 4] {
        let ids = self.instances[index].ids;
        [ids[0], 0, ids[2], ids[3]]
    }
    fn selected_object(&self, index: usize) -> pomelo_core::selection::SelectedObject {
        pomelo_core::selection::SelectedObject::Drawing(pomelo_core::model::ObjectId(
            if self.instances[index].low[2] == 1.0 {
                self.instances[index].ids[0]
            } else {
                u32::MAX
            },
        ))
    }
    fn font(&self) -> Option<&crate::text::msdf::MsdfFont> {
        Some(&self.font)
    }
    fn atlas_page(&self, index: usize) -> u16 {
        self.instances[index].page()
    }
    fn label_category(&self, index: usize) -> u32 {
        self.instances[index].ids[1]
    }
}

pub(super) struct UploadedTracks<S: InstanceSource = PreparedTracks> {
    pub source: Arc<S>,
    chunks: Vec<Chunk>,
    uploaded: usize,
}

impl<S: InstanceSource> UploadedTracks<S> {
    pub fn new(source: Arc<S>) -> anyhow::Result<Self> {
        ensure!(
            source.instances().len() <= u32::MAX as usize,
            "GPU_TRACE_INSTANCE_LIMIT"
        );
        let mut chunks = Vec::new();
        chunks.try_reserve_exact(source.instances().len().div_ceil(CHUNK_INSTANCES))?;
        for batch in source.batches() {
            let end = batch
                .start
                .checked_add(batch.count)
                .context("GPU_TRACE_BATCH_OVERFLOW")?;
            ensure!(
                end as usize <= source.instances().len(),
                "GPU_TRACE_BATCH_RANGE"
            );
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
            if start == self.source.instances().len() {
                break;
            }
            let count = CHUNK_INSTANCES.min(self.source.instances().len() - start);
            let bytes = count * std::mem::size_of::<S::Instance>();
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
                        StructureByteStride: std::mem::size_of::<S::Instance>() as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: self.source.instances()[start..start + count]
                            .as_ptr()
                            .cast(),
                        ..Default::default()
                    }),
                    Some(&mut buffer),
                )?;
                let buffer = buffer.context("GPU_TRACE_BUFFER_MISSING")?;
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
                view: view.context("GPU_TRACE_SRV_MISSING")?,
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
    atlas: Option<super::msdf_d3d11::AtlasResources>,
}

fn msdf_shader_source() -> String {
    format!(
        "#define PCB_SOURCE_TEXT_CATEGORY {}u\n{}",
        pomelo_core::display::DisplayCategory::Text as u32,
        include_str!("../../shaders/label.hlsl")
    )
}

impl Pipeline {
    pub fn new_msdf(
        device: &ID3D11Device,
        font: &crate::text::msdf::MsdfFont,
    ) -> anyhow::Result<Self> {
        let mut pipeline = Self::new_with_source(
            device,
            &msdf_shader_source(),
            s!("pomelo-render/label.hlsl"),
        )?;
        pipeline.atlas = Some(super::msdf_d3d11::AtlasResources::new(device, font)?);
        Ok(pipeline)
    }
    pub fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        Self::new_with_source(
            device,
            include_str!("../../shaders/trace.hlsl"),
            s!("pomelo-render/trace.hlsl"),
        )
    }

    pub fn new_text(device: &ID3D11Device) -> anyhow::Result<Self> {
        Self::new_with_source(
            device,
            include_str!("../../shaders/text.hlsl"),
            s!("pomelo-render/text.hlsl"),
        )
    }

    fn new_with_source(
        device: &ID3D11Device,
        source: &str,
        name: windows::core::PCSTR,
    ) -> anyhow::Result<Self> {
        let vertex_blob = compile_shader(source, name, s!("trace_vertex"), s!("vs_5_0"))?;
        let fragment_blob = compile_shader(source, name, s!("trace_fragment"), s!("ps_5_0"))?;
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
            atlas: None,
            vertex: vertex.context("GPU_TRACE_VERTEX_MISSING")?,
            fragment: fragment.context("GPU_TRACE_FRAGMENT_MISSING")?,
            uniforms: uniforms.context("GPU_TRACE_UNIFORMS_MISSING")?,
            blend: blend.context("GPU_TRACE_BLEND_MISSING")?,
            rasterizer: rasterizer.context("GPU_TRACE_RASTERIZER_MISSING")?,
        })
    }

    pub fn draw<S: InstanceSource, T: InstanceSource>(
        &self,
        context: &NativeGpuContext<'_>,
        frame: &TraceFrame<T>,
        cache: &UploadedTracks<S>,
        scope: super::board::TraceScope,
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
                "GPU_TRACE_CAMERA_FIT"
            );
            camera
        };
        ensure!(
            camera.pixels_per_mm.is_finite()
                && camera.pixels_per_mm > 0.0
                && (camera.pixels_per_mm as f32).is_finite(),
            "GPU_TRACE_CAMERA_SCALE"
        );
        let [x, dx] = split_position(camera.center.x);
        let [y, dy] = split_position(camera.center.y);
        ensure!(
            [x, dx, y, dy].into_iter().all(f32::is_finite),
            "GPU_TRACE_CAMERA_CENTER"
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
        let (selection_id, selection_kind, highlight) = match frame.highlighted_trace {
            Some((super::board::TraceSelection::Segment(id), color)) => (id.0, 2, color),
            Some((super::board::TraceSelection::Track(id), color)) => (id.0, 3, color),
            None => frame
                .highlighted_net
                .map_or((0, 0, [0.0; 4]), |(net, color)| {
                    (net.0, u32::from(net.0 != 0), color)
                }),
        };
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
                frame.opacity,
                frame.pass as u8 as f32,
            ],
            color: frame.fallback_color,
            batch: [
                0,
                selection_id,
                selection_kind,
                match scope {
                    super::board::TraceScope::ZoneOutlines(_, true) => 1,
                    super::board::TraceScope::ZoneOutlines(_, false) => 2,
                    _ => 0,
                },
            ],
            highlight,
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
            for batch in cache.source.batches() {
                let selected = match scope {
                    super::board::TraceScope::All => true,
                    super::board::TraceScope::Layer(layer)
                    | super::board::TraceScope::ZoneOutlines(layer, _) => {
                        !batch.outline && batch.layer == layer
                    }
                    super::board::TraceScope::Outline => batch.outline,
                    super::board::TraceScope::Labels(layer, _, _) => batch.layer == layer,
                    super::board::TraceScope::Pads(layer, _) => batch.layer == layer,
                };
                if !selected {
                    continue;
                }
                uniforms.color = if batch.outline {
                    let mut color = frame.fallback_color;
                    color[3] *= frame.opacity;
                    color
                } else {
                    frame.layer_color(batch.layer)
                };
                let base_color = uniforms.color;
                for chunk in &cache.chunks {
                    let start = (batch.start as usize).max(chunk.start);
                    let end = ((batch.start + batch.count) as usize).min(chunk.start + chunk.count);
                    if start >= end {
                        continue;
                    }
                    let mut cursor = start;
                    while cursor < end {
                        let span_start = cursor;
                        let accepted = |index: usize| match scope {
                            super::board::TraceScope::Pads(_, pin) => {
                                matches!(
                                    cache.source.selected_object(index),
                                    pomelo_core::selection::SelectedObject::Pin(_)
                                ) == pin
                            }
                            super::board::TraceScope::Labels(_, category, owner) => {
                                cache.source.label_category(index) == category as u32
                                    && owner.is_none_or(|id| {
                                        cache.source.selection_ids(index)[0] == id.0
                                    })
                            }
                            _ => true,
                        };
                        let accept = accepted(cursor);
                        let page = cache.source.atlas_page(cursor);
                        let selected = |index: usize| {
                            if batch.outline {
                                return None;
                            }
                            frame.object_highlight(
                                cache.source.selected_object(index),
                                pomelo_core::model::NetId(cache.source.selection_ids(index)[3]),
                                Some(pomelo_core::model::ObjectId(
                                    cache.source.selection_ids(index)[1],
                                )),
                            )
                        };
                        let color = selected(cursor);
                        cursor += 1;
                        if frame.pass == super::board::OverlayPass::Base
                            && !S::MSDF
                            && !matches!(scope, super::board::TraceScope::Pads(_, _))
                            && frame.highlighted_related_objects.is_none()
                            && frame.highlighted_object.is_none()
                            && frame.hovered_object.is_none()
                        {
                            cursor = end;
                        } else {
                            while cursor < end
                                && selected(cursor) == color
                                && accepted(cursor) == accept
                                && cache.source.atlas_page(cursor) == page
                            {
                                cursor += 1;
                            }
                        }
                        if !accept {
                            continue;
                        }
                        if frame.pass != super::board::OverlayPass::Base && color.is_none() {
                            continue;
                        }
                        uniforms.color = color.unwrap_or(base_color);
                        // Network colors are computed per instance; object overrides still win.
                        uniforms.view[2] = if S::MSDF {
                            frame.opacity
                        } else {
                            f32::from(u8::from(
                                frame.color_mode == pomelo_core::display::ColorMode::Net
                                    && !batch.outline
                                    && !S::COMPACT_TEXT
                                    && color.is_none(),
                            ))
                        };
                        uniforms.batch[2] = if color.is_some() { 0 } else { selection_kind };
                        uniforms.batch[0] = (span_start - chunk.start) as u32;
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
                        if let Some(atlas) = &self.atlas {
                            let resource =
                                atlas.pages.get(&page).context("GPU_MSDF_PAGE_MISSING")?;
                            ctx.PSSetShaderResources(0, Some(&[Some(resource.clone())]));
                            ctx.PSSetSamplers(0, Some(&[Some(atlas.sampler.clone())]));
                        }
                        // Explicit shader offset avoids relying on StartInstanceLocation for an SRV.
                        ctx.DrawInstanced(4, (cursor - span_start) as u32, 0, 0);
                        draws += 1;
                    }
                }
            }
            ctx.VSSetShaderResources(0, Some(&[None]));
            if self.atlas.is_some() {
                ctx.PSSetShaderResources(0, Some(&[None]));
                ctx.PSSetSamplers(0, Some(&[None]));
            }
        }
        Ok(draws)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_pad_shaders_compile_for_both_d3d11_stages() {
        let source = include_str!("../../shaders/pad.hlsl");
        compile_shader(source, s!("pad.hlsl"), s!("pad_vertex"), s!("vs_5_0")).unwrap();
        compile_shader(source, s!("pad.hlsl"), s!("pad_fragment"), s!("ps_5_0")).unwrap();
    }

    #[test]
    fn real_trace_shaders_compile_for_both_d3d11_stages() {
        let source = include_str!("../../shaders/trace.hlsl");
        compile_shader(source, s!("trace.hlsl"), s!("trace_vertex"), s!("vs_5_0")).unwrap();
        compile_shader(source, s!("trace.hlsl"), s!("trace_fragment"), s!("ps_5_0")).unwrap();
    }
    #[test]
    fn web_msdf_label_shader_compiles_for_both_d3d11_stages() {
        let source = msdf_shader_source();
        compile_shader(&source, s!("label.hlsl"), s!("trace_vertex"), s!("vs_5_0")).unwrap();
        compile_shader(
            &source,
            s!("label.hlsl"),
            s!("trace_fragment"),
            s!("ps_5_0"),
        )
        .unwrap();
    }
}

#[cfg(test)]
#[path = "trace_pixels.rs"]
mod pixel_tests;
