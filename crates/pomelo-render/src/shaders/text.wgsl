struct Stroke {
    a: vec4f,
    b: vec4f,
    ids: vec4u,
    flags: vec4u
}

@group(0) @binding(1)
var<storage, read> strokes: array<Stroke>;
struct Out {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) a: vec4f,
    @location(1) @interpolate(flat) b: vec4f,
    @location(2) @interpolate(flat) width: f32
}

@vertex
fn vertex_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> Out {
    let s = strokes[u.batch.x + iid];
    var o: Out;
    o.a = relative(s.a);
    o.b = relative(s.b);
    o.width = bitcast<f32>(s.ids.w);
    let a = o.a.xy + o.a.zw;
    let b = o.b.xy + o.b.zw;
    let padding = o.width * 0.5 + 2.0 * u.viewport.z / u.view.x;
    let limit = u.canvas.zw * 0.5 / u.view.x;
    let lo = clamp(min(a, b) - vec2f(padding), - limit, limit);
    let hi = clamp(max(a, b) + vec2f(padding), - limit, limit);
    o.position = screen_position(u.canvas.xy + u.canvas.zw * 0.5 + mix(lo, hi, quad_corner(vid)) * vec2f(u.view.y, - 1.0) * u.view.x);
    return o;
}

fn fragment_color(i: Out) -> vec4f {
    if clipped(i.position.xy) {
        discard;
    }
    let screen = board_position(i.position.xy);
    let a = delta(screen, i.a);
    let b = delta(screen, i.b);
    let ab = (a.xy + a.zw) - (b.xy + b.zw);
    let ap = a.xy + a.zw;
    let t = clamp(dot(ap, ab) / max(dot(ab, ab), 1e-30), 0.0, 1.0);
    let px = u.viewport.z / u.view.x;
    let distance = length(ap - ab * t) - max(i.width * 0.5, px * 0.5);
    let alpha = 1.0 - smoothstep(- px * 0.65, px * 0.65, distance);
    if alpha < 0.001 {
        discard;
    }
    return vec4f(u.color.rgb, u.color.a * alpha);
}
