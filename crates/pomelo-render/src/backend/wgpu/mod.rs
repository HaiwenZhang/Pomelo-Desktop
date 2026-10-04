//! wgpu resources and GPUI adapter for the common PCB renderer.
mod driver;
mod shaders;
use super::{NativeGpuContext, NativeGpuRenderer};
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
struct PainterAdapter<R>(R);
impl<R: NativeGpuRenderer> gpui_wgpu::WgpuPainter for PainterAdapter<R> {
    fn paint(
        &mut self,
        context: &mut gpui_wgpu::WgpuPaintContext<'_>,
        data: &(dyn std::any::Any + Send + Sync),
    ) -> anyhow::Result<()> {
        let target = context.target;
        let device = Device::new(context);
        self.0.draw(
            &NativeGpuContext {
                device,
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
pub fn painter(renderer: impl NativeGpuRenderer) -> impl gpui::GpuPainter {
    gpui_wgpu::wgpu_painter(PainterAdapter(renderer))
}
pub const NAME: &str = "GPUI_WGPU";
