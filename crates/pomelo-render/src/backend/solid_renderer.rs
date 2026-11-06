use super::{NativeGpuContext, NativeGpuRenderer};
use crate::scene::board3d::PreparedBoard3d;
use pomelo_core::{geometry3d::Bounds3, interaction::camera3d::Camera3d};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub struct SolidFrame {
    pub scene: Arc<PreparedBoard3d>,
    pub camera: Camera3d,
    pub materials: Arc<Vec<[f32; 4]>>,
    pub z_scale: f64,
}
#[derive(Default)]
pub struct SolidTelemetry {
    pub uploaded_bytes: AtomicU64,
    pub pending_pages: AtomicU64,
    pub frame_upload_bytes: AtomicU64,
    pub frame_evicted_bytes: AtomicU64,
    pub draw_calls: AtomicU64,
    pub frames: AtomicU64,
}
pub struct SolidRenderer {
    #[cfg(target_os = "macos")]
    pipeline: Option<super::metal::SolidPipeline>,
    source: Option<Arc<PreparedBoard3d>>,
    telemetry: Arc<SolidTelemetry>,
}
impl SolidRenderer {
    pub fn new(telemetry: Arc<SolidTelemetry>) -> Self {
        Self {
            #[cfg(target_os = "macos")]
            pipeline: None,
            source: None,
            telemetry,
        }
    }
    pub fn supported() -> bool {
        cfg!(target_os = "macos")
    }
}
impl NativeGpuRenderer for SolidRenderer {
    type Snapshot = SolidFrame;
    fn draw(&mut self, context: &NativeGpuContext<'_>, frame: &SolidFrame) -> anyhow::Result<()> {
        anyhow::ensure!(
            frame.materials.len() == frame.scene.sources.len() + 1,
            "SOLID_MATERIAL_COUNT"
        );
        anyhow::ensure!(
            frame.z_scale.is_finite() && (0.001..=1000.0).contains(&frame.z_scale),
            "SOLID_Z_SCALE"
        );
        #[cfg(target_os = "macos")]
        {
            if self
                .source
                .as_ref()
                .is_none_or(|s| !Arc::ptr_eq(s, &frame.scene))
            {
                self.pipeline = Some(super::metal::SolidPipeline::new(
                    &context.device,
                    &frame.scene,
                )?);
                self.source = Some(Arc::clone(&frame.scene));
            }
            let calls = self
                .pipeline
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("SOLID_PIPELINE_MISSING"))?
                .draw(context, frame)?;
            let pipeline = self
                .pipeline
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("SOLID_PIPELINE_MISSING"))?;
            self.telemetry
                .uploaded_bytes
                .store(pipeline.resident_bytes as u64, Ordering::Relaxed);
            self.telemetry
                .pending_pages
                .store(pipeline.pending_pages as u64, Ordering::Relaxed);
            self.telemetry
                .frame_upload_bytes
                .store(pipeline.uploaded_this_frame as u64, Ordering::Relaxed);
            self.telemetry
                .frame_evicted_bytes
                .store(pipeline.evicted_this_frame as u64, Ordering::Relaxed);
            self.telemetry.draw_calls.store(calls, Ordering::Relaxed);
            self.telemetry.frames.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (context, frame);
            anyhow::bail!("SOLID_BACKEND_NOT_AVAILABLE");
        }
    }
    fn reset(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.pipeline = None;
        }
        self.source = None;
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SolidUniforms {
    pub matrix: [[f32; 4]; 4],
    pub viewport: [f32; 4],
    pub canvas: [f32; 4],
    pub parameters: [f32; 4],
}
const _: () = assert!(size_of::<SolidUniforms>() == 112);
pub fn display_bounds(bounds: Bounds3, z_scale: f64) -> Bounds3 {
    Bounds3 {
        min: [bounds.min[0], bounds.min[1], bounds.min[2] * z_scale],
        max: [bounds.max[0], bounds.max[1], bounds.max[2] * z_scale],
    }
}
#[cfg(any(test, target_os = "macos"))]
fn validated() -> anyhow::Result<(naga::Module, naga::valid::ModuleInfo)> {
    let source = include_str!("../shaders/solid.wgsl");
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| anyhow::anyhow!("{}", e.emit_to_string(source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)?;
    Ok((module, info))
}
#[cfg(target_os = "macos")]
pub(crate) fn metal_source() -> anyhow::Result<String> {
    use naga::back::msl;
    let (module, info) = validated()?;
    let resource = |binding| naga::ResourceBinding { group: 0, binding };
    let entry = msl::EntryPointResources {
        resources: [
            (
                resource(0),
                msl::BindTarget {
                    buffer: Some(0),
                    ..Default::default()
                },
            ),
            (
                resource(3),
                msl::BindTarget {
                    buffer: Some(3),
                    ..Default::default()
                },
            ),
            (
                resource(4),
                msl::BindTarget {
                    texture: Some(0),
                    ..Default::default()
                },
            ),
            (
                resource(5),
                msl::BindTarget {
                    texture: Some(1),
                    ..Default::default()
                },
            ),
            (
                resource(6),
                msl::BindTarget {
                    texture: Some(2),
                    ..Default::default()
                },
            ),
        ]
        .into(),
        sizes_buffer: Some(4),
        ..Default::default()
    };
    let options = msl::Options {
        lang_version: (2, 4),
        per_entry_point_map: module
            .entry_points
            .iter()
            .map(|e| (e.name.clone(), entry.clone()))
            .collect(),
        fake_missing_bindings: false,
        ..Default::default()
    };
    Ok(msl::write_string(&module, &info, &options, &msl::PipelineOptions::default())?.0)
}
#[cfg(test)]
mod tests {
    #[test]
    fn solid_shader_validates_with_opaque_transparent_and_composite_entries() {
        let (module, _) = super::validated().unwrap();
        assert_eq!(module.entry_points.len(), 5);
        #[cfg(target_os = "macos")]
        assert!(super::metal_source().unwrap().contains("solid_transparent"));
    }
}
