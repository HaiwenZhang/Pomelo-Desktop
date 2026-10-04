# Large-board and MCM geometry residency

Requested implementation: reduce simultaneous geometry residency and adapt budgets to available resources. The user's subsequent constraint excludes subdividing large copper fills. Copper keeps its original complete geometry; upload conversion may use bounded temporary byte ranges without spatial subdivision. Increasing independent fixed caps is not the final design.

## Requirements and verification

- Measure preparation stages and process peak memory against `E:\PCB\brd_cases\15061-1b.brd`. Include fonts, picking and GPU in final application validation; a geometry-only probe cannot prove application loading.
- Also validate `E:\PCB\brd_cases\ntpcb_320mb.brd` through the same production preparation policy and include it in later residency/GPU acceptance.
- Include `E:\PCB\mcm\ARM_Test.mcm` in import, complete preparation and performance validation. It uses the same Allegro importer; extension alone does not determine scene compatibility.
- Eliminate intermediate geometry copies; charge final instance storage and actual layer batches. Preserve signed arcs, holes, source ordering, IDs, dynamic shape flags, cancellation and allocation failure reporting.
- Compact straight and curved geometry separately, with batch-level common attributes and coordinate origins. Validate precision at large coordinates and native shader output on supported backends.
- Share an adaptive CPU budget across import scratch, persistent scene, preparation, indexes and caches. Count temporary overlapping allocations. Handle memory availability changes without unlimited automatic growth. Keep GPU residency independently bounded.
- For traces and text, prioritize visible geometry and neighboring runs, evict rebuildable offscreen data, and bound upload work each frame. Preserve source ordering. Do not spatially subdivide large copper fills.
- Provide overview detail levels backed by exact original data for selection. Defer detailed decoding and cache derived blocks on disk with source identity/version validation.
- Verify the real board can open and interact, including text and picking, and compare memory/time before and after. Run regression tests and all-feature Clippy for changed crates.

## Current implementation and evidence (2026-10-04)

`PreparedTracks::build_zone_outlines` now streams source paths and compact rings directly into final instances; it no longer retains a board-wide `Vec<Segment>`. Dynamic IDs remain separately bounded scratch. Batch memory is charged by actual layer runs after in-place instance sorting, before batch allocation.

`cargo run --release -p pomelo-render --example geometry_memory --locked -- E:\brd_cases\15061-1b.brd` imports the production scene and retains the prepared tracks, drawings, outlines, copper, pads, custom meshes, drills, default MSDF text and picking index together. It reports stage-specific failures rather than one generic localized message. It does not apply an external ANSI override, build the label index or allocate GPU resources.

Initial real-board run after removing outline scratch:

| Stage | Count | Bytes |
| --- | --- | --- |
| Tracks | 313,185 instances, 6 batches | 40,087,776 resident |
| Drawings | 2,823 instances, 1 batch | 361,360 resident |
| Zone outlines | 258,356 instances, 8 batches | 33,069,696 resident |
| Copper | 3,646,827 vertices, 10,611,171 indices | 100,793,916 upload |

Pads, custom meshes, drills and picking without text quads also passed their production default limits in that run. A 10ms process sample of that geometry-only version recorded 1,633,153,024 B peak working set and 2,096,427,008 B sampled private bytes, exit 0, 22,329ms total. These are post-change measurements, not a demonstrated before/after memory reduction; no text or GPU was included. The local report is `docs/gpu-validation/geometry-memory-15061.json` (binary hash included; validation reports are ignored by Git).

Adding the production MSDF text path reproduced the budget error: 507,358 text pick quads caused `DrawingIndex::build` to reject 297,945,704 B against its 268,435,456 B ceiling. The drawing index charged all board glyphs as drawing entries, then grouped/copied all of them, although only drawing-owned text was retained. It now filters by drawing ownership before budgeting or copying, budgets entry and glyph storage separately, and sorts one compact glyph buffer instead of growing a vector per object. The rotated text regression now includes 2,000 unrelated quads under a 64KiB shared index budget while checking drawing ownership, source order and visibility.

The release probe rerun passed with all 507,358 default MSDF text pick quads supplied, and picking `Ok(())`, without raising any fixed budget. Raw local logs are `target/geometry-memory-15061-text{,-progress}.log`. This establishes the CPU reproduction/fix; complete application and GPU acceptance remain outstanding.

## Shared preparation budget and second large-board case

The desktop now shares a `MemoryBudget` across document geometry preparation. `PreparationMemory` refreshes OS memory telemetry before each major geometry build, bounds the pool to half of physical RAM and grants at most three quarters of available memory plus existing managed residency. A missing telemetry result uses a bounded fallback. Builders receive the remaining byte allowance with intrinsic `u32` count ceilings; retained vector capacities are charged after build. Failed builds release their temporary grants. Reservations now belong to the prepared geometry itself and survive external frame/renderer Arc clones; see the allocation lifetime section below. CPU import caches, source scenes, fonts/text and picking/search are still outside this managed pool, and GPU residency is separate. Complete cross-document accounting remains open for these untracked allocations.

Managed stages: tracks/board outlines, drawing strokes, zone boundaries, copper output, pad output/custom boundaries, custom pad meshes, drill geometry/scopes. Pad batch arrays now allocate actual layer-run counts instead of one batch slot per analytic pad. `MemoryBudget` has tests for failed growth, shrinking under pressure, zero-byte work, and competing workers. The preparation controller tests failed/cancelled stages and release across multiple document controllers. Budget peaks represent reserved allowances, not measured process peaks.

New external tests in `crates/pomelo-render/tests/large_board_cases.rs` use the same `PreparationMemory` implementation as the application and also build MSDF text, label indexing, text picking and search. They are ignored in default CI because BRD files are private local inputs. Run both explicitly:

```powershell
cargo test --release -p pomelo-render --test large_board_cases --locked -- --ignored --test-threads=1 --nocapture
```

Optional `POMELO_LARGE_BOARD_CASES` overrides the default `E:/PCB/brd_cases` directory. The default follows the user's updated input location; historical logs below retain their original paths. These two tests use Windows-1252; external ANSI font overrides, window interactions and GPU rendering are not covered.

The first fixed-limit run passed 15061 and rejected ntpcb at `RENDER_PREPARE_COUNT_LIMIT actual=53748068 limit=32000000`. The adaptive run passed both: ntpcb has 53,748,068 copper vertices and 158,012,190 indices, with 2,070,267,204 B managed geometry residency; 15061 has 239,663,108 B managed geometry residency. Raw logs are `target/geometry-large-board-{cases,adaptive}.log`. The test never drops geometry to satisfy the ceiling.

Independent-process post-change measurements, including default MSDF text, label index and picking/search, excluding window/GPU and external fonts:

| Case | Process elapsed | Peak working set | Sampled peak private bytes | Exit |
| --- | --- | --- | --- | --- |
| 15061-1b | 7,127 ms | 1,636,720,640 B | 2,097,004,544 B | 0 |
| ntpcb_320mb | 7,309 ms | 4,515,143,680 B | 4,644,167,680 B | 0 |

`Process.PeakWorkingSet64` and private bytes were sampled at requested 10ms intervals; the private allocation lifetime peak is not guaranteed. Source and test-binary SHA-256 hashes are recorded in the local ignored report `docs/gpu-validation/geometry-adaptive-two-boards-memory.json`. This is one run per case with uncontrolled OS caches, not a controlled before/after benchmark.

The compact rendering layouts, extension of the shared budget to import/scene/text/index/cache lifetimes, trace/text residency improvements, detail levels, delayed materialization/disk cache, complete-process before/after memory comparison and complete application/GPU validation remain outstanding. Large copper subdivision is excluded by the user's constraint. This file records progress, not completion.

## Trace residency prototype and performance priorities

Trace, outline and text GPU uploads now support source-ordered runs of 4,096 instances, visible pinning, neighboring prefetch and offscreen eviction. The desktop uses a 64 MiB soft target per trace source. This is not a global VRAM limit: visible data may exceed it and copper fills/pads still upload eagerly. All geometry upload paths share a 4 MiB per-frame byte allowance. Eviction candidates are sorted once rather than repeatedly scanning for each victim. Telemetry reports visible readiness and distinguishes cumulative uploads from current residency. CPU geometry remains fully prepared.

Hardware D3D11 tests verified synthetic pan/eviction/return, stable views, reset, ordering and exact eager-path pixels. Real 15061 and ntpcb tests verified tracks and zone outlines at three views (including a mirrored view), exact eager-path pixels, zero extra stable-view uploads and the upload allowance. These tests do not verify copper fills, pads, interactive window behavior or GPU frame times. Logs: `target/geometry-residency-hardware.log` and `target/geometry-residency-two-board-hardware.log`.

Initial-view uploads in this test:

| Case/source | Full upload | Cached initial upload | Reduction |
| --- | ---: | ---: | ---: |
| 15061 tracks | 40,087,680 B | 32,747,648 B | 18.3% |
| 15061 zone outlines | 33,069,568 B | 21,535,232 B | 34.9% |
| ntpcb tracks | 34,475,136 B | 31,853,696 B | 7.6% |
| ntpcb zone outlines | 174,842,368 B | 129,753,600 B | 25.8% |

Source-ordered trace run bounds are often broad. The ntpcb outline return-view cumulative uploads reached 198,959,616 B, exceeding the eager path's 174,842,368 B because eviction required reuploads. This identifies a limitation, not an across-the-board speedup. Further trace/text work should evaluate tighter indexing while preserving blending order and source IDs, and measure draw-call/metadata overhead as well as residency. This does not call for subdividing copper fills.

The external CPU tests now log each preparation stage. A serial release run with uncontrolled OS caches recorded:

| Stage | 15061 | ntpcb |
| --- | ---: | ---: |
| Import and scene construction | 5,621 ms | 4,597 ms |
| Tracks | 30 ms | 21 ms |
| Zone outlines | 40 ms | 243 ms |
| Copper output | 28 ms | 402 ms |
| Pads | 48 ms | 354 ms |
| Picking | 79 ms | 401 ms |
| Total preparation | 6,098 ms | 6,246 ms |

