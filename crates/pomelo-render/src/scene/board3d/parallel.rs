//! Bounded CPU preparation, with a shared process ceiling and disjoint byte allowances.
use super::*;
use pomelo_core::task::preparation::PreparationPermit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
pub(super) fn worker_limit() -> usize {
    pomelo_core::task::preparation::worker_limit()
}
struct Permit {
    _permit: PreparationPermit,
}
impl Permit {
    fn acquire(cancel: &CancellationToken) -> Result<Self, SolidPrepareError> {
        Ok(Self {
            _permit: PreparationPermit::acquire(cancel).map_err(|e| Builder::error(e.into()))?,
        })
    }
}
fn reserve<T>(out: &mut Vec<T>, count: usize) -> Result<(), SolidPrepareError> {
    out.try_reserve_exact(count)
        .map_err(|_| Builder::error(SolidError::Allocation))
}
fn memory_error(error: &SolidPrepareError) -> bool {
    matches!(
        error,
        SolidPrepareError::Scene(SolidError::Limit { .. })
            | SolidPrepareError::Geometry {
                source: SolidError::Limit { .. },
                ..
            }
    )
}
impl PreparedBoard3d {
    /// Prepare on at most `max_workers` CPU threads. One requests serial preparation;
    /// the default also reserves CPU capacity for UI/rendering and caps workers at four.
    pub fn preview_with_workers(
        board: &BoardScene,
        preview: PreviewStackup,
        limits: SolidLimits,
        cancel: &CancellationToken,
        max_workers: usize,
    ) -> Result<Self, SolidPrepareError> {
        cancel.check().map_err(|e| Builder::error(e.into()))?;
        let workers = max_workers.clamp(1, worker_limit());
        let work = board
            .segments
            .len()
            .saturating_add(
                board
                    .zones
                    .iter()
                    .map(|z| z.mesh.vertices.len())
                    .sum::<usize>(),
            )
            .saturating_add(board.pins.iter().map(|p| p.pads.len()).sum::<usize>())
            .saturating_add(board.vias.iter().map(|v| v.pads.len() + 1).sum::<usize>());
        if workers > 1 && work >= 10_000 {
            match Self::prepare_parallel(board, preview, limits, cancel, workers) {
                // A skewed layer or a small allowance can outgrow its share. Drop all
                // partial outputs before retrying with the original serial budget.
                Err(e) if memory_error(&e) => {}
                result => return result,
            }
        }
        let _permit = Permit::acquire(cancel)?;
        Self::preview_partition(board, preview, limits, cancel, None)
    }

