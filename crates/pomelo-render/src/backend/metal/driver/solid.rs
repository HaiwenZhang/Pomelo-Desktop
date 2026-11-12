use super::{Buffer, Device};
use crate::{
    backend::{
        NativeGpuContext,
        solid_cache::{GeometryCache, cache_target, system_memory, upload_memory},
        solid_renderer::{SolidFrame, SolidUniforms, metal_source},
    },
    scene::board3d::{Instance3d, PreparedBoard3d, Vertex3d},
};
use anyhow::{Context as _, ensure};
use metal::objc::{msg_send, sel, sel_impl};
use metal::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct Targets {
    size: [u64; 2],
    color: Texture,
    depth: Texture,
    accum: Texture,
    reveal: Texture,
}

struct GpuMesh {
    vertices: Buffer,
    indices: Buffer,
}

pub(crate) struct SolidPipeline {
    meshes: Vec<Option<GpuMesh>>,
    instances: Vec<Option<Buffer>>,
    active_mask: Vec<bool>,
    upload_plan: Vec<usize>,
    planned_meshes: Vec<bool>,
    cache: GeometryCache,
    sampled_at: Option<Instant>,
    cache_budget_bytes: usize,
    pub(crate) evicted_this_frame: usize,
    pub(crate) pending_pages: usize,
    pub(crate) resident_bytes: usize,
    pub(crate) uploaded_this_frame: usize,
    materials: Option<Buffer>,
    material_source: Option<Arc<Vec<[f32; 4]>>>,
    opaque: RenderPipelineState,
    transparent: RenderPipelineState,
    edges: RenderPipelineState,
    composite: RenderPipelineState,
    depth_write: DepthStencilState,
    depth_read: DepthStencilState,
    targets: Option<Targets>,
    visible: Vec<usize>,
    visible_mask: Vec<bool>,
    opaque_batches: Vec<usize>,
    transparent_batches: Vec<usize>,
    edge_batches: Vec<usize>,
}
impl SolidPipeline {
    #[allow(
        unexpected_cfgs,
        reason = "objc 0.2 selector macros contain a legacy cargo-clippy cfg"
    )]
    fn texture(device: &DeviceRef, descriptor: &TextureDescriptorRef) -> anyhow::Result<Texture> {
        let raw: *mut metal::MTLTexture =
            unsafe { msg_send![device,newTextureWithDescriptor:descriptor] };
        unsafe { super::owned_gpu_resource(raw) }
    }
    pub fn new(device: &Device<'_>, scene: &PreparedBoard3d) -> anyhow::Result<Self> {
        let source = metal_source()?;
        let library = device
            .raw
            .new_library_with_source(&source, &CompileOptions::new())
            .map_err(|e| anyhow::anyhow!("Metal solid: {e}"))?;
        Self::with_library(device, scene, &library, None)
    }
    fn with_library(
        device: &Device<'_>,
        scene: &PreparedBoard3d,
        library: &LibraryRef,
        archive: Option<&BinaryArchiveRef>,
    ) -> anyhow::Result<Self> {
        ensure!(
            scene.meshes.iter().all(|m| m.valid_lods()),
            "SOLID_LOD_RANGE"
        );
        for size in scene
            .meshes
            .iter()
            .flat_map(|m| {
                [
                    m.vertices.len() * size_of::<Vertex3d>(),
                    m.indices.len() * 4,
                ]
            })
            .chain(
                scene
                    .pages
                    .iter()
                    .map(|p| p.instances.len() * size_of::<Instance3d>()),
            )
        {
            ensure!(
                size > 0 && size <= device.raw.max_buffer_length() as usize,
                "SOLID_GPU_BUFFER_LIMIT"
            );
        }
        let vertex = library
            .get_function("solid_vertex", None)
            .map_err(|e| anyhow::anyhow!(e))?;
        let descriptor = VertexDescriptor::new();
        for (location, format, offset, buffer) in [
            (0, MTLVertexFormat::Float4, 0, 1),
            (1, MTLVertexFormat::Float4, 16, 1),
            (2, MTLVertexFormat::UInt4, 32, 1),
            (3, MTLVertexFormat::Float4, 0, 2),
            (4, MTLVertexFormat::UInt4, 16, 2),
        ] {
            let a = descriptor
                .attributes()
                .object_at(location)
                .context("SOLID_ATTRIBUTE")?;
            a.set_format(format);
            a.set_offset(offset);
            a.set_buffer_index(buffer);
        }
        let layout = descriptor
            .layouts()
            .object_at(1)
            .context("SOLID_VERTEX_LAYOUT")?;
        layout.set_stride(size_of::<Vertex3d>() as u64);
        layout.set_step_function(MTLVertexStepFunction::PerVertex);
        let layout = descriptor
            .layouts()
            .object_at(2)
            .context("SOLID_INSTANCE_LAYOUT")?;
        layout.set_stride(size_of::<Instance3d>() as u64);
        layout.set_step_function(MTLVertexStepFunction::PerInstance);
        layout.set_step_rate(1);
        let edge_vertex = library
            .get_function("edge_vertex", None)
            .map_err(|e| anyhow::anyhow!(e))?;
        let geometry = |entry: &str, transparent: bool| -> anyhow::Result<RenderPipelineState> {
            let desc = RenderPipelineDescriptor::new();
            desc.set_vertex_function(Some(if entry == "solid_edge" {
                &edge_vertex
            } else {
                &vertex
            }));
            desc.set_vertex_descriptor(Some(descriptor));
            let fragment = library
                .get_function(entry, None)
                .map_err(|e| anyhow::anyhow!(e))?;
            desc.set_fragment_function(Some(&fragment));
            desc.set_depth_attachment_pixel_format(MTLPixelFormat::Depth32Float);
            if let Some(archive) = archive {
                desc.set_binary_archives(&[archive]);
            }
            let color = desc
                .color_attachments()
                .object_at(0)
                .context("SOLID_COLOR")?;
            color.set_pixel_format(MTLPixelFormat::RGBA16Float);
            if transparent {
                color.set_blending_enabled(true);
                color.set_source_rgb_blend_factor(MTLBlendFactor::One);
                color.set_destination_rgb_blend_factor(MTLBlendFactor::One);
                color.set_source_alpha_blend_factor(MTLBlendFactor::One);
                color.set_destination_alpha_blend_factor(MTLBlendFactor::One);
                let reveal = desc
                    .color_attachments()
                    .object_at(1)
                    .context("SOLID_REVEAL")?;
                reveal.set_pixel_format(MTLPixelFormat::R16Float);
                reveal.set_blending_enabled(true);
                reveal.set_source_rgb_blend_factor(MTLBlendFactor::Zero);
                reveal.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceColor);
            }
            device
                .raw
                .new_render_pipeline_state(&desc)
                .map_err(|e| anyhow::anyhow!("Metal solid pipeline: {e}"))
        };
        let opaque = geometry("solid_opaque", false)?;
        let transparent = geometry("solid_transparent", true)?;
        let edges = geometry("solid_edge", false)?;
        let desc = RenderPipelineDescriptor::new();
        let cv = library
            .get_function("composite_vertex", None)
            .map_err(|e| anyhow::anyhow!(e))?;
        let cf = library
            .get_function("composite_fragment", None)
            .map_err(|e| anyhow::anyhow!(e))?;
        desc.set_vertex_function(Some(&cv));
        desc.set_fragment_function(Some(&cf));
        desc.set_raster_sample_count(device.color.sample_count());
        if let Some(archive) = archive {
            desc.set_binary_archives(&[archive]);
        }
        let color = desc
            .color_attachments()
            .object_at(0)
            .context("SOLID_COMPOSITE")?;
        color.set_pixel_format(device.format);
        color.set_blending_enabled(true);
        color.set_source_rgb_blend_factor(MTLBlendFactor::One);
        color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        color.set_source_alpha_blend_factor(MTLBlendFactor::One);
        color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        let composite = device
            .raw
            .new_render_pipeline_state(&desc)
            .map_err(|e| anyhow::anyhow!(e))?;
        let depth = DepthStencilDescriptor::new();
        depth.set_depth_compare_function(MTLCompareFunction::LessEqual);
        depth.set_depth_write_enabled(true);
        let depth_write = device.raw.new_depth_stencil_state(&depth);
        depth.set_depth_write_enabled(false);
        let depth_read = device.raw.new_depth_stencil_state(&depth);
        Ok(Self {
            meshes: (0..scene.meshes.len()).map(|_| None).collect(),
            instances: (0..scene.pages.len()).map(|_| None).collect(),
            active_mask: vec![false; scene.pages.len()],
            upload_plan: vec![],
            planned_meshes: vec![false; scene.meshes.len()],
            cache: GeometryCache::new(
                scene
                    .meshes
                    .iter()
                    .map(|m| m.vertices.len() * size_of::<Vertex3d>() + m.indices.len() * 4),
                scene
                    .pages
                    .iter()
                    .map(|p| (p.mesh, p.instances.len() * size_of::<Instance3d>())),
            ),
            sampled_at: None,
            cache_budget_bytes: 0,
            evicted_this_frame: 0,
            pending_pages: 0,
            resident_bytes: 0,
            uploaded_this_frame: 0,
            materials: None,
            material_source: None,
            opaque,
            transparent,
            edges,
            composite,
            depth_write,
            depth_read,
            targets: None,
            visible: vec![],
            visible_mask: vec![],
            opaque_batches: vec![],
            transparent_batches: vec![],
            edge_batches: vec![],
        })
    }
    fn evict_to(&mut self, target: usize) {
        for victim in self.cache.trim(&self.visible_mask, target) {
            // GPUI uses retaining command buffers; in-flight draws keep their resources alive.
            if let Some(page) = victim.page {
                self.instances[page] = None;
            }
            if let Some(mesh) = victim.mesh {
                self.meshes[mesh] = None;
            }
            self.evicted_this_frame += victim.bytes;
        }
        self.resident_bytes = self.cache.resident_bytes;
    }
    fn memory_pressure(&mut self, device: &Device<'_>) {
        if self
            .sampled_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            let (total, available) = system_memory();
            self.cache_budget_bytes = cache_target(
                total,
                available,
                device.raw.recommended_max_working_set_size(),
            );
            self.sampled_at = Some(Instant::now());
        }
        self.evicted_this_frame = 0;
        self.cache.begin_frame(&self.visible_mask);
        self.evict_to(self.cache_budget_bytes);
    }
    fn upload_visible(
        &mut self,
        device: &Device<'_>,
        scene: &PreparedBoard3d,
    ) -> anyhow::Result<()> {
        const FRAME_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
        self.uploaded_this_frame = 0;
        self.pending_pages = 0;
        self.upload_plan.clear();
        self.planned_meshes.fill(false);
        let mut planned_bytes = 0usize;
        for &id in &self.visible {
            if !self.visible_mask[id] || self.instances[id].is_some() {
                continue;
            }
            let page = &scene.pages[id];
            let mesh = &scene.meshes[page.mesh];
            let mesh_bytes = if self.meshes[page.mesh].is_none() && !self.planned_meshes[page.mesh]
            {
                mesh.vertices.len() * size_of::<Vertex3d>() + mesh.indices.len() * 4
            } else {
                0
            };
            let bytes = mesh_bytes + page.instances.len() * size_of::<Instance3d>();
            if planned_bytes.saturating_add(bytes) > FRAME_UPLOAD_BYTES {
                self.pending_pages += 1;
                continue;
            }
            self.upload_plan.try_reserve(1)?;
            self.upload_plan.push(id);
            self.planned_meshes[page.mesh] = true;
            planned_bytes += bytes;
        }
        if self.upload_plan.is_empty() {
            return Ok(());
        }
        let _memory = upload_memory(planned_bytes).map_err(|e| {
            anyhow::anyhow!(
                "SOLID_SYSTEM_MEMORY_PRESSURE requested={} headroom={}",
                e.actual,
                e.limit
            )
        })?;
        for &id in &self.upload_plan {
            let page = &scene.pages[id];
            let mesh = &scene.meshes[page.mesh];
            let bytes = self.cache.upload_bytes(id);
            if self.meshes[page.mesh].is_none() {
                self.meshes[page.mesh] = Some(GpuMesh {
                    vertices: device.buffer(bytemuck::cast_slice(&mesh.vertices), false)?,
                    indices: device.buffer(bytemuck::cast_slice(&mesh.indices), false)?,
                });
                self.cache.geometry_loaded(page.mesh);
            }
            self.instances[id] = Some(device.buffer(bytemuck::cast_slice(&page.instances), false)?);
            self.cache.loaded(id);
            self.uploaded_this_frame += bytes;
            self.resident_bytes = self.cache.resident_bytes;
        }
        debug_assert_eq!(self.uploaded_this_frame, planned_bytes);
        Ok(())
    }

    pub fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        frame: &SolidFrame,
    ) -> anyhow::Result<u64> {
        let device = &context.device;
        device.finish();
        self.uploaded_this_frame = 0;
        self.pending_pages = 0;
        self.evicted_this_frame = 0;
        let clip = context.bounds.intersect(&context.content_mask.bounds);
        if clip.size.width.0 <= 0.0 || clip.size.height.0 <= 0.0 {
            self.visible.clear();
            self.visible_mask.clear();
            self.visible_mask.resize(frame.scene.pages.len(), false);
            self.memory_pressure(device);
            return Ok(0);
        }
        let size = [
            context.bounds.size.width.0.ceil() as u64,
            context.bounds.size.height.0.ceil() as u64,
        ];
        ensure!(
            size.into_iter().all(|v| v > 0 && v <= 16384),
            "SOLID_VIEWPORT_SIZE"
        );
        ensure!(
            size[0].saturating_mul(size[1]).saturating_mul(22) <= 512 * 1024 * 1024,
            "SOLID_TARGET_MEMORY_LIMIT"
        );
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            let _memory = upload_memory((size[0] * size[1] * 22) as usize).map_err(|e| {
                anyhow::anyhow!(
                    "SOLID_SYSTEM_MEMORY_PRESSURE requested={} headroom={}",
                    e.actual,
                    e.limit
                )
            })?;
            let texture = |format, usage| -> anyhow::Result<Texture> {
                let d = TextureDescriptor::new();
                d.set_width(size[0]);
                d.set_height(size[1]);
                d.set_pixel_format(format);
                d.set_storage_mode(MTLStorageMode::Private);
                d.set_usage(usage);
                Self::texture(device.raw, &d)
            };
            let usage = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
            self.targets = Some(Targets {
                size,
                color: texture(MTLPixelFormat::RGBA16Float, usage)?,
                depth: texture(MTLPixelFormat::Depth32Float, MTLTextureUsage::RenderTarget)?,
                accum: texture(MTLPixelFormat::RGBA16Float, usage)?,
                reveal: texture(MTLPixelFormat::R16Float, usage)?,
            });
        }
        if self
            .material_source
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(s, &frame.materials))
        {
            ensure!(
                frame
                    .materials
                    .iter()
                    .all(|c| c.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&c[3])),
                "SOLID_MATERIAL_INVALID"
            );
            let material_bytes = frame
                .materials
                .len()
                .checked_mul(size_of::<[f32; 4]>())
                .context("SOLID_MATERIAL_BUFFER_LIMIT")?;
            ensure!(
                material_bytes <= device.raw.max_buffer_length() as usize
                    && u32::try_from(material_bytes).is_ok(),
                "SOLID_MATERIAL_BUFFER_LIMIT"
            );
            let _memory = upload_memory(material_bytes).map_err(|e| {
                anyhow::anyhow!(
                    "SOLID_SYSTEM_MEMORY_PRESSURE requested={} headroom={}",
                    e.actual,
                    e.limit
                )
            })?;
            self.materials =
                Some(device.buffer(bytemuck::cast_slice(frame.materials.as_slice()), false)?);
            drop(_memory);
            self.active_mask.fill(false);
            self.opaque_batches.clear();
            self.transparent_batches.clear();
            self.edge_batches.clear();
            let edge_offset = frame.edge_offset();
            for (i, batch) in frame.scene.pages.iter().enumerate() {
                if !frame.scene.meshes[batch.mesh].edges.is_empty()
                    && batch.sources.iter().any(|id| {
                        frame
                            .materials
                            .get(edge_offset + *id as usize)
                            .is_some_and(|m| m[3] > 0.0)
                    })
                {
                    self.edge_batches.push(i);
                    self.active_mask[i] = true;
                }
                if batch
                    .sources
                    .iter()
                    .any(|id| frame.materials[*id as usize][3] >= 0.999)
                {
                    self.opaque_batches.push(i);
                    self.active_mask[i] = true;
                }
                if batch.sources.iter().any(|id| {
                    let a = frame.materials[*id as usize][3];
                    a > 0.0 && a < 0.999
                }) {
                    self.transparent_batches.push(i);
                    self.active_mask[i] = true;
                }
            }
            self.material_source = Some(Arc::clone(&frame.materials));
        }
        frame.scene.visible_pages(
            frame.camera,
            [
                f64::from(context.bounds.size.width.0),
                f64::from(context.bounds.size.height.0),
            ],
            frame.z_map(),
            &mut self.visible,
        )?;
        self.visible_mask.clear();
        self.visible_mask.resize(frame.scene.pages.len(), false);
        for &id in &self.visible {
            self.visible_mask[id] = self.active_mask[id];
        }
        self.memory_pressure(device);
        self.upload_visible(device, &frame.scene)?;
        let targets = self.targets.as_ref().context("SOLID_TARGETS")?;
        let material = self.materials.as_ref().context("SOLID_MATERIALS")?;
        let z_map = frame.z_map();
        let bounds = z_map.bounds(frame.scene.bounds);
        let matrix = frame
            .camera
            .clip_matrix(
                [
                    f64::from(context.bounds.size.width.0),
                    f64::from(context.bounds.size.height.0),
                ],
                bounds,
                [frame.scene.origin.x, frame.scene.origin.y, 0.0],
            )
            .context("SOLID_CAMERA")?;
        let uniforms = SolidUniforms {
            matrix,
            viewport: [
                context.viewport[0],
                context.viewport[1],
                size[0] as f32,
                size[1] as f32,
            ],
            canvas: [
                context.bounds.origin.x.0,
                context.bounds.origin.y.0,
                context.bounds.size.width.0,
                context.bounds.size.height.0,
            ],
            parameters: [
                frame.z_scale as f32,
                device.opacity,
                f32::from(!self.transparent_batches.is_empty()),
                ((size[1] as f64).max(
                    size[0] as f64 * f64::from(context.bounds.size.height.0)
                        / f64::from(context.bounds.size.width.0),
                ) / frame.camera.view_height()) as f32,
            ],
            extra: [
                f32::from_bits(frame.edge_offset() as u32),
                z_map.uniforms().0,
                0.0,
                0.0,
            ],
            stretch: z_map.uniforms().1,
        };
        let sizes = [material.size as u32];
        let draw = |encoder: &RenderCommandEncoderRef, batches: &[usize], edges: bool| {
            encoder.set_cull_mode(MTLCullMode::None);
            encoder.set_vertex_bytes(
                0,
                size_of::<SolidUniforms>() as u64,
                bytemuck::bytes_of(&uniforms).as_ptr().cast(),
            );
            encoder.set_fragment_buffer(3, Some(&material.raw), 0);
            encoder.set_fragment_bytes(4, size_of_val(&sizes) as u64, sizes.as_ptr().cast());
            let mut calls = 0;
            for &i in batches {
                let b = &frame.scene.pages[i];
                if !self.visible_mask[i] {
                    continue;
                }
                let (Some(mesh), Some(instances)) = (&self.meshes[b.mesh], &self.instances[i])
                else {
                    continue;
                };
                encoder.set_vertex_buffer(1, Some(&mesh.vertices.raw), 0);
                encoder.set_vertex_buffer(2, Some(&instances.raw), 0);
                calls += 1;
                let indices = if edges {
                    frame.scene.meshes[b.mesh].edges.clone()
                } else {
                    frame.scene.meshes[b.mesh].draw_indices(uniforms.parameters[3])
                };
                encoder.draw_indexed_primitives_instanced_base_instance(
                    if edges {
                        MTLPrimitiveType::Line
                    } else {
                        MTLPrimitiveType::Triangle
                    },
                    u64::from(indices.end - indices.start),
                    MTLIndexType::UInt32,
                    &mesh.indices.raw,
                    u64::from(indices.start) * 4,
                    b.instances.len() as u64,
                    0,
                    0,
                );
            }
            calls
        };
        let mut calls = 0;
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .context("SOLID_PASS")?;
        color.set_texture(Some(&targets.color));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_store_action(MTLStoreAction::Store);
        color.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        let depth = pass.depth_attachment().context("SOLID_DEPTH_ATTACHMENT")?;
        depth.set_texture(Some(&targets.depth));
        depth.set_load_action(MTLLoadAction::Clear);
        depth.set_store_action(MTLStoreAction::Store);
        depth.set_clear_depth(1.0);
        let encoder = device.command_buffer.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.opaque);
        encoder.set_depth_stencil_state(&self.depth_write);
        calls += draw(encoder, &self.opaque_batches, false);
        if !self.edge_batches.is_empty() {
            encoder.set_render_pipeline_state(&self.edges);
            encoder.set_depth_stencil_state(&self.depth_read);
            calls += draw(encoder, &self.edge_batches, true);
        }
        encoder.end_encoding();
        if !self.transparent_batches.is_empty() {
            let pass = RenderPassDescriptor::new();
            for (i, texture, clear) in [
                (0, &targets.accum, MTLClearColor::new(0.0, 0.0, 0.0, 0.0)),
                (1, &targets.reveal, MTLClearColor::new(1.0, 1.0, 1.0, 1.0)),
            ] {
                let a = pass
                    .color_attachments()
                    .object_at(i)
                    .context("SOLID_OIT_PASS")?;
                a.set_texture(Some(texture));
                a.set_load_action(MTLLoadAction::Clear);
                a.set_store_action(MTLStoreAction::Store);
                a.set_clear_color(clear);
            }
            let depth = pass.depth_attachment().context("SOLID_DEPTH_ATTACHMENT")?;
            depth.set_texture(Some(&targets.depth));
            depth.set_load_action(MTLLoadAction::Load);
            depth.set_store_action(MTLStoreAction::Store);
            let encoder = device.command_buffer.new_render_command_encoder(pass);
            encoder.set_render_pipeline_state(&self.transparent);
            encoder.set_depth_stencil_state(&self.depth_read);
            calls += draw(encoder, &self.transparent_batches, false);
            encoder.end_encoding();
        }
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .context("SOLID_WINDOW_PASS")?;
        color.set_texture(Some(device.color));
        color.set_load_action(MTLLoadAction::Load);
        color.set_store_action(MTLStoreAction::Store);
        let encoder = device.command_buffer.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&self.composite);
        let x = clip.origin.x.0.floor().max(0.0) as u64;
        let y = clip.origin.y.0.floor().max(0.0) as u64;
        let right = (clip.origin.x.0 + clip.size.width.0)
            .ceil()
            .min(context.viewport[0]) as u64;
        let bottom = (clip.origin.y.0 + clip.size.height.0)
            .ceil()
            .min(context.viewport[1]) as u64;
        if right > x && bottom > y {
            encoder.set_scissor_rect(MTLScissorRect {
                x,
                y,
                width: right - x,
                height: bottom - y,
            });
            encoder.set_vertex_bytes(
                0,
                size_of::<SolidUniforms>() as u64,
                bytemuck::bytes_of(&uniforms).as_ptr().cast(),
            );
            encoder.set_fragment_bytes(
                0,
                size_of::<SolidUniforms>() as u64,
                bytemuck::bytes_of(&uniforms).as_ptr().cast(),
            );
            encoder.set_fragment_texture(0, Some(&targets.color));
            encoder.set_fragment_texture(1, Some(&targets.accum));
            encoder.set_fragment_texture(2, Some(&targets.reveal));
            encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
        }
        encoder.end_encoding();
        Ok(calls + 1)
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/backend/metal/solid.rs"]
mod tests;
