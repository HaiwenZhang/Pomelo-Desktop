use pomelo_core::{
    geometry3d::solid::{SolidError, SolidLimits, SolidMesh, SurfaceKind},
    model::Point,
    task::CancellationToken,
};

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<Point> {
    vec![
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ]
}

fn build(rings: &[Vec<Point>]) -> SolidMesh {
    SolidMesh::extrude_coverage(
        rings,
        [-0.5, 0.5],
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn difference(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn volume(mesh: &SolidMesh) -> f64 {
    let origin = mesh.vertices.first().map_or([0.0; 3], |v| v.position);
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let a = difference(mesh.vertices[t[0] as usize].position, origin);
            let b = difference(mesh.vertices[t[1] as usize].position, origin);
            let c = difference(mesh.vertices[t[2] as usize].position, origin);
            let n = cross(b, c);
            (0..3).map(|i| a[i] * n[i]).sum::<f64>() / 6.0
        })
        .sum()
}
fn top_area(mesh: &SolidMesh) -> f64 {
    mesh.surfaces
        .iter()
        .filter(|s| s.kind == SurfaceKind::Top)
        .map(|s| {
            mesh.indices[s.indices.start as usize..s.indices.end as usize]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let a = mesh.vertices[t[0] as usize].position;
                    let b = mesh.vertices[t[1] as usize].position;
                    let c = mesh.vertices[t[2] as usize].position;
                    cross(difference(b, a), difference(c, a))[2] / 2.0
                })
                .sum::<f64>()
        })
        .sum()
}
fn check_normals(mesh: &SolidMesh) {
    for triangle in mesh.indices.as_chunks::<3>().0.iter() {
        let a = mesh.vertices[triangle[0] as usize];
        let b = mesh.vertices[triangle[1] as usize];
        let c = mesh.vertices[triangle[2] as usize];
        let n = cross(
            difference(b.position, a.position),
            difference(c.position, a.position),
        );
        assert!((0..3).map(|i| n[i] * f64::from(a.normal[i])).sum::<f64>() > 0.0);
        let length = a
            .normal
            .into_iter()
            .map(|v| f64::from(v).powi(2))
            .sum::<f64>();
        assert!((length - 1.0).abs() < 1e-6);
    }
}

fn check_closed(mesh: &SolidMesh) {
    // Distinct extruded islands can touch; validate each shell without welding them together.
    let mut starts: Vec<_> = mesh
        .surfaces
        .iter()
        .filter(|s| s.kind == SurfaceKind::Top)
        .map(|s| s.indices.start as usize)
        .collect();
    if mesh.indices.is_empty() {
        assert!(starts.is_empty());
        return;
    }
    assert_eq!(starts.first(), Some(&0));
    starts.push(mesh.indices.len());
    for range in starts.windows(2) {
        check_closed_shell(mesh, &mesh.indices[range[0]..range[1]]);
    }
}

fn check_closed_shell(mesh: &SolidMesh, indices: &[u32]) {
    let mut edges = std::collections::BTreeMap::<_, (u32, i32)>::new();
    for triangle in indices.as_chunks::<3>().0.iter() {
        let p: Vec<_> = triangle
            .iter()
            .map(|&i| {
                mesh.vertices[i as usize]
                    .position
                    .map(|v| if v == 0.0 { 0 } else { v.to_bits() })
            })
            .collect();
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            assert_ne!(a, b);
            let (key, direction) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
            let entry = edges.entry(key).or_default();
            entry.0 += 1;
            entry.1 += direction;
        }
    }
    assert!(
        edges.values().all(|edge| *edge == (2, 0)),
        "unmatched or inconsistently oriented mesh edges"
    );
}

#[test]
fn extrudes_holes_with_correct_volume_normals_and_surface_roles() {
    let mesh = build(&[
        rectangle(0.0, 0.0, 10.0, 10.0),
        rectangle(2.0, 2.0, 4.0, 4.0),
    ]);
    assert!((top_area(&mesh) - 84.0).abs() < 1e-9);
    assert!((volume(&mesh) - 84.0).abs() < 1e-9);
    check_normals(&mesh);
    check_closed(&mesh);
    let hole = mesh
        .surfaces
        .iter()
        .find(|s| s.kind == SurfaceKind::HoleWall)
        .unwrap();
    for &index in &mesh.indices[hole.indices.start as usize..hole.indices.end as usize] {
        let v = mesh.vertices[index as usize];
        assert!(
            f64::from(v.normal[0]) * (4.0 - v.position[0])
                + f64::from(v.normal[1]) * (4.0 - v.position[1])
                > 0.0
        );
    }
    let bounds = mesh.bounds.unwrap();
    assert_eq!(bounds.min, [0.0, 0.0, -0.5]);
    assert_eq!(bounds.max, [10.0, 10.0, 0.5]);
}

