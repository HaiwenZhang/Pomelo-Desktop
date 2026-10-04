// Native Metal shader. Maintained independently from Linux WGSL and Windows HLSL.
// Camera-relative arithmetic requires fast math disabled.
// language: metal2.4
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size1;
};

struct Frame {
    metal::float4 viewport;
    metal::float4 canvas;
    metal::float4 clip_bounds;
    metal::float4 camera;
    metal::float4 view;
    metal::float4 color;
    metal::uint4 batch;
    metal::float4 highlight;
    metal::float4 inherited;
};
struct type_3 {
    uint inner[203];
};
struct Stroke {
    metal::float4 a;
    metal::float4 b;
    metal::uint4 ids;
    metal::uint4 flags;
};
typedef Stroke type_8[1];
struct Out {
    metal::float4 position;
    metal::float4 a;
    metal::float4 b;
    float width;
    char _pad4[12];
};
constant type_3 pcb_net_colors = type_3 {{4279066170u, 4279908959u, 4280987247u, 4293313101u, 4291381854u, 4292658869u, 4292128567u, 4281228684u, 4288556846u, 4279909166u, 4292462259u, 4281236786u, 4281228618u, 4280966274u, 4289255525u, 4290518598u, 4291311415u, 4292394968u, 4290292118u, 4290093250u, 4288499292u, 4281227912u, 4287477370u, 4294308070u, 4294305504u, 4293974480u, 4281290575u, 4294278144u, 4283070120u, 4289778915u, 4292462256u, 4282015563u, 4287401100u, 4288730804u, 4282077020u, 4293325301u, 4291254891u, 4282036364u, 4293062859u, 4293982463u, 4292463544u, 4285319048u, 4294901887u, 4283105410u, 4280179343u, 4294826655u, 4287262347u, 4290889942u, 4292391128u, 4288453796u, 4287317267u, 4281240407u, 4291624704u, 4292124695u, 4294505705u, 4289906848u, 4294965479u, 4282081082u, 4291683554u, 4293457136u, 4292391128u, 4291933183u, 4288383720u, 4294892513u, 4291351511u, 4287327823u, 4290283019u, 4294965488u, 4292391088u, 4293178820u, 4291338952u, 4289917145u, 4286292133u, 4284235827u, 4292111674u, 4293094735u, 4286293176u, 4291231801u, 4291070569u, 4288449357u, 4286267233u, 4279910002u, 4290226341u, 4283080092u, 4294956800u, 4286227290u, 4290957792u, 4282088029u, 4294301822u, 4294308838u, 4292266424u, 4292004028u, 4294111462u, 4287323443u, 4292115038u, 4289020955u, 4292110954u, 4282069297u, 4294308841u, 4287327559u, 4286497006u, 4289906848u, 4291999898u, 4289235567u, 4290569686u, 4291246916u, 4281023055u, 4287248954u, 4287509834u, 4283076058u, 4291250765u, 4281227887u, 4294930273u, 4293325050u, 4293643688u, 4284365947u, 4285218364u, 4282088575u, 4291271274u, 4284238403u, 4292453780u, 4291339662u, 4294567077u, 4292391112u, 4293325567u, 4291042874u, 4294489554u, 4292419873u, 4285336350u, 4282088023u, 4294957235u, 4294937600u, 4289093422u, 4289649108u, 4285307850u, 4294630109u, 4291880953u, 4294101095u, 4290789424u, 4294921549u, 4282299529u, 4281223994u, 4283145427u, 4281032572u, 4290291154u, 4291684601u, 4291373863u, 4285581476u, 4287546601u, 4292929519u, 4287513661u, 4283070550u, 4294956800u, 4286233025u, 4283070619u, 4290172840u, 4289251812u, 4292395900u, 4291420281u, 4283401471u, 4288731352u, 4288987816u, 4286221195u, 4285238436u, 4287253404u, 4285238938u, 4284921808u, 4278233195u, 4278203249u, 4281223974u, 4283068326u, 4283133068u, 4285581424u, 4289940291u, 4287090411u, 4289779944u, 4287013287u, 4282280509u, 4287327580u, 4294493297u, 4282534736u, 4294929259u, 4293076331u, 4291016583u, 4294112244u, 4280106842u, 4294505446u, 4292144631u, 4280963774u, 4291998860u, 4286364072u, 4294201644u, 4282811060u, 4288684272u, 4282168177u, 4291221467u, 4287000469u, 4280960863u, 4285238819u, 4289907122u, 4278202706u, 4286286989u, 4283142028u}};
constant uint PCB_SOURCE_TEXT_CATEGORY = 6u;

