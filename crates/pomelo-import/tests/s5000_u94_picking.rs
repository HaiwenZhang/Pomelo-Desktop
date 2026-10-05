//! Opt-in, bounded production canvas-picking checks over a cached real-source subset.
//! Explicit env paths are required; ordinary CI never reads local board/cache files.

use pomelo_core::{
    copper::CopperMesh,
    display::{BoardDisplay, DisplayCategory, LayerPrimitive},
    model::{
        BoardScene, Bounds, ComponentPlacement, Layer, LayerFunction, LayerId, NetId, ObjectId,
        Pin, Point, Segment, Via, Zone, ZoneKind,
    },
    picking::{PickCategory, PickFilter, PickQuery},
    picking_index::{SegmentIndex, SelectionAnchor},
    selection::SelectedObject,
    task::CancellationToken,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
struct Target {
    source: Value,
    objects: Objects,
}

#[derive(Deserialize)]
struct Objects {
    pins: Vec<Pin>,
    vias: Vec<Via>,
    traces: Vec<Segment>,
    components: Vec<ComponentPlacement>,
    zones: Vec<SourceZone>,
}

#[derive(Deserialize)]
struct SourceZone {
    id: ObjectId,
    layer: LayerId,
    net: NetId,
    kind: String,
    paths: Vec<Vec<Segment>>,
}

struct Case {
    name: &'static str,
    point: Point,
    filter: PickFilter,
    display: BoardDisplay,
    first: Option<SelectionAnchor>,
    exact_order: Option<Vec<SelectedObject>>,
    contains: Vec<SelectedObject>,
    excludes: Vec<SelectedObject>,
}

fn zone(id: u32) -> SelectedObject {
    SelectedObject::Zone(ObjectId(id))
}
fn pin(id: u32) -> SelectedObject {
    SelectedObject::Pin(ObjectId(id))
}
fn via(id: u32) -> SelectedObject {
    SelectedObject::Via(ObjectId(id))
}
fn segment(id: u32) -> SelectedObject {
    SelectedObject::Segment(ObjectId(id))
}
fn only(category: PickCategory) -> PickFilter {
    let mut filter = PickFilter::none();
    filter.set(category, true);
    filter
}
fn anchor(object: SelectedObject, category: DisplayCategory) -> SelectionAnchor {
    SelectionAnchor {
        object,
        layer: if category == DisplayCategory::Drill {
            LayerId::UNASSIGNED
        } else {
            LayerId(0)
        },
        category,
    }
}
fn case(name: &'static str, point: Point, first: Option<SelectionAnchor>) -> Case {
    let mut display = BoardDisplay::default();
    // The source vias span all 14 physical layers; this probe matches the
    // controlled TOP-only viewport rather than inferring visibility from a
    // truncated scene layer list.
    display.hidden_layers.extend((1..14).map(LayerId));
    Case {
        name,
        point,
        filter: PickFilter::all(),
        display,
        first,
        exact_order: None,
        contains: Vec::new(),
        excludes: Vec::new(),
    }
}

#[test]
#[ignore = "requires explicit POMELO_S5000_U94_TARGET and POMELO_S5000_U94_PICK_REPORT env paths"]
fn real_u94_subset_uses_production_canvas_picking_and_filters() {
    let input = PathBuf::from(std::env::var_os("POMELO_S5000_U94_TARGET").expect("target env"));
    let output =
        PathBuf::from(std::env::var_os("POMELO_S5000_U94_PICK_REPORT").expect("report env"));
    assert_ne!(input, output, "report must not overwrite input evidence");
    let mut bytes = Vec::new();
    File::open(&input)
        .expect("open explicitly selected target")
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("read bounded target");
    assert!(bytes.len() <= 256 * 1024, "target exceeds 256 KiB bound");
    let input_sha256 = hex::encode(Sha256::digest(&bytes));
    let target: Target = serde_json::from_slice(&bytes).expect("cached target schema");
    assert_eq!(
        target.source["sha256"],
        "09c3e6f170ead979f4f8537ab27bc50f04021855b2c8cde117e5b4935aa393c7"
    );
    assert_eq!(target.source["bytes"], 303_867_712);
    assert_eq!(target.objects.zones.len(), 2);
    assert!(target.objects.pins.len() <= 3 && target.objects.vias.len() == 2);
    assert_eq!(target.objects.traces.len(), 2);
    let zones = target
        .objects
        .zones
        .into_iter()
        .map(|source| Zone {
            id: source.id,
            layer: source.layer,
            net: source.net,
            kind: match source.kind.to_ascii_lowercase().as_str() {
                "static" => ZoneKind::Static,
                "dynamic" => ZoneKind::Dynamic,
                other => panic!("unexpected source kind {other}"),
            },
            paths: source.paths,
            // query_visible_hits uses the preserved exact paths. This probe does
            // not recreate imported GPU meshes or exercise legacy mesh-only picks.
            mesh: CopperMesh::default(),
        })
        .collect();
    let scene = Arc::new(BoardScene {
        layers: vec![Layer {
            id: LayerId(0),
            name: "TOP".into(),
            function: LayerFunction::Conductor,
            color: "#58b5ed".into(),
            source_flags: None,
        }],
        special_layers: Vec::new(),
        nets: BTreeMap::from([(NetId(64836), "GND".into()), (NetId(64953), String::new())]),
        segments: target.objects.traces,
        pins: target.objects.pins,
        components: target.objects.components,
        vias: target.objects.vias,
        zones,
        outline: Vec::new(),
        texts: Vec::new(),
        drawing_layers: Vec::new(),
        drawings: Vec::new(),
        bounds: Bounds {
            min: Point::new(15.0, 72.0),
            max: Point::new(18.5, 76.0),
        },
        diagnostics: Vec::new(),
    });
    let cancel = CancellationToken::default();
    let index = SegmentIndex::build_with_budget(Arc::clone(&scene), 16, 4 * 1024 * 1024, &cancel)
        .expect("bounded production index");
    let hole = Point::new(16.9801794, 75.1774214);
    let gap = Point::new(17.265, 75.1774214);
    let fill = Point::new(16.98, 75.60);
    let overlap = Point::new(15.8, 73.5);
    let pin40 = scene
        .pins
        .iter()
        .find(|p| p.id == ObjectId(834394))
        .unwrap()
        .at;
    let via1 = scene
        .vias
        .iter()
        .find(|v| v.id == ObjectId(1332220))
        .unwrap()
        .at;
    let via2 = scene
        .vias
        .iter()
        .find(|v| v.id == ObjectId(1656947))
        .unwrap()
        .at;
    let short = scene
        .segments
        .iter()
        .find(|s| s.id == ObjectId(4242052))
        .unwrap();
    let long = scene
        .segments
        .iter()
        .find(|s| s.id == ObjectId(4241766))
        .unwrap();
    let short_point = Point::new((short.a.x + short.b.x) / 2.0, (short.a.y + short.b.y) / 2.0);
    let long_point = Point::new(
        long.a.x * 0.9 + long.b.x * 0.1,
        long.a.y * 0.9 + long.b.y * 0.1,
    );
    let mut cases = Vec::new();
    for (name, point) in [
        ("closed_void_center_zones", hole),
        ("closed_void_empty_clearance_all", gap),
    ] {
        let mut c = case(name, point, None);
        if point == hole {
            c.filter = only(PickCategory::Zone);
        }
        c.exact_order = Some(Vec::new());
        cases.push(c);
    }
    let mut c = case(
        "outside_void_dynamic_fill",
        fill,
        Some(anchor(zone(5598600), DisplayCategory::Zone)),
    );
    c.filter = only(PickCategory::Zone);
    c.exact_order = Some(vec![zone(5598600)]);
    cases.push(c);
    for solid in [false, true] {
        let mut c = case(
            if solid {
                "static_dynamic_overlap_solid"
            } else {
                "static_dynamic_overlap_stipple"
            },
            overlap,
            Some(anchor(zone(5598600), DisplayCategory::Zone)),
        );
        c.filter = only(PickCategory::Zone);
        c.display.static_shapes_fill_solid = solid;
        c.exact_order = Some(vec![zone(5598600), zone(933674)]);
        cases.push(c);
    }
    let mut c = case(
        "pin41_inside_zone_void",
        hole,
        Some(anchor(pin(901969), DisplayCategory::Pin)),
    );
    c.excludes = vec![zone(5598600), zone(933674)];
    cases.push(c);
    let mut c = case(
        "pin40_overlapping_two_zones",
        pin40,
        Some(anchor(pin(834394), DisplayCategory::Pin)),
    );
    c.contains = vec![zone(5598600), zone(933674)];
    cases.push(c);
    for (name, point, id) in [
        ("via1332220_finished_drill_center", via1, 1332220),
        ("via1656947_finished_drill_center", via2, 1656947),
    ] {
        let mut c = case(name, point, Some(anchor(via(id), DisplayCategory::Drill)));
        c.contains = vec![zone(5598600), zone(933674)];
        cases.push(c);
    }
    cases.push(case(
        "via1332220_annulus",
        Point::new(via1.x + 0.17, via1.y),
        Some(anchor(via(1332220), DisplayCategory::Via)),
    ));
    for (name, point, id) in [
        ("short_trace_category_only", short_point, 4242052),
        ("long_trace_category_only", long_point, 4241766),
    ] {
        let mut c = case(
            name,
            point,
            Some(anchor(segment(id), DisplayCategory::Trace)),
        );
        c.filter = only(PickCategory::Segment);
        c.exact_order = Some(vec![segment(id)]);
        cases.push(c);
    }
    cases.push(case(
        "long_trace_topmost_all_categories",
        long_point,
        Some(anchor(segment(4241766), DisplayCategory::Trace)),
    ));
    for (name, point, object, category) in [
        (
            "zero_global_alpha_pin",
            hole,
            pin(901969),
            DisplayCategory::Pin,
        ),
        (
            "zero_global_alpha_via_drill",
            via1,
            via(1332220),
            DisplayCategory::Drill,
        ),
    ] {
        let mut c = case(name, point, Some(anchor(object, category)));
        c.display.global_opacity = 0.0;
        cases.push(c);
    }
    let mut c = case("hidden_TOP_excludes_all", pin40, None);
    c.display.hidden_layers.insert(LayerId(0));
    c.exact_order = Some(Vec::new());
    cases.push(c);
    let mut c = case("empty_pick_category_filter", via1, None);
    c.filter = PickFilter::none();
    c.exact_order = Some(Vec::new());
    cases.push(c);
    for (name, point, category, primitive) in [
        (
            "hidden_pad_primitive",
            pin40,
            PickCategory::Pin,
            LayerPrimitive::Pads,
        ),
        (
            "hidden_via_primitive",
            via1,
            PickCategory::Via,
            LayerPrimitive::Vias,
        ),
        (
            "hidden_trace_primitive",
            short_point,
            PickCategory::Segment,
            LayerPrimitive::Traces,
        ),
    ] {
        let mut c = case(name, point, None);
        c.filter = only(category);
        c.display.set_primitive(LayerId(0), primitive, false);
        c.exact_order = Some(Vec::new());
        cases.push(c);
    }
    let mut c = case("hidden_copper_excludes_zone", overlap, None);
    c.filter = only(PickCategory::Zone);
    c.display.show_copper = false;
    c.exact_order = Some(Vec::new());
    cases.push(c);
    let mut c = case("zero_copper_alpha_fill_is_not_outline", fill, None);
    c.filter = only(PickCategory::Zone);
    c.display.copper_opacity = 0.0;
    c.exact_order = Some(Vec::new());
    cases.push(c);
    let mut c = case(
        "zero_copper_alpha_void_boundary_pick",
        Point::new(16.98, 74.9003836),
        Some(anchor(zone(5598600), DisplayCategory::ZoneOutline)),
    );
    c.filter = only(PickCategory::Zone);
    c.display.copper_opacity = 0.0;
    c.exact_order = Some(vec![zone(5598600)]);
    cases.push(c);

    let mut results = Vec::new();
    for c in cases {
        let query = PickQuery::new(c.point, 5.0 / 280.0).unwrap();
        let hits = index
            .query_visible_hits(query, 280.0, c.filter, &c.display, 64, &cancel)
            .expect("production canvas query");
        let objects: Vec<_> = hits.iter().map(|hit| hit.anchor.object).collect();
        let passed = hits.first().map(|hit| hit.anchor) == c.first
            && c.exact_order
                .as_ref()
                .is_none_or(|expected| expected == &objects)
            && c.contains.iter().all(|object| objects.contains(object))
            && c.excludes.iter().all(|object| !objects.contains(object));
        results.push(json!({"name":c.name,"point_mm":c.point,"pixels_per_mm":280,"tolerance_mm":5.0 / 280.0,"filter_bits":u8::from(c.filter),"display":c.display,"expected_first":c.first,"expected_order":c.exact_order,"required_objects":c.contains,"excluded_objects":c.excludes,"actual_hits":hits,"passed":passed}));
    }
    let failures: Vec<_> = results
        .iter()
        .filter(|c| c["passed"] != true)
        .map(|c| c["name"].clone())
        .collect();
    let report = json!({"method":"Production SegmentIndex::query_visible_hits / signed coverage / display rank with exact source paths restored from prior importer-output subset", "input_target":input,"input_target_bytes":bytes.len(),"input_target_sha256":input_sha256,"source":target.source,"index_budget_bytes":4*1024*1024,"counts":{"pins":scene.pins.len(),"vias":scene.vias.len(),"segments":scene.segments.len(),"zones":scene.zones.len()},"cases":results,"failure_names":failures,"passed":failures.is_empty(),"full_board_reimported_this_run":false,"gpu_mesh_validated":false,"allegro_visual_validated":false,"limitations":["Prior parsed-source subset only; omitted neighbors and global board ordering are not tested","Target SHA256 calculated inside this test from the exact bytes deserialized","No original BRD, full scene, GUI, screenshot, font or ANSI access","Display/category/alpha behavior is current Pomelo CPU product semantics, not Allegro parity","Imported GPU mesh is not restored; canvas query uses exact preserved analytic paths"]});
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
        .expect("write explicit report");
    assert!(
        failures.is_empty(),
        "source canvas-picking mismatches: {failures:?}"
    );
}
