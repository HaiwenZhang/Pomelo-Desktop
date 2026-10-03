//! Network material colors shared by UI, CPU copper batches and GPU instances.
//! Mirrors Pomelo Web's pcb-net-colors.ts; source NetId, not row order, selects the color.
use pomelo_core::{display::ColorMode, model::NetId};

const NET_COLORS: [u32; 203] = [
    0xff0d5e3a, 0xff1a3a5f, 0xff2aae6f, 0xffe6c24d, 0xffc94a5e, 0xffdcc6b5, 0xffd4af37, 0xff2e5d8c,
    0xff9e2f2e, 0xff1a3b2e, 0xffd9c6b3, 0xff2e7d32, 0xff2e5d4a, 0xff2a5c82, 0xffa8d865, 0xffbc1e46,
    0xffc83737, 0xffd8bfd8, 0xffb8a996, 0xffb5a0c2, 0xff9d4e5c, 0xff2e5a88, 0xff8db67a, 0xfff5f0e6,
    0xfff5e6e0, 0xfff0d9d0, 0xff2f4f4f, 0xfff57c00, 0xff4a76a8, 0xffb0d4e3, 0xffd9c6b0, 0xff3a5f4b,
    0xff8c8c8c, 0xffa0d6b4, 0xff3b4f5c, 0xffe6f1f5, 0xffc75a6b, 0xff3ab08c, 0xffe2f0cb, 0xfff0f8ff,
    0xffd9cbb8, 0xff6cc788, 0xffff007f, 0xff4b0082, 0xff1e5a8f, 0xfffdda9f, 0xff8a6e8b, 0xffc1c8d6,
    0xffd8b0d8, 0xff9c9ca4, 0xff8b4513, 0xff2e8b57, 0xffccff00, 0xffd4a017, 0xfff8f4e9, 0xffb2c8a0,
    0xfffff8e7, 0xff3b5f3a, 0xffcde4e2, 0xffe8f4f0, 0xffd8b0d8, 0xffd1b3ff, 0xff9b8ae8, 0xfffedbe1,
    0xffc8d3d7, 0xff8b6e4f, 0xffb8860b, 0xfffff8f0, 0xffd8b0b0, 0xffe4b5c4, 0xffc8a2c8, 0xffb2f0d9,
    0xff7ba0a5, 0xff5c4033, 0xffd46d3a, 0xffe36d4f, 0xff7ba4b8, 0xffc70039, 0xffc48a69, 0xff9c8b4d,
    0xff7b3f61, 0xff1a3e72, 0xffb7a8a5, 0xff4a9d9c, 0xffffd700, 0xff7aa35a, 0xffc2d1e0, 0xff3b7a5d,
    0xfff5d87e, 0xfff5f3e6, 0xffd6c9b8, 0xffd2c8bc, 0xfff2f0e6, 0xff8b5d33, 0xffd47a5e, 0xffa5441b,
    0xffd46a6a, 0xff3b3131, 0xfff5f3e9, 0xff8b6d47, 0xff7ec0ee, 0xffb2c8a0, 0xffd2b89a, 0xffa88a6f,
    0xffbce5d6, 0xffc73b44, 0xff2b3a4f, 0xff8a3a3a, 0xff8e354a, 0xff4a8dda, 0xffc74a4d, 0xff2e5a6f,
    0xffff6f61, 0xffe6f0fa, 0xffebcda8, 0xff5e3c7b, 0xff6b3e3c, 0xff3b7c7f, 0xffc79a6a, 0xff5c4a43,
    0xffd9a594, 0xffc8a58e, 0xfff9e4a5, 0xffd8b0c8, 0xffe6f2ff, 0xffc41e3a, 0xfff8b5d2, 0xffd92121,
    0xff6d0b1e, 0xff3b7a57, 0xffffd8b3, 0xffff8c00, 0xffa65f2e, 0xffaed9d4, 0xff6c9bca, 0xfffadadd,
    0xffd0e7f9, 0xfff2c867, 0xffc04030, 0xffff4d4d, 0xff3eb489, 0xff2e4b3a, 0xff4b9cd3, 0xff2b5f7c,
    0xffb8a5d2, 0xffcde8f9, 0xffc92b27, 0xff70c8a4, 0xff8ec4e9, 0xffe0e7ef, 0xff8e443d, 0xff4a7856,
    0xffffd700, 0xff7ab9c1, 0xff4a789b, 0xffb6d7a8, 0xffa8c9e4, 0xffd8c37c, 0xffc9e079, 0xff4f84ff,
    0xffa0d8d8, 0xffa4c2a8, 0xff7a8b8b, 0xff6b8ca4, 0xff8a4b9c, 0xff6b8e9a, 0xff66b7d0, 0xff00a86b,
    0xff003371, 0xff2e4b26, 0xff4a6fa6, 0xff4b6c8c, 0xff70c870, 0xffb34b43, 0xff87ceeb, 0xffb0d8e8,
    0xff86a1a7, 0xff3e6a3d, 0xff8b6d5c, 0xfff8c471, 0xff424b50, 0xffff6b6b, 0xffe3256b, 0xffc3b787,
    0xfff2f3f4, 0xff1d3f5a, 0xfff8f3e6, 0xffd4edf7, 0xff2a52be, 0xffd2b48c, 0xff7cb9a8, 0xfff4512c,
    0xff4682b4, 0xffa020f0, 0xff3cb371, 0xffc6d7db, 0xff866f95, 0xff2a475f, 0xff6b8e23, 0xffb2c9b2,
    0xff003152, 0xff7b8c8d, 0xff4b8f8c,
];

