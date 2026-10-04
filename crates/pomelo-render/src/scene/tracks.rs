//! Immutable analytic trace batches. Camera changes never rebuild this buffer.

use pomelo_core::{
    i18n::MessageKey,
    model::{Diagnostic, LayerId, ObjectId, Point, Segment},
    task::CancellationToken,
};

use crate::split_position;
use std::borrow::Borrow;

/// Eight 16-byte vectors; keep the matching shader declaration in this order.
/// Point vectors store [x_high, y_high, x_low, y_low].
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct TraceInstance {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub center: [f32; 4],
    /// [radius_high, radius_low, start_radians, signed_sweep_radians].
    pub arc: [f32; 4],
    pub bounds_min: [f32; 4],
    pub bounds_max: [f32; 4],
    /// [object, track, layer, net].
    pub ids: [u32; 4],
    /// [kind (0 line / 1 arc), combined source index, width bits, flags].
    /// Flags: full arc=1, long arc=2, board outline=4, zone=8,
    /// custom pin/via edge=16/32, drawing=64, reliable Dynamic zone=128.
    pub flags: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<TraceInstance>() == 128);
const _: () = assert!(std::mem::offset_of!(TraceInstance, ids) == 96);
const _: () = assert!(std::mem::offset_of!(TraceInstance, flags) == 112);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceBatch {
    pub layer: LayerId,
    pub start: u32,
    pub count: u32,
    pub outline: bool,
}

#[derive(Debug)]
pub struct PreparedTracks {
    pub instances: Vec<TraceInstance>,
    pub batches: Vec<TraceBatch>,
}

#[derive(Debug, Clone, Copy)]
pub struct TraceLimits {
    pub max_instances: usize,
    pub max_bytes: usize,
}

impl Default for TraceLimits {
    fn default() -> Self {
        Self {
            max_instances: 4_000_000,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("RENDER_PREPARE_CANCELLED")]
    Cancelled,
    #[error("RENDER_PREPARE_LIMIT actual={actual} limit={limit}")]
    Limit { actual: usize, limit: usize },
    #[error("RENDER_PREPARE_COUNT_LIMIT actual={actual} limit={limit}")]
    CountLimit { actual: usize, limit: usize },
    #[error("RENDER_PREPARE_INVALID object={0:?}")]
    Invalid(ObjectId),
    #[error("RENDER_PREPARE_ALLOCATION")]
    Allocation,
}

impl PrepareError {
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Cancelled => Diagnostic::error("RENDER_PREPARE_CANCELLED", MessageKey::Cancelled),
            Self::CountLimit { actual, limit } => {
                let mut diagnostic =
                    Diagnostic::error("RENDER_PREPARE_COUNT_LIMIT", MessageKey::GeometryCountLimit);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::Limit { actual, limit } => {
                let mut diagnostic =
                    Diagnostic::error("RENDER_PREPARE_LIMIT", MessageKey::GeometryLimit);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::Invalid(id) => {
                let mut diagnostic =
                    Diagnostic::error("RENDER_PREPARE_INVALID", MessageKey::RenderPrepareInvalid);
                diagnostic.object = Some(*id);
                diagnostic.message = diagnostic.message.arg("object", id.0);
                diagnostic
            }
            Self::Allocation => Diagnostic::error(
                "RENDER_PREPARE_ALLOCATION",
                MessageKey::RenderPrepareAllocation,
            ),
        }
    }
}

fn split_point(point: Point) -> [f32; 4] {
    let [x, dx] = split_position(point.x);
    let [y, dy] = split_position(point.y);
    [x, y, dx, dy]
}

impl PreparedTracks {
    /// Exact zone boundaries, including holes, retain their owning zone identity.
    /// Compact polygon rings are used only when no analytic paths are retained.
    pub fn build_zone_outlines(
        zones: &[pomelo_core::model::Zone],
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let count = zones.iter().fold(0usize, |count, zone| {
            count.saturating_add(if zone.paths.is_empty() {
                zone.mesh.vertices.len()
            } else {
                zone.paths.iter().map(Vec::len).sum()
            })
        });
        if count > limits.max_instances {
            return Err(PrepareError::CountLimit {
                actual: count,
                limit: limits.max_instances,
            });
        }
        let dynamic_count = zones
            .iter()
            .filter(|zone| zone.kind == pomelo_core::model::ZoneKind::Dynamic)
            .count();
        let bytes = count
            .saturating_mul(size_of::<TraceInstance>() + size_of::<Segment>())
            .saturating_add(dynamic_count.saturating_mul(size_of::<ObjectId>()));
        if bytes > limits.max_bytes {
            return Err(PrepareError::Limit {
                actual: bytes,
                limit: limits.max_bytes,
            });
        }
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation)?;
        let mut dynamic_ids = Vec::new();
        dynamic_ids
            .try_reserve_exact(dynamic_count)
            .map_err(|_| PrepareError::Allocation)?;
        for zone in zones {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            if zone.kind == pomelo_core::model::ZoneKind::Dynamic {
                dynamic_ids.push(zone.id);
            }
            if zone.paths.is_empty() {
                for range in zone.mesh.ring_offsets.windows(2) {
                    let ring = &zone.mesh.vertices[range[0] as usize..range[1] as usize];
                    for (index, &a) in ring.iter().enumerate() {
                        if cancellation.is_cancelled() {
                            return Err(PrepareError::Cancelled);
                        }
                        edges.push(Segment {
                            id: zone.id,
                            track_id: ObjectId(0),
                            layer: zone.layer,
                            net: zone.net,
                            a,
                            b: ring[(index + 1) % ring.len()],
                            width: 0.0,
                            arc: None,
                            bond_wire: None,
                        });
                    }
                }
            } else {
                for edge in zone.paths.iter().flatten() {
                    if cancellation.is_cancelled() {
                        return Err(PrepareError::Cancelled);
                    }
                    edges.push(Segment {
                        id: zone.id,
                        track_id: ObjectId(0),
                        layer: zone.layer,
                        net: zone.net,
                        a: edge.a,
                        b: edge.b,
                        width: 0.0,
                        arc: edge.arc,
                        bond_wire: None,
                    });
                }
            }
        }
        dynamic_ids.sort_unstable();
        let mut prepared = Self::build(&edges, limits, cancellation)?;
        for (index, instance) in prepared.instances.iter_mut().enumerate() {
            if index % 256 == 0 && cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            instance.flags[3] |= 8;
            if dynamic_ids
                .binary_search(&ObjectId(instance.ids[0]))
                .is_ok()
            {
                // Resident kind metadata lets shape alpha change without re-uploading edges.
                instance.flags[3] |= 128;
            }
        }
        Ok(prepared)
    }
    /// Bounds include round caps and directed arc extrema. Stable source order
    /// is preserved within each layer, including multiple segments of one track.
    pub fn build(
        segments: &[Segment],
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        Self::build_with_outline(segments, &[], limits, cancellation)
    }