Other stages are logged in `target/geometry-two-board-stage-timings.log`. These are wall times for CPU preparation, not interactive GPU timings or controlled before/after results. Import dominates initial preparation, so overall optimization should profile import sub-stages and redundant decoding/geometry materialization before prioritizing additional GPU scheduling work. The existing record decoder is already on demand; deferred scene materialization must be distinguished from deferred record decoding.

Validation after this step: native render library tests (98 passed, 35 hardware/external tests ignored by default); both external CPU cases passed; both real-board trace hardware tests passed; all-target/all-feature Clippy for core/render/desktop and formatting passed. Full application acceptance remains open.

## Import index optimization

`SceneBuilder::build_with_stage_observer` and the `pomelo-import` example `import_profile` expose coarse stage wall times without per-record timers. The example uses the production index/scene builders and content hash with Windows-1252. Its total excludes desktop font/picking preparation, GPU allocation and window interaction.

This profile identified index scanning and copper construction as the main import costs. Index progress previously queried the Windows clock after every record although callbacks are capped at 10 Hz. It now checks time after approximately 64 KiB of scan progress; cancellation still runs for every record and forced start/completion callbacks remain. Duplicate-string/record checking and insertion now use `HashMap::entry` to avoid hashing successful keys twice. Invalid/duplicate record diagnostics and pre-allocation budget checks retain their ordering. A small-board regression verifies final progress and cancellation triggered by that final callback.

Saved baseline and optimized release executables were run in three alternating pairs, with both cases sequential per process and uncontrolled warm OS caches:

| Case | Index median before / after | Total CPU import median before / after |
| --- | --- | --- |
| 15061 | 2,629 / 2,019 ms (23.2% lower) | 5,493 / 4,920 ms (10.4% lower) |
| ntpcb | 786 / 645 ms (17.9% lower) | 4,576 / 4,407 ms (3.7% lower) |

Copper construction was effectively unchanged (15061 2,137 / 2,141 ms, ntpcb 3,246 / 3,230 ms), making it the next CPU hotspot. These measurements establish an index/CPU import improvement on this machine, not an application frame-rate or memory reduction. Both variants report identical source hashes and scene counts. Raw trial logs: `target/geometry-import-final-{baseline,optimized}-{1,2,3}.log`. The local ignored report `docs/gpu-validation/geometry-import-index-benchmark.json` records all stage samples and executable hashes.

Importer regression tests, both real-board CPU preparation/text/picking regressions and all-target/all-feature Clippy for core/import/render/desktop passed. The real-board regression log is `target/geometry-import-optimized-two-board-regression.log`. At this stage compact representation and deferred copper materialization still required implementation; the source-backed conversion below subsequently addresses the duplicate copper buffers. Full application validation remains open.

## Copper construction measurements and exact deduplication shortcut

An ownership-consuming mesh builder was prototyped to release temporary input rings during copying and triangulate from retained vertices. Three alternating independent-process pairs showed no material process-peak reduction: median peak working set was approximately 1.633 GB for 15061 and 2.860/2.862 GB before/after for ntpcb. Total process times were essentially unchanged. The prototype and its API were removed. The ignored local report `docs/gpu-validation/geometry-owned-copper-memory.json` retains the binary hashes and all samples; this experiment does not substantiate a memory improvement.

The retained change is a cheap separation test in `flatten_path`: if either coordinate delta exceeds the existing `1e-9` coincidence threshold, the points are distinct without evaluating `web_hypot`. Near-threshold points still use the original exact distance calculation. Coordinates, arc subdivision, stored endpoints, closing-point removal and point limits are unchanged. A differential threshold regression covers adjacent floating-point values, diagonal deltas, large coordinate origins and very large finite deltas.

Three alternating release pairs against the saved post-index-optimization baseline recorded:

| Case | Copper median before / after | CPU import median before / after |
| --- | --- | --- |
| 15061 | 2,173 / 2,191 ms | 4,962 / 4,955 ms |
| ntpcb | 3,363 / 3,136 ms (6.8% lower) | 4,600 / 4,368 ms (5.0% lower) |

15061 shows no demonstrated improvement from this shortcut; the small differences are within run variability. These are warm uncontrolled-cache wall times with no GPU or desktop preparation. Raw logs are `target/geometry-copper-distance-{baseline,optimized}-{1,2,3}.log`, with all stage samples and executable hashes in the local ignored `docs/gpu-validation/geometry-copper-distance-benchmark.json`.

Core/import regression suites, both real-board full CPU preparation/text/picking cases and all-target/all-feature Clippy passed. Three D3D11 curved-fill hardware tests passed, covering 216 curve cases, hole union, label masking, cache stability and the shared 4 MiB upload allowance. Logs: `target/geometry-copper-distance-{regressions,two-board-regression,hardware,clippy}.log`. This does not complete real-board copper/window acceptance.

The largest remaining copper memory cost at this stage was simultaneous CPU source meshes and fully materialized GPU-format vertex/rebased-index vectors. The source-backed implementation below retains an immutable copper provider and converts only bounded upload ranges. Preserve high/residual coordinates, global draw ordering, hole coverage and all source IDs; reducing precision is not an acceptable substitute for removing duplicate storage. The user's subsequent constraint excludes spatial copper subdivision.

## Source-backed copper preparation

`PreparedCopper::build_scene` now retains an immutable `Arc<BoardScene>` plus prefix spans and draw-batch metadata. It validates the same geometry/count constraints as the eager builder, but does not retain another board-wide GPU-format vertex array or rebased index array. `vertex_count`/`index_count` describe logical size; materialized vectors are empty for this mode. Uploads request contiguous blocks, convert the original f64 coordinates to the existing high/residual representation, and rebase original indices exactly as the eager builder does. The common native uploader bounds these temporary blocks with the shared 4 MiB per-frame allowance. Eager custom-pad and curve meshes remain supported.

The desktop uses this path. Telemetry/readiness use logical counts. The CPU external tests use the same source-backed path by default; `POMELO_EAGER_COPPER=1` is a diagnostic test-only comparison with the prior eager representation. No production environment switch was added. Source scene storage remains outside the shared preparation pool; the provider's Arc preserves source lifetime after its original document reference is dropped. The complete cross-document accounting requirement remains open.

Unit checks compare byte-exact high/residual vertices and rebased indices across chunk boundaries, empty zones and reordered layers at large coordinates, including source lifetime after dropping the original Arc. Both real-board D3D11 tests uploaded the complete copper buffers, compared eager/source pixels for board fit and two local views (including mirror), checked nonempty fit coverage, shared upload allowance and stable-view reuse. CPU source-backed buffers reduce residency; GPU allocation is still monolithic and unchanged.

Three alternating eager/source independent-process pairs, using the same release executable and inputs, include default MSDF text, label indexing and picking/search, excluding GPU/window:

| Case | Median peak working set eager / source | Reduction |
| --- | ---: | ---: |
| 15061 | 1,636,974,592 / 1,636,593,664 B | No material peak improvement |
| ntpcb | 4,512,542,720 / 3,020,050,432 B | 1,492,492,288 B (33.1%) |

For 15061 the peak occurs earlier; removing approximately 100 MB of prepared copper does not lower that process peak. Copper owned preparation capacities are 103,201,020 / 2,922,912 B eager/source for 15061 and 1,492,184,392 / 202,232 B for ntpcb. The ntpcb managed geometry total falls to 578,285,044 B, while the borrowed source scene still retains its original exact meshes.

In the one-run hardware upload comparison, 15061 eager/source took 42 / 43 ms and ntpcb 450 / 603 ms to upload identical byte totals. This is an expected conversion-at-upload tradeoff, not a frame-rate measurement or controlled GPU timing. A full upload remains 100,793,916 B / 1,492,017,848 B respectively, with 25 / 356 bounded callbacks. GPU copper allocation is unchanged; spatial subdivision is excluded from further work by the user's constraint.

Raw CPU samples, method and executable hash are in local ignored `docs/gpu-validation/geometry-source-copper-memory.json`; raw per-case/trial logs are `target/geometry-source-copper-{15061,ntpcb_320mb}-{eager,source}-{1,2,3}.{log,err}`. Hardware log: `target/geometry-source-copper-hardware.log`. Desktop tests (74 passed), native render tests, block/lifetime checks and all-target/all-feature Clippy passed. Complete GPUI window interactions, compact trace/pad representation, overview detail levels and disk caching remain open.

## User constraint: no large-copper subdivision

The unintegrated copper-piece metadata and GPU residency experiment was removed. The generic trace cache retains its original fixed-stride accounting. Source-backed copper conversion and original complete meshes remain intact. Further optimization focuses on import cost, duplicate computation, cache reuse and allocation lifetimes. After removal, the default render library suite passed (95 passed, one ignored), including source-backed byte/lifetime equivalence and trace residency regressions.

## Convex triangulation traversal

The convex triangulation fast path now wraps predecessor/successor indices with boundary comparisons instead of variable-divisor remainder operations. It visits exactly the same vertices and emits the same triangles in the same order. Concave/collinear fallback, f64 predicates, cancellation checkpoints and allocation bounds are unchanged. This does not subdivide copper.

Serial release comparisons used separate baseline/changed executables on both source files. Rounds 1–3 ran baseline first; rounds 4–6 ran changed first. Round 1 overlapped a compilation and is retained in raw samples but excluded from the medians. The five remaining pairs use warm uncontrolled OS caches; they measure CPU import only, excluding desktop preparation/GPU.

| Case | Copper stage median before / after | Total CPU import median before / after |
| --- | ---: | ---: |
| 15061 | 2,187 / 2,162 ms | 5,085 / 5,036 ms |
| ntpcb | 3,198 / 3,070 ms (4.0% lower) | 4,400 / 4,288 ms (2.5% lower) |

The ntpcb copper stage improved in every retained pair, including reversed execution order. The approximately 1% difference on 15061 is not counted as a demonstrated benefit. Raw samples, methodology and executable SHA-256 hashes are in local ignored `docs/gpu-validation/geometry-convex-wrap-benchmark.json`; logs are `target/geometry-convex-{baseline,optimized}-{1..6}.log`.