#[test]
fn overlapping_and_nested_holes_subtract_their_union_for_either_winding() {
    let rings = [
        rectangle(0.0, 0.0, 10.0, 10.0),
        rectangle(2.0, 2.0, 4.0, 4.0),
        rectangle(4.0, 4.0, 4.0, 4.0),
        rectangle(3.0, 3.0, 1.0, 1.0),
    ];
    for reverse_mask in 0..16 {
        let rings: Vec<_> = rings
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut r = r.clone();
                if reverse_mask & (1 << i) != 0 {
                    r.reverse();
                }
                r
            })
            .collect();
        let mesh = build(&rings);
        assert!((top_area(&mesh) - 72.0).abs() < 1e-9);
        assert!((volume(&mesh) - 72.0).abs() < 1e-9);
        check_normals(&mesh);
        check_closed(&mesh);
    }
}

#[test]
fn a_cut_can_touch_the_exterior_split_it_into_islands_or_remove_all_material() {
    let outer = rectangle(0.0, 0.0, 10.0, 10.0);
    for (cut, area, islands) in [
        (rectangle(0.0, 2.0, 4.0, 4.0), 84.0, 1),
        (rectangle(4.0, -1.0, 2.0, 12.0), 80.0, 2),
        (rectangle(-1.0, -1.0, 12.0, 12.0), 0.0, 0),
    ] {
        let mesh = build(&[outer.clone(), cut]);
        assert!((volume(&mesh) - area).abs() < 1e-9);
        assert!((top_area(&mesh) - area).abs() < 1e-9);
        assert_eq!(
            mesh.surfaces
                .iter()
                .filter(|s| s.kind == SurfaceKind::Top)
                .count(),
            islands
        );
        assert_eq!(mesh.bounds.is_none(), islands == 0);
        check_normals(&mesh);
        check_closed(&mesh);
    }
}

#[test]
fn even_odd_exterior_is_normalized_before_extrusion() {
    let mesh = build(&[vec![
        Point::new(0.0, 0.0),
        Point::new(4.0, 4.0),
        Point::new(0.0, 4.0),
        Point::new(4.0, 0.0),
    ]]);
    assert!((top_area(&mesh) - 8.0).abs() < 1e-9);
    assert!((volume(&mesh) - 8.0).abs() < 1e-9);
    assert_eq!(
        mesh.surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Top)
            .count(),
        2
    );
    check_normals(&mesh);
    check_closed(&mesh);
}

#[test]
fn collinear_boundary_points_do_not_create_cap_wall_t_junctions() {
    let mesh = build(&[vec![
        Point::new(0.0, 0.0),
        Point::new(5.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
        Point::new(0.0, 10.0),
    ]]);
    assert!((volume(&mesh) - 100.0).abs() < 1e-9);
    check_closed(&mesh);
}

#[test]
fn large_board_origin_does_not_consume_the_clipping_precision_budget() {
    let mesh = build(&[
        rectangle(1e9, -1e9, 10.0, 10.0),
        rectangle(1e9 + 2.0, -1e9 + 2.0, 4.0, 4.0),
    ]);
    assert!((top_area(&mesh) - 84.0).abs() < 1e-8);
    assert!((volume(&mesh) - 84.0).abs() < 1e-8);
}

#[test]
fn invalid_inputs_cancellation_and_output_budgets_are_explicit_errors() {
    let outer = vec![rectangle(0.0, 0.0, 10.0, 10.0)];
    let cancel = CancellationToken::default();
    for z in [[0.0, 0.0], [1.0, 0.0], [0.0, f64::NAN]] {
        assert!(matches!(
            SolidMesh::extrude_coverage(&outer, z, SolidLimits::default(), &cancel),
            Err(SolidError::Invalid("Z_INTERVAL"))
        ));
    }
    let bad = vec![vec![
        Point::default(),
        Point::new(f64::NAN, 1.0),
        Point::new(1.0, 1.0),
    ]];
    assert!(matches!(
        SolidMesh::extrude_coverage(&bad, [0.0, 1.0], SolidLimits::default(), &cancel),
        Err(SolidError::Invalid(_))
    ));
    for limits in [
        SolidLimits {
            max_vertices: 0,
            ..Default::default()
        },
        SolidLimits {
            max_indices: 0,
            ..Default::default()
        },
        SolidLimits {
            max_allocation_bytes: 1,
            ..Default::default()
        },
        SolidLimits {
            max_rings: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            SolidMesh::extrude_coverage(&outer, [0.0, 1.0], limits, &cancel),
            Err(SolidError::Limit { .. })
        ));
    }
    cancel.cancel();
    assert!(matches!(
        SolidMesh::extrude_coverage(&outer, [0.0, 1.0], SolidLimits::default(), &cancel),
        Err(SolidError::Cancelled)
    ));
}

