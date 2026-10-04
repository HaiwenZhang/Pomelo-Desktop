use super::*;
use pomelo_core::model::{DrillShape, NetId, ObjectId, Pad};
pub(crate) fn backdrilled_via() -> Via {
    use pomelo_core::model::{Backdrill, BackdrillDefinition, BackdrillSpan};
    let mut pads = vec![Pad::circle(LayerId(0), 6.0), Pad::circle(LayerId(2), 6.0)];
    for layer in [1, 3] {
        let mut base = Pad::circle(LayerId(layer), 8.0);
        base.backdrill_base = true;
        let mut marker = base.clone();
        marker.backdrill_base = false;
        marker.backdrill = true;
        pads.extend([base, marker]);
    }
    Via {
        id: ObjectId(42),
        net: NetId(7),
        at: Point::new(10.0, 20.0),
        drill: 2.0,
        drill_shape: DrillShape {
            width: 2.0,
            height: 2.0,
            plated: true,
        },
        padstack: ObjectId(43),
        padstack_name: String::new(),
        start_layer: Some(LayerId(0)),
        end_layer: Some(LayerId(3)),
        pads: std::sync::Arc::from(pads),
        backdrill: Some(Backdrill {
            definition: BackdrillDefinition {
                spans: vec![
                    BackdrillSpan {
                        start_layer: LayerId(1),
                        stop_layer: LayerId(1),
                        protected_layer: LayerId(0),
                    },
                    BackdrillSpan {
                        start_layer: LayerId(3),
                        stop_layer: LayerId(3),
                        protected_layer: LayerId(2),
                    },
                ],
                display_diameter: 8.0,
                start_pad_diameter: 8.0,
                label_diameter: 8.0,
            },
            source_reference: ObjectId(44),
            rotation_degrees: 330.0,
            mirrored: false,
        }),
        stackup_region: None,
        angle: 0.37,
        mirrored: true,
        finger: None,
    }
}
#[test]
fn two_ended_backdrill_has_one_pattern_after_holes_and_only_cut_layer_scope() {
    let prepared = PreparedDrills::build(
        &[pin(1.0, 1.0)],
        &[backdrilled_via()],
        PadLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let geometry = prepared.geometry;
    assert_eq!(geometry.analytic.len(), 3);
    assert_eq!(geometry.analytic[2].ids, [42, u32::MAX, 7, 2]);
    assert_eq!(geometry.analytic[2].shape, [8.0, 8.0, 0.0, 2.0]);
    assert_eq!(geometry.analytic[2].source, [1, 0, 0, 2]);
    assert_eq!(
        geometry.backdrill_scopes.unwrap()[0],
        [LayerId(1), LayerId(3)]
    );
    assert_eq!(
        geometry.drill_scopes.unwrap()[0],
        [LayerId(0), LayerId(1), LayerId(2), LayerId(3)]
    );
}
#[test]
fn cut_markers_are_excluded_from_layer_pads_but_bases_are_retained() {
    let pads = PreparedPads::build(
        &[],
        &[backdrilled_via()],
        PadLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(pads.analytic.len(), 4);
    assert!(pads.analytic.iter().all(|pad| pad.source[3] & 2 == 0));
    assert_eq!(
        pads.analytic
            .iter()
            .filter(|pad| pad.source[3] & 4 != 0)
            .count(),
        2
    );
}
#[test]
fn backdrill_patterns_obey_instance_byte_and_cancellation_limits() {
    let via = backdrilled_via();
    let cancellation = CancellationToken::default();
    for limits in [
        PadLimits {
            max_pads: 1,
            ..PadLimits::default()
        },
        PadLimits {
            max_bytes: 255,
            ..PadLimits::default()
        },
    ] {
        assert!(matches!(
            PreparedDrills::build(&[], std::slice::from_ref(&via), limits, &cancellation),
            Err(PrepareError::Limit { .. })
        ));
    }
    cancellation.cancel();
    assert!(matches!(
        PreparedDrills::build(&[], &[via], PadLimits::default(), &cancellation),
        Err(PrepareError::Cancelled)
    ));
}
#[test]
fn invalid_backdrill_diameter_is_rejected_with_owner_identity() {
    for diameter in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut via = backdrilled_via();
        via.backdrill.as_mut().unwrap().definition.display_diameter = diameter;
        assert!(matches!(
            PreparedDrills::build(
                &[],
                &[via],
                PadLimits::default(),
                &CancellationToken::default()
            ),
            Err(PrepareError::Invalid(ObjectId(42)))
        ));
    }
}
fn pin(width: f64, height: f64) -> Pin {
    Pin {
        id: ObjectId(9),
        owner_id: ObjectId(8),
        net: NetId(7),
        name: String::new(),
        reference: String::new(),
        at: Point::new(10.0, 20.0),
        angle: std::f64::consts::FRAC_PI_2,
        mirrored: false,
        drill: width,
        drill_shape: DrillShape {
            width,
            height,
            plated: true,
        },
        pads: vec![Pad::circle(LayerId(1), 4.0), Pad::circle(LayerId(2), 4.0)],
        stackup_region: None,
        die: None,
    }
}
#[test]
fn slot_has_one_rotated_instance_regardless_of_copper_layer_count() {
    let prepared = PreparedDrills::build(
        &[pin(4.0, 2.0)],
        &[],
        PadLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(prepared.geometry.analytic.len(), 1);
    let instance = prepared.geometry.analytic[0];
    assert_eq!(instance.ids, [9, u32::MAX, 7, 11]);
    assert_eq!(instance.shape, [4.0, 2.0, 1.0, 0.0]);
    assert_eq!(instance.source, [0, 0, 0, 8]);
    assert!((instance.bounds_min[0] - 9.0).abs() < 1e-6);
    assert!((instance.bounds_min[1] - 18.0).abs() < 1e-6);
}
#[test]
fn absent_drill_is_skipped_and_invalid_geometry_is_reported() {
    let prepared = PreparedDrills::build(
        &[pin(0.0, 0.0)],
        &[],
        PadLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(prepared.geometry.analytic.is_empty());
    assert!(prepared.geometry.batches.is_empty());
    assert!(matches!(
        PreparedDrills::build(
            &[pin(f64::NAN, 2.0)],
            &[],
            PadLimits::default(),
            &CancellationToken::default()
        ),
        Err(PrepareError::Invalid(ObjectId(9)))
    ));
}
