//! Platform GPU implementations; only Windows is implemented.
#[cfg(all(target_os = "windows", feature = "native-gpu"))]
pub mod d3d11;
