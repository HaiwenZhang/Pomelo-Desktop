struct Frame {
    matrix: mat4x4f,
    viewport: vec4f,
    canvas: vec4f,
    parameters: vec4f,
}
@group(0) @binding(0) var<uniform> u: Frame;
struct Materials { colors: array<vec4f> }
@group(0) @binding(3) var<storage, read> materials: Materials;
@group(0) @binding(4) var scene_color: texture_2d<f32>;
@group(0) @binding(5) var accumulation: texture_2d<f32>;
@group(0) @binding(6) var revealage: texture_2d<f32>;
struct Vertex {
    @location(0) position: vec4f,
    @location(1) normal: vec4f,
    @location(2) owner: vec4u,
    @location(3) transform: vec4f,
    @location(4) instance_owner: vec4u,
}
struct Surface {
    @builtin(position) position: vec4f,
    @location(0) lighting_position: vec4f,
    @location(1) @interpolate(flat) owner: u32,
}
@vertex fn solid_vertex(v: Vertex) -> Surface {
    let c = v.transform.z;
    let s = v.transform.w;
    var x = v.position.x;
    if v.instance_owner.y == 1u {
        x += sign(x)*bitcast<f32>(v.instance_owner.z);
    }
    let local = vec3f(c*x-s*v.position.y,s*x+c*v.position.y,v.position.z*u.parameters.x);
    let p = local+vec3f(v.transform.xy,0.0);
    var o: Surface;
    o.position = u.matrix*vec4f(p,1.0);
    // Derivatives are translation invariant. Removing the instance translation
    // makes xy radial for circular prototypes and fits the mode in the spare w.
    o.lighting_position = vec4f(local,f32(v.instance_owner.w));
    o.owner = select(v.owner.x,v.instance_owner.x,v.instance_owner.x != 0u);
    return o;
}
// Preserve flat caps and authored corners. Round instances use a continuous radial
// side normal without splitting the welded cap/wall vertices at normal seams.
fn surface_normal(i: Surface) -> vec3f {
    let face = cross(dpdx(i.lighting_position.xyz),dpdy(i.lighting_position.xyz));
    if i.lighting_position.w > 0.5 && dot(face.xy,face.xy) > face.z*face.z {
        return vec3f(i.lighting_position.xy,0.0);
    }
    return face;
}
fn lit(color: vec3f, normal: vec3f) -> vec3f {
    let diffuse = abs(dot(normalize(normal),normalize(vec3f(0.3,-0.5,1.0))));
    return color*(0.35+0.65*diffuse);
}
@fragment fn solid_opaque(i: Surface) -> @location(0) vec4f {
    let normal = surface_normal(i);
    let color = materials.colors[i.owner];
    if color.a < 0.999 { discard; }
    return vec4f(lit(color.rgb,normal),1.0);
}
struct Transparent { @location(0) accumulated: vec4f, @location(1) reveal: f32 }
@fragment fn solid_transparent(i: Surface) -> Transparent {
    let normal = surface_normal(i);
    let color = materials.colors[i.owner];
    if color.a <= 0.0 || color.a >= 0.999 { discard; }
    let weight = 0.1+8.0*pow(1.0-clamp(i.position.z,0.0,1.0),3.0);
    var o: Transparent;
    o.accumulated = vec4f(lit(color.rgb,normal)*color.a,color.a)*weight;
    o.reveal = color.a;
    return o;
}
@vertex fn composite_vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let corners = array<vec2f,6>(vec2f(0.0,0.0),vec2f(1.0,0.0),vec2f(0.0,1.0),vec2f(0.0,1.0),vec2f(1.0,0.0),vec2f(1.0,1.0));
    let p = u.canvas.xy+corners[i]*u.canvas.zw;
    return vec4f(p.x/u.viewport.x*2.0-1.0,1.0-p.y/u.viewport.y*2.0,0.0,1.0);
}
@fragment fn composite_fragment(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let p = clamp(vec2i(floor(position.xy-u.canvas.xy)),vec2i(0),vec2i(textureDimensions(scene_color))-vec2i(1));
    let base = textureLoad(scene_color,p,0);
    if u.parameters.z == 0.0 { return base*u.parameters.y; }
    let accumulated = textureLoad(accumulation,p,0);
    let alpha = 1.0-clamp(textureLoad(revealage,p,0).r,0.0,1.0);
    let color = accumulated.rgb/max(accumulated.a,0.00001);
    return vec4f(color*alpha+base.rgb*(1.0-alpha),alpha+base.a*(1.0-alpha))*u.parameters.y;
}
