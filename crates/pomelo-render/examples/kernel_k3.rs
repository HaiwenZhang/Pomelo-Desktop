//! solidcore comparison, variant K3 (docs/pcb-3d-solidcore-experiment-plan.md §4): copper
//! with drilled holes the general CAD way (a slab minus drill cylinders, 3D boolean) against the
//! planar way (the 2D boolean of the profile, then one extrusion). Small crops only.
//!
//! Each zone on `--layer` is clipped to `--window`, and every via or pin drill inside is
//! subtracted. Both results are meshed and their volumes compared.
//!
//! ```text
//! kernel_k3 <board> --layer NAME --window x0,y0,x1,y1 [--max-drills N]
//! ```
use clipper2_rust::{
    ClipType, FillRule, PathD, PointD, PolyTreeD, boolean_op_tree_d, difference_d,
};
use pomelo_core::model::Point;
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use solidcore_ops::{Body, BooleanOp, prism};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
use solidcore_tess::MeshTolerance;
use std::{f64::consts::PI, time::Instant};

const Z: [f64; 2] = [0.0, 0.035];
const CHORD: f64 = 0.005;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2 { x, y }
}

fn polygon(ring: &[Point], ccw: bool) -> Loop2 {
    let area: f64 = (0..ring.len())
        .map(|i| {
            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    let mut pts = ring.to_vec();
    if (area > 0.0) != ccw {
        pts.reverse();
    }
    Loop2 {
        segs: (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                Seg2::Line {
                    a: v(a.x, a.y),
                    b: v(b.x, b.y),
                }
            })
            .collect(),
    }
}

fn triangles(body: &Body) -> anyhow::Result<(usize, usize)> {
    let (record, _) = body.to_record()?;
    let (lo, hi) = body.bounds()?;
    let size = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
    let (plan, stats) = solidcore_tess::plan(&record, &MeshTolerance::chord(CHORD), size)?;
    let (mesh, refused) = plan.evaluate();
    Ok((mesh.triangles.len(), stats.unmeshed.len() + refused.len()))
}

fn main() -> anyhow::Result<()> {
    let mut path = None;
    let mut layer = None;
    let mut window = None;
    let mut max_drills = usize::MAX;
    let mut size = 3.0;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--layer" => layer = it.next(),
            "--max-drills" => max_drills = it.next().unwrap_or_default().parse()?,
            "--size" => size = it.next().unwrap_or_default().parse()?,
            "--window" => {
                let w: Vec<f64> = it
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                anyhow::ensure!(w.len() == 4, "--window x0,y0,x1,y1");
                window = Some([
                    w[0].min(w[2]),
                    w[1].min(w[3]),
                    w[0].max(w[2]),
                    w[1].max(w[3]),
                ]);
            }
            _ => path = Some(a),
        }
    }
    let (Some(path), Some(layer)) = (path, layer) else {
        anyhow::bail!(
            "usage: kernel_k3 <board> --layer NAME [--window x0,y0,x1,y1 | --size MM] [--max-drills N]"
        );
    };
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
    let layer_id = board
        .layers
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(&layer))
        .map(|l| l.id)
        .ok_or_else(|| anyhow::anyhow!("no layer {layer}"))?;
    // Default window: `size` mm square around the middle via of the board.
    let w = match window {
        Some(w) => w,
        None => {
            let mut vias: Vec<Point> = board.vias.iter().map(|v| v.at).collect();
            vias.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            let c = *vias
                .get(vias.len() / 2)
                .ok_or_else(|| anyhow::anyhow!("no vias"))?;
            [
                c.x - size / 2.0,
                c.y - size / 2.0,
                c.x + size / 2.0,
                c.y + size / 2.0,
            ]
        }
    };
    let inside = |p: Point| p.x >= w[0] && p.x <= w[2] && p.y >= w[1] && p.y <= w[3];
    let drills: Vec<(Point, f64)> = board
        .vias
        .iter()
        .map(|v| (v.at, v.drill))
        .chain(board.pins.iter().map(|p| (p.at, p.drill)))
        .filter(|&(at, d)| d > 0.0 && inside(at))
        .take(max_drills)
        .collect();
    let drill_regions: Vec<Region2> = drills
        .iter()
        .map(|&(at, d)| Region2 {
            outer: Loop2 {
                segs: vec![Seg2::Arc {
                    center: v(at.x, at.y),
                    radius: d * 0.5,
                    start: 0.0,
                    sweep: 2.0 * PI,
                }],
            },
            holes: vec![],
        })
        .collect();

    let mut zones = Vec::new();
    for zone in board.zones.iter().filter(|z| z.layer == layer_id) {
        let m = &zone.mesh;
        let rings: Vec<Vec<Point>> = m
            .ring_offsets
            .windows(2)
            .map(|r| m.vertices[r[0] as usize..r[1] as usize].to_vec())
            .collect();
        let Some((outer, holes)) = rings.split_first() else {
            continue;
        };
        let started = Instant::now();
        // Cut the zone (first ring minus the rest) to the window with Clipper2 first: the
        // kernel's planar profiles take at most 8192 segments, which a plane layer exceeds.
        let to_path = |r: &Vec<Point>| r.iter().map(|p| PointD::new(p.x, p.y)).collect::<Vec<_>>();
        let coverage = difference_d(
            &vec![to_path(outer)],
            &holes.iter().map(to_path).collect(),
            FillRule::NonZero,
            6,
        );
        let rect = vec![vec![
            PointD::new(w[0], w[1]),
            PointD::new(w[2], w[1]),
            PointD::new(w[2], w[3]),
            PointD::new(w[0], w[3]),
        ]];
        let mut tree = PolyTreeD::new();
        boolean_op_tree_d(
            ClipType::Intersection,
            FillRule::NonZero,
            &coverage,
            &rect,
            &mut tree,
            6,
        );
        let pts = |p: &PathD| p.iter().map(|q| Point::new(q.x, q.y)).collect::<Vec<_>>();
        let mut clipped = Vec::new();
        for (i, node) in tree.nodes.iter().enumerate().skip(1) {
            if tree.is_hole(i) {
                continue;
            }
            clipped.push(Region2 {
                outer: polygon(&pts(node.polygon()), true),
                holes: node
                    .children()
                    .iter()
                    .map(|&c| polygon(&pts(tree.nodes[c].polygon()), false))
                    .collect(),
            });
        }
        if clipped.is_empty() {
            continue;
        }
        let segments: usize = clipped
            .iter()
            .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
            .map(|l| l.segs.len())
            .sum();
        let clip_ms = started.elapsed().as_secs_f64() * 1e3;

        // Planar: 2D boolean of the profile minus the drill circles, then one extrusion.
        let started = Instant::now();
        let planar =
            prism::planar_regions_boolean(&clipped, &drill_regions, BooleanOp::Subtract, 1e-6)
                .map_err(anyhow::Error::from)
                .and_then(|r| Ok(prism::extrude_planar_regions(&Plane::XY, &r, Z[0], Z[1])?));
        let planar_ms = started.elapsed().as_secs_f64() * 1e3;

        // 3D: the clipped slab minus the union of drill cylinders.
        let started = Instant::now();
        let solid = (|| -> anyhow::Result<Option<Body>> {
            let slab = prism::extrude_planar_regions(&Plane::XY, &clipped, Z[0], Z[1])?;
            if drills.is_empty() {
                return Ok(Some(slab));
            }
            let tools = drills
                .iter()
                .map(|&(at, d)| {
                    solidcore_ops::make_cylinder(
                        [at.x, at.y, Z[0] - 0.1],
                        d * 0.5,
                        Z[1] - Z[0] + 0.2,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let tool = solidcore_ops::boolean::union_all(tools)?;
            Ok(solidcore_ops::boolean::boolean(
                slab,
                tool,
                BooleanOp::Subtract,
            )?)
        })();
        let solid_ms = started.elapsed().as_secs_f64() * 1e3;

        let describe = |b: &anyhow::Result<Body>| match b {
            Ok(body) => {
                let volume = body.volume(1e-9).ok();
                let tess = triangles(body).map_err(|e| e.to_string());
                serde_json::json!({"ok": true, "volume": volume, "faces_edges_vertices": body.counts().ok(),
                    "triangles": tess.as_ref().ok().map(|t| t.0), "unmeshed": tess.as_ref().ok().map(|t| t.1), "tess_error": tess.err()})
            }
            Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
        };
        let solid = solid.and_then(|b| b.ok_or_else(|| anyhow::anyhow!("empty result")));
        zones.push(serde_json::json!({
            "zone": zone.id.0,
            "clip_ms": clip_ms,
            "regions": clipped.len(),
            "segments": segments,
            "planar_2d_boolean": {"ms": planar_ms, "result": describe(&planar)},
            "solid_3d_boolean": {"ms": solid_ms, "result": describe(&solid)},
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "path": path, "layer": layer, "window": w, "drills": drills.len(), "zones": zones,
        }))?
    );
    Ok(())
}
