use std::collections::BTreeMap;

use pomelo_core::{
    model::{BoardScene, Bounds, ComponentPlacement, LayerId, NetId, ObjectId, Point, Segment},
    search::{SearchIndex, SearchTarget},
    task::CancellationToken,
};

fn scene() -> BoardScene {
    BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: BTreeMap::from([(NetId(7), "電源_A".into()), (NetId(99), "unused".into())]),
        segments: [7, 0, 8, 7]
            .into_iter()
            .enumerate()
            .map(|(id, net)| Segment {
                id: ObjectId(id as u32),
                track_id: ObjectId(10),
                layer: LayerId(1),
                net: NetId(net),
                a: Point::new(0.0, 0.0),
                b: Point::new(1.0, 0.0),
                width: 0.1,
                arc: None,
                bond_wire: None,
            })
            .collect(),
        components: [30, 31]
            .into_iter()
            .map(|id| ComponentPlacement {
                id: ObjectId(id),
                source_reference: None,
                reference: "U1".into(),
                at: Point::new(id as f64, 0.0),
                angle: 0.0,
                mirrored: false,
                pins: vec![ObjectId(id + 100)],
            })
            .collect(),
        pins: vec![],
        vias: vec![],
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        diagnostics: vec![],
        bounds: Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(40.0, 1.0),
        },
    }
}