Eleven copper mesh regressions passed, including frozen triangle order for both windings, concave/duplicate-vertex fallback, hole coverage and budgets/cancellation. Both real-board CPU preparation regressions passed, including MSDF text, labels, picking and search (`target/geometry-convex-two-board-regression.log`). Core/import all-target/all-feature Clippy passed. This is a small import improvement, not completion of the overall performance objective.

Both real-board D3D11 copper tests passed after this change (`target/geometry-convex-copper-hardware.log`): source-backed/eager pixels match at board fit and local/mirrored views, and stable views do not reupload. These compare the two storage paths with the current triangulator, not before/after screenshots; the frozen triangle-order regression covers the changed traversal. GPU byte totals remain unchanged. Formatting and diff whitespace checks passed.

## Current paths and ARM MCM acceptance

Both BRD files now reside in `E:/PCB/brd_cases`. Their SHA-256 hashes match the prior measurements: 15061 `5248efdfa6da2d3f4b49431c68c3f0935a0d5d35d480c6daaf4a010229c1cc09`, ntpcb `0f130e3e2b37acbfdbbae85f1e13d521e8fd3c71fb9a0f09e035eecea4f9b1ae`. CPU and native hardware defaults were updated; old raw logs retain their historical paths.

The newly requested `E:/PCB/mcm/ARM_Test.mcm` is 488,308,156 B, SHA-256 `f30913f40b90e22451a515353ca0ffbaf44d708b9f070185f4f04bb77d5e3af3`. The existing Allegro importer builds its scene successfully; the first full CPU test failed with `PICK_INDEX_BYTE_LIMIT actual=578247576 limit=536870912`. It has 1,083,806 vias, 143,776 pins and 83,377 zones. The failure occurred after scene/geometry/text/search preparation, during picking.

Owner hierarchy accounting formerly charged 384 B for every pin/via/zone, independent of the actual entry/tree layout. `BoundsIndex` now reserves entries and its balanced tree once, using the source iterator count and the exact number of tree nodes for its leaf threshold. Picking bounds each owner's hierarchy using these concrete layouts instead of the fixed per-object growth estimate. Budgets are still checked before construction, invalid bounds and cancellation remain errors, and candidate IDs/order are unchanged. No picking budget was raised.

All three external CPU regressions pass under the existing 512 MiB picking ceiling, including text, labels, picking and search (`target/geometry-three-case-picking-regression.log`). A single uncontrolled-cache MCM run completed in 7,186 ms, with 23,796,832 copper vertices, 68,767,962 indices, 3,520,333 pads and 968,523,112 B managed geometry. This is acceptance evidence, not a demonstrated before/after timing or process-memory reduction, because the old complete preparation failed. The MCM test uses optional `POMELO_LARGE_MCM_CASE` for an explicit file override. All core tests and all-target/all-feature Clippy for core/import/render/desktop passed; new hierarchy tests check source-ordered reference candidates and capacity bounds across empty, split-boundary and large input sizes. Complete application/window interaction and comprehensive process/GPU performance measurements remain open for all three cases.

The new MCM D3D11 copper test passed (`target/geometry-arm-mcm-copper-hardware.log`). Source-backed/eager pixels match at fit and local/mirrored views, fit coverage is nonempty, stable views do not reupload, and upload callbacks obey the shared allowance. Owned copper preparation is 665,159,384 B eager versus 11,339,272 B source-backed; original scene geometry is still retained separately. Both upload the same 655,821,160 B to GPU in 157 callbacks. Single-run upload times were 184/283 ms, reflecting conversion-at-upload cost rather than an FPS claim. These are copper component checks, not complete-window rendering or total process peaks.

## Source-read tail capacity

Source reading previously doubled capacity up to the file budget, even near known EOF. For stable inputs the final requested capacities were 1,073,741,824 B for 15061, and 536,870,912 B for both ntpcb and ARM MCM. The reader now caps geometric spare capacity at the metadata length after a successful read. It does not preallocate the full metadata length. If actual bytes exceed that initial length, normal budget-bounded growth resumes; metadata never truncates a growing file. Cancellation/progress and maximum-file-byte checks are unchanged. Tests compare exact bytes for a non-power-of-two file, append data after the metadata read, and verify a subsequent growing read still fails at the file budget.

Three independent-process before/after pairs per case include import, full geometry, default fonts/text/labels, picking and search. Order is reversed in round two; OS caches are warm and uncontrolled. Requested sampling is 10 ms. Kernel working-set peaks and sampled private-byte peaks are reported separately; brief private peaks may be missed.

| Case | Median peak working set before / after (B) | Median sampled peak private bytes before / after (B) |
| --- | ---: | ---: |
| 15061 | 1,637,642,240 / 1,637,105,664 | 2,097,881,088 / 1,708,666,880 |
| ntpcb | 3,020,931,072 / 3,019,882,496 | 3,210,141,696 / 3,137,830,912 |
| ARM MCM | 3,159,396,352 / 3,159,310,336 | 3,578,179,584 / 3,579,203,584 |

Private-byte peak decreased by 389,214,208 B (18.6%) for 15061 and 72,310,784 B (2.3%) for ntpcb. No material working-set reduction was demonstrated for any case, and no MCM process-peak improvement was demonstrated: later preparation dominates its peak. Median process times were 6,104/6,121 ms (15061), 6,337/6,426 ms (ntpcb), and 8,068/7,990 ms (MCM); these do not establish a timing improvement. Spare source capacity and private-byte peak must not be presented as reduced physical residency.

All eighteen real-input test processes passed. Full importer regressions, source/growth/budget tests, importer all-target/all-feature Clippy, formatting and whitespace checks passed. Local ignored report `docs/gpu-validation/geometry-source-growth-memory.json` records all samples, methodology and baseline/changed executable hashes. Per-run logs: `target/geometry-source-growth-{15061,ntpcb,ARM_Test}-{baseline,optimized}-{1,2,3}.{log,err}`. Harness: `target/geometry-source-growth-benchmark.ps1`. This improves source allocation headroom; the overall performance goal and complete window acceptance remain open.

## Allocation lifetime across document close/reload

The previous document-owned preparation controller returned all reservations on document destruction, even when a pending frame, viewport or GPU uploader retained an Arc to prepared data. The managed pool could therefore grant those bytes to a subsequent import before the old buffers were released. Reservations now reside in `PreparedTracks`, `PreparedCopper` and `PreparedPads` as their final fields. `PreparedDrills` delegates the reservation to its owned pad geometry so moving that geometry into an Arc preserves its charge. The controller can drop after preparation; the last geometry owner drops buffers before releasing their reservation. The obsolete boxed controller field was removed from `PreparedDocument`.

The preparation API is sealed to these geometry owners, preventing an arbitrary return value from losing its reservation. Existing stage failure/cancellation and two-controller tests retain live outputs explicitly. A native trace allocation regression drops the controller/document reference, verifies the pending frame still consumes the whole budget and blocks another byte, then releases the last frame and verifies the capacity is available again. All three complete real-input CPU regressions passed (`target/geometry-allocation-lifetime-three-case.log`), as did 74 desktop tests and all-target/all-feature Clippy for core/import/render/desktop. This corrects managed allocation lifetimes; it is not a measured memory/FPS improvement and does not extend the pool to source scene, text, indexes or VRAM.

The native render library suite also passed (100 passed, 38 hardware/external cases ignored by default; `target/geometry-allocation-lifetime-native.log`), including the pending-frame allocation test and existing source-backed copper/trace cache regressions. Formatting and whitespace checks passed.

## GUI crash and three-case rendering acceptance

Actual GUI opening reproduced the ARM MCM crash on the current source build. Windows recorded `0xc0000409` in `ucrtbase.dll`; a temporary panic hook located the cause at `scene/thumbnail.rs:356`: the recent-file CPU thumbnail sliced the empty materialized copper index vector using logical source-backed ranges. The import worker panic crosses the Windows callback boundary and terminates the application. This was missed by the earlier complete CPU preparation and GPU copper component checks, which did not generate the application thumbnail.

Thumbnail rasterization now obtains each admitted batch's indices through the storage-aware accessor and reads individual GPU-format vertices from either storage form. Its existing triangle quota bounds the temporary index conversion; the full copper mesh is not duplicated or subdivided. Pixel work limits, whole-batch hole subtraction and cancellation checks remain intact. A regression compares every RGBA pixel and copper/limited summaries between source-backed and materialized storage, including a nonzero batch offset, intervening empty zone and a hole. The temporary panic hook was removed.

The fixed `target/debug/pomelo.exe` was built and all three files were opened through the native GUI Open dialog in one application instance. ARM MCM reached Ready after its 1,289,991,832 B geometry upload, and close-up vias and network text rendered. 15061 reached Ready and displayed close-up tracks, pads, drills and text. ntpcb reached Ready and displayed the board geometry. All three document tabs remained open; this is real window rendering acceptance, not a frame-rate benchmark or comprehensive interaction coverage. User input also occurred during the observations, so observed zoom levels are not attributed solely to automation.

Thumbnail tests passed (9 passed, 1 external case ignored), the native render library suite passed (101 passed, 38 ignored), and render/desktop all-target/all-feature Clippy passed. Logs: `target/geometry-thumbnail-source-tests.log`, `target/geometry-thumbnail-native-regression.log`, `target/geometry-thumbnail-source-clippy.log`; crash stack: `target/geometry-mcm-gui-panic.log`. Formatting and whitespace checks passed. Existing dist/release executables were not replaced by this debug build.

The user additionally requires ARM MCM Zoom, Pan and highlight performance. Initial CPU canvas picking measurements isolate a substantial copper-query cost: at the board-center point, MCM fit/local medians were 40,666/37,648 microseconds with all picking categories versus 190/6 microseconds with Zone disabled. These are three repetitions of individual CPU queries in an uncontrolled warm process, not GUI latency or FPS. The same experiment gives ntpcb 67,675/81,349 versus 121/72 microseconds. Raw exact hit identities and parameters are retained in `target/geometry-three-case-picking-categories.jsonl`. The next optimization must preserve analytic copper contours, holes and source draw order; large-copper subdivision remains excluded. Frame construction, GPU drawing and highlight costs still need separate attribution and measured before/after GUI acceptance. The overall performance goal remains active.

