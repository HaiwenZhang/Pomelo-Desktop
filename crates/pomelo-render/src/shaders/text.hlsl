// Compact text instances match text_instances::TextInstance (64 bytes).
struct TextStroke { float4 a; float4 b; uint4 ids; uint4 flags; };
StructuredBuffer<TextStroke> strokes : register(t0);
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
float2 ds_mul(float2 a, float2 b) {
    precise float ca = 4097.0 * a.x;
    precise float ah = ca - (ca - a.x);
    precise float al = a.x - ah;
    precise float cb = 4097.0 * b.x;
    precise float bh = cb - (cb - b.x);
    precise float bl = b.x - bh;
    precise float product = a.x * b.x;
    precise float error = ((ah * bh - product) + ah * bl + al * bh) + al * bl + a.x * b.y + a.y * b.x;
    return ds_add(float2(product, 0), float2(error, 0));
}
float4 relative(float4 p) {
    float2 x = ds_add(p.xz, -camera.xz);
    float2 y = ds_add(p.yw, -camera.yw);
    return float4(x.x, y.x, x.y, y.y);
}
float4 delta(float2 screen, float4 p) {
    float2 x = ds_add(float2(screen.x, 0), -p.xz);
    float2 y = ds_add(float2(screen.y, 0), -p.yw);
    return float4(x.x, y.x, x.y, y.y);
}

struct TextOut {
    float4 position : SV_Position;
    nointerpolation float4 a : TEXCOORD0;
    nointerpolation float4 b : TEXCOORD1;
    nointerpolation float width : TEXCOORD2;
};
TextOut trace_vertex(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID) {
    TextStroke stroke = strokes[batch.x + instance_id];
    TextOut output;
    output.a = relative(stroke.a);
    output.b = relative(stroke.b);
    output.width = asfloat(stroke.ids.w);
    float2 a = output.a.xy + output.a.zw;
    float2 b = output.b.xy + output.b.zw;
    float padding = output.width * 0.5 + 2.0 / view.x;
    float2 limit = canvas.zw * 0.5 / view.x;
    float2 lo = clamp(min(a, b) - padding, -limit, limit);
    float2 hi = clamp(max(a, b) + padding, -limit, limit);
    float2 corner = float2(vertex_id & 1u, (vertex_id >> 1u) & 1u);
    float2 screen = canvas.xy + canvas.zw * 0.5 + lerp(lo, hi, corner) * float2(view.y, -1) * view.x;
    output.position = float4(screen / viewport.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    return output;
}
float4 trace_fragment(TextOut input) : SV_Target {
    float2 screen_px = input.position.xy;
    clip(screen_px - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - screen_px);
    float2 screen = (screen_px - canvas.xy - canvas.zw * 0.5) * float2(view.y, -1) / view.x;
    float4 a = delta(screen, input.a);
    float4 b = delta(screen, input.b);
    float2 ab = (a.xy + a.zw) - (b.xy + b.zw);
    float2 ap = a.xy + a.zw;
    float t = saturate(dot(ap, ab) / max(dot(ab, ab), 1e-30));
    float distance = length(ap - ab * t);
    float pixel_mm = 1.0 / view.x;
    distance -= max(input.width * 0.5, pixel_mm * 0.5);
    float alpha = 1.0 - smoothstep(-pixel_mm * 0.65, pixel_mm * 0.65, distance);
    clip(alpha - 0.001);
    return float4(color.rgb, color.a * alpha);
}