#[test]
fn canvas_backdrill_picking_uses_cut_scope_and_independent_switch_with_base_fallback() {
    use pomelo_core::{
        display::{BoardDisplay, LayerPrimitives},
        model::{Backdrill, BackdrillDefinition, BackdrillSpan, DrillShape, Pad, Via},
        picking::{PickFilter, PickQuery},
        picking_index::SegmentIndex,
        selection::SelectedObject,
    };
    let mut board = scene();
    board.segments.clear();
    let mut base = Pad::circle(LayerId(1), 8.0);
    base.backdrill_base = true;
    let mut marker = base.clone();
    marker.backdrill_base = false;
    marker.backdrill = true;
    board.vias.push(Via {
        id: ObjectId(42),
        net: NetId(7),
        at: Point::default(),
        drill: 2.0,
        drill_shape: DrillShape {
            width: 2.0,
            height: 2.0,
            plated: true,
        },
        padstack: ObjectId(43),
        padstack_name: String::new(),
        start_layer: Some(LayerId(0)),
        end_layer: Some(LayerId(1)),
        pads: std::sync::Arc::from([Pad::circle(LayerId(0), 6.0), base, marker]),
        backdrill: Some(Backdrill {
            definition: BackdrillDefinition {
                spans: vec![BackdrillSpan {
                    start_layer: LayerId(1),
                    stop_layer: LayerId(1),
                    protected_layer: LayerId(0),
                }],
                display_diameter: 8.0,
                start_pad_diameter: 8.0,
                label_diameter: 8.0,
            },
            source_reference: ObjectId(44),
            rotation_degrees: 330.0,
            mirrored: false,
        }),
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    });
    let cancel = CancellationToken::default();
    let index = SegmentIndex::build(std::sync::Arc::new(board), 0, &cancel).unwrap();
    for (back, drill, filled, layer, radius, expected) in [
        (true, false, true, 1, 0.0, true),
        (true, false, false, 1, 0.0, true),
        (true, false, true, 1, 3.5, true),
        (true, false, false, 1, 3.5, true),
        (true, false, true, 0, 3.5, false),
        (true, true, false, 0, 0.0, true),
        (false, false, true, 1, 3.5, true),
        (false, false, false, 1, 3.5, false),
        (false, false, false, 1, 0.0, false),
        (false, true, false, 1, 0.0, true),
    ] {
        let mut display = BoardDisplay {
            show_backdrills: back,
            show_drills: drill,
            filled,
            ..BoardDisplay::default()
        };
        display.hidden_layers.insert(LayerId(1 - layer));
        let hits = index
            .query_visible_objects(
                PickQuery::new(Point::new(radius, 0.0), 0.0).unwrap(),
                100.0,
                PickFilter::all(),
                &display,
                64,
                &cancel,
            )
            .unwrap();
        assert_eq!(
            hits.first().map(|hit| hit.object),
            expected.then_some(SelectedObject::Via(ObjectId(42))),
            "back={back} drill={drill} filled={filled} layer={layer} radius={radius}"
        );
    }
    let mut display = BoardDisplay::default();
    display.hidden_layers.insert(LayerId(0));
    display.layer_primitives.insert(
        LayerId(1),
        LayerPrimitives {
            vias: false,
            ..LayerPrimitives::default()
        },
    );
    assert!(
        index
            .query_visible_objects(
                PickQuery::new(Point::default(), 0.0).unwrap(),
                100.0,
                PickFilter::all(),
                &display,
                64,
                &cancel
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rotated_text_quads_pick_last_source_owner_and_respect_text_visibility() {
    use pomelo_core::{
        display::BoardDisplay,
        interaction::SelectionMode,
        model::{BoardDrawing, BoardText, TextAlignment},
        picking::{PickCategory, PickFilter, PickQuery},
        picking_index::{SegmentIndex, TextPickQuad},
        selection::{SelectedObject, resolve_canvas_candidates},
    };
    let mut board = scene();
    board.segments.clear();
    // Owner order deliberately differs from global text submission order.
    for (owner, text_id) in [(78, 11), (77, 10)] {
        board.drawings.push(BoardDrawing {
            id: ObjectId(owner),
            owner_id: None,
            layer: LayerId(1),
            net: NetId(0),
            graphic_ids: vec![],
            segments: vec![],
            text_ids: vec![ObjectId(text_id)],
        });
    }
    for id in [10, 11] {
        board.texts.push(BoardText {
            id: ObjectId(id),
            owner_id: None,
            layer: LayerId(1),
            class_id: 0,
            subclass: 0,
            text: "G".into(),
            at: Point::new(4.0, 1.0),
            angle: 0.7,
            mirrored: true,
            align: TextAlignment::Center,
            font_index: 0,
            width: 1.0,
            height: 1.0,
            spacing: 0.0,
            line_spacing: 1.0,
            stroke_width: 0.0,
        });
    }
    let corners = [
        Point::new(4.0, 0.0),
        Point::new(5.0, 1.0),
        Point::new(4.0, 2.0),
        Point::new(3.0, 1.0),
    ];
    let mut quads = vec![
        TextPickQuad {
            text: ObjectId(10),
            corners,
        },
        TextPickQuad {
            text: ObjectId(11),
            corners,
        },
        TextPickQuad {
            text: ObjectId(10),
            corners: [
                Point::new(20.0, 20.0),
                Point::new(21.0, 20.0),
                Point::new(21.0, 21.0),
                Point::new(20.0, 21.0),
            ],
        },
    ];
    quads.extend(std::iter::repeat_n(
        TextPickQuad {
            text: ObjectId(999),
            corners,
        },
        2_000,
    ));
    let cancel = CancellationToken::default();
    let board = std::sync::Arc::new(board);
    let index = SegmentIndex::build_with_budget(board.clone(), 10, 64 * 1024, &cancel)
        .unwrap()
        .with_text_quads(&quads, &cancel)
        .unwrap();
    let query = PickQuery::new(Point::new(4.0, 1.0), 0.05).unwrap();
    let mut display = BoardDisplay::default();
    assert!(
        index
            .query_visible_objects(query, 100.0, PickFilter::all(), &display, 64, &cancel)
            .unwrap()
            .is_empty()
    );
    display.show_texts = true;
    display.show_drawings = false;
    let hits = index
        .query_visible_objects(query, 100.0, PickFilter::all(), &display, 64, &cancel)
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.object).collect::<Vec<_>>(),
        [
            SelectedObject::Drawing(ObjectId(78)),
            SelectedObject::Drawing(ObjectId(77))
        ]
    );
    for mode in [
        SelectionMode::Object,
        SelectionMode::Track,
        SelectionMode::Net,
        SelectionMode::Component,
    ] {
        let resolved = resolve_canvas_candidates(&board, &hits, mode, 1, &cancel).unwrap();
        assert_eq!(
            resolved[0].target,
            pomelo_core::selection::SelectionTarget::Object(SelectedObject::Drawing(ObjectId(78)))
        );
    }
    let gap_edge = PickQuery::new(Point::new(5.052, 1.0), 0.05).unwrap();
    let edge_hits = index
        .query_visible_objects(gap_edge, 100.0, PickFilter::all(), &display, 64, &cancel)
        .unwrap();
    assert_eq!(
        edge_hits[0].object,
        SelectedObject::Drawing(ObjectId(77)),
        "whole-string bounds retain the half-pixel glyph edge in whitespace between separated glyphs"
    );
    let mut filter = PickFilter::all();
    filter.set(PickCategory::Drawing, false);
    assert!(
        index
            .query_visible_objects(query, 100.0, filter, &display, 64, &cancel)
            .unwrap()
            .is_empty()
    );
    display.hidden_layers.insert(LayerId(1));
    assert!(
        index
            .query_visible_objects(query, 100.0, PickFilter::all(), &display, 64, &cancel)
            .unwrap()
            .is_empty()
    );
    cancel.cancel();
    assert!(matches!(
        index.query_visible_objects(query, 100.0, PickFilter::all(), &display, 64, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn member_pages_cover_large_network_without_duplicates_or_unconnected_rows() {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    let mut board = scene();
    let prototype = board.segments[0].clone();
    board.segments = (0..1200)
        .map(|id| Segment {
            id: ObjectId(id),
            net: NetId(if id % 2 == 0 { 7 } else { 0 }),
            ..prototype.clone()
        })
        .collect();
    let cancel = CancellationToken::default();
    let target = SelectionTarget::Net(NetId(7));
    let mut collected = Vec::new();
    for (offset, expected_len) in [(0, 256), (256, 256), (512, 88), (768, 0)] {
        let page = target
            .members_page(&board, offset, usize::MAX, &cancel)
            .unwrap();
        assert_eq!(page.total, 600);
        assert_eq!(page.objects.len(), expected_len);
        collected.extend(page.objects);
    }
    let expected: Vec<_> = (0..1200)
        .step_by(2)
        .map(|id| SelectedObject::Segment(ObjectId(id)))
        .collect();
    assert_eq!(collected, expected);
    let empty = target.members_page(&board, 256, 0, &cancel).unwrap();
    assert_eq!(empty.total, 600);
    assert!(empty.objects.is_empty());
    cancel.cancel();
    assert!(matches!(
        target.members_page(&board, usize::MAX, 0, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn mixed_candidates_keep_category_identity_and_global_distance_order() {
    use pomelo_core::{
        copper::CopperMesh,
        display::BoardDisplay,
        model::{DrillShape, Pad, Pin, Via, Zone},
        picking::{PickFilter, PickQuery},
        picking_index::SegmentIndex,
        selection::SelectedObject,
    };
    let mut board = scene();
    let pad = Pad::circle(LayerId(1), 1.0);
    board.pins.push(Pin {
        id: ObjectId(0),
        owner_id: ObjectId(30),
        net: NetId(0),
        name: String::new(),
        reference: String::new(),
        at: Point::new(0.5, 0.0),
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        pads: vec![pad.clone()],
        stackup_region: None,
        die: None,
    });
    board.vias.push(Via {
        id: ObjectId(0),
        net: NetId(7),
        at: Point::new(0.5, 0.0),
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        padstack: ObjectId(9),
        padstack_name: String::new(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(1)),
        pads: vec![pad].into(),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    });
    board.zones.push(Zone {
        kind: pomelo_core::model::ZoneKind::Unknown,
        id: ObjectId(0),
        layer: LayerId(1),
        net: NetId(7),
        paths: vec![],
        mesh: CopperMesh {
            vertices: vec![
                Point::new(-1.0, -1.0),
                Point::new(2.0, -1.0),
                Point::new(2.0, 1.0),
                Point::new(-1.0, 1.0),
            ],
            ring_offsets: vec![0, 4],
            ..Default::default()
        },
    });
    let cancel = CancellationToken::default();
    board.components[0].pins = vec![ObjectId(0)];
    let board = std::sync::Arc::new(board);
    let index = SegmentIndex::build(board.clone(), 100, &cancel).unwrap();
    let display = BoardDisplay::default();
    let query = PickQuery::new(Point::new(0.5, 0.0), 1.0).unwrap();
    let hits = index
        .query_objects(query, PickFilter::all(), &display, 64, &cancel, false)
        .unwrap();
    assert_eq!(
        hits.iter().map(|h| h.object).collect::<Vec<_>>(),
        vec![
            SelectedObject::Pin(ObjectId(0)),
            SelectedObject::Via(ObjectId(0)),
            SelectedObject::Segment(ObjectId(0)),
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(2)),
            SelectedObject::Segment(ObjectId(3)),
            SelectedObject::Zone(ObjectId(0))
        ]
    );
    use pomelo_core::{
        interaction::SelectionMode,
        selection::{SelectionTarget, resolve_candidates},
    };
    // Display switches must remove hidden categories before candidate truncation,
    // while zones and same-ID objects from another category remain eligible.
    for mask in 0..8 {
        use pomelo_core::display::LayerPrimitive;
        let mut filtered = display.clone();
        for (bit, kind) in [
            LayerPrimitive::Traces,
            LayerPrimitive::Vias,
            LayerPrimitive::Pads,
        ]
        .into_iter()
        .enumerate()
        {
            filtered.set_primitive(LayerId(1), kind, mask & (1 << bit) != 0);
        }
        let expected: Vec<_> = hits
            .iter()
            .filter(|hit| match hit.object {
                SelectedObject::Segment(_) => mask & 1 != 0,
                SelectedObject::Via(_) => mask & 2 != 0,
                SelectedObject::Pin(_) => mask & 4 != 0,
                SelectedObject::Zone(_) | SelectedObject::Drawing(_) => true,
            })
            .map(|hit| hit.object)
            .collect();
        for limit in [1, 64] {
            let actual = index
                .query_objects(query, PickFilter::all(), &filtered, limit, &cancel, false)
                .unwrap();
            assert_eq!(
                actual.iter().map(|hit| hit.object).collect::<Vec<_>>(),
                expected.iter().copied().take(limit).collect::<Vec<_>>()
            );
        }
        assert_eq!(
            query
                .pins(&board, PickFilter::all(), &filtered, 64, &cancel)
                .unwrap()
                .len(),
            usize::from(mask & 4 != 0)
        );
        assert_eq!(
            query
                .vias_where(&board, PickFilter::all(), &filtered, 64, &cancel, |_| true)
                .unwrap()
                .len(),
            usize::from(mask & 2 != 0)
        );
        assert_eq!(
            query
                .segments(&board.segments, PickFilter::all(), &filtered, 64, &cancel)
                .unwrap()
                .len(),
            if mask & 1 != 0 { 4 } else { 0 }
        );
    }
    let nets = resolve_candidates(&board, &hits, SelectionMode::Net, 64, &cancel).unwrap();
    assert_eq!(
        nets.iter().map(|c| c.target).collect::<Vec<_>>(),
        vec![
            SelectionTarget::Net(NetId(7)),
            SelectionTarget::Net(NetId(8))
        ]
    );
    assert_eq!(nets[0].object, SelectedObject::Via(ObjectId(0)));
    let objects = resolve_candidates(&board, &hits, SelectionMode::Object, 64, &cancel).unwrap();
    assert_eq!(objects.len(), 7);
    let via_bounds = SelectionTarget::Object(SelectedObject::Via(ObjectId(0)))
        .bounds(&board, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(via_bounds.min, Point::new(0.0, -0.5));
    assert_eq!(via_bounds.max, Point::new(1.0, 0.5));
    assert_eq!(
        SelectionTarget::Object(SelectedObject::Pin(ObjectId(0)))
            .bounds(&board, &cancel)
            .unwrap(),
        Some(via_bounds)
    );
    let zone_bounds = SelectionTarget::Object(SelectedObject::Zone(ObjectId(0)))
        .bounds(&board, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(zone_bounds.min, Point::new(-1.0, -1.0));
    assert_eq!(zone_bounds.max, Point::new(2.0, 1.0));
    assert!(
        SelectionTarget::Track(ObjectId(10))
            .bounds(&board, &cancel)
            .unwrap()
            .is_some()
    );
    let mut inconsistent = scene();
    let mut bond_board = scene();
    bond_board.pins = board.pins.clone();
    bond_board.vias = board.vias.clone();
    bond_board.components[0].pins = vec![ObjectId(0)];
    bond_board.segments[0].bond_wire = Some(pomelo_core::model::BondWireInfo {
        profile: String::new(),
        material: None,
        source_pin: ObjectId(0),
        finger: ObjectId(0),
        reference: String::new(),
        pin_name: String::new(),
    });
    bond_board.vias[0].finger = Some(pomelo_core::model::BondFinger {
        reference: String::new(),
        name: String::new(),
        source_pin: Some(ObjectId(0)),
    });
    for object in [
        SelectedObject::Segment(ObjectId(0)),
        SelectedObject::Via(ObjectId(0)),
    ] {
        assert_eq!(
            object
                .resolve(&bond_board, SelectionMode::Component, &cancel)
                .unwrap(),
            Some(SelectionTarget::Component(ObjectId(30)))
        );
    }
    bond_board.vias[0].finger.as_mut().unwrap().source_pin = Some(ObjectId(u32::MAX));
    assert!(matches!(
        SelectedObject::Via(ObjectId(0)).resolve(&bond_board, SelectionMode::Component, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(
            u32::MAX
        )))
    ));
    bond_board.vias[0].finger.as_mut().unwrap().source_pin = Some(ObjectId(0));
    assert_eq!(
        SelectionTarget::Component(ObjectId(30))
            .related_bond_objects(&bond_board, &cancel)
            .unwrap(),
        vec![
            SelectedObject::Segment(ObjectId(0)),
            SelectedObject::Via(ObjectId(0))
        ]
    );
    assert!(
        SelectionTarget::Component(ObjectId(31))
            .related_bond_objects(&board, &cancel)
            .is_err()
    );
    inconsistent.pins = board.pins.clone();
    bond_board.segments[0].a = Point::new(-40.0, 0.0);
    bond_board.vias[0].at = Point::new(60.0, 0.0);
    let component_bounds = SelectionTarget::Component(ObjectId(30))
        .bounds(&bond_board, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(component_bounds.min.x, -40.05);
    assert_eq!(component_bounds.max.x, 60.5);
    bond_board.segments[0].width = f64::NAN;
    assert!(matches!(
        SelectionTarget::Component(ObjectId(30)).bounds(&bond_board, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(0)))
    ));
    inconsistent.components = board.components.clone();
    inconsistent.pins[0].owner_id = ObjectId(31);
    assert!(matches!(
        SelectionTarget::Component(ObjectId(30)).members(&inconsistent, 0, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(0)))
    ));
    let mut missing = scene();
    assert!(matches!(
        SelectionTarget::Component(ObjectId(30)).members(&missing, 1, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(130)))
    ));
    missing.components[0].pins.push(ObjectId(130));
    assert!(matches!(
        SelectionTarget::Component(ObjectId(30)).members(&missing, 1, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(130)))
    ));
    let members = SelectionTarget::Net(NetId(7))
        .members(&board, 2, &cancel)
        .unwrap();
    assert_eq!(members.total, 4);
    let remaining = SelectionTarget::Net(NetId(7))
        .members_page(&board, 2, 256, &cancel)
        .unwrap();
    let complete = SelectionTarget::Net(NetId(7))
        .members(&board, 256, &cancel)
        .unwrap();
    assert_eq!(remaining.total, complete.total);
    assert_eq!(remaining.objects, complete.objects[2..]);
    let beyond = SelectionTarget::Net(NetId(7))
        .members_page(&board, usize::MAX, 256, &cancel)
        .unwrap();
    assert_eq!(beyond.total, 4);
    assert!(beyond.objects.is_empty());
    assert!(matches!(
        SelectionTarget::Component(ObjectId(30)).members_page(&missing, usize::MAX, 0, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(130)))
    ));
    assert_eq!(
        members.objects,
        vec![
            SelectedObject::Segment(ObjectId(0)),
            SelectedObject::Segment(ObjectId(3))
        ]
    );
    assert!(
        SelectionTarget::Net(NetId(0))
            .members(&board, 256, &cancel)
            .unwrap()
            .objects
            .is_empty()
    );
    assert_eq!(
        SelectionTarget::Component(ObjectId(30))
            .members(&board, 256, &cancel)
            .unwrap()
            .objects,
        vec![SelectedObject::Pin(ObjectId(0))]
    );
    assert_eq!(
        SelectionTarget::Track(ObjectId(10))
            .members(&board, 0, &cancel)
            .unwrap()
            .total,
        4
    );
    assert_eq!(
        SelectionTarget::Object(SelectedObject::Via(ObjectId(0)))
            .members(&board, 256, &cancel)
            .unwrap()
            .objects,
        vec![SelectedObject::Via(ObjectId(0))]
    );
    assert_eq!(
        resolve_candidates(&board, &hits, SelectionMode::Component, 64, &cancel)
            .unwrap()
            .iter()
            .map(|c| c.target)
            .collect::<Vec<_>>(),
        vec![SelectionTarget::Component(ObjectId(30))]
    );
    assert_eq!(
        resolve_candidates(&board, &hits, SelectionMode::Track, 64, &cancel)
            .unwrap()
            .iter()
            .map(|c| c.target)
            .collect::<Vec<_>>(),
        vec![SelectionTarget::Track(ObjectId(10))]
    );
    assert_eq!(
        resolve_candidates(&board, &hits, SelectionMode::Net, 1, &cancel)
            .unwrap()
            .len(),
        1
    );
    let mut invalid_hits = hits.clone();
    invalid_hits.push(pomelo_core::picking::ObjectHit {
        object: SelectedObject::Zone(ObjectId(9999)),
        distance_mm: 0.0,
    });
    assert!(matches!(
        resolve_candidates(&board, &invalid_hits, SelectionMode::Net, 1, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(9999)))
    ));
    assert_eq!(
        index
            .query_objects(query, PickFilter::all(), &display, 1, &cancel, true)
            .unwrap()[0]
            .object,
        SelectedObject::Via(ObjectId(0))
    );
    // A nearer category wins over priority: the pin/via are outside their radius here.
    let away = PickQuery::new(Point::new(1.1, 0.0), 1.0).unwrap();
    assert_eq!(
        index
            .query_objects(away, PickFilter::all(), &display, 1, &cancel, false)
            .unwrap()[0]
            .object,
        SelectedObject::Zone(ObjectId(0))
    );
    cancel.cancel();
    assert!(matches!(
        SelectionTarget::Object(SelectedObject::Via(ObjectId(0))).bounds(&board, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
    assert!(matches!(
        SelectionTarget::Net(NetId(7)).members(&board, 256, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
    assert!(matches!(
        resolve_candidates(&board, &hits, SelectionMode::Net, 64, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
    assert!(matches!(
        index.query_objects(query, PickFilter::all(), &display, 0, &cancel, false),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn zone_candidates_exclude_holes_and_filter_before_truncation() {
    use pomelo_core::{
        copper::CopperMesh,
        display::BoardDisplay,
        model::Zone,
        picking::{PickCategory, PickFilter, PickQuery},
    };
    let mut board = scene();
    let mesh = CopperMesh {
        vertices: vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
            Point::new(4.0, 4.0),
            Point::new(6.0, 4.0),
            Point::new(6.0, 6.0),
            Point::new(4.0, 6.0),
        ],
        ring_offsets: vec![0, 4, 8],
        ..Default::default()
    };
    board.zones = (0..66)
        .map(|id| Zone {
            kind: pomelo_core::model::ZoneKind::Unknown,
            id: ObjectId(1000 + id),
            layer: LayerId(1),
            net: NetId(if id == 65 { 7 } else { 0 }),
            paths: vec![],
            mesh: mesh.clone(),
        })
        .collect();
    let cancel = CancellationToken::default();
    let mut display = BoardDisplay::default();
    let filter = PickFilter::all();
    let query = PickQuery::new(Point::new(2.0, 2.0), 0.5).unwrap();
    let hits = query
        .zones_where(&board, filter, &display, usize::MAX, &cancel, |_| true)
        .unwrap();
    assert_eq!(hits.len(), 64);
    assert_eq!(hits[0].zone.id, ObjectId(1000));
    assert_eq!(hits[63].zone.id, ObjectId(1063));
    display.show_copper = false;
    assert!(
        query
            .zones_where(&board, filter, &display, usize::MAX, &cancel, |_| true)
            .unwrap()
            .is_empty()
    );
    display.show_copper = true;
    assert_eq!(
        query
            .zones_where(&board, filter, &display, 1, &cancel, |z| z.net != NetId(0))
            .unwrap()[0]
            .zone
            .id,
        ObjectId(1065)
    );
    for point in [
        Point::new(5.0, 5.0),
        Point::new(4.0, 5.0),
        Point::new(-0.1, 2.0),
    ] {
        assert!(
            PickQuery::new(point, 0.5)
                .unwrap()
                .zones_where(&board, filter, &display, 1, &cancel, |_| true)
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(
        PickQuery::new(Point::new(0.0, 2.0), 0.0)
            .unwrap()
            .zones_where(&board, filter, &display, 1, &cancel, |_| true)
            .unwrap()
            .len(),
        1
    );
    display.hidden_layers.insert(LayerId(1));
    assert!(
        query
            .zones_where(&board, filter, &display, 1, &cancel, |_| true)
            .unwrap()
            .is_empty()
    );
    display.hidden_layers.clear();
    let mut disabled = filter;
    disabled.set(PickCategory::Zone, false);
    assert!(
        query
            .zones_where(&board, disabled, &display, 1, &cancel, |_| true)
            .unwrap()
            .is_empty()
    );
    let mut indexed_scene = scene();
    indexed_scene.segments.clear();
    indexed_scene.zones = board.zones.clone();
    // Add a second overlapping hole to the indexed and reference fixtures.
    for zone in &mut indexed_scene.zones {
        zone.mesh.vertices.extend([
            Point::new(5.0, 5.0),
            Point::new(7.0, 5.0),
            Point::new(7.0, 7.0),
            Point::new(5.0, 7.0),
        ]);
        zone.mesh.ring_offsets.push(12);
    }
    board.zones = indexed_scene.zones.clone();
    let indexed_scene = std::sync::Arc::new(indexed_scene);
    let index =
        pomelo_core::picking_index::SegmentIndex::build(indexed_scene.clone(), 100, &cancel)
            .unwrap();
    for x in [
        -1.0, 0.0, 2.0, 4.0, 5.0, 5.5, 6.0, 6.5, 7.0, 8.0, 10.0, 11.0,
    ] {
        for y in [
            -1.0, 0.0, 2.0, 4.0, 5.0, 5.5, 6.0, 6.5, 7.0, 8.0, 10.0, 11.0,
        ] {
            let query = PickQuery::new(Point::new(x, y), 0.5).unwrap();
            for connected in [false, true] {
                let reference = query
                    .zones_where(&board, filter, &display, 20, &cancel, |z| {
                        !connected || z.net != NetId(0)
                    })
                    .unwrap();
                let actual = index
                    .query_objects(query, filter, &display, 20, &cancel, connected)
                    .unwrap();
                assert_eq!(
                    actual.iter().map(|h| h.object).collect::<Vec<_>>(),
                    reference
                        .iter()
                        .map(|h| pomelo_core::selection::SelectedObject::Zone(h.zone.id))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    display.hidden_layers.insert(LayerId(1));
    assert!(
        index
            .query_objects(query, filter, &display, 20, &cancel, false)
            .unwrap()
            .is_empty()
    );
    display.hidden_layers.clear();
    assert!(
        index
            .query_objects(query, disabled, &display, 20, &cancel, false)
            .unwrap()
            .is_empty()
    );
    for point in [
        Point::new(5.5, 5.5),
        Point::new(6.5, 6.5),
        Point::new(7.0, 6.0),
    ] {
        assert!(
            index
                .query_objects(
                    PickQuery::new(point, 0.5).unwrap(),
                    filter,
                    &display,
                    20,
                    &cancel,
                    false
                )
                .unwrap()
                .is_empty()
        );
    }
    assert!(matches!(
        pomelo_core::picking_index::SegmentIndex::build_with_budget(
            indexed_scene,
            100,
            100,
            &cancel
        ),
        Err(pomelo_core::picking_index::IndexError::ByteLimit { .. })
    ));
    board.zones[65].mesh.ring_offsets = vec![0, 999];
    assert!(matches!(
        query.zones_where(&board, filter, &display, 1, &cancel, |_| true),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(1065)))
    ));
    let mut malformed = scene();
    malformed.segments.clear();
    malformed.zones = board.zones.clone();
    assert!(matches!(
        pomelo_core::picking_index::SegmentIndex::build(
            std::sync::Arc::new(malformed),
            100,
            &cancel
        ),
        Err(pomelo_core::picking_index::IndexError::Geometry(
            pomelo_core::geometry::PathError::Invalid(ObjectId(1065))
        ))
    ));
    cancel.cancel();
    assert!(matches!(
        query.zones_where(&board, filter, &display, 0, &cancel, |_| true),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn via_candidates_follow_copper_layers_and_ignore_drill_visibility() {
    use pomelo_core::{
        display::BoardDisplay,
        model::{DrillShape, Pad, PadKind, Via},
        picking::{PickFilter, PickQuery},
    };
    let mut board = scene();
    let mut pad = Pad::circle(LayerId(1), 4.0);
    pad.kind = PadKind::DONUT;
    pad.inner_diameter = Some(2.0);
    let mut second = pad.clone();
    second.layer = LayerId(2);
    board.vias.push(Via {
        id: ObjectId(900),
        net: NetId(7),
        at: Point::new(10.0, 10.0),
        drill: 1.0,
        drill_shape: DrillShape {
            width: 1.0,
            height: 1.0,
            plated: true,
        },
        padstack: ObjectId(901),
        padstack_name: "source".into(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(2)),
        pads: std::sync::Arc::from(vec![pad, second]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    });
    let cancel = CancellationToken::default();
    let mut display = BoardDisplay {
        show_drills: false,
        ..BoardDisplay::default()
    };
    let rim = PickQuery::new(Point::new(11.5, 10.0), 0.0).unwrap();
    let hits = rim
        .vias_where(&board, PickFilter::all(), &display, 20, &cancel, |_| true)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].via.id, ObjectId(900));
    assert_eq!(hits[0].distance_mm, 0.0);
    assert!(
        PickQuery::new(Point::new(10.0, 10.0), 0.0)
            .unwrap()
            .vias_where(&board, PickFilter::all(), &display, 20, &cancel, |_| true)
            .unwrap()
            .is_empty()
    );
    display.hidden_layers.insert(LayerId(1));
    assert_eq!(
        rim.vias_where(&board, PickFilter::all(), &display, 20, &cancel, |_| true)
            .unwrap()
            .len(),
        1
    );
    display.hidden_layers.insert(LayerId(2));
    assert!(
        rim.vias_where(&board, PickFilter::all(), &display, 20, &cancel, |_| true)
            .unwrap()
            .is_empty()
    );
    cancel.cancel();
    assert!(matches!(
        rim.vias_where(&board, PickFilter::all(), &display, 20, &cancel, |_| true),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn mode_eligibility_is_applied_before_the_candidate_limit() {
    use pomelo_core::{
        picking::{PickFilter, PickQuery},
        picking_index::SegmentIndex,
    };
    let mut board = scene();
    let mut connected = board.segments[0].clone();
    connected.id = ObjectId(65);
    board.segments = (0..65)
        .map(|index| {
            let mut segment = connected.clone();
            segment.id = ObjectId(index);
            segment.net = NetId(0);
            segment
        })
        .collect();
    board.segments.push(connected);
    let cancel = CancellationToken::default();
    let index = SegmentIndex::build(std::sync::Arc::new(board), 66, &cancel).unwrap();
    let hits = index
        .query_where(
            PickQuery::new(Point::new(0.5, 0.0), 0.15).unwrap(),
            PickFilter::all(),
            &pomelo_core::display::BoardDisplay::default(),
            1,
            &cancel,
            |segment| segment.net.0 != 0,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].segment.net, NetId(7));
}

#[test]
fn track_bounds_index_matches_reference_candidates_and_rejects_partial_builds() {
    use pomelo_core::{
        geometry::PathError,
        picking::{PickFilter, PickQuery},
        picking_index::SegmentIndex,
    };
    use std::sync::Arc;
    let mut board = scene();
    board.segments = (0..1025)
        .map(|index| {
            let mut segment = board.segments[0].clone();
            segment.id = ObjectId(index);
            segment.a = Point::new(f64::from(index % 41), f64::from(index / 41));
            segment.b = Point::new(segment.a.x + 2.0, segment.a.y);
            segment.layer = LayerId(index % 3);
            segment
        })
        .collect();
    let board = Arc::new(board);
    let cancel = CancellationToken::default();
    let index = SegmentIndex::build(board.clone(), 1025, &cancel).unwrap();
    assert!(matches!(
        SegmentIndex::build_with_budget(board.clone(), 1025, 0, &cancel),
        Err(pomelo_core::picking_index::IndexError::ByteLimit { actual, limit: 0 }) if actual > 0
    ));
    assert!(SegmentIndex::build_with_budget(board.clone(), 1025, 256 * 1024, &cancel).is_ok());
    let mut display = pomelo_core::display::BoardDisplay::default();
    display.hidden_layers.insert(LayerId(1));
    for x in [0.0, 1.0, 20.5, 40.5, 100.0] {
        for y in [0.0, 3.2, 24.9] {
            for limit in [0, 1, 20, 100] {
                let query = PickQuery::new(Point::new(x, y), 1.2).unwrap();
                let indexed = index
                    .query(query, PickFilter::all(), &display, limit, &cancel)
                    .unwrap();
                let reference = query
                    .segments(&board.segments, PickFilter::all(), &display, limit, &cancel)
                    .unwrap();
                assert_eq!(
                    indexed
                        .iter()
                        .map(|hit| (hit.segment.id, hit.distance_mm))
                        .collect::<Vec<_>>(),
                    reference
                        .iter()
                        .map(|hit| (hit.segment.id, hit.distance_mm))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    assert!(matches!(
        SegmentIndex::build(board.clone(), 1024, &cancel),
        Err(pomelo_core::picking_index::IndexError::Geometry(
            PathError::PointLimit {
                actual: 1025,
                limit: 1024
            }
        ))
    ));
    cancel.cancel();
    assert!(matches!(
        index.query(
            PickQuery::new(Point::default(), 1.0).unwrap(),
            PickFilter::all(),
            &display,
            20,
            &cancel
        ),
        Err(PathError::Cancelled)
    ));
    assert!(matches!(
        SegmentIndex::build(board, 1025, &cancel),
        Err(pomelo_core::picking_index::IndexError::Geometry(
            PathError::Cancelled
        ))
    ));
    let mut empty = scene();
    empty.segments.clear();
    let empty = SegmentIndex::build(Arc::new(empty), 0, &CancellationToken::default()).unwrap();
    assert!(
        empty
            .query(
                PickQuery::new(Point::default(), 1.0).unwrap(),
                PickFilter::all(),
                &display,
                20,
                &CancellationToken::default()
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn scene_index_counts_used_nets_and_does_not_invent_search_groups_without_pins() {
    let scene = scene();
    let index = SearchIndex::build(&scene, &CancellationToken::default())
        .unwrap()
        .unwrap();
    let actual: Vec<_> = index
        .entries()
        .iter()
        .map(|entry| (entry.target, entry.name.as_str(), entry.count))
        .collect();
    assert_eq!(
        actual,
        vec![
            (SearchTarget::Net(NetId(7)), "電源_A", 2),
            (SearchTarget::Net(NetId(8)), "8", 1),
        ]
    );
    assert!(index.find("unused", 20).is_empty());
}

#[test]
fn hit_resolution_keeps_object_track_and_network_identities_distinct() {
    use pomelo_core::{
        geometry::PathError,
        interaction::SelectionMode,
        selection::{SelectedObject, SelectionTarget},
    };
    let board = scene();
    let cancel = CancellationToken::default();
    let hit = SelectedObject::Segment(ObjectId(0));
    assert_eq!(
        hit.resolve(&board, SelectionMode::Object, &cancel).unwrap(),
        Some(SelectionTarget::Object(hit))
    );
    assert_eq!(
        hit.resolve(&board, SelectionMode::Track, &cancel).unwrap(),
        Some(SelectionTarget::Track(ObjectId(10)))
    );
    assert_eq!(
        hit.resolve(&board, SelectionMode::Net, &cancel).unwrap(),
        Some(SelectionTarget::Net(NetId(7)))
    );
    assert_eq!(
        hit.resolve(&board, SelectionMode::Component, &cancel)
            .unwrap(),
        None
    );
    assert_eq!(
        SelectedObject::Segment(ObjectId(1))
            .resolve(&board, SelectionMode::Net, &cancel)
            .unwrap(),
        None
    );
    assert!(matches!(
        SelectedObject::Via(ObjectId(0)).resolve(&board, SelectionMode::Object, &cancel),
        Err(PathError::Invalid(ObjectId(0)))
    ));
    cancel.cancel();
    assert!(matches!(
        hit.resolve(&board, SelectionMode::Object, &cancel),
        Err(PathError::Cancelled)
    ));
}

#[test]
fn cancelled_scene_build_returns_no_index_including_for_an_empty_scene() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(SearchIndex::build(&scene(), &cancel).unwrap().is_none());
    let mut empty = scene();
    empty.segments.clear();
    empty.components.clear();
    assert!(SearchIndex::build(&empty, &cancel).unwrap().is_none());
}

#[test]
fn locating_uses_stroke_extent_and_exact_component_identity() {
    let mut scene = scene();
    for component in &mut scene.components {
        component.pins.clear();
    }
    let cancel = CancellationToken::default();
    let net = SearchTarget::Net(NetId(7))
        .bounds(&scene, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(net.min, Point::new(-0.05, -0.05));
    assert_eq!(net.max, Point::new(1.05, 0.05));
    for id in [30, 31] {
        let bounds = SearchTarget::Component(ObjectId(id))
            .bounds(&scene, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(bounds.center(), Point::new(id as f64, 0.0));
    }
    assert!(
        SearchTarget::Component(ObjectId(999))
            .bounds(&scene, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        SearchTarget::Net(NetId(99))
            .bounds(&scene, &cancel)
            .unwrap()
            .is_none()
    );
}

#[test]
fn locating_rejects_missing_component_pin_instead_of_publishing_anchor_only() {
    let scene = scene();
    assert!(matches!(
        SearchTarget::Component(ObjectId(30)).bounds(&scene, &CancellationToken::default()),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(130)))
    ));
}

#[test]
fn component_summary_rejects_missing_and_duplicate_pin_references() {
    use pomelo_core::{geometry::PathError, selection::SelectionTarget};
    let mut scene = scene();
    let target = SelectionTarget::Component(ObjectId(30));
    let cancel = CancellationToken::default();
    assert!(matches!(
        target.summarize(&scene, &cancel),
        Err(PathError::Invalid(ObjectId(130)))
    ));
    scene.components[0].pins.push(ObjectId(130));
    assert!(matches!(
        target.summarize(&scene, &cancel),
        Err(PathError::Invalid(ObjectId(130)))
    ));
    cancel.cancel();
    assert!(matches!(
        target.summarize(&scene, &cancel),
        Err(PathError::Cancelled)
    ));
}

#[test]
fn source_network_names_preserve_blank_values_and_missing_names_use_stable_ids() {
    let mut scene = scene();
    scene.nets.insert(NetId(7), " \t ".into());
    scene.nets.remove(&NetId(8));
    let cancel = CancellationToken::default();
    let index = SearchIndex::build(&scene, &cancel).unwrap().unwrap();
    assert_eq!(index.entries()[0].name, " \t ");
    assert_eq!(index.find("8", 20)[0].target, SearchTarget::Net(NetId(8)));
    scene.nets.insert(NetId(8), String::new());
    assert_eq!(
        SearchIndex::build(&scene, &cancel)
            .unwrap()
            .unwrap()
            .entries()[1]
            .name,
        ""
    );
    assert!(
        SearchTarget::Net(NetId(0))
            .bounds(&scene, &cancel)
            .unwrap()
            .is_none()
    );
    cancel.cancel();
    assert!(matches!(
        SearchTarget::Net(NetId(0)).bounds(&scene, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn locating_component_includes_rotated_offset_pad_and_physical_drill() {
    use pomelo_core::model::{DrillShape, Pad, PadKind, Pin};
    let mut scene = scene();
    let component = &mut scene.components[0];
    component.pins = vec![ObjectId(130)];
    scene.pins.push(Pin {
        id: ObjectId(130),
        owner_id: component.id,
        net: NetId(7),
        name: "1".into(),
        reference: component.reference.clone(),
        at: Point::new(30.0, 0.0),
        angle: std::f64::consts::FRAC_PI_2,
        mirrored: false,
        drill: 3.0,
        drill_shape: DrillShape {
            width: 3.0,
            height: 3.0,
            plated: true,
        },
        pads: vec![Pad {
            layer: LayerId(1),
            width: 4.0,
            height: 2.0,
            offset: Point::new(5.0, 0.0),
            kind: PadKind(5),
            corner: 0.0,
            inner_diameter: None,
            custom: None,
            backdrill: false,
            backdrill_base: false,
        }],
        stackup_region: None,
        die: None,
    });
    {
        use pomelo_core::{
            display::BoardDisplay,
            picking::{PickFilter, PickQuery},
        };
        let query = PickQuery::new(Point::new(35.0, 0.0), 0.0).unwrap();
        let mut display = BoardDisplay::default();
        let cancel = CancellationToken::default();
        let hits = query
            .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].pin.id, ObjectId(130));
        assert_eq!(hits[0].distance_mm, 0.0);
        assert!(
            PickQuery::new(Point::new(30.0, 0.0), 0.0)
                .unwrap()
                .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .is_empty()
        );
        display.hidden_layers.insert(LayerId(1));
        assert!(
            query
                .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .is_empty()
        );
        cancel.cancel();
        assert!(matches!(
            query.analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel),
            Err(pomelo_core::geometry::PathError::Cancelled)
        ));
        let cancel = CancellationToken::default();
        let display = BoardDisplay::default();
        let original_pads = scene.pins[0].pads.clone();
        let pad = &mut scene.pins[0].pads[0];
        pad.kind = PadKind::DONUT;
        pad.width = 4.0;
        pad.height = 4.0;
        pad.inner_diameter = Some(2.0);
        let mut second_layer = pad.clone();
        second_layer.layer = LayerId(2);
        scene.pins[0].pads.push(second_layer);
        assert!(
            query
                .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .is_empty(),
            "donut opening is not copper coverage"
        );
        assert_eq!(
            PickQuery::new(Point::new(36.5, 0.0), 0.0)
                .unwrap()
                .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .len(),
            1,
            "multiple layers must not duplicate a pin candidate"
        );
        scene.pins[0].pads[0].inner_diameter = Some(f64::NAN);
        assert!(matches!(
            query.analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel),
            Err(pomelo_core::geometry::PathError::Invalid(ObjectId(130)))
        ));
        scene.pins[0].pads = original_pads.clone();
        scene.pins[0].pads[0].kind = PadKind::CUSTOM;
        scene.pins[0].pads[0].custom =
            Some(std::sync::Arc::new(pomelo_core::model::CustomPadGeometry {
                contours: vec![
                    vec![
                        Point::new(-1.0, -1.0),
                        Point::new(1.0, -1.0),
                        Point::new(1.0, 1.0),
                        Point::new(-1.0, 1.0),
                    ],
                    vec![
                        Point::new(-0.25, -0.25),
                        Point::new(0.25, -0.25),
                        Point::new(0.25, 0.25),
                        Point::new(-0.25, 0.25),
                    ],
                ],
                paths: vec![],
            }));
        assert!(
            query
                .pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .is_empty(),
            "custom hole remains unpicked"
        );
        let fill_query = PickQuery::new(Point::new(35.5, 0.0), 0.0).unwrap();
        assert_eq!(
            fill_query
                .pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()[0]
                .pin
                .id,
            ObjectId(130)
        );
        assert!(
            fill_query
                .analytic_pins(&scene, PickFilter::all(), &display, 20, &cancel)
                .unwrap()
                .is_empty()
        );
        let mut dense = crate::scene();
        let connected = scene.pins[0].clone();
        dense.pins = (0..65)
            .map(|index| {
                let mut pin = connected.clone();
                pin.id = ObjectId(200 + index);
                pin.net = NetId(0);
                pin
            })
            .collect();
        dense.pins.push(connected);
        let eligible = fill_query
            .pins_where(&dense, PickFilter::all(), &display, 1, &cancel, |pin| {
                pin.net.0 != 0
            })
            .unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].pin.id, ObjectId(130));
        scene.pins[0].pads = original_pads;
    }
    let bounds = SearchTarget::Component(ObjectId(30))
        .bounds(&scene, &CancellationToken::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        pomelo_core::selection::SelectedObject::Pin(ObjectId(130))
            .resolve(
                &scene,
                pomelo_core::interaction::SelectionMode::Component,
                &CancellationToken::default()
            )
            .unwrap(),
        Some(pomelo_core::selection::SelectionTarget::Component(
            ObjectId(30)
        ))
    );
    assert_eq!(
        pomelo_core::selection::SelectionTarget::Component(ObjectId(30))
            .summarize(&scene, &CancellationToken::default())
            .unwrap()
            .pins,
        1
    );
    assert!((bounds.min.x - 28.5).abs() < 1e-12);
    assert!((bounds.max.x - 36.0).abs() < 1e-12);
    assert!((bounds.min.y + 2.0).abs() < 1e-12);
    assert!((bounds.max.y - 2.0).abs() < 1e-12);
    let net = SearchTarget::Net(NetId(7))
        .bounds(&scene, &CancellationToken::default())
        .unwrap()
        .unwrap();
    assert!((net.max.x - bounds.max.x).abs() < 1e-12);
    assert!(net.min.x < bounds.min.x);
}

#[test]
fn selection_summary_uses_centerlines_and_keeps_net_and_track_scopes_distinct() {
    use pomelo_core::selection::SelectionTarget;
    let mut scene = scene();
    scene.segments[0].arc = Some(pomelo_core::model::Arc {
        center: Point::new(0.0, 0.0),
        radius: 2.0,
        start: 0.0,
        sweep: -std::f64::consts::FRAC_PI_2,
    });
    let cancel = CancellationToken::default();
    let net = SelectionTarget::Net(NetId(7))
        .summarize(&scene, &cancel)
        .unwrap();
    assert_eq!(net.segments, 2);
    assert!((net.centerline_length_mm - (std::f64::consts::PI + 1.0)).abs() < 1e-12);
    let track = SelectionTarget::Track(ObjectId(10))
        .summarize(&scene, &cancel)
        .unwrap();
    assert_eq!(track.segments, 4);
    assert!((track.centerline_length_mm - (std::f64::consts::PI + 3.0)).abs() < 1e-12);
    assert_eq!(
        SelectionTarget::Net(NetId(0))
            .summarize(&scene, &cancel)
            .unwrap()
            .segments,
        0
    );
    scene.segments[0].arc.as_mut().unwrap().radius = f64::NAN;
    assert!(matches!(
        SelectionTarget::Net(NetId(7)).summarize(&scene, &cancel),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(0)))
    ));
    cancel.cancel();
    assert!(matches!(
        SelectionTarget::Net(NetId(7)).summarize(&scene, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}

#[test]
fn locating_rejects_invalid_geometry_and_cancelled_partial_results() {
    let mut scene = scene();
    scene.segments[0].a.x = f64::NAN;
    assert!(matches!(
        SearchTarget::Net(NetId(7)).bounds(&scene, &CancellationToken::default()),
        Err(pomelo_core::geometry::PathError::Invalid(ObjectId(0)))
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        SearchTarget::Component(ObjectId(999)).bounds(&scene, &cancel),
        Err(pomelo_core::geometry::PathError::Cancelled)
    ));
}
