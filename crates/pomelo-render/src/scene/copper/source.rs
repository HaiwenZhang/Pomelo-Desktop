//! Immutable source-backed copper with bounded, exact upload conversion.
use super::*;
use pomelo_core::model::BoardScene;
use std::{borrow::Cow, sync::Arc};

#[derive(Debug)]
struct SourceSpan {
    zone: usize,
    vertex_start: usize,
    index_start: usize,
}
#[derive(Debug)]
pub(crate) struct CopperSource {
    scene: Arc<BoardScene>,
    spans: Vec<SourceSpan>,
    vertices: usize,
    indices: usize,
}
pub(super) fn metadata_bytes(zones: usize) -> usize {
    zones.saturating_mul(size_of::<SourceSpan>())
}
impl PreparedCopper {
    /// Retain source geometry and batch metadata; convert only requested upload
    /// ranges. The borrowed scene allocation is managed by its existing owner.
    pub fn build_scene(
        scene: Arc<BoardScene>,
        limits: CopperLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        let mut result = Self::build_impl(&scene.zones, limits, cancel, false)?;
        let mut spans = reserve(scene.zones.len())?;
        let (mut vertices, mut indices) = (0usize, 0usize);
        for (zone, value) in scene.zones.iter().enumerate() {
            check_cancelled(cancel)?;
            spans.push(SourceSpan {
                zone,
                vertex_start: vertices,
                index_start: indices,
            });
            vertices += value.mesh.vertices.len();
            indices += value.mesh.indices.len();
        }
        result.source = Some(CopperSource {
            scene,
            spans,
            vertices,
            indices,
        });
        Ok(result)
    }
    pub fn vertex_count(&self) -> usize {
        self.source
            .as_ref()
            .map_or(self.vertices.len(), |source| source.vertices)
    }
    pub fn index_count(&self) -> usize {
        self.source
            .as_ref()
            .map_or(self.indices.len(), |source| source.indices)
    }
    pub(super) fn source_allocation_bytes(&self) -> usize {
        self.source
            .as_ref()
            .map_or(0, |source| metadata_bytes(source.spans.capacity()))
    }
    pub(crate) fn vertex_block(
        &self,
        range: Range<usize>,
    ) -> Result<Cow<'_, [CopperVertex]>, PrepareError> {
        let invalid = || PrepareError::Invalid(ObjectId(0));
        if range.start > range.end || range.end > self.vertex_count() {
            return Err(invalid());
        }
        let Some(source) = &self.source else {
            return Ok(Cow::Borrowed(&self.vertices[range]));
        };
        let mut output = reserve(range.len())?;
        let first = source.spans.partition_point(|span| {
            span.vertex_start + source.scene.zones[span.zone].mesh.vertices.len() <= range.start
        });
        for span in &source.spans[first..] {
            if span.vertex_start >= range.end {
                break;
            }
            let mesh = &source.scene.zones[span.zone].mesh;
            let start = range.start.saturating_sub(span.vertex_start);
            let end = range
                .end
                .saturating_sub(span.vertex_start)
                .min(mesh.vertices.len());
            for point in &mesh.vertices[start..end] {
                let [x, dx] = split_position(point.x);
                let [y, dy] = split_position(point.y);
                output.push(CopperVertex {
                    position: [x, y, dx, dy],
                });
            }
        }
        if output.len() != range.len() {
            return Err(invalid());
        }
        Ok(Cow::Owned(output))
    }
    /// Read one GPU-format vertex without materializing the source mesh.
    pub(crate) fn vertex_at(&self, index: usize) -> Option<CopperVertex> {
        let Some(source) = &self.source else {
            return self.vertices.get(index).copied();
        };
        if index >= source.vertices {
            return None;
        }
        let first = source.spans.partition_point(|span| {
            span.vertex_start + source.scene.zones[span.zone].mesh.vertices.len() <= index
        });
        let span = source.spans.get(first)?;
        let point = source.scene.zones[span.zone]
            .mesh
            .vertices
            .get(index - span.vertex_start)?;
        let [x, dx] = split_position(point.x);
        let [y, dy] = split_position(point.y);
        Some(CopperVertex {
            position: [x, y, dx, dy],
        })
    }
    pub(crate) fn index_block(&self, range: Range<usize>) -> Result<Cow<'_, [u32]>, PrepareError> {
        let invalid = || PrepareError::Invalid(ObjectId(0));
        if range.start > range.end || range.end > self.index_count() {
            return Err(invalid());
        }
        let Some(source) = &self.source else {
            return Ok(Cow::Borrowed(&self.indices[range]));
        };
        let mut output = reserve(range.len())?;
        let first = source.spans.partition_point(|span| {
            span.index_start + source.scene.zones[span.zone].mesh.indices.len() <= range.start
        });
        for span in &source.spans[first..] {
            if span.index_start >= range.end {
                break;
            }
            let mesh = &source.scene.zones[span.zone].mesh;
            let start = range.start.saturating_sub(span.index_start);
            let end = range
                .end
                .saturating_sub(span.index_start)
                .min(mesh.indices.len());
            output.extend(
                mesh.indices[start..end]
                    .iter()
                    .map(|index| *index + span.vertex_start as u32),
            );
        }
        if output.len() != range.len() {
            return Err(invalid());
        }
        Ok(Cow::Owned(output))
    }
}
