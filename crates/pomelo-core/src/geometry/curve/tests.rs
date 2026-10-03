use super::*;
use crate::{
    geometry::path_contains,
    model::{LayerId, NetId, ObjectId},
};

fn circle(radius: f64, sweep: f64) -> Segment {
    Segment {
        id: ObjectId(7),
        track_id: ObjectId(0),
        layer: LayerId(1),
        net: NetId(1),
        a: Point::new(radius, 0.0),
        b: Point::new(radius * sweep.cos(), radius * sweep.sin()),
        width: 0.0,
        arc: Some(Arc {
            center: Point::default(),
            radius,
            start: 0.0,
            sweep,
        }),
        bond_wire: None,
    }
}
fn view(radius: f64) -> Bounds {
    Bounds {
        min: Point::new(-radius, -radius),
        max: Point::new(radius, radius),
    }
}

#[test]
fn both_arc_directions_bound_visible_chords_to_quarter_physical_pixel() {
    let tolerance = 0.25 / 16384.0;
    for sweep in [std::f64::consts::TAU, -std::f64::consts::TAU] {
        let points = tessellate_curve_ring(
            &[circle(10.0, sweep)],
            view(11.0),
            tolerance,
            CurveLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        let maximum = points
            .windows(2)
            .map(|pair| {
                let middle =
                    Point::new((pair[0].x + pair[1].x) / 2.0, (pair[0].y + pair[1].y) / 2.0);
                10.0 - middle.x.hypot(middle.y)
            })
            .fold(0.0, f64::max);
        assert!(
            maximum <= tolerance * 1.000001,
            "chord error {maximum} exceeds {tolerance}"
        );
    }
}

#[test]
fn microscope_view_refines_only_the_visible_part_of_a_large_circle() {
    let view = Bounds {
        min: Point::new(999999.995, -0.01),
        max: Point::new(1000000.005, 0.01),
    };
    let points = tessellate_curve_ring(
        &[circle(1_000_000.0, std::f64::consts::TAU)],
        view,
        1e-8,
        CurveLimits {
            max_points: 2048,
            ..CurveLimits::default()
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(
        !points.is_empty()
            && points.len() < 2048
            && points.iter().all(|p| p.x >= view.min.x
                && p.x <= view.max.x
                && p.y >= view.min.y
                && p.y <= view.max.y)
    );
}

#[test]
fn clipped_disjoint_islands_keep_even_odd_coverage_without_polygon_repair() {
    let input = vec![
        Point::new(-3.0, -2.0),
        Point::new(3.0, -2.0),
        Point::new(3.0, 3.0),
        Point::new(1.0, 3.0),
        Point::new(1.0, 0.0),
        Point::new(-1.0, 0.0),
        Point::new(-1.0, 3.0),
        Point::new(-3.0, 3.0),
    ];
    let clipped = clip_ring(
        input,
        Bounds {
            min: Point::new(-2.5, 1.0),
            max: Point::new(2.5, 2.0),
        },
        CurveLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let edges: Vec<_> = clipped
        .iter()
        .zip(clipped.iter().cycle().skip(1))
        .map(|(&a, &b)| Segment {
            a,
            b,
            arc: None,
            ..circle(1.0, 1.0)
        })
        .collect();
    for x in [-2.0, -0.5, 0.0, 0.5, 2.0] {
        assert_eq!(
            path_contains(&edges, Point::new(x, 1.5), &CancellationToken::default()).unwrap(),
            x.abs() > 1.0
        );
    }
}

#[test]
fn endpoint_connectors_are_retained_when_arc_endpoints_differ() {
    let mut arc = circle(1.0, std::f64::consts::FRAC_PI_2);
    arc.a = Point::new(1.2, 0.0);
    arc.b = Point::new(0.0, 1.2);
    let points = tessellate_curve_ring(
        &[arc],
        view(2.0),
        0.01,
        CurveLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        (points[0], points[1], points.last().copied()),
        (
            Point::new(1.2, 0.0),
            Point::new(1.0, 0.0),
            Some(Point::new(0.0, 1.2))
        )
    );
}

#[test]
fn point_limit_aborts_refinement_instead_of_returning_coarse_geometry() {
    let result = tessellate_curve_ring(
        &[circle(10.0, std::f64::consts::TAU)],
        view(11.0),
        1e-9,
        CurveLimits {
            max_points: 8,
            ..CurveLimits::default()
        },
        &CancellationToken::default(),
    );
    assert!(matches!(
        result,
        Err(MeshError::Limit {
            resource: "curve_points",
            ..
        })
    ));
}

#[test]
fn clipping_accounts_for_live_input_and_output_before_allocation() {
    let points = vec![
        Point::new(-2.0, -2.0),
        Point::new(2.0, -2.0),
        Point::new(2.0, 2.0),
        Point::new(-2.0, 2.0),
    ];
    let result = clip_ring(
        points,
        view(1.0),
        CurveLimits {
            max_allocation_bytes: 4 * size_of::<Point>(),
            ..CurveLimits::default()
        },
        &CancellationToken::default(),
    );
    assert!(matches!(
        result,
        Err(MeshError::Limit {
            resource: "curve_bytes",
            ..
        })
    ));
}

#[test]
fn cancellation_is_checked_before_empty_path_and_clipping() {
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        tessellate_curve_ring(&[], view(1.0), 0.01, CurveLimits::default(), &token),
        Err(MeshError::Cancelled)
    ));
    assert!(matches!(
        clip_ring(vec![], view(1.0), CurveLimits::default(), &token),
        Err(MeshError::Cancelled)
    ));
}

#[test]
fn invalid_tolerance_view_arc_and_coordinates_return_structured_errors() {
    let token = CancellationToken::default();
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            tessellate_curve_ring(&[], view(1.0), tolerance, CurveLimits::default(), &token),
            Err(MeshError::Invalid {
                field: "CURVE_TOLERANCE",
                ..
            })
        ));
    }
    let mut arc = circle(1.0, 1.0);
    arc.arc.as_mut().unwrap().radius = -1.0;
    assert!(matches!(
        tessellate_curve_ring(&[arc], view(1.0), 0.01, CurveLimits::default(), &token),
        Err(MeshError::Invalid {
            field: "CURVE_ARC",
            ..
        })
    ));
    assert!(matches!(
        clip_ring(
            vec![Point::new(f64::NAN, 0.0)],
            view(1.0),
            CurveLimits::default(),
            &token
        ),
        Err(MeshError::Invalid {
            field: "CURVE_COORDINATES",
            ..
        })
    ));
    assert!(matches!(
        clip_ring(vec![], view(f64::INFINITY), CurveLimits::default(), &token),
        Err(MeshError::Invalid {
            field: "CURVE_VIEW",
            ..
        })
    ));
}

#[test]
fn web_endpoint_rounding_preserves_the_sub_ulp_connector() {
    // USBC_FPC zone 26426: removing this distinct arc endpoint changes the
    // reference ring's vertex count even though it is visually subpixel.
    let arc = Arc {
        center: Point::new(1.0007626327580201, -0.7247072141818008),
        radius: 0.15676857255622229,
        start: 1.7165103813543867,
        sweep: -0.7914194671792147,
    };
    assert_eq!(
        arc_point(arc, arc.start + arc.sweep),
        Point::new(1.0951, -0.5994999999999999)
    );
}
