//! Metal resources and GPUI adapter for the common PCB renderer.
mod driver;
mod shaders;
use super::{NativeGpuContext, NativeGpuRenderer};
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
struct PainterAdapter<R>(R);
impl<R: NativeGpuRenderer> gpui_apple::MetalPainter for PainterAdapter<R> {
    type Snapshot = R::Snapshot;

    fn paint(
        &mut self,
        context: &mut gpui_apple::MetalPaintContext<'_>,
        data: &Self::Snapshot,
    ) -> anyhow::Result<()> {
        let device = Device::new(context);
        self.0.draw(
            &NativeGpuContext {
                device,
                viewport: context.target.size.map(|v| v as f32),
                bounds: context.target.bounds,
                content_mask: gpui::ContentMask {
                    bounds: context.target.clip,
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
    gpui_apple::metal_painter(PainterAdapter(renderer))
}
pub const NAME: &str = "GPUI_METAL";
