//! solidcore comparison, variant K2 (docs/pcb-3d-solidcore-experiment-plan.md §4): exact
//! profiles (lines and arcs) against the polygons the direct path extrudes.
//!
//! For a deterministic sample of every trace, pad and via pad, single-threaded on one kernel:
//! - K2: the exact profile extruded and meshed at several chord tolerances;
//! - K1: the same shape flattened at the preview chord (0.005 mm), as the direct path does.
//!
//! Reported per object kind: kernel time, triangles per chord, and the top-cap area error
//! against the analytic area, which is the cost and the accuracy of keeping curves exact.
//!
//! ```text
//! kernel_k2 <board> [--layer NAME] [--max-per-kind N]
//! ```
use pomelo_core::{
    model::{Point, Segment},
    pad::{PadOutline, PadPlacement},
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
use solidcore_tess::MeshTolerance;
use std::{collections::BTreeMap, f64::consts::PI, time::Instant};

const CHORDS: [f64; 4] = [0.05, 0.01, 0.005, 0.001];
const PREVIEW_CHORD: f64 = 0.005;
const Z: [f64; 2] = [0.0, 0.035];

fn v(p: Point) -> Vec2 {
    Vec2 { x: p.x, y: p.y }
}

fn polar(c: Point, r: f64, a: f64) -> Point {
    Point::new(c.x + r * a.cos(), c.y + r * a.sin())
}

fn arc(center: Point, radius: f64, start: f64, sweep: f64) -> Seg2 {
    Seg2::Arc {
        center: v(center),
        radius,
        start,
        sweep,
    }
}

fn circle(center: Point, radius: f64, ccw: bool) -> Loop2 {
    Loop2 {
        segs: vec![arc(
            center,
            radius,
            0.0,
            if ccw { 2.0 * PI } else { -2.0 * PI },
        )],
    }
}

/// A shape with its exact profile and analytic area.
struct Shape {
    kind: &'static str,
    what: String,
    region: Region2,
    area: f64,
}

fn capsule(kind: &'static str, a: Point, b: Point, r: f64) -> Option<Shape> {
    if r.is_nan() || r <= 0.0 {
        return None;
    }
    let len = a.distance(b);
    if len < 1e-9 {
        return Some(Shape {
            what: String::new(),
            kind,
            region: Region2 {
                outer: circle(a, r, true),
                holes: vec![],
            },
            area: PI * r * r,
        });
    }
    let t = (b.y - a.y).atan2(b.x - a.x);
    let n = t - PI / 2.0; // right-hand side
    let segs = vec![
        Seg2::Line {
            a: v(polar(a, r, n)),
            b: v(polar(b, r, n)),
        },
        arc(b, r, n, PI),
        Seg2::Line {
            a: v(polar(b, r, n + PI)),
            b: v(polar(a, r, n + PI)),
        },
        arc(a, r, n + PI, PI),
    ];
    Some(Shape {
        what: String::new(),
        kind,
        region: Region2 {
            outer: Loop2 { segs },
            holes: vec![],
        },
        area: 2.0 * r * len + PI * r * r,
    })
}

fn trace(s: &Segment) -> Option<Shape> {
    let r = s.width * 0.5;
    let Some(a) = s.arc else {
        return capsule("trace-line", s.a, s.b, r);
    };
    let (big, small) = (a.radius + r, a.radius - r);
    if small.is_nan() || small <= 1e-6 {
        return None;
    }
    if a.sweep.abs() >= 2.0 * PI - 1e-9 {
        return Some(Shape {
            what: String::new(),
            kind: "trace-ring",
            region: Region2 {
                outer: circle(a.center, big, true),
                holes: vec![circle(a.center, small, false)],
            },
            area: PI * (big * big - small * small),
        });
    }
    let (t0, sw) = if a.sweep >= 0.0 {
        (a.start, a.sweep)
    } else {
        (a.start + a.sweep, -a.sweep)
    };
    let t1 = t0 + sw;
    let (e0, e1) = (polar(a.center, a.radius, t0), polar(a.center, a.radius, t1));
    let segs = vec![
        arc(a.center, big, t0, sw),
        arc(e1, r, t1, PI),
        arc(a.center, small, t1, -sw),
        arc(e0, r, t0 + PI, PI),
    ];
    Some(Shape {
        what: String::new(),
        kind: "trace-arc",
        region: Region2 {
            outer: Loop2 { segs },
            holes: vec![],
        },
        area: 2.0 * a.radius * r * sw + PI * r * r,
    })
}

fn shoelace(ring: &[Point]) -> f64 {
    (0..ring.len())
        .map(|i| {
            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
            p.x * q.y - q.x * p.y
        })
        .sum::<f64>()
        * 0.5
}

fn line_loop(ring: &[Point], ccw: bool) -> Loop2 {
    let mut pts = ring.to_vec();
    if (shoelace(&pts) > 0.0) != ccw {
        pts.reverse();
    }
    Loop2 {
        segs: (0..pts.len())
            .map(|i| Seg2::Line {
                a: v(pts[i]),
                b: v(pts[(i + 1) % pts.len()]),
            })
            .collect(),
    }
}

fn pad(kind: &'static str, outline: PadOutline) -> Option<Shape> {
    match outline {
        PadOutline::Circle { center, radius } => Some(Shape {
            what: String::new(),
            kind,
            region: Region2 {
                outer: circle(center, radius, true),
                holes: vec![],
            },
            area: PI * radius * radius,
        }),
        PadOutline::Capsule { a, b, radius } => capsule(kind, a, b, radius),
        // Polygonal families: exact already; only a single simple ring is taken here.
        PadOutline::Rings(rings) if rings.len() == 1 && rings[0].len() >= 3 => Some(Shape {
            what: String::new(),
            kind: "pad-polygon",
            area: shoelace(&rings[0]).abs(),
            region: Region2 {
                outer: line_loop(&rings[0], true),
                holes: vec![],
            },
        }),
        PadOutline::Rings(_) => None,
    }
}

/// Flattened copy at `chord`, the way the direct path sees a curve.
fn flatten(region: &Region2, chord: f64) -> Region2 {
    let flat = |lp: &Loop2| {
        let mut pts = Vec::new();
        for s in &lp.segs {
            match *s {
                Seg2::Line { a, .. } => pts.push(Point::new(a.x, a.y)),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    let step = 2.0 * (1.0 - chord / radius).clamp(-1.0, 1.0).acos();
                    let min = if sweep.abs() >= 2.0 * PI - 1e-9 {
                        32
                    } else {
                        4
                    };
                    let n = ((sweep.abs() / step).ceil() as usize).max(min);
                    for i in 0..n {
                        pts.push(polar(
                            Point::new(center.x, center.y),
                            radius,
                            start + sweep * i as f64 / n as f64,
                        ));
                    }
                }
                _ => {}
            }
        }
        let ccw = shoelace(&pts) > 0.0;
        line_loop(&pts, ccw)
    };
    Region2 {
        outer: flat(&region.outer),
        holes: region.holes.iter().map(flat).collect(),
    }
}

