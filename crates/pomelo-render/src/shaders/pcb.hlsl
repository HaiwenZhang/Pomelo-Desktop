// Primitive distance fields adapted from Pomelo Web's primitive.wgsl.
// Test-only rectangular zone difference; arbitrary production zones need triangulation.
cbuffer Frame : register(b0) {
    float4 viewport;
    float4 canvas;
    float4 clip_bounds;
    float4 shape_a;
    float4 shape_b;
    float4 shape_color;
    uint4 shape_kind;
};

struct VertexOutput { float4 position : SV_Position; float3 color : COLOR0; };

VertexOutput pcb_vertex(uint vertex_id : SV_VertexID) {
    float2 uv = float2(vertex_id & 1u, (vertex_id >> 1u) & 1u);
    if (shape_kind.x == 4u) {
        uv = vertex_id == 0u ? float2(0.5, 0.1) : vertex_id == 1u ? float2(0.1, 0.9) : float2(0.9, 0.9);
    }
    float2 position = canvas.xy + uv * canvas.zw;
    VertexOutput output;
    output.position = float4(position / viewport.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    uint color_id = (vertex_id + shape_kind.y) % 3u;
    output.color = color_id == 0u ? float3(1,0,0) : color_id == 1u ? float3(0,1,0) : float3(0,0,1);
    return output;
}

float box_distance(float2 position, float4 box) {
    float2 delta = abs(position - box.xy) - box.zw;
    return length(max(delta, 0.0)) + min(max(delta.x, delta.y), 0.0);
}

float4 pcb_fragment(VertexOutput input) : SV_Target {
    float2 p = input.position.xy;
    clip(p - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - p);
    if (shape_kind.x == 4u) return float4(input.color, 1);
    if (shape_kind.x == 3u) {
        float2 cell = abs(frac((p - canvas.xy) / 24.0) - 0.5) * 24.0;
        float grid_coverage = 1.0 - smoothstep(0.4, 1.2, min(cell.x, cell.y));
        return float4(lerp(float3(0.025,0.045,0.055), float3(0.09,0.14,0.17), grid_coverage), 1);
    }
    float distance;
    if (shape_kind.x == 0u) {
        float2 ab = shape_a.zw - shape_a.xy;
        float2 ap = p - shape_a.xy;
        float t = saturate(dot(ap, ab) / max(dot(ab, ab), 1e-20));
        distance = length(ap - ab * t) - shape_b.x;
    } else if (shape_kind.x == 1u) {
        distance = length(p - shape_a.xy) - shape_a.z;
    } else {
        // Difference keeps the inner square transparent, exposing the earlier grid draw.
        distance = max(box_distance(p, shape_a), -box_distance(p, shape_b));
    }
    float coverage = 1.0 - smoothstep(-0.75, 0.75, distance);
    clip(coverage - 0.001);
    return float4(shape_color.rgb, shape_color.a * coverage);
}
