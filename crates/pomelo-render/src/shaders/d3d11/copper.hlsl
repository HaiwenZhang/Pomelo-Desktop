// 16-byte position ABI; each indexed vertex belongs to one independent coverage mesh.
StructuredBuffer<float4> vertices : register(t0);
cbuffer Frame : register(b0) {
    float4 viewport; float4 canvas; float4 clip_bounds;
    float4 camera; float4 view; float4 color; float4 rectangle;
    uint4 pattern_mask[4];
    float4 inherited;
};
float2 ds_add(float2 a, float2 b) {
    precise float sum = a.x + b.x;
    precise float v = sum - a.x;
    precise float error = (a.x - (sum - v)) + (b.x - v) + a.y + b.y;
    precise float high = sum + error;
    return float2(high, error - (high - sum));
}
float4 screen_position(float2 screen) {
    return float4(screen / viewport.xy * float2(2,-2) + float2(-1,1), 0, 1);
}
float4 copper_vertex(uint id : SV_VertexID) : SV_Position {
    float4 p = vertices[id];
    float2 x = ds_add(p.xz, -camera.xz);
    float2 y = ds_add(p.yw, -camera.yw);
    float2 relative = float2(x.x + x.y, y.x + y.y);
    return screen_position(canvas.xy + canvas.zw * 0.5 + relative * float2(view.y,-1) * view.x);
}
float4 clear_vertex(uint id : SV_VertexID) : SV_Position {
    float2 corner = float2(id & 1u, (id >> 1u) & 1u);
    return screen_position(lerp(rectangle.xy, rectangle.zw, corner));
}
float4 fragment_color(float4 position) {
    clip(position.xy - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - position.xy);
    if (view.w > 1.5) return float4(0.63,1,0.85,0.18);
    if (view.w > 0.5 && view.z > 3.5) {
        bool dynamic = view.z > 4.5;
        float2 sample = position.xy - canvas.xy;
        if (!dynamic) sample /= viewport.z;
        uint2 pixel = uint2(max(floor(sample), 0.0)) & 15u;
        uint row = pattern_mask[pixel.y >> 2u][pixel.y & 3u];
        float ink = float((row >> pixel.x) & 1u);
        // Dynamic gaps also receive the original material at shape alpha.
        // Static gaps stay transparent, retaining the unchanged sparse fill.
        if (dynamic) return float4(lerp(color.rgb, float3(1,1,1), ink), color.a);
        return float4(color.rgb, color.a * ink);
    }
    if (view.w > 0.5 && view.z > 1.5) {
        // Network highlighting has a separate physical-pixel bitmap. The
        // observed Windows reference does not scale it like static base stipple.
        uint2 pixel = uint2(max(floor(position.xy - canvas.xy), 0.0)) & 15u;
        uint row = pattern_mask[pixel.y >> 2u][pixel.y & 3u];
        return float4(color.rgb, color.a * float((row >> pixel.x) & 1u));
    }
    if (view.w > 0.5) {
        float2 screen = position.xy / viewport.z;
        float2 cell = screen - floor(screen / 5.0) * 5.0 - 2.5;
        return float4(1,1,1,(1.0 - smoothstep(0.65,1.25,length(cell))) * 0.8);
    }
    float4 fill = color;
    if (view.z > 2.5) {
        // Suppress only base color, preserving complete coverage for labels.
        fill.a = 0;
    } else if (view.z > 0.5) {
        // Keep the 16-pixel pattern in logical screen coordinates, matching
        // Allegro at Windows DPI scaling without changing zoom or board geometry.
        uint2 pixel = uint2(max(floor((position.xy - canvas.xy) / viewport.z), 0.0)) & 15u;
        uint row = pattern_mask[pixel.y >> 2u][pixel.y & 3u];
        // Preserve full stencil coverage. Discarding gaps would also punch gaps
        // in mask writes and in the independently clipped embedded labels.
        fill.a *= float((row >> pixel.x) & 1u);
    }
    return fill;
}

float4 copper_fragment(float4 position : SV_Position) : SV_Target { float4 result = fragment_color(position); return float4(result.rgb, result.a * inherited.x); }
