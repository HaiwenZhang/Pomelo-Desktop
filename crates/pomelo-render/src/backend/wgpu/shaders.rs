//! WGSL source assembly and validation for the wgpu backend.
use crate::backend::Shader;
const FRAME: &str = "struct Frame {viewport: vec4f, canvas: vec4f, clip_bounds: vec4f, camera: vec4f, view: vec4f, color: vec4f, batch: vec4u, highlight: vec4f, inherited: vec4f, material_pattern: array<vec4u,4>}\n@group(0) @binding(0) var<uniform> u: Frame;\n";
const COPPER_FRAME: &str = "struct Frame {viewport: vec4f, canvas: vec4f, clip_bounds: vec4f, camera: vec4f, view: vec4f, color: vec4f, rectangle: vec4f, pattern_mask: array<vec4u, 4>, inherited: vec4f, material_pattern: array<vec4u,4>}\n@group(0) @binding(0) var<uniform> u: Frame;\n";
pub(super) fn source(shader: Shader) -> String {
    if shader == Shader::Probe {
        return with_opacity(
            include_str!("../../shaders/wgpu/probe.wgsl").to_owned(),
            shader,
        );
    }
    let body = match shader {
        Shader::Trace => include_str!("../../shaders/wgpu/trace.wgsl"),
        Shader::Pad => include_str!("../../shaders/wgpu/pad.wgsl"),
        Shader::Text => include_str!("../../shaders/wgpu/text.wgsl"),
        Shader::Label => include_str!("../../shaders/wgpu/label.wgsl"),
        Shader::Probe => unreachable!(),
        Shader::Copper => include_str!("../../shaders/wgpu/copper.wgsl"),
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
            include_str!("../../shaders/wgpu/common.wgsl"),
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
#[cfg(test)]
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
#[cfg(test)]
#[path = "../../../tests/unit/backend/wgpu/shaders.rs"]
mod tests;
