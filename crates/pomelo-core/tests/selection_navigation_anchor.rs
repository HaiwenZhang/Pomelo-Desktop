use pomelo_core::{
    display::{BoardDisplay, DisplayCategory, LayerPrimitive, LayerPriority},
    interaction::component_reference::ComponentAnchor,
    model::{BoardScene, LayerId, NetId, ObjectId, Pad},
    picking_index::{SegmentIndex, SelectionAnchor},
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
fn anchor(id: u32, layer: u32, category: DisplayCategory) -> Option<SelectionAnchor> {
    Some(SelectionAnchor {
        object: SelectedObject::Pin(ObjectId(id)),
        layer: LayerId(layer),
        category,
    })
}
fn group() -> SelectionTarget {
    SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10)))
}

fn submission_scene() -> BoardScene {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/selection-navigation-anchor.json"
    ))
    .unwrap()
}

#[test]
fn preferred_hidden_member_survives_a_group_mode_change() {
    let index = index(submission_scene());
    let preferred = anchor(31, 0, DisplayCategory::Pin);
    let mut display = BoardDisplay::default();
    display.hidden_layers.insert(LayerId(0));
    assert_eq!(
        index
            .selection_anchor_with_preferred(
                SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(30))),
                &display,
                preferred,
                &CancellationToken::default()
            )
            .unwrap(),
        preferred
    );
}

#[test]
fn preferred_member_of_another_group_is_rejected() {
    let index = index(submission_scene());
    assert_eq!(
        index
            .selection_anchor_with_preferred(
                SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(30))),
                &BoardDisplay::default(),
                anchor(70, 2, DisplayCategory::Trace),
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(30, 0, DisplayCategory::Pin)
    );
}

#[test]
fn nonexistent_preferred_layer_falls_back_to_a_valid_entry() {
    assert_eq!(
        index(submission_scene())
            .selection_anchor_with_preferred(
                SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(30))),
                &BoardDisplay::default(),
                anchor(30, 999, DisplayCategory::Pin),
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(30, 0, DisplayCategory::Pin)
    );
}

#[test]
fn preferred_zone_boundary_category_survives_restore() {
    let preferred = Some(SelectionAnchor {
        object: SelectedObject::Zone(ObjectId(50)),
        layer: LayerId(1),
        category: DisplayCategory::ZoneOutline,
    });
    assert_eq!(
        index(submission_scene())
            .selection_anchor_with_preferred(
                SelectionTarget::Net(NetId(9)),
                &BoardDisplay::default(),
                preferred,
                &CancellationToken::default()
            )
            .unwrap(),
        preferred
    );
}

#[test]
fn canvas_die_hit_retains_regular_copper_layer_and_etch_category() {
    let display = BoardDisplay {
        active_layer: Some(LayerId(2)),
        ..BoardDisplay::default()
    };
    let mut filter = pomelo_core::picking::PickFilter::none();
    filter.set(pomelo_core::picking::PickCategory::Pin, true);
    let hits = index(submission_scene())
        .query_visible_hits(
            pomelo_core::picking::PickQuery::new(pomelo_core::model::Point::new(0.0, 0.0), 0.05)
                .unwrap(),
            100.0,
            filter,
            &display,
            1,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        hits.first().map(|hit| hit.anchor),
        anchor(70, 2, DisplayCategory::Trace)
    );
}

#[test]
fn canvas_bond_wire_hit_retains_distinct_category() {
    let hits = index(submission_scene())
        .query_visible_hits(
            pomelo_core::picking::PickQuery::new(pomelo_core::model::Point::new(1.0, 1.0), 0.05)
                .unwrap(),
            100.0,
            pomelo_core::picking::PickFilter::all(),
            &BoardDisplay::default(),
            64,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        hits.iter()
            .find(|hit| hit.anchor.object == SelectedObject::Segment(ObjectId(80)))
            .map(|hit| hit.anchor.category),
        Some(DisplayCategory::BondWire)
    );
}

