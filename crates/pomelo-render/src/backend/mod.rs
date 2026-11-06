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
    ObjectColors, OverlayPass, TraceFrame, TraceRenderer, TraceSelection, TraceStatistics,
    TraceTelemetry,
};
#[cfg(feature = "native-gpu")]
pub use copper_renderer::{
    BoardFrame, BoardRenderer, CopperRenderer, CopperStatistics, CopperTelemetry,
};
#[cfg(feature = "native-gpu")]
pub use pad_renderer::PadRenderer;

/// Bridge the existing board renderer to GPUI's typed native paint callback.
#[cfg(feature = "native-gpu")]
macro_rules! native_painter {
    ($backend:ident, $context:ident, $device:path, $name:literal) => {
        struct PainterAdapter<R>(R);
        impl<R: super::NativeGpuRenderer> gpui::GpuPainter for PainterAdapter<R> {
            type Snapshot = R::Snapshot;
            fn supports_backend(&self, backend: gpui::GpuBackend) -> bool {
                backend == gpui::GpuBackend::$backend
            }
            fn paint(
                &mut self,
                context: &mut gpui::GpuPaintContext<'_>,
                data: &Self::Snapshot,
            ) -> anyhow::Result<()> {
                let context = context.$context()?;
                let target = context.target;
                self.0.draw(
                    &super::NativeGpuContext {
                        device: $device(context),
                        viewport: target.size.map(|v| v as f32),
                        bounds: target.bounds,
                        content_mask: gpui::ContentMask {
                            bounds: target.clip,
                        },
                    },
                    data,
                )
            }
            fn reset(&mut self, _: gpui::GpuResetReason) {
                self.0.reset();
            }
        }
        pub fn painter<R: super::NativeGpuRenderer>(
            renderer: R,
        ) -> impl gpui::GpuPainter<Snapshot = R::Snapshot> {
            PainterAdapter(renderer)
        }
        pub const NAME: &str = $name;
    };
}

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

#[cfg(all(feature = "native-gpu", any(target_os = "macos", test)))]
mod solid_cache;
#[cfg(feature = "native-gpu")]
mod wgsl;
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
#[cfg(feature = "native-gpu")]
mod solid_renderer;
#[cfg(feature = "native-gpu")]
pub use solid_renderer::{SolidFrame, SolidRenderer, SolidTelemetry, display_bounds};
