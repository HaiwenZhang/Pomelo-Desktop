use super::*;

#[test]
fn analytic_pad_shaders_compile_for_both_d3d11_stages() {
    let source = include_str!("../../../../src/shaders/d3d11/pad.hlsl");
    compile_shader(source, s!("pad.hlsl"), s!("pad_vertex"), s!("vs_5_0")).unwrap();
    compile_shader(source, s!("pad.hlsl"), s!("pad_fragment"), s!("ps_5_0")).unwrap();
}

#[test]
fn real_trace_shaders_compile_for_both_d3d11_stages() {
    let source = include_str!("../../../../src/shaders/d3d11/trace.hlsl");
    compile_shader(source, s!("trace.hlsl"), s!("trace_vertex"), s!("vs_5_0")).unwrap();
    compile_shader(source, s!("trace.hlsl"), s!("trace_fragment"), s!("ps_5_0")).unwrap();
}
#[test]
fn web_msdf_label_shader_compiles_for_both_d3d11_stages() {
    let source = msdf_shader_source();
    compile_shader(&source, s!("label.hlsl"), s!("trace_vertex"), s!("vs_5_0")).unwrap();
    compile_shader(
        &source,
        s!("label.hlsl"),
        s!("trace_fragment"),
        s!("ps_5_0"),
    )
    .unwrap();
}
