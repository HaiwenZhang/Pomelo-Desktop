# Native board readers

All active parsers are Rust implementations inside `pomelo-import`. `FormatImporter` selects a reader, checks source size and cancellation, decompresses ODB++ gzip archives, and finalizes saved copper through `pomelo-core::copper::CopperMesh`. Readers produce the shared board model directly; no JavaScript runtime, JSON scene bridge, Node.js tool or external reference checkout is required.

| Reader | Source handling |
| --- | --- |
| `allegro/` | Bounded BRD binary records, indexed database, source layout versions, connectivity, component placement, padstacks, routes, copper and annotations |
| `altium/` | Checked OLE compound streams, Board6 layer stacks, tracks/arcs, saved region fills, component placement, per-layer padstacks, drills, graphics and text |
| `odb/` | Bounded TAR/gzip, matrix/step/layer features, cached standard/custom symbols, EDA network ownership, component toeprints, pads, drill spans and profile |
| `pads/` | Checked binary SDB sections, padstack/footprint definitions, pin and junction network ownership, routing graph, placed pins/vias, saved copper and board outline |
| `kicad.rs`, `sexpr.rs` | Bounded S-expression reader, layer/network tables, tracks/arcs, footprints, custom/per-layer pads, vias, saved filled polygons and board drawings |
| `hfss/` | Unencrypted EDB 12.1 typed DEF archive, AEDT property grammar, layer/net/component tables, analytic primitives, saved voids, pad definitions and placed instances |

PADS binary version tags accepted by the source reader are 2011, 2017, 2019, 2020, 2021, 2022 and 2024–2027. This is a version-layout allowlist, not a claim that every tool release has been tested. PADS ASCII is unsupported. ODB++ imports a single selected board step; HFSS imports a single board cell and explicit physical quantities. Unsupported geometry, inconsistent references and truncated data return format errors rather than a partial board.

The source layouts were ported from the local Pomelo reference implementation. Its MIT notice is preserved in `../../REFERENCE-LICENSE`. Saved fills are consumed without repouring. Curve flattening and PADS rounded offsets use a **0.001 mm chord tolerance**; analytic arcs are retained for tracks, outlines and custom pad boundaries. This intentionally reduces geometry relative to the reference's finer tessellation. Copper offsets and boolean operations use `clipper2-rust`.

Current coverage limits remain visible through diagnostics: PADS ordinary graphics/text, KiCad text and filled graphic presentation, and opaque ODB++ description properties are omitted. Unsupported HFSS extensions and geometry fail explicitly. Altium imports saved region copper and reports polygons without a saved fill. Source-format compatibility beyond the supplied cases needs further fixtures.

The local retired TS files are archived in the ignored `.cache/retired-format-import` directory for recovery only. They are outside the active source tree and have no build or runtime entry point.

See [the validation guide](../../../../docs/validation-guide.md) for validation commands and [the code organization guide](../../../../docs/code-organization.md) for module responsibilities.
