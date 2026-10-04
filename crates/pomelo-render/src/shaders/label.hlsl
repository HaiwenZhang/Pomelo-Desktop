// Native counterpart of the latest Web label.wgsl, using its unchanged MSDF atlas.
struct Glyph {float4 xywh;float4 uv;float4 color;float4 rotation;float4 low;uint4 ids;};
StructuredBuffer<Glyph> glyphs:register(t0);
Texture2D<float4> atlas:register(t0);
SamplerState linear_sampler:register(s0);
cbuffer Frame:register(b0){float4 viewport;float4 canvas;float4 clip_bounds;float4 camera;float4 view;float4 color;uint4 batch;float4 highlight;};
float2 ds_add(float2 a,float2 b){precise float sum=a.x+b.x;precise float v=sum-a.x;precise float error=(a.x-(sum-v))+(b.x-v)+a.y+b.y;precise float high=sum+error;return float2(high,error-(high-sum));}
struct Out{float4 position:SV_Position;float2 uv:TEXCOORD0;float4 color:COLOR0;};
Out trace_vertex(uint vertex_id:SV_VertexID,uint instance_id:SV_InstanceID){
    Glyph g=glyphs[batch.x+instance_id];float2 corner=float2(vertex_id&1u,(vertex_id>>1u)&1u);float2 d=corner*g.xywh.zw;
    float2 x=ds_add(float2(g.xywh.x,g.low.x),-camera.xz);float2 y=ds_add(float2(g.xywh.y,g.low.y),-camera.yw);
    float2 relative=float2(x.x+x.y,y.x+y.y)+float2((d.x*g.rotation.x-d.y*g.rotation.y)*g.rotation.z,d.x*g.rotation.y+d.y*g.rotation.x);
    float2 screen=canvas.xy+canvas.zw*.5+relative*float2(view.y,-1)*view.x;
    Out output;output.position=float4(screen/viewport.xy*float2(2,-2)+float2(-1,1),0,1);
    output.uv=lerp(float2(g.uv.x,g.uv.w),float2(g.uv.z,g.uv.y),corner);
    // Source board text uses its layer color; automatic labels carry calibrated colors.
    // Low bit is independent opacity; upper bits still select the unchanged atlas page.
    // Drill spans and via names share a batch, so classify opacity per glyph.
    float alpha=(uint(g.rotation.w)&1u)!=0u?1.0:view.z;
    output.color=g.color*(g.ids.y==PCB_SOURCE_TEXT_CATEGORY?color:float4(1,1,1,alpha));return output;
}
float4 trace_fragment(Out input):SV_Target{
    clip(input.position.xy-clip_bounds.xy);clip(clip_bounds.xy+clip_bounds.zw-input.position.xy);
    float3 sample=atlas.Sample(linear_sampler,input.uv).rgb;
    float distance=max(min(sample.r,sample.g),min(max(sample.r,sample.g),sample.b));
    uint w,h;atlas.GetDimensions(w,h);float2 atlas_size=float2(w,h);
    float dx=length(ddx(input.uv)*atlas_size);float dy=length(ddy(input.uv)*atlas_size);
    float screen_range=4.0/max(max(dx,dy),.0001);float softness=.5/max(screen_range,1.0);
    float body=smoothstep(.5-softness,.5+softness,distance);
    float3 overlay=view.w>1.5?float3(.63,1,.85):float3(1,1,1);
    return float4(view.w>.5?overlay:input.color.rgb,input.color.a*body);
}
