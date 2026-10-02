// Diagnostic raster content: vertex colors test interpolation and channel order.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.75),
        vec2<f32>(-0.75, -0.65),
        vec2<f32>(0.75, -0.65),
    );
    let colors = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.color = colors[index];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
