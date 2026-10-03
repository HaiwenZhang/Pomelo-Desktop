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
mod tests {
    use super::*;
    use crate::pads::CustomPadInstance;
    use pomelo_core::{
        model::{CustomPadGeometry, LayerId, NetId, Pad, PadKind, Point},
        pad::PadPlacement,
    };
    use std::sync::Arc as Shared;

    fn source(paths: Vec<Vec<Segment>>, category: u32) -> PreparedPads {
        let ring = vec![
            Point::new(-2.0, -2.0),
            Point::new(2.0, -2.0),
            Point::new(2.0, 2.0),
            Point::new(-2.0, 2.0),
        ];
        let mut pad = Pad::circle(LayerId(7), 4.0);
        pad.offset = Point::new(3.0, 4.0);
        pad.kind = PadKind::CUSTOM;
        pad.custom = Some(Shared::new(CustomPadGeometry {
            contours: vec![
                ring,
                vec![
                    Point::new(-0.5, -0.5),
                    Point::new(0.5, -0.5),
                    Point::new(0.5, 0.5),
                    Point::new(-0.5, 0.5),
                ],
            ],
            paths,
        }));
        let placement = PadPlacement {
            at: Point::new(100_000.000_123, 20.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: true,
        };
        PreparedPads {
            analytic: vec![],
            custom: vec![CustomPadInstance {
                object: ObjectId(42),
                net: NetId(5),
                bounds: pad.bounds(placement).unwrap(),
                placement,
                pad,
                source: [category, 0, 0, 0],
            }],
            batches: vec![],
            custom_mesh: None,
            custom_outlines: None,
            drill_scopes: None,
            backdrill_scopes: None,
        }
    }

    #[test]
    fn custom_edges_close_both_exterior_and_hole_rings_with_owner_identity() {
        let cancel = CancellationToken::default();
        for category in [0, 1] {
            let pads = source(vec![], category);
            let edges = pads
                .build_custom_outlines(TraceLimits::default(), &cancel)
                .unwrap();
            assert_eq!(edges.instances.len(), 8);
            let expected = pads.custom[0]
                .pad
                .to_world(Point::new(-2.0, -2.0), pads.custom[0].placement);
            let first = edges
                .instances
                .iter()
                .find(|edge| edge.flags[1] == 0)
                .unwrap();
            assert!((f64::from(first.a[0]) + f64::from(first.a[2]) - expected.x).abs() < 1e-9);
            assert!((f64::from(first.a[1]) + f64::from(first.a[3]) - expected.y).abs() < 1e-9);
            for edge in &edges.instances {
                assert_eq!(edge.ids, [42, 0, 7, 5]);
                assert_eq!(edge.flags[3], if category == 0 { 16 } else { 32 });
                assert_eq!(f32::from_bits(edge.flags[2]), 0.0);
            }
            assert!(!edges.batches[0].outline);
        }
    }

    #[test]
    fn custom_edges_keep_analytic_arcs_and_reverse_mirrored_sweep() {
        let edge = Segment {
            id: ObjectId(0),
            track_id: ObjectId(0),
            layer: LayerId(0),
            net: NetId(0),
            a: Point::new(2.0, 0.0),
            b: Point::new(0.0, 2.0),
            width: 9.0,
            arc: Some(Arc {
                center: Point::default(),
                radius: 2.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            }),
            bond_wire: None,
        };
        let pads = source(vec![vec![edge]], 0);
        let edges = pads
            .build_custom_outlines(TraceLimits::default(), &CancellationToken::default())
            .unwrap();
        assert_eq!(
            edges.instances.len(),
            1,
            "analytic paths replace the fill approximation"
        );
        let edge = &edges.instances[0];
        assert_eq!(edge.flags[0], 1);
        assert_eq!(edge.arc[0], 2.0);
        assert!((edge.arc[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((edge.arc[3] + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(edge.center[1], 24.0);
        assert_eq!(f32::from_bits(edge.flags[2]), 0.0);
    }

    #[test]
    fn custom_edges_respect_memory_limits_and_cancellation() {
        let pads = source(vec![], 0);
        assert!(matches!(
            pads.build_custom_outlines(
                TraceLimits {
                    max_bytes: 0,
                    ..TraceLimits::default()
                },
                &CancellationToken::default()
            ),
            Err(PrepareError::Limit { .. })
        ));
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(matches!(
            pads.build_custom_outlines(TraceLimits::default(), &cancel),
            Err(PrepareError::Cancelled)
        ));
    }
}
