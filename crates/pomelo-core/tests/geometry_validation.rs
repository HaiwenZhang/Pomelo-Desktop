use pomelo_core::{
    geometry::{PathError, path_contains},
    model::{Arc, Bounds, LayerId, NetId, ObjectId, Point, Segment},
    task::CancellationToken,
};

#[test]
fn bounds_from_points_rejects_a_nan_point_in_any_position() {
    let nan = Point::new(f64::NAN, 1.0);
    let finite = [Point::new(0.0, 0.0), Point::new(2.0, 3.0)];
    for position in 0..=finite.len() {
        let mut points = finite.to_vec();
        points.insert(position, nan);
        assert_eq!(Bounds::from_points(points), None, "NaN at {position}");
    }
}

#[test]
fn bounds_from_points_rejects_infinite_coordinates() {
    let points = [Point::new(0.0, 0.0), Point::new(1.0, f64::INFINITY)];
    assert_eq!(Bounds::from_points(points), None);
}

#[test]
fn bounds_from_points_keeps_finite_extent() {
    let bounds = Bounds::from_points([Point::new(1.0, -2.0), Point::new(-3.0, 4.0)]).unwrap();
    assert_eq!(bounds.min, Point::new(-3.0, -2.0));
    assert_eq!(bounds.max, Point::new(1.0, 4.0));
}

fn arc_edge(start: f64, sweep: f64) -> Segment {
    let at = |angle: f64| Point::new(angle.cos(), angle.sin());
    Segment {
        id: ObjectId(9),
        track_id: ObjectId(9),
        layer: LayerId(1),
        net: NetId(0),
        a: at(start),
        b: at(start + sweep),
        width: 0.0,
        arc: Some(Arc {
            center: Point::default(),
            radius: 1.0,
            start,
            sweep,
        }),
        bond_wire: None,
    }
}

#[test]
fn path_contains_rejects_arc_whose_cardinal_steps_cannot_advance() {
    let edge = arc_edge(1e18, 1.0);
    assert!(
        edge.centreline_bounds().is_some(),
        "input passes the bounds precheck"
    );
    // Run on a worker so a regression fails the test instead of hanging the suite.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = path_contains(&[edge], Point::new(0.5, 0.0), &CancellationToken::default());
        let _ = sender.send(result);
    });
    let result = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("path_contains must terminate for huge finite start angles");
    assert!(matches!(result, Err(PathError::Invalid(ObjectId(9)))));
}

#[test]
fn path_contains_still_handles_ordinary_full_circle() {
    let edge = arc_edge(0.0, std::f64::consts::TAU);
    let cancel = CancellationToken::default();
    assert!(path_contains(std::slice::from_ref(&edge), Point::new(0.5, 0.0), &cancel).unwrap());
    assert!(!path_contains(&[edge], Point::new(1.5, 0.0), &cancel).unwrap());
}
