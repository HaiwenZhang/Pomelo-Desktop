//! HLSL source selection and the synchronous D3D compiler boundary.
use super::super::Shader;
use anyhow::Context as _;
use windows::{
    Win32::Graphics::Direct3D::{
        Fxc::{D3DCOMPILE_IEEE_STRICTNESS, D3DCompile},
        ID3DBlob, ID3DInclude,
    },
    core::{PCSTR, s},
};

pub(super) fn source(shader: Shader) -> (String, PCSTR, PCSTR) {
    match shader {
        Shader::Trace => (
            include_str!("../../shaders/d3d11/trace.hlsl").into(),
            s!("trace_vertex"),
            s!("trace_fragment"),
        ),
        Shader::Pad => (
            include_str!("../../shaders/d3d11/pad.hlsl").into(),
            s!("pad_vertex"),
            s!("pad_fragment"),
        ),
        Shader::Text => (
            include_str!("../../shaders/d3d11/text.hlsl").into(),
            s!("trace_vertex"),
            s!("trace_fragment"),
        ),
        Shader::Label => (
            msdf_shader_source(),
            s!("trace_vertex"),
            s!("trace_fragment"),
        ),
        Shader::Copper => (
            include_str!("../../shaders/d3d11/copper.hlsl").into(),
            s!("copper_vertex"),
            s!("copper_fragment"),
        ),
        Shader::Probe => (
            include_str!("../../shaders/d3d11/pcb.hlsl").into(),
            s!("pcb_vertex"),
            s!("pcb_fragment"),
        ),
    }
}
pub(super) fn msdf_shader_source() -> String {
    format!(
        "#define PCB_SOURCE_TEXT_CATEGORY {}u\n{}",
        pomelo_core::display::DisplayCategory::Text as u32,
        include_str!("../../shaders/d3d11/label.hlsl")
    )
}
pub(super) fn compile_shader(
    source: &str,
    name: PCSTR,
    entry: PCSTR,
    target: PCSTR,
) -> anyhow::Result<ID3DBlob> {
    let source = crate::scene::colors::hlsl_library() + source;
    let mut code = None;
    let mut errors = None;
    // SAFETY: the owned UTF-8 source and out-pointers live for the synchronous compiler call.
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            name,
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            D3DCOMPILE_IEEE_STRICTNESS,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let details = errors
            .map(|blob| {
                // SAFETY: D3DBlob owns this byte range until the conversion completes.
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    )
                };
                String::from_utf8_lossy(bytes)
                    .trim_end_matches('\0')
                    .to_owned()
            })
            .unwrap_or_default();
        return Err(anyhow::Error::new(error).context(format!("GPU_SHADER_COMPILE: {details}")));
    }
    code.context("GPU_SHADER_BLOB_MISSING")
}
