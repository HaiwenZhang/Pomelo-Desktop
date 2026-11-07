use pomelo_core::{
    model::{DrillShape, Point},
    pad::PadPlacement,
};

#[test]
fn older_serialized_drills_keep_round_ended_shape() {
    let drill: DrillShape =
        serde_json::from_str(r#"{"width":2.0,"height":1.0,"plated":true}"#).unwrap();
    assert!(!drill.rectangular);
    assert!(
        drill
            .pad()
            .unwrap()
            .analytic_distance(Point::new(0.9, 0.4), PadPlacement::default())
            .unwrap()
            > 0.0
    );
}

#[test]
fn rectangular_holes_include_the_corners_that_round_slots_remove() {
    let drill = DrillShape {
        custom: None,
        width: 2.0,
        height: 1.0,
        plated: true,
        rectangular: true,
    };
    let pad = drill.pad().unwrap();
    assert!(
        pad.analytic_distance(Point::new(0.9, 0.4), PadPlacement::default())
            .unwrap()
            < 0.0
    );
    assert!(
        pad.analytic_distance(Point::new(1.1, 0.4), PadPlacement::default())
            .unwrap()
            > 0.0
    );
}

#[test]
fn custom_hole_roundtrips_and_keeps_the_polygon_instead_of_its_bounds() {
    use pomelo_core::{model::CustomPadGeometry, task::CancellationToken};
    let drill = DrillShape {
        width: 0.6,
        height: 0.6,
        plated: true,
        rectangular: false,
        custom: Some(std::sync::Arc::new(CustomPadGeometry {
            contours: vec![vec![
                Point::new(-0.2, 0.3),
                Point::new(0.2, 0.3),
                Point::new(0.3, 0.2),
                Point::new(0.3, -0.2),
                Point::new(0.2, -0.3),
                Point::new(-0.2, -0.3),
                Point::new(-0.3, -0.2),
                Point::new(-0.3, 0.2),
            ]],
            paths: vec![],
        })),
    };
    let shared = drill.pad().unwrap();
    assert!(std::sync::Arc::ptr_eq(
        shared.custom.as_ref().unwrap(),
        drill.custom.as_ref().unwrap()
    ));
    let decoded: DrillShape =
        serde_json::from_str(&serde_json::to_string(&drill).unwrap()).unwrap();
    let cancel = CancellationToken::default();
    let pad = decoded.pad().unwrap();
    assert_eq!(
        pad.custom_covers_fill(Point::new(0.0, 0.0), PadPlacement::default(), &cancel)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        pad.custom_covers_fill(Point::new(0.29, 0.29), PadPlacement::default(), &cancel)
            .unwrap(),
        Some(false)
    );
}
