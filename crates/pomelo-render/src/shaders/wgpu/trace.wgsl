struct Trace {
    a: vec4f,
    b: vec4f,
    center: vec4f,
    arc: vec4f,
    bounds_min: vec4f,
    bounds_max: vec4f,
    ids: vec4u,
    flags: vec4u
}

@group(0) @binding(1)
var<storage, read> traces: array<Trace>;
struct Out {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) a: vec4f,
    @location(1) @interpolate(flat) b: vec4f,
    @location(2) @interpolate(flat) center: vec4f,
    @location(3) @interpolate(flat) arc: vec4f,
    @location(4) @interpolate(flat) flags: vec4u,
    @location(5) @interpolate(flat) tint: vec4f,
}

@vertex
fn vertex_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> Out {
    let t = traces[u.batch.x + iid];
    let low = relative(t.bounds_min);
    let high = relative(t.bounds_max);
    let limit = u.canvas.zw * 0.5 / u.view.x;
    let lo = clamp(low.xy + low.zw - vec2f(2.0 * u.viewport.z / u.view.x), - limit, limit);
    let hi = clamp(high.xy + high.zw + vec2f(2.0 * u.viewport.z / u.view.x), - limit, limit);
    var o: Out;
    o.position = screen_position(u.canvas.xy + u.canvas.zw * 0.5 + mix(lo, hi, quad_corner(vid)) * vec2f(u.view.y, - 1.0) * u.view.x);
    o.a = relative(t.a);
    o.b = relative(t.b);
    o.center = relative(t.center);
    o.arc = t.arc;
    o.flags = t.flags;
    let selected = (u.batch.z == 1u && t.ids.w == u.batch.y) || (u.batch.z == 2u && t.ids.x == u.batch.y) || (u.batch.z == 3u && t.ids.y == u.batch.y);
    var material = u.color;
    if u.view.z != 0.0 && t.ids.w != 0u && (t.flags.w & 4u) == 0u {
        material = vec4f(pcb_net_color(t.ids.w), material.a);
    }
    o.tint = select(material, u.highlight, selected && (t.flags.w & 4u) == 0u);
    if t.flags.x == 0u && !any(lo >= hi) && length((o.b.xy + o.b.zw) - (o.a.xy + o.a.zw)) * u.view.x / u.viewport.z > 16384.0 {
        let dx = ds_add(o.b.xz, - o.a.xz);
        let dy = ds_add(o.b.yw, - o.a.yw);
        let d = vec4f(dx.x, dy.x, dx.y, dy.y);
        let magnitude = length(d.xy + d.zw);
        let cross_value = ds_add(ds_mul(o.a.xz, d.yw), - ds_mul(o.a.yw, d.xz));
        let start = precise_dot(o.a, d) / magnitude;
        let end = precise_dot(o.b, d) / magnitude;
        o.a = vec4f((d.xy + d.zw) / magnitude, (cross_value.x + cross_value.y) / magnitude, start);
        o.b.x = end;
        o.flags.x = 2u;
    }
    return o;
}

fn fragment_color(i: Out) -> vec4f {
    if clipped(i.position.xy) {
        discard;
    }
    let dynamic_base = (i.flags.w & 128u) != 0u && u.view.w < 0.5 && u.batch.w != 0u;
    let screen = board_position(i.position.xy);
    let a = delta(screen, i.a);
    let b = delta(screen, i.b);
    var distance: f32;
    if i.flags.x == 2u {
        let along = dot(screen, i.a.xy);
        let normal = dot(screen, vec2f(- i.a.y, i.a.x)) + i.a.z;
        let cap = max(max(i.a.w - along, along - i.b.x), 0.0);
        distance = length(vec2f(cap, normal));
    }
    else if i.flags.x == 0u {
        let ab = (a.xy + a.zw) - (b.xy + b.zw);
        let ap = a.xy + a.zw;
        let t = clamp(dot(ap, ab) / max(dot(ab, ab), 1e-30), 0.0, 1.0);
        distance = length(ap - ab * t);
    }
    else {
        let p = delta(screen, i.center);
        let r = i.arc.xy;
        let square = ds_add(ds_add(ds_mul(p.xz, p.xz), ds_mul(p.yw, p.yw)), - ds_mul(r, r));
        let radial = (square.x + square.y) / max(length(p.xy + p.zw) + r.x + r.y, 1e-30);
        let direction = sign(i.arc.w);
        let after = precise_dot(a, tangent(i.a, i.center)) * direction >= 0.0;
        let before = precise_dot(b, tangent(i.b, i.center)) * direction <= 0.0;
        let full = (i.flags.w & 1u) != 0u;
        let long_arc = (i.flags.w & 2u) != 0u;
        let inside = full || (direction != 0.0 && select(after && before, after || before, long_arc));
        distance = select(min(length(a.xy + a.zw), length(b.xy + b.zw)), abs(radial), inside);
    }
    let px = u.viewport.z / u.view.x;
    let width = select(bitcast<f32>(i.flags.z), 0.0, (i.flags.w & 4u) != 0u);
    let aa = select(px * 0.65, 0.5 / u.view.x, dynamic_base);
    distance -= select(select(max(width * 0.5, px * 0.5), px * 0.65, (i.flags.w & 8u) != 0u), 0.5 / u.view.x, dynamic_base);
    let alpha = 1.0 - smoothstep(- aa, aa, distance);
    if u.view.w > 0.5 {
        let edge = 1.0 - smoothstep(px * 0.6, px * 1.6, abs(distance));
        let logical = i.position.xy / u.viewport.z;
        let cell = logical - floor(logical / 5.0) * 5.0 - vec2f(2.5);
        let dots = 1.0 - smoothstep(0.65, 1.25, length(cell));
        let coverage = max(edge, select(dots * alpha, 0.0, u.view.w > 1.5 && u.view.w < 2.5));
        return vec4f(select(vec3f(1.0), vec3f(0.63, 1.0, 0.85), u.view.w > 1.5), coverage * 0.9);
    }
    if alpha < 0.001 {
        discard;
    }
    return vec4f(i.tint.rgb, i.tint.a * alpha);
}
