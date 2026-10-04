use pomelo_core::{
    copper::{CopperMesh, MeshError, MeshLimits},
    model::{Arc, LayerId, NetId, ObjectId, Point, Segment},
    task::CancellationToken,
};

fn square(x: f64, y: f64, size: f64) -> Vec<Point> {
    vec![
        Point::new(x, y),
        Point::new(x + size, y),
        Point::new(x + size, y + size),
        Point::new(x, y + size),
    ]
}
fn build(rings: &[Vec<Point>]) -> CopperMesh {
    CopperMesh::build(
        rings,
        &[],
        &MeshLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap()
}

#[test]
fn fill_query_excludes_hole_union_and_hole_boundaries() {
    let mesh = build(&[
        square(0.0, 0.0, 10.0),
        square(2.0, 2.0, 4.0),
        square(4.0, 4.0, 4.0),
    ]);
    let cancel = CancellationToken::default();
    for point in [
        Point::new(1.0, 1.0),
        Point::new(0.0, 5.0),
        Point::new(9.0, 9.0),
    ] {
        assert!(mesh.covers_fill(point, &cancel).unwrap());
    }
    for point in [
        Point::new(3.0, 3.0),
        Point::new(5.0, 5.0),
        Point::new(7.0, 7.0),
        Point::new(2.0, 3.0),
        Point::new(-1.0, 0.0),
    ] {
        assert!(!mesh.covers_fill(point, &cancel).unwrap());
    }
    let mut reversed = [square(0.0, 0.0, 10.0), square(2.0, 2.0, 4.0)];
    for ring in &mut reversed {
        ring.reverse();
    }
    assert!(
        build(&reversed)
            .covers_fill(Point::new(1.0, 1.0), &cancel)
            .unwrap()
    );
    assert!(
        !build(&reversed)
            .covers_fill(Point::new(3.0, 3.0), &cancel)
            .unwrap()
    );
    cancel.cancel();
    assert!(matches!(
        mesh.covers_fill(Point::default(), &cancel),
        Err(MeshError::Cancelled)
    ));
}

#[test]
fn fill_query_rejects_malformed_ranges_without_panicking() {
    let mut mesh = build(&[square(0.0, 0.0, 10.0)]);
    mesh.ring_offsets = vec![0, 100, 4];
    assert!(matches!(
        mesh.covers_fill(Point::default(), &CancellationToken::default()),
        Err(MeshError::Invalid {
            field: "RING_OFFSETS",
            ..
        })
    ));
}
fn coverage(mesh: &CopperMesh, point: Point, indices: &[u32]) -> bool {
    indices.as_chunks::<3>().0.iter().any(|triangle| {
        let p: Vec<_> = triangle
            .iter()
            .map(|index| mesh.vertices[*index as usize])
            .collect();
        let side =
            |a: Point, b: Point| (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
        let values = [side(p[0], p[1]), side(p[1], p[2]), side(p[2], p[0])];
        values.iter().all(|v| *v >= 0.0) || values.iter().all(|v| *v <= 0.0)
    })
}
fn area(mesh: &CopperMesh) -> f64 {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
        })
        .sum()
}

#[test]
fn overlapping_holes_are_a_union_and_source_vertices_are_preserved() {
    let rings = vec![
        square(0.0, 0.0, 10.0),
        square(2.0, 2.0, 4.0),
        square(4.0, 4.0, 4.0),
    ];
    let mesh = build(&rings);
    assert_eq!(
        mesh.vertices,
        rings.iter().flatten().copied().collect::<Vec<_>>()
    );
    assert_eq!(mesh.ring_offsets, [0, 4, 8, 12]);
    let filled = |point| {
        coverage(&mesh, point, &mesh.indices[..mesh.outer_count as usize])
            && !coverage(&mesh, point, &mesh.indices[mesh.outer_count as usize..])
    };
    assert!(filled(Point::new(1.0, 1.0)));
    assert!(
        !filled(Point::new(5.0, 5.0)),
        "overlapping holes must never fill again"
    );
    assert!(!filled(Point::new(7.0, 7.0)));
    assert!(!filled(Point::new(20.0, 20.0)));
}

#[test]
fn convex_contours_preserve_frozen_triangle_order_for_both_windings() {
    let ring = square(0.0, 0.0, 10.0);
    assert_eq!(
        build(std::slice::from_ref(&ring)).indices,
        [2, 3, 0, 2, 0, 1]
    );
    let reversed: Vec<_> = ring.into_iter().rev().collect();
    assert_eq!(build(&[reversed]).indices, [1, 0, 3, 1, 3, 2]);
}

