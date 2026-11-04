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
    pub text_height: Option<f64>,
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
        cancel.check()?;
        let limit = max_bytes;
        let owner_count = scene
            .drawings
            .iter()
            .fold(0usize, |n, d| n.saturating_add(d.text_ids.len()));
        let owner_bytes = owner_count.saturating_mul(128);
        if owner_bytes > limit {
            return Err(IndexError::ByteLimit {
                actual: owner_bytes,
                limit,
            });
        }
        let mut owners: std::collections::BTreeMap<_, _> = scene
            .drawings
            .iter()
            .flat_map(|d| d.text_ids.iter().map(move |id| (*id, d.id)))
            .collect();
        // Only drawing-owned text belongs to this index. Board labels can number
        // hundreds of thousands and must not consume drawing storage or budget.
        let mut glyph_count = 0usize;
        for quad in quads {
            cancel.check()?;
            glyph_count += usize::from(owners.contains_key(&quad.text));
        }
        let stroke_count = scene
            .drawings
            .iter()
            .fold(0usize, |n, d| n.saturating_add(d.segments.len()));
        let text_count = scene
            .texts
            .iter()
            .filter(|text| owners.contains_key(&text.id))
            .count();
        let count = stroke_count.saturating_add(text_count);
        // Sorting keys, duplicate text quads and the spatial hierarchy live in
        // different phases. Charge the largest phase, with actual type sizes.
        let glyph_bytes = glyph_count.saturating_mul(size_of::<TextPickQuad>());
        let sort_bytes = stroke_count.saturating_mul(size_of::<(LayerId, bool, usize)>());
        let hierarchy_bytes = BoundsIndex::max_allocation_bytes(count).saturating_add(glyph_bytes);
        let bytes = count
            .saturating_mul(size_of::<DrawingEntry>())
            .saturating_add(
                sort_bytes
                    .max(glyph_bytes.saturating_mul(2))
                    .max(hierarchy_bytes),
            )
            .saturating_add(owner_bytes);
        if bytes > limit {
            return Err(IndexError::ByteLimit {
                actual: bytes,
                limit,
            });
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| IndexError::Allocation)?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(stroke_count)
            .map_err(|_| IndexError::Allocation)?;
        for (drawing, d) in scene.drawings.iter().enumerate() {
            for (segment, s) in d.segments.iter().enumerate() {
                cancel.check()?;
                crate::picking::segment_centerline_distance_mm(s, s.a)?;
                let bounds = s.bounds().ok_or(PathError::Invalid(d.id))?;
                keys.push((d.layer, s.arc.is_some(), entries.len()));
                entries.push(DrawingEntry {
                    owner: d.id,
                    layer: d.layer,
                    sequence: 0,
                    geometry: DrawingGeometry::Stroke { drawing, segment },
                    text_height: None,
                    bounds,
                });
            }
        }
        keys.sort_unstable_by_key(|key| (key.0, key.2));
        let mut first = 0;
        while first < keys.len() {
            let layer = keys[first].0;
            let end = first + keys[first..].partition_point(|key| key.0 == layer);
            // Match native 16,384-stroke batches: lines, then arcs, preserving
            // source sequence within each kind. No per-layer map is retained.
            for chunk in keys[first..end].chunks_mut(16384) {
                chunk.sort_unstable_by_key(|key| (key.1, key.2));
            }
            first = end;
        }
        for (sequence, key) in keys.into_iter().enumerate() {
            entries[key.2].sequence = sequence;
        }
        let mut text_quads = Vec::new();
        text_quads
            .try_reserve_exact(glyph_count)
            .map_err(|_| IndexError::Allocation)?;
        for &quad in quads {
            cancel.check()?;
            if owners.contains_key(&quad.text) {
                text_quads.push(quad);
            }
        }
        text_quads.sort_unstable_by_key(|quad| quad.text);
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(glyph_count)
            .map_err(|_| IndexError::Allocation)?;
        for (sequence, text) in scene.texts.iter().enumerate() {
            cancel.check()?;
            if let Some(owner) = owners.remove(&text.id) {
                let start = text_quads.partition_point(|quad| quad.text < text.id);
                let end = start + text_quads[start..].partition_point(|quad| quad.text == text.id);
                let group = &text_quads[start..end];
                if group.is_empty() {
                    continue;
                }
                // Web broad-phase bounds cover the entire string, including spaces
                // between rotated glyphs; narrow-phase tests individual glyph quads.
                let bounds = Bounds::from_points(group.iter().flat_map(|quad| quad.corners))
                    .ok_or(PathError::Invalid(text.id))?;
                let start = glyphs.len();
                let count = group.len();
                glyphs.extend_from_slice(group);
                entries.push(DrawingEntry {
                    owner,
                    layer: text.layer,
                    sequence,
                    geometry: DrawingGeometry::Glyphs { start, count },
                    text_height: Some(text.height),
                    bounds,
                });
            }
        }
        drop(text_quads);
        let bounds = BoundsIndex::build(entries.iter().map(|e| Some(e.bounds)), cancel)?;
        Ok(Self {
            entries,
            glyphs,
            bounds,
        })
    }
}
impl BoardPickingIndex {
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
