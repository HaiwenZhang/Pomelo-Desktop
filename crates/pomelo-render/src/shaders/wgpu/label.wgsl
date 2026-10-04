struct Glyph {xywh: vec4f, uv: vec4f, color: vec4f, rotation: vec4f, low: vec4f, ids: vec4u}
@group(0) @binding(1) var<storage, read> glyphs: array<Glyph>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;
struct Out {@builtin(position) position: vec4f, @location(0) uv: vec2f, @location(1) color: vec4f}
@vertex fn vertex_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> Out {
 let g = glyphs[u.batch.x + iid]; let corner = quad_corner(vid); let d = corner * g.xywh.zw;
 let x = ds_add(vec2f(g.xywh.x, g.low.x), -u.camera.xz); let y = ds_add(vec2f(g.xywh.y, g.low.y), -u.camera.yw);
 let r = vec2f(x.x + x.y, y.x + y.y) + vec2f((d.x * g.rotation.x - d.y * g.rotation.y) * g.rotation.z, d.x * g.rotation.y + d.y * g.rotation.x);
 var o: Out; o.position = screen_position(u.canvas.xy + u.canvas.zw * 0.5 + r * vec2f(u.view.y, -1.0) * u.view.x);
 o.uv = mix(vec2f(g.uv.x, g.uv.w), vec2f(g.uv.z, g.uv.y), corner);
 let alpha = select(u.view.z, 1.0, (u32(g.rotation.w) & 1u) != 0u);
 o.color = g.color * select(vec4f(1.0, 1.0, 1.0, alpha), u.color, g.ids.y == PCB_SOURCE_TEXT_CATEGORY); return o;
}
@fragment fn fragment_main(i: Out) -> @location(0) vec4f {
 // Compute derivatives before divergent clip branches.
 let dimensions = vec2f(textureDimensions(atlas));
 let dx = length(dpdx(i.uv) * dimensions); let dy = length(dpdy(i.uv) * dimensions);
 let sample = textureSample(atlas, linear_sampler, i.uv).rgb;
 let distance = max(min(sample.r, sample.g), min(max(sample.r, sample.g), sample.b));
 let screen_range = 4.0 / max(max(dx, dy), 0.0001); let softness = 0.5 / max(screen_range, 1.0);
 let body = smoothstep(0.5 - softness, 0.5 + softness, distance);
 if clipped(i.position.xy) {discard;}
 let overlay = select(vec3f(1.0), vec3f(0.63, 1.0, 0.85), u.view.w > 1.5);
 return vec4f(select(i.color.rgb, overlay, u.view.w > 0.5), i.color.a * body);
}