#[test]
fn die_pad_follows_active_etch_layer_even_on_a_regular_copper_layer() {
    let display = BoardDisplay {
        active_layer: Some(LayerId(2)),
        ..BoardDisplay::default()
    };
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(70)),
                &display,
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(70, 2, DisplayCategory::Trace)
    );
}

#[test]
fn bond_wire_anchor_retains_its_display_category() {
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(80)),
                &BoardDisplay::default(),
                &CancellationToken::default()
            )
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Segment(ObjectId(80)),
            layer: LayerId::BOND_WIRE_TOP,
            category: DisplayCategory::BondWire
        })
    );
}

#[test]
fn custom_pad_is_submitted_after_later_analytic_pin_in_same_group() {
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(30)));
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                target,
                &BoardDisplay::default(),
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(30, 0, DisplayCategory::Pin)
    );
}

#[test]
fn drill_scope_group_order_wins_over_last_via_source_position() {
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(50)),
                &BoardDisplay::default(),
                &CancellationToken::default()
            )
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Via(ObjectId(51)),
            layer: LayerId::UNASSIGNED,
            category: DisplayCategory::Drill
        })
    );
}

#[test]
fn backdrill_anchor_is_independent_of_regular_drill_switch() {
    let display = BoardDisplay {
        show_drills: false,
        ..BoardDisplay::default()
    };
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(60)),
                &display,
                &CancellationToken::default()
            )
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Via(ObjectId(60)),
            layer: LayerId::UNASSIGNED,
            category: DisplayCategory::Drill
        })
    );
}

#[test]
fn backdrill_base_pad_is_visible_when_backdrill_and_drill_marks_are_disabled() {
    let display = BoardDisplay {
        show_drills: false,
        show_backdrills: false,
        ..BoardDisplay::default()
    };
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(60)),
                &display,
                &CancellationToken::default()
            )
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Via(ObjectId(60)),
            layer: LayerId(0),
            category: DisplayCategory::Via
        })
    );
}

#[test]
fn hidden_via_copper_controls_do_not_hide_an_independent_pin_drill() {
    let mut display = BoardDisplay::default();
    display.set_primitive(LayerId(0), LayerPrimitive::Vias, false);
    assert_eq!(
        index(submission_scene())
            .selection_anchor(
                SelectionTarget::Net(NetId(40)),
                &display,
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(40, u32::MAX, DisplayCategory::Drill)
    );
}

#[test]
fn drawing_anchor_uses_indexed_glyphs_only_when_text_display_is_enabled() {
    use pomelo_core::{model::Point, picking_index::TextPickQuad};
    let mut scene = submission_scene();
    scene.drawings[0].segments = vec![scene.zones[0].paths[0][0].clone()];
    scene.drawings[0].layer = LayerId(2);
    let quad = TextPickQuad {
        text: ObjectId(100),
        corners: [
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ],
    };
    let cancel = CancellationToken::default();
    let index = index(scene).with_text_quads(&[quad], &cancel).unwrap();
    let target = SelectionTarget::Object(SelectedObject::Drawing(ObjectId(200)));
    assert_eq!(
        index
            .selection_anchor(target, &BoardDisplay::default(), &cancel)
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Drawing(ObjectId(200)),
            layer: LayerId(2),
            category: DisplayCategory::Drawing
        })
    );
    let display = BoardDisplay {
        show_texts: true,
        ..BoardDisplay::default()
    };
    assert_eq!(
        index.selection_anchor(target, &display, &cancel).unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Drawing(ObjectId(200)),
            layer: LayerId(1),
            category: DisplayCategory::Text
        })
    );
}

#[test]
fn visible_lower_layer_wins_over_hidden_top_without_reducing_bounds() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0), Pad::circle(LayerId(1), 4.0)];
    let index = index(scene);
    let mut display = BoardDisplay::default();
    display.hidden_layers.insert(LayerId(0));
    let bounds = index
        .selection_bounds(group(), &CancellationToken::default())
        .unwrap();
    assert_eq!(
        index
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        anchor(10, 1, DisplayCategory::Pin)
    );
    assert_eq!(
        index
            .selection_bounds(group(), &CancellationToken::default())
            .unwrap(),
        bounds
    );
}

