//! Compact straight-stroke preparation; GPU integration is a separate step.
use crate::{
    split_position,
    text::{PreparedTexts, StrokeGlyphs, TextPreparationSummary},
    tracks::PrepareError,
};
use pomelo_core::{
    model::{BoardText, Diagnostic, LayerId, ObjectId},
    task::CancellationToken,
};

/// Four 16-byte vectors. Endpoints retain high/residual precision; a future
/// text shader derives its quad from endpoints and width rather than storing
/// arc data and redundant bounds for every straight font stroke.
#[derive(Debug, PartialEq)]
#[repr(C)]
pub struct TextInstance {
    pub a: [f32; 4],
    pub b: [f32; 4],
    /// Source object, layer, global source stroke ordinal, width bits.
    pub ids: [u32; 4],
    /// Reserved for the matching text shader; always initialized to zero.
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
}

impl PreparedTextInstances {
    /// Explicit CPU experiment. Does not claim shader or viewport support.
    pub fn build(
        texts: &[BoardText],
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        // Bound instances and the worst case of one layer run per stroke.
        // Object identities, diagnostics and fonts have separate budgets.
        let stride = std::mem::size_of::<TextInstance>() + std::mem::size_of::<TextLayerBatch>();
        let count_limit = (max_bytes / stride).min(u32::MAX as usize);
        let count = crate::text::count_text_strokes(
            &texts[..texts.len().min(max_objects)],
            font,
            max_characters,
            cancellation,
        )
        .map_or(0, |(count, _)| count);
        let mut instances = Vec::new();
        instances
            .try_reserve_exact(count.min(count_limit))
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
        })
    }
}
