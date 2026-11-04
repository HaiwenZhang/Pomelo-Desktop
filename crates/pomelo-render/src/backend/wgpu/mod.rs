//! Native platform adapter for the existing PCB renderer.
mod driver;
mod shaders;
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
native_painter!(Wgpu, wgpu, Device::new, "GPUI_WGPU");
