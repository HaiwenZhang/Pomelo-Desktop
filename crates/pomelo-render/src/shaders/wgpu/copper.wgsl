@group(0) @binding(1) var<storage, read> vertices: array<vec4f>;
@vertex fn vertex_main(@builtin(vertex_index) id: u32) -> @builtin(position) vec4f {
 let p = relative(vertices[id]); let r = p.xy + p.zw;
 return screen_position(u.canvas.xy + u.canvas.zw * 0.5 + r * vec2f(u.view.y, -1.0) * u.view.x);
}
@vertex fn clear_main(@builtin(vertex_index) id: u32) -> @builtin(position) vec4f {return screen_position(mix(u.rectangle.xy, u.rectangle.zw, quad_corner(id)));}
fn stipple(sample: vec2f) -> f32 {
 let pixel = vec2u(max(floor(sample), vec2f(0.0))) & vec2u(15u);
 let row = u.pattern_mask[pixel.y >> 2u][pixel.y & 3u]; return f32((row >> pixel.x) & 1u);
}
@fragment fn fragment_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
 if clipped(position.xy) {discard;}
 if u.view.w > 1.5 {return vec4f(0.63, 1.0, 0.85, 0.18);}
 if u.view.w > 0.5 && u.view.z > 3.5 {
 let dynamic = u.view.z > 4.5; let sample = (position.xy - u.canvas.xy) / select(u.viewport.z, 1.0, dynamic); let ink = stipple(sample);
 if dynamic {let screen = position.xy / u.viewport.z; let cell = screen - floor(screen / 5.0) * 5.0 - vec2f(2.5); return vec4f(1.0, 1.0, 1.0, (1.0 - smoothstep(0.65, 1.25, length(cell))) * 0.9);} return vec4f(u.color.rgb, u.color.a * ink);
 }
 if u.view.w > 0.5 && u.view.z > 1.5 {return vec4f(u.color.rgb, u.color.a * stipple(position.xy - u.canvas.xy));}
 if u.view.w > 0.5 {let screen = position.xy / u.viewport.z; let cell = screen - floor(screen / 5.0) * 5.0 - vec2f(2.5); return vec4f(1.0, 1.0, 1.0, (1.0 - smoothstep(0.65, 1.25, length(cell))) * 0.8);}
 var fill = u.color; if u.view.z > 2.5 {fill.a = 0.0;} else if u.view.z > 0.5 {fill.a *= stipple((position.xy - u.canvas.xy) / u.viewport.z);} return fill;
}
