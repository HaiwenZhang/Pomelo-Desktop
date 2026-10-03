// Matches pads::PadInstance: eight 16-byte vectors, 128 bytes per instance.
struct Pad {
    float4 center; float4 shape; float4 rotation;
    float4 bounds_min; float4 bounds_max;
    uint4 ids; uint4 source; float4 drill;
};
StructuredBuffer<Pad> pads : register(t0);
cbuffer Frame : register(b0) {
    float4 viewport; float4 canvas; float4 clip_bounds;
    float4 camera; float4 view; float4 color; uint4 batch; float4 highlight;
};
float2 ds_add(float2 a, float2 b) {
    precise float sum = a.x + b.x;
    precise float v = sum - a.x;
    precise float error = (a.x - (sum - v)) + (b.x - v) + a.y + b.y;
    precise float high = sum + error;
    return float2(high, error - (high - sum));
}
float4 relative(float4 p) {
    float2 x = ds_add(p.xz, -camera.xz);
    float2 y = ds_add(p.yw, -camera.yw);
    return float4(x.x, y.x, x.y, y.y);
}
struct Out {
    float4 position : SV_Position;
    nointerpolation float4 center : TEXCOORD0;
    nointerpolation float4 shape : TEXCOORD1;
    nointerpolation float2 rotation : TEXCOORD2;
    nointerpolation uint kind : TEXCOORD3;
    nointerpolation float4 tint : TEXCOORD4;
    nointerpolation uint source_flags : TEXCOORD5;
};
Out pad_vertex(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID) {
    Pad pad = pads[batch.x + instance_id];
    float4 low = relative(pad.bounds_min), high = relative(pad.bounds_max);
    float2 limit = canvas.zw * 0.5 / view.x;
    float2 lo = clamp(low.xy + low.zw - 2.0 * viewport.z / view.x, -limit, limit);
    float2 hi = clamp(high.xy + high.zw + 2.0 * viewport.z / view.x, -limit, limit);
    float2 corner = float2(vertex_id & 1u, (vertex_id >> 1u) & 1u);
    float2 screen = canvas.xy + canvas.zw * 0.5 + lerp(lo, hi, corner) * float2(view.y,-1) * view.x;
    Out output;
    output.position = float4(screen / viewport.xy * float2(2,-2) + float2(-1,1), 0, 1);
    output.center = relative(pad.center);
    output.shape = pad.shape; output.rotation = pad.rotation.xy; output.kind = pad.ids.w;
    output.source_flags = pad.source.w;
    float4 material = color;
    if (view.z != 0 && pad.ids.z != 0u) material.rgb = pcb_net_color(pad.ids.z);
    output.tint = batch.w != 0u || (batch.z != 0u && pad.ids.z == batch.y) ? highlight : material;
    return output;
}
float4 pad_fragment(Out input) : SV_Target {
    clip(input.position.xy - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - input.position.xy);
    float2 board = (input.position.xy - canvas.xy - canvas.zw * 0.5) / view.x * float2(view.y,-1);
    float2 x = ds_add(float2(board.x,0), -input.center.xz);
    float2 y = ds_add(float2(board.y,0), -input.center.yw);
    float2 delta = float2(x.x+x.y, y.x+y.y);
    float2 p = float2(dot(delta,input.rotation), dot(delta,float2(-input.rotation.y,input.rotation.x)));
    float distance;
    if (input.kind == 2 || input.kind == 25) {
        float radius = length(p);
        distance = radius - input.shape.x * 0.5;
        if (input.kind == 25) distance = max(distance, input.shape.w * 0.5 - radius);
    } else {
        float2 q = abs(p) - input.shape.xy * 0.5;
        float corner = input.shape.z;
        if (input.kind == 3 || input.kind == 28)
            distance = max(max(q.x,q.y),(q.x+q.y+corner)*0.7071067811865475);
        else
            distance = length(max(q+corner,0)) + min(max(q.x+corner,q.y+corner),0) - corner;
    }
    float px = viewport.z / view.x;
    bool backdrill = (input.source_flags & 2u) != 0u;
    if ((viewport.w < 0.5 && !backdrill) || view.w > 0.5) distance = abs(distance) - px * 0.65;
    float coverage = 1.0 - smoothstep(-px * 0.65, px * 0.65, distance);
    if (view.w > 0.5) return float4(view.w > 1.5 ? float3(0.63,1,0.85) : float3(1,1,1), coverage * 0.9);
    if (backdrill) {
        float2 logical_screen = input.position.xy / viewport.z;
        float2 diagonal = float2(logical_screen.x + logical_screen.y, logical_screen.x - logical_screen.y);
        float2 cell = abs(frac(diagonal / 8.0) - 0.5) * 8.0;
        float hatch = 1.0 - smoothstep(0.35, 1.05, min(cell.x, cell.y));
        float annulus = smoothstep(-px * 0.65, px * 0.65, length(p) - input.shape.w * 0.5);
        float pattern_coverage = max(hatch, annulus);
        float3 pattern_color = lerp(float3(0,1,0), float3(1,1,1), hatch / max(pattern_coverage, 0.0001));
        return float4(pattern_color, input.tint.a * coverage * pattern_coverage);
    }
    return float4(input.tint.rgb, input.tint.a * coverage);
}