#[test]
fn concave_contour_and_reversed_winding_preserve_fill_area() {
    let ring = vec![
        Point::new(0.0, 0.0),
        Point::new(4.0, 0.0),
        Point::new(4.0, 1.0),
        Point::new(1.0, 1.0),
        Point::new(1.0, 4.0),
        Point::new(0.0, 4.0),
    ];
    let mut reversed = ring.clone();
    reversed.reverse();
    for ring in [ring, reversed] {
        let mesh = build(&[ring]);
        assert_eq!(area(&mesh), 7.0);
        assert!(!coverage(&mesh, Point::new(2.0, 2.0), &mesh.indices));
    }
}

#[test]
fn collinear_and_duplicate_vertices_use_earcut_without_changing_coordinates() {
    let ring = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(2.0, 2.0),
        Point::new(0.0, 2.0),
        Point::new(0.0, 0.0),
    ];
    let mesh = build(std::slice::from_ref(&ring));
    assert_eq!(mesh.vertices, ring);
    assert_eq!(area(&mesh), 4.0);
}

#[test]
fn morton_hole_order_has_stable_ties_and_chunks_of_at_most_sixty_four() {
    let mut rings = vec![square(0.0, 0.0, 100.0)];
    rings.extend((0..130).map(|_| square(20.0, 30.0, 1.0)));
    let mesh = build(&rings);
    assert_eq!(mesh.ring_order, (0..131).collect::<Vec<u32>>());
    assert_eq!(
        mesh.hole_chunks
            .iter()
            .map(|c| (c.ring_start, c.ring_count, c.start, c.count))
            .collect::<Vec<_>>(),
        [(1, 64, 6, 384), (65, 64, 390, 384), (129, 2, 774, 12)]
    );
}

#[test]
fn spatial_hole_order_changes_only_draw_order() {
    let rings = vec![
        square(0.0, 0.0, 100.0),
        square(90.0, 90.0, 1.0),
        square(1.0, 1.0, 1.0),
    ];
    let mesh = build(&rings);
    assert_eq!(mesh.ring_order, [0, 2, 1]);
    assert_eq!(mesh.ring_offsets, [0, 4, 8, 12]);
}

#[test]
fn analytic_circle_expands_bounds_beyond_the_sampled_ring() {
    let path = vec![Segment {
        id: ObjectId(1),
        track_id: ObjectId(1),
        layer: LayerId(0),
        net: NetId(0),
        a: Point::new(5.0, 0.0),
        b: Point::new(5.0, 0.0),
        width: 100.0,
        bond_wire: None,
        arc: Some(Arc {
            center: Point::default(),
            radius: 5.0,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        }),
    }];
    let mesh = CopperMesh::build(
        &[square(-1.0, -1.0, 2.0)],
        &[path],
        &MeshLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(mesh.curved);
    assert_eq!(mesh.ring_bounds[0].min, Point::new(-5.0, -5.0));
    assert_eq!(mesh.ring_bounds[0].max, Point::new(5.0, 5.0));
}

#[test]
fn invalid_coordinates_and_path_count_fail_instead_of_producing_partial_mesh() {
    let token = CancellationToken::default();
    let limits = MeshLimits::default();
    assert!(matches!(
        CopperMesh::build(&[vec![Point::new(f64::NAN, 0.0)]], &[], &limits, &token),
        Err(MeshError::Invalid {
            field: "COORDINATES",
            ..
        })
    ));
    assert!(matches!(
        CopperMesh::build(&[square(0.0, 0.0, 1.0)], &[vec![], vec![]], &limits, &token),
        Err(MeshError::Invalid {
            field: "PATH_RING_COUNT",
            ..
        })
    ));
}

#[test]
fn limits_and_pre_cancel_are_checked_before_triangulation() {
    let token = CancellationToken::default();
    let rings = [square(0.0, 0.0, 1.0)];
    for limits in [
        MeshLimits {
            max_points: 3,
            ..MeshLimits::default()
        },
        MeshLimits {
            max_rings: 0,
            ..MeshLimits::default()
        },
        MeshLimits {
            max_allocation_bytes: 1,
            ..MeshLimits::default()
        },
    ] {
        assert!(matches!(
            CopperMesh::build(&rings, &[], &limits, &token),
            Err(MeshError::Limit { .. })
        ));
    }
    token.cancel();
    assert!(matches!(
        CopperMesh::build(&rings, &[], &MeshLimits::default(), &token),
        Err(MeshError::Cancelled)
    ));
}
