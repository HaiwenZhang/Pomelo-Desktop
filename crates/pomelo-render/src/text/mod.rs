//! Shared MSDF board text and labels; legacy stroke layout for compatibility checks.
mod ansi;
pub mod instances;
pub use ansi::AnsiStrokeFont;
mod layout;
pub use layout::*;
pub mod msdf;