#[test]
fn all_hidden_entries_fall_back_to_highest_display_rank() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0), Pad::circle(LayerId(1), 2.0)];
    let mut display = BoardDisplay::default();
    display.hidden_layers.extend([LayerId(0), LayerId(1)]);
    assert_eq!(
        index(scene)
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        anchor(10, 0, DisplayCategory::Pin)
    );
}

#[test]
fn geometryless_component_has_no_anchor() {
    assert_eq!(
        index(scene())
            .selection_anchor(
                group(),
                &BoardDisplay::default(),
                &CancellationToken::default()
            )
            .unwrap(),
        None
    );
}

#[test]
fn manual_pin_promotion_overrides_physical_top_order() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0), Pad::circle(LayerId(1), 2.0)];
    let display = BoardDisplay {
        priorities: vec![LayerPriority {
            layer: LayerId(1),
            category: DisplayCategory::Pin,
        }],
        ..BoardDisplay::default()
    };
    assert_eq!(
        index(scene)
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        anchor(10, 1, DisplayCategory::Pin)
    );
}

#[test]
fn independent_pin_drill_remains_visible_when_all_copper_is_hidden() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0)];
    scene.pins[0].drill_shape.width = 1.0;
    scene.pins[0].drill_shape.height = 1.0;
    let mut display = BoardDisplay::default();
    display.hidden_layers.insert(LayerId(0));
    assert_eq!(
        index(scene)
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        anchor(10, u32::MAX, DisplayCategory::Drill)
    );
}

#[test]
fn disabled_drill_uses_visible_copper_pad() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0)];
    scene.pins[0].drill_shape.width = 1.0;
    scene.pins[0].drill_shape.height = 1.0;
    let display = BoardDisplay {
        show_drills: false,
        ..BoardDisplay::default()
    };
    assert_eq!(
        index(scene)
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        anchor(10, 0, DisplayCategory::Pin)
    );
}

#[test]
fn last_group_member_beyond_inspector_page_is_the_anchor() {
    let mut scene = scene();
    let pin = scene.pins[0].clone();
    scene.pins = (0..600)
        .map(|id| {
            let mut pin = pin.clone();
            pin.id = ObjectId(id);
            pin.pads = vec![Pad::circle(LayerId(0), 2.0)];
            pin
        })
        .collect();
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(0)));
    assert_eq!(
        index(scene)
            .selection_anchor(
                target,
                &BoardDisplay::default(),
                &CancellationToken::default()
            )
            .unwrap(),
        anchor(599, 0, DisplayCategory::Pin)
    );
}

#[test]
fn hiding_pin_primitives_keeps_other_visible_group_members_eligible() {
    let mut scene = scene();
    scene.pins[0].pads = vec![Pad::circle(LayerId(0), 2.0)];
    scene.vias[0].finger.as_mut().unwrap().reference = scene.pins[0].reference.clone();
    scene.vias[0].pads = vec![Pad::circle(LayerId(1), 2.0)].into();
    let mut display = BoardDisplay::default();
    display.set_primitive(LayerId(0), LayerPrimitive::Pads, false);
    assert_eq!(
        index(scene)
            .selection_anchor(group(), &display, &CancellationToken::default())
            .unwrap(),
        Some(SelectionAnchor {
            object: SelectedObject::Via(ObjectId(10)),
            layer: LayerId(1),
            category: DisplayCategory::Via
        })
    );
}

#[test]
fn cancelled_lookup_never_returns_partial_anchor() {
    let index = index(scene());
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        index.selection_anchor(
            SelectionTarget::Net(NetId(1)),
            &BoardDisplay::default(),
            &cancel
        ),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}