## Analytic contour bounds for canvas picking

SegmentIndex now caches bounds of each original analytic contour and accounts for both inner bounds buffers and outer vector capacity in the existing picking budget before constructing them. Canvas queries reuse exterior bounds; containment tests omit holes whose analytic bounds exclude the point, and outline-distance queries omit contours outside the tolerance plus rendered half-width. Source paths, meshes, hole union and ordering remain unchanged. This is contour metadata reuse, not subdivision or tessellation of large copper.

Three paired independent release processes per variant and case, reversed order in round two, each repeat five locations at fit/local scales and all/no-zone filters three times. All 540 paired full hit lists match exactly (anchor identity/category/layer, order and distance); OS caches are uncontrolled and warm. Median center-query microseconds:

| Case | Fit before / after | Local before / after |
| --- | ---: | ---: |
| 15061 | 3,656 / 266 | 2,397 / 145 |
| ntpcb | 72,216 / 1,925 | 67,378 / 1,541 |
| ARM MCM | 40,021 / 3,657 | 41,321 / 2,504 |

MCM adds 19,484,608 B requested contour metadata capacity. Its mean index construction time increased from 438,486 to 471,502 microseconds. This trades bounded import-time metadata for repeated query savings; it does not prove a frame-rate improvement. Raw paired data: `target/geometry-picking-{baseline,contours}-round-{1,2,3}.jsonl`; summary: `target/geometry-picking-contours-summary.json`. Core tests and core/import all-target/all-feature Clippy pass. A grid regression compares the optimized path with unpruned analytic queries over circular exterior/hole contours, both fill/outline opacity modes and two scales, retaining exact hit-order/distance bits. All three complete real-case CPU preparation tests still pass with the existing picking ceiling (`target/geometry-picking-contours-three-case.log`). The independently built `pomelo-contour-validation.exe` also opens and renders ARM MCM in the GUI; final Ready/highlight interaction acceptance is still being observed.

## FPS priority and TypeScript reference

The user's primary objective is now frame rate during Zoom, Pan and highlight, with reference repository `C:/Users/Zen/Desktop/gitrepo/pomelo`. Initial inspected paths: `src/lib/render/batch-range-index.ts` builds contiguous ordinary primitive hierarchies with eight-instance leaves; `webgpu-frame.ts` queries those ranges and separates selection/hover batches; `draw-bundle-cache.ts` reuses stable command sequences after a repeated request, excluding copper dynamic stencil and labels; `components/workspace/useRendererView.ts` subscribes only view controls to camera updates. These are concrete reference mechanisms, not measured attribution of the native slowdown. Native `backend/common/pad.rs` currently walks instance ranges and evaluates visibility/highlight per instance for each applicable pass. Profile that CPU work, frame assembly, draw counts and GPU execution before selecting the next rendering change. Preserve the explicit prohibition on large-copper subdivision. CPU picking/import measurements must not be reported as FPS gains. The overall goal remains active.

## Empty compositor overlays and production geometry timing

The contour-cache GUI validation of ARM MCM subsequently reached Ready, with the complete board visible. The next source change skips a compositor Selection pass when all selection sources are empty and skips Hover when both hover sources are absent. Net zero does not activate selection; any direct object/trace, nonempty object set or nonzero net retains selection. Conservative hover activation preserves all target variants. Geometry upload preparation still runs before this decision. Original primitive ordering and copper geometry are unchanged.

New ignored hardware diagnostic `hardware_real_board_frame_profile` prepares the real case and submits through production BoardRenderer at 1280 x 720 after complete geometry upload. It omits glyphs, dynamic labels, text and curves, and does not invoke GPUI/presentation. Two independent release processes per variant, two warmup plus five measured frames per mode, produced the following medians (10 samples). OS caches and other desktop GPU activity were uncontrolled. Readback synchronizes each frame and includes texture copying; neither timing is GUI FPS.

| ARM MCM mode | Submission before / after (ms) | Submission + synchronized readback before / after (ms) |
| --- | ---: | ---: |
| Fit | 363.138 / 198.764 | 367.595 / 201.376 |
| Local x16 | 232.062 / 72.418 | 239.546 / 75.573 |
| Pan at x16 | 228.875 / 69.976 | 238.669 / 73.045 |
| Net selection at x16 | 525.418 / 453.967 | 526.799 / 454.869 |

All four final RGBA images match byte for byte in each pair. Logs `target/geometry-frame-profile-{baseline,empty-overlay}-mcm{,-round2}.log`; summary `target/geometry-frame-profile-empty-overlay-summary.json`. Cargo-run first-pair images are under `crates/pomelo-render/target`, direct-run second-pair images under workspace `target`. Baseline runner preserved as `target/geometry-frame-profile-baseline.exe`. The native suite passed (101 passed, 39 ignored including the new diagnostic); three explicit hardware compositor pixel tests passed, including transparent layering and independent selection/hover at zero base opacity. Render all-target/all-feature Clippy and whitespace checks passed.

The changed desktop binary was built as `target/debug/pomelo-frame-validation.exe`, but its Computer Use launch returned `Computer Use app approval timed out`; a subsequent window inventory confirmed it had not launched. No GUI/frame-rate acceptance is claimed for the overlay change. Earlier crash-fix and contour-cache GUI acceptance remain valid for their respective builds. Next work should reduce nonempty selection and ordinary-pad submission costs and measure actual window frame times. The overall performance goal remains active.

## Ordered visible ranges for analytic pads and drills

Following the TypeScript BatchRangeIndex mechanism, native analytic-pad GPU chunks now retain a hierarchy of double-precision bounds over eight contiguous instances per leaf. Queries emit merged, source-ordered ranges within each original layer/chunk intersection. The shader's two-pixel vertex allowance, double-single coordinate error and viewport rounding are included conservatively. Original geometry, transparency order, visibility predicates and overlay classification remain unchanged. This covers analytic pads and drill geometry, not copper subdivision. Index construction is incremental with the existing GPU upload allowance; one full 4096-instance chunk adds 32 KiB requested CPU bounds capacity. This metadata is part of the GPU cache, outside the preparation allocation pool, and is freed with that cache.

Two independent release processes per variant/case, reversing variant order in round two, run the existing production geometry diagnostic (five measured frames per process/mode). OS caches and desktop GPU activity are uncontrolled; all 24 paired final RGBA images match byte for byte. Medians of ten submission measurements, before / after ordered ranges (ms):

| Case | Fit | Local x16 | Pan at x16 | Net selection at x16 |
| --- | ---: | ---: | ---: | ---: |
| 15061 | 52.584 / 51.720 | 5.351 / 1.992 | 4.699 / 2.005 | 29.558 / 15.854 |
| ntpcb | 69.059 / 56.279 | 48.799 / 9.199 | 45.746 / 9.442 | 96.837 / 32.040 |
| ARM MCM | 199.429 / 212.174 | 70.435 / 24.168 | 70.186 / 24.005 | 461.569 / 97.187 |

The initial range implementation regressed MCM fit submission by about 6.4%. The final source bypasses range pruning when the viewport covers the complete scene bounds, retaining the original contiguous submission in fit views. A subsequent five-frame MCM diagnostic reported fit/local/pan/net medians of 200.199/24.018/23.706/87.001 ms, all four images matching the uncullled baseline. Subsequent final-source 15061 and ntpcb diagnostics also match all four baseline images; concurrent GUI import/upload occurred during those timings, so they do not establish an additional speedup. Fit performance remains a measurement target, rather than a claimed improvement. These diagnostics still exclude GPUI, text, labels, curves and presentation; no GUI FPS is inferred from their reciprocal timings.

Raw pairs: `target/geometry-frame-{pad-baseline,pad-ranges}-{15061,ntpcb,mcm}{,-round2}.log`; summary `target/geometry-pad-ranges-summary.json`; image comparisons `target/geometry-pad-ranges-pixel-comparison.json`. Final-source diagnostics: `target/geometry-frame-pad-ranges-fit-bypass-mcm.log` and `target/geometry-frame-pad-ranges-final-{15061,ntpcb}.log`. Separate ignored hardware pixel oracle compares culled/unculled pads over 4097 instances, view-edge geometry, 9e9 coordinate offsets, flips, DPI 1/2, fill/outline, base/selection/hover and pin filtering. It passes, as do 102 ordinary native tests (40 ignored), three explicit compositor hardware tests and render all-target/all-feature Clippy. Logs `target/geometry-pad-ranges-{native,pixels,compositor,clippy}.log`.

The user now requests reuse of `target/debug/pomelo.exe`, closing the prior instance before each rebuild, to avoid repeated app permissions. This workflow succeeded: the range-index build opened all three cases in a single GUI instance and each reached Ready. ARM MCM VSS selection rendered, zoom changed from 84% to 523%, and a pan drag moved visible geometry while retaining the selected net and hover trace. This is functional interaction evidence, not a latency/FPS measurement. The fit-bypass update was then rebuilt at the same path after closing that instance. All three cases again rendered and reached Ready on that final executable (SHA-256 `6da510f141a72208fe47cdeafb59eeeefa913f450a717d89ef0ed8163116dd52`): 15061 displayed close-up traces, pads, holes and text; ntpcb displayed board geometry; ARM MCM displayed its local view and subsequently the complete board after clearing selection and clicking Fit board. All three tabs remain open in one instance. Window frame construction still lays out labels on camera-key changes synchronously in the prepaint callback (`viewport/mod.rs`), and should be timed separately from native drawing. Actual window FPS, full-board drawing cost and label/frame construction remain open. The overall goal is active.

## Full GUI callback attribution

An opt-in local diagnostic now records viewport prepaint, label layout on cache misses, native BoardRenderer elapsed time and category/pass submission times. Records use a bounded nonblocking channel and a background JSONL writer. The diagnostic marker is supported in debug builds or with the explicit render `frame-profiling` feature; ordinary release startup remains inactive. A marker can request continuous full-window repaint, including the application's normal prepaint work. Removing it latches diagnostics off and restores demand-driven painting without restarting the application. Queue overflow may drop records; there is no dropped-record counter. These measurements include profiling overhead and category timers, and do not establish presented/scanout FPS or input latency.

