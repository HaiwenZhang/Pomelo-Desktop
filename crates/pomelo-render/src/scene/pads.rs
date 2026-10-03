//! Immutable pad placement data; custom contours retain their own geometry contract.

use crate::{split_position, tracks::PrepareError};
mod outlines;
use pomelo_core::{
    model::{Bounds, NetId, ObjectId, Pad, Pin, Point, Via},
    pad::PadPlacement,
    task::CancellationToken,
};

/// Eight 16-byte vectors. Coordinates store high X/Y followed by residual X/Y.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct PadInstance {
    pub center: [f32; 4],
    /// Width, height, corner parameter, donut inner diameter, all in mm.
    pub shape: [f32; 4],
    pub rotation: [f32; 4],
    pub bounds_min: [f32; 4],
    pub bounds_max: [f32; 4],
    /// Object, layer, net, source shape family.
    pub ids: [u32; 4],
    /// Owner category (pin=0/via=1), owner index, pad index, flags:
    /// mirrored, backdrill, backdrill base, plated drill in bits 0..3.
    pub source: [u32; 4],
    /// Drill width/height, independent from the donut opening; reserved components zero.
    pub drill: [f32; 4],
}
const _: () = assert!(std::mem::size_of::<PadInstance>() == 128);
const _: () = assert!(std::mem::offset_of!(PadInstance, ids) == 80);

#[derive(Debug)]
pub struct CustomPadInstance {
    pub object: ObjectId,
    pub net: NetId,
    pub placement: PadPlacement,
    /// Cloning a pad shares its immutable custom geometry, without cloning contours.
    pub pad: Pad,
    pub bounds: Bounds,
    pub source: [u32; 4],
}

#[derive(Debug)]
pub struct PreparedPads {
    pub analytic: Vec<PadInstance>,
    pub custom: Vec<CustomPadInstance>,
    pub batches: Vec<PadBatch>,
    pub custom_mesh: Option<std::sync::Arc<crate::copper::PreparedCopper>>,
    /// Exact custom boundaries; shared by non-filled and selection/hover passes.
    pub custom_outlines: Option<std::sync::Arc<crate::tracks::PreparedTracks>>,
    /// Via owner index -> sorted source pad layers, populated only for drill geometry.
    pub drill_scopes: Option<std::sync::Arc<Vec<Vec<pomelo_core::model::LayerId>>>>,
    /// Via owner index -> cut layers, populated only for independent backdrill patterns.
    pub backdrill_scopes: Option<std::sync::Arc<Vec<Vec<pomelo_core::model::LayerId>>>>,
}