struct Built {
    kernel_ns: u128,
    record: solidcore_record::brep::Brep,
    size: f64,
}

fn build(k: &mut solidcore::Kernel, region: &Region2) -> Result<Built, String> {
    let start = Instant::now();
    let profile = solidcore_convert::region2(region);
    solidcore::prism::profile::check_regions(std::iter::once(&profile), 1e-7)
        .map_err(|e| format!("profile: {e:?}"))?;
    let frame = solidcore_convert::plane(&Plane::XY);
    let mut brep = solidcore::prism::extrude::extrude_region(k, &frame, &profile, Z[0], Z[1])
        .map_err(|e| format!("extrude: {e}"))?;
    if !solidcore::topology::brep::p2::is_manifold_solid(&brep).map_err(|e| e.to_string())? {
        return Err("not a closed solid".into());
    }
    let (record, _) =
        solidcore_convert::brep::to_record(k, &mut brep).map_err(|e| format!("record: {e:?}"))?;
    let kernel_ns = start.elapsed().as_nanos();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for s in region.outer.segs.iter() {
        if let Seg2::Line { a, .. } | Seg2::Arc { center: a, .. } = s {
            let r = if let Seg2::Arc { radius, .. } = s {
                *radius
            } else {
                0.0
            };
            lo = [lo[0].min(a.x - r), lo[1].min(a.y - r)];
            hi = [hi[0].max(a.x + r), hi[1].max(a.y + r)];
        }
    }
    let size = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(Z[1] - Z[0]);
    Ok(Built {
        kernel_ns,
        record,
        size,
    })
}

