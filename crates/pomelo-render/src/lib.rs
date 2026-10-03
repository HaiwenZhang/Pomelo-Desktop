//! PCB rendering preparation. GPU resources are owned by a viewport backend.

pub mod backend;
pub mod scene;
pub mod text;
// Retain the crate's existing public API while implementations live in their
// architectural modules. New code may use scene::* and text::instances directly.
pub use scene::{copper, coverage, drills, pads, tracks};
pub use text::instances as text_instances;

/// Split a board-space double into float32 high and residual components.
/// Both are uploaded once; camera motion must not rebuild static geometry.
pub fn split_position(value: f64) -> [f32; 2] {
    let high = value as f32;
    [high, (value - f64::from(high)) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_retains_small_geometry_at_large_coordinate() {
        let coordinate = 100_000.000_123;
        let [high, low] = split_position(coordinate);
        assert!((f64::from(high) + f64::from(low) - coordinate).abs() < 1e-10);
    }
}
