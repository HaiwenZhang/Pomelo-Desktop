//! Shared native GPU context and pipeline scheduling.
pub(super) mod copper;
pub(super) mod pad;
pub(super) mod probe;
pub(super) mod trace;
use super::Shader;
#[cfg(target_os = "windows")]
use super::d3d11::Device;
pub(super) mod driver;
#[cfg(target_os = "macos")]
use super::metal::Device;
#[cfg(target_os = "linux")]
use super::wgpu::Device;
use gpui::{Bounds, ContentMask, ScaledPixels};
use std::any::Any;
#[derive(Clone, Copy)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}
pub struct NativeGpuContext<'a> {
    pub(crate) device: Device<'a>,
    pub viewport: [f32; 2],
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}
pub trait NativeGpuRenderer: Send + 'static {
    fn draw(
        &mut self,
        context: &NativeGpuContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()>;
    fn reset(&mut self);
}