/// Triangles and top-cap area at `chord`.
fn mesh(built: &Built, chord: f64) -> Result<(usize, f64, u128), String> {
    let start = Instant::now();
    let (plan, stats) =
        solidcore_tess::plan(&built.record, &MeshTolerance::chord(chord), built.size)
            .map_err(|e| e.to_string())?;
    if !stats.unmeshed.is_empty() {
        return Err(format!("{} unmeshed", stats.unmeshed.len()));
    }
    let (m, refused) = plan.evaluate();
    if !refused.is_empty() {
        return Err(format!("{} refused", refused.len()));
    }
    let ns = start.elapsed().as_nanos();
    let mut top = 0.0;
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        if [a.z, b.z, c.z].iter().all(|z| (z - Z[1]).abs() < 1e-9) {
            top += 0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs();
        }
    }
    Ok((m.triangles.len(), top, ns))
}

#[derive(Default)]
struct Kind {
    objects: usize,
    failures: usize,
    samples: Vec<String>,
    k2_kernel_ns: Vec<u128>,
    k1_kernel_ns: Vec<u128>,
    k2_tess_ns: BTreeMap<String, u128>,
    k2_triangles: BTreeMap<String, usize>,
    k2_area_err: BTreeMap<String, f64>,
    k1_triangles: usize,
    k1_area_err: f64,
    worst: (f64, String),
}

fn percentile(v: &mut [u128], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p).round() as usize] as f64 / 1e3
}

