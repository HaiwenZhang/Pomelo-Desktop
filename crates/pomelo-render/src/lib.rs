//! PCB rendering preparation. GPU resources are owned by a viewport backend.

pub mod backend;
pub mod frame_timing;
pub mod scene;
pub mod text;
// Retain the crate's existing public API while implementations live in their
// architectural modules. New code may use scene::* and text::instances directly.
pub use scene::{copper, coverage, drills, pads, preparation_memory, tracks};
pub use text::instances as text_instances;

/// Split a board-space double into float32 high and residual components.
/// Both are uploaded once; camera motion must not rebuild static geometry.
pub fn split_position(value: f64) -> [f32; 2] {
    let high = value as f32;
    [high, (value - f64::from(high)) as f32]
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