uint naga_mod(uint lhs, uint rhs) {
    return lhs % metal::select(rhs, 1u, rhs == 0u);
}

metal::float3 pcb_net_color(
    uint net
) {
    uint rgb = pcb_net_colors.inner[naga_mod(net, 203u)];
    return metal::float3(static_cast<float>((rgb >> 16u) & 255u), static_cast<float>((rgb >> 8u) & 255u), static_cast<float>(rgb & 255u)) / metal::float3(255.0);
}

metal::float2 ds_add(
    metal::float2 a,
    metal::float2 b
) {
    float sum = a.x + b.x;
    float v = sum - a.x;
    float error = (((a.x - (sum - v)) + (b.x - v)) + a.y) + b.y;
    float high = sum + error;
    return metal::float2(high, error - (high - sum));
}

metal::float2 ds_mul(
    metal::float2 a_1,
    metal::float2 b_1
) {
    float ca = 4097.0 * a_1.x;
    float ah = ca - (ca - a_1.x);
    float al = a_1.x - ah;
    float cb = 4097.0 * b_1.x;
    float bh = cb - (cb - b_1.x);
    float bl = b_1.x - bh;
    float product = a_1.x * b_1.x;
    float error_1 = ((((((ah * bh) - product) + (ah * bl)) + (al * bh)) + (al * bl)) + (a_1.x * b_1.y)) + (a_1.y * b_1.x);
    metal::float2 _e41 = ds_add(metal::float2(product, 0.0), metal::float2(error_1, 0.0));
    return _e41;
}

metal::float4 relative(
    metal::float4 p,
    constant Frame& u
) {
    metal::float4 _e4 = u.camera;
    metal::float2 _e7 = ds_add(p.xz, -(_e4.xz));
    metal::float4 _e11 = u.camera;
    metal::float2 _e14 = ds_add(p.yw, -(_e11.yw));
    return metal::float4(_e7.x, _e14.x, _e7.y, _e14.y);
}

metal::float4 delta(
    metal::float2 screen,
    metal::float4 p_1
) {
    metal::float2 _e7 = ds_add(metal::float2(screen.x, 0.0), -(p_1.xz));
    metal::float2 _e13 = ds_add(metal::float2(screen.y, 0.0), -(p_1.yw));
    return metal::float4(_e7.x, _e13.x, _e7.y, _e13.y);
}

float precise_dot(
    metal::float4 a_2,
    metal::float4 b_2
) {
    metal::float2 _e4 = ds_mul(a_2.xz, b_2.xz);
    metal::float2 _e7 = ds_mul(a_2.yw, b_2.yw);
    metal::float2 _e8 = ds_add(_e4, _e7);
    return _e8.x + _e8.y;
}

metal::float4 tangent(
    metal::float4 p_2,
    metal::float4 center
) {
    metal::float2 _e5 = ds_add(p_2.xz, -(center.xz));
    metal::float2 _e9 = ds_add(p_2.yw, -(center.yw));
    return metal::float4(-(_e9.x), _e5.x, -(_e9.y), _e5.y);
}

metal::float4 screen_position(
    metal::float2 screen_1,
    constant Frame& u
) {
    metal::float4 _e3 = u.viewport;
    return metal::float4(((screen_1 / _e3.xy) * metal::float2(2.0, -2.0)) + metal::float2(-1.0, 1.0), 0.0, 1.0);
}

metal::float2 board_position(
    metal::float2 position,
    constant Frame& u
) {
    metal::float4 _e3 = u.canvas;
    metal::float4 _e8 = u.canvas;
    float _e16 = u.view.y;
    float _e23 = u.view.x;
    return (((position - _e3.xy) - (_e8.zw * 0.5)) * metal::float2(_e16, -1.0)) / metal::float2(_e23);
}

bool clipped(
    metal::float2 position_1,
    constant Frame& u
) {
    bool local = {};
    metal::float4 _e3 = u.clip_bounds;
    if (!(metal::any(position_1 < _e3.xy))) {
        metal::float4 _e12 = u.clip_bounds;
        metal::float4 _e16 = u.clip_bounds;
        local = metal::any(position_1 > (_e12.xy + _e16.zw));
    } else {
        local = true;
    }
    bool _e22 = local;
    return _e22;
}

metal::float2 quad_corner(
    uint id
) {
    return metal::float2(static_cast<float>(id & 1u), static_cast<float>((id >> 1u) & 1u));
}

