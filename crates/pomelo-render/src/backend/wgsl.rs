//! One WGSL source for all native backends; naga translates Metal and D3D11.
use crate::backend::Shader;
const FRAME: &str = "struct Frame {viewport: vec4f, canvas: vec4f, clip_bounds: vec4f, camera: vec4f, view: vec4f, color: vec4f, batch: vec4u, highlight: vec4f, inherited: vec4f, material_pattern: array<vec4u,4>}\n@group(0) @binding(0) var<uniform> u: Frame;\n";
const COPPER_FRAME: &str = "struct Frame {viewport: vec4f, canvas: vec4f, clip_bounds: vec4f, camera: vec4f, view: vec4f, color: vec4f, rectangle: vec4f, pattern_mask: array<vec4u, 4>, inherited: vec4f, material_pattern: array<vec4u,4>}\n@group(0) @binding(0) var<uniform> u: Frame;\n";
pub(super) fn source(shader: Shader) -> String {
    if shader == Shader::Probe {
        return with_opacity(include_str!("../shaders/probe.wgsl").to_owned(), shader);
    }
    let body = match shader {
        Shader::Trace => include_str!("../shaders/trace.wgsl"),
        Shader::Pad => include_str!("../shaders/pad.wgsl"),
        Shader::Text => include_str!("../shaders/text.wgsl"),
        Shader::Label => include_str!("../shaders/label.wgsl"),
        Shader::Probe => unreachable!(),
        Shader::Copper => include_str!("../shaders/copper.wgsl"),
    };
    with_opacity(
        format!(
            "{}\n{}\nconst PCB_SOURCE_TEXT_CATEGORY: u32 = {}u;\n{}\n{}",
            if shader == Shader::Copper {
                COPPER_FRAME
            } else {
                FRAME
            },
            crate::scene::colors::wgsl_library(),
            pomelo_core::display::DisplayCategory::Text as u32,
            include_str!("../shaders/common.wgsl"),
            body
        ),
        shader,
    )
}
fn with_opacity(mut source: String, shader: Shader) -> String {
    if shader == Shader::Probe {
        source.push_str("\n@fragment fn fragment_main(i: Out) -> @location(0) vec4f {let result=fragment_color(i);return vec4f(result.rgb*u.inherited.y,result.a*u.inherited.x); }\n");
        return source;
    }

    // Shader bodies define fragment_color; only this wrapper is an entry point.
    let (argument, value) = if shader == Shader::Copper {
        ("@builtin(position) position: vec4f", "position")
    } else {
        ("i: Out", "i")
    };
    let position = if shader == Shader::Copper {
        "position"
    } else {
        "i.position"
    };
    source.push_str(&format!("\n@fragment fn fragment_main({argument}) -> @location(0) vec4f {{let result = fragment_color({value}); let cell=vec2u(max(floor(({position}.xy-u.canvas.xy)/u.viewport.z),vec2f(0.0)))&vec2u(15u); let row=u.material_pattern[cell.y>>2u][cell.y&3u]; let ink=f32((row>>cell.x)&1u); return vec4f(result.rgb * u.inherited.y, result.a * u.inherited.x * ink);}}\n"));
    source
}
#[cfg(any(test, target_os = "windows", target_os = "macos"))]
pub(super) fn validated(shader: Shader) -> anyhow::Result<(naga::Module, naga::valid::ModuleInfo)> {
    let source = source(shader);
    let module = naga::front::wgsl::parse_str(&source)
        .map_err(|error| anyhow::anyhow!("{}", error.emit_to_string(&source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| anyhow::anyhow!("{}", error.emit_to_string(&source)))?;
    Ok((module, info))
}

/// WGSL resource bindings shared by every pipeline, in group 0.
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
mod binding {
    pub const FRAME: u32 = 0;
    pub const GEOMETRY: u32 = 1;
    pub const TEXTURE: u32 = 2;
    pub const SAMPLER: u32 = 3;
}

/// Metal Shading Language for one variant. Slots match the Metal driver:
/// uniforms buffer(0), geometry buffer(1), runtime sizes buffer(2),
/// texture(0) and sampler(0).
#[cfg(any(test, target_os = "macos"))]
pub(super) fn msl(shader: Shader) -> anyhow::Result<String> {
    use naga::back::msl;
    let (module, info) = validated(shader)?;
    let resource = |binding| naga::ResourceBinding { group: 0, binding };
    let buffer = |slot| msl::BindTarget {
        buffer: Some(slot),
        ..Default::default()
    };
    let resources = msl::EntryPointResources {
        resources: [
            (resource(binding::FRAME), buffer(0)),
            (resource(binding::GEOMETRY), buffer(1)),
            (
                resource(binding::TEXTURE),
                msl::BindTarget {
                    texture: Some(0),
                    ..Default::default()
                },
            ),
            (
                resource(binding::SAMPLER),
                msl::BindTarget {
                    sampler: Some(msl::BindSamplerTarget::Resource(0)),
                    ..Default::default()
                },
            ),
        ]
        .into(),
        sizes_buffer: Some(2),
        ..Default::default()
    };
    let options = msl::Options {
        lang_version: (2, 4),
        per_entry_point_map: module
            .entry_points
            .iter()
            .map(|entry| (entry.name.clone(), resources.clone()))
            .collect(),
        fake_missing_bindings: false,
        ..Default::default()
    };
    let (source, _) = msl::write_string(&module, &info, &options, &msl::PipelineOptions::default())
        .map_err(|error| anyhow::anyhow!("MSL {shader:?}: {error}"))?;
    Ok(source)
}

/// Shader Model 5.0 HLSL and its translated entry point names.
#[cfg(any(test, target_os = "windows"))]
pub(super) struct HlslSource {
    pub text: String,
    pub vertex: String,
    pub fragment: String,
    /// Copper's clear-quad vertex entry point.
    pub clear: Option<String>,
}

/// HLSL for one variant. Registers match the D3D11 driver: cbuffer b0,
/// geometry t0 (vertex stage), texture t2 and sampler s0.
#[cfg(any(test, target_os = "windows"))]
pub(super) fn hlsl(shader: Shader) -> anyhow::Result<HlslSource> {
    use naga::back::hlsl;
    let (module, info) = validated(shader)?;
    let resource = |binding| naga::ResourceBinding { group: 0, binding };
    let register = |register| hlsl::BindTarget {
        space: 0,
        register,
        binding_array_size: None,
        dynamic_storage_buffer_offsets_index: None,
        restrict_indexing: false,
    };
    let options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        binding_map: [
            (resource(binding::FRAME), register(0)),
            (resource(binding::GEOMETRY), register(0)),
            (resource(binding::TEXTURE), register(2)),
            // Sampler registers index naga's heap table; `bind_d3d11_samplers` rewrites it.
            (resource(binding::SAMPLER), register(0)),
        ]
        .into(),
        // The index table is removed with the heap; it needs only a placeholder.
        sampler_buffer_binding_map: [(hlsl::SamplerIndexBufferKey { group: 0 }, register(0))]
            .into(),
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut text = String::new();
    let pipeline = hlsl::PipelineOptions::default();
    let reflection = hlsl::Writer::new(&mut text, &options, &pipeline)
        .write(&module, &info, None)
        .map_err(|error| anyhow::anyhow!("HLSL {shader:?}: {error}"))?;
    let name = |wanted: &str| -> anyhow::Result<Option<String>> {
        let Some(index) = module
            .entry_points
            .iter()
            .position(|entry| entry.name == wanted)
        else {
            return Ok(None);
        };
        reflection.entry_point_names[index]
            .clone()
            .map(Some)
            .map_err(|error| anyhow::anyhow!("HLSL {shader:?} {wanted}: {error}"))
    };
    Ok(HlslSource {
        text: bind_d3d11_samplers(&text)?,
        vertex: name("vertex_main")?.ok_or_else(|| anyhow::anyhow!("HLSL {shader:?}: vertex"))?,
        fragment: name("fragment_main")?
            .ok_or_else(|| anyhow::anyhow!("HLSL {shader:?}: fragment"))?,
        clear: name("clear_main")?,
    })
}

/// naga emits D3D12-style sampler heaps in register spaces, which Shader Model
/// 5.0 lacks. Each shader binds at most one sampler, so declare it directly.
#[cfg(any(test, target_os = "windows"))]
pub(super) fn bind_d3d11_samplers(text: &str) -> anyhow::Result<String> {
    const HEAP: &str = "nagaSamplerHeap[nagaGroup0SamplerIndexArray[";
    let mut output = String::with_capacity(text.len());
    let mut samplers = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("SamplerState nagaSamplerHeap[")
            || trimmed.starts_with("SamplerComparisonState nagaComparisonSamplerHeap[")
            || trimmed.starts_with("StructuredBuffer<uint> nagaGroup0SamplerIndexArray")
        {
            continue;
        }
        if let Some(declaration) = trimmed.strip_prefix("static const SamplerState ")
            && let Some((name, value)) = declaration.split_once(" = ")
            && value.starts_with(HEAP)
        {
            anyhow::ensure!(samplers == 0, "HLSL_SAMPLER_COUNT");
            output.push_str(&format!("SamplerState {name} : register(s0);\n"));
            samplers += 1;
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    anyhow::ensure!(
        !output.contains("nagaSamplerHeap")
            && !output.contains("nagaComparisonSamplerHeap")
            && !output.contains(", space"),
        "HLSL_SAMPLER_HEAP_REMAINS"
    );
    Ok(output)
}

#[cfg(test)]
#[path = "../../tests/unit/backend/wgsl.rs"]
mod tests;
