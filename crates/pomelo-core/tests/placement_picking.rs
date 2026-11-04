use pomelo_core::{
    display::{BoardDisplay, DisplayCategory, PlacementQuickview},
    interaction::{Camera, SelectionMode},
    model::{
        BoardScene, Bounds, ComponentBody, ComponentPlacement, ComponentRegion, LayerId, NetId,
        ObjectId, Point, Segment,
    },
    picking::{PickCategory, PickFilter, PickQuery},
    picking_index::BoardPickingIndex,
    selection::{SelectedObject, SelectionTarget, canvas_target},
    task::CancellationToken,
};
use std::sync::Arc;

fn rectangle(id: u32, min: Point, max: Point, at: Point, angle: f64) -> Vec<Segment> {
    let points = [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)].map(|p| {
        Point::new(
            at.x + p.x * angle.cos() - p.y * angle.sin(),
            at.y + p.x * angle.sin() + p.y * angle.cos(),
        )
    });
    (0..4)
        .map(|i| Segment {
            id: ObjectId(id),
            track_id: ObjectId(id),
            layer: LayerId(0),
            net: NetId(0),
            a: points[i],
            b: points[(i + 1) % 4],
            width: 0.0,
            arc: None,
            bond_wire: None,
        })
        .collect()
}
fn component(id: u32, layer: LayerId, angle: f64, at: Point) -> ComponentPlacement {
    ComponentPlacement {
        id: ObjectId(id),
        source_reference: None,
        reference: format!("U{id}"),
        at,
        angle,
        mirrored: layer == LayerId(1),
        pins: vec![],
        body: Some(ComponentBody {
            layer,
            regions: vec![ComponentRegion {
                source: ObjectId(id + 100),
                source_layer: LayerId::allegro_drawing(0xfb09),
                paths: vec![
                    rectangle(id, Point::new(-4.0, -3.0), Point::new(4.0, 3.0), at, angle),
                    rectangle(id, Point::new(-0.5, -0.5), Point::new(0.5, 0.5), at, angle),
                ],
            }],
        }),
    }
}
fn scene(components: Vec<ComponentPlacement>) -> Arc<BoardScene> {
    Arc::new(BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: Default::default(),
        segments: vec![],
        pins: vec![],
        components,
        vias: vec![],
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        diagnostics: vec![],
        bounds: Bounds {
            min: Point::new(-30.0, -30.0),
            max: Point::new(30.0, 30.0),
        },
    })
}
fn display(layer: LayerId) -> BoardDisplay {
    let mut display = BoardDisplay {
        quickview: Some(PlacementQuickview { layer, pins: false }),
        show_texts: false,
        ..Default::default()
    };
    display.hidden_layers.extend([LayerId(0), LayerId(1)]);
    display
}
fn query(point: Point, scale: f64, flipped: bool) -> PickQuery {
    let camera = Camera {
        center: Point::new(point.x + 0.1, point.y - 0.1),
        pixels_per_mm: scale,
        flipped,
    };
    let size = Point::new(800.0, 600.0);
    PickQuery::from_viewport(
        camera,
        camera.board_to_view(point, size.x, size.y),
        size,
        5.0,
    )
    .unwrap()
}

#[test]
fn rotated_placement_picks_actual_regions_not_their_axis_aligned_bounds() {
    let at = Point::new(12.0, -7.0);
    let angle = 0.6_f64;
    let local = |x: f64, y: f64| {
        Point::new(
            at.x + x * angle.cos() - y * angle.sin(),
            at.y + x * angle.sin() + y * angle.cos(),
        )
    };
    let scene = scene(vec![component(80, LayerId(0), angle, at)]);
    let cancel = CancellationToken::default();
    let index = BoardPickingIndex::build(Arc::clone(&scene), 100, &cancel).unwrap();
    let display = display(LayerId(0));
    for flipped in [false, true] {
        for scale in [5.0, 50.0, 500.0] {
            let hits = index
                .query_visible_hits(
                    query(local(2.0, 1.0), scale, flipped),
                    scale,
                    PickFilter::all(),
                    &display,
                    64,
                    &cancel,
                )
                .unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0].anchor.object,
                SelectedObject::Placement(ObjectId(80))
            );
            assert_eq!(hits[0].anchor.category, DisplayCategory::Placement);
            assert_eq!(
                canvas_target(
                    &scene,
                    hits[0].anchor.object,
                    SelectionMode::Object,
                    &cancel
                )
                .unwrap(),
                SelectionTarget::Component(ObjectId(80))
            );
            assert!(
                index
                    .query_visible_hits(
                        query(at, scale, flipped),
                        scale,
                        PickFilter::all(),
                        &display,
                        64,
                        &cancel
                    )
                    .unwrap()
                    .is_empty(),
                "source void remains unpickable"
            );
            let outside = Point::new(at.x + 4.8, at.y + 4.4);
            assert!(
                index
                    .query_visible_hits(
                        query(outside, scale, flipped),
                        scale,
                        PickFilter::all(),
                        &display,
                        64,
                        &cancel
                    )
                    .unwrap()
                    .is_empty(),
                "empty corners of rotated bounds are not component area"
            );
        }
    }
    let target = SelectionTarget::Component(ObjectId(80));
    let bounds = index.selection_bounds(target, &cancel).unwrap().unwrap();
    assert!(bounds.min.x < at.x - 4.0 && bounds.max.x > at.x + 4.0);
    let anchor = index
        .selection_anchor(target, &display, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(anchor.object, SelectedObject::Placement(ObjectId(80)));
    assert_eq!(
        target.summarize(&scene, &cancel).unwrap().components.len(),
        1
    );
    assert!(
        target
            .component_objects(&scene, &cancel)
            .unwrap()
            .contains(&anchor.object)
    );
    let json = serde_json::to_string(&anchor).unwrap();
    assert_eq!(
        serde_json::from_str::<pomelo_core::picking_index::SelectionAnchor>(&json).unwrap(),
        anchor
    );
}

