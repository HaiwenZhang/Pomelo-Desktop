//! Format-independent PCB data, geometry and document task identity.

pub mod appearance;
pub mod display;
pub mod geometry;
pub mod i18n;
pub mod interaction;
pub mod memory;
pub mod model;
pub mod search;
pub mod task;
pub mod view_state;

rust_i18n::i18n!("../../locales", fallback = "en");

// Stable public paths for existing consumers of the shared core.
pub use geometry::{copper, pad, units};
pub(crate) use interaction::zone_index;
pub use interaction::{picking, picking_index, selection};
