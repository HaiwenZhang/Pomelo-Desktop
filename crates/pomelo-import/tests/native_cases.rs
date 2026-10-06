//! Explicit local-case regression; no proprietary board data is checked into the repository.
use pomelo_core::{
    model::{LayerId, ObjectId},
    task::CancellationToken,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use std::{collections::HashSet, path::PathBuf};

#[test]
#[ignore = "requires POMELO_NATIVE_CASES pointing at the local pomelo_cases directory"]
fn native_readers_preserve_case_counts_connectivity_and_finite_geometry() {
    let root =
        PathBuf::from(std::env::var_os("POMELO_NATIVE_CASES").expect("set POMELO_NATIVE_CASES"));
    let cancel = CancellationToken::default();
    for (file, counts) in [
        ("MT6753_Layout.pcb", [10, 677, 19461, 3725, 869, 16736, 459]),
        (
            "OpenRex_V1I1_PCB.PcbDoc",
            [10, 708, 22525, 3831, 1077, 2373, 324],
        ),
        (
            "odb++_case.tgz",
            [10, 1060, 459945, 5894, 1528, 16410, 19578],
        ),
        ("edb.def", [8, 4004, 241280, 21605, 3745, 11589, 988]),
    ] {
        let board = FormatImporter
            .import(
                &root.join(file),
                &ImportOptions::default(),
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )
            .unwrap();
        let s = &board.scene;
        assert_eq!(
            [
                s.layers.len(),
                s.nets.len(),
                s.segments.len(),
                s.pins.len(),
                s.components.len(),
                s.vias.len(),
                s.zones.len()
            ],
            counts,
            "{file}"
        );
        assert!(s.bounds.is_valid(), "{file}");
        let layers: HashSet<_> = s
            .layers
            .iter()
            .map(|l| l.id)
            .chain(s.special_layers.iter().map(|l| l.id))
            .collect();
        let net_exists = |n| n == pomelo_core::model::NetId(0) || s.nets.contains_key(&n);
        let pins: std::collections::HashMap<_, _> = s.pins.iter().map(|p| (p.id, p)).collect();
        let mut member_ids = HashSet::new();
        for component in &s.components {
            for id in &component.pins {
                let pin = pins.get(id).unwrap();
                assert_eq!(pin.owner_id, component.id, "{file}");
                assert!(member_ids.insert(*id), "{file}");
            }
        }
        for pin in &s.pins {
            assert!(
                net_exists(pin.net) && pin.at.x.is_finite() && pin.at.y.is_finite(),
                "{file}"
            );
            if pin.owner_id != ObjectId(0) {
                assert!(member_ids.contains(&pin.id), "{file}");
            }
            for pad in &pin.pads {
                assert!(
                    layers.contains(&pad.layer) && pad.width.is_finite() && pad.height.is_finite(),
                    "{file}"
                );
            }
        }
        for via in &s.vias {
            assert!(net_exists(via.net), "{file}");
            if let (Some(first), Some(last)) = (via.start_layer, via.end_layer) {
                assert!(
                    first <= last && layers.contains(&first) && layers.contains(&last),
                    "{file}"
                );
            }
        }
        let mut edges = HashSet::new();
        for edge in s
            .segments
            .iter()
            .chain(&s.outline)
            .chain(s.drawings.iter().flat_map(|d| &d.segments))
        {
            assert!(
                edges.insert(edge.id),
                "duplicate edge {:?} in {file}",
                edge.id
            );
            assert!(edge.bounds().is_some() && net_exists(edge.net), "{file}");
            assert!(
                edge.layer == LayerId::UNASSIGNED
                    || layers.contains(&edge.layer)
                    || s.drawing_layers.iter().any(|l| l.id == edge.layer),
                "{file}"
            );
        }
        for zone in &s.zones {
            assert!(
                layers.contains(&zone.layer) && net_exists(zone.net),
                "{file}"
            );
            assert!(!zone.mesh.indices.is_empty(), "{file}");
        }
    }
}
