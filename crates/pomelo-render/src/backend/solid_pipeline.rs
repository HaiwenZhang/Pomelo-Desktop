//! Shared residency and visibility policy for the D3D11 and wgpu solid renderers.
#[cfg(target_os = "windows")]
use super::d3d11::{Buffer, SolidDriver};
#[cfg(target_os = "linux")]
use super::wgpu::{Buffer, SolidDriver};
use super::{
    NativeGpuContext,
    common::{Device, Rect},
    solid_cache::{GeometryCache, cache_target, system_memory, upload_memory},
    solid_renderer::{SolidFrame, SolidUniforms, display_bounds},
};
use crate::scene::board3d::{Instance3d, PreparedBoard3d, Vertex3d};
use anyhow::{Context as _, ensure};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum BufferKind {
    Vertex,
    Index,
    Material,
}
pub(crate) struct GpuMesh<B = Buffer> {
    pub vertices: B,
    pub indices: B,
}
pub(crate) struct SolidDraw<'a, B = Buffer> {
    pub uniforms: SolidUniforms,
    pub size: [u32; 2],
    pub rect: Rect,
    pub scene: &'a PreparedBoard3d,
    pub meshes: &'a [Option<GpuMesh<B>>],
    pub instances: &'a [Option<B>],
    pub material: &'a B,
    pub opaque: &'a [usize],
    pub transparent: &'a [usize],
    pub visible: &'a [bool],
}
pub(crate) struct SolidPipeline {
    driver: SolidDriver,
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
    visible: Vec<usize>,
    visible_mask: Vec<bool>,
    opaque_batches: Vec<usize>,
    transparent_batches: Vec<usize>,
}
impl SolidPipeline {
    pub fn new(device: &Device<'_>, scene: &PreparedBoard3d) -> anyhow::Result<Self> {
        for mesh in &scene.meshes {
            ensure!(mesh.valid_lods(), "SOLID_LOD_RANGE");
            ensure!(
                u32::try_from(mesh.indices.len()).is_ok(),
                "SOLID_INDEX_COUNT"
            );
            ensure!(
                !mesh.vertices.is_empty() && !mesh.indices.is_empty(),
                "SOLID_EMPTY_MESH"
            );
        }
        Ok(Self {
            driver: SolidDriver::new(device)?,
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
            visible: vec![],
            visible_mask: vec![],
            opaque_batches: vec![],
            transparent_batches: vec![],
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
    fn memory_pressure(&mut self) {
        if self
            .sampled_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            let (total, available) = system_memory();
            self.cache_budget_bytes = cache_target(total, available, 0);
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
            if planned_bytes > 0 && planned_bytes.saturating_add(bytes) > FRAME_UPLOAD_BYTES {
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
                    vertices: device
                        .solid_buffer(bytemuck::cast_slice(&mesh.vertices), BufferKind::Vertex)?,
                    indices: device
                        .solid_buffer(bytemuck::cast_slice(&mesh.indices), BufferKind::Index)?,
                });
                self.cache.geometry_loaded(page.mesh);
            }
            self.instances[id] = Some(
                device.solid_buffer(bytemuck::cast_slice(&page.instances), BufferKind::Vertex)?,
            );
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
        self.uploaded_this_frame = 0;
        self.pending_pages = 0;
        self.evicted_this_frame = 0;
        let clip = context.bounds.intersect(&context.content_mask.bounds);
        if clip.size.width.0 <= 0.0 || clip.size.height.0 <= 0.0 {
            self.visible.clear();
            self.visible_mask.clear();
            self.visible_mask.resize(frame.scene.pages.len(), false);
            self.memory_pressure();
            return Ok(0);
        }
        let extent = [
            context.bounds.size.width.0.ceil(),
            context.bounds.size.height.0.ceil(),
        ];
        ensure!(
            extent
                .into_iter()
                .all(|v| v.is_finite() && v > 0.0 && v <= 16384.0),
            "SOLID_VIEWPORT_SIZE"
        );
        let size = extent.map(|v| v as u32);
        ensure!(
            u64::from(size[0]) * u64::from(size[1]) * 22 <= 512 * 1024 * 1024,
            "SOLID_TARGET_MEMORY_LIMIT"
        );
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
                material_bytes <= device.buffer_limit(false)
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
            self.materials = Some(device.solid_buffer(
                bytemuck::cast_slice(frame.materials.as_slice()),
                BufferKind::Material,
            )?);
            drop(_memory);
            self.active_mask.fill(false);
            self.opaque_batches.clear();
            self.transparent_batches.clear();
            for (i, batch) in frame.scene.pages.iter().enumerate() {
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
            frame.z_scale,
            &mut self.visible,
        )?;
        self.visible_mask.clear();
        self.visible_mask.resize(frame.scene.pages.len(), false);
        for &id in &self.visible {
            self.visible_mask[id] = self.active_mask[id];
        }
        self.memory_pressure();
        self.upload_visible(device, &frame.scene)?;
        let bounds = display_bounds(frame.scene.bounds, frame.z_scale);
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
                device.solid_opacity(),
                f32::from(!self.transparent_batches.is_empty()),
                (f64::from(size[1]).max(
                    f64::from(size[0]) * f64::from(context.bounds.size.height.0)
                        / f64::from(context.bounds.size.width.0),
                ) / frame.camera.view_height()) as f32,
            ],
        };

        let x = clip.origin.x.0.floor().max(0.0) as i32;
        let y = clip.origin.y.0.floor().max(0.0) as i32;
        let right = (clip.origin.x.0 + clip.size.width.0)
            .ceil()
            .min(context.viewport[0]) as i32;
        let bottom = (clip.origin.y.0 + clip.size.height.0)
            .ceil()
            .min(context.viewport[1]) as i32;
        if right <= x || bottom <= y {
            return Ok(0);
        }
        self.driver.draw(
            device,
            SolidDraw {
                uniforms,
                size,
                rect: Rect {
                    left: x,
                    top: y,
                    right,
                    bottom,
                },
                scene: &frame.scene,
                meshes: &self.meshes,
                instances: &self.instances,
                material: self.materials.as_ref().context("SOLID_MATERIALS")?,
                opaque: &self.opaque_batches,
                transparent: &self.transparent_batches,
                visible: &self.visible_mask,
            },
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/backend/solid.rs"]
pub(crate) mod tests;
