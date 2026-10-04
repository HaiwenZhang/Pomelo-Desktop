use super::*;

#[test]
fn residual_retains_small_geometry_at_large_coordinate() {
    let coordinate = 100_000.000_123;
    let [high, low] = split_position(coordinate);
    assert!((f64::from(high) + f64::from(low) - coordinate).abs() < 1e-10);
}
