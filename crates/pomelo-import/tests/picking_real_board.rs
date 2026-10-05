use pomelo_core::{
    display::BoardDisplay,
    model::Point,
    picking::{PickFilter, PickQuery},
    picking_index::SegmentIndex,
    task::CancellationToken,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "requires explicitly selected local BRD and report paths"]
fn real_board_member_pages_match_source_network_membership() {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    let source = PathBuf::from(std::env::var_os("POMELO_PICK_CASE").expect("explicit case path"));
    let report =
        PathBuf::from(std::env::var_os("POMELO_PICK_REPORT").expect("explicit report path"));
    let cancel = CancellationToken::default();
    let mut options = ImportOptions::default();
    if std::env::var("POMELO_PICK_ENCODING").as_deref() == Ok("windows-1252") {
        options.text_encoding = pomelo_import::TextEncoding::Windows1252;
    }
    let board = AllegroImporter
        .import(
            &source,
            &options,
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let scene = &board.scene;
    let mut networks = std::collections::BTreeMap::new();
    for net in scene
        .segments
        .iter()
        .map(|s| s.net)
        .chain(scene.pins.iter().map(|p| p.net))
        .chain(scene.vias.iter().map(|v| v.net))
        .chain(scene.zones.iter().map(|z| z.net))
    {
        if net.0 != 0 {
            *networks.entry(net).or_insert(0_usize) += 1;
        }
    }
    let (&net, &total) = networks
        .iter()
        .max_by_key(|(_, count)| **count)
        .expect("connected network");
    let expected: Vec<_> = scene
        .segments
        .iter()
        .filter(|s| s.net == net)
        .map(|s| SelectedObject::Segment(s.id))
        .chain(
            scene
                .pins
                .iter()
                .filter(|p| p.net == net)
                .map(|p| SelectedObject::Pin(p.id)),
        )
        .chain(
            scene
                .vias
                .iter()
                .filter(|v| v.net == net)
                .map(|v| SelectedObject::Via(v.id)),
        )
        .chain(
            scene
                .zones
                .iter()
                .filter(|z| z.net == net)
                .map(|z| SelectedObject::Zone(z.id)),
        )
        .collect();
    let mut collected = Vec::new();
    let mut pages = Vec::new();
    for offset in (0..total).step_by(256) {
        let started = Instant::now();
        let page = SelectionTarget::Net(net)
            .members_page(scene, offset, 256, &cancel)
            .unwrap();
        let query_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(page.total, total);
        assert_eq!(page.objects, expected[offset..(offset + 256).min(total)]);
        pages.push(
            serde_json::json!({"offset":offset,"rows":page.objects.len(),"query_ms":query_ms}),
        );
        collected.extend(page.objects);
    }
    assert_eq!(collected, expected);
    assert!(
        SelectionTarget::Net(net)
            .members_page(scene, total, 256, &cancel)
            .unwrap()
            .objects
            .is_empty()
    );
    std::fs::write(report, serde_json::to_vec_pretty(&serde_json::json!({
        "scope":"cpu_source_network_member_pages", "source":source,
        "sha256":hex::encode(Sha256::digest(std::fs::read(&source).unwrap())),
        "encoding":options.text_encoding.tag(), "network_id":net.0,"total":total,"pages":pages,
        "profile":"optimized_debug", "runs":1,"ui_validated":false,"electrical_continuity_validated":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires explicitly selected local BRD and report paths"]
fn real_board_mixed_candidates_match_category_reference_queries() {
    use pomelo_core::{model::NetId, picking::ObjectHit, selection::SelectedObject};
    let source = PathBuf::from(std::env::var_os("POMELO_PICK_CASE").expect("explicit case path"));
    let report =
        PathBuf::from(std::env::var_os("POMELO_PICK_REPORT").expect("explicit report path"));
    let cancel = CancellationToken::default();
    let mut options = ImportOptions::default();
    if std::env::var("POMELO_PICK_ENCODING").as_deref() == Ok("windows-1252") {
        options.text_encoding = pomelo_import::TextEncoding::Windows1252;
    }
    let board = AllegroImporter
        .import(
            &source,
            &options,
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let index = SegmentIndex::build(board.scene.clone(), 4_000_000, &cancel).unwrap();
    let mut points = Vec::new();
    let mut sample = |values: Vec<(Point, pomelo_core::model::LayerId)>| {
        let step = (values.len() / 32).max(1);
        points.extend(values.into_iter().step_by(step).take(32));
    };
    sample(
        board
            .scene
            .segments
            .iter()
            .map(|s| (s.a, s.layer))
            .collect(),
    );
    sample(
        board
            .scene
            .pins
            .iter()
            .filter_map(|p| p.pads.first().map(|pad| (p.at, pad.layer)))
            .collect(),
    );
    sample(
        board
            .scene
            .vias
            .iter()
            .filter_map(|v| v.pads.first().map(|pad| (v.at, pad.layer)))
            .collect(),
    );
    sample(
        board
            .scene
            .zones
            .iter()
            .filter_map(|z| z.mesh.vertices.first().map(|p| (*p, z.layer)))
            .collect(),
    );
    assert!(!points.is_empty());
    let mut probes = Vec::new();
    for (point, layer) in points {
        for (connected, hidden) in [(false, false), (true, false), (false, true)] {
            let mut display = BoardDisplay::default();
            if hidden {
                display.hidden_layers.insert(layer);
            }
            let query = PickQuery::new(point, 0.15).unwrap();
            let started = Instant::now();
            let actual = index
                .query_objects(query, PickFilter::all(), &display, 20, &cancel, connected)
                .unwrap();
            let query_ms = started.elapsed().as_secs_f64() * 1000.0;
            // Separate repeated production queries with only one category enabled.
            // These are not nested timings and do not sum exactly to query_ms.
            let mut category_ms = serde_json::Map::new();
            for (name, category) in [
                ("pin", pomelo_core::picking::PickCategory::Pin),
                ("via", pomelo_core::picking::PickCategory::Via),
                ("segment", pomelo_core::picking::PickCategory::Segment),
                ("zone", pomelo_core::picking::PickCategory::Zone),
            ] {
                let mut filter = PickFilter::none();
                filter.set(category, true);
                let started = Instant::now();
                let category_hits = index
                    .query_objects(query, filter, &display, 20, &cancel, connected)
                    .unwrap();
                category_ms.insert(
                    name.into(),
                    serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
                );
                assert!(category_hits.iter().all(|hit| pomelo_core::picking::PickCategory::from(hit.object) == category));
            }
            // Independent full-category merge; source order breaks ties within each category.
            let mut expected: Vec<ObjectHit> = Vec::new();
            expected.extend(
                query
                    .pins_where(
                        &board.scene,
                        PickFilter::all(),
                        &display,
                        64,
                        &cancel,
                        |p| !connected || p.net != NetId(0),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|h| ObjectHit {
                        object: SelectedObject::Pin(h.pin.id),
                        distance_mm: h.distance_mm,
                    }),
            );
            expected.extend(
                query
                    .vias_where(
                        &board.scene,
                        PickFilter::all(),
                        &display,
                        64,
                        &cancel,
                        |v| !connected || v.net != NetId(0),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|h| ObjectHit {
                        object: SelectedObject::Via(h.via.id),
                        distance_mm: h.distance_mm,
                    }),
            );
            for segment in &board.scene.segments {
                if !display.layer_visible(segment.layer) || (connected && segment.net == NetId(0)) {
                    continue;
                }
                let distance_mm =
                    pomelo_core::picking::segment_distance_mm(segment, point).unwrap();
                if distance_mm <= 0.15 {
                    expected.push(ObjectHit {
                        object: SelectedObject::Segment(segment.id),
                        distance_mm,
                    });
                }
            }
            expected.extend(
                query
                    .zones_where(
                        &board.scene,
                        PickFilter::all(),
                        &display,
                        64,
                        &cancel,
                        |z| !connected || z.net != NetId(0),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|h| ObjectHit {
                        object: SelectedObject::Zone(h.zone.id),
                        distance_mm: h.distance_mm,
                    }),
            );
            expected.sort_by(|a, b| a.distance_mm.total_cmp(&b.distance_mm));
            expected.truncate(20);
            assert_eq!(
                actual, expected,
                "point {point:?}, connected {connected}, hidden {hidden}"
            );
            probes.push(serde_json::json!({"point":point,"connected_only":connected,
                "hidden_layer":hidden.then_some(layer),"query_ms":query_ms,"category_query_ms":category_ms,
                "candidates":actual.iter().map(|h| serde_json::json!({"object":format!("{:?}",h.object),"distance_mm":h.distance_mm})).collect::<Vec<_>>() }));
        }
    }
    std::fs::write(report, serde_json::to_vec_pretty(&serde_json::json!({
        "scope":"cpu_mixed_candidate_merge_parity", "source":source,
        "sha256":hex::encode(Sha256::digest(std::fs::read(&source).unwrap())),
        "encoding":options.text_encoding.tag(),"segments":board.scene.segments.len(),
        "pins":board.scene.pins.len(),"vias":board.scene.vias.len(),"zones":board.scene.zones.len(),
        "probes":probes,"ui_validated":false,"gpu_geometry_parity_validated":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires explicitly selected local BRD and report paths"]
fn real_board_index_matches_precise_reference_scan() {
    let source = PathBuf::from(std::env::var_os("POMELO_PICK_CASE").expect("explicit case path"));
    let report =
        PathBuf::from(std::env::var_os("POMELO_PICK_REPORT").expect("explicit report path"));
    let cancel = CancellationToken::default();
    let mut options = ImportOptions::default();
    if std::env::var("POMELO_PICK_ENCODING").as_deref() == Ok("windows-1252") {
        options.text_encoding = pomelo_import::TextEncoding::Windows1252;
    }
    let board = AllegroImporter
        .import(
            &source,
            &options,
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let started = Instant::now();
    let index = SegmentIndex::build(board.scene.clone(), 4_000_000, &cancel).unwrap();
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut probes = Vec::new();
    let step = (board.scene.segments.len() / 128).max(1);
    for segment in board.scene.segments.iter().step_by(step).take(128) {
        let midpoint = segment.arc.map_or_else(
            || {
                Point::new(
                    segment.a.x * 0.5 + segment.b.x * 0.5,
                    segment.a.y * 0.5 + segment.b.y * 0.5,
                )
            },
            |arc| {
                let angle = arc.start + arc.sweep * 0.5;
                Point::new(
                    arc.center.x + arc.radius * angle.cos(),
                    arc.center.y + arc.radius * angle.sin(),
                )
            },
        );
        for (point, hidden) in [(segment.a, false), (midpoint, false), (midpoint, true)] {
            let mut display = BoardDisplay::default();
            if hidden {
                display.hidden_layers.insert(segment.layer);
            }
            let query = PickQuery::new(point, 0.15).unwrap();
            let started = Instant::now();
            let indexed = index
                .query(query, PickFilter::all(), &display, 20, &cancel)
                .unwrap();
            let query_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = Instant::now();
            let reference = query
                .segments(
                    &board.scene.segments,
                    PickFilter::all(),
                    &display,
                    20,
                    &cancel,
                )
                .unwrap();
            let scan_ms = started.elapsed().as_secs_f64() * 1000.0;
            let actual: Vec<_> = indexed
                .iter()
                .map(|hit| (hit.segment.id, hit.distance_mm))
                .collect();
            let expected: Vec<_> = reference
                .iter()
                .map(|hit| (hit.segment.id, hit.distance_mm))
                .collect();
            assert_eq!(
                actual, expected,
                "source {:?}, point {:?}, hidden {}",
                segment.id, point, hidden
            );
            probes.push(serde_json::json!({"source_segment": segment.id, "point": point, "hidden_layer": hidden.then_some(segment.layer), "query_ms": query_ms, "reference_scan_ms": scan_ms, "candidates": actual}));
        }
    }
    assert!(!probes.is_empty());
    let data = serde_json::json!({"scope": "cpu_track_index_reference_parity", "source": source,
        "sha256": hex::encode(Sha256::digest(std::fs::read(&source).unwrap())), "encoding": options.text_encoding.tag(),
        "segments": index.segment_count(), "build_ms": build_ms, "probes": probes,
        "ui_validated": false, "gpu_pixel_parity_validated": false, "pad_zone_picking_validated": false});
    std::fs::write(report, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
}
