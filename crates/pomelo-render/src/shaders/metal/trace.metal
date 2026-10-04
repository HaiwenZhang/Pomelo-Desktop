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
struct Trace {
    metal::float4 a;
    metal::float4 b;
    metal::float4 center;
    metal::float4 arc;
    metal::float4 bounds_min;
    metal::float4 bounds_max;
    metal::uint4 ids;
    metal::uint4 flags;
};
typedef Trace type_8[1];
struct Out {
    metal::float4 position;
    metal::float4 a;
    metal::float4 b;
    metal::float4 center;
    metal::float4 arc;
    metal::uint4 flags;
    metal::float4 tint;
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
    bool local_10 = {};
    metal::float4 _e3 = u.clip_bounds;
    if (!(metal::any(position_1 < _e3.xy))) {
        metal::float4 _e12 = u.clip_bounds;
        metal::float4 _e16 = u.clip_bounds;
        local_10 = metal::any(position_1 > (_e12.xy + _e16.zw));
    } else {
        local_10 = true;
    }
    bool _e22 = local_10;
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
    bool local_11 = {};
    bool local_12 = {};
    float distance = {};
    bool local_14 = {};
    bool local_15 = {};
    bool local_16 = {};
    bool local_17 = {};
    bool local_18 = {};
    bool _e3 = clipped(i_1.position.xy, u);
    if (_e3) {
        metal::discard_fragment();
    }
    if ((i_1.flags.w & 128u) != 0u) {
        float _e15 = u.view.w;
        local_11 = _e15 < 0.5;
    } else {
        local_11 = false;
    }
    bool _e19 = local_11;
    if (_e19) {
        uint _e25 = u.batch.w;
        local_12 = _e25 != 0u;
    } else {
        local_12 = false;
    }
    bool dynamic_base = local_12;
    metal::float2 _e42 = board_position(i_1.position.xy, u);
    metal::float4 _e44 = delta(_e42, i_1.a);
    metal::float4 _e46 = delta(_e42, i_1.b);
    if (i_1.flags.x == 2u) {
        float along = metal::dot(_e42, i_1.a.xy);
        float normal = metal::dot(_e42, metal::float2(-(i_1.a.y), i_1.a.x)) + i_1.a.z;
        float cap = metal::max(metal::max(i_1.a.w - along, along - i_1.b.x), 0.0);
        distance = metal::length(metal::float2(cap, normal));
    } else {
        if (i_1.flags.x == 0u) {
            metal::float2 ab = (_e44.xy + _e44.zw) - (_e46.xy + _e46.zw);
            metal::float2 ap = _e44.xy + _e44.zw;
            float t = metal::clamp(metal::dot(ap, ab) / metal::max(metal::dot(ab, ab), 0.000000000000000000000000000001), 0.0, 1.0);
            distance = metal::length(ap - (ab * t));
        } else {
            metal::float4 _e102 = delta(_e42, i_1.center);
            metal::float2 r = i_1.arc.xy;
            metal::float2 _e107 = ds_mul(_e102.xz, _e102.xz);
            metal::float2 _e110 = ds_mul(_e102.yw, _e102.yw);
            metal::float2 _e111 = ds_add(_e107, _e110);
            metal::float2 _e112 = ds_mul(r, r);
            metal::float2 _e114 = ds_add(_e111, -(_e112));
            float radial = (_e114.x + _e114.y) / metal::max((metal::length(_e102.xy + _e102.zw) + r.x) + r.y, 0.000000000000000000000000000001);
            float direction = metal::sign(i_1.arc.w);
            metal::float4 _e134 = tangent(i_1.a, i_1.center);
            float _e135 = precise_dot(_e44, _e134);
            bool after = (_e135 * direction) >= 0.0;
            metal::float4 _e141 = tangent(i_1.b, i_1.center);
            float _e142 = precise_dot(_e46, _e141);
            bool before = (_e142 * direction) <= 0.0;
            bool full = (i_1.flags.w & 1u) != 0u;
            bool long_arc = (i_1.flags.w & 2u) != 0u;
            if (!(full)) {
                if (direction != 0.0) {
                    if (after) {
                        local_16 = before;
                    } else {
                        local_16 = false;
                    }
                    bool _e168 = local_16;
                    if (!(after)) {
                        local_17 = before;
                    } else {
                        local_17 = true;
                    }
                    bool _e173 = local_17;
                    local_15 = long_arc ? _e173 : _e168;
                } else {
                    local_15 = false;
                }
                bool _e176 = local_15;
                local_14 = _e176;
            } else {
                local_14 = true;
            }
            bool inside = local_14;
            distance = inside ? metal::abs(radial) : metal::min(metal::length(_e44.xy + _e44.zw), metal::length(_e46.xy + _e46.zw));
        }
    }
    float _e193 = u.viewport.z;
    float _e197 = u.view.x;
    float px = _e193 / _e197;
    float width = ((i_1.flags.w & 4u) != 0u) ? 0.0 : as_type<float>(i_1.flags.z);
    float _e215 = u.view.x;
    float aa = dynamic_base ? (0.5 / _e215) : (px * 0.65);
    float _e219 = distance;
    float _e237 = u.view.x;
    distance = _e219 - (dynamic_base ? (0.5 / _e237) : (((i_1.flags.w & 8u) != 0u) ? (px * 0.65) : metal::max(width * 0.5, px * 0.5)));
    float _e243 = distance;
    float alpha = 1.0 - metal::smoothstep(-(aa), aa, _e243);
    float _e250 = u.view.w;
    if (_e250 > 0.5) {
        float _e257 = distance;
        float edge = 1.0 - metal::smoothstep(px * 0.6, px * 1.6, metal::abs(_e257));
        float _e267 = u.viewport.z;
        metal::float2 logical = i_1.position.xy / metal::float2(_e267);
        metal::float2 cell = (logical - (metal::floor(logical / metal::float2(5.0)) * 5.0)) - metal::float2(2.5);
        float dots = 1.0 - metal::smoothstep(0.65, 1.25, metal::length(cell));
        float _e290 = u.view.w;
        if (_e290 > 1.5) {
            float _e298 = u.view.w;
            local_18 = _e298 < 2.5;
        } else {
            local_18 = false;
        }
        bool _e302 = local_18;
        float coverage = metal::max(edge, _e302 ? 0.0 : (dots * alpha));
        float _e315 = u.view.w;
        return metal::float4((_e315 > 1.5) ? metal::float3(0.63, 1.0, 0.85) : metal::float3(1.0), coverage * 0.9);
    }
    if (alpha < 0.001) {
        metal::discard_fragment();
    }
    return metal::float4(i_1.tint.xyz, i_1.tint.w * alpha);
}

