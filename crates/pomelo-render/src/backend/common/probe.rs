use super::super::probe::{ProbeScene, geometry};
use super::driver::{Buffer, Draw, Pipeline as GpuPipeline, StencilMode};
use super::{Device, NativeGpuContext, Rect, Shader};
pub(crate) struct Pipeline {
    pipeline: GpuPipeline,
    dummy: Buffer,
}
impl Pipeline {
    pub fn new(device: &Device<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pipeline: device.pipeline(Shader::Probe)?,
            dummy: device.buffer(&[0; 16], false)?,
        })
    }
    pub fn draw(&self, context: &NativeGpuContext<'_>, scene: &ProbeScene) -> anyhow::Result<u64> {
        let clip = context.bounds.intersect(&context.content_mask.bounds);
        let rect = Rect {
            left: clip.origin.x.0.floor().max(0.0) as i32,
            top: clip.origin.y.0.floor().max(0.0) as i32,
            right: (clip.origin.x.0 + clip.size.width.0)
                .ceil()
                .min(context.viewport[0]) as i32,
            bottom: (clip.origin.y.0 + clip.size.height.0)
                .ceil()
                .min(context.viewport[1]) as i32,
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Ok(0);
        }
        let geometry = geometry(context, scene);
        let count = geometry.len();
        for uniform in geometry {
            context.device.draw(
                &self.pipeline,
                Draw {
                    uniforms: bytemuck::bytes_of(&uniform),
                    vertices: &self.dummy,
                    indices: None,
                    start: 0,
                    count: if scene.triangle { 3 } else { 4 },
                    instances: 1,
                    rect,
                    stencil: StencilMode::Inherited,
                    atlas: None,
                },
            )?;
        }
        Ok(count as u64)
    }
}
