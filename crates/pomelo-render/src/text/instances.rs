//! Source text strokes and picking geometry shared by native backends.
use crate::{
    split_position,
    text::{PreparedTexts, StrokeGlyphs, TextPreparationSummary},
    tracks::PrepareError,
};
use pomelo_core::{
    model::{BoardText, Diagnostic, LayerId, ObjectId},
    task::CancellationToken,
};

/// Four 16-byte vectors. Endpoints retain high/residual precision; the native
/// text shader derives its quad from endpoints and width rather than storing
/// arc data and redundant bounds for every straight font stroke.
#[derive(Debug, PartialEq, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct TextInstance {
    pub a: [f32; 4],
    pub b: [f32; 4],
    /// Source object, layer, global source stroke ordinal, width bits.
    pub ids: [u32; 4],
    /// Drawing owner and bound flag; remaining slots are reserved.
    pub flags: [u32; 4],
}
const _: () = assert!(std::mem::size_of::<TextInstance>() == 64);
const _: () = assert!(std::mem::offset_of!(TextInstance, ids) == 32);
const _: () = assert!(std::mem::offset_of!(TextInstance, flags) == 48);

pub type TextLayerBatch = crate::tracks::TraceBatch;

#[derive(Debug)]
pub struct PreparedTextInstances {
    pub instances: Vec<TextInstance>,
    pub batches: Vec<TextLayerBatch>,
    pub objects: Vec<ObjectId>,
    pub summary: TextPreparationSummary,
    pub pick_quads: Vec<pomelo_core::picking_index::TextPickQuad>,
}

impl PreparedTextInstances {
    pub fn bind_drawing_owners(&mut self, scene: &pomelo_core::model::BoardScene) {
        let owners: std::collections::BTreeMap<_, _> = scene
            .drawings
            .iter()
            .flat_map(|drawing| drawing.text_ids.iter().map(move |id| (id.0, drawing.id.0)))
            .collect();
        for stroke in &mut self.instances {
            if let Some(&owner) = owners.get(&stroke.ids[0]) {
                stroke.flags[0] = owner;
                stroke.flags[1] = 1;
            }
        }
    }

    /// Prepare source text in board coordinates for the native stroke pipeline.
    pub fn build<'a, I>(
        texts: I,
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic>
    where
        I: IntoIterator<Item = &'a BoardText>,
        I::IntoIter: Clone,
    {
        // Bound instances and the worst case of one layer run per stroke.
        // Object identities, diagnostics and fonts have separate budgets.
        let stride = std::mem::size_of::<TextInstance>()
            + std::mem::size_of::<TextLayerBatch>()
            + std::mem::size_of::<pomelo_core::picking_index::TextPickQuad>();
        let count_limit = (max_bytes / stride).min(u32::MAX as usize);
        let texts = texts.into_iter();
        let count = crate::text::count_text_strokes(
            texts.clone().take(max_objects),
            font,
            max_characters,
            cancellation,
        )
        .map_or(0, |(count, _)| count)
        .min(count_limit);
        let mut instances = Vec::new();
        instances
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation.diagnostic())?;
        let mut pick_quads = Vec::new();
        pick_quads
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation.diagnostic())?;
        let mut objects = Vec::new();
        let summary = PreparedTexts::visit_recovering_missing_glyphs(
            texts,
            font,
            max_objects,
            max_characters,
            count_limit,
            cancellation,
            |text, strokes| {
                instances
                    .try_reserve_exact(strokes.len())
                    .map_err(|_| PrepareError::Allocation.diagnostic())?;
                objects
                    .try_reserve(1)
                    .map_err(|_| PrepareError::Allocation.diagnostic())?;
                pick_quads
                    .try_reserve_exact(strokes.len())
                    .map_err(|_| PrepareError::Allocation.diagnostic())?;
                for stroke in strokes {
                    if cancellation.is_cancelled() {
                        return Err(PrepareError::Cancelled.diagnostic());
                    }
                    let split = |point: [f64; 2]| {
                        let [x, dx] = split_position(point[0]);
                        let [y, dy] = split_position(point[1]);
                        [x, y, dx, dy]
                    };
                    let a = split(stroke.a);
                    let b = split(stroke.b);
                    let width = stroke.width as f32;
                    let half_width = stroke.width * 0.5;
                    let bounds_valid = (0..2).all(|axis| {
                        [
                            stroke.a[axis].min(stroke.b[axis]) - half_width,
                            stroke.a[axis].max(stroke.b[axis]) + half_width,
                        ]
                        .into_iter()
                        .all(|value| value.is_finite() && (value as f32).is_finite())
                    });
                    if !a.into_iter().chain(b).all(f32::is_finite)
                        || !width.is_finite()
                        || width < 0.0
                        || !bounds_valid
                    {
                        return Err(PrepareError::Invalid(text.id).diagnostic());
                    }
                    // Each capsule gets a conservative oriented quad. Pen lifts
                    // stay outside the narrow phase, and no MSDF metrics leak in.
                    let dx = stroke.b[0] - stroke.a[0];
                    let dy = stroke.b[1] - stroke.a[1];
                    let length = dx.hypot(dy);
                    let (ux, uy) = if length > 0.0 {
                        (dx / length, dy / length)
                    } else {
                        (1.0, 0.0)
                    };
                    let corner = |point: [f64; 2], along: f64, normal: f64| {
                        pomelo_core::model::Point::new(
                            point[0] + ux * along - uy * normal,
                            point[1] + uy * along + ux * normal,
                        )
                    };
                    pick_quads.push(pomelo_core::picking_index::TextPickQuad {
                        text: text.id,
                        corners: [
                            corner(stroke.a, -half_width, -half_width),
                            corner(stroke.b, half_width, -half_width),
                            corner(stroke.b, half_width, half_width),
                            corner(stroke.a, -half_width, half_width),
                        ],
                    });
                    instances.push(TextInstance {
                        a,
                        b,
                        ids: [
                            text.id.0,
                            text.layer.0,
                            instances.len() as u32,
                            width.to_bits(),
                        ],
                        flags: [0; 4],
                    });
                }
                objects.push(text.id);
                Ok(())
            },
        )?;
        instances.sort_unstable_by_key(|instance| (instance.ids[1], instance.ids[2]));
        let mut batches: Vec<TextLayerBatch> = Vec::new();
        for (index, instance) in instances.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled.diagnostic());
            }
            let layer = LayerId(instance.ids[1]);
            if let Some(batch) = batches.last_mut().filter(|batch| batch.layer == layer) {
                batch.count += 1;
            } else {
                batches
                    .try_reserve(1)
                    .map_err(|_| PrepareError::Allocation.diagnostic())?;
                batches.push(TextLayerBatch {
                    layer,
                    start: index as u32,
                    count: 1,
                    outline: false,
                });
            }
        }
        Ok(Self {
            instances,
            batches,
            objects,
            summary,
            pick_quads,
        })
    }
}
