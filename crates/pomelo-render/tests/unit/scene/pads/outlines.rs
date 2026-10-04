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
