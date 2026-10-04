struct Pad {center: vec4f, shape: vec4f, rotation: vec4f, bounds_min: vec4f, bounds_max: vec4f, ids: vec4u, source: vec4u, drill: vec4f}
@group(0) @binding(1) var<storage, read> pads: array<Pad>;
struct Out {
 @builtin(position) position: vec4f,
 @location(0) @interpolate(flat) center: vec4f,
 @location(1) @interpolate(flat) shape: vec4f,
 @location(2) @interpolate(flat) rotation: vec2f,
 @location(3) @interpolate(flat) kind: u32,
 @location(4) @interpolate(flat) tint: vec4f,
 @location(5) @interpolate(flat) source_flags: u32,
}
@vertex fn vertex_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> Out {
 let p = pads[u.batch.x + iid]; let low = relative(p.bounds_min); let high = relative(p.bounds_max);
 let limit = u.canvas.zw * 0.5 / u.view.x;
 let lo = clamp(low.xy + low.zw - vec2f(2.0 * u.viewport.z / u.view.x), -limit, limit);
 let hi = clamp(high.xy + high.zw + vec2f(2.0 * u.viewport.z / u.view.x), -limit, limit);
 var o: Out; o.position = screen_position(u.canvas.xy + u.canvas.zw * 0.5 + mix(lo, hi, quad_corner(vid)) * vec2f(u.view.y, -1.0) * u.view.x);
 if u.batch.w == 2u && p.ids.z != u.batch.y { o.position = vec4f(2.0, 2.0, 0.0, 1.0); }
 o.center = relative(p.center); o.shape = p.shape; o.rotation = p.rotation.xy; o.kind = p.ids.w; o.source_flags = p.source.w;
 var material = u.color; if u.view.z != 0.0 && p.ids.z != 0u {material = vec4f(pcb_net_color(p.ids.z), material.a);}
 o.tint = select(material, u.highlight, u.batch.w != 0u || (u.batch.z != 0u && p.ids.z == u.batch.y)); return o;
}
@fragment fn fragment_main(i: Out) -> @location(0) vec4f {
 if clipped(i.position.xy) {discard;}
 let board = board_position(i.position.xy); let x = ds_add(vec2f(board.x, 0.0), -i.center.xz); let y = ds_add(vec2f(board.y, 0.0), -i.center.yw);
 let d = vec2f(x.x + x.y, y.x + y.y); let p = vec2f(dot(d, i.rotation), dot(d, vec2f(-i.rotation.y, i.rotation.x)));
 var distance: f32;
 if i.kind == 2u || i.kind == 25u {let radius = length(p); distance = radius - i.shape.x * 0.5; if i.kind == 25u {distance = max(distance, i.shape.w * 0.5 - radius);}}
 else {let q = abs(p) - i.shape.xy * 0.5; let corner = i.shape.z;
 if i.kind == 3u || i.kind == 28u {distance = max(max(q.x, q.y), (q.x + q.y + corner) * 0.7071067811865475);}
 else {distance = length(max(q + vec2f(corner), vec2f(0.0))) + min(max(q.x + corner, q.y + corner), 0.0) - corner;}}
 let px = u.viewport.z / u.view.x; let backdrill = (i.source_flags & 2u) != 0u;
 if (u.viewport.w < 0.5 && !backdrill) || u.view.w > 0.5 {distance = abs(distance) - px * 0.65;}
 let coverage = 1.0 - smoothstep(-px * 0.65, px * 0.65, distance);
 if u.view.w > 0.5 {return vec4f(select(vec3f(1.0), vec3f(0.63, 1.0, 0.85), u.view.w > 1.5), coverage * 0.9);}
 if backdrill {
 let logical = i.position.xy / u.viewport.z; let diagonal = vec2f(logical.x + logical.y, logical.x - logical.y);
 let cell = abs(fract(diagonal / 8.0) - vec2f(0.5)) * 8.0;
 let hatch = 1.0 - smoothstep(0.35, 1.05, min(cell.x, cell.y)); let annulus = smoothstep(-px * 0.65, px * 0.65, length(p) - i.shape.w * 0.5);
 let pattern_coverage = max(hatch, annulus); let pattern_color = mix(vec3f(0.0, 1.0, 0.0), vec3f(1.0), hatch / max(pattern_coverage, 0.0001));
 return vec4f(pattern_color, i.tint.a * coverage * pattern_coverage);
 } return vec4f(i.tint.rgb, i.tint.a * coverage);
}
