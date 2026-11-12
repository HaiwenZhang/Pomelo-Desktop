//! Experimental solidcore backend for the 3D comparison
//! (docs/pcb-3d-solidcore-experiment-plan.md, variant K1).
//!
//! Each normalized region that the direct extruder would triangulate is instead built as a
//! B-rep prism by the solidcore kernel, meshed by `solidcore-tess`, and appended to the same
//! [`SolidMesh`]. Everything downstream (paging, instancing, picking, GPU) is unchanged, so the
//! two backends differ only in who makes the solid mesh.
//!
//! Two ways to call the kernel are measured:
//! - [`Threading::Ops`]: through `solidcore-ops`, as solidcore's layering asks of applications.
//!   Ops holds one process-wide kernel behind a lock, so calls are serialized.
//! - [`Threading::PerThread`]: one kernel per worker thread, calling the kernel's extrusion
//!   directly. This bypasses the layering rule and exists only to measure the parallel ceiling.

use pomelo_core::geometry3d::{
    Bounds3,
    solid::{
        RegionExtruder, RegionInput, SolidError, SolidMesh, SolidSurface, SolidVertex, SurfaceKind,
        set_region_extruder,
    },
};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, brep::Brep as Record};
use solidcore_tess::MeshTolerance;
use std::{
    cell::RefCell,
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

/// How the kernel is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threading {
    Ops,
    PerThread,
}

impl Threading {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ops => "ops",
            Self::PerThread => "thread",
        }
    }
}

/// Which 3D backend a run uses, parsed from `POMELO_3D_BACKEND` or a command-line value:
/// `direct` (default), `kernel-ops`, `kernel-thread`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend3d {
    Direct,
    Kernel(Threading),
}

impl Backend3d {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "direct" | "d" => Some(Self::Direct),
            "kernel-ops" | "k1-ops" | "kernel" | "k1" => Some(Self::Kernel(Threading::Ops)),
            "kernel-thread" | "k1-thread" => Some(Self::Kernel(Threading::PerThread)),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        std::env::var("POMELO_3D_BACKEND")
            .ok()
            .and_then(|v| Self::parse(&v))
            .unwrap_or(Self::Direct)
    }

    pub fn name(self) -> String {
        match self {
            Self::Direct => "direct".into(),
            Self::Kernel(t) => format!("kernel-{}", t.name()),
        }
    }

    /// Installs this backend process-wide and returns the kernel counters, if any. `chord_mm`
    /// is the tessellation chord tolerance (the preview's curve tolerance).
    pub fn install(self, chord_mm: f64) -> Option<Arc<KernelStats>> {
        match self {
            Self::Direct => {
                set_region_extruder(None);
                None
            }
            Self::Kernel(threading) => {
                let stats = Arc::new(KernelStats::default());
                set_region_extruder(Some(Arc::new(KernelExtruder {
                    threading,
                    chord_mm,
                    stats: stats.clone(),
                })));
                Some(stats)
            }
        }
    }
}

/// Counters across every region the kernel built. Times are summed over threads.
#[derive(Debug, Default)]
pub struct KernelStats {
    pub regions: AtomicU64,
    pub segments: AtomicU64,
    pub failures: AtomicU64,
    pub panics: AtomicU64,
    pub unmeshed_faces: AtomicU64,
    pub lenient_faces: AtomicU64,
    pub over_tolerance_faces: AtomicU64,
    pub brep_faces: AtomicU64,
    pub triangles: AtomicU64,
    pub extrude_ns: AtomicU64,
    pub record_ns: AtomicU64,
    pub tess_ns: AtomicU64,
    pub adapt_ns: AtomicU64,
    samples: Mutex<Vec<String>>,
    /// Per region: [ring points, holes, kernel ms (extrude + record), tess ms], first ones only.
    regions_detail: Mutex<Vec<[f64; 4]>>,
}

const MAX_SAMPLES: usize = 16;
const MAX_REGION_DETAIL: usize = 20_000;

