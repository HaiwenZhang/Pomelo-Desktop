//! Native platform adapter for the existing PCB renderer.
mod driver;
mod shaders;
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
native_painter!(D3D11, d3d11, Device::from_paint, "GPUI_D3D11");

#[cfg(test)]
mod tests;
