//! Uniform native painter API; the compilation target selects the implementation.
#[cfg(target_os = "windows")]
pub use super::d3d11::{NAME, painter};
#[cfg(target_os = "macos")]
pub use super::metal::{NAME, painter};
#[cfg(target_os = "linux")]
pub use super::wgpu::{NAME, painter};
pub use super::{
    BoardFrame, BoardRenderer, CopperRenderer, CopperStatistics, CopperTelemetry, NativeGpuContext,
    NativeGpuRenderer, OverlayPass, PadRenderer, PcbProbeRenderer, ProbeScene, ProbeStatistics,
    ProbeTelemetry, TraceFrame, TraceRenderer, TraceSelection, TraceStatistics, TraceTelemetry,
};
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
compile_error!("native-gpu supports Windows, macOS and Linux");
