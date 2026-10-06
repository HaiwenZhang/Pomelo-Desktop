//! Metal resources and command encoders on GPUI's borrowed command buffer.
use super::super::Shader;
use super::super::common::Rect;
use super::super::common::driver::{Draw, StencilMode, StencilScope};
use super::shaders;
use anyhow::{Context as _, ensure};
use metal::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
pub(crate) struct Buffer {
    raw: metal::Buffer,
    size: usize,
}
pub(crate) struct Atlas {
    texture: Texture,
    sampler: SamplerState,
}
pub(crate) struct Pipeline {
    strip: BTreeMap<bool, RenderPipelineState>,
    triangles: BTreeMap<bool, RenderPipelineState>,
    states: BTreeMap<(StencilMode, bool), DepthStencilState>,
}
pub(crate) struct Device<'a> {
    raw: &'a DeviceRef,
    command_buffer: &'a CommandBufferRef,
    color: &'a TextureRef,
    format: MTLPixelFormat,
    opacity: f32,
    stencil: RefCell<Option<Texture>>,
    encoder: RefCell<Option<RenderCommandEncoder>>,
    active: Cell<bool>,
}
impl<'a> Device<'a> {
    pub fn new(context: &gpui_apple::MetalPaintContext<'a>) -> Self {
        Self {
            raw: context.device,
            command_buffer: context.command_buffer,
            color: context.color_target,
            format: context.color_format,
            opacity: context.target.opacity,
            stencil: RefCell::new(None),
            encoder: RefCell::new(None),
            active: Cell::new(false),
        }
    }
    pub fn structured_buffer(&self, data: &[u8], _: usize) -> anyhow::Result<Buffer> {
        self.buffer(data, false)
    }
    pub fn buffer(&self, data: &[u8], _: bool) -> anyhow::Result<Buffer> {
        ensure!(!data.is_empty(), "GPU_BUFFER_EMPTY");
        Ok(Buffer {
            raw: self.raw.new_buffer_with_data(
                data.as_ptr().cast(),
                data.len() as u64,
                MTLResourceOptions::StorageModeShared,
            ),
            size: data.len(),
        })
    }
    /// Maximum geometry buffer allocation supported by the Metal device.
    pub fn buffer_limit(&self, _: bool) -> usize {
        self.raw.max_buffer_length() as usize
    }
    pub fn buffer_empty(&self, size: usize, _: bool) -> anyhow::Result<Buffer> {
        ensure!(size != 0, "GPU_BUFFER_EMPTY");
        Ok(Buffer {
            raw: self
                .raw
                .new_buffer(size as u64, MTLResourceOptions::StorageModeShared),
            size,
        })
    }
    pub fn write_buffer(&self, buffer: &Buffer, offset: usize, data: &[u8]) -> anyhow::Result<()> {
        ensure!(
            offset
                .checked_add(data.len())
                .is_some_and(|end| end <= buffer.size),
            "GPU_UPLOAD_RANGE"
        );
        // SAFETY: bounds checked shared storage. Each staged upload writes only previously
        // unused bytes, before encoding any draw that references those bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                buffer.raw.contents().cast::<u8>().add(offset),
                data.len(),
            );
        }
        Ok(())
    }
    pub fn atlas(&self, width: u32, height: u32, data: &[u8]) -> anyhow::Result<Atlas> {
        ensure!(
            data.len() == width as usize * height as usize * 4,
            "GPU_ATLAS_SIZE"
        );
        let desc = TextureDescriptor::new();
        desc.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
        desc.set_width(width as u64);
        desc.set_height(height as u64);
        desc.set_usage(MTLTextureUsage::ShaderRead);
        desc.set_storage_mode(MTLStorageMode::Shared);
        let texture = self.raw.new_texture(&desc);
        texture.replace_region(
            MTLRegion::new_2d(0, 0, width as u64, height as u64),
            0,
            data.as_ptr().cast(),
            width as u64 * 4,
        );
        let desc = SamplerDescriptor::new();
        desc.set_min_filter(MTLSamplerMinMagFilter::Linear);
        desc.set_mag_filter(MTLSamplerMinMagFilter::Linear);
        desc.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
        desc.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
        Ok(Atlas {
            texture,
            sampler: self.raw.new_sampler(&desc),
        })
    }
    pub fn pipeline(&self, shader: Shader) -> anyhow::Result<Pipeline> {
        let source = shaders::source(shader);
        let options = CompileOptions::new();
        options.set_fast_math_enabled(false);
        let library = self
            .raw
            .new_library_with_source(source, &options)
            .map_err(|e| anyhow::anyhow!("Metal {shader:?}: {e}"))?;
        let function = |entry: &str| -> anyhow::Result<Function> {
            library
                .get_function(entry, None)
                .map_err(|e| anyhow::anyhow!(e))
        };
        let vertex = function("vertex_main")?;
        let fragment = function("fragment_main")?;
        let clear = if shader == Shader::Copper {
            Some(function("clear_main")?)
        } else {
            None
        };
        let mut pipeline = Pipeline {
            strip: BTreeMap::new(),
            triangles: BTreeMap::new(),
            states: BTreeMap::new(),
        };
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
                if !pipeline.strip.contains_key(&mode.color()) {
                    let desc = RenderPipelineDescriptor::new();
                    desc.set_vertex_function(Some(&vertex));
                    desc.set_fragment_function(Some(&fragment));
                    desc.set_stencil_attachment_pixel_format(MTLPixelFormat::Stencil8);
                    desc.set_raster_sample_count(self.color.sample_count());
                    let color = desc
                        .color_attachments()
                        .object_at(0)
                        .context("GPU_COLOR_DESCRIPTOR")?;
                    color.set_pixel_format(self.format);
                    color.set_write_mask(if mode.color() {
                        MTLColorWriteMask::All
                    } else {
                        MTLColorWriteMask::empty()
                    });
                    color.set_blending_enabled(true);
                    color.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
                    color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
                    color.set_source_alpha_blend_factor(MTLBlendFactor::One);
                    color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
                    pipeline.triangles.insert(
                        mode.color(),
                        self.raw
                            .new_render_pipeline_state(&desc)
                            .map_err(|e| anyhow::anyhow!(e))?,
                    );
                    if let Some(clear) = &clear {
                        desc.set_vertex_function(Some(clear));
                    }
                    pipeline.strip.insert(
                        mode.color(),
                        self.raw
                            .new_render_pipeline_state(&desc)
                            .map_err(|e| anyhow::anyhow!(e))?,
                    );
                }
                let (read, write, _, equal, invert, zero) = mode.state(inherited);
                let stencil = StencilDescriptor::new();
                stencil.set_read_mask(read);
                stencil.set_write_mask(write);
                stencil.set_stencil_compare_function(if equal {
                    MTLCompareFunction::Equal
                } else {
                    MTLCompareFunction::Always
                });
                stencil.set_stencil_failure_operation(MTLStencilOperation::Keep);
                stencil.set_depth_failure_operation(MTLStencilOperation::Keep);
                stencil.set_depth_stencil_pass_operation(if invert {
                    MTLStencilOperation::Invert
                } else if zero {
                    MTLStencilOperation::Zero
                } else if write != 0 {
                    MTLStencilOperation::Replace
                } else {
                    MTLStencilOperation::Keep
                });
                let depth = DepthStencilDescriptor::new();
                depth.set_depth_compare_function(MTLCompareFunction::Always);
                depth.set_depth_write_enabled(false);
                depth.set_front_face_stencil(Some(&stencil));
                depth.set_back_face_stencil(Some(&stencil));
                pipeline
                    .states
                    .insert((mode, inherited), self.raw.new_depth_stencil_state(&depth));
            }
        }
        Ok(pipeline)
    }
    fn ensure_stencil(&self) {
        if self.stencil.borrow().is_some() {
            return;
        }
        let desc = TextureDescriptor::new();
        desc.set_width(self.color.width());
        desc.set_height(self.color.height());
        desc.set_pixel_format(MTLPixelFormat::Stencil8);
        desc.set_storage_mode(MTLStorageMode::Private);
        desc.set_usage(MTLTextureUsage::RenderTarget);
        desc.set_sample_count(self.color.sample_count());
        if self.color.sample_count() > 1 {
            desc.set_texture_type(MTLTextureType::D2Multisample);
        }
        *self.stencil.borrow_mut() = Some(self.raw.new_texture(&desc));
    }
    pub fn stencil_scope(&self, _: [f32; 2]) -> anyhow::Result<StencilScope<'_>> {
        self.ensure_stencil();
        let previous = self.active.replace(true);
        Ok(StencilScope {
            active: &self.active,
            previous,
        })
    }
    // Keep all PCB batches in one render pass. Creating a render encoder per
    // batch can exhaust Metal's per-command-buffer encoder resources on large boards.
    fn ensure_encoder(&self) -> anyhow::Result<()> {
        if self.encoder.borrow().is_some() {
            return Ok(());
        }
        self.ensure_stencil();
        let stencil = self.stencil.borrow();
        let stencil = stencil.as_ref().context("GPU_STENCIL_MISSING")?;
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .context("GPU_COLOR_DESCRIPTOR")?;
        color.set_texture(Some(self.color));
        color.set_load_action(MTLLoadAction::Load);
        color.set_store_action(MTLStoreAction::Store);
        let mask = pass
            .stencil_attachment()
            .context("GPU_STENCIL_DESCRIPTOR")?;
        mask.set_texture(Some(stencil));
        mask.set_clear_stencil(0);
        mask.set_load_action(MTLLoadAction::Clear);
        mask.set_store_action(MTLStoreAction::Store);
        *self.encoder.borrow_mut() = Some(
            self.command_buffer
                .new_render_command_encoder(pass)
                .to_owned(),
        );
        Ok(())
    }
    fn finish(&self) {
        if let Some(encoder) = self.encoder.borrow_mut().take() {
            encoder.end_encoding();
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
            right > left && bottom > top && left >= 0 && top >= 0,
            "GPU_SCISSOR_INVALID"
        );
        self.ensure_encoder()?;
        let encoder = self.encoder.borrow();
        let encoder = encoder.as_ref().context("GPU_ENCODER_MISSING")?;
        let key = (draw.stencil, self.active.get());
        encoder.set_render_pipeline_state(if draw.indices.is_some() {
            &pipeline.triangles[&draw.stencil.color()]
        } else {
            &pipeline.strip[&draw.stencil.color()]
        });
        encoder.set_depth_stencil_state(&pipeline.states[&key]);
        encoder.set_stencil_reference_value(draw.stencil.state(self.active.get()).2);
        encoder.set_cull_mode(MTLCullMode::None);
        encoder.set_scissor_rect(MTLScissorRect {
            x: left as u64,
            y: top as u64,
            width: (right - left) as u64,
            height: (bottom - top) as u64,
        });
        let mut uniforms = draw.uniforms.to_vec();
        uniforms.extend_from_slice(bytemuck::cast_slice(&[self.opacity, 0.0, 0.0, 0.0]));
        encoder.set_vertex_bytes(0, uniforms.len() as u64, uniforms.as_ptr().cast());
        encoder.set_fragment_bytes(0, uniforms.len() as u64, uniforms.as_ptr().cast());
        encoder.set_vertex_buffer(1, Some(&draw.vertices.raw), 0);
        let sizes = [draw.vertices.size as u32];
        encoder.set_vertex_bytes(2, 4, sizes.as_ptr().cast());
        if let Some(atlas) = draw.atlas {
            encoder.set_fragment_texture(0, Some(&atlas.texture));
            encoder.set_fragment_sampler_state(0, Some(&atlas.sampler));
        }
        if let Some(indices) = draw.indices {
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                draw.count as u64,
                MTLIndexType::UInt32,
                &indices.raw,
                draw.start as u64 * 4,
            );
        } else {
            encoder.draw_primitives_instanced(
                MTLPrimitiveType::TriangleStrip,
                draw.start as u64,
                draw.count as u64,
                draw.instances as u64,
            );
        }
        Ok(())
    }
}

impl Drop for Device<'_> {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/backend/metal/pixels.rs"]
mod tests;
