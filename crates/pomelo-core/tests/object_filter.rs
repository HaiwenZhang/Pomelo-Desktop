use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    display::{BoardDisplay, filter::ObjectFilter},
    model::*,
    picking::{PickFilter, PickQuery},
    picking_index::BoardPickingIndex,
    task::CancellationToken,
};
use std::sync::Arc;
fn source(paths: bool) -> Arc<BoardScene> {
    let rings = vec![
        vec![
            Point::new(-5.0, -5.0),
            Point::new(5.0, -5.0),
            Point::new(5.0, 5.0),
            Point::new(-5.0, 5.0),
        ],
        vec![
            Point::new(-0.25, -0.25),
            Point::new(0.25, -0.25),
            Point::new(0.25, 0.25),
            Point::new(-0.25, 0.25),
        ],
    ];
    let paths = if paths {
        rings
            .iter()
            .map(|r| {
                r.iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(&a, &b)| Segment {
                        id: ObjectId(1),
                        track_id: ObjectId(1),
                        layer: LayerId(0),
                        net: NetId(0),
                        a,
                        b,
                        width: 0.0,
                        arc: None,
                        bond_wire: None,
                    })
                    .collect()
            })
            .collect()
    } else {
        vec![]
    };
    let mesh = CopperMesh::build(
        &rings,
        &paths,
        &MeshLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    Arc::new(BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: Default::default(),
        segments: vec![],
        pins: vec![Pin {
            id: ObjectId(2),
            owner_id: ObjectId(3),
            net: NetId(0),
            name: "1".into(),
            reference: "J1".into(),
            at: Point::new(8.0, 0.0),
            angle: 0.7,
            mirrored: false,
            drill: 0.0,
            drill_shape: DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            pads: vec![Pad::circle(LayerId(0), 1.0)],
            stackup_region: None,
            die: None,
        }],
        components: vec![],
        vias: vec![],
        zones: vec![Zone {
            id: ObjectId(1),
            layer: LayerId(0),
            net: NetId(0),
            kind: ZoneKind::Dynamic,
            paths,
            mesh,
        }],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        bounds: Bounds {
            min: Point::new(-10.0, -10.0),
            max: Point::new(10.0, 10.0),
        },
        diagnostics: vec![],
    })
}
#[test]
fn visible_picking_follows_pad_and_void_filtering_after_zoom() {
    let cancel = CancellationToken::default();
    for paths in [false, true] {
        let index = BoardPickingIndex::build(source(paths), 100, &cancel).unwrap();
        let query = |point, scale, display: &BoardDisplay| {
            index
                .query_visible_objects(
                    PickQuery::new(point, 0.0).unwrap(),
                    scale,
                    PickFilter::all(),
                    display,
                    16,
                    &cancel,
                )
                .unwrap()
        };
        let mut display = BoardDisplay::default();
        assert!(query(Point::default(), 10.0, &display).is_empty());
        display.object_filter.minimum_pixels = 6;
        assert_eq!(
            query(Point::default(), 10.0, &display).len(),
            1,
            "filtered hole is visually filled and pickable"
        );
        assert!(
            query(Point::default(), 20.0, &display).is_empty(),
            "hole becomes large enough after zoom"
        );
        display.object_filter.voids = false;
        assert!(query(Point::default(), 10.0, &display).is_empty());
        display.object_filter = ObjectFilter {
            minimum_pixels: 11,
            ..Default::default()
        };
        assert!(query(Point::new(8.0, 0.0), 10.0, &display).is_empty());
        assert_eq!(query(Point::new(8.0, 0.0), 20.0, &display).len(), 1);
        display.object_filter.pins = false;
        assert_eq!(query(Point::new(8.0, 0.0), 10.0, &display).len(), 1);
    }
}
