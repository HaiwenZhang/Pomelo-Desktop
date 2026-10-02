//! Native PCB experiment. Geometry, shader semantics, pipeline and cache all live here.

// Direct3D COM calls require unsafe FFI. Keep this exception limited to the backend.
mod board;
#[allow(unsafe_code)]
mod pad_d3d11;
mod pad_renderer;
pub use pad_renderer::PadRenderer;
#[allow(unsafe_code)]
mod copper_d3d11;
mod copper_renderer;
#[allow(unsafe_code)]
mod pipeline;
#[allow(unsafe_code)]
mod trace_d3d11;
pub use board::{TraceFrame, TraceRenderer, TraceSelection, TraceStatistics, TraceTelemetry};
pub use copper_renderer::{
    BoardFrame, BoardRenderer, CopperRenderer, CopperStatistics, CopperTelemetry,
};

use anyhow::Context as _;
use gpui::{NativeGpuContext, NativeGpuRenderer};
use serde::Serialize;
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Small immutable frame snapshot, passed opaquely through GPUI.
pub struct ProbeScene {
    pub(crate) palette: u32,
    pub(crate) triangle: bool,
}

impl ProbeScene {
    pub fn pcb(palette: u32) -> Self {
        Self {
            palette: palette % 3,
            triangle: false,
        }
    }
    pub fn triangle(palette: u32) -> Self {
        Self {
            palette: palette % 3,
            triangle: true,
        }
    }
}

#[derive(Default)]
pub struct ProbeTelemetry {
    pipeline_builds: AtomicU64,
    resets: AtomicU64,
    draw_calls: AtomicU64,
}

#[derive(Serialize)]
pub struct ProbeStatistics {
    pub pipeline_builds: u64,
    pub resets: u64,
    pub draw_calls: u64,
}

impl ProbeTelemetry {
    pub fn snapshot(&self) -> ProbeStatistics {
        ProbeStatistics {
            pipeline_builds: self.pipeline_builds.load(Ordering::Relaxed),
            resets: self.resets.load(Ordering::Relaxed),
            draw_calls: self.draw_calls.load(Ordering::Relaxed),
        }
    }
}

pub struct PcbProbeRenderer {
    pipeline: Option<pipeline::Pipeline>,
    telemetry: Arc<ProbeTelemetry>,
}

impl PcbProbeRenderer {
    pub fn new(telemetry: Arc<ProbeTelemetry>) -> Self {
        Self {
            pipeline: None,
            telemetry,
        }
    }
}

impl NativeGpuRenderer for PcbProbeRenderer {
    fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()> {
        let scene = data
            .downcast_ref::<ProbeScene>()
            .context("GPU_PROBE_PAYLOAD_INVALID")?;
        if self.pipeline.is_none() {
            self.pipeline = Some(pipeline::Pipeline::new(context.device)?);
            self.telemetry
                .pipeline_builds
                .fetch_add(1, Ordering::Relaxed);
        }
        let draws = self
            .pipeline
            .as_ref()
            .context("GPU_PIPELINE_MISSING")?
            .draw(context, scene)?;
        self.telemetry
            .draw_calls
            .fetch_add(draws, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) {
        self.pipeline = None;
        self.telemetry.resets.fetch_add(1, Ordering::Relaxed);
    }
}

/// Explicit constant-buffer layout, paired with `pcb.hlsl`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct Uniforms {
    viewport: [f32; 4],
    canvas: [f32; 4],
    clip: [f32; 4],
    a: [f32; 4],
    b: [f32; 4],
    color: [f32; 4],
    kind: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<Uniforms>() == 112);
const _: () = assert!(std::mem::offset_of!(Uniforms, kind) == 96);

/// A test scene in 160 × 80 board units, fitted without distorting circles or squares.
pub(crate) fn geometry(context: &NativeGpuContext<'_>, scene: &ProbeScene) -> Vec<Uniforms> {
    let bounds = context.bounds;
    let clip = bounds.intersect(&context.content_mask.bounds);
    let canvas = [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ];
    let base = Uniforms {
        viewport: [context.viewport[0], context.viewport[1], 0.0, 0.0],
        canvas,
        clip: [
            clip.origin.x.0,
            clip.origin.y.0,
            clip.size.width.0,
            clip.size.height.0,
        ],
        kind: [3, scene.palette, 0, 0],
        ..Default::default()
    };
    if scene.triangle {
        return vec![Uniforms {
            kind: [4, scene.palette, 0, 0],
            ..base
        }];
    }
    let scale = (canvas[2] / 160.0).min(canvas[3] / 80.0);
    let origin = [
        canvas[0] + (canvas[2] - 160.0 * scale) * 0.5,
        canvas[1] + (canvas[3] - 80.0 * scale) * 0.5,
    ];
    let point = |x: f32, y: f32| [origin[0] + x * scale, origin[1] + y * scale];
    let colors = [
        [0.98, 0.55, 0.12, 1.0],
        [0.10, 0.78, 0.83, 1.0],
        [0.40, 0.74, 0.31, 1.0],
    ];
    let color = |index: usize| colors[(index + scene.palette as usize) % colors.len()];
    let a = point(12.0, 26.0);
    let b = point(43.0, 54.0);
    let disc = point(80.0, 40.0);
    let outer = point(133.0, 40.0);
    vec![
        base,
        Uniforms {
            kind: [0, 0, 0, 0],
            a: [a[0], a[1], b[0], b[1]],
            b: [2.5 * scale, 0.0, 0.0, 0.0],
            color: color(0),
            ..base
        },
        Uniforms {
            kind: [1, 0, 0, 0],
            a: [disc[0], disc[1], 14.0 * scale, 0.0],
            color: color(1),
            ..base
        },
        Uniforms {
            kind: [2, 0, 0, 0],
            a: [outer[0], outer[1], 18.0 * scale, 18.0 * scale],
            b: [outer[0], outer[1], 8.0 * scale, 8.0 * scale],
            color: color(2),
            ..base
        },
    ]
}