#[derive(Debug, Clone, Copy)]
pub struct PadBatch {
    pub layer: pomelo_core::model::LayerId,
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PadLimits {
    pub max_pads: usize,
    /// Output allocations only; shared source geometry and future GPU resources are separate.
    pub max_bytes: usize,
}
impl Default for PadLimits {
    fn default() -> Self {
        Self {
            max_pads: 4_000_000,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

impl PreparedPads {
    /// Prepare independent exterior/hole coverage meshes for custom pad placements.
    /// Exact source paths remain in `custom` for later curved-edge rendering and picking.
    pub fn build_custom_meshes(
        &self,
        limits: crate::copper::CopperLimits,
        mesh_limits: &pomelo_core::copper::MeshLimits,
        cancellation: &CancellationToken,
    ) -> Result<crate::copper::PreparedCopper, PrepareError> {
        use pomelo_core::{
            copper::{CopperMesh, MeshError},
            model::Zone,
        };
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let mut points = 0usize;
        let mut indices = 0usize;
        let mut ring_count = 0usize;
        for source in &self.custom {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let geometry = source
                .pad
                .custom
                .as_ref()
                .ok_or(PrepareError::Invalid(source.object))?;
            for ring in &geometry.contours {
                ring_count = ring_count.saturating_add(1);
                points = points.saturating_add(ring.len());
                indices = indices.saturating_add(ring.len().saturating_sub(2).saturating_mul(3));
            }
        }
        for (actual, limit) in [
            (self.custom.len(), limits.max_zones),
            (points, limits.max_vertices.min(u32::MAX as usize)),
            (indices, limits.max_indices.min(u32::MAX as usize)),
        ] {
            if actual > limit {
                return Err(PrepareError::Limit { actual, limit });
            }
        }
        // Conservative retained CPU meshes, transformed contours, metadata and GPU preparation.
        // Triangulator scratch is separately bounded by mesh_limits for each contour set.
        let bytes = points
            .saturating_mul(64)
            .saturating_add(indices.saturating_mul(8))
            .saturating_add(ring_count.saturating_mul(128))
            .saturating_add(self.custom.len().saturating_mul(1024));
        if bytes > limits.max_bytes {
            return Err(PrepareError::Limit {
                actual: bytes,
                limit: limits.max_bytes,
            });
        }
        let mut zones = Vec::new();
        zones
            .try_reserve_exact(self.custom.len())
            .map_err(|_| PrepareError::Allocation)?;
        for source in &self.custom {
            let geometry = source
                .pad
                .custom
                .as_ref()
                .ok_or(PrepareError::Invalid(source.object))?;
            let mut rings = Vec::new();
            rings
                .try_reserve_exact(geometry.contours.len())
                .map_err(|_| PrepareError::Allocation)?;
            for ring in &geometry.contours {
                let mut transformed = Vec::new();
                transformed
                    .try_reserve_exact(ring.len())
                    .map_err(|_| PrepareError::Allocation)?;
                for (index, point) in ring.iter().enumerate() {
                    if index % 1024 == 0 && cancellation.is_cancelled() {
                        return Err(PrepareError::Cancelled);
                    }
                    transformed.push(source.pad.to_world(*point, source.placement));
                }
                rings.push(transformed);
            }
            let mut mesh =
                CopperMesh::build(&rings, &[], mesh_limits, cancellation).map_err(|error| {
                    match error {
                        MeshError::Cancelled => PrepareError::Cancelled,
                        MeshError::Allocation => PrepareError::Allocation,
                        MeshError::Limit { actual, limit, .. } => {
                            PrepareError::Limit { actual, limit }
                        }
                        MeshError::Invalid { .. } => PrepareError::Invalid(source.object),
                    }
                })?;
            mesh.curved = geometry
                .paths
                .iter()
                .flatten()
                .any(|segment| segment.arc.is_some());
            zones.push(Zone {
                id: source.object,
                layer: source.pad.layer,
                net: source.net,
                paths: vec![],
                mesh,
            });
        }
        let mut prepared = crate::copper::PreparedCopper::build(&zones, limits, cancellation)?;
        for batch in &mut prepared.batches {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let source = &self.custom[batch.source_index as usize];
            batch.selected_object = if source.source[0] == 0 {
                pomelo_core::selection::SelectedObject::Pin(source.object)
            } else {
                pomelo_core::selection::SelectedObject::Via(source.object)
            };
        }
        Ok(prepared)
    }

    pub fn build(
        pins: &[Pin],
        vias: &[Via],
        limits: PadLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, PrepareError> {
        if cancellation.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let mut analytic_count = 0usize;
        let mut custom_count = 0usize;
        for pad in pins.iter().flat_map(|pin| &pin.pads).chain(
            vias.iter()
                .flat_map(|via| via.pads.iter().filter(|pad| !pad.backdrill)),
        ) {
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            if pad.custom.is_some() {
                custom_count = custom_count.saturating_add(1);
            } else {
                analytic_count = analytic_count.saturating_add(1);
            }
        }
        let count = analytic_count.saturating_add(custom_count);
        let count_limit = limits.max_pads.min(u32::MAX as usize);
        if count > count_limit || pins.len().max(vias.len()) > u32::MAX as usize {
            return Err(PrepareError::Limit {
                actual: count.max(pins.len()).max(vias.len()),
                limit: count_limit,
            });
        }
        let bytes = analytic_count
            .checked_mul(std::mem::size_of::<PadInstance>())
            .and_then(|a| {
                custom_count
                    .checked_mul(std::mem::size_of::<CustomPadInstance>())
                    .and_then(|b| a.checked_add(b))
            })
            .ok_or(PrepareError::Limit {
                actual: usize::MAX,
                limit: limits.max_bytes,
            })?;
        let bytes = bytes
            .checked_add(analytic_count.saturating_mul(std::mem::size_of::<PadBatch>()))
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
        let mut output = Self {
            analytic: Vec::new(),
            custom: Vec::new(),
            batches: Vec::new(),
            custom_mesh: None,
            custom_outlines: None,
            drill_scopes: None,
            backdrill_scopes: None,
        };
        output
            .analytic
            .try_reserve_exact(analytic_count)
            .map_err(|_| PrepareError::Allocation)?;
        output
            .custom
            .try_reserve_exact(custom_count)
            .map_err(|_| PrepareError::Allocation)?;
        output
            .batches
            .try_reserve_exact(analytic_count)
            .map_err(|_| PrepareError::Allocation)?;
        let owners = pins
            .iter()
            .enumerate()
            .map(|(index, pin)| {
                (
                    pin.id,
                    pin.net,
                    PadPlacement {
                        at: pin.at,
                        angle: pin.angle,
                        mirrored: pin.mirrored,
                    },
                    pin.drill_shape,
                    pin.pads.as_slice(),
                    0,
                    index,
                )
            })
            .chain(vias.iter().enumerate().map(|(index, via)| {
                (
                    via.id,
                    via.net,
                    PadPlacement {
                        at: via.at,
                        angle: via.angle,
                        mirrored: via.mirrored,
                    },
                    via.drill_shape,
                    via.pads.as_ref(),
                    1,
                    index,
                )
            }));
        for (object, net, placement, drill, pads, category, index) in owners {
            for (pad_index, pad) in pads.iter().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(PrepareError::Cancelled);
                }
                // The display marker is submitted once in the global drill pass.
                if category == 1 && pad.backdrill {
                    continue;
                }
                if let Some(custom) = &pad.custom {
                    if custom.contours.is_empty()
                        || custom.contours.iter().any(|ring| ring.len() < 3)
                    {
                        return Err(PrepareError::Invalid(object));
                    }
                    for (index, point) in custom.contours.iter().flatten().enumerate() {
                        if index % 1024 == 0 && cancellation.is_cancelled() {
                            return Err(PrepareError::Cancelled);
                        }
                        let world = pad.to_world(*point, placement);
                        if ![point.x, point.y, world.x, world.y]
                            .into_iter()
                            .all(|v| v.is_finite() && (v as f32).is_finite())
                        {
                            return Err(PrepareError::Invalid(object));
                        }
                    }
                }
                let bounds = pad.bounds(placement).ok_or(PrepareError::Invalid(object))?;
                let source = [
                    category,
                    index as u32,
                    pad_index as u32,
                    u32::from(placement.mirrored)
                        | (u32::from(pad.backdrill) << 1)
                        | (u32::from(pad.backdrill_base) << 2)
                        | (u32::from(drill.plated) << 3),
                ];
                if !pad.supported()
                    || pad.width < 0.0
                    || pad.height < 0.0
                    || ![drill.width, drill.height, pad.corner]
                        .into_iter()
                        .all(|v| v.is_finite() && (v as f32).is_finite())
                    || drill.width < 0.0
                    || drill.height < 0.0
                {
                    return Err(PrepareError::Invalid(object));
                }
                if pad.custom.is_some() {
                    output.custom.push(CustomPadInstance {
                        object,
                        net,
                        placement,
                        pad: pad.clone(),
                        bounds,
                        source,
                    });
                    continue;
                }
                let center = pad.to_world(Point::default(), placement);
                if pad.analytic_distance(center, placement).is_none() {
                    return Err(PrepareError::Invalid(object));
                }
                let (sin, cos) = placement.angle.sin_cos();
                let instance = PadInstance {
                    center: split_point(center),
                    shape: [
                        pad.width as f32,
                        pad.height as f32,
                        pad.corner_radius() as f32,
                        pad.inner_diameter.unwrap_or(0.0) as f32,
                    ],
                    rotation: [cos as f32, sin as f32, 0.0, 0.0],
                    bounds_min: split_point(bounds.min),
                    bounds_max: split_point(bounds.max),
                    ids: [object.0, pad.layer.0, net.0, u32::from(pad.kind.0)],
                    source,
                    drill: [drill.width as f32, drill.height as f32, 0.0, 0.0],
                };
                if instance
                    .center
                    .iter()
                    .chain(&instance.shape)
                    .chain(&instance.bounds_min)
                    .chain(&instance.bounds_max)
                    .any(|v| !v.is_finite())
                {
                    return Err(PrepareError::Invalid(object));
                }
                output.analytic.push(instance);
            }
        }
        output
            .analytic
            .sort_unstable_by_key(|pad| (pad.ids[1], pad.source[0], pad.source[1], pad.source[2]));
        output.custom.sort_unstable_by_key(|pad| {
            (pad.pad.layer, pad.source[0], pad.source[1], pad.source[2])
        });
        for (index, instance) in output.analytic.iter().enumerate() {
            let layer = pomelo_core::model::LayerId(instance.ids[1]);
            if let Some(batch) = output
                .batches
                .last_mut()
                .filter(|batch| batch.layer == layer)
            {
                batch.count += 1;
            } else {
                output.batches.push(PadBatch {
                    layer,
                    start: index as u32,
                    count: 1,
                });
            }
        }
        if !output.custom.is_empty() {
            output.custom_outlines = Some(std::sync::Arc::new(output.build_custom_outlines(
                crate::tracks::TraceLimits {
                    max_bytes: limits.max_bytes.saturating_sub(bytes),
                    ..crate::tracks::TraceLimits::default()
                },
                cancellation,
            )?));
        }
        Ok(output)
    }
}

fn split_point(point: Point) -> [f32; 4] {
    let [x, dx] = split_position(point.x);
    let [y, dy] = split_position(point.y);
    [x, y, dx, dy]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::model::{DrillShape, LayerId, PadKind};

    fn pin() -> Pin {
        let mut pad = Pad::circle(LayerId(7), 4.0);
        pad.offset = Point::new(3.0, 0.0);
        pad.kind = PadKind::DONUT;
        pad.inner_diameter = Some(2.0);
        Pin {
            id: ObjectId(42),
            owner_id: ObjectId(9),
            net: NetId(5),
            name: String::new(),
            reference: String::new(),
            at: Point::new(100_000.000_123, 20.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: true,
            drill: 0.5,
            drill_shape: DrillShape {
                width: 0.5,
                height: 0.5,
                plated: true,
            },
            pads: vec![pad],
            stackup_region: None,
            die: None,
        }
    }

    #[test]
    fn analytic_instances_keep_precision_offset_identity_and_separate_holes() {
        let prepared = PreparedPads::build(
            &[pin()],
            &[],
            PadLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        let instance = prepared.analytic[0];
        assert!(
            (f64::from(instance.center[0]) + f64::from(instance.center[2]) - 100_003.000_123).abs()
                < 1e-9
        );
        assert_eq!(instance.center[1], 20.0);
        assert_eq!(instance.ids, [42, 7, 5, 25]);
        assert_eq!(instance.source, [0, 0, 0, 9]);
        assert_eq!(instance.shape[3], 2.0);
        assert_eq!(instance.drill[0], 0.5);
        assert!(prepared.custom.is_empty());
    }

    #[test]
    fn budgets_and_cancellation_precede_invalid_pad_processing() {
        let mut source = pin();
        source.pads[0].width = f64::NAN;
        assert!(matches!(
            PreparedPads::build(
                std::slice::from_ref(&source),
                &[],
                PadLimits {
                    max_bytes: 0,
                    ..PadLimits::default()
                },
                &CancellationToken::default()
            ),
            Err(PrepareError::Limit { .. })
        ));
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            PreparedPads::build(&[source], &[], PadLimits::default(), &cancellation),
            Err(PrepareError::Cancelled)
        ));
    }

    #[test]
    fn custom_contours_remain_shared_and_apply_mirror_before_rotation() {
        use pomelo_core::model::CustomPadGeometry;
        use std::sync::Arc;
        let mut source = pin();
        let geometry = Arc::new(CustomPadGeometry {
            contours: vec![vec![
                Point::new(0.0, 0.0),
                Point::new(2.0, 0.0),
                Point::new(0.0, 1.0),
            ]],
            paths: vec![],
        });
        source.at = Point::new(10.0, 20.0);
        source.pads[0].custom = Some(Arc::clone(&geometry));
        source.pads[0].kind = PadKind::CUSTOM;
        let output = PreparedPads::build(
            &[source],
            &[],
            PadLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(output.analytic.is_empty());
        let custom = &output.custom[0];
        assert!(Arc::ptr_eq(custom.pad.custom.as_ref().unwrap(), &geometry));
        assert!((custom.bounds.min.x - 13.0).abs() < 1e-12);
        assert!((custom.bounds.max.x - 14.0).abs() < 1e-12);
        assert!((custom.bounds.max.y - 22.0).abs() < 1e-12);
    }

    #[test]
    fn malformed_custom_vertices_are_not_hidden_by_bounds_reduction() {
        use pomelo_core::model::CustomPadGeometry;
        use std::sync::Arc;
        for invalid in [f64::NAN, f64::INFINITY, f64::MAX] {
            let mut source = pin();
            source.pads[0].custom = Some(Arc::new(CustomPadGeometry {
                contours: vec![vec![
                    Point::default(),
                    Point::new(1.0, 0.0),
                    Point::new(invalid, 1.0),
                ]],
                paths: vec![],
            }));
            assert!(matches!(
                PreparedPads::build(
                    &[source],
                    &[],
                    PadLimits::default(),
                    &CancellationToken::default()
                ),
                Err(PrepareError::Invalid(ObjectId(42)))
            ));
        }
    }

    #[test]
    fn custom_mesh_preserves_overlapping_holes_and_placement_identity() {
        use pomelo_core::{copper::MeshLimits, model::CustomPadGeometry};
        use std::sync::Arc;
        let square = |x: f64, y: f64, side: f64| {
            vec![
                Point::new(x, y),
                Point::new(x + side, y),
                Point::new(x + side, y + side),
                Point::new(x, y + side),
            ]
        };
        let mut source = pin();
        source.pads[0].kind = PadKind::CUSTOM;
        source.pads[0].custom = Some(Arc::new(CustomPadGeometry {
            contours: vec![
                square(0.0, 0.0, 10.0),
                square(2.0, 2.0, 4.0),
                square(4.0, 4.0, 4.0),
            ],
            paths: vec![],
        }));
        let cancellation = CancellationToken::default();
        let prepared =
            PreparedPads::build(&[source], &[], PadLimits::default(), &cancellation).unwrap();
        let mesh = prepared
            .build_custom_meshes(
                crate::copper::CopperLimits::default(),
                &MeshLimits::default(),
                &cancellation,
            )
            .unwrap();
        assert_eq!(mesh.vertices.len(), 12);
        assert_eq!(mesh.batches[0].outer_indices().len(), 6);
        assert_eq!(mesh.batches[0].hole_indices().len(), 12);
        assert_eq!(mesh.batches[0].object, ObjectId(42));
        assert_eq!(
            mesh.batches[0].selected_object,
            pomelo_core::selection::SelectedObject::Pin(ObjectId(42))
        );
        assert_eq!(mesh.batches[0].layer, LayerId(7));
        assert_eq!(mesh.batches[0].net, NetId(5));
        assert!(matches!(
            prepared.build_custom_meshes(
                crate::copper::CopperLimits {
                    max_bytes: 0,
                    ..crate::copper::CopperLimits::default()
                },
                &MeshLimits::default(),
                &cancellation
            ),
            Err(PrepareError::Limit { .. })
        ));
    }
}
