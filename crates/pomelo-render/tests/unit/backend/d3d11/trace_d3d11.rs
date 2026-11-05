use super::*;
use windows::core::PCSTR;

#[test]
fn every_wgsl_shader_compiles_for_d3d11() {
    for shader in [
        Shader::Trace,
        Shader::Pad,
        Shader::Text,
        Shader::Label,
        Shader::Copper,
        Shader::Probe,
    ] {
        let source = source(shader).unwrap();
        for (entry, target) in [
            (Some(source.vertex.as_str()), s!("vs_5_0")),
            (Some(source.fragment.as_str()), s!("ps_5_0")),
            (source.clear.as_deref(), s!("vs_5_0")),
        ] {
            if let Some(entry) = entry {
                let entry = std::ffi::CString::new(entry).unwrap();
                compile_shader(
                    &source.text,
                    s!("generated WGSL"),
                    PCSTR(entry.as_ptr().cast()),
                    target,
                )
                .unwrap_or_else(|error| panic!("{shader:?}: {error:#}"));
            }
        }
    }
}
