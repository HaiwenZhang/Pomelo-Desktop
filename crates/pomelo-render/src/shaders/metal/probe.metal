// Native Metal shader. Maintained independently from Linux WGSL and Windows HLSL.
// Camera-relative arithmetic requires fast math disabled.
// language: metal2.4
#include <metal_stdlib>
#include <simd/simd.h>

using metal::uint;

struct Frame {
    metal::float4 viewport;
    metal::float4 canvas;
    metal::float4 clip_bounds;
    metal::float4 shape_a;
    metal::float4 shape_b;
    metal::float4 shape_color;
    metal::uint4 shape_kind;
    metal::float4 inherited;
};
struct Out {
    metal::float4 position;
    metal::float3 color;
};

float box_distance(
    metal::float2 p,
    metal::float4 box
) {
    metal::float2 d = metal::abs(p - box.xy) - box.zw;
    return metal::length(metal::max(d, metal::float2(0.0))) + metal::min(metal::max(d.x, d.y), 0.0);
}

metal::float4 fragment_color(
    Out i_1,
    constant Frame& u
) {
    bool local = {};
    float distance = {};
    metal::float2 p_1 = i_1.position.xy;
    metal::float4 _e5 = u.clip_bounds;
    if (!(metal::any(p_1 < _e5.xy))) {
        metal::float4 _e14 = u.clip_bounds;
        metal::float4 _e18 = u.clip_bounds;
        local = metal::any(p_1 > (_e14.xy + _e18.zw));
    } else {
        local = true;
    }
    bool _e24 = local;
    if (_e24) {
        metal::discard_fragment();
    }
    uint _e28 = u.shape_kind.x;
    if (_e28 == 4u) {
        return metal::float4(i_1.color, 1.0);
    }
    uint _e37 = u.shape_kind.x;
    if (_e37 == 3u) {
        metal::float4 _e42 = u.canvas;
        metal::float2 cell = metal::abs(metal::fract((p_1 - _e42.xy) / metal::float2(24.0)) - metal::float2(0.5)) * 24.0;
        float grid = 1.0 - metal::smoothstep(0.4, 1.2, metal::min(cell.x, cell.y));
        return metal::float4(metal::mix(metal::float3(0.025, 0.045, 0.055), metal::float3(0.09, 0.14, 0.17), grid), 1.0);
    }
    uint _e78 = u.shape_kind.x;
    if (_e78 == 0u) {
        metal::float4 _e83 = u.shape_a;
        metal::float4 _e87 = u.shape_a;
        metal::float2 ab = _e83.zw - _e87.xy;
        metal::float4 _e92 = u.shape_a;
        metal::float2 ap = p_1 - _e92.xy;
        float t = metal::clamp(metal::dot(ap, ab) / metal::max(metal::dot(ab, ab), 0.00000000000000000001), 0.0, 1.0);
        float _e109 = u.shape_b.x;
        distance = metal::length(ap - (ab * t)) - _e109;
    } else {
        uint _e114 = u.shape_kind.x;
        if (_e114 == 1u) {
            metal::float4 _e119 = u.shape_a;
            float _e126 = u.shape_a.z;
            distance = metal::length(p_1 - _e119.xy) - _e126;
        } else {
            metal::float4 _e130 = u.shape_a;
            float _e131 = box_distance(p_1, _e130);
            metal::float4 _e134 = u.shape_b;
            float _e135 = box_distance(p_1, _e134);
            distance = metal::max(_e131, -(_e135));
        }
    }
    float _e138 = distance;
    float coverage = 1.0 - metal::smoothstep(-0.75, 0.75, _e138);
    if (coverage < 0.001) {
        metal::discard_fragment();
    }
    metal::float4 _e148 = u.shape_color;
    float _e153 = u.shape_color.w;
    return metal::float4(_e148.xyz, _e153 * coverage);
}
uint naga_mod(uint lhs, uint rhs) {
    return lhs % metal::select(rhs, 1u, rhs == 0u);
}


struct vertex_mainInput {
};
struct vertex_mainOutput {
    metal::float4 position [[position]];
    metal::float3 color [[user(loc0), center_perspective]];
};
vertex vertex_mainOutput vertex_main(
  uint id [[vertex_id]]
, constant Frame& u [[buffer(0)]]
) {
    metal::float2 uv = {};
    Out o = {};
    uv = metal::float2(static_cast<float>(id & 1u), static_cast<float>((id >> 1u) & 1u));
    uint _e14 = u.shape_kind.x;
    if (_e14 == 4u) {
        if (id == 0u) {
            uv = metal::float2(0.5, 0.1);
        } else {
            if (id == 1u) {
                uv = metal::float2(0.1, 0.9);
            } else {
                uv = metal::float2(0.9, 0.9);
            }
        }
    }
    metal::float4 _e32 = u.canvas;
    metal::float2 _e34 = uv;
    metal::float4 _e37 = u.canvas;
    metal::float2 position = _e32.xy + (_e34 * _e37.zw);
    metal::float4 _e45 = u.viewport;
    o.position = metal::float4(((position / _e45.xy) * metal::float2(2.0, -2.0)) + metal::float2(-1.0, 1.0), 0.0, 1.0);
    uint _e62 = u.shape_kind.y;
    uint color_id = naga_mod(id + _e62, 3u);
    o.color = (color_id == 0u) ? metal::float3(1.0, 0.0, 0.0) : ((color_id == 1u) ? metal::float3(0.0, 1.0, 0.0) : metal::float3(0.0, 0.0, 1.0));
    Out _e85 = o;
    const auto _tmp = _e85;
    return vertex_mainOutput { _tmp.position, _tmp.color };
}


struct fragment_mainInput {
    metal::float3 color [[user(loc0), center_perspective]];
};
struct fragment_mainOutput {
    metal::float4 member_1 [[color(0)]];
};
fragment fragment_mainOutput fragment_main(
  fragment_mainInput varyings_1 [[stage_in]]
, metal::float4 position_1 [[position]]
, constant Frame& u [[buffer(0)]]
) {
    const Out i = { position_1, varyings_1.color };
    metal::float4 _e1 = fragment_color(i, u);
    float _e7 = u.inherited.x;
    return fragment_mainOutput { metal::float4(_e1.xyz, _e1.w * _e7) };
}
