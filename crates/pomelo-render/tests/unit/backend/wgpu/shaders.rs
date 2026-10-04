use super::*;
#[test]
fn all_wgpu_shaders_validate() {
    for shader in [
        Shader::Trace,
        Shader::Pad,
        Shader::Text,
        Shader::Label,
        Shader::Copper,
        Shader::Probe,
    ] {
        validated(shader).unwrap_or_else(|e| panic!("WGSL {shader:?}: {e:#}"));
    }
}
