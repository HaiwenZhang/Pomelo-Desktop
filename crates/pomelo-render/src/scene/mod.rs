//! Shared immutable PCB batches and coverage contracts.
pub mod board3d;
pub mod colors;
pub mod copper;
pub mod coverage;
pub mod curves;
pub mod drills;
#[cfg(feature = "kernel-3d")]
pub mod kernel3d;
pub mod pads;
pub mod placement;
pub mod preparation_memory;
pub mod residency;
pub mod snapshot;
pub mod solid;
pub mod thumbnail;
pub mod tracks;