/// Unassigned objects retain their source layer material.
pub fn net_color(net: NetId) -> Option<[f32; 4]> {
    if net.0 == 0 {
        return None;
    }
    let value = NET_COLORS[net.0 as usize % NET_COLORS.len()];
    Some([
        ((value >> 16) & 255) as f32 / 255.0,
        ((value >> 8) & 255) as f32 / 255.0,
        (value & 255) as f32 / 255.0,
        1.0,
    ])
}

pub fn copper_color(mode: ColorMode, layer: [f32; 4], net: NetId) -> [f32; 4] {
    if mode == ColorMode::Net
        && let Some(mut color) = net_color(net)
    {
        color[3] = layer[3];
        color
    } else {
        layer
    }
}

/// Inject the same integer palette into Shader Model 5, avoiding a second color table.
#[cfg(all(target_os = "windows", feature = "native-gpu"))]
pub(crate) fn hlsl_library() -> String {
    use std::fmt::Write as _;
    let mut source = format!(
        "static const uint pcb_net_colors[{}] = {{",
        NET_COLORS.len()
    );
    for color in NET_COLORS {
        // Writing to String cannot fail.
        let _ = write!(source, "0x{color:08x}u,");
    }
    source.push_str(&format!(
        "}};\nfloat3 pcb_net_color(uint net) {{\nuint rgb = pcb_net_colors[net % {}u];\nreturn float3((rgb >> 16u) & 255u, (rgb >> 8u) & 255u, rgb & 255u) / 255.0;\n}}\n",
        NET_COLORS.len()
    ));
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assigned_networks_match_the_web_palette_and_preserve_material_alpha() {
        let layer = [0.1, 0.2, 0.3, 0.35];
        assert_eq!(net_color(NetId(0)), None);
        assert_eq!(copper_color(ColorMode::Net, layer, NetId(0)), layer);
        assert_eq!(copper_color(ColorMode::Layer, layer, NetId(1)), layer);
        assert_eq!(
            net_color(NetId(1)),
            Some([26.0 / 255.0, 58.0 / 255.0, 95.0 / 255.0, 1.0])
        );
        for net in 1..=NET_COLORS.len() as u32 {
            let first = net_color(NetId(net)).unwrap();
            assert_eq!(net_color(NetId(net + NET_COLORS.len() as u32)), Some(first));
            assert!(
                first
                    .into_iter()
                    .all(|channel| channel.is_finite() && (0.0..=1.0).contains(&channel))
            );
            let material = copper_color(ColorMode::Net, layer, NetId(net));
            assert_eq!(&material[..3], &first[..3]);
            assert_eq!(material[3], layer[3]);
        }
        assert!(net_color(NetId(u32::MAX)).is_some());
    }
}