The optimized release desktop was built with `frame-profiling`, copied to the authorized fixed `target/debug/pomelo.exe` after the old instance closed, and launched through Computer Use. Executable SHA-256: `0dcd48e872428ea7d3569538fa83508ef5812b96a7d508b094a842dd83e39c28`. ARM MCM opened through its GUI recent-file entry and reached Ready; fit, zoom to 619%, pan, VSS selection, and a 523% selected local view rendered. 15061 and ntpcb opened through the native Open dialog in the same instance and reached Ready. This run adds actual GUI diagnostics, not another geometry optimization or a measured before/after gain.

Raw records: `target/geometry-gui-frame-baseline.jsonl`; analysis: `target/summarize_gui_frames.py`; summary: `target/geometry-gui-frame-baseline-summary.json`. Samples are single-process steady tails, 79–100 frames for the listed MCM modes and 100 for other boards. Pointer-hover frames are excluded using the timed Hover category pass. Case identity is determined by source segment/zone counts; visibility and camera values are retained. OS scheduling and other desktop/GPU activity are uncontrolled. The whole-window render target is 2160 x 1380 physical pixels with a smaller canvas. Full-layer MCM means all 40 scene layer IDs visible, including nonconductors, rather than only its 35 imported board layers. Other case samples retain their persisted visibility settings (15061: 10 IDs; ntpcb: 37 IDs).

| GUI state | Native callback median (ms) | Callback cadence median (ms) | Major category medians (ms) |
| --- | ---: | ---: | --- |
| MCM full layers, fit | 192.934 | 197.880 | Copper 127.037; pin 23.736; via 29.059; drill 11.832 |
| MCM full layers, local 619% | 64.344 | 69.162 | Copper 34.512; pin 8.210; via 14.042; drill 6.487 |
| MCM full layers, panned 619% | 62.934 | 66.668 | Copper 33.324; pin 8.253; via 13.904; drill 6.231 |
| MCM full layers, VSS fit 84% | 556.055 | 562.122 | Copper 143.572; pin 59.784; via 243.095; drill 86.007 |
| MCM full layers, VSS local 523% | 219.488 | 224.820 | Copper 47.438; pin 24.264; via 92.814; drill 36.886 |
| 15061 persisted layers, fit | 49.395 | 53.457 | Copper 32.471; pin 14.124 |
| ntpcb persisted layers, saved 70% | 57.237 | 61.399 | Copper 2.005; pin 20.380; via 32.281 |

MCM TOP-only local 299% native/cadence medians are 14.223/18.542 ms, with 10.572 ms in drill submission. At TOP-only fit they are 31.082/35.589 ms, with 23.432 ms in drills. Its drill scope predicate produces many disjoint visible source ranges when other layers are hidden. By contrast, full-layer ordinary fit performs 251,011 copper draw calls per frame and only 67 drill calls; full-layer VSS fit performs 668,306 analytic pad calls and 199,502 drill calls per frame. Cumulative telemetry is differenced between matching frames. These counts and category costs distinguish ordinary copper submission from selection fragmentation, rather than attributing every slowdown to large copper size.

Measured MCM label layout cache misses range from approximately 3 to 18 ms in the observed camera/display changes; stable-camera layout reuses the cache and reports zero. This is a small set of individual cache-miss observations, not a camera-motion benchmark. It adds cost during interaction but does not explain the 193/556 ms static native callbacks.

Next candidates are preserving original order while coalescing equivalent native draw ranges, caching drill scope visibility when display settings are unchanged, reducing selection range fragmentation, and reducing D3D11 state/uniform work per draw. The TypeScript renderer also coalesces contiguous hole-free unannotated solid zones and directly addresses zone-owned labels. Each needs native stencil/opacity/order validation; none of these further optimizations is implemented or claimed by this profiling change. Large copper subdivision remains excluded. Marker removal was verified after sampling; the app and three tabs remain open, with MCM restored to TOP-only display. Native tests pass (102 passed, 40 ignored), and render/desktop all-target/all-feature Clippy passes (`target/geometry-gui-profile-native-tests.log`, `target/geometry-gui-profile-clippy-final.log`). The overall FPS goal remains active.

## Adaptive single-net analytic-pad selection

The native pad/drill pipeline now avoids per-instance selection classification and fragmented submissions for a single nonzero selected net when there are no other selected objects, traces or nonempty selected-object sets. A chunk retains a scalar summary (net ID, selected count and contiguous run count) for the most recently requested net. Changing the net recomputes it once; empty chunks are omitted. Dense fragmented chunks (at least 128 selected instances and 32 runs) preserve their original instance order in a single visibility range, with vertex-shader mode 2 collapsing nonmatching-net quads outside the clip volume. Sparse chunks retain the CPU classification path. Object/mixed selection and hover semantics are unchanged. This adds no geometry or selection GPU buffers; each existing analytic upload chunk gains only scalar CPU metadata. Copper geometry is unchanged and not subdivided.

The first version applied GPU filtering to every eligible chunk. Two real-case process pairs found ntpcb net submission regressed from 31.440 to 37.080 ms despite identical pixels. This motivated the cached empty-chunk rejection and density/run policy above. Initial logs remain under `target/geometry-pad-net-{baseline,net}-*-round*.log`, with initial baseline logs preserved as `geometry-pad-net-baseline-*-initial-round*.log` and summary `geometry-pad-net-three-case-summary.json`. Subsequent paired baseline runs replace only the baseline round logs; all image hashes are retained in the initial summary.

Final source was measured in two independent release processes per variant/case, reversing order in round two. Each mode has two warmup and five measured frames per process. All 24 paired final RGBA images match byte for byte. These production geometry measurements exclude GPUI, text, labels, curves and presentation. OS/desktop activity and cache effects are uncontrolled; ordinary-path fluctuations are not claimed as gains.

| Case | Fit before / after (ms) | Local x16 | Pan at x16 | First-via-net selection x16 |
| --- | ---: | ---: | ---: | ---: |
| 15061 | 49.111 / 49.769 | 1.291 / 1.288 | 1.250 / 1.240 | 10.643 / 10.493 |
| ntpcb | 59.591 / 56.917 | 9.004 / 9.322 | 9.058 / 9.505 | 32.003 / 28.646 |
| ARM MCM | 197.578 / 206.208 | 24.854 / 23.548 | 23.943 / 23.506 | 92.408 / 62.982 |

MCM fit submission is 4.4% higher in this small geometry sample; first-round fit medians were 205.246/206.198 ms, while second-round medians were 193.059/209.011 ms. This remains a performance check rather than an ordinary-view improvement claim. Full GUI ordinary fit must be assessed separately. Raw final pairs: `target/geometry-pad-net-{baseline,adaptive}-{15061,ntpcb,mcm}-round{1,2}.log`; summary: `target/geometry-pad-adaptive-three-case-summary.json`. Harness executables retain the pre-change runner `geometry-frame-profile-pad-ranges-final.exe` and final runner `geometry-frame-profile-pad-net-adaptive.exe`.

The hardware pad pixel oracle now explicitly forces the reference cache to the original CPU selection path and compares net 1, net 2, net 1 again and net zero, with/without an additional selected pin. It retains viewport edges, 4097 instances, large coordinate residuals, DPI, flip, fill/outline, pin visibility and compositor passes, and asserts reduced submissions for the pure-net cases. It passes (`target/geometry-pad-net-adaptive-pixels.log`); native tests pass (102 passed, 40 ignored, `geometry-pad-net-adaptive-native.log`), and render/desktop all-target/all-feature Clippy passes (`geometry-pad-net-adaptive-clippy.log`). HLSL stages compile on Windows. Metal/WGSL counterparts receive the same mode-2 rejection condition, but their platform compilers were not run on this Windows host.

The final optimized release executable was copied to the authorized `target/debug/pomelo.exe` after closing the previous GUI instance, and launched through Computer Use (SHA-256 `911f5fd01b52a29df351bed2cf8f1afcfa17cadcbe279c091d5221e2a544676a`). ARM MCM reached Ready and rendered full layers, selected nets and local views. User input also changed zoom, pan, layer visibility and selected nets during observation; those interactions are not attributed to automation. Matching full-layer, zero-pan steady views were compared against the preceding GUI baseline, using 100 final frames and 79–100 baseline frames after excluding timed hover passes. Whole-window target size is 2160 x 1380 physical pixels.

| Matching MCM GUI state | Native callback before / after (ms) | Callback cadence before / after (ms) |
| --- | ---: | ---: |
| Ordinary fit, 5.312888743 pixels/mm | 192.934 / 193.631 | 197.880 / 197.430 |
| VSS selection fit, 4.485378028 pixels/mm, net 151102 | 556.055 / 290.877 | 562.122 / 296.239 |

For matching VSS fit, analytic pad calls drop from 668,306 to 502 and drill calls from 199,502 to 154 per frame. Copper calls remain 281,540. Via submission drops from 243.095 to 53.052 ms and drill submission from 86.007 to 22.502 ms; copper remains about 144 ms. Ordinary fit is unchanged in this GUI sample. These are native callback timings and callback cadence, including diagnostic overhead, not presented FPS or input latency. Unmatched local views and different selected nets do not establish a before/after comparison. Raw records and analysis: `target/geometry-gui-frame-pad-net.jsonl` and `geometry-gui-frame-pad-net-summary.json`, compared with the preceding `geometry-gui-frame-baseline-summary.json`. The marker was removed after sampling to restore demand-driven painting. Large copper remains intact; ordinary copper submission is still an open performance target.

After the user explicitly authorized continued GUI verification, both BRD cases were opened through the native file dialog in this same final executable. 15061 reached Ready (8 layers, 6,848 components, 7,484 nets) and rendered its fitted full board. ntpcb reached Ready (34 layers, 12,850 components, 12,299 nets); Fit board centered and rendered the complete board at 100%. Returning to ARM MCM retained its current local view and Ready status while all three tabs remained open. No crash occurred during this validation. Diagnostics were already disabled, so these two BRD checks establish functional import/render acceptance, not new GUI timing measurements. The overall FPS goal remains active.

