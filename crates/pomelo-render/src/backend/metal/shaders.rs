//! Independent MSL sources for the Metal backend.
use crate::backend::Shader;
pub(super) fn source(shader: Shader) -> &'static str {
    match shader {
        Shader::Trace => include_str!("../../shaders/metal/trace.metal"),
        Shader::Pad => include_str!("../../shaders/metal/pad.metal"),
        Shader::Text => include_str!("../../shaders/metal/text.metal"),
        Shader::Label => include_str!("../../shaders/metal/label.metal"),
        Shader::Copper => include_str!("../../shaders/metal/copper.metal"),
        Shader::Probe => include_str!("../../shaders/metal/probe.metal"),
    }
}
#[cfg(test)]
#[path = "../../../tests/unit/backend/metal/shaders.rs"]
mod tests;
