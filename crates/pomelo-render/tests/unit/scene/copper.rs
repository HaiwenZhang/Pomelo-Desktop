use super::*;
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::Point,
};

fn square(x: f64, y: f64, side: f64) -> Vec<Point> {
    vec![
        Point::new(x, y),
        Point::new(x + side, y),
        Point::new(x + side, y + side),
        Point::new(x, y + side),
    ]
}
fn zone(id: u32, layer: u32) -> Zone {
    Zone {
        id: ObjectId(id),
        kind: ZoneKind::Unknown,
        layer: LayerId(layer),
        net: NetId(7),
        paths: vec![],
        mesh: CopperMesh::build(
            &[
                square(100_000.000_123, 0.0, 20.0),
                square(100_004.000_123, 4.0, 6.0),
                square(100_007.000_123, 7.0, 6.0),
            ],
            &[],
            &MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }
}

#[test]
fn source_shape_kinds_survive_batch_sorting() {
    let mut source = [zone(1, 2), zone(2, 0), zone(3, 1)];
    source[0].kind = ZoneKind::Unknown;
    source[1].kind = ZoneKind::Static;
    source[2].kind = ZoneKind::Dynamic;
    let prepared = PreparedCopper::build(
        &source,
        CopperLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .batches
            .iter()
            .map(|batch| (batch.object, batch.kind))
            .collect::<Vec<_>>(),
        [
            (ObjectId(2), ZoneKind::Static),
            (ObjectId(3), ZoneKind::Dynamic),
            (ObjectId(1), ZoneKind::Unknown)
        ]
    );
}

#[test]
fn overlapping_holes_remain_separate_coverage_and_indices_rebase_per_zone() {
    let zones = [zone(9, 2), zone(3, 1), zone(8, 2)];
    let prepared = PreparedCopper::build(
        &zones,
        CopperLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        prepared
            .batches
            .iter()
            .map(|b| b.object.0)
            .collect::<Vec<_>>(),
        [3, 9, 8]
    );
    for batch in &prepared.batches {
        let source = &zones[batch.source_index as usize];
        assert_eq!(batch.outer_indices().len(), 6);
        assert_eq!(batch.hole_indices().len(), 12);
        let start = batch.outer_indices().start as usize;
        let end = batch.hole_indices().end as usize;
        assert_eq!(
            &prepared.indices[start..end],
            source
                .mesh
                .indices
                .iter()
                .map(|index| index + batch.vertex_start)
                .collect::<Vec<_>>()
        );
        let position = prepared.vertices[batch.vertex_start as usize].position;
        assert!(
            (f64::from(position[0]) + f64::from(position[2]) - source.mesh.vertices[0].x).abs()
                < 1e-10
        );
    }
}

#[test]
fn invalid_triangle_roles_and_indices_are_rejected() {
    for mutation in 0..3 {
        let mut source = zone(42, 1);
        match mutation {
            0 => source.mesh.indices[0] = 4,
            1 => source.mesh.indices[6] = 0,
            _ => source.mesh.indices[6] = u32::MAX,
        }
        assert!(matches!(
            PreparedCopper::build(
                &[source],
                CopperLimits::default(),
                &CancellationToken::default()
            ),
            Err(PrepareError::Invalid(ObjectId(42)))
        ));
    }
}

#[test]
fn count_and_byte_limits_have_distinct_localized_units_before_mesh_processing() {
    let mut source = zone(42, 1);
    source.mesh.vertices[0].x = f64::NAN;
    for limits in [
        CopperLimits {
            max_vertices: 0,
            ..CopperLimits::default()
        },
        CopperLimits {
            max_indices: 0,
            ..CopperLimits::default()
        },
        CopperLimits {
            max_zones: 0,
            ..CopperLimits::default()
        },
    ] {
        let error = PreparedCopper::build(
            std::slice::from_ref(&source),
            limits,
            &CancellationToken::default(),
        )
        .unwrap_err();
        assert!(matches!(error, PrepareError::CountLimit { .. }));
        assert_eq!(
            error.diagnostic().code.as_ref(),
            "RENDER_PREPARE_COUNT_LIMIT"
        );
        for locale in pomelo_core::i18n::Locale::ALL {
            let message = error.diagnostic().message.display(locale);
            assert!(
                !message.contains(" B"),
                "count diagnostic claims bytes: {message}"
            );
            assert!(!message.contains("%{"));
        }
    }
    let error = PreparedCopper::build(
        &[source],
        CopperLimits {
            max_bytes: 0,
            ..CopperLimits::default()
        },
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(matches!(error, PrepareError::Limit { .. }));
    for locale in pomelo_core::i18n::Locale::ALL {
        assert!(error.diagnostic().message.display(locale).contains(" B"));
    }
}

#[test]
fn limits_and_cancellation_precede_invalid_mesh_processing() {
    let mut source = zone(42, 1);
    source.mesh.vertices[0].x = f64::NAN;
    assert!(matches!(
        PreparedCopper::build(
            std::slice::from_ref(&source),
            CopperLimits {
                max_bytes: 0,
                ..CopperLimits::default()
            },
            &CancellationToken::default()
        ),
        Err(PrepareError::Limit { .. })
    ));
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        PreparedCopper::build(&[source], CopperLimits::default(), &cancellation),
        Err(PrepareError::Cancelled)
    ));
}

#[test]
fn malformed_outer_count_and_non_finite_coordinates_are_rejected() {
    let mut source = zone(42, 1);
    source.mesh.outer_count = 5;
    assert!(
        PreparedCopper::build(
            std::slice::from_ref(&source),
            CopperLimits::default(),
            &CancellationToken::default()
        )
        .is_err()
    );
    source.mesh.outer_count = 6;
    source.mesh.vertices[0].x = f64::INFINITY;
    assert!(
        PreparedCopper::build(
            &[source],
            CopperLimits::default(),
            &CancellationToken::default()
        )
        .is_err()
    );
}
