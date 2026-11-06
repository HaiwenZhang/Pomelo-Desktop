//! Native platform adapter for the existing PCB renderer.
mod driver;
pub(crate) use driver::SolidPipeline;
pub(crate) use driver::{Atlas, Buffer, Device, Pipeline};
native_painter!(Metal, metal, Device::new, "GPUI_METAL");

#[cfg(test)]
#[path = "../../../tests/unit/backend/metal/shaders.rs"]
mod shader_tests;