    fn prepare_parallel(
        board: &BoardScene,
        preview: PreviewStackup,
        limits: SolidLimits,
        cancel: &CancellationToken,
        workers: usize,
    ) -> Result<Self, SolidPrepareError> {
        let mut jobs: Vec<_> = board
            .layers
            .iter()
            .filter(|l| l.id.is_copper() && l.function != LayerFunction::Dielectric)
            .enumerate()
            .map(|(order, l)| (order, l.id, 1usize))
            .collect();
        if jobs.len() < 2 {
            let _permit = Permit::acquire(cancel)?;
            return Self::preview_partition(board, preview, limits, cancel, None);
        }
        // Weights only distribute the shared ceiling. They are not permission to
        // exceed it; each partition's existing checks enforce its exclusive share.
        for job in &mut jobs {
            cancel.check().map_err(|e| Builder::error(e.into()))?;
            let layer = job.1;
            job.2 += board
                .zones
                .iter()
                .filter(|z| z.layer == layer)
                .map(|z| z.mesh.vertices.len().saturating_mul(16))
                .sum::<usize>();
            job.2 += board
                .segments
                .iter()
                .filter(|s| s.layer == layer)
                .count()
                .saturating_mul(128);
            job.2 += board
                .pins
                .iter()
                .flat_map(|p| &p.pads)
                .filter(|p| p.layer == layer)
                .count()
                .saturating_mul(32);
            job.2 += board
                .vias
                .iter()
                .flat_map(|v| v.pads.iter())
                .filter(|p| p.layer == layer)
                .count()
                .saturating_mul(32);
            job.2 += board
                .vias
                .iter()
                .filter(|v| v.start_layer == Some(layer))
                .count()
                .saturating_mul(32);
        }
        let total: u128 = jobs.iter().map(|j| j.2 as u128).sum();
        // Keep one eighth for merge metadata and the rebuilt spatial indexes.
        let allowance = limits.max_allocation_bytes / 8 * 7;
        let mut jobs: Vec<_> = jobs
            .into_iter()
            .map(|(order, layer, weight)| {
                (
                    order,
                    layer,
                    weight,
                    (allowance as u128 * weight as u128 / total) as usize,
                )
            })
            .collect();
        jobs.sort_by_key(|j| std::cmp::Reverse(j.2));
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let batches = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            let mut spawn_failed = false;
            for _ in 0..workers.min(jobs.len()) {
                let jobs = &jobs;
                let next = &next;
                let failed = &failed;
                let handle = std::thread::Builder::new()
                    .name("pcb-3d-prepare".into())
                    .spawn_scoped(scope, move || {
                        let mut done = Vec::new();
                        loop {
                            if failed.load(Ordering::Relaxed) {
                                break;
                            }
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(&(order, layer, _, bytes)) = jobs.get(i) else {
                                break;
                            };
                            let result = Permit::acquire(cancel).and_then(|_permit| {
                                Self::preview_partition(
                                    board,
                                    preview,
                                    SolidLimits {
                                        max_allocation_bytes: bytes,
                                        ..limits
                                    },
                                    cancel,
                                    Some(layer),
                                )
                            });
                            if result.is_err() {
                                failed.store(true, Ordering::Relaxed);
                            }
                            done.push((order, result));
                        }
                        done
                    });
                match handle {
                    Ok(handle) => handles.push(handle),
                    Err(_) => {
                        failed.store(true, Ordering::Relaxed);
                        spawn_failed = true;
                        break;
                    }
                }
            }
            let mut batches = Vec::new();
            let mut panicked = false;
            for handle in handles {
                match handle.join() {
                    Ok(batch) => batches.extend(batch),
                    Err(_) => {
                        failed.store(true, Ordering::Relaxed);
                        panicked = true;
                    }
                }
            }
            if spawn_failed {
                return Err(Builder::error(SolidError::Allocation));
            }
            if panicked {
                return Err(Builder::error(SolidError::Invalid("PREPARATION_WORKER")));
            }
            Ok(batches)
        })?;
        cancel.check().map_err(|e| Builder::error(e.into()))?;
        let _merge_permit = Permit::acquire(cancel)?;
        let mut batches = batches;
        batches.sort_by_key(|b| b.0);
        // Resolve every error before merging; partially completed jobs never publish.
        let mut results: Vec<Self> = batches
            .into_iter()
            .map(|(_, r)| r)
            .collect::<Result<_, _>>()?;
        let mut counts = [0usize; 4];
        let mut resident = 0usize;
        for scene in &mut results {
            // Partition indexes are no longer used. Free them before rebuilding.
            scene.source_index = spatial::SpatialIndex3d::default();
            scene.page_index = spatial::SpatialIndex3d::default();
            resident = resident.saturating_add(scene.allocation_bytes());
            for (n, value) in counts.iter_mut().zip([
                scene.meshes.len(),
                scene.pages.len(),
                scene.sources.len(),
                scene.parts.len(),
            ]) {
                *n = n.saturating_add(value);
            }
        }
        let merge_bytes = (128 * size_of::<SolidPrepareError>()
            + jobs.len() * size_of::<LayerId>())
        .saturating_add(counts[0].saturating_mul(size_of::<Mesh3d>()))
        .saturating_add(counts[1].saturating_mul(size_of::<Page3d>()))
        .saturating_add(counts[2].saturating_mul(size_of::<Source3d>()))
        .saturating_add(counts[3].saturating_mul(size_of::<SourcePart3d>()))
        .saturating_add(spatial::SpatialIndex3d::estimate_bytes(counts[2]))
        .saturating_add(spatial::SpatialIndex3d::estimate_bytes(counts[1]));
        if resident.saturating_add(merge_bytes) > limits.max_allocation_bytes {
            return Err(Builder::error(SolidError::Limit {
                resource: "parallel merge bytes",
                actual: resident.saturating_add(merge_bytes),
                limit: limits.max_allocation_bytes,
            }));
        }
        let mut meshes = Vec::new();
        let mut pages = Vec::new();
        let mut sources = Vec::new();
        let mut parts = Vec::new();
        reserve(&mut meshes, counts[0])?;
        reserve(&mut pages, counts[1])?;
        reserve(&mut sources, counts[2])?;
        reserve(&mut parts, counts[3])?;
        let mut warnings = Vec::new();
        reserve(&mut warnings, 128)?;
        let mut warning_count = 0;
        let mut bounds: Option<Bounds3> = None;
        for mut scene in results {
            cancel.check().map_err(|e| Builder::error(e.into()))?;
            let source_base = u32::try_from(sources.len())
                .map_err(|_| Builder::error(SolidError::Invalid("MATERIAL_ID_RANGE")))?;
            u32::try_from(sources.len().saturating_add(scene.sources.len()))
                .map_err(|_| Builder::error(SolidError::Invalid("MATERIAL_ID_RANGE")))?;
            let mesh_base = meshes.len();
            let part_base = parts.len();
            for mesh in &mut scene.meshes {
                for v in &mut mesh.vertices {
                    if v.owner[0] != 0 {
                        v.owner[0] += source_base;
                    }
                }
            }
            for page in &mut scene.pages {
                page.mesh += mesh_base;
                for id in &mut page.sources {
                    *id += source_base;
                }
                for instance in &mut page.instances {
                    if instance.owner[0] != 0 {
                        instance.owner[0] += source_base;
                    }
                }
            }
            for source in &mut scene.sources {
                source.parts.start += part_base;
                source.parts.end += part_base;
            }
            for part in &mut scene.parts {
                part.mesh += mesh_base;
            }
            if !scene.sources.is_empty() {
                if let Some(b) = &mut bounds {
                    for axis in 0..3 {
                        b.min[axis] = b.min[axis].min(scene.bounds.min[axis]);
                        b.max[axis] = b.max[axis].max(scene.bounds.max[axis]);
                    }
                } else {
                    bounds = Some(scene.bounds);
                }
            }
            meshes.extend(scene.meshes);
            pages.extend(scene.pages);
            sources.extend(scene.sources);
            parts.extend(scene.parts);
            warning_count += scene.warning_count;
            warnings.extend(
                scene
                    .warnings
                    .into_iter()
                    .take(128usize.saturating_sub(warnings.len())),
            );
        }
        let source_index =
            spatial::SpatialIndex3d::build(sources.len(), |i| sources[i].bounds, cancel)?;
        let page_index = spatial::SpatialIndex3d::build(pages.len(), |i| pages[i].bounds, cancel)?;
        let scene = Self {
            memory_reservation: None,
            meshes,
            pages,
            parts,
            source_index,
            page_index,
            sources,
            origin: board.bounds.center(),
            bounds: bounds.ok_or(SolidPrepareError::Scene(SolidError::Invalid(
                "EMPTY_3D_BOARD",
            )))?,
            layers: board
                .layers
                .iter()
                .filter(|l| l.id.is_copper() && l.function != LayerFunction::Dielectric)
                .map(|l| l.id)
                .collect(),
            warnings,
            warning_count,
            preview,
        };
        if scene.allocation_bytes() > limits.max_allocation_bytes {
            return Err(SolidPrepareError::Scene(SolidError::Limit {
                resource: "parallel merged bytes",
                actual: scene.allocation_bytes(),
                limit: limits.max_allocation_bytes,
            }));
        }
        Ok(scene)
    }
}
