//! Source-space custom pad edges, independent from triangulated fill contours.

use super::PreparedPads;
use crate::tracks::{PrepareError, PreparedTracks, TraceBatch, TraceInstance, TraceLimits};
use pomelo_core::{
    model::{Arc, ObjectId, Segment},
    task::CancellationToken,
};

impl PreparedPads {
    /// Preserve analytic arcs when available; otherwise close every exterior/hole ring.
    /// The temporary segment buffer is included in the preparation memory budget.
    pub fn build_custom_outlines(
        &self,
        limits: TraceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PreparedTracks, PrepareError> {
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let mut count = 0usize;
        for source in &self.custom {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let geometry = source
                .pad
                .custom
                .as_ref()
                .ok_or(PrepareError::Invalid(source.object))?;
            count = if geometry.paths.is_empty() {
                geometry
                    .contours
                    .iter()
                    .fold(count, |count, ring| count.saturating_add(ring.len()))
            } else {
                geometry
                    .paths
                    .iter()
                    .fold(count, |count, path| count.saturating_add(path.len()))
            };
        }
        let stride = size_of::<Segment>() + size_of::<TraceInstance>() + size_of::<TraceBatch>();
        let limit = limits
            .max_instances
            .min(u32::MAX as usize)
            .min(limits.max_bytes / stride);
        if count > limit {
            return Err(PrepareError::Limit {
                actual: count,
                limit,
            });
        }
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation)?;
        for source in &self.custom {
            let geometry = source
                .pad
                .custom
                .as_ref()
                .ok_or(PrepareError::Invalid(source.object))?;
            let mut append = |edge: &Segment| -> Result<(), PrepareError> {
                if cancellation.is_cancelled() {
                    return Err(PrepareError::Cancelled);
                }
                let a = source.pad.to_world(edge.a, source.placement);
                let b = source.pad.to_world(edge.b, source.placement);
                let arc = edge.arc.map(|arc| {
                    let center = source.pad.to_world(arc.center, source.placement);
                    Arc {
                        center,
                        radius: arc.radius,
                        start: (a.y - center.y).atan2(a.x - center.x),
                        sweep: arc.sweep * if source.placement.mirrored { -1.0 } else { 1.0 },
                    }
                });
                edges.push(Segment {
                    id: source.object,
                    // Temporary owner tag; removed before publishing GPU instances.
                    track_id: ObjectId(source.source[0] + 1),
                    layer: source.pad.layer,
                    net: source.net,
                    a,
                    b,
                    width: 0.0,
                    arc,
                    bond_wire: None,
                });
                Ok(())
            };
            if geometry.paths.is_empty() {
                for ring in &geometry.contours {
                    for (index, &a) in ring.iter().enumerate() {
                        append(&Segment {
                            id: source.object,
                            track_id: ObjectId(0),
                            layer: source.pad.layer,
                            net: source.net,
                            a,
                            b: ring[(index + 1) % ring.len()],
                            width: 0.0,
                            arc: None,
                            bond_wire: None,
                        })?;
                    }
                }
            } else {
                for edge in geometry.paths.iter().flatten() {
                    append(edge)?;
                }
            }
        }
        let mut prepared = PreparedTracks::build(&edges, limits, cancellation)?;
        for instance in &mut prepared.instances {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            instance.flags[3] |= if instance.ids[1] == 1 { 16 } else { 32 };
            instance.ids[1] = 0;
        }
        Ok(prepared)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/scene/pads/outlines.rs"]
mod tests;
