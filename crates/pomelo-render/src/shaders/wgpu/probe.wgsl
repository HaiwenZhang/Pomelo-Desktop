struct Frame {
    viewport: vec4f,
    canvas: vec4f,
    clip_bounds: vec4f,
    shape_a: vec4f,
    shape_b: vec4f,
    shape_color: vec4f,
    shape_kind: vec4u,
    inherited: vec4f
}

@group(0) @binding(0)
var<uniform> u: Frame;
struct Out {
    @builtin(position) position: vec4f,
    @location(0) color: vec3f
}

@vertex
fn vertex_main(@builtin(vertex_index) id: u32) -> Out {
    var uv = vec2f(f32(id & 1u), f32((id >> 1u) & 1u));
    if u.shape_kind.x == 4u {
        if id == 0u {
            uv = vec2f(0.5, 0.1);
        }
        else if id == 1u {
            uv = vec2f(0.1, 0.9);
        }
        else {
            uv = vec2f(0.9, 0.9);
        }
    }
    let position = u.canvas.xy + uv * u.canvas.zw;
    var o: Out;
    o.position = vec4f(position / u.viewport.xy * vec2f(2.0, - 2.0) + vec2f(- 1.0, 1.0), 0.0, 1.0);
    let color_id = (id + u.shape_kind.y) % 3u;
    o.color = select(select(vec3f(0.0, 0.0, 1.0), vec3f(0.0, 1.0, 0.0), color_id == 1u), vec3f(1.0, 0.0, 0.0), color_id == 0u);
    return o;
}

fn box_distance(p: vec2f, box: vec4f) -> f32 {
    let d = abs(p - box.xy) - box.zw;
    return length(max(d, vec2f(0.0))) + min(max(d.x, d.y), 0.0);
}

fn fragment_color(i: Out) -> vec4f {
    let p = i.position.xy;
    if any(p < u.clip_bounds.xy) || any(p > u.clip_bounds.xy + u.clip_bounds.zw) {
        discard;
    }
    if u.shape_kind.x == 4u {
        return vec4f(i.color, 1.0);
    }
    if u.shape_kind.x == 3u {
        let cell = abs(fract((p - u.canvas.xy) / 24.0) - vec2f(0.5)) * 24.0;
        let grid = 1.0 - smoothstep(0.4, 1.2, min(cell.x, cell.y));
        return vec4f(mix(vec3f(0.025, 0.045, 0.055), vec3f(0.09, 0.14, 0.17), grid), 1.0);
    }
    var distance: f32;
    if u.shape_kind.x == 0u {
        let ab = u.shape_a.zw - u.shape_a.xy;
        let ap = p - u.shape_a.xy;
        let t = clamp(dot(ap, ab) / max(dot(ab, ab), 1e-20), 0.0, 1.0);
        distance = length(ap - ab * t) - u.shape_b.x;
    }
    else if u.shape_kind.x == 1u {
        distance = length(p - u.shape_a.xy) - u.shape_a.z;
    }
    else {
        distance = max(box_distance(p, u.shape_a), - box_distance(p, u.shape_b));
    }
    let coverage = 1.0 - smoothstep(- 0.75, 0.75, distance);
    if coverage < 0.001 {
        discard;
    }
    return vec4f(u.shape_color.rgb, u.shape_color.a * coverage);
}
