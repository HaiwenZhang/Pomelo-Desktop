# Native board readers

All active parsers are Rust implementations inside `pomelo-import`. `FormatImporter` selects a reader, checks source size and cancellation, decompresses ODB++ gzip archives, and finalizes saved copper through `pomelo-core::copper::CopperMesh`. Readers produce the shared board model directly; no JavaScript runtime, JSON scene bridge, Node.js tool or external reference checkout is required.

| Reader | Source handling |
| --- | --- |
| `allegro/` | Bounded `.brd` / `.mcm` binary records, indexed database, source layout versions, connectivity, component placement, padstacks, routes, copper and annotations |
| `altium/` | Checked OLE compound streams, Board6 layer stacks, tracks/arcs, saved region fills, component placement, per-layer padstacks, drills, graphics and text |
| `odb/` | Bounded TAR/gzip, matrix/step/layer features, cached standard/custom symbols, EDA network ownership, component toeprints, pads, drill spans and profile |
| `pads/` | Checked binary SDB sections, padstack/footprint definitions, pin and junction network ownership, routing graph, placed pins/vias, saved copper and board outline |
| `kicad.rs`, `sexpr.rs` | Byte-span board index with one bounded S-expression object decoded at a time; layer/network tables, tracks/arcs, footprints, custom/per-layer pads, vias, saved filled polygons and board drawings |
| `hfss/` | Self-describing unencrypted EDB typed DEF archives, AEDT property grammar, layer/net/component tables, analytic primitives, saved voids, pad definitions and placed instances |

Allegro 18.0 and 18.1 use the `0x00150000` and `0x00150200` layout families. Both support 92-byte paired-net records and header-defined chain sentinels; their writer metadata, coordinate units and divisor remain at version-specific header offsets. Synthetic coverage is in `tests/allegro_181.rs`. Release labels and directory names do not identify the on-disk layout.

EDB compatibility uses the archive's field IDs and type table, rather than a version allowlist. Local archives with headers 7.3, 8.6, 10.6 and 12.1 share the consumed geometry schema; additional fields and object types are decoded from their own declarations. The actual archive version is retained in source metadata. Structurally compatible unseen versions are accepted, while changed core field encodings and encountered unknown wire types fail explicitly. See [EDB compatibility](../../../../docs/edb-format-compatibility.md) for evidence and limits.

The local cases cover component-local coordinates with translation, rotation and mirroring, both rectangle representations, clipped and nonround trace boundaries, packed polygon instances, and auxiliary drawing layers. Numeric text in plating metadata is parsed explicitly; unresolved out-of-range ratios are reported. Polygon drills retain shared, exact source paths and fill contours. Non-copper filled graphics render as boundaries. Non-unit component scale remains unsupported.

PADS binary version tags accepted by the source reader are 2011, 2017, 2019, 2020, 2021, 2022 and 2024–2027. This is a version-layout allowlist, not a claim that every tool release has been tested. PADS ASCII is unsupported. ODB++ imports a single selected board step. HFSS displays the first cell of a multi-cell archive with an explicit diagnostic, and reports omitted simulation terminals and text annotations. Empty databases return `NoGeometry`. Unsupported geometry, ambiguous references and truncated data return format errors.

The source layouts were ported from the local Pomelo reference implementation. Its MIT notice is preserved in `../../REFERENCE-LICENSE`. Saved fills are consumed without repouring. Curve flattening and PADS rounded offsets use a **0.001 mm chord tolerance**; analytic arcs are retained for tracks, outlines and custom pad boundaries. This intentionally reduces geometry relative to the reference's finer tessellation. Copper offsets and boolean operations use `clipper2-rust`.

Current coverage limits remain visible through diagnostics: PADS ordinary graphics/text, KiCad text and filled graphic presentation, and opaque ODB++ description properties are omitted. Unsupported HFSS extensions and geometry fail explicitly. Altium imports saved region copper and reports polygons without a saved fill. Source-format compatibility beyond the supplied cases needs further fixtures.

HFSS version coverage is verified against AMD LP5X and Intel Haswell EDB cases saved by 2021R1, 2022R2, 2025R1 and 2026R1. Run `POMELO_AEDB_CASES=/path/to/aedb_testcase cargo test -p pomelo-import --test native_cases hfss_versions -- --ignored` to check those local files; proprietary board data is not included in the repository.

The PyEDB TEDB regression uses `POMELO_HFSS_TEDB=/path/to/pyedb/tests/example_models/TEDB cargo test -p pomelo-import --test hfss_tedb -- --ignored`. Its 34 nonempty databases import; `test_107.aedb` has no board geometry. Both `Encrypted=false` and `EncryptionLevel=0` unencrypted headers are supported. The 3D preview displays all vias as solid bodies without subtracting their drill centers from pads, traces or zones; 2D drill geometry remains exact.

The local retired TS files are archived in the ignored `.cache/retired-format-import` directory for recovery only. They are outside the active source tree and have no build or runtime entry point.

See [the validation guide](../../../../docs/validation-guide.md) for validation commands and [the code organization guide](../../../../docs/code-organization.md) for module responsibilities.