fn main() -> anyhow::Result<()> {
    let mut path = None;
    let mut layer = None;
    let mut cap = 2000usize;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--layer" => layer = it.next(),
            "--max-per-kind" => cap = it.next().unwrap_or_default().parse()?,
            _ => path = Some(a),
        }
    }
    let path = path.ok_or_else(|| {
        anyhow::anyhow!("usage: kernel_k2 <board> [--layer NAME] [--max-per-kind N]")
    })?;
    let cancel = pomelo_core::task::CancellationToken::default();
    let board = FormatImporter
        .import(
            path.as_ref(),
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )?
        .scene;
    let layer_id = layer.as_ref().and_then(|n| {
        board
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(n))
            .map(|l| l.id)
    });
    let on_layer = |id| layer_id.is_none_or(|l| l == id);

    let mut shapes: Vec<Shape> = Vec::new();
    let mut skipped = 0usize;
    for s in board.segments.iter().filter(|s| on_layer(s.layer)) {
        match trace(s) {
            Some(mut shape) => {
                shape.what = format!("{s:?}");
                shapes.push(shape)
            }
            None => skipped += 1,
        }
    }
    let mut add_pads = |pads: &[pomelo_core::model::Pad], placement: PadPlacement, kind| {
        for p in pads.iter().filter(|p| on_layer(p.layer)) {
            match p.outline(placement, PREVIEW_CHORD) {
                Ok(Some(o)) => match pad(kind, o) {
                    Some(shape) => shapes.push(shape),
                    None => skipped += 1,
                },
                _ => skipped += 1,
            }
        }
    };
    for pin in &board.pins {
        let placement = PadPlacement {
            at: pin.at,
            angle: pin.angle,
            mirrored: pin.mirrored,
        };
        add_pads(&pin.pads, placement, "pad-round");
    }
    for via in &board.vias {
        let placement = PadPlacement {
            at: via.at,
            angle: via.angle,
            mirrored: via.mirrored,
        };
        add_pads(&via.pads, placement, "via-pad-round");
    }

    // A deterministic sample: every n-th shape of each kind, up to `cap`.
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &shapes {
        *totals.entry(s.kind).or_default() += 1;
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, Kind> = BTreeMap::new();
    let mut k = solidcore::Kernel::default();
    let started = Instant::now();
    for shape in &shapes {
        let total = totals[shape.kind];
        let index = seen.entry(shape.kind).or_default();
        *index += 1;
        let stride = total.div_ceil(cap).max(1);
        if !(*index - 1).is_multiple_of(stride) {
            continue;
        }
        let kind = kinds.entry(shape.kind).or_default();
        kind.objects += 1;
        let fail = |kind: &mut Kind, why: String| {
            kind.failures += 1;
            if kind.samples.len() < 5 {
                kind.samples.push(why);
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let exact = build(&mut k, &shape.region)?;
            let mut per_chord = Vec::new();
            for chord in CHORDS {
                per_chord.push((chord, mesh(&exact, chord)?));
            }
            let flat = build(&mut k, &flatten(&shape.region, PREVIEW_CHORD))?;
            let flat_mesh = mesh(&flat, PREVIEW_CHORD)?;
            Ok::<_, String>((exact.kernel_ns, per_chord, flat.kernel_ns, flat_mesh))
        }));
        match result {
            Ok(Ok((k2_ns, per_chord, k1_ns, (k1_tris, k1_area, _)))) => {
                kind.k2_kernel_ns.push(k2_ns);
                kind.k1_kernel_ns.push(k1_ns);
                for (chord, (tris, area, ns)) in per_chord {
                    let key = format!("{chord}");
                    *kind.k2_tess_ns.entry(key.clone()).or_default() += ns;
                    *kind.k2_triangles.entry(key.clone()).or_default() += tris;
                    let err = (area - shape.area).abs() / shape.area;
                    if chord == 0.001 && err > kind.worst.0 {
                        kind.worst = (
                            err,
                            format!("{} exact={} mesh={area}", shape.what, shape.area),
                        );
                    }
                    let e = kind.k2_area_err.entry(key).or_default();
                    *e = e.max(err);
                }
                kind.k1_triangles += k1_tris;
                kind.k1_area_err = kind
                    .k1_area_err
                    .max((k1_area - shape.area).abs() / shape.area);
            }
            Ok(Err(why)) => fail(kind, why),
            Err(_) => {
                k = solidcore::Kernel::default();
                fail(kind, "kernel panic".into());
            }
        }
    }

    let report: serde_json::Map<String, serde_json::Value> = kinds
        .iter_mut()
        .map(|(name, kd)| {
            let n = kd.objects.saturating_sub(kd.failures).max(1) as f64;
            (
                (*name).to_owned(),
                serde_json::json!({
                    "objects_on_board": totals[name],
                    "sampled": kd.objects,
                    "failures": kd.failures,
                    "failure_samples": kd.samples,
                    "k2_kernel_us_p50": percentile(&mut kd.k2_kernel_ns, 0.5),
                    "k2_kernel_us_p95": percentile(&mut kd.k2_kernel_ns, 0.95),
                    "k1_kernel_us_p50": percentile(&mut kd.k1_kernel_ns, 0.5),
                    "k1_kernel_us_p95": percentile(&mut kd.k1_kernel_ns, 0.95),
                    "k2_tess_us_mean": kd.k2_tess_ns.iter().map(|(c, ns)| (c.clone(), *ns as f64 / n / 1e3)).collect::<BTreeMap<_, _>>(),
                    "k2_triangles_mean": kd.k2_triangles.iter().map(|(c, t)| (c.clone(), *t as f64 / n)).collect::<BTreeMap<_, _>>(),
                    "k2_area_rel_err_max": kd.k2_area_err,
                    "k1_triangles_mean_at_0.005": kd.k1_triangles as f64 / n,
                    "k1_area_rel_err_max_at_0.005": kd.k1_area_err,
                    "worst_at_0.001": kd.worst.1,
                }),
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "path": path,
            "layer": layer,
            "max_per_kind": cap,
            "skipped_shapes": skipped,
            "elapsed_ms": started.elapsed().as_millis(),
            "kinds": report,
        }))?
    );
    Ok(())
}
