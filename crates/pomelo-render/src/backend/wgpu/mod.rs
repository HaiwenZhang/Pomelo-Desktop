//! Native platform adapter for the existing PCB renderer.
mod driver;
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline, SolidDriver};
native_painter!(Wgpu, wgpu, Device::new, "GPUI_WGPU");
