use super::*;

#[test]
fn copper_mesh_and_stencil_clear_shaders_compile() {
    let source = include_str!("../../../../src/shaders/d3d11/copper.hlsl");
    compile_shader(source, s!("copper.hlsl"), s!("copper_vertex"), s!("vs_5_0")).unwrap();
    compile_shader(source, s!("copper.hlsl"), s!("clear_vertex"), s!("vs_5_0")).unwrap();
    compile_shader(
        source,
        s!("copper.hlsl"),
        s!("copper_fragment"),
        s!("ps_5_0"),
    )
    .unwrap();
}
