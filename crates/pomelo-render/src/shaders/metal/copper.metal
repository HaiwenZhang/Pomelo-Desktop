// Native Metal shader. Maintained independently from Linux WGSL and Windows HLSL.
// Camera-relative arithmetic requires fast math disabled.
// language: metal2.4
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct _mslBufferSizes {
    uint size1;
};

struct type_2 {
    metal::uint4 inner[4];
};
struct Frame {
    metal::float4 viewport;
    metal::float4 canvas;
    metal::float4 clip_bounds;
    metal::float4 camera;
    metal::float4 view;
    metal::float4 color;
    metal::float4 rectangle;
    type_2 pattern_mask;
    metal::float4 inherited;
};
struct type_4 {
    uint inner[203];
};
typedef metal::float4 type_9[1];
constant type_4 pcb_net_colors = type_4 {{4279066170u, 4279908959u, 4280987247u, 4293313101u, 4291381854u, 4292658869u, 4292128567u, 4281228684u, 4288556846u, 4279909166u, 4292462259u, 4281236786u, 4281228618u, 4280966274u, 4289255525u, 4290518598u, 4291311415u, 4292394968u, 4290292118u, 4290093250u, 4288499292u, 4281227912u, 4287477370u, 4294308070u, 4294305504u, 4293974480u, 4281290575u, 4294278144u, 4283070120u, 4289778915u, 4292462256u, 4282015563u, 4287401100u, 4288730804u, 4282077020u, 4293325301u, 4291254891u, 4282036364u, 4293062859u, 4293982463u, 4292463544u, 4285319048u, 4294901887u, 4283105410u, 4280179343u, 4294826655u, 4287262347u, 4290889942u, 4292391128u, 4288453796u, 4287317267u, 4281240407u, 4291624704u, 4292124695u, 4294505705u, 4289906848u, 4294965479u, 4282081082u, 4291683554u, 4293457136u, 4292391128u, 4291933183u, 4288383720u, 4294892513u, 4291351511u, 4287327823u, 4290283019u, 4294965488u, 4292391088u, 4293178820u, 4291338952u, 4289917145u, 4286292133u, 4284235827u, 4292111674u, 4293094735u, 4286293176u, 4291231801u, 4291070569u, 4288449357u, 4286267233u, 4279910002u, 4290226341u, 4283080092u, 4294956800u, 4286227290u, 4290957792u, 4282088029u, 4294301822u, 4294308838u, 4292266424u, 4292004028u, 4294111462u, 4287323443u, 4292115038u, 4289020955u, 4292110954u, 4282069297u, 4294308841u, 4287327559u, 4286497006u, 4289906848u, 4291999898u, 4289235567u, 4290569686u, 4291246916u, 4281023055u, 4287248954u, 4287509834u, 4283076058u, 4291250765u, 4281227887u, 4294930273u, 4293325050u, 4293643688u, 4284365947u, 4285218364u, 4282088575u, 4291271274u, 4284238403u, 4292453780u, 4291339662u, 4294567077u, 4292391112u, 4293325567u, 4291042874u, 4294489554u, 4292419873u, 4285336350u, 4282088023u, 4294957235u, 4294937600u, 4289093422u, 4289649108u, 4285307850u, 4294630109u, 4291880953u, 4294101095u, 4290789424u, 4294921549u, 4282299529u, 4281223994u, 4283145427u, 4281032572u, 4290291154u, 4291684601u, 4291373863u, 4285581476u, 4287546601u, 4292929519u, 4287513661u, 4283070550u, 4294956800u, 4286233025u, 4283070619u, 4290172840u, 4289251812u, 4292395900u, 4291420281u, 4283401471u, 4288731352u, 4288987816u, 4286221195u, 4285238436u, 4287253404u, 4285238938u, 4284921808u, 4278233195u, 4278203249u, 4281223974u, 4283068326u, 4283133068u, 4285581424u, 4289940291u, 4287090411u, 4289779944u, 4287013287u, 4282280509u, 4287327580u, 4294493297u, 4282534736u, 4294929259u, 4293076331u, 4291016583u, 4294112244u, 4280106842u, 4294505446u, 4292144631u, 4280963774u, 4291998860u, 4286364072u, 4294201644u, 4282811060u, 4288684272u, 4282168177u, 4291221467u, 4287000469u, 4280960863u, 4285238819u, 4289907122u, 4278202706u, 4286286989u, 4283142028u}};
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
    metal::float2 position_1,
    constant Frame& u
) {
    metal::float4 _e3 = u.canvas;
    metal::float4 _e8 = u.canvas;
    float _e16 = u.view.y;
    float _e23 = u.view.x;
    return (((position_1 - _e3.xy) - (_e8.zw * 0.5)) * metal::float2(_e16, -1.0)) / metal::float2(_e23);
}

bool clipped(
    metal::float2 position_2,
    constant Frame& u
) {
    bool local = {};
    metal::float4 _e3 = u.clip_bounds;
    if (!(metal::any(position_2 < _e3.xy))) {
        metal::float4 _e12 = u.clip_bounds;
        metal::float4 _e16 = u.clip_bounds;
        local = metal::any(position_2 > (_e12.xy + _e16.zw));
    } else {
        local = true;
    }
    bool _e22 = local;
    return _e22;
}

metal::float2 quad_corner(
    uint id_2
) {
    return metal::float2(static_cast<float>(id_2 & 1u), static_cast<float>((id_2 >> 1u) & 1u));
}

metal::uint2 naga_f2u32(metal::float2 value) {
    return static_cast<metal::uint2>(metal::clamp(value, 0.0, 4294967000.0));
}

