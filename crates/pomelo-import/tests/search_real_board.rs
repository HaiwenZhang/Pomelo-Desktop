use std::{collections::BTreeMap, path::PathBuf, time::Instant};

use pomelo_core::{
    search::{SearchIndex, SearchTarget},
    task::CancellationToken,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires explicitly selected local BRD and report paths"]
fn real_board_search_counts_and_all_target_bounds() {
    let source = PathBuf::from(std::env::var_os("POMELO_SEARCH_CASE").expect("explicit case path"));
    let report =
        PathBuf::from(std::env::var_os("POMELO_SEARCH_REPORT").expect("explicit report path"));
    let cancel = CancellationToken::default();
    let mut options = ImportOptions::default();
    if std::env::var("POMELO_SEARCH_ENCODING").as_deref() == Ok("windows-1252") {
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
    let index = SearchIndex::build(&board.scene, &cancel).unwrap();
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut counts = BTreeMap::new();
    for net in board
        .scene
        .segments
        .iter()
        .map(|v| v.net)
        .chain(board.scene.pins.iter().map(|v| v.net))
        .chain(board.scene.vias.iter().map(|v| v.net))
        .chain(board.scene.zones.iter().map(|v| v.net))
    {
        if net.0 != 0 {
            *counts.entry(net).or_insert(0_usize) += 1;
        }
    }
    let mut targets = Vec::new();
    for entry in index.entries() {
        if let SearchTarget::Net(net) = entry.target {
            assert_eq!(entry.count, counts[&net]);
        }
        let started = Instant::now();
        let bounds = entry
            .target
            .bounds(&board.scene, &cancel)
            .unwrap()
            .expect("source target geometry");
        let locate_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert!(bounds.is_valid());
        let query_started = Instant::now();
        let query_results = index.find(&entry.name, 20);
        let query_ms = query_started.elapsed().as_secs_f64() * 1000.0;
        assert!(
            index
                .find(&entry.name, index.entries().len())
                .iter()
                .any(|result| result.target == entry.target),
            "source name {:?}, target {:?}",
            entry.name,
            entry.target
        );
        targets.push(serde_json::json!({"target": format!("{:?}", entry.target),
            "name": entry.name, "count": entry.count, "bounds": bounds, "locate_ms": locate_ms,
            "query_ms": query_ms, "query_result_count": query_results.len()}));
    }
    let data = serde_json::json!({"source": source, "sha256": format!("{:x}", Sha256::digest(std::fs::read(&source).unwrap())),
        "scope": "cpu_search_counts_and_target_bounds", "encoding": options.text_encoding.tag(), "build_ms": build_ms,
        "entries": targets, "ui_validated": false, "web_parity_validated": false});
    std::fs::write(report, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
}