impl KernelStats {
    fn fail(&self, why: String) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        if samples.len() < MAX_SAMPLES {
            samples.push(why);
        }
    }

    pub fn failure_samples(&self) -> Vec<String> {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn detail(&self, row: [f64; 4]) {
        let mut rows = self
            .regions_detail
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if rows.len() < MAX_REGION_DETAIL {
            rows.push(row);
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let ms = |a: &AtomicU64| a.load(Ordering::Relaxed) as f64 / 1e6;
        serde_json::json!({
            "regions": n(&self.regions),
            "segments": n(&self.segments),
            "failures": n(&self.failures),
            "panics": n(&self.panics),
            "unmeshed_faces": n(&self.unmeshed_faces),
            "lenient_faces": n(&self.lenient_faces),
            "over_tolerance_faces": n(&self.over_tolerance_faces),
            "brep_faces": n(&self.brep_faces),
            "triangles": n(&self.triangles),
            "extrude_ms_sum": ms(&self.extrude_ns),
            "record_ms_sum": ms(&self.record_ns),
            "tess_ms_sum": ms(&self.tess_ns),
            "adapt_ms_sum": ms(&self.adapt_ns),
            "failure_samples": self.failure_samples(),
            "regions_detail": self.regions_detail.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        })
    }
}

struct KernelExtruder {
    threading: Threading,
    chord_mm: f64,
    stats: Arc<KernelStats>,
}

thread_local! {
    static KERNEL: RefCell<Option<solidcore::Kernel>> = const { RefCell::new(None) };
}

fn elapsed(start: Instant, into: &AtomicU64) {
    into.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

/// The region as a solidcore profile: straight segments between consecutive ring points.
fn profile(rings: &[Vec<[f64; 2]>]) -> Option<Region2> {
    let loop_of = |ring: &Vec<[f64; 2]>| Loop2 {
        segs: (0..ring.len())
            .map(|i| {
                let [ax, ay] = ring[i];
                let [bx, by] = ring[(i + 1) % ring.len()];
                Seg2::Line {
                    a: Vec2 { x: ax, y: ay },
                    b: Vec2 { x: bx, y: by },
                }
            })
            .collect(),
    };
    let (outer, holes) = rings.split_first()?;
    Some(Region2 {
        outer: loop_of(outer),
        holes: holes.iter().map(loop_of).collect(),
    })
}

impl KernelExtruder {
    fn record(&self, region: &Region2, z: [f64; 2]) -> Result<Record, String> {
        match self.threading {
            Threading::Ops => {
                let start = Instant::now();
                let body = solidcore_ops::prism::extrude_planar_regions(
                    &Plane::XY,
                    std::slice::from_ref(region),
                    z[0],
                    z[1],
                )
                .map_err(|e| format!("extrude: {e}"))?;
                elapsed(start, &self.stats.extrude_ns);
                let start = Instant::now();
                let (record, _) = body.to_record().map_err(|e| format!("record: {e}"))?;
                elapsed(start, &self.stats.record_ns);
                Ok(record)
            }
            Threading::PerThread => KERNEL.with(|cell| {
                let mut slot = cell.borrow_mut();
                let kernel = slot.get_or_insert_with(solidcore::Kernel::default);
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let start = Instant::now();
                    let profile = solidcore_convert::region2(region);
                    let frame = solidcore_convert::plane(&Plane::XY);
                    let mut brep = solidcore::prism::extrude::extrude_region(
                        kernel, &frame, &profile, z[0], z[1],
                    )
                    .map_err(|e| format!("extrude: {e}"))?;
                    if !solidcore::topology::brep::p2::is_manifold_solid(&brep)
                        .map_err(|e| format!("solid test: {e}"))?
                    {
                        return Err("extrude: not a closed solid".to_owned());
                    }
                    elapsed(start, &self.stats.extrude_ns);
                    let start = Instant::now();
                    let (record, _) = solidcore_convert::brep::to_record(kernel, &mut brep)
                        .map_err(|e| format!("record: {e:?}"))?;
                    elapsed(start, &self.stats.record_ns);
                    Ok(record)
                }));
                match result {
                    Ok(r) => r,
                    Err(_) => {
                        // The kernel may be half-updated; start the next call on a fresh one.
                        *slot = None;
                        self.stats.panics.fetch_add(1, Ordering::Relaxed);
                        Err("kernel panic".to_owned())
                    }
                }
            }),
        }
    }

    fn build(&self, input: &RegionInput<'_>, mesh: &mut SolidMesh) -> Result<(), String> {
        let region = profile(input.rings).ok_or("empty region")?;
        let kernel_start = Instant::now();
        let record = self.record(&region, input.z)?;
        let kernel_ms = kernel_start.elapsed().as_secs_f64() * 1e3;

        let start = Instant::now();
        let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in input.rings.iter().flatten() {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let size = (max[0] - min[0])
            .hypot(max[1] - min[1])
            .hypot(input.z[1] - input.z[0]);
        let (plan, stats) =
            solidcore_tess::plan(&record, &MeshTolerance::chord(self.chord_mm), size)
                .map_err(|e| format!("tess plan: {e}"))?;
        let s = &self.stats;
        s.lenient_faces
            .fetch_add(stats.lenient.len() as u64, Ordering::Relaxed);
        s.over_tolerance_faces
            .fetch_add(stats.over_tolerance.len() as u64, Ordering::Relaxed);
        if !stats.unmeshed.is_empty() {
            s.unmeshed_faces
                .fetch_add(stats.unmeshed.len() as u64, Ordering::Relaxed);
            return Err(format!("tess: {} face(s) unmeshed", stats.unmeshed.len()));
        }
        let (tess, refused) = plan.evaluate();
        if !refused.is_empty() {
            s.unmeshed_faces
                .fetch_add(refused.len() as u64, Ordering::Relaxed);
            return Err(format!("tess: {} face(s) refused", refused.len()));
        }
        elapsed(start, &s.tess_ns);
        s.detail([
            input.rings.iter().map(Vec::len).sum::<usize>() as f64,
            input.rings.len().saturating_sub(1) as f64,
            kernel_ms,
            start.elapsed().as_secs_f64() * 1e3,
        ]);

        let start = Instant::now();
        append(input, &tess, mesh)?;
        elapsed(start, &s.adapt_ns);
        s.brep_faces.fetch_add(
            tess.tri_face.iter().max().map_or(0, |&f| u64::from(f) + 1),
            Ordering::Relaxed,
        );
        s.triangles
            .fetch_add(tess.triangles.len() as u64, Ordering::Relaxed);
        Ok(())
    }
}

