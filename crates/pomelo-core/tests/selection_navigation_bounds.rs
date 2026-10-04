use pomelo_core::{
    interaction::component_reference::ComponentAnchor,
    model::{BoardDrawing, BoardScene, Bounds, LayerId, NetId, ObjectId, Pad, Point, Zone},
    picking_index::{SegmentIndex, TextPickQuad},
    selection::{SelectedObject, SelectionTarget},
    task::CancellationToken,
};
use std::sync::Arc;

fn scene() -> BoardScene {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/component-reference-groups.json"
    ))
    .unwrap()
}

fn index(scene: BoardScene) -> SegmentIndex {
    SegmentIndex::build(Arc::new(scene), 1000, &CancellationToken::default()).unwrap()
}

fn extent(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Bounds {
    Bounds {
        min: Point::new(min_x, min_y),
        max: Point::new(max_x, max_y),
    }
}

#[test]
fn group_bounds_use_pad_extents_without_placement_or_pin_origins() {
    let mut scene = scene();
    let mut pad = Pad::circle(LayerId(1), 2.0);
    pad.offset = Point::new(10.0, 5.0);
    scene.pins[0].at = Point::new(0.0, 0.0);
    scene.pins[0].pads = vec![pad];
    // Duplicate-reference placements and members without geometry remain source
    // members, but their far-away origins cannot inflate navigation bounds.
    scene.pins[1].at = Point::new(1000.0, 0.0);
    let bounds = index(scene)
        .selection_bounds(
            SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10))),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(bounds, Some(extent(9.0, 4.0, 11.0, 6.0)));
}

#[test]
fn geometryless_members_and_unsupported_pads_have_no_navigation_bounds() {
    let mut scene = scene();
    let mut unsupported = Pad::circle(LayerId(1), 999.0);
    unsupported.kind = pomelo_core::model::PadKind(0xffff);
    scene.pins[0].pads.push(unsupported);
    let index = index(scene);
    for target in [
        SelectionTarget::Object(SelectedObject::Pin(ObjectId(10))),
        SelectionTarget::ComponentGroup(ComponentAnchor::Finger(ObjectId(20))),
        SelectionTarget::Net(NetId(0)),
    ] {
        assert_eq!(
            index
                .selection_bounds(target, &CancellationToken::default())
                .unwrap(),
            None
        );
    }
}

#[test]
fn group_bounds_include_members_after_the_first_inspector_page() {
    let mut scene = scene();
    let prototype = scene.pins[0].clone();
    scene.pins = (0..600)
        .map(|id| {
            let mut pin = prototype.clone();
            pin.id = ObjectId(id);
            pin.at = Point::new(f64::from(id), 0.0);
            pin.pads = vec![Pad::circle(LayerId(1), 2.0)];
            pin
        })
        .collect();
    let bounds = index(scene)
        .selection_bounds(
            SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(0))),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(bounds, Some(extent(-1.0, -1.0, 600.0, 1.0)));
}

#[test]
fn zone_bounds_use_exterior_only_without_hole_or_stroke_expansion() {
    let mut scene = scene();
    let mesh = pomelo_core::copper::CopperMesh::build(
        &[
            vec![
                Point::new(1.0, 1.0),
                Point::new(3.0, 1.0),
                Point::new(3.0, 4.0),
            ],
            vec![
                Point::new(-50.0, -50.0),
                Point::new(50.0, -50.0),
                Point::new(50.0, 50.0),
            ],
        ],
        &[],
        &pomelo_core::copper::MeshLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    scene.zones.push(Zone {
        kind: pomelo_core::model::ZoneKind::Unknown,
        id: ObjectId(50),
        layer: LayerId(1),
        net: NetId(9),
        paths: vec![],
        mesh,
    });
    let bounds = index(scene)
        .selection_bounds(
            SelectionTarget::Object(SelectedObject::Zone(ObjectId(50))),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(bounds, Some(extent(1.0, 1.0, 3.0, 4.0)));
}

#[test]
fn drawing_bounds_use_exact_supplied_glyphs_for_objects_and_nets() {
    let mut scene = scene();
    scene.drawings.push(BoardDrawing {
        id: ObjectId(88),
        owner_id: None,
        layer: LayerId(1),
        net: NetId(9),
        graphic_ids: vec![],
        segments: vec![],
        text_ids: vec![ObjectId(77)],
    });
    // Only the text ID and layer are needed by the core glyph adapter.
    let text: pomelo_core::model::BoardText = serde_json::from_value(serde_json::json!({
        "id":77,"owner_id":null,"layer":1,"class_id":0,"subclass":0,"text":"測試",
        "at":{"x":1000.0,"y":1000.0},"angle":1.0,"mirrored":true,"align":"center",
        "font_index":0,"width":1000.0,"height":1000.0,"spacing":0.0,"line_spacing":1.0,"stroke_width":0.0
    })).unwrap();
    scene.texts.push(text);
    let cancel = CancellationToken::default();
    let index = index(scene)
        .with_text_quads(
            &[TextPickQuad {
                text: ObjectId(77),
                corners: [
                    Point::new(2.0, 4.0),
                    Point::new(3.0, 5.0),
                    Point::new(2.0, 6.0),
                    Point::new(1.0, 5.0),
                ],
            }],
            &cancel,
        )
        .unwrap();
    for target in [
        SelectionTarget::Object(SelectedObject::Drawing(ObjectId(88))),
        SelectionTarget::Net(NetId(9)),
    ] {
        assert_eq!(
            index.selection_bounds(target, &cancel).unwrap(),
            Some(extent(1.0, 4.0, 3.0, 6.0))
        );
    }
}

#[test]
fn navigation_bounds_cancel_without_publishing_partial_geometry() {
    let index = index(scene());
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        index.selection_bounds(
            SelectionTarget::Object(SelectedObject::Pin(ObjectId(10))),
            &cancel
        ),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}
