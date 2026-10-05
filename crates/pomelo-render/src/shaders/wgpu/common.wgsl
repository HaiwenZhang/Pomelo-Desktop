// Camera-relative double-single arithmetic. Metal compilation disables fast math.
fn ds_add(a: vec2f, b: vec2f) -> vec2f {
    let sum = a.x + b.x;
    let v = sum - a.x;
    let error = (a.x - (sum - v)) + (b.x - v) + a.y + b.y;
    let high = sum + error;
    return vec2f(high, error - (high - sum));
}

fn ds_mul(a: vec2f, b: vec2f) -> vec2f {
    let ca = 4097.0 * a.x;
    let ah = ca - (ca - a.x);
    let al = a.x - ah;
    let cb = 4097.0 * b.x;
    let bh = cb - (cb - b.x);
    let bl = b.x - bh;
    let product = a.x * b.x;
    let error = ((ah * bh - product) + ah * bl + al * bh) + al * bl + a.x * b.y + a.y * b.x;
    return ds_add(vec2f(product, 0.0), vec2f(error, 0.0));
}

fn relative(p: vec4f) -> vec4f {
    let x = ds_add(p.xz, - u.camera.xz);
    let y = ds_add(p.yw, - u.camera.yw);
    return vec4f(x.x, y.x, x.y, y.y);
}

fn delta(screen: vec2f, p: vec4f) -> vec4f {
    let x = ds_add(vec2f(screen.x, 0.0), - p.xz);
    let y = ds_add(vec2f(screen.y, 0.0), - p.yw);
    return vec4f(x.x, y.x, x.y, y.y);
}

fn precise_dot(a: vec4f, b: vec4f) -> f32 {
    let d = ds_add(ds_mul(a.xz, b.xz), ds_mul(a.yw, b.yw));
    return d.x + d.y;
}

fn tangent(p: vec4f, center: vec4f) -> vec4f {
    let x = ds_add(p.xz, - center.xz);
    let y = ds_add(p.yw, - center.yw);
    return vec4f(- y.x, x.x, - y.y, x.y);
}

fn screen_position(screen: vec2f) -> vec4f {
    return vec4f(screen / u.viewport.xy * vec2f(2.0, - 2.0) + vec2f(- 1.0, 1.0), 0.0, 1.0);
}

fn board_position(position: vec2f) -> vec2f {
    return (position - u.canvas.xy - u.canvas.zw * 0.5) * vec2f(u.view.y, - 1.0) / u.view.x;
}

fn clipped(position: vec2f) -> bool {
    return any(position < u.clip_bounds.xy) || any(position > u.clip_bounds.xy + u.clip_bounds.zw);
}

fn quad_corner(id: u32) -> vec2f {
    return vec2f(f32(id & 1u), f32((id >> 1u) & 1u));
}
