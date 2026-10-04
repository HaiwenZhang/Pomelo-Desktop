//! One display instance per physical drill placement, independent of copper pad layers.
use crate::{
    pads::{PadBatch, PadInstance, PadLimits, PreparedPads},
    split_position,
    tracks::PrepareError,
};
use pomelo_core::{
    model::{LayerId, Pin, Point, Via},
    pad::PadPlacement,
    task::CancellationToken,
};

pub struct PreparedDrills {
    /// Instances use the analytic pad ABI. Source category/index resolves drill layer scope.
    pub geometry: PreparedPads,
}
impl PreparedDrills {
    pub fn build(
        pins: &[Pin],
        vias: &[Via],
        limits: PadLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let count = pins
            .len()
            .saturating_add(vias.len())
            .saturating_add(vias.iter().filter(|via| via.backdrill.is_some()).count());
        let limit = limits.max_pads.min(u32::MAX as usize);
        if count > limit {
            return Err(PrepareError::Limit {
                actual: count,
                limit,
            });
        }
        let scope_layers = vias.iter().fold(0usize, |count, via| {
            count
                .saturating_add(via.pads.len())
                .saturating_add(via.pads.iter().filter(|pad| pad.backdrill).count())
        });
        let bytes = count
            .saturating_mul(std::mem::size_of::<PadInstance>())
            .saturating_add(std::mem::size_of::<PadBatch>())
            .saturating_add(scope_layers.saturating_mul(std::mem::size_of::<LayerId>()))
            .saturating_add(
                vias.len()
                    .saturating_mul(2 * std::mem::size_of::<Vec<LayerId>>()),
            );
        if bytes > limits.max_bytes {
            return Err(PrepareError::Limit {
                actual: bytes,
                limit: limits.max_bytes,
            });
        }
        let mut analytic = Vec::new();
        analytic
            .try_reserve_exact(count)
            .map_err(|_| PrepareError::Allocation)?;
        let owners = pins
            .iter()
            .enumerate()
            .map(|(index, pin)| {
                (
                    pin.id,
                    pin.net,
                    pin.at,
                    pin.angle,
                    pin.mirrored,
                    pin.drill_shape,
                    0,
                    index,
                )
            })
            .chain(vias.iter().enumerate().map(|(index, via)| {
                (
                    via.id,
                    via.net,
                    via.at,
                    via.angle,
                    via.mirrored,
                    via.drill_shape,
                    1,
                    index,
                )
            }));
        for (object, net, at, angle, mirrored, drill, category, index) in owners {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            if ![at.x, at.y, angle, drill.width, drill.height]
                .into_iter()
                .all(|v| v.is_finite() && (v as f32).is_finite())
                || drill.width < 0.0
                || drill.height < 0.0
            {
                return Err(PrepareError::Invalid(object));
            }
            let Some(pad) = drill.pad() else {
                continue;
            };
            let placement = PadPlacement {
                at,
                angle,
                mirrored,
            };
            let bounds = pad.bounds(placement).ok_or(PrepareError::Invalid(object))?;
            let (sin, cos) = angle.sin_cos();
            let instance = PadInstance {
                center: split_point(at),
                shape: [
                    pad.width as f32,
                    pad.height as f32,
                    pad.corner_radius() as f32,
                    0.0,
                ],
                rotation: [cos as f32, sin as f32, 0.0, 0.0],
                bounds_min: split_point(bounds.min),
                bounds_max: split_point(bounds.max),
                ids: [
                    object.0,
                    LayerId::UNASSIGNED.0,
                    net.0,
                    u32::from(pad.kind.0),
                ],
                source: [
                    category,
                    index as u32,
                    0,
                    u32::from(mirrored) | (u32::from(drill.plated) << 3),
                ],
                drill: [drill.width as f32, drill.height as f32, 0.0, 0.0],
            };
            if instance
                .bounds_min
                .iter()
                .chain(&instance.bounds_max)
                .any(|v| !v.is_finite())
            {
                return Err(PrepareError::Invalid(object));
            }
            analytic.push(instance);
        }
        // Web submits one screen-space pattern per via after all ordinary holes.
        for (index, via) in vias.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let Some(backdrill) = &via.backdrill else {
                continue;
            };
            let diameter = backdrill.definition.display_diameter;
            if ![via.at.x, via.at.y, diameter, via.drill]
                .into_iter()
                .all(|v| v.is_finite() && (v as f32).is_finite())
                || diameter <= 0.0
                || via.drill < 0.0
                || !via.pads.iter().any(|pad| pad.backdrill)
            {
                return Err(PrepareError::Invalid(via.id));
            }
            let radius = diameter * 0.5;
            let instance = PadInstance {
                center: split_point(via.at),
                shape: [diameter as f32, diameter as f32, 0.0, via.drill as f32],
                rotation: [1.0, 0.0, 0.0, 0.0],
                bounds_min: split_point(Point::new(via.at.x - radius, via.at.y - radius)),
                bounds_max: split_point(Point::new(via.at.x + radius, via.at.y + radius)),
                ids: [via.id.0, LayerId::UNASSIGNED.0, via.net.0, 2],
                source: [1, index as u32, 0, 2],
                drill: [via.drill as f32, via.drill as f32, 0.0, 0.0],
            };
            if instance
                .bounds_min
                .iter()
                .chain(&instance.bounds_max)
                .any(|v| !v.is_finite())
            {
                return Err(PrepareError::Invalid(via.id));
            }
            analytic.push(instance);
        }
        let mut batches = Vec::new();
        if !analytic.is_empty() {
            batches
                .try_reserve_exact(1)
                .map_err(|_| PrepareError::Allocation)?;
            batches.push(PadBatch {
                layer: LayerId::UNASSIGNED,
                start: 0,
                count: analytic.len() as u32,
            });
        }
        let mut scopes = Vec::new();
        scopes
            .try_reserve_exact(vias.len())
            .map_err(|_| PrepareError::Allocation)?;
        let mut backdrill_scopes = Vec::new();
        backdrill_scopes
            .try_reserve_exact(vias.len())
            .map_err(|_| PrepareError::Allocation)?;
        for via in vias {
            let mut layers = Vec::new();
            layers
                .try_reserve_exact(via.pads.len())
                .map_err(|_| PrepareError::Allocation)?;
            let mut cut_layers = Vec::new();
            cut_layers
                .try_reserve_exact(via.pads.iter().filter(|pad| pad.backdrill).count())
                .map_err(|_| PrepareError::Allocation)?;
            for (index, pad) in via.pads.iter().enumerate() {
                if index % 1024 == 0 && cancellation.is_cancelled() {
                    return Err(PrepareError::Cancelled);
                }
                layers.push(pad.layer);
                if pad.backdrill {
                    cut_layers.push(pad.layer);
                }
            }
            layers.sort_unstable();
            layers.dedup();
            scopes.push(layers);
            cut_layers.sort_unstable();
            cut_layers.dedup();
            backdrill_scopes.push(cut_layers);
        }
        Ok(Self {
            geometry: PreparedPads {
                analytic,
                custom: vec![],
                batches,
                custom_mesh: None,
                custom_outlines: None,
                drill_scopes: Some(std::sync::Arc::new(scopes)),
                backdrill_scopes: Some(std::sync::Arc::new(backdrill_scopes)),
            },
        })
    }
}
fn split_point(point: Point) -> [f32; 4] {
    let [x, dx] = split_position(point.x);
    let [y, dy] = split_position(point.y);
    [x, y, dx, dy]
}

#[cfg(test)]
#[path = "../../tests/unit/scene/drills.rs"]
pub(crate) mod tests;
