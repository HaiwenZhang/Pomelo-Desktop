//! Explicit native draw commands. Frame encoders and targets remain borrowed.
#[cfg(target_os = "windows")]
pub(super) use super::super::d3d11::{Atlas, Buffer, Pipeline};
#[cfg(target_os = "macos")]
pub(super) use super::super::metal::{Atlas, Buffer, Pipeline};
#[cfg(target_os = "linux")]
pub(super) use super::super::wgpu::{Atlas, Buffer, Pipeline};
use super::Rect;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StencilMode {
    Inherited,
    Clear,
    Exterior,
    Holes,
    Toggle,
    ApplyOuter,
    ApplyHole,
    ClearScratch,
    Shade,
}
impl StencilMode {
    pub fn state(self, inherited: bool) -> (u32, u32, u32, bool, bool, bool) {
        // read mask, write mask, reference, compare-equal, invert, zero
        match self {
            Self::Inherited if inherited => (3, 0, 1, true, false, false),
            Self::Inherited => (0, 0, 0, false, false, false),
            Self::Clear => (3, 3, 0, false, false, true),
            Self::Exterior => (3, 1, 1, false, false, false),
            Self::Holes => (3, 2, 2, false, false, false),
            Self::Toggle => (2, 2, 0, false, true, false),
            Self::ApplyOuter => (2, 1, 3, true, false, false),
            Self::ApplyHole => (2, 1, 2, true, false, false),
            Self::ClearScratch => (2, 2, 0, false, false, true),
            Self::Shade => (3, 0, 1, true, false, false),
        }
    }
    pub fn color(self) -> bool {
        matches!(self, Self::Inherited | Self::Shade)
    }
}
pub(crate) struct Draw<'a, B = Buffer, A = Atlas> {
    pub uniforms: &'a [u8],
    pub vertices: &'a B,
    pub indices: Option<&'a B>,
    pub start: u32,
    pub count: u32,
    pub instances: u32,
    pub rect: Rect,
    pub stencil: StencilMode,
    pub atlas: Option<&'a A>,
}
pub(crate) struct StencilScope<'a> {
    pub active: &'a std::cell::Cell<bool>,
    pub previous: bool,
}
impl Drop for StencilScope<'_> {
    fn drop(&mut self) {
        self.active.set(self.previous);
    }
}
