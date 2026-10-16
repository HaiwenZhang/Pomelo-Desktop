//! wgpu resources and GPUI adapter for the common PCB renderer.
mod driver;
mod shaders;
use super::{NativeGpuContext, NativeGpuRenderer};
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
struct PainterAdapter<R>(R);
impl<R: NativeGpuRenderer> gpui_wgpu::WgpuPainter for PainterAdapter<R> {
    type Snapshot = R::Snapshot;

    fn paint(
        &mut self,
        context: &mut gpui_wgpu::WgpuPaintContext<'_>,
        data: &Self::Snapshot,
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
pub fn painter<R: NativeGpuRenderer>(renderer: R) -> gpui::GpuPainter<R::Snapshot> {
    gpui_wgpu::wgpu_painter(PainterAdapter(renderer))
}
pub const NAME: &str = "GPUI_WGPU";
