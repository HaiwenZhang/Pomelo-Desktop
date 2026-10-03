//! Source-space drawing strokes and font-independent glyph quads.
use super::*;
use crate::{
    interaction::spatial::BoundsIndex,
    model::{LayerId, ObjectId, Point},
};

#[derive(Debug, Clone, Copy)]
pub struct TextPickQuad {
    pub text: ObjectId,
    pub corners: [Point; 4],
}
pub(super) enum DrawingGeometry {
    Stroke { drawing: usize, segment: usize },
    Glyphs { start: usize, count: usize },
}
pub(super) struct DrawingEntry {
    pub owner: ObjectId,
    pub layer: LayerId,
    pub sequence: usize,
    pub geometry: DrawingGeometry,
    pub bounds: Bounds,
}
pub(super) struct DrawingIndex {
    pub entries: Vec<DrawingEntry>,
    pub glyphs: Vec<TextPickQuad>,
    pub bounds: BoundsIndex,
}
impl DrawingIndex {
    pub fn build(
        scene: &BoardScene,
        quads: &[TextPickQuad],
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Self, IndexError> {
        let count = scene
            .drawings
            .iter()
            .fold(quads.len(), |n, d| n.saturating_add(d.segments.len()));
        let limit = max_bytes.min(256 * 1024 * 1024);
        if count.saturating_mul(std::mem::size_of::<DrawingEntry>() + 512) > limit {
            return Err(IndexError::ByteLimit {
                actual: count.saturating_mul(std::mem::size_of::<DrawingEntry>() + 512),
                limit,
            });
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| IndexError::Allocation)?;
        let mut layer_counts = std::collections::BTreeMap::<LayerId, usize>::new();
        let mut keys = Vec::new();
        keys.try_reserve_exact(count)
            .map_err(|_| IndexError::Allocation)?;
        for (drawing, d) in scene.drawings.iter().enumerate() {
            for (segment, s) in d.segments.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled.into());
                }
                crate::picking::segment_centerline_distance_mm(s, s.a)?;
                let bounds = s.bounds().ok_or(PathError::Invalid(d.id))?;
                let offset = layer_counts.entry(d.layer).or_default();
                keys.push((
                    d.layer,
                    *offset / 16384,
                    s.arc.is_some(),
                    *offset,
                    entries.len(),
                ));
                *offset += 1;
                entries.push(DrawingEntry {
                    owner: d.id,
                    layer: d.layer,
                    sequence: 0,
                    geometry: DrawingGeometry::Stroke { drawing, segment },
                    bounds,
                });
            }
        }
        keys.sort_unstable();
        for (sequence, key) in keys.into_iter().enumerate() {
            entries[key.4].sequence = sequence;
        }
        let owners: std::collections::BTreeMap<_, _> = scene
            .drawings
            .iter()
            .flat_map(|d| d.text_ids.iter().map(move |id| (*id, d.id)))
            .collect();
        let mut text_quads = std::collections::BTreeMap::<ObjectId, Vec<TextPickQuad>>::new();
        for &quad in quads {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            let group = text_quads.entry(quad.text).or_default();
            group.try_reserve(1).map_err(|_| IndexError::Allocation)?;
            group.push(quad);
        }
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(quads.len())
            .map_err(|_| IndexError::Allocation)?;
        for (sequence, text) in scene.texts.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            if let (Some(&owner), Some(group)) = (owners.get(&text.id), text_quads.remove(&text.id))
            {
                if group.is_empty() {
                    continue;
                }
                // Web broad-phase bounds cover the entire string, including spaces
                // between rotated glyphs; narrow-phase tests individual glyph quads.
                let bounds = Bounds::from_points(group.iter().flat_map(|quad| quad.corners))
                    .ok_or(PathError::Invalid(text.id))?;
                let start = glyphs.len();
                let count = group.len();
                glyphs.extend(group);
                entries.push(DrawingEntry {
                    owner,
                    layer: text.layer,
                    sequence,
                    geometry: DrawingGeometry::Glyphs { start, count },
                    bounds,
                });
            }
        }
        let bounds = BoundsIndex::build(entries.iter().map(|e| Some(e.bounds)), cancel)?;
        Ok(Self {
            entries,
            glyphs,
            bounds,
        })
    }
}
impl SegmentIndex {
    /// Glyph metrics are supplied by the shared renderer; core retains only f64 geometry.
    pub fn with_text_quads(
        mut self,
        quads: &[TextPickQuad],
        cancel: &CancellationToken,
    ) -> Result<Self, IndexError> {
        // Release the previous index before allocating its replacement.
        self.drawings.entries = Vec::new();
        self.drawings.glyphs = Vec::new();
        self.drawings.bounds = BoundsIndex::build(std::iter::empty(), cancel)?;
        self.drawings = DrawingIndex::build(&self.scene, quads, self.drawing_budget, cancel)?;
        Ok(self)
    }
}
