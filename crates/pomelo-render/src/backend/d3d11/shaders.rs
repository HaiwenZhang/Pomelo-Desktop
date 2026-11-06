//! The synchronous D3D compiler boundary for naga-translated HLSL.
pub(super) use super::super::wgsl::hlsl as source;
use anyhow::Context as _;
use windows::{
    Win32::Graphics::Direct3D::{
        Fxc::{D3DCOMPILE_IEEE_STRICTNESS, D3DCompile},
        ID3DBlob, ID3DInclude,
    },
    core::PCSTR,
};

pub(crate) fn compile_shader(
    source: &str,
    name: PCSTR,
    entry: PCSTR,
    target: PCSTR,
) -> anyhow::Result<ID3DBlob> {
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
