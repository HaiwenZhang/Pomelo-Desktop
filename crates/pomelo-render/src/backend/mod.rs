//! Shared PCB drawing policy and platform GPU resource implementations.
#[cfg(feature = "native-gpu")]
mod board;
#[cfg(feature = "native-gpu")]
mod copper_renderer;
#[cfg(feature = "native-gpu")]
mod instances;
#[cfg(feature = "native-gpu")]
mod pad_renderer;
#[cfg(feature = "native-gpu")]
pub use board::{
    OverlayPass, TraceFrame, TraceRenderer, TraceSelection, TraceStatistics, TraceTelemetry,
};
#[cfg(feature = "native-gpu")]
pub use copper_renderer::{
    BoardFrame, BoardRenderer, CopperRenderer, CopperStatistics, CopperTelemetry,
};
#[cfg(feature = "native-gpu")]
pub use pad_renderer::PadRenderer;

#[cfg(all(target_os = "windows", feature = "native-gpu"))]
#[allow(unsafe_code)]
mod d3d11;

#[cfg(feature = "native-gpu")]
mod common;
#[cfg(feature = "native-gpu")]
use common::{
    copper as copper_pipeline, pad as pad_pipeline, probe as probe_pipeline,
    trace as trace_pipeline,
};
#[cfg(all(target_os = "macos", feature = "native-gpu"))]
#[allow(unsafe_code)]
mod metal;

#[cfg(all(target_os = "linux", feature = "native-gpu"))]
mod wgpu;
#[cfg(feature = "native-gpu")]
pub use common::{NativeGpuContext, NativeGpuRenderer};
#[cfg(feature = "native-gpu")]
pub use probe::{PcbProbeRenderer, ProbeScene, ProbeStatistics, ProbeTelemetry};
#[cfg(feature = "native-gpu")]
pub mod native;

#[cfg(all(
    feature = "native-gpu",
    any(target_os = "windows", target_os = "macos", target_os = "linux", test)
))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shader {
    Trace,
    Pad,
    Text,
    Label,
    Copper,
    Probe,
}

#[cfg(feature = "native-gpu")]
mod probe;
