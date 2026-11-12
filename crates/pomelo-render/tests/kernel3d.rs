//! Direct vs solidcore (K1) single-object equivalence: identical normalized input, so the
//! meshes must agree on area, volume, closure and surface classes.
#![cfg(feature = "kernel-3d")]

use pomelo_core::{
    geometry3d::solid::{SolidLimits, SolidMesh, SurfaceKind},
    model::Point,
    task::CancellationToken,
};
use pomelo_render::scene::kernel3d::{Backend3d, Threading};
use std::{collections::HashMap, sync::Mutex};

/// The extruder is process-wide; tests in this file take turns.
static LOCK: Mutex<()> = Mutex::new(());

const CHORD: f64 = 0.005;

fn circle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![
        Point::new(x0, y0),
        Point::new(x1, y0),
        Point::new(x1, y1),
        Point::new(x0, y1),
    ]
}

fn signed_volume(mesh: &SolidMesh) -> f64 {
    mesh.indices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[t[i] as usize].position);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

fn surface_area(mesh: &SolidMesh, kind: SurfaceKind) -> f64 {
    mesh.surfaces
        .iter()
        .filter(|s| s.kind == kind)
        .flat_map(|s| mesh.indices[s.indices.start as usize..s.indices.end as usize].chunks(3))
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[t[i] as usize].position);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
        })
        .sum()
}

/// Every directed edge, matched by position, has exactly one opposite: closed and consistently
/// oriented.
fn assert_closed(mesh: &SolidMesh, what: &str) {
    let key = |i: u32| {
        let p = mesh.vertices[i as usize].position;
        [p[0], p[1], p[2]].map(|v| (v * 1e7).round() as i64)
    };
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for t in mesh.indices.chunks(3) {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (key(a), key(b));
            *edges.entry((a, b)).or_default() += 1;
        }
    }
    // Long edges may be split differently on neighbouring faces (T-junctions); measure the
    // boundary length instead of requiring exact pairing.
    let open: Vec<_> = edges
        .iter()
        .filter(|((a, b), n)| edges.get(&(*b, *a)).copied().unwrap_or(0) != **n)
        .collect();
    assert!(
        open.is_empty(),
        "{what}: {} unmatched directed edges, e.g. {:?}",
        open.len(),
        open.iter().take(3).collect::<Vec<_>>()
    );
}

fn build(backend: Backend3d, f: &dyn Fn() -> SolidMesh) -> SolidMesh {
    let stats = backend.install(CHORD);
    let mesh = f();
    Backend3d::Direct.install(CHORD);
    if let Some(stats) = stats {
        let json = stats.to_json();
        assert_eq!(json["failures"], 0, "{json}");
    }
    mesh
}

fn compare(what: &str, f: &dyn Fn() -> SolidMesh) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let direct = build(Backend3d::Direct, f);
    assert_closed(&direct, &format!("{what} direct"));
    for threading in [Threading::Ops, Threading::PerThread] {
        let kernel = build(Backend3d::Kernel(threading), f);
        let label = format!("{what} kernel-{}", threading.name());
        assert_closed(&kernel, &label);
        let (dv, kv) = (signed_volume(&direct), signed_volume(&kernel));
        assert!(dv > 0.0 && kv > 0.0, "{label}: volumes {dv} {kv}");
        assert!((dv - kv).abs() <= dv * 1e-9, "{label}: volume {dv} vs {kv}");
        for kind in [
            SurfaceKind::Top,
            SurfaceKind::Bottom,
            SurfaceKind::OuterWall,
            SurfaceKind::HoleWall,
        ] {
            let (da, ka) = (surface_area(&direct, kind), surface_area(&kernel, kind));
            assert!(
                (da - ka).abs() <= da.max(1e-12) * 1e-9,
                "{label}: {kind:?} area {da} vs {ka}"
            );
        }
        let (db, kb) = (direct.bounds.unwrap(), kernel.bounds.unwrap());
        for i in 0..3 {
            assert!((db.min[i] - kb.min[i]).abs() < 1e-9 && (db.max[i] - kb.max[i]).abs() < 1e-9);
        }
    }
}

fn limits() -> SolidLimits {
    SolidLimits::default()
}

#[test]
fn rectangle_matches() {
    compare("rectangle", &|| {
        SolidMesh::extrude_coverage(
            &[rect(10.0, 20.0, 12.0, 21.0)],
            [0.0, 0.035],
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}

#[test]
fn pad_with_drill_matches() {
    compare("pad with drill", &|| {
        SolidMesh::extrude_coverage_minus(
            &[circle(5.0, 5.0, 0.4, 64)],
            &[circle(5.0, 5.0, 0.15, 32)],
            [-0.8, -0.765],
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}

#[test]
fn zone_with_islands_matches() {
    compare("zone with holes", &|| {
        SolidMesh::extrude_coverage(
            &[
                rect(0.0, 0.0, 20.0, 10.0),
                circle(4.0, 5.0, 1.0, 48),
                rect(10.0, 2.0, 14.0, 8.0),
            ],
            [0.0, 0.035],
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}

#[test]
fn stroke_matches() {
    compare("stroke", &|| {
        SolidMesh::extrude_stroke(
            &[
                Point::new(0.0, 0.0),
                Point::new(3.0, 0.0),
                Point::new(4.0, 1.0),
            ],
            0.1,
            false,
            [0.0, 0.035],
            CHORD,
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}

#[test]
fn even_odd_pad_matches() {
    compare("even-odd custom pad", &|| {
        SolidMesh::extrude_even_odd(
            &[rect(0.0, 0.0, 2.0, 2.0), rect(0.5, 0.5, 1.5, 1.5)],
            [0.0, 0.035],
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}

#[test]
fn far_from_origin_matches() {
    compare("far from origin", &|| {
        SolidMesh::extrude_coverage_minus(
            &[rect(900.0, -700.0, 901.5, -699.0)],
            &[circle(900.75, -699.5, 0.2, 32)],
            [0.0, 1.6],
            limits(),
            &CancellationToken::default(),
        )
        .unwrap()
    });
}