## Direct and coalesced hole-free copper submission

Following TypeScript `webgpu-frame.ts`, native copper now draws hole-free earcut exteriors directly when they have no embedded annotations. Contiguous original index ranges are coalesced only when they use the same uploaded cache and identical camera, clipping, color, opacity, pass and shape-pattern uniforms. Indexed copper does not read the clear-quad rectangle uniform; its run uses the union of original conservative pixel scissors. Original index/primitive order and source-over blending are retained. Geometry is neither copied nor subdivided. Holed, parity-ring and annotated copper keeps the original private stencil sequence; each private scope ends before the next direct run so stale zone masks are not inherited. Direct runs retain an already-active caller stencil.

GUI zone-label owners are resolved once per frame from the current prepared glyph source, using a scalar-ID set. Only those owners enter the annotation path; an unspecified owner set conservatively retains annotation callbacks for every batch. No new GPU buffers are created. The hardware oracle forces the reference upload cache through the original stencil path and compares exact RGBA output for interleaved solid/holed/annotated copper, overlapping voids, merged same-material runs, translucent colors, shape stipple, net styling, base/selection/hover, view-edge clipping, 9e9 coordinate residuals, DPI 1/2 and flips. It also checks annotation order, visible-zone counts and reduced draw calls. The oracle, three compositor hardware tests and the full-coverage selected-zone label test pass. Final-source native tests pass (102 passed, 41 ignored), and render/desktop all-target/all-feature Clippy passes. Logs are `target/geometry-copper-direct-{pixels,compositor,labels,native-final,clippy}.log`.

Three curve hardware tests also pass (`geometry-copper-direct-curves.log`), including 216 disjoint-island/void-union/analytic-coverage cases and clipped zone labels. The final release desktop build reuses `target/debug/pomelo.exe` after closing the prior instance. SHA-256 is `14c83b2045cc55e9a09e953c1943c16270a92005f3b274ddbbeacd7648e64f0d`. Computer Use opened ARM MCM through its recent-file entry, and it reached Ready. Full-layer fit, zoom to 619%, pan, VSS fit selection, selected zoom to 523% and selected pan all rendered.

The final GUI diagnostic uses `target/geometry-gui-frame-copper-direct.jsonl` and its `-summary.json`. Matching zero-pan MCM views retain all 40 scene layer IDs and the previous 2160 x 1380 target. At ordinary fit (5.312888743 pixels/mm), 100 final frames give native/cadence medians of 80.082/84.870 ms, versus 193.631/197.430 ms in the preceding adaptive-pad GUI build. Copper submission is 12.658 ms and 16,361 calls per frame, versus approximately 127 ms and 251,011 calls previously. At VSS fit (4.485378028 pixels/mm, net 151102), native/cadence medians are 165.594/170.760 ms over 100 frames, versus 290.877/296.239 ms previously. Ordinary local/panned views are about 39 ms and selected local about 99 ms, but their camera centers differ slightly from the earlier GUI measurements; no exact local before/after comparison is claimed. Callback timings include diagnostics and are not presented FPS or input latency. Pads/vias/drills now dominate the remaining ordinary full-board cost. The overall goal remains active.

Both BRD files subsequently opened through the native file dialog in this same final executable and reached Ready, rendering their complete fitted boards. Matching 15061 fit camera and 10 visible layer IDs give a final 100-frame native callback median of 6.885 ms, versus 49.395 ms in the earlier GUI baseline. Its last sampled callback cadence also includes background import of ntpcb, so no steady presented-FPS comparison is derived from it. ntpcb fit with 37 visible IDs gives 44.175 ms; its camera differs from the earlier persisted 70% GUI view, so this is a final-state measurement rather than an exact GUI before/after pair. The diagnostic marker was removed and its JSONL stopped growing. All three tabs remain open without a crash; MCM has its prior TOP-only visibility restored, no test selection, and a fitted full-board view.

The geometry harness compares the preceding adaptive-pad runner with final direct/coalesced copper in two independent process pairs, reversing order in round two. First-round BRD measurements overlapped the desktop release compiler and showed a 15061 net timing outlier. Those affected pairs were repeated after compilation and GUI sampling finished, with diagnostics disabled. Original logs are retained as `geometry-copper-*-{15061,ntpcb}-round1-during-build.log`, and the original medians/image hashes are in `geometry-copper-direct-initial-three-case-summary.json`. MCM pairs did not overlap that compilation and were retained. Final paired medians (ten samples per mode/variant) are below; all 24 paired RGBA images are exact matches.

| Case | Fit before / after (ms) | Local x16 | Pan at x16 | First-via-net selection x16 |
| --- | ---: | ---: | ---: | ---: |
| 15061 | 48.283 / 6.275 | 1.327 / 1.388 | 1.309 / 1.341 | 10.571 / 10.285 |
| ntpcb | 55.696 / 42.878 | 9.220 / 9.787 | 9.312 / 9.642 | 28.604 / 29.393 |
| ARM MCM | 191.559 / 78.971 | 23.871 / 21.630 | 24.293 / 20.987 | 63.299 / 60.603 |

Small local/net fluctuations on the BRD cases are not claimed as gains; ntpcb local is about 6.1% higher in this sample, while its fit is about 23% lower. The harness excludes GPUI, labels, curves and presentation, and OS/GPU activity remains uncontrolled. Final logs/images are `target/geometry-copper-{baseline,direct}-{15061,ntpcb,mcm}-round{1,2}`; summary is `geometry-copper-direct-three-case-summary.json`. The hardware proofs above validate Windows D3D11; Metal/wgpu platform execution was not performed on this host. Remaining work includes ordinary pad/drill classification and fragmented scope visibility, group-hover submission, and measuring interaction/presentation beyond steady callback time.

## Cached analytic-pad and drill visibility

Native analytic chunks now lazily retain compact accepted-instance bitmaps for the four pin/via and backdrill-display combinations. Drill acceptance is cached against a retained immutable `Arc<BoardDisplay>`; changing layer visibility, Via category visibility or drill/backdrill switches changes that snapshot and rebuilds acceptance. Retaining the Arc also makes `Arc::make_mut` allocate a new snapshot rather than mutate cached settings in place. A changed geometry source replaces the upload cache and all of these masks. New upload chunks initialize their masks independently. Camera changes intersect cached acceptance with the existing ordered spatial ranges. Arbitrary predicate callers retain uncached evaluation.

Bitmaps use one bit per instance per requested filter (2 KiB per full 16,384-instance upload chunk), with at most four pad masks and one drill mask per chunk. They belong to native GPU-cache CPU metadata, outside the preparation allocation pool, and are released with that cache. No geometry or GPU buffers are added. Accepted runs cross 64-bit word boundaries and preserve original instance order. When a base pass has no object/set/hover color override, or a dense single-net shader pass has a uniform highlight, the renderer submits each accepted span directly instead of reclassifying every instance. Sparse or mixed selection and hover retain the existing classification semantics.

Bit-range unit tests cover empty/all/sparse acceptance, 64-bit boundaries, subranges and partial chunks. A hardware oracle compares the typed cached scopes with explicit original predicates while changing visibility snapshots, showing/hiding Via categories, normal drills, backdrills and replacement pads, at two zooms and base/selection/hover with and without selected nets. It includes partial upload and a subsequent 16,385th instance. The earlier pad-range oracle now forces its reference through uncached visibility and original per-instance base classification. Both exact pixel comparisons pass, as do backdrill and three compositor hardware tests, 103 ordinary native tests (42 ignored), and render/desktop all-target/all-feature Clippy. Logs are `target/geometry-pad-visibility-{pixels,range-pixels,backdrills,compositor,native-final,clippy}.log`. This cache does not yet remove fragmented visibility draw calls; it removes their repeated predicate work.

Two independent process pairs per case compare the preceding direct-copper runner with the cached-visibility runner, reversing variant order in round two. Desktop compilation finished before these runs; no GUI instance was running. Each mode has ten measured samples and all 24 paired final RGBA images match byte for byte. Submission medians (before / after, ms):

| Case | Fit | Local x16 | Pan x16 | First-via-net selection x16 |
| --- | ---: | ---: | ---: | ---: |
| 15061 | 6.098 / 2.244 | 1.308 / 1.183 | 1.278 / 1.142 | 10.593 / 10.099 |
| ntpcb | 42.717 / 2.208 | 9.900 / 5.237 | 9.670 / 5.217 | 28.968 / 24.756 |
| ARM MCM | 77.813 / 11.497 | 20.499 / 12.572 | 20.853 / 12.851 | 60.518 / 46.168 |

These geometry submissions exclude GPUI, labels, curves and presentation; synchronized texture readback also remains in raw records, not inferred as presented FPS. Logs/images: `target/geometry-pad-visibility-{baseline,cached}-{15061,ntpcb,mcm}-round{1,2}`; summary: `geometry-pad-visibility-three-case-summary.json`. Harness baseline `geometry-frame-profile-copper-direct.exe` and final runner `geometry-frame-profile-pad-visibility.exe` are retained. The old GUI instance was closed before replacing and launching the fixed `target/debug/pomelo.exe`; its final SHA-256 is `5334d67ba8db272dd97ada75dce7bf46b93ac854c804e8f38ce652c51f03292c`.

After explicit authorization to continue GUI verification, this final executable rendered MCM with all 40 scene layer IDs, zoomed to 619%, panned, and selected VSS (net 151102). Both BRDs were then opened through the native file dialog and reached Ready with complete fitted boards: 15061 has 8 board layers, 6,848 components and 7,484 nets; ntpcb has 34 board layers, 12,850 components and 12,299 nets. All three tabs remain open in the same process without a crash during this validation. User interactions also appear in the diagnostic log; they are not attributed to automation. A final attempted tab switch was withheld when user input was detected, preserving the user's current ntpcb local view.