struct vertex_mainInput {
};
struct vertex_mainOutput {
    metal::float4 position [[position]];
    metal::float4 a [[user(loc0), flat]];
    metal::float4 b [[user(loc1), flat]];
    metal::float4 center [[user(loc2), flat]];
    metal::float4 arc [[user(loc3), flat]];
    metal::uint4 flags [[user(loc4), flat]];
    metal::float4 tint [[user(loc5), flat]];
};
vertex vertex_mainOutput vertex_main(
  uint vid [[vertex_id]]
, uint iid [[instance_id]]
, constant Frame& u [[buffer(0)]]
, device type_8 const& traces [[buffer(1)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(2)]]
) {
    Out o = {};
    bool local = {};
    bool local_1 = {};
    bool local_2 = {};
    bool local_3 = {};
    bool local_4 = {};
    metal::float4 material = {};
    bool local_5 = {};
    bool local_6 = {};
    bool local_7 = {};
    bool local_8 = {};
    bool local_9 = {};
    uint _e6 = u.batch.x;
    Trace t_1 = traces[_e6 + iid];
    metal::float4 _e11 = relative(t_1.bounds_min, u);
    metal::float4 _e13 = relative(t_1.bounds_max, u);
    metal::float4 _e16 = u.canvas;
    float _e23 = u.view.x;
    metal::float2 limit = (_e16.zw * 0.5) / metal::float2(_e23);
    float _e32 = u.viewport.z;
    float _e38 = u.view.x;
    metal::float2 lo = metal::clamp((_e11.xy + _e11.zw) - metal::float2((2.0 * _e32) / _e38), -(limit), limit);
    float _e50 = u.viewport.z;
    float _e56 = u.view.x;
    metal::float2 hi = metal::clamp((_e13.xy + _e13.zw) + metal::float2((2.0 * _e50) / _e56), -(limit), limit);
    metal::float4 _e66 = u.canvas;
    metal::float4 _e70 = u.canvas;
    metal::float2 _e75 = quad_corner(vid);
    float _e80 = u.view.y;
    float _e87 = u.view.x;
    metal::float4 _e90 = screen_position((_e66.xy + (_e70.zw * 0.5)) + ((metal::mix(lo, hi, _e75) * metal::float2(_e80, -1.0)) * _e87), u);
    o.position = _e90;
    metal::float4 _e93 = relative(t_1.a, u);
    o.a = _e93;
    metal::float4 _e96 = relative(t_1.b, u);
    o.b = _e96;
    metal::float4 _e99 = relative(t_1.center, u);
    o.center = _e99;
    o.arc = t_1.arc;
    o.flags = t_1.flags;
    uint _e107 = u.batch.z;
    if (_e107 == 1u) {
        uint _e117 = u.batch.y;
        local = t_1.ids.w == _e117;
    } else {
        local = false;
    }
    bool _e120 = local;
    if (!(_e120)) {
        uint _e127 = u.batch.z;
        if (_e127 == 2u) {
            uint _e137 = u.batch.y;
            local_2 = t_1.ids.x == _e137;
        } else {
            local_2 = false;
        }
        bool _e140 = local_2;
        local_1 = _e140;
    } else {
        local_1 = true;
    }
    bool _e142 = local_1;
    if (!(_e142)) {
        uint _e149 = u.batch.z;
        if (_e149 == 3u) {
            uint _e159 = u.batch.y;
            local_4 = t_1.ids.y == _e159;
        } else {
            local_4 = false;
        }
        bool _e162 = local_4;
        local_3 = _e162;
    } else {
        local_3 = true;
    }
    bool selected = local_3;
    metal::float4 _e167 = u.color;
    material = _e167;
    float _e172 = u.view.z;
    if (_e172 != 0.0) {
        local_5 = t_1.ids.w != 0u;
    } else {
        local_5 = false;
    }
    bool _e182 = local_5;
    if (_e182) {
        local_6 = (t_1.flags.w & 4u) == 0u;
    } else {
        local_6 = false;
    }
    bool _e192 = local_6;
    if (_e192) {
        metal::float3 _e195 = pcb_net_color(t_1.ids.w);
        float _e197 = material.w;
        material = metal::float4(_e195, _e197);
    }
    metal::float4 _e200 = material;
    metal::float4 _e203 = u.highlight;
    if (selected) {
        local_7 = (t_1.flags.w & 4u) == 0u;
    } else {
        local_7 = false;
    }
    bool _e213 = local_7;
    o.tint = _e213 ? _e203 : _e200;
    if (t_1.flags.x == 0u) {
        local_8 = !(metal::any(lo >= hi));
    } else {
        local_8 = false;
    }
    bool _e225 = local_8;
    if (_e225) {
        metal::float4 _e229 = o.b;
        metal::float4 _e232 = o.b;
        metal::float4 _e236 = o.a;
        metal::float4 _e239 = o.a;
        float _e247 = u.view.x;
        float _e252 = u.viewport.z;
        local_9 = ((metal::length((_e229.xy + _e232.zw) - (_e236.xy + _e239.zw)) * _e247) / _e252) > 16384.0;
    } else {
        local_9 = false;
    }
    bool _e257 = local_9;
    if (_e257) {
        metal::float4 _e259 = o.b;
        metal::float4 _e262 = o.a;
        metal::float2 _e265 = ds_add(_e259.xz, -(_e262.xz));
        metal::float4 _e267 = o.b;
        metal::float4 _e270 = o.a;
        metal::float2 _e273 = ds_add(_e267.yw, -(_e270.yw));
        metal::float4 d = metal::float4(_e265.x, _e273.x, _e265.y, _e273.y);
        float magnitude = metal::length(d.xy + d.zw);
        metal::float4 _e284 = o.a;
        metal::float2 _e287 = ds_mul(_e284.xz, d.yw);
        metal::float4 _e289 = o.a;
        metal::float2 _e292 = ds_mul(_e289.yw, d.xz);
        metal::float2 _e294 = ds_add(_e287, -(_e292));
        metal::float4 _e296 = o.a;
        float _e297 = precise_dot(_e296, d);
        float start = _e297 / magnitude;
        metal::float4 _e300 = o.b;
        float _e301 = precise_dot(_e300, d);
        float end = _e301 / magnitude;
        o.a = metal::float4((d.xy + d.zw) / metal::float2(magnitude), (_e294.x + _e294.y) / magnitude, start);
        o.b.x = end;
        o.flags.x = 2u;
    }
    Out _e319 = o;
    const auto _tmp = _e319;
    return vertex_mainOutput { _tmp.position, _tmp.a, _tmp.b, _tmp.center, _tmp.arc, _tmp.flags, _tmp.tint };
}


struct fragment_mainInput {
    metal::float4 a [[user(loc0), flat]];
    metal::float4 b [[user(loc1), flat]];
    metal::float4 center [[user(loc2), flat]];
    metal::float4 arc [[user(loc3), flat]];
    metal::uint4 flags [[user(loc4), flat]];
    metal::float4 tint [[user(loc5), flat]];
};
struct fragment_mainOutput {
    metal::float4 member_1 [[color(0)]];
};
fragment fragment_mainOutput fragment_main(
  fragment_mainInput varyings_1 [[stage_in]]
, metal::float4 position_2 [[position]]
, constant Frame& u [[buffer(0)]]
) {
    const Out i = { position_2, varyings_1.a, varyings_1.b, varyings_1.center, varyings_1.arc, varyings_1.flags, varyings_1.tint };
    metal::float4 _e1 = fragment_color(i, u);
    float _e7 = u.inherited.x;
    return fragment_mainOutput { metal::float4(_e1.xyz, _e1.w * _e7) };
}
