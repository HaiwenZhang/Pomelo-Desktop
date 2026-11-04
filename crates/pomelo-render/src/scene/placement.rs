//! Prepare source placement coverage once; Quickview switches reuse the same geometry.
use crate::{
    copper::{CopperLimits, PreparedCopper},
    tracks::PrepareError,
};
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    geometry::flatten_path,
    model::{BoardScene, NetId, Zone, ZoneKind},
    task::CancellationToken,
};
pub fn prepare(
    scene: &BoardScene,
    limits: CopperLimits,
    cancel: &CancellationToken,
) -> Result<PreparedCopper, PrepareError> {
    let mut zones = Vec::new();
    let mut retained = 0usize;
    for component in &scene.components {
        if cancel.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let Some(body) = &component.body else {
            continue;
        };
        for region in &body.regions {
            if zones.len() >= limits.max_zones {
                return Err(PrepareError::Limit {
                    actual: zones.len() + 1,
                    limit: limits.max_zones,
                });
            }
            let mut rings = Vec::new();
            for path in &region.paths {
                let ring =
                    flatten_path(path, 0.001, limits.max_vertices, cancel).map_err(
                        |e| match e {
                            pomelo_core::geometry::PathError::Cancelled => PrepareError::Cancelled,
                            pomelo_core::geometry::PathError::PointLimit { actual, limit } => {
                                PrepareError::Limit { actual, limit }
                            }
                            _ => PrepareError::Invalid(component.id),
                        },
                    )?;
                rings.push(ring);
            }
            let mesh = CopperMesh::build(
                &rings,
                &[],
                &MeshLimits {
                    max_allocation_bytes: limits.max_bytes.saturating_sub(retained),
                    ..Default::default()
                },
                cancel,
            )
            .map_err(|e| match e {
                pomelo_core::copper::MeshError::Cancelled => PrepareError::Cancelled,
                pomelo_core::copper::MeshError::Limit { actual, limit, .. } => {
                    PrepareError::Limit { actual, limit }
                }
                pomelo_core::copper::MeshError::Allocation => PrepareError::Allocation,
                _ => PrepareError::Invalid(component.id),
            })?;
            retained = retained.saturating_add(mesh.allocation_bytes());
            if retained > limits.max_bytes {
                return Err(PrepareError::Limit {
                    actual: retained,
                    limit: limits.max_bytes,
                });
            }
            zones.push(Zone {
                id: component.id,
                layer: body.layer,
                net: NetId(0),
                kind: ZoneKind::Unknown,
                paths: vec![],
                mesh,
            });
        }
    }
    PreparedCopper::build(&zones, limits, cancel)
}