    /// Borrow both source collections; board outlines form separate batches after traces.
    pub fn build_with_outline(
        segments: &[Segment],
        outline: &[Segment],
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        Self::build_segments(
            segments.iter().chain(outline),
            segments.len().saturating_add(outline.len()),
            segments.len(),
            limits,
            cancellation,
        )
    }

    /// Prepare dimension strokes without cloning or flattening source geometry.
    /// Draw these in a separate renderer with PCB selection disabled.
    pub fn build_drawings(
        drawings: &[pomelo_core::model::BoardDrawing],
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        let mut count = 0usize;
        for drawing in drawings {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            count = count
                .checked_add(drawing.segments.len())
                .ok_or(PrepareError::Limit {
                    actual: usize::MAX,
                    limit: limits.max_instances,
                })?;
        }
        let mut prepared = Self::build_segments(
            drawings.iter().flat_map(|drawing| drawing.segments.iter()),
            count,
            usize::MAX,
            limits,
            cancellation,
        )?;
        let mut owners = Vec::new();
        owners
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation)?;
        for drawing in drawings {
            for _ in &drawing.segments {
                owners.push(drawing.id);
            }
        }
        for instance in &mut prepared.instances {
            instance.ids[0] = owners[instance.flags[1] as usize].0;
            instance.ids[1] = 0;
            instance.ids[3] = 0;
            instance.flags[3] |= 64;
        }
        // Web flushes each 16,384-stroke layer chunk as lines followed by arcs.
        let mut offsets = std::collections::BTreeMap::new();
        let mut ranks = std::collections::BTreeMap::new();
        for i in &prepared.instances {
            ranks.insert(i.flags[1], 0usize);
        }
        let mut source_order: Vec<_> = prepared
            .instances
            .iter()
            .map(|i| (i.flags[1], i.ids[2]))
            .collect();
        source_order.sort_unstable();
        for (source, layer) in source_order {
            let offset = offsets.entry(layer).or_insert(0usize);
            ranks.insert(source, *offset);
            *offset += 1;
        }
        prepared.instances.sort_unstable_by_key(|i| {
            (
                i.ids[2],
                ranks[&i.flags[1]] / 16384,
                i.flags[0],
                ranks[&i.flags[1]],
            )
        });
        Ok(prepared)
    }

    /// Text strokes use their own renderer source; PCB selection must be disabled
    /// on its draw frame even when source numeric object IDs overlap.
    pub fn build_texts(
        texts: &crate::text::PreparedTexts,
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        let mut cursor = 0;
        for batch in &texts.batches {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            if batch.strokes.start != cursor
                || batch.strokes.end < cursor
                || batch.strokes.end > texts.strokes.len()
            {
                return Err(PrepareError::Invalid(batch.object));
            }
            cursor = batch.strokes.end;
        }
        if cursor != texts.strokes.len() {
            return Err(PrepareError::Invalid(
                texts
                    .batches
                    .last()
                    .map_or(ObjectId(0), |batch| batch.object),
            ));
        }
        let segments = texts.batches.iter().flat_map(|batch| {
            texts.strokes[batch.strokes.clone()]
                .iter()
                .map(move |stroke| Segment {
                    id: batch.object,
                    track_id: ObjectId(0),
                    layer: batch.layer,
                    net: pomelo_core::model::NetId(0),
                    a: Point {
                        x: stroke.a[0],
                        y: stroke.a[1],
                    },
                    b: Point {
                        x: stroke.b[0],
                        y: stroke.b[1],
                    },
                    width: stroke.width,
                    arc: None,
                    bond_wire: None,
                })
        });
        Self::build_segments(segments, cursor, usize::MAX, limits, cancellation)
    }

    /// Convert one text object at a time, retaining only final GPU instances.
    pub fn build_source_texts(
        texts: &[pomelo_core::model::BoardText],
        font: &impl crate::text::StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<(Self, Vec<ObjectId>, crate::text::TextPreparationSummary), Diagnostic> {
        let mut instances: Vec<TraceInstance> = Vec::new();
        let mut objects = Vec::new();
        let instance_limit = limits.max_instances.min(u32::MAX as usize);
        let byte_stride = std::mem::size_of::<TraceInstance>() + std::mem::size_of::<TraceBatch>();
        let capacity_limit = instance_limit.min(limits.max_bytes / byte_stride);
        // Size the final allocation without retaining transformed board geometry.
        // Growing a large Vec can temporarily keep both old and new allocations.
        // Sizing is advisory: semantic failures must be reported by the ordered
        // visitor, where an earlier object/budget error takes precedence.
        let stroke_count = crate::text::count_text_strokes(
            &texts[..texts.len().min(max_objects)],
            font,
            max_characters,
            cancellation,
        )
        .map_or(0, |(count, _)| count);
        instances
            .try_reserve_exact(stroke_count.min(capacity_limit))
            .map_err(|_| PrepareError::Allocation.diagnostic())?;
        let summary = crate::text::PreparedTexts::visit_recovering_missing_glyphs(
            texts,
            font,
            max_objects,
            max_characters,
            instance_limit,
            cancellation,
            |text, strokes| {
                let count = instances.len().checked_add(strokes.len()).ok_or_else(|| {
                    PrepareError::Limit {
                        actual: usize::MAX,
                        limit: limits.max_bytes,
                    }
                    .diagnostic()
                })?;
                let bytes = count.saturating_mul(byte_stride);
                if bytes > limits.max_bytes {
                    return Err(PrepareError::Limit {
                        actual: bytes,
                        limit: limits.max_bytes,
                    }
                    .diagnostic());
                }
                let segments = strokes.iter().map(|stroke| Segment {
                    id: text.id,
                    track_id: ObjectId(0),
                    layer: text.layer,
                    net: pomelo_core::model::NetId(0),
                    a: Point::new(stroke.a[0], stroke.a[1]),
                    b: Point::new(stroke.b[0], stroke.b[1]),
                    width: stroke.width,
                    arc: None,
                    bond_wire: None,
                });
                let mut object =
                    Self::build_segments(segments, strokes.len(), usize::MAX, limits, cancellation)
                        .map_err(|error| error.diagnostic())?;
                let offset = instances.len() as u32;
                for instance in &mut object.instances {
                    instance.flags[1] += offset;
                }
                if count > instances.capacity() {
                    let capacity = count
                        .max(instances.capacity().saturating_mul(2))
                        .min(capacity_limit);
                    instances
                        .try_reserve_exact(capacity - instances.len())
                        .map_err(|_| PrepareError::Allocation.diagnostic())?;
                }
                objects
                    .try_reserve(1)
                    .map_err(|_| PrepareError::Allocation.diagnostic())?;
                instances.append(&mut object.instances);
                objects.push(text.id);
                Ok(())
            },
        )?;
        let tracks =
            Self::finish_instances(instances, cancellation).map_err(|error| error.diagnostic())?;
        Ok((tracks, objects, summary))
    }

    fn build_segments(
        segments: impl Iterator<Item = impl Borrow<Segment>>,
        count: usize,
        outline_start: usize,
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let count_limit = limits.max_instances.min(u32::MAX as usize);
        if count > count_limit {
            return Err(PrepareError::Limit {
                actual: count,
                limit: count_limit,
            });
        }
        // Bound the worst case: every instance belongs to a different layer.
        // Sorting is in-place and does not allocate a second instance array.
        let bytes = count
            .checked_mul(std::mem::size_of::<TraceInstance>() + std::mem::size_of::<TraceBatch>())
            .ok_or(PrepareError::Limit {
                actual: usize::MAX,
                limit: limits.max_bytes,
            })?;
        if bytes > limits.max_bytes {
            return Err(PrepareError::Limit {
                actual: bytes,
                limit: limits.max_bytes,
            });
        }
        let mut instances = Vec::new();
        instances
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation)?;
        for (index, segment) in segments.enumerate() {
            let segment = segment.borrow();
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let valid = [
                segment.a.x,
                segment.a.y,
                segment.b.x,
                segment.b.y,
                segment.width,
            ]
            .into_iter()
            .all(|value| value.is_finite() && (value as f32).is_finite())
                && segment.width >= 0.0;
            if !valid {
                return Err(PrepareError::Invalid(segment.id));
            }
            let (center, arc, kind) = if let Some(arc) = segment.arc {
                if ![arc.center.x, arc.center.y, arc.radius, arc.start, arc.sweep]
                    .into_iter()
                    .all(|value| value.is_finite() && (value as f32).is_finite())
                    || arc.radius <= 0.0
                    || arc.sweep.abs() > std::f64::consts::TAU + 1e-12
                {
                    return Err(PrepareError::Invalid(segment.id));
                }
                let [radius, residual] = split_position(arc.radius);
                (
                    split_point(arc.center),
                    [
                        radius,
                        residual,
                        arc.start.rem_euclid(std::f64::consts::TAU) as f32,
                        arc.sweep as f32,
                    ],
                    1,
                )
            } else {
                ([0.0; 4], [0.0; 4], 0)
            };
            let bounds = segment.bounds().ok_or(PrepareError::Invalid(segment.id))?;
            if !bounds.is_valid() {
                return Err(PrepareError::Invalid(segment.id));
            }
            let bounds_min = split_point(bounds.min);
            let bounds_max = split_point(bounds.max);
            if !bounds_min.into_iter().chain(bounds_max).all(f32::is_finite) {
                return Err(PrepareError::Invalid(segment.id));
            }
            instances.push(TraceInstance {
                a: split_point(segment.a),
                b: split_point(segment.b),
                center,
                arc,
                bounds_min,
                bounds_max,
                ids: [
                    segment.id.0,
                    segment.track_id.0,
                    segment.layer.0,
                    segment.net.0,
                ],
                flags: [
                    kind,
                    index as u32,
                    (segment.width as f32).to_bits(),
                    (u32::from(index >= outline_start) << 2)
                        | segment.arc.map_or(0, |arc| {
                            u32::from(arc.sweep.abs() >= std::f64::consts::TAU - 1e-12)
                                | (u32::from(arc.sweep.abs() > std::f64::consts::PI) << 1)
                        }),
                ],
            });
        }
        Self::finish_instances(instances, cancellation)
    }

    fn finish_instances(
        mut instances: Vec<TraceInstance>,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        let mut batches: Vec<TraceBatch> = Vec::new();
        instances.sort_unstable_by_key(|instance| {
            (
                instance.flags[3] & 4,
                instance.ids[2],
                instance.flags[0],
                instance.flags[1],
            )
        });
        // Allocate batch metadata for actual layer/outline runs, rather than
        // one slot per stroke (millions of strokes can share a few layers).
        let mut batch_count = 0;
        let mut previous_key = None;
        for instance in &instances {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let key = (instance.flags[3] & 4, instance.ids[2]);
            if previous_key != Some(key) {
                batch_count += 1;
                previous_key = Some(key);
            }
        }
        batches
            .try_reserve_exact(batch_count)
            .map_err(|_| PrepareError::Allocation)?;
        for (index, instance) in instances.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let layer = LayerId(instance.ids[2]);
            let outline = instance.flags[3] & 4 != 0;
            if let Some(batch) = batches
                .last_mut()
                .filter(|batch| batch.layer == layer && batch.outline == outline)
            {
                batch.count += 1;
            } else {
                batches.push(TraceBatch {
                    layer,
                    start: index as u32,
                    count: 1,
                    outline,
                });
            }
        }
        Ok(Self { instances, batches })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/scene/tracks.rs"]
mod tests;