Exact camera, visible-layer-ID and selected-net matches against the preceding direct-copper GUI summary give these medians over the final 100 non-hover frames in each state. The physical whole-window target remains 2160 x 1380.

| Matching GUI state | Native callback before / after (ms) | Callback cadence before / after (ms) |
| --- | ---: | ---: |
| MCM full-layer fit | 80.082 / 14.134 | 84.870 / 17.979 |
| MCM ordinary local, 32.896006739 pixels/mm | 38.686 / 18.921 | 43.230 / 23.209 |
| MCM ordinary pan at the same scale | 38.683 / 19.686 | 42.274 / 23.123 |
| MCM VSS fit, 4.485378028 pixels/mm | 165.594 / 43.444 | 170.760 / 48.037 |
| 15061 fit, 10 visible IDs | 6.885 / 2.788 | Background import; no cadence comparison |
| ntpcb fit, 37 visible IDs | 44.175 / 2.743 | 48.230 / 16.706 |

The 15061 cadence includes subsequent background ntpcb import, so it does not establish steady display performance. Other callback cadences also include diagnostic overhead and do not measure presented FPS or input latency. In MCM VSS fit, zone fill and outline still take about 19.0 and 17.9 ms; selected trace submission takes about 5.0 ms. Ordinary MCM fit is now dominated by zone submission (about 12.7 ms). Raw records and summaries are `target/geometry-gui-frame-pad-visibility.jsonl` and `geometry-gui-frame-pad-visibility-summary.json`, compared with `geometry-gui-frame-copper-direct-summary.json`. The diagnostic marker was removed after sampling to restore demand-driven painting. Overall performance work remains active: group hover, fragmented visibility calls, selected copper/outline submission and interaction/presentation measurements remain open. Large copper is still intact.

## Copper batch indices per layer

Native copper previously scanned every source batch for each layer command. UploadedCopper now retains original batch indices grouped by layer, built once with fallible vector reservation. Per-layer drawing visits only that layer's original batches, in their original order; an absent layer visits none, and an unspecified layer retains the full source traversal. Override lookup, visibility predicates, upload readiness, coalescing, holes, annotations and scissor rules remain in the same downstream path. This adds one usize per batch plus per-layer map/vector metadata to native GPU-cache CPU metadata outside the preparation pool; no geometry or GPU buffers are copied or subdivided. Source replacement builds a new index.

The hardware copper oracle includes a second layer interleaved in the original source and draws all layers, layer 0, layer 1 and missing layer 99. Its forced-stencil reference also retains the original unindexed traversal. Exact RGBA, annotation order and visible counts match through opacity, base/selection/hover, large coordinates, DPI and flips. The compiled snapshot passes 103 ordinary native tests (42 ignored), the expanded hardware oracle and render all-target/all-feature Clippy (`geometry-layer-index-{native,pixels,clippy}.log`). Retained test runner `target/geometry-frame-profile-layer-index.exe` SHA-256: `a676c063118fbed75587b8308cb850ca06e85d7cf012d7ad6e1243f1a20b3eb2`.

Two process pairs per case reverse baseline/indexed order in round two; ten samples per mode/variant give these geometry submission medians, with all 24 paired final RGBA images identical:

| Case | Fit before / after (ms) | Local x16 | Pan x16 | First-via-net selection x16 |
| --- | ---: | ---: | ---: | ---: |
| 15061 | 3.138 / 2.820 | 1.633 / 1.378 | 1.432 / 1.398 | 13.163 / 12.727 |
| ntpcb | 2.734 / 1.859 | 5.488 / 6.320 | 6.640 / 5.900 | 28.699 / 28.363 |
| ARM MCM | 13.707 / 8.529 | 14.683 / 8.545 | 13.435 / 8.520 | 48.456 / 37.685 |

The existing GUI remained open, diagnostics disabled, and the user continued interacting with it. These samples therefore have uncontrolled desktop/GPU contention and do not establish isolated performance acceptance. In particular ntpcb local submission is higher in this sample; repeat without concurrent interaction before making a regression judgment. Logs/images: `target/geometry-layer-index-{baseline,indexed}-{15061,ntpcb,mcm}-round{1,2}`; summary: `geometry-layer-index-three-case-summary.json`.

After this runner compiled, another task modified copper styling, trace submission, compositor and viewport files; the user confirmed that concurrent task. Those edits are preserved. The above tests and measurements prove only the retained compiled snapshot, not the later combined working tree. No GUI executable was replaced in this iteration. The prior verified fixed-path GUI remains running. Final combined compilation, isolated measurements and GUI verification of all three cases remain required; the overall goal stays active.

## Cached analytic-pad overlay membership and combined GUI validation

Selection and hover now retain accepted-instance bitmaps per uploaded analytic-pad chunk, separate from camera and display visibility. Each chunk has at most two overlay slots: selection and hover/group-hover. Keys include selected object/net/trace IDs, hovered object and target, and retained immutable Arc identities for selected-pin, related-object and hover-member sets. Arc retention preserves copy-on-write invalidation without comparing large sets every frame. Changed geometry replaces the upload cache; newly uploaded chunks build their own acceptance. Ordered spatial and display ranges intersect these masks before drawing accepted spans in original order. Dense pure-net selection keeps the existing shader path. No large copper is subdivided, and no geometry or GPU buffers are added; each populated overlay mask adds one bit per instance (2 KiB per full 16,384-instance chunk), outside the preparation allocation pool.

The new hardware oracle compares original scalar classification with the cached path across 13 selection/hover states, repeated frames, copy-on-write set mutations, partial upload and a subsequent 16,385th instance. All 156 paired RGBA comparisons pass. The existing pad range and display-visibility oracles, expanded copper oracle, three compositor tests and two concurrent-task copper-highlight hardware tests also pass. The combined renderer passes 103 ordinary native tests (37 ignored) and all-target/all-feature Clippy with warnings denied. Logs: `target/geometry-overlay-{invalidation-pixels,range-pixels,visibility-pixels,compositor,zone-pixels,baseline-pixels,native,clippy}.log`.

The paired runner baseline already includes the layer index and concurrent-task copper styling. Two reversed-order process pairs per case, with two warmups and five samples per mode in each process, compare only the additional pad overlay cache. GUI and compilation were stopped during the initial paired runs. All 36 paired final RGBA images match byte for byte. Geometry submission medians (before / after, ms):

| Case | Fit | Local x16 | Pan x16 | Net selection x16 | Net hover fit | Net hover x16 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 15061 | 1.971 / 2.010 | 0.957 / 0.993 | 0.909 / 0.912 | 9.536 / 9.770 | 20.990 / 18.772 | 9.804 / 9.614 |
| ntpcb | 1.821 / 2.104 | 4.475 / 5.320 | 4.509 / 5.524 | 22.930 / 23.146 | 36.607 / 17.330 | 23.757 / 22.262 |
| ARM MCM | 8.579 / 8.573 | 8.202 / 8.568 | 8.066 / 8.450 | 36.680 / 37.662 | 324.214 / 302.323 | 60.974 / 59.112 |

ntpcb ordinary timings varied between rounds. A further two reversed-order pairs after disabling GUI diagnostics, with the GUI idle, produced fit 2.568 / 1.748, local 4.551 / 4.543, pan 6.121 / 4.541, net 28.532 / 21.803, hover fit 38.692 / 16.962 and hover local 28.694 / 21.391 ms; all 12 repeated paired images are identical. The local slowdown did not reproduce consistently, and ordinary-path speedups are not attributed to this cache. Desktop/GPU activity remains uncontrolled. The harness excludes GPUI, labels, curves and presentation; its hover target is a pure net group without a hovered-object base-color override. Original measurements remain in `geometry-pad-overlay-three-case-summary.json`; repeats in `geometry-pad-overlay-ntpcb-repeat-summary.json`, with corresponding `target/geometry-pad-overlay-*` logs/images. Baseline runner SHA-256 is `F4537460AF7D6232EFFB5A293CC2B315F4A2BBD05AB51FA397C737883B5FFD5C`; cached runner is `DC67471C04B1C2750AAB1B2C46124FFCA59912DE5652369044A29AD5D0950D75`.

The combined release executable was built successfully and copied to the same `target/debug/pomelo.exe` after closing the preceding GUI instance. SHA-256: `8D96C48914805FEAE72789B5AA2726A1E5FA93A3FAD33FF40C6DEFEADC28DCE9`. GUI validation opened all three files through the native dialog and rendered them to Ready without a crash during these checks. MCM was tested with all 40 scene layer IDs, fit, 619% zoom, pan and VSS selection. PAN suppressed hover. Both BRDs rendered fitted boards; their persisted visibility/net states differ from earlier full-layer measurements, so those GUI timings are not used as paired performance evidence. Concurrent import/encoding edits made after this build are preserved and are not proved by this executable.

At a 2160 x 1380 physical target, MCM ordinary full-layer fit had a 9.245 ms native callback median; local zoom 13.749 ms, pan 14.325 ms and VSS fit 32.542 ms. These are combined results, including changed copper highlight styling, rather than isolated overlay-cache gains. Actual GUI hover over Via 1478050 at 4.505869222 pixels/mm highlighted a dense net group. Its last 100 frames had a 288.993 ms native callback median and 292.818 ms callback cadence. Category medians were via 181.142 ms, drill 61.210 ms, pin 15.570 ms, zone outline 16.356 ms, zone fill 8.946 ms and trace 4.738 ms. Per-frame submissions included 668,306 pad and 199,502 drill calls. This demonstrates a remaining fragmented-submission bottleneck despite cached membership; hover responsiveness is not fixed. Raw diagnostics and ordinary/hover summaries are `target/geometry-gui-frame-pad-overlay*`. The diagnostic marker was removed, continuous diagnostic repainting stopped, hover cleared and all three GUI tabs left open. Callback cadence is not presented FPS or input latency. Overall performance work remains active; next work must reduce dense-net hover submissions while preserving original geometry, ordering and pixels.

## Dense network hover in the analytic-pad GPU path

