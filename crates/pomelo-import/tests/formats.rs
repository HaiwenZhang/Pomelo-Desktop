use std::path::Path;

use pomelo_core::{
    model::{NetId, Point},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportError, ImportOptions,
    formats::{BoardFormat, FormatImporter},
};

const KICAD: &str = include_str!("data/basic.kicad_pcb");

#[test]
fn dispatcher_recognizes_extensions_case_insensitively() {
    for (name, expected) in [
        ("B.BRD", BoardFormat::Allegro),
        ("B.PcbDoc", BoardFormat::Altium),
        ("B.tar.gz", BoardFormat::Odb),
        ("B.TGZ", BoardFormat::Odb),
        ("B.tar", BoardFormat::Odb),
        ("B.pcb", BoardFormat::Pads),
        ("B.kicad_pcb", BoardFormat::KiCad),
        ("edb.def", BoardFormat::Hfss),
    ] {
        assert_eq!(BoardFormat::from_path(Path::new(name)), Some(expected));
    }
    assert_eq!(BoardFormat::from_path(Path::new("B.zip")), None);
}

#[test]
fn dispatcher_probes_all_supported_source_families() {
    let mut tar = vec![0; 512];
    tar[257..262].copy_from_slice(b"ustar");
    let mut hfss = vec![0; 5];
    hfss.extend_from_slice(b"$begin 'Hdr'");
    for (bytes, expected) in [
        (
            vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1],
            "altium",
        ),
        (vec![0, 0xff, 0x26, 0x20], "pads"),
        (vec![0x1f, 0x8b], "odb"),
        (tar, "odb"),
        (hfss, "hfss"),
        (KICAD.as_bytes().to_vec(), "kicad"),
    ] {
        assert_eq!(
            FormatImporter
                .probe(&bytes, &ImportOptions::default())
                .unwrap()
                .format,
            expected
        );
    }
}

#[test]
fn cancellation_interrupts_native_parser_execution() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.kicad_pcb");
    let mut source = String::from(
        "(kicad_pcb (version 20260206) (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal))",
    );
    for _ in 0..20_000 {
        source.push_str("(segment (start 0 0) (end 1 1) (width 0.2) (layer \"F.Cu\"))");
    }
    source.push(')');
    std::fs::write(&path, source).unwrap();
    let cancel = CancellationToken::default();
    std::thread::scope(|scope| {
        let progress = |update: pomelo_core::task::ImportProgress| {
            if update.stage == pomelo_core::task::ImportStage::Decoding {
                scope.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    cancel.cancel();
                });
            }
        };
        let result = FormatImporter.import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &progress,
            },
        );
        assert!(matches!(result, Err(ImportError::Cancelled)));
    });
}

#[test]
fn kicad_tracks_pads_and_fill_share_net_identity_and_component_membership() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.kicad_pcb");
    std::fs::write(&path, KICAD).unwrap();
    let cancel = CancellationToken::default();
    let board = FormatImporter
        .import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let scene = &board.scene;
    assert_eq!(board.source.format, "kicad");
    assert_eq!(scene.nets[&NetId(7)], "GND");
    assert_eq!(scene.segments[0].net, scene.pins[0].net);
    assert_eq!(scene.pins[1].net, scene.zones[0].net);
    assert_eq!(scene.components.len(), 1);
    assert_eq!(
        scene.components[0].pins,
        scene.pins.iter().map(|p| p.id).collect::<Vec<_>>()
    );
    assert_eq!(scene.pins[0].owner_id, scene.components[0].id);
    assert_eq!(scene.zones[0].mesh.vertices.len(), 4);
    assert!(
        scene.zones[0]
            .mesh
            .covers_fill(Point::new(0.5, -0.5), &cancel)
            .unwrap()
    );
    assert_eq!(scene.pins[0].at, Point::new(10.0, -20.0));
}

#[test]
fn malformed_sources_return_format_specific_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    for extension in ["pcbdoc", "pcb", "def", "tar", "kicad_pcb"] {
        let path = directory.path().join(format!("bad.{extension}"));
        std::fs::write(&path, b"not a board").unwrap();
        let cancel = CancellationToken::default();
        let result = FormatImporter.import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        );
        assert!(
            matches!(result, Err(ImportError::Format { .. })),
            "{extension}"
        );
    }
}

#[test]
fn cancellation_and_input_budgets_apply_before_parsing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("board.kicad_pcb");
    std::fs::write(&path, KICAD).unwrap();
    let cancel = CancellationToken::default();
    let options = ImportOptions {
        max_file_bytes: 1,
        ..Default::default()
    };
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    assert!(matches!(
        FormatImporter.import(&path, &options, &context),
        Err(ImportError::ResourceLimit { .. })
    ));
    cancel.cancel();
    assert!(matches!(
        FormatImporter.import(&path, &ImportOptions::default(), &context),
        Err(ImportError::Cancelled)
    ));
}
