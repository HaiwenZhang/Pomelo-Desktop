// 16-byte position ABI; each indexed vertex belongs to one independent coverage mesh.
StructuredBuffer<float4> vertices : register(t0);
cbuffer Frame : register(b0) {
    float4 viewport; float4 canvas; float4 clip_bounds;
    float4 camera; float4 view; float4 color; float4 rectangle;
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
float4 copper_fragment(float4 position : SV_Position) : SV_Target {
    clip(position.xy - clip_bounds.xy);
    clip(clip_bounds.xy + clip_bounds.zw - position.xy);
    if (view.w > 1.5) return float4(0.63,1,0.85,0.18);
    if (view.w > 0.5) {
        float2 screen = position.xy / viewport.z;
        float2 cell = screen - floor(screen / 5.0) * 5.0 - 2.5;
        return float4(1,1,1,(1.0 - smoothstep(0.65,1.25,length(cell))) * 0.8);
    }
    return color;
}