#[test]
fn quickview_side_off_and_picking_filter_control_placement_eligibility() {
    let scene = scene(vec![
        component(80, LayerId(0), 0.0, Point::default()),
        component(81, LayerId(1), 0.0, Point::default()),
    ]);
    let cancel = CancellationToken::default();
    let index = BoardPickingIndex::build(scene, 100, &cancel).unwrap();
    let point = query(Point::new(2.0, 1.0), 100.0, false);
    let mut display = display(LayerId(0));
    display.object_filter.minimum_pixels = 100;
    let hits = |display: &BoardDisplay, filter| {
        index
            .query_visible_hits(point, 100.0, filter, display, 64, &cancel)
            .unwrap()
    };
    assert_eq!(
        hits(&display, PickFilter::all())[0].anchor.object,
        SelectedObject::Placement(ObjectId(80))
    );
    display.quickview = Some(PlacementQuickview {
        layer: LayerId(1),
        pins: false,
    });
    assert_eq!(
        hits(&display, PickFilter::all())[0].anchor.object,
        SelectedObject::Placement(ObjectId(81))
    );
    let mut filter = PickFilter::all();
    filter.set(PickCategory::Pin, false);
    assert!(hits(&display, filter).is_empty());
    display.quickview = None;
    assert!(hits(&display, PickFilter::all()).is_empty());
}

#[test]
fn overlapping_placements_preserve_topmost_order_and_cyclable_component_targets() {
    let scene = scene(vec![
        component(80, LayerId(0), 0.0, Point::default()),
        component(81, LayerId(0), 0.0, Point::default()),
    ]);
    let cancel = CancellationToken::default();
    let index = BoardPickingIndex::build(Arc::clone(&scene), 100, &cancel).unwrap();
    let hits = index
        .query_visible_hits_grouped(
            query(Point::new(2.0, 1.0), 100.0, false),
            100.0,
            PickFilter::all(),
            &display(LayerId(0)),
            64,
            &cancel,
            |object| canvas_target(&scene, object, SelectionMode::Component, &cancel),
        )
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.anchor.object).collect::<Vec<_>>(),
        vec![
            SelectedObject::Placement(ObjectId(81)),
            SelectedObject::Placement(ObjectId(80))
        ]
    );
    let candidates = pomelo_core::selection::resolve_canvas_candidates(
        &scene,
        &hits
            .into_iter()
            .map(|hit| hit.object_hit())
            .collect::<Vec<_>>(),
        SelectionMode::Component,
        64,
        &cancel,
    )
    .unwrap();
    assert_eq!(
        candidates.iter().map(|c| c.target).collect::<Vec<_>>(),
        vec![
            SelectionTarget::Component(ObjectId(81)),
            SelectionTarget::Component(ObjectId(80))
        ]
    );
}

#[test]
fn analytic_round_regions_keep_voids_and_check_cancellation() {
    use pomelo_core::model::Arc as BoardArc;
    let circle = |radius| Segment {
        id: ObjectId(180),
        track_id: ObjectId(180),
        layer: LayerId(0),
        net: NetId(0),
        a: Point::new(radius, 0.0),
        b: Point::new(radius, 0.0),
        width: 0.0,
        arc: Some(BoardArc {
            center: Point::default(),
            radius,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        }),
        bond_wire: None,
    };
    let mut c = component(80, LayerId(0), 0.0, Point::default());
    c.body.as_mut().unwrap().regions[0].paths = vec![vec![circle(3.0)], vec![circle(1.0)]];
    let cancel = CancellationToken::default();
    let index = BoardPickingIndex::build(scene(vec![c]), 100, &cancel).unwrap();
    for (p, expected) in [
        (Point::new(2.0, 0.0), 1),
        (Point::default(), 0),
        (Point::new(2.8, 2.8), 0),
        (Point::new(3.0, 0.0), 1),
    ] {
        assert_eq!(
            index
                .query_visible_hits(
                    query(p, 100.0, false),
                    100.0,
                    PickFilter::all(),
                    &display(LayerId(0)),
                    64,
                    &cancel
                )
                .unwrap()
                .len(),
            expected
        );
    }
    cancel.cancel();
    assert!(matches!(
        index.query_visible_hits(
            query(Point::new(2.0, 0.0), 100.0, false),
            100.0,
            PickFilter::all(),
            &display(LayerId(0)),
            64,
            &cancel
        ),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn unnamed_placement_keeps_its_component_identity_and_typed_saved_highlights() {
    use pomelo_core::{
        appearance::RgbColor,
        review::highlight::{HighlightGroup, ObjectCatalog},
    };
    let mut c = component(80, LayerId(0), 0.0, Point::default());
    c.reference.clear();
    let scene = scene(vec![c]);
    let cancel = CancellationToken::default();
    let object = SelectedObject::Placement(ObjectId(80));
    for mode in [SelectionMode::Object, SelectionMode::Component] {
        assert_eq!(
            canvas_target(&scene, object, mode, &cancel).unwrap(),
            SelectionTarget::Component(ObjectId(80))
        );
    }
    let group = HighlightGroup::new(1, "Selected parts".into(), RgbColor([10, 20, 30]), [object]);
    let json = serde_json::to_string(&group).unwrap();
    assert!(json.contains("placements"));
    let restored: HighlightGroup = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.members, vec![object]);
    assert_eq!(
        ObjectCatalog::build(&scene, &cancel)
            .unwrap()
            .missing_members(&restored),
        0
    );
    assert_eq!(
        pomelo_core::review::selection_set::count_objects([object]).total(),
        1
    );
}
