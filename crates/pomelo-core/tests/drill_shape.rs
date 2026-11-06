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
