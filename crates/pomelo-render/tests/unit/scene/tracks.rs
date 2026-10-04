use super::*;
use pomelo_core::model::{Arc, NetId};

#[test]
fn zone_outline_kind_survives_layer_reordering_and_analytic_arc_preparation() {
    use pomelo_core::model::{Zone, ZoneKind};
    let source = [ZoneKind::Unknown, ZoneKind::Static, ZoneKind::Dynamic]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let mut arc = line(index as u32, index as u32);
            arc.a = Point::new(1.0, 0.0);
            arc.b = Point::new(0.0, 1.0);
            arc.arc = Some(Arc {
                center: Point::default(),
                radius: 1.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            });
            Zone {
                id: ObjectId(10 + index as u32),
                layer: LayerId(2 - index as u32),
                net: NetId(1),
                kind,
                paths: vec![vec![line(index as u32, 0), arc]],
                mesh: Default::default(),
            }
        })
        .collect::<Vec<_>>();
    let prepared = PreparedTracks::build_zone_outlines(
        &source,
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(prepared.instances.len(), 6);
    for instance in prepared.instances {
        assert_ne!(instance.flags[3] & 8, 0);
        assert_eq!(instance.flags[3] & 128 != 0, instance.ids[0] == 12);
    }
}

#[test]
fn empty_dynamic_zone_outline_scratch_is_bounded_and_cancellation_wins() {
    let source = [pomelo_core::model::Zone {
        id: ObjectId(1),
        layer: LayerId(0),
        net: NetId(1),
        kind: pomelo_core::model::ZoneKind::Dynamic,
        paths: vec![],
        mesh: Default::default(),
    }];
    let limits = TraceLimits {
        max_instances: 0,
        max_bytes: size_of::<ObjectId>() - 1,
    };
    assert!(matches!(
        PreparedTracks::build_zone_outlines(&source, limits, &CancellationToken::default()),
        Err(PrepareError::Limit {
            actual: 4,
            limit: 3
        })
    ));
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        PreparedTracks::build_zone_outlines(&source, limits, &token),
        Err(PrepareError::Cancelled)
    ));
}

fn line(id: u32, layer: u32) -> Segment {
    Segment {
        id: ObjectId(id),
        track_id: ObjectId(100),
        layer: LayerId(layer),
        net: NetId(7),
        a: Point::new(100_000.000_123, 2.0),
        b: Point::new(100_001.0, 3.0),
        width: 0.1,
        arc: None,
        bond_wire: None,
    }
}

fn drawing(id: u32, segments: Vec<Segment>) -> pomelo_core::model::BoardDrawing {
    pomelo_core::model::BoardDrawing {
        id: ObjectId(id),
        owner_id: None,
        layer: LayerId::DIMENSION,
        net: NetId(0),
        graphic_ids: Vec::new(),
        segments,
        text_ids: Vec::new(),
    }
}

#[test]
fn drawing_groups_preserve_source_strokes_layers_and_do_not_become_board_outlines() {
    let groups = [
        drawing(100, vec![line(9, 2), line(3, 1)]),
        drawing(101, vec![line(8, 2)]),
    ];
    let prepared = PreparedTracks::build_drawings(
        &groups,
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .instances
            .iter()
            .map(|instance| instance.ids[0])
            .collect::<Vec<_>>(),
        [100, 100, 101]
    );
    assert_eq!(
        prepared
            .instances
            .iter()
            .map(|instance| instance.flags[1])
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    assert!(prepared.batches.iter().all(|batch| !batch.outline));
    assert_eq!(prepared.batches.len(), 2);
    assert_eq!(groups[0].segments[0].id, ObjectId(9));
    assert_eq!(groups[1].id, ObjectId(101));
}

#[test]
fn drawing_preparation_checks_aggregate_budget_and_reports_invalid_stroke_identity() {
    let mut groups = [
        drawing(100, vec![line(9, 1)]),
        drawing(101, vec![line(8, 1)]),
    ];
    let limits = TraceLimits {
        max_instances: 1,
        ..TraceLimits::default()
    };
    assert!(matches!(
        PreparedTracks::build_drawings(&groups, limits, &CancellationToken::default()),
        Err(PrepareError::Limit {
            actual: 2,
            limit: 1
        })
    ));
    groups[1].segments[0].width = f64::NAN;
    assert!(matches!(
        PreparedTracks::build_drawings(
            &groups,
            TraceLimits::default(),
            &CancellationToken::default()
        ),
        Err(PrepareError::Invalid(ObjectId(8)))
    ));
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        PreparedTracks::build_drawings(&groups, limits, &cancellation),
        Err(PrepareError::Cancelled)
    ));
}

#[test]
fn batches_group_layers_without_reordering_source_segments_within_layer() {
    let prepared = PreparedTracks::build(
        &[line(9, 2), line(3, 1), line(8, 2)],
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .instances
            .iter()
            .map(|i| i.ids[0])
            .collect::<Vec<_>>(),
        [3, 9, 8]
    );
    assert_eq!(
        prepared.batches,
        [
            TraceBatch {
                layer: LayerId(1),
                start: 0,
                count: 1,
                outline: false
            },
            TraceBatch {
                layer: LayerId(2),
                start: 1,
                count: 2,
                outline: false
            }
        ]
    );
}

