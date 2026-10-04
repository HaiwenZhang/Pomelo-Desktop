//! Existing Windows FXC and hardware pixel regressions exercise common rendering.
use super::shaders::{compile_shader, msdf_shader_source};
use crate::backend::common::trace::{CHUNK_INSTANCES, Pipeline, UploadedTracks};
use crate::backend::*;
use crate::tracks::PreparedTracks;
use pomelo_core::interaction::Camera;
use std::sync::Arc;
use windows::{Win32::Graphics::Direct3D11::*, core::s};
#[path = "../../../tests/unit/backend/d3d11/trace_pixels.rs"]
mod pixel_tests;
#[path = "../../../tests/unit/backend/d3d11/copper_d3d11.rs"]
mod shaders_copper;
#[path = "../../../tests/unit/backend/d3d11/trace_d3d11.rs"]
mod shaders_trace;