impl RegionExtruder for KernelExtruder {
    fn extrude(&self, input: &RegionInput<'_>, mesh: &mut SolidMesh) -> Result<(), SolidError> {
        self.stats.regions.fetch_add(1, Ordering::Relaxed);
        self.stats.segments.fetch_add(
            input.rings.iter().map(|r| r.len() as u64).sum(),
            Ordering::Relaxed,
        );
        self.build(input, mesh).map_err(|why| {
            self.stats.fail(why);
            SolidError::Unsupported("KERNEL_REGION")
        })
    }
}

/// Grid key for matching a mesh vertex to the ring point it came from (0.1 µm).
fn key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1e4).round() as i64, (y * 1e4).round() as i64)
}

/// Appends the kernel's mesh, grouping its triangles into the direct extruder's surfaces:
/// caps by the facet normal, walls by which ring their vertices lie on.
fn append(
    input: &RegionInput<'_>,
    tess: &solidcore_tess::Mesh,
    mesh: &mut SolidMesh,
) -> Result<(), String> {
    if tess.triangles.is_empty() || tess.positions.len() != tess.normals.len() {
        return Err("tess: empty or malformed mesh".into());
    }
    let ring_of: HashMap<(i64, i64), usize> = input
        .rings
        .iter()
        .enumerate()
        .flat_map(|(i, r)| r.iter().map(move |p| (key(p[0], p[1]), i)))
        .collect();
    let faces = tess.tri_face.iter().max().map_or(0, |&f| f as usize + 1);
    // Per B-rep face: its kind, from the first triangle's facet normal and its vertices.
    let mut kinds: Vec<Option<SurfaceKind>> = vec![None; faces];
    let position = |i: u32| tess.positions[i as usize];
    for (t, tri) in tess.triangles.iter().enumerate() {
        let face = tess.tri_face.get(t).copied().unwrap_or(0) as usize;
        if kinds[face].is_some() {
            continue;
        }
        let [a, b, c] = tri.map(position);
        let (ux, uy, uz) = (b.x - a.x, b.y - a.y, b.z - a.z);
        let (vx, vy, vz) = (c.x - a.x, c.y - a.y, c.z - a.z);
        let nz = ux * vy - uy * vx;
        let length = (uy * vz - uz * vy).hypot(uz * vx - ux * vz).hypot(nz);
        if length.is_nan() || length <= 0.0 {
            continue;
        }
        kinds[face] = Some(if nz / length > 0.5 {
            SurfaceKind::Top
        } else if nz / length < -0.5 {
            SurfaceKind::Bottom
        } else {
            let hole = [a, b, c]
                .iter()
                .find_map(|p| ring_of.get(&key(p.x, p.y)))
                .is_some_and(|&ring| ring > 0);
            if hole {
                SurfaceKind::HoleWall
            } else {
                SurfaceKind::OuterWall
            }
        });
    }

    let base = u32::try_from(mesh.vertices.len()).map_err(|_| "vertex overflow")?;
    let [ox, oy] = [input.origin.x, input.origin.y];
    mesh.vertices
        .try_reserve(tess.positions.len())
        .map_err(|_| "allocation")?;
    for (p, n) in tess.positions.iter().zip(&tess.normals) {
        let position = [ox + p.x, oy + p.y, p.z];
        if !position.iter().all(|v| v.is_finite()) {
            return Err("tess: non-finite position".into());
        }
        mesh.vertices.push(SolidVertex {
            position,
            normal: [n.x as f32, n.y as f32, n.z as f32],
        });
        let bounds = mesh.bounds.get_or_insert(Bounds3 {
            min: position,
            max: position,
        });
        for (i, &value) in position.iter().enumerate() {
            bounds.min[i] = bounds.min[i].min(value);
            bounds.max[i] = bounds.max[i].max(value);
        }
    }
    mesh.indices
        .try_reserve(tess.triangles.len() * 3)
        .map_err(|_| "allocation")?;
    for kind in [
        SurfaceKind::Top,
        SurfaceKind::Bottom,
        SurfaceKind::OuterWall,
        SurfaceKind::HoleWall,
    ] {
        let start = mesh.indices.len() as u32;
        for (t, tri) in tess.triangles.iter().enumerate() {
            let face = tess.tri_face.get(t).copied().unwrap_or(0) as usize;
            if kinds[face].unwrap_or(SurfaceKind::OuterWall) == kind {
                mesh.indices.extend(tri.iter().map(|&i| base + i));
            }
        }
        let end = mesh.indices.len() as u32;
        if end > start {
            mesh.surfaces.push(SolidSurface {
                kind,
                indices: start..end,
            });
        }
    }
    Ok(())
}
