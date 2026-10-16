//! Windows resources and GPUI adapter for the common PCB renderer.
mod driver;
mod shaders;
use super::{NativeGpuContext, NativeGpuRenderer};
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
use gpui::ContentMask;

struct PainterAdapter<R>(R);

impl<R: NativeGpuRenderer> gpui_windows::D3D11Painter for PainterAdapter<R> {
    type Snapshot = R::Snapshot;

    fn paint(
        &mut self,
        context: &mut gpui_windows::D3D11PaintContext<'_>,
        data: &Self::Snapshot,
    ) -> anyhow::Result<()> {
        self.0.draw(
            &NativeGpuContext {
                device: Device::from_paint(context),
                viewport: context.target.size.map(|size| size as f32),
                bounds: context.target.bounds,
                content_mask: ContentMask {
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

/// Wrap an application renderer in the fork's window-scoped D3D11 painter.
pub fn painter<R: NativeGpuRenderer>(renderer: R) -> gpui::GpuPainter<R::Snapshot> {
    gpui_windows::d3d11_painter(PainterAdapter(renderer))
}

pub const NAME: &str = "GPUI_D3D11";

#[cfg(test)]
mod tests;
