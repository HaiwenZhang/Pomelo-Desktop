use pomelo_core::{
    interaction::{SelectionMode, component_reference::ComponentAnchor},
    model::{
        BoardScene, BondFinger, Bounds, ComponentPlacement, DrillShape, LayerId, NetId, ObjectId,
        Pad, Pin, Point, Via,
    },
    picking::ObjectHit,
    search::{SearchIndex, SearchTarget},
    selection::{SelectedObject, SelectionTarget, resolve_canvas_candidates},
    task::CancellationToken,
};
use std::{collections::BTreeMap, sync::Arc};

fn pin(id: u32, owner: u32, reference: &str, x: f64) -> Pin {
    Pin {
        id: ObjectId(id),
        owner_id: ObjectId(owner),
        reference: reference.into(),
        net: NetId(0),
        name: id.to_string(),
        at: Point::new(x, 0.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        pads: vec![Pad::circle(LayerId(1), 1.0)],
        stackup_region: None,
        die: None,
    }
}
fn finger(id: u32, reference: &str, x: f64) -> Via {
    Via {
        id: ObjectId(id),
        net: NetId(0),
        at: Point::new(x, 0.0),
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        padstack: ObjectId(1),
        padstack_name: String::new(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(1)),
        pads: Arc::from([Pad::circle(LayerId(1), 1.0)]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: Some(BondFinger {
            reference: reference.into(),
            name: id.to_string(),
            source_pin: None,
        }),
    }
}
fn board() -> BoardScene {
    BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: BTreeMap::new(),
        segments: vec![],
        components: vec![
            ComponentPlacement {
                id: ObjectId(90),
                source_reference: None,
                reference: "U1".into(),
                at: Point::new(-100.0, 0.0),
                angle: 0.0,
                mirrored: false,
                pins: vec![ObjectId(10)],
            },
            ComponentPlacement {
                id: ObjectId(91),
                source_reference: None,
                reference: "U1".into(),
                at: Point::new(100.0, 0.0),
                angle: 0.0,
                mirrored: false,
                pins: vec![ObjectId(11)],
            },
        ],
        pins: vec![
            pin(10, 90, "U1", 1.0),
            pin(11, 91, "U1", 5.0),
            pin(12, 92, "u1", 20.0),
            pin(13, 93, "", 30.0),
        ],
        vias: vec![finger(10, "U1", 9.0), finger(20, "孤立_전원", 40.0)],
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        diagnostics: vec![],
        bounds: Bounds {
            min: Point::new(0.0, -1.0),
            max: Point::new(50.0, 1.0),
        },
    }
}

#[test]
fn search_groups_exact_references_counts_fingers_and_preserves_source_placements() {
    let board = board();
    let cancel = CancellationToken::default();
    let index = SearchIndex::build(&board, &cancel).unwrap().unwrap();
    let actual: Vec<_> = index
        .entries()
        .iter()
        .map(|entry| (entry.name.as_str(), entry.count, entry.target))
        .collect();
    assert_eq!(
        actual,
        vec![
            (
                "U1",
                3,
                SearchTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10)))
            ),
            (
                "u1",
                1,
                SearchTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(12)))
            ),
            (
                "孤立_전원",
                1,
                SearchTarget::ComponentGroup(ComponentAnchor::Finger(ObjectId(20)))
            )
        ]
    );
    assert_eq!(
        board.components.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![ObjectId(90), ObjectId(91)]
    );
}

#[test]
fn canvas_canonicalizes_duplicate_owners_and_typed_pin_finger_ids() {
    let board = board();
    let cancel = CancellationToken::default();
    let hits = [
        SelectedObject::Pin(ObjectId(11)),
        SelectedObject::Via(ObjectId(10)),
        SelectedObject::Pin(ObjectId(10)),
    ]
    .map(|object| ObjectHit {
        object,
        distance_mm: 0.0,
    });
    let result =
        resolve_canvas_candidates(&board, &hits, SelectionMode::Component, 64, &cancel).unwrap();
    assert_eq!(result.len(), 1);
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10)));
    assert_eq!(result[0].target, target);
    assert_eq!(
        target.members(&board, 256, &cancel).unwrap().objects,
        vec![
            SelectedObject::Pin(ObjectId(10)),
            SelectedObject::Pin(ObjectId(11)),
            SelectedObject::Via(ObjectId(10))
        ]
    );
    assert_eq!(target.summarize(&board, &cancel).unwrap().pins, 2);
    assert_eq!(target.summarize(&board, &cancel).unwrap().vias, 1);
    assert_eq!(target.component_objects(&board, &cancel).unwrap().len(), 3);
}

#[test]
fn finger_only_reference_selects_without_fabricated_owner_and_empty_reference_falls_back() {
    let board = board();
    let cancel = CancellationToken::default();
    for (object, target) in [
        (
            SelectedObject::Via(ObjectId(20)),
            SelectionTarget::ComponentGroup(ComponentAnchor::Finger(ObjectId(20))),
        ),
        (
            SelectedObject::Pin(ObjectId(13)),
            SelectionTarget::Object(SelectedObject::Pin(ObjectId(13))),
        ),
    ] {
        let hits = [ObjectHit {
            object,
            distance_mm: 0.0,
        }];
        assert_eq!(
            resolve_canvas_candidates(&board, &hits, SelectionMode::Component, 1, &cancel).unwrap()
                [0]
            .target,
            target
        );
    }
}

#[test]
fn grouped_bounds_and_legacy_restore_include_members_without_placement_positions() {
    let board = board();
    let cancel = CancellationToken::default();
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10)));
    assert_eq!(
        target.bounds(&board, &cancel).unwrap(),
        Some(Bounds {
            min: Point::new(0.5, -0.5),
            max: Point::new(9.5, 0.5)
        })
    );
    assert_eq!(
        SelectionTarget::Component(ObjectId(91))
            .canonical_reference(&board, &cancel)
            .unwrap(),
        target
    );
    let json = serde_json::to_string(&target).unwrap();
    let restored: SelectionTarget = serde_json::from_str(&json).unwrap();
    assert!(restored.exists(&board, &cancel).unwrap());
    assert_eq!(restored, target);
}

#[test]
fn pages_cover_full_group_and_gpu_members_are_not_limited_to_inspector_page() {
    let mut board = board();
    board.pins = (0..600)
        .map(|id| pin(1000 + id, 90, "U1", id as f64))
        .collect();
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(1000)));
    let cancel = CancellationToken::default();
    let mut objects = vec![];
    for offset in [0, 256, 512] {
        let page = target
            .members_page(&board, offset, usize::MAX, &cancel)
            .unwrap();
        assert_eq!(page.total, 601);
        objects.extend(page.objects);
    }
    assert_eq!(objects.len(), 601);
    assert_eq!(objects.last(), Some(&SelectedObject::Via(ObjectId(10))));
    assert_eq!(target.pin_ids(&board, &cancel).unwrap().len(), 600);
    assert_eq!(
        target.component_objects(&board, &cancel).unwrap().len(),
        601
    );
}

#[test]
fn stale_typed_anchor_and_cancellation_do_not_publish_partial_group_results() {
    let board = board();
    let cancel = CancellationToken::default();
    let missing = SelectionTarget::ComponentGroup(ComponentAnchor::Finger(ObjectId(999)));
    assert!(matches!(
        missing.members(&board, 256, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(999)))
    ));
    cancel.cancel();
    let target = SelectionTarget::ComponentGroup(ComponentAnchor::Pin(ObjectId(10)));
    assert!(matches!(
        target.component_objects(&board, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
    assert!(SearchIndex::build(&board, &cancel).unwrap().is_none());
}