#[test]
fn many_strokes_on_one_layer_do_not_allocate_one_batch_slot_per_stroke() {
    let segments: Vec<_> = (0..10_000).map(|id| line(id, 1)).collect();
    let prepared = PreparedTracks::build(
        &segments,
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(prepared.instances.len(), 10_000);
    assert_eq!(prepared.batches.len(), 1);
    assert_eq!(prepared.batches[0].count, 10_000);
    assert!(prepared.batches.capacity() < 10_000);
}

#[test]
fn outline_keeps_source_identity_and_separate_batches_after_traces() {
    let prepared = PreparedTracks::build_with_outline(
        &[line(10, 3), line(20, 1)],
        &[line(30, 1), line(40, 2)],
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .instances
            .iter()
            .map(|instance| instance.ids[0])
            .collect::<Vec<_>>(),
        [20, 10, 30, 40]
    );
    assert_eq!(
        prepared
            .batches
            .iter()
            .map(|batch| batch.outline)
            .collect::<Vec<_>>(),
        [false, false, true, true]
    );
    assert_eq!(prepared.instances[2].flags[1], 2);
    assert_eq!(prepared.instances[2].flags[3] & 4, 4);
    assert_eq!(prepared.instances[2].ids[2], 1);
}

#[test]
fn combined_trace_and_outline_count_obeys_one_budget() {
    assert!(matches!(
        PreparedTracks::build_with_outline(
            &[line(1, 1)],
            &[line(2, 1)],
            TraceLimits {
                max_instances: 1,
                ..TraceLimits::default()
            },
            &CancellationToken::default()
        ),
        Err(PrepareError::Limit {
            actual: 2,
            limit: 1
        })
    ));
}

#[test]
fn uploaded_points_preserve_small_features_at_large_coordinates() {
    let segment = line(1, 1);
    let prepared = PreparedTracks::build(
        std::slice::from_ref(&segment),
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let a = prepared.instances[0].a;
    assert!((f64::from(a[0]) + f64::from(a[2]) - segment.a.x).abs() < 1e-10);
}

#[test]
fn negative_arc_sweep_and_cardinal_bounds_survive_preparation() {
    let mut segment = line(1, 1);
    segment.a = Point::new(10.0, 0.0);
    segment.b = Point::new(-10.0, 0.0);
    segment.arc = Some(Arc {
        center: Point::new(0.0, 0.0),
        radius: 10.0,
        start: 0.0,
        sweep: -std::f64::consts::PI,
    });
    let prepared = PreparedTracks::build(
        &[segment],
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let instance = prepared.instances[0];
    assert_eq!(instance.flags[0], 1);
    assert!(instance.arc[3] < 0.0);
    assert!(
        (f64::from(instance.bounds_min[1]) + f64::from(instance.bounds_min[3]) + 10.05).abs()
            < 1e-10
    );
}

#[test]
fn byte_budget_is_checked_before_allocating_instances() {
    assert!(matches!(
        PreparedTracks::build(
            &[line(1, 1)],
            TraceLimits {
                max_bytes: 127,
                ..TraceLimits::default()
            },
            &CancellationToken::default()
        ),
        Err(PrepareError::Limit { .. })
    ));
}

#[test]
fn non_finite_input_is_not_uploaded() {
    let mut segment = line(1, 1);
    segment.a.x = f64::NAN;
    assert!(matches!(
        PreparedTracks::build(
            &[segment],
            TraceLimits::default(),
            &CancellationToken::default()
        ),
        Err(PrepareError::Invalid(ObjectId(1)))
    ));
}

#[test]
fn full_circle_flag_is_not_inferred_from_rounded_shader_angles() {
    let mut full = line(1, 1);
    full.a = Point::new(10.0, 0.0);
    full.b = full.a;
    full.arc = Some(Arc {
        center: Point::new(0.0, 0.0),
        radius: 10.0,
        start: 0.0,
        sweep: std::f64::consts::TAU,
    });
    let mut partial = full.clone();
    partial.id = ObjectId(2);
    partial.arc.as_mut().unwrap().sweep -= 1e-7;
    let prepared = PreparedTracks::build(
        &[full, partial],
        TraceLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .instances
            .iter()
            .map(|instance| instance.flags[3] & 1)
            .collect::<Vec<_>>(),
        [1, 0]
    );
}

#[test]
fn cancellation_wins_before_resource_checks() {
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        PreparedTracks::build(
            &[line(1, 1)],
            TraceLimits {
                max_bytes: 0,
                max_instances: 0
            },
            &token
        ),
        Err(PrepareError::Cancelled)
    ));
}

#[test]
fn preparation_errors_render_in_all_supported_languages() {
    for error in [
        PrepareError::Cancelled,
        PrepareError::Invalid(ObjectId(1)),
        PrepareError::Allocation,
        PrepareError::Limit {
            actual: 9,
            limit: 8,
        },
    ] {
        for locale in pomelo_core::i18n::Locale::ALL {
            assert!(error.diagnostic().message.render(locale).is_ok());
        }
    }
}
