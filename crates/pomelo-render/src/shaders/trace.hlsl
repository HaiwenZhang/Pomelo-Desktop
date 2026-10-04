// Real analytic tracks; the 128-byte ABI matches tracks::TraceInstance.
// Compensated distances follow Pomelo Web arc.wgsl and line-frame.wgsl.
struct Trace {
    float4 a; float4 b; float4 center; float4 arc;
    float4 bounds_min; float4 bounds_max;
    uint4 ids; uint4 flags;
};
StructuredBuffer<Trace> traces : register(t0);
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
float4 tangent(float4 p, float4 center) {
    float2 x = ds_add(p.xz, -center.xz);
    float2 y = ds_add(p.yw, -center.yw);
    return float4(-y.x, x.x, -y.y, x.y);
}
float precise_dot(float4 a, float4 b) {
    float2 d = ds_add(ds_mul(a.xz, b.xz), ds_mul(a.yw, b.yw));
    return d.x + d.y;
}
struct Out {
    float4 position : SV_Position;
    nointerpolation float4 a : TEXCOORD0;
    nointerpolation float4 b : TEXCOORD1;
    nointerpolation float4 center : TEXCOORD2;
    nointerpolation float4 arc : TEXCOORD3;
    nointerpolation uint4 flags : TEXCOORD4;
    nointerpolation float4 tint : TEXCOORD5;
};
Out trace_vertex(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID) {
    Trace trace = traces[batch.x + instance_id];
    float2 lo = relative(trace.bounds_min).xy + relative(trace.bounds_min).zw;
    float2 hi = relative(trace.bounds_max).xy + relative(trace.bounds_max).zw;
    // Clamp raster bounds in camera-relative space to preserve deep-zoom precision.
    float2 limit = canvas.zw * 0.5 / view.x;
    lo = clamp(lo - 2.0 * viewport.z / view.x, -limit, limit);
    hi = clamp(hi + 2.0 * viewport.z / view.x, -limit, limit);
    float2 corner = float2(vertex_id & 1u, (vertex_id >> 1u) & 1u);
    float2 screen = canvas.xy + canvas.zw * 0.5 + lerp(lo, hi, corner) * float2(view.y, -1) * view.x;
    Out output;
    output.position = float4(screen / viewport.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    output.a = relative(trace.a); output.b = relative(trace.b);
    output.center = relative(trace.center); output.arc = trace.arc; output.flags = trace.flags;
    bool selected = (batch.z == 1u && trace.ids.w == batch.y)
        || (batch.z == 2u && trace.ids.x == batch.y)
        || (batch.z == 3u && trace.ids.y == batch.y);
    float4 material = color;
    if (view.z != 0 && trace.ids.w != 0u && (trace.flags.w & 4u) == 0u)
        material.rgb = pcb_net_color(trace.ids.w);
    output.tint = selected && (trace.flags.w & 4u) == 0u ? highlight : material;
    // Test with ordinary coordinates first; short/offscreen lines keep the cheap path.
    if (trace.flags.x == 0u && !any(lo >= hi)
        && length((output.b.xy + output.b.zw) - (output.a.xy + output.a.zw)) * view.x / viewport.z > 16384.0) {
        float2 dx = ds_add(output.b.xz, -output.a.xz);
        float2 dy = ds_add(output.b.yw, -output.a.yw);
        float4 d = float4(dx.x, dy.x, dx.y, dy.y);
        float magnitude = length(d.xy + d.zw);
        // A distant endpoint loses subpixel precision in ap - ab*t.
        // Keep the signed normal offset and cap projections instead;
        // this is the Web primitive.wgsl long-line frame, in world mm.
        float2 cross = ds_add(ds_mul(output.a.xz, d.yw), -ds_mul(output.a.yw, d.xz));
        float start = precise_dot(output.a, d) / magnitude;
        float end = precise_dot(output.b, d) / magnitude;
        output.a = float4((d.xy + d.zw) / magnitude, (cross.x + cross.y) / magnitude, start);
        output.b.x = end;
        // Vertex-local tag only; the immutable 128-byte instance ABI is unchanged.
        output.flags.x = 2u;
    }
    return output;
}
float4 trace_fragment(Out input) : SV_Target {
    float2 screen_px = input.position.xy;
    clip(screen_px - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - screen_px);
    // Only reliable Dynamic Base boundaries use this rule. Static, Unknown,
    // selection and hover retain their existing analytic outline coverage.
    bool dynamic_base = (input.flags.w & 128u) != 0u && view.w < 0.5 && batch.w != 0u;
    if (dynamic_base && batch.w == 2u) discard;
    float2 screen = (screen_px - canvas.xy - canvas.zw * 0.5) * float2(view.y, -1) / view.x;
    float4 a = delta(screen, input.a);
    float4 b = delta(screen, input.b);
    float distance;
    if (input.flags.x == 2u) {
        float along = dot(screen, input.a.xy);
        float normal = dot(screen, float2(-input.a.y, input.a.x)) + input.a.z;
        float cap = max(max(input.a.w - along, along - input.b.x), 0.0);
        distance = length(float2(cap, normal));
    } else if (input.flags.x == 0u) {
        float2 ab = (a.xy + a.zw) - (b.xy + b.zw);
        float2 ap = a.xy + a.zw;
        float t = saturate(dot(ap, ab) / max(dot(ab, ab), 1e-30));
        distance = length(ap - ab * t);
    } else {
        float4 p = delta(screen, input.center);
        float2 r = input.arc.xy;
        float2 square = ds_add(ds_add(ds_mul(p.xz, p.xz), ds_mul(p.yw, p.yw)), -ds_mul(r, r));
        float radial = (square.x + square.y) / max(length(p.xy + p.zw) + r.x + r.y, 1e-30);
        float direction = sign(input.arc.w);
        bool after = precise_dot(a, tangent(input.a, input.center)) * direction >= 0;
        bool before = precise_dot(b, tangent(input.b, input.center)) * direction <= 0;
        bool full = (input.flags.w & 1u) != 0u;
        bool long_arc = (input.flags.w & 2u) != 0u;
        bool inside = full || (direction != 0 && (long_arc ? after || before : after && before));
        distance = inside ? abs(radial) : min(length(a.xy + a.zw), length(b.xy + b.zw));
    }
    float pixel_mm = viewport.z / view.x;
    float width = (input.flags.w & 4u) != 0u ? 0.0 : asfloat(input.flags.z);
    float antialias_mm = dynamic_base ? 0.5 / view.x : pixel_mm * 0.65;
    distance -= dynamic_base ? 0.5 / view.x : (input.flags.w & 8u) != 0u ? pixel_mm * 0.65 : max(width * 0.5, pixel_mm * 0.5);
    float alpha = 1.0 - smoothstep(-antialias_mm, antialias_mm, distance);
    if (view.w > 0.5) {
        float edge = 1.0 - smoothstep(pixel_mm * 0.6, pixel_mm * 1.6, abs(distance));
        float2 screen = input.position.xy / viewport.z;
        float2 cell = screen - floor(screen / 5.0) * 5.0 - 2.5;
        float dots = 1.0 - smoothstep(0.65, 1.25, length(cell));
        float coverage = max(edge, view.w > 1.5 && view.w < 2.5 ? 0.0 : dots * alpha);
        return float4(view.w > 1.5 ? float3(0.63, 1.0, 0.85) : float3(1,1,1), coverage * 0.9);
    }
    clip(alpha - 0.001);
    return float4(input.tint.rgb, input.tint.a * alpha);
}
