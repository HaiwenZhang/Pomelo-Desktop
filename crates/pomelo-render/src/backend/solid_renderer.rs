#[cfg(target_os = "macos")]
use super::metal::SolidPipeline;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::solid_pipeline::SolidPipeline;
use super::{NativeGpuContext, NativeGpuRenderer};
use crate::scene::board3d::PreparedBoard3d;
use pomelo_core::{geometry3d::Bounds3, interaction::camera3d::Camera3d};
use std::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicU64};

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
    pipeline: Option<SolidPipeline>,
    source: Option<Arc<PreparedBoard3d>>,
    telemetry: Arc<SolidTelemetry>,
}
impl SolidRenderer {
    pub fn new(telemetry: Arc<SolidTelemetry>) -> Self {
        Self {
            pipeline: None,
            source: None,
            telemetry,
        }
    }
    pub fn supported() -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        ))
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
        if self
            .source
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(s, &frame.scene))
        {
            self.pipeline = Some(SolidPipeline::new(&context.device, &frame.scene)?);
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
    fn reset(&mut self) {
        self.pipeline = None;
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
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
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
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn hlsl_source() -> anyhow::Result<(String, [String; 5])> {
    use naga::back::hlsl;
    let (module, info) = validated()?;
    let options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        binding_map: [0, 3, 4, 5, 6]
            .map(|binding| {
                (
                    naga::ResourceBinding { group: 0, binding },
                    hlsl::BindTarget {
                        space: 0,
                        register: binding,
                        binding_array_size: None,
                        dynamic_storage_buffer_offsets_index: None,
                        restrict_indexing: false,
                    },
                )
            })
            .into(),
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut text = String::new();
    let reflection = hlsl::Writer::new(&mut text, &options, &hlsl::PipelineOptions::default())
        .write(&module, &info, None)?;
    let mut entries = [const { String::new() }; 5];
    for (output, wanted) in entries.iter_mut().zip([
        "solid_vertex",
        "solid_opaque",
        "solid_transparent",
        "composite_vertex",
        "composite_fragment",
    ]) {
        let index = module
            .entry_points
            .iter()
            .position(|entry| entry.name == wanted)
            .ok_or_else(|| anyhow::anyhow!("SOLID_HLSL_ENTRY: {wanted}"))?;
        *output = reflection.entry_point_names[index]
            .clone()
            .map_err(|error| anyhow::anyhow!("SOLID_HLSL_ENTRY {wanted}: {error}"))?;
    }
    Ok((text, entries))
}
#[cfg(test)]
mod tests {
    #[test]
    fn solid_shader_translates_all_depth_transparency_and_composite_entries_to_hlsl() {
        let (source, entries) = super::hlsl_source().unwrap();
        assert!(source.contains("register(b0)") && source.contains("register(t3)"));
        assert!(entries.iter().all(|entry| source.contains(entry)));
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn solid_shader_compiles_all_five_entries_for_d3d11() {
        use windows::core::{PCSTR, s};
        let (source, entries) = super::hlsl_source().unwrap();
        for (i, entry) in entries.iter().enumerate() {
            let entry = std::ffi::CString::new(entry.as_str()).unwrap();
            crate::backend::d3d11::compile_shader(
                &source,
                s!("solid test"),
                PCSTR(entry.as_ptr().cast()),
                if i == 0 || i == 3 {
                    s!("vs_5_0")
                } else {
                    s!("ps_5_0")
                },
            )
            .unwrap();
        }
    }
    #[test]
    fn solid_shader_validates_with_opaque_transparent_and_composite_entries() {
        let (module, _) = super::validated().unwrap();
        assert_eq!(module.entry_points.len(), 5);
        #[cfg(target_os = "macos")]
        assert!(super::metal_source().unwrap().contains("solid_transparent"));
    }
}
