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
