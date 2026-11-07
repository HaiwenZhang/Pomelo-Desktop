//! Bounded, deterministic completion of exclusively owned zone meshes.
use crate::ImportError;
use pomelo_core::{
    copper::{CopperMesh, MeshError, PendingCopperMesh},
    model::Zone,
    task::{
        CancellationToken,
        preparation::{PreparationPermit, worker_limit},
    },
};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::sync::OnceLock;

static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();
pub(crate) struct MeshJob {
    pub key: u32,
    pub offset: usize,
    pub mesh: Option<PendingCopperMesh>,
}
impl MeshJob {
    pub fn new(key: u32, offset: usize, mesh: PendingCopperMesh) -> Self {
        Self {
            key,
            offset,
            mesh: Some(mesh),
        }
    }
    fn scratch(&self) -> usize {
        self.mesh
            .as_ref()
            .map_or(0, PendingCopperMesh::scratch_bytes)
    }
    pub fn points(&self) -> usize {
        self.mesh.as_ref().map_or(0, PendingCopperMesh::point_count)
    }
    fn complete(&mut self, zone: &mut Zone, cancel: &CancellationToken) -> Result<(), ImportError> {
        if zone.id.0 != self.key {
            return Err(ImportError::InvalidGeometry {
                key: self.key,
                offset: self.offset,
                field: "MESH_JOB_OWNER",
            });
        }
        let mesh = self.mesh.take().ok_or(ImportError::InvalidGeometry {
            key: self.key,
            offset: self.offset,
            field: "MESH_JOB_CONSUMED",
        })?;
        zone.mesh = mesh
            .finish(cancel)
            .map_err(|source| ImportError::CopperMesh {
                key: self.key,
                offset: self.offset,
                source,
            })?;
        Ok(())
    }
}
/// Placeholder bounds are used only by the unpublished scene's fitting pass.
pub(crate) fn placeholder(mesh: &PendingCopperMesh) -> Result<CopperMesh, MeshError> {
    let mut result = CopperMesh::default();
    if let Some(bounds) = mesh.outer_bounds() {
        result
            .ring_bounds
            .try_reserve_exact(1)
            .map_err(|_| MeshError::Allocation)?;
        result.ring_bounds.push(bounds);
    }
    Ok(result)
}
fn physical_allowance() -> usize {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if system.total_memory() == 0 {
        return 512 * 1024 * 1024;
    }
    usize::try_from((system.available_memory() / 4).min(system.total_memory() / 8))
        .unwrap_or(usize::MAX)
}
fn count(
    cpu: usize,
    requested: usize,
    jobs: usize,
    points: usize,
    scratch: usize,
    allowance: usize,
) -> usize {
    if points < 20_000 {
        return 1;
    }
    let requested = if requested == 0 { cpu } else { requested };
    cpu.min(requested.max(1))
        .min(jobs.max(1))
        .min((allowance / scratch.max(1)).max(1))
        .max(1)
}
// Contiguous ranges retain source order while balancing uneven polygon sizes.
fn partition(jobs: &[MeshJob], workers: usize) -> Vec<usize> {
    let mut sizes = Vec::new();
    let mut start = 0;
    let mut remaining = jobs.iter().map(|j| j.points().max(1) as u128).sum::<u128>();
    for worker in 0..workers {
        if worker + 1 == workers {
            sizes.push(jobs.len() - start);
            break;
        }
        let target = remaining.div_ceil((workers - worker) as u128);
        let mut weight = 0u128;
        let mut end = start;
        while end < jobs.len() - (workers - worker - 1) && (weight < target || end == start) {
            weight += jobs[end].points().max(1) as u128;
            end += 1;
        }
        sizes.push(end - start);
        start = end;
        remaining = remaining.saturating_sub(weight);
    }
    sizes
}
pub(crate) fn complete(
    jobs: &mut [MeshJob],
    zones: &mut [Zone],
    extra_bytes: usize,
    requested: usize,
    cancel: &CancellationToken,
) -> Result<(), ImportError> {
    cancel.check().map_err(|_| ImportError::Cancelled)?;
    if jobs.is_empty() {
        return Ok(());
    }
    let start = zones
        .len()
        .checked_sub(jobs.len())
        .ok_or(ImportError::InvalidGeometry {
            key: 0,
            offset: 0,
            field: "MESH_JOB_RANGE",
        })?;
    let zones = &mut zones[start..];
    let largest = jobs.iter().map(MeshJob::scratch).max().unwrap_or(0);
    if largest > extra_bytes {
        return Err(ImportError::CopperMesh {
            key: jobs[0].key,
            offset: jobs[0].offset,
            source: MeshError::Limit {
                resource: "parallel mesh scratch",
                actual: largest,
                limit: extra_bytes,
            },
        });
    }
    let allowance = physical_allowance().min(extra_bytes);
    let points = jobs.iter().map(MeshJob::points).sum();
    let workers = count(
        worker_limit(),
        requested,
        jobs.len(),
        points,
        largest,
        allowance,
    );
    let pool = if workers > 1 {
        POOL.get_or_init(|| {
            ThreadPoolBuilder::new()
                .num_threads(worker_limit())
                .thread_name(|i| format!("pcb-zone-{i}"))
                .build()
                .ok()
        })
        .as_ref()
    } else {
        None
    };
    let Some(pool) = pool else {
        let _permit = PreparationPermit::with_scratch(cancel, largest, allowance)
            .map_err(|_| ImportError::Cancelled)?;
        for (job, zone) in jobs.iter_mut().zip(zones) {
            job.complete(zone, cancel)?;
        }
        return Ok(());
    };
    let sizes = partition(jobs, workers);
    let mut chunks = Vec::new();
    let mut job_rest = jobs;
    let mut zone_rest = zones;
    for size in sizes {
        let (a, b) = job_rest.split_at_mut(size);
        job_rest = b;
        let (c, d) = zone_rest.split_at_mut(size);
        zone_rest = d;
        chunks.push((a, c));
    }
    let results = pool.install(|| {
        chunks
            .into_par_iter()
            .map(|(jobs, zones)| {
                let scratch = jobs.iter().map(MeshJob::scratch).max().unwrap_or(0);
                let _permit = PreparationPermit::with_scratch(cancel, scratch, allowance)
                    .map_err(|_| ImportError::Cancelled)?;
                for (job, zone) in jobs.iter_mut().zip(zones) {
                    job.complete(zone, cancel)?;
                }
                Ok::<_, ImportError>(())
            })
            .collect::<Vec<_>>()
    });
    cancel.check().map_err(|_| ImportError::Cancelled)?;
    for result in results {
        result?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_cpu_and_work_all_limit_parallelism() {
        assert_eq!(count(4, 0, 100, 30_000, 100, 1000), 4);
        assert_eq!(count(4, 0, 100, 30_000, 100, 200), 2);
        assert_eq!(count(4, 0, 100, 30_000, 100, 50), 1);
        assert_eq!(count(4, 0, 100, 100, 1, 1000), 1);
        assert_eq!(count(1, 0, 100, 30_000, 1, 1000), 1);
        assert_eq!(count(4, 2, 100, 30_000, 1, 1000), 2);
    }
    fn fixture(count: usize) -> (Vec<MeshJob>, Vec<Zone>, Vec<CopperMesh>) {
        use pomelo_core::{
            copper::MeshLimits,
            model::{LayerId, NetId, ObjectId, Point, ZoneKind},
        };
        let token = CancellationToken::default();
        let mut jobs = Vec::new();
        let mut zones = Vec::new();
        let mut expected = Vec::new();
        for index in 0..count {
            let ring = (0..5000 + index * 13)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / (5000 + index * 13) as f64;
                    let r = if i % 2 == 0 { 5.0 } else { 4.8 };
                    Point::new(a.cos() * r + index as f64 * 20.0, a.sin() * r)
                })
                .collect::<Vec<_>>();
            let rings = vec![
                ring,
                vec![
                    Point::new(index as f64 * 20.0, 0.),
                    Point::new(index as f64 * 20.0 + 1., 0.),
                    Point::new(index as f64 * 20.0, 1.),
                ],
            ];
            let mesh = CopperMesh::prepare(&rings, &[], &MeshLimits::default(), &token).unwrap();
            let id = ObjectId(index as u32 + 1);
            zones.push(Zone {
                id,
                layer: LayerId(index as u32 % 2),
                net: NetId(index as u32 + 1),
                kind: ZoneKind::Static,
                paths: vec![],
                mesh: placeholder(&mesh).unwrap(),
            });
            jobs.push(MeshJob::new(id.0, index, mesh));
            expected.push(CopperMesh::build(&rings, &[], &MeshLimits::default(), &token).unwrap());
        }
        (jobs, zones, expected)
    }
    fn same(a: &CopperMesh, b: &CopperMesh) {
        assert_eq!(a.vertices, b.vertices);
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.outer_count, b.outer_count);
        assert_eq!(a.ring_offsets, b.ring_offsets);
        assert_eq!(a.ring_bounds, b.ring_bounds);
        assert_eq!(a.ring_order, b.ring_order);
        assert_eq!(a.curved, b.curved);
        assert_eq!(a.hole_chunks.len(), b.hole_chunks.len());
        for (a, b) in a.hole_chunks.iter().zip(&b.hole_chunks) {
            assert_eq!(
                (a.start, a.count, a.ring_start, a.ring_count, a.bounds),
                (b.start, b.count, b.ring_start, b.ring_count, b.bounds)
            );
        }
    }
    #[test]
    fn parallel_zone_completion_keeps_exact_indices_and_typed_owners() {
        let (mut jobs, mut zones, expected) = fixture(8);
        let sizes = partition(&jobs, 4);
        assert_eq!(sizes.iter().sum::<usize>(), jobs.len());
        assert!(sizes.iter().all(|n| *n > 0));
        complete(
            &mut jobs,
            &mut zones,
            256 * 1024 * 1024,
            4,
            &CancellationToken::default(),
        )
        .unwrap();
        for (i, (zone, expected)) in zones.iter().zip(&expected).enumerate() {
            assert_eq!(zone.id.0, i as u32 + 1);
            assert_eq!(zone.net.0, i as u32 + 1);
            same(&zone.mesh, expected);
        }
    }
    #[test]
    fn one_workspace_allowance_serializes_without_changing_output() {
        let (mut jobs, mut zones, expected) = fixture(8);
        let bytes = jobs.iter().map(MeshJob::scratch).max().unwrap();
        complete(
            &mut jobs,
            &mut zones,
            bytes,
            4,
            &CancellationToken::default(),
        )
        .unwrap();
        for (zone, expected) in zones.iter().zip(&expected) {
            same(&zone.mesh, expected);
        }
    }
    #[test]
    fn cancellation_and_invalid_budget_leave_every_pending_mesh_unconsumed() {
        let (mut jobs, mut zones, _) = fixture(4);
        let token = CancellationToken::default();
        token.cancel();
        assert!(matches!(
            complete(&mut jobs, &mut zones, usize::MAX, 4, &token),
            Err(ImportError::Cancelled)
        ));
        assert!(jobs.iter().all(|j| j.mesh.is_some()));
        assert!(complete(&mut jobs, &mut zones, 0, 4, &CancellationToken::default()).is_err());
        assert!(jobs.iter().all(|j| j.mesh.is_some()));
        assert!(zones.iter().all(|z| z.mesh.indices.is_empty()));
    }
}
