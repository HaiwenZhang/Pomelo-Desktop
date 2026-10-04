use super::*;
#[test]
#[ignore = "requires a macOS Metal device"]
fn all_metal_shader_sources_compile() {
    let device = ::metal::Device::system_default().expect("Metal device");
    let options = ::metal::CompileOptions::new();
    options.set_fast_math_enabled(false);
    for shader in [
        Shader::Trace,
        Shader::Pad,
        Shader::Text,
        Shader::Label,
        Shader::Copper,
        Shader::Probe,
    ] {
        device
            .new_library_with_source(source(shader), &options)
            .unwrap_or_else(|e| panic!("MSL {shader:?}: {e}"));
    }
}