#[test]
fn parity_islands_and_drill_subtraction_have_different_fill_rules() {
    let rings = [
        rectangle(0.0, 0.0, 10.0, 10.0),
        rectangle(2.0, 2.0, 6.0, 6.0),
        rectangle(4.0, 4.0, 2.0, 2.0),
    ];
    let cancel = CancellationToken::default();
    let mesh =
        SolidMesh::extrude_even_odd(&rings, [-0.5, 0.5], SolidLimits::default(), &cancel).unwrap();
    assert!((top_area(&mesh) - 68.0).abs() < 1e-9);
    check_closed(&mesh);
    let mesh = SolidMesh::extrude_even_odd_minus(
        &rings,
        &[rectangle(3.0, 3.0, 4.0, 4.0)],
        [-0.5, 0.5],
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!((top_area(&mesh) - 64.0).abs() < 1e-9);
    assert!((volume(&mesh) - 64.0).abs() < 1e-9);
    check_closed(&mesh);
}

#[test]
fn strokes_have_round_caps_and_closed_strokes_keep_their_opening() {
    let cancel = CancellationToken::default();
    let open = SolidMesh::extrude_stroke(
        &[Point::default(), Point::new(10.0, 0.0)],
        2.0,
        false,
        [-0.5, 0.5],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!((volume(&open) - (20.0 + std::f64::consts::PI)).abs() < 0.01);
    check_closed(&open);
    check_normals(&open);
    let closed = SolidMesh::extrude_stroke(
        &rectangle(0.0, 0.0, 10.0, 10.0),
        2.0,
        true,
        [-0.5, 0.5],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!((volume(&closed) - (76.0 + std::f64::consts::PI)).abs() < 0.02);
    assert!(
        closed
            .surfaces
            .iter()
            .any(|s| s.kind == SurfaceKind::HoleWall)
    );
    check_closed(&closed);
    check_normals(&closed);
}

#[test]
fn concave_cap_with_many_holes_has_no_filled_hole_triangles() {
    // An L-shaped exterior exercises both convex-hull rejection and hole parity.
    let mut rings = vec![vec![
        Point::new(0., 0.),
        Point::new(30., 0.),
        Point::new(30., 10.),
        Point::new(10., 10.),
        Point::new(10., 30.),
        Point::new(0., 30.),
    ]];
    for y in 0..9 {
        for x in 0..9 {
            rings.push(rectangle(x as f64 + 0.1, y as f64 + 0.1, 0.5, 0.5));
        }
    }
    let mesh = build(&rings);
    assert!((top_area(&mesh) - (500.0 - 81.0 * 0.25)).abs() < 1e-8);
    assert!((volume(&mesh) - (500.0 - 81.0 * 0.25)).abs() < 1e-8);
    check_closed(&mesh);
    check_normals(&mesh);
    for surface in mesh.surfaces.iter().filter(|s| s.kind == SurfaceKind::Top) {
        for triangle in mesh.indices[surface.indices.start as usize..surface.indices.end as usize]
            .as_chunks::<3>()
            .0
            .iter()
        {
            let center = [0, 1].map(|axis| {
                triangle
                    .iter()
                    .map(|&i| mesh.vertices[i as usize].position[axis])
                    .sum::<f64>()
                    / 3.0
            });
            assert!(center[0] <= 10. || center[1] <= 10.);
            for hole in &rings[1..] {
                assert!(
                    !(center[0] > hole[0].x
                        && center[0] < hole[2].x
                        && center[1] > hole[0].y
                        && center[1] < hole[2].y)
                );
            }
        }
    }
}