metal::float4 fragment_color(
    Out i_1,
    constant Frame& u
) {
    bool _e3 = clipped(i_1.position.xy, u);
    if (_e3) {
        metal::discard_fragment();
    }
    metal::float2 _e6 = board_position(i_1.position.xy, u);
    metal::float4 _e8 = delta(_e6, i_1.a);
    metal::float4 _e10 = delta(_e6, i_1.b);
    metal::float2 ab = (_e8.xy + _e8.zw) - (_e10.xy + _e10.zw);
    metal::float2 ap = _e8.xy + _e8.zw;
    float t = metal::clamp(metal::dot(ap, ab) / metal::max(metal::dot(ab, ab), 0.000000000000000000000000000001), 0.0, 1.0);
    float _e32 = u.viewport.z;
    float _e36 = u.view.x;
    float px = _e32 / _e36;
    float distance = metal::length(ap - (ab * t)) - metal::max(i_1.width * 0.5, px * 0.5);
    float alpha = 1.0 - metal::smoothstep(-(px) * 0.65, px * 0.65, distance);
    if (alpha < 0.001) {
        metal::discard_fragment();
    }
    metal::float4 _e60 = u.color;
    float _e65 = u.color.w;
    return metal::float4(_e60.xyz, _e65 * alpha);
}

struct vertex_mainInput {
};
struct vertex_mainOutput {
    metal::float4 position [[position]];
    metal::float4 a [[user(loc0), flat]];
    metal::float4 b [[user(loc1), flat]];
    float width [[user(loc2), flat]];
};
vertex vertex_mainOutput vertex_main(
  uint vid [[vertex_id]]
, uint iid [[instance_id]]
, constant Frame& u [[buffer(0)]]
, device type_8 const& strokes [[buffer(1)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(2)]]
) {
    Out o = {};
    uint _e6 = u.batch.x;
    Stroke s = strokes[_e6 + iid];
    metal::float4 _e13 = relative(s.a, u);
    o.a = _e13;
    metal::float4 _e16 = relative(s.b, u);
    o.b = _e16;
    o.width = as_type<float>(s.ids.w);
    metal::float4 _e22 = o.a;
    metal::float4 _e25 = o.a;
    metal::float2 a_3 = _e22.xy + _e25.zw;
    metal::float4 _e29 = o.b;
    metal::float4 _e32 = o.b;
    metal::float2 b_3 = _e29.xy + _e32.zw;
    float _e36 = o.width;
    float _e42 = u.viewport.z;
    float _e48 = u.view.x;
    float padding = (_e36 * 0.5) + ((2.0 * _e42) / _e48);
    metal::float4 _e53 = u.canvas;
    float _e60 = u.view.x;
    metal::float2 limit = (_e53.zw * 0.5) / metal::float2(_e60);
    metal::float2 lo = metal::clamp(metal::min(a_3, b_3) - metal::float2(padding), -(limit), limit);
    metal::float2 hi = metal::clamp(metal::max(a_3, b_3) + metal::float2(padding), -(limit), limit);
    metal::float4 _e76 = u.canvas;
    metal::float4 _e80 = u.canvas;
    metal::float2 _e85 = quad_corner(vid);
    float _e90 = u.view.y;
    float _e97 = u.view.x;
    metal::float4 _e100 = screen_position((_e76.xy + (_e80.zw * 0.5)) + ((metal::mix(lo, hi, _e85) * metal::float2(_e90, -1.0)) * _e97), u);
    o.position = _e100;
    Out _e101 = o;
    const auto _tmp = _e101;
    return vertex_mainOutput { _tmp.position, _tmp.a, _tmp.b, _tmp.width };
}


struct fragment_mainInput {
    metal::float4 a [[user(loc0), flat]];
    metal::float4 b [[user(loc1), flat]];
    float width [[user(loc2), flat]];
};
struct fragment_mainOutput {
    metal::float4 member_1 [[color(0)]];
};
fragment fragment_mainOutput fragment_main(
  fragment_mainInput varyings_1 [[stage_in]]
, metal::float4 position_2 [[position]]
, constant Frame& u [[buffer(0)]]
) {
    const Out i = { position_2, varyings_1.a, varyings_1.b, varyings_1.width };
    metal::float4 _e1 = fragment_color(i, u);
    float _e7 = u.inherited.x;
    return fragment_mainOutput { metal::float4(_e1.xyz, _e1.w * _e7) };
}