float stipple(
    metal::float2 sample,
    constant Frame& u
) {
    metal::uint2 pixel = naga_f2u32(metal::max(metal::floor(sample), metal::float2(0.0))) & metal::uint2(15u);
    uint row = u.pattern_mask.inner[pixel.y >> 2u][pixel.y & 3u];
    return static_cast<float>((row >> pixel.x) & 1u);
}

metal::float4 fragment_color(
    metal::float4 position_3,
    constant Frame& u
) {
    bool local_1 = {};
    bool local_2 = {};
    metal::float4 fill = {};
    bool _e2 = clipped(position_3.xy, u);
    if (_e2) {
        metal::discard_fragment();
    }
    float _e6 = u.view.w;
    if (_e6 > 1.5) {
        return metal::float4(0.63, 1.0, 0.85, 0.18);
    }
    float _e17 = u.view.w;
    if (_e17 > 0.5) {
        float _e25 = u.view.z;
        local_1 = _e25 > 3.5;
    } else {
        local_1 = false;
    }
    bool _e29 = local_1;
    if (_e29) {
        float _e33 = u.view.z;
        bool dynamic = _e33 > 4.5;
        metal::float4 _e39 = u.canvas;
        float _e45 = u.viewport.z;
        metal::float2 sample_1 = (position_3.xy - _e39.xy) / metal::float2(dynamic ? 1.0 : _e45);
        float _e50 = stipple(sample_1, u);
        if (dynamic) {
            metal::float2 screen = position_3.xy / metal::float2(u.viewport.z);
            metal::float2 cell = screen - metal::floor(screen / metal::float2(5.0)) * 5.0 - metal::float2(2.5);
            return metal::float4(1.0, 1.0, 1.0, (1.0 - metal::smoothstep(0.65, 1.25, metal::length(cell))) * 0.9);
        }
        metal::float4 _e65 = u.color;
        float _e70 = u.color.w;
        return metal::float4(_e65.xyz, _e70 * _e50);
    }
    float _e76 = u.view.w;
    if (_e76 > 0.5) {
        float _e84 = u.view.z;
        local_2 = _e84 > 1.5;
    } else {
        local_2 = false;
    }
    bool _e88 = local_2;
    if (_e88) {
        metal::float4 _e91 = u.color;
        float _e96 = u.color.w;
        metal::float4 _e100 = u.canvas;
        float _e103 = stipple(position_3.xy - _e100.xy, u);
        return metal::float4(_e91.xyz, _e96 * _e103);
    }
    float _e109 = u.view.w;
    if (_e109 > 0.5) {
        float _e116 = u.viewport.z;
        metal::float2 screen_2 = position_3.xy / metal::float2(_e116);
        metal::float2 cell = (screen_2 - (metal::floor(screen_2 / metal::float2(5.0)) * 5.0)) - metal::float2(2.5);
        return metal::float4(1.0, 1.0, 1.0, (1.0 - metal::smoothstep(0.65, 1.25, metal::length(cell))) * 0.8);
    }
    metal::float4 _e143 = u.color;
    fill = _e143;
    float _e148 = u.view.z;
    if (_e148 > 2.5) {
        fill.w = 0.0;
    } else {
        float _e156 = u.view.z;
        if (_e156 > 0.5) {
            float _e160 = fill.w;
            metal::float4 _e164 = u.canvas;
            float _e170 = u.viewport.z;
            float _e173 = stipple((position_3.xy - _e164.xy) / metal::float2(_e170), u);
            fill.w = _e160 * _e173;
        }
    }
    metal::float4 _e175 = fill;
    return _e175;
}

struct vertex_mainInput {
};
struct vertex_mainOutput {
    metal::float4 member [[position]];
};
vertex vertex_mainOutput vertex_main(
  uint id [[vertex_id]]
, constant Frame& u [[buffer(0)]]
, device type_9 const& vertices [[buffer(1)]]
, constant _mslBufferSizes& _buffer_sizes [[buffer(2)]]
) {
    metal::float4 _e3 = vertices[id];
    metal::float4 _e4 = relative(_e3, u);
    metal::float2 r = _e4.xy + _e4.zw;
    metal::float4 _e10 = u.canvas;
    metal::float4 _e14 = u.canvas;
    float _e22 = u.view.y;
    float _e29 = u.view.x;
    metal::float4 _e32 = screen_position((_e10.xy + (_e14.zw * 0.5)) + ((r * metal::float2(_e22, -1.0)) * _e29), u);
    return vertex_mainOutput { _e32 };
}


struct clear_mainInput {
};
struct clear_mainOutput {
    metal::float4 member_1 [[position]];
};
vertex clear_mainOutput clear_main(
  uint id_1 [[vertex_id]]
, constant Frame& u [[buffer(0)]]
) {
    metal::float4 _e3 = u.rectangle;
    metal::float4 _e7 = u.rectangle;
    metal::float2 _e9 = quad_corner(id_1);
    metal::float4 _e11 = screen_position(metal::mix(_e3.xy, _e7.zw, _e9), u);
    return clear_mainOutput { _e11 };
}


struct fragment_mainInput {
};
struct fragment_mainOutput {
    metal::float4 member_2 [[color(0)]];
};
fragment fragment_mainOutput fragment_main(
  metal::float4 position [[position]]
, constant Frame& u [[buffer(0)]]
) {
    metal::float4 _e1 = fragment_color(position, u);
    float _e7 = u.inherited.x;
    return fragment_mainOutput { metal::float4(_e1.xyz, _e1.w * _e7) };
}
