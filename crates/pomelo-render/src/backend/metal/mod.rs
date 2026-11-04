//! Native platform adapter for the existing PCB renderer.
mod driver;
mod shaders;
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
native_painter!(Metal, metal, Device::new, "GPUI_METAL");
