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