The existing adaptive net-only selection path now also serves net hover/group-hover when there is no active selection exclusion. It reuses the same mode-2 pad shader and original instanced ranges; nonmatching network quads collapse before rasterization. A hovered pointer object is checked in each chunk: if it contributes any instance outside the network, that chunk retains cached exact membership rather than dropping the extra object. Empty matching chunks skip submission only if they also contain no extra pointer object. Zero nets, mixed selections and non-net hover groups retain the exact path. The net summary retains pointer identity as well as net identity, and source replacement/partial upload preserve cache lifecycle. Display predicates and ordered camera ranges still intersect before GPU submission. No shaders, geometry, GPU buffers or large copper subdivisions are added.

TS `overlay-controller.ts` reuses stable group members and prepared hover packets; `webgpu-frame.ts` submits the resulting ordered batches. The Rust change follows the same principle of avoiding per-object hover submissions while retaining the existing uploaded geometry. The hardware overlay oracle now covers 19 states and three category scopes, including dense pure-net hover, a same-net pointer, an extra different-net pointer, mixed selected-net exclusion, zero/missing nets, repeated keys and partial upload. All 684 paired RGBA comparisons pass, and dense hover explicitly asserts fewer draws than original scalar classification. The range/large-coordinate/DPI/flip oracle also passes, along with 103 ordinary native tests (37 ignored), render all-target/all-feature Clippy, and the release GUI build. Logs: `target/geometry-hover-net-{overlay-pixels,range-pixels,native,clippy,gui-build}.log`.

Two reversed-order process pairs per case compared the preceding overlay-cache runner with the new GPU-hover runner, after our compilation and old GUI process had stopped. Each mode has ten measured samples; all 36 paired RGBA images are identical. External desktop activity was not controlled; a separate rustc process was briefly observed near the end of round two, so small ordinary-path changes are not claimed as gains. Geometry submission medians, before / after (ms):

| Case | Fit | Local x16 | Pan x16 | Net selection x16 | Net hover fit | Net hover x16 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 15061 | 2.011 / 2.066 | 0.954 / 0.968 | 0.934 / 0.923 | 9.524 / 9.637 | 18.568 / 10.918 | 9.297 / 9.351 |
| ntpcb | 1.877 / 1.943 | 4.556 / 4.659 | 4.570 / 4.649 | 21.819 / 22.134 | 17.450 / 16.943 | 21.725 / 21.146 |
| ARM MCM | 8.775 / 8.251 | 8.711 / 8.541 | 8.712 / 8.209 | 38.140 / 37.645 | 301.471 / 30.876 | 57.841 / 37.155 |

The harness still excludes GPUI, labels, curves and presentation, and its net hover does not set a pointer object. Logs/images are `target/geometry-hover-net-{baseline,gpu}-{15061,ntpcb,mcm}-round{1,2}`; summary is `geometry-hover-net-three-case-summary.json`. The new runner SHA-256 is `C00CFB050AC248621CCC35EBEB95EF7E8C9A42741BD30C5B695C40E8EEC4FBD2`.

The same fixed `target/debug/pomelo.exe` was replaced after closing the old process, then launched via GUI. SHA-256: `E75AE1048AC099499D5BFC82648A2B075E0C3515B7FBD7688816793CBDA0C90D`. Actual GUI hover over the same Via 1478050 at the exact prior camera, 40 visible scene IDs, no selected net and 2160 x 1380 target confirms the improvement: last-100-frame native callback median 32.794 ms versus 288.993 ms, callback cadence 37.484 versus 292.818 ms. Pad draws fell from 668,306 to 502 per frame and drills from 199,502 to 154. Pin/via/drill category medians fell to 0.251 / 0.505 / 0.170 ms. The remaining zone-outline median is 16.900 ms, zone fill 9.096 ms and trace 4.835 ms. Diagnostics and summaries: `target/geometry-gui-frame-hover-net*`. The same GUI rendered MCM, zoomed to 525%, panned with hover suppressed and selected VSS; ordinary fit was 9.116 ms, ordinary local/pan about 15.4 ms, and VSS fit 33.272 ms. This establishes reduced submission cost, not presented FPS, interaction latency or a complete fix for all highlight states. The overall goal remains active, with copper/trace outline submission and mixed selection hover still requiring work.

Final GUI checks also imported both BRDs through the native dialog and displayed complete fitted boards: 15061 reached Ready with 8 board layers, 6,848 components and 7,484 nets; ntpcb with 34 board layers, 12,850 components and 12,299 nets. Their saved states each have one visible scene ID, so those checks establish functional rendering rather than full-layer performance comparisons. All three tabs remain open in the same new process (PID 27384), without a crash during validation. The pointer and VSS selection were cleared, MCM left in ordinary PAN mode, and the diagnostic marker removed to stop continuous sampling. Concurrent user-task modifications were retained.

## Cached trace and boundary overlay membership

UploadedTracks now lazily caches two accepted-instance bitmaps against the same immutable selection keys used by analytic pads: one selection slot and one hover/group-hover slot. Each bitmap follows the complete immutable source order; upload and spatial-residency chunks draw only their intersecting accepted ranges. A source replacement drops its masks. Retained Arc keys preserve copy-on-write invalidation of member sets. Camera, display scope and style changes retain the membership mask; category predicates and MSDF atlas pages still split accepted spans at draw time. Overlay color is uniform for each pass, so ordinary trace/boundary ranges avoid per-instance reclassification after acceptance is cached. Outline batches remain excluded from overlays, and each original span's first owner still determines white zone-boundary styling. The scalar test reference bypasses both membership caching and this fast path.

This adds at most two bits per source instance plus immutable keys to native-cache CPU metadata, outside the preparation pool. First use or a changed membership key scans the full source; stable frames reuse the mask, including after residency eviction/reupload. No geometry, GPU buffers, shaders or large copper subdivisions are added. Pointer identity remains part of the hover key, so moving across objects may rebuild membership even within the same net; that transient interaction cost remains to be measured and optimized.

The new hardware oracle checks 1,440 paired RGBA images and identical draw counts across ten changing selection/hover states, six scopes, repeated calls, partial upload, eager upload and spatial residency. It includes copy-on-write member changes, typed segment/zone/pin/via/drawing owners, track selection, layer scopes and outline suppression. Copper white-outline and sparse-dot/hole tests and the mixed-batch label-opacity test also pass. Native tests: 103 passed, 38 ignored; render all-target/all-feature Clippy passes with warnings denied. Release GUI build succeeds. Logs: `target/geometry-trace-overlay-{invalidation-pixels,outline-pixels,zone-pixels,label-pixels,native,clippy,gui-build}.log`.

Two reversed-order process pairs per case compare the prior GPU-pad-hover runner with the new trace-overlay cache after our compilation and GUI process stopped. Ten samples per mode/variant, all 36 paired final RGBA images identical; external desktop/GPU activity remains uncontrolled. Geometry submission medians, before / after (ms):

| Case | Fit | Local x16 | Pan x16 | Net selection x16 | Net hover fit | Net hover x16 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 15061 | 2.089 / 2.125 | 1.043 / 1.090 | 0.928 / 1.005 | 9.687 / 3.933 | 10.844 / 7.337 | 9.485 / 3.950 |
| ntpcb | 1.829 / 1.827 | 5.254 / 4.699 | 5.326 / 4.733 | 21.719 / 5.555 | 16.776 / 2.062 | 21.271 / 5.583 |
| ARM MCM | 8.937 / 8.500 | 9.274 / 8.758 | 8.961 / 9.019 | 40.852 / 18.592 | 33.364 / 12.622 | 38.213 / 18.843 |

Small ordinary-path fluctuations are not attributed to the overlay cache. The harness excludes GPUI, labels, curves and presentation, and does not set a pointer object for its net hover. Logs/images: `target/geometry-trace-overlay-{baseline,gpu}-{15061,ntpcb,mcm}-round{1,2}`; summary: `geometry-trace-overlay-three-case-summary.json`. New runner SHA-256: `D8036FCC42AA9B767D984B436F6AFA20CF962A31ED2E4A45CBA4274E36CE8A1A`. The same fixed GUI executable was replaced after closing the preceding instance; SHA-256: `281FD19B2C38AD1A4CCAB7CB765D347ED83B48AB28023F89E583CBE87B622BCB`.

Actual GUI validation (PID 25428) again rendered all three cases without a crash during these checks. MCM was restored to all 40 scene layer IDs and layer colors for paired diagnostics, selected VSS, then cleared selection and hovered the same Via 1478050 at the exact prior camera and 2160 x 1380 target. The last-100-frame hover callback median is 13.118 ms versus 32.794 ms; cadence 16.694 versus 37.484 ms. Boundary category median fell from 16.900 to 0.582 ms; trace is 1.791 ms, zone fill 9.110 ms. Per-frame draw counts remain unchanged (trace 5,314, pads 502, drills 154), confirming that this change removes repeated classification rather than drawing less geometry. VSS fit callback was about 13.1 ms versus 32.5 ms. Ordinary full-layer 84% view was 9.321 ms; 525% zoom and PAN rendered at about 15.6 ms, with hover suppressed in PAN. Pan cadence includes later import activity and is not used as a steady comparison.

The first matching real GUI hover frame was still 75.969 ms, followed by approximately 13 ms stable frames. That first frame spent about 27 ms in pin submission, 15 ms in drills and 17 ms in zone outlines, reflecting membership/summary construction. This transient cost, pointer movement across objects and presented-frame/input-latency measurement remain open; stable callback cadence does not prove displayed FPS. Diagnostics and summaries are `target/geometry-gui-frame-trace-overlay*`.

Both BRDs were imported through the native dialog and showed complete fitted boards: 15061 Ready with 8 board layers, 6,848 components and 7,484 nets; ntpcb Ready with 34 board layers, 12,850 components and 12,299 nets. Saved BRD visibility/selection states and user activity during validation differ from the full-layer harness, so these GUI checks establish rendering rather than isolated full-layer performance comparisons. All three tabs remain open; ntpcb was fitted and remained active. Subsequent user zooms also appear in the diagnostic log. The diagnostic marker was removed to stop continuous sampling. Concurrent user-task changes are preserved. The overall optimization goal remains active.
