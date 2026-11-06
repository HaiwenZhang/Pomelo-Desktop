//! Residency accounting shared by cache decisions and the native buffer owner.
use std::sync::{Mutex, MutexGuard, OnceLock};

fn memory_sampler() -> &'static Mutex<sysinfo::System> {
    static MEMORY: OnceLock<Mutex<sysinfo::System>> = OnceLock::new();
    MEMORY.get_or_init(|| Mutex::new(sysinfo::System::new()))
}
pub(super) fn system_memory() -> (u64, u64) {
    let mut system = memory_sampler().lock().unwrap_or_else(|p| p.into_inner());
    system.refresh_memory();
    (system.total_memory(), system.available_memory())
}
fn upload_headroom(total: u64, available: u64) -> usize {
    if total == 0 && available == 0 {
        256 * 1024 * 1024
    } else {
        usize::try_from(available / 2).unwrap_or(usize::MAX)
    }
}
/// Keep the process-wide sampling lock through allocation, so windows cannot reuse a stale grant.
pub(super) fn upload_memory(
    bytes: usize,
) -> Result<MutexGuard<'static, sysinfo::System>, pomelo_core::memory::MemoryLimit> {
    let mut system = memory_sampler().lock().unwrap_or_else(|p| p.into_inner());
    system.refresh_memory();
    let limit = upload_headroom(system.total_memory(), system.available_memory());
    if bytes > limit {
        return Err(pomelo_core::memory::MemoryLimit {
            actual: bytes,
            limit,
        });
    }
    Ok(system)
}
pub(super) struct GeometryCache {
    meshes: Vec<Mesh>,
    pages: Vec<Page>,
    frame: u64,
    pub resident_bytes: usize,
}
struct Mesh {
    bytes: usize,
    users: usize,
    resident: bool,
    visible: bool,
}
struct Page {
    mesh: usize,
    bytes: usize,
    resident: bool,
    last_used: u64,
}
pub(super) struct EvictedPage {
    pub page: Option<usize>,
    pub mesh: Option<usize>,
    pub bytes: usize,
}

/// A soft target for reusable geometry; visible pages may exceed it.
pub(super) fn cache_target(total: u64, available: u64, recommended: u64) -> usize {
    let physical = if total == 0 {
        256 * 1024 * 1024
    } else {
        total / 8
    };
    let working = if recommended == 0 {
        physical
    } else {
        recommended / 4
    };
    let headroom = if total == 0 && available == 0 {
        physical
    } else {
        available / 4
    };
    usize::try_from(physical.min(working).min(headroom)).unwrap_or(usize::MAX)
}

impl GeometryCache {
    pub fn new(
        mesh_bytes: impl IntoIterator<Item = usize>,
        pages: impl IntoIterator<Item = (usize, usize)>,
    ) -> Self {
        Self {
            meshes: mesh_bytes
                .into_iter()
                .map(|bytes| Mesh {
                    bytes,
                    users: 0,
                    resident: false,
                    visible: false,
                })
                .collect(),
            pages: pages
                .into_iter()
                .map(|(mesh, bytes)| Page {
                    mesh,
                    bytes,
                    resident: false,
                    last_used: 0,
                })
                .collect(),
            frame: 0,
            resident_bytes: 0,
        }
    }
    pub fn begin_frame(&mut self, visible: &[bool]) {
        if self.frame == u64::MAX {
            for page in &mut self.pages {
                page.last_used = 0;
            }
            self.frame = 0;
        }
        self.frame += 1;
        for (page, &visible) in self.pages.iter_mut().zip(visible) {
            if visible {
                page.last_used = self.frame;
            }
        }
    }
    pub fn upload_bytes(&self, id: usize) -> usize {
        let p = &self.pages[id];
        if p.resident {
            0
        } else {
            p.bytes
                + if !self.meshes[p.mesh].resident {
                    self.meshes[p.mesh].bytes
                } else {
                    0
                }
        }
    }
    pub fn loaded(&mut self, id: usize) {
        if self.pages[id].resident {
            return;
        }
        self.resident_bytes += self.upload_bytes(id);
        let p = &mut self.pages[id];
        p.resident = true;
        p.last_used = self.frame;
        self.meshes[p.mesh].users += 1;
        self.meshes[p.mesh].resident = true;
    }
    pub fn geometry_loaded(&mut self, id: usize) {
        let mesh = &mut self.meshes[id];
        if !mesh.resident {
            self.resident_bytes += mesh.bytes;
            mesh.resident = true;
        }
    }
    pub fn trim(&mut self, visible: &[bool], target: usize) -> Vec<EvictedPage> {
        if self.resident_bytes <= target {
            return vec![];
        }
        for mesh in &mut self.meshes {
            mesh.visible = false;
        }
        for (p, &visible) in self.pages.iter().zip(visible) {
            if visible {
                self.meshes[p.mesh].visible = true;
            }
        }
        let mut candidates: Vec<_> = self
            .pages
            .iter()
            .enumerate()
            .filter(|(i, p)| p.resident && !visible[*i])
            .map(|(i, p)| (p.last_used, i))
            .collect();
        candidates.sort_unstable();
        let mut evicted = Vec::new();
        for (_, id) in candidates {
            if self.resident_bytes <= target {
                break;
            }
            let p = &mut self.pages[id];
            p.resident = false;
            let mesh = &mut self.meshes[p.mesh];
            mesh.users -= 1;
            let free_mesh = mesh.users == 0 && !mesh.visible;
            let freed = p.bytes + if free_mesh { mesh.bytes } else { 0 };
            if free_mesh {
                mesh.resident = false;
            }
            self.resident_bytes -= freed;
            evicted.push(EvictedPage {
                page: Some(id),
                mesh: free_mesh.then_some(p.mesh),
                bytes: freed,
            });
        }
        for (id, mesh) in self.meshes.iter_mut().enumerate() {
            if self.resident_bytes <= target {
                break;
            }
            if mesh.resident && mesh.users == 0 && !mesh.visible {
                mesh.resident = false;
                self.resident_bytes -= mesh.bytes;
                evicted.push(EvictedPage {
                    page: None,
                    mesh: Some(id),
                    bytes: mesh.bytes,
                });
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocations_share_one_sampling_lock_across_renderer_contexts() {
        let grant = upload_memory(0).unwrap();
        assert!(memory_sampler().try_lock().is_err());
        drop(grant);
        assert!(upload_memory(0).is_ok());
        let _ = system_memory();
        assert_eq!(upload_headroom(16 * 1024, 4 * 1024), 2 * 1024);
        assert_eq!(upload_headroom(16 * 1024, 0), 0);
        assert!(upload_headroom(0, 0) > 0);
    }
    #[test]
    fn pressure_evicts_oldest_hidden_pages_and_keeps_shared_visible_geometry() {
        let mut cache = GeometryCache::new([100, 200], [(0, 10), (0, 20), (1, 30)]);
        cache.begin_frame(&[true, true, true]);
        for id in 0..3 {
            cache.loaded(id);
        }
        assert_eq!(cache.resident_bytes, 360);
        cache.begin_frame(&[false, true, false]);
        let freed = cache.trim(&[false, true, false], 150);
        assert_eq!(
            freed.iter().map(|e| e.page).collect::<Vec<_>>(),
            [Some(0), Some(2)]
        );
        assert_eq!(freed[0].mesh, None);
        assert_eq!(freed[1].mesh, Some(1));
        assert_eq!(freed.iter().map(|e| e.bytes).sum::<usize>(), 240);
        assert_eq!(cache.resident_bytes, 120);
        assert_eq!(cache.upload_bytes(0), 10);
        assert_eq!(cache.upload_bytes(2), 230);
        cache.loaded(0);
        cache.loaded(2);
        assert_eq!(cache.resident_bytes, 360);
        assert!(cache.trim(&[true, true, true], 0).is_empty());
    }
    #[test]
    fn cache_hits_do_not_double_charge_and_only_pressure_causes_eviction() {
        let mut cache = GeometryCache::new([100], [(0, 10), (0, 20)]);
        cache.begin_frame(&[true, false]);
        cache.geometry_loaded(0);
        cache.geometry_loaded(0);
        assert_eq!(cache.resident_bytes, 100);
        cache.loaded(0);
        cache.loaded(0);
        cache.begin_frame(&[false, true]);
        cache.loaded(1);
        assert_eq!(cache.resident_bytes, 130);
        assert!(cache.trim(&[false, false], 130).is_empty());
        let freed = cache.trim(&[false, false], 125);
        assert_eq!(freed[0].page, Some(0));
        assert_eq!(cache.resident_bytes, 120);
        let freed = cache.trim(&[false, false], 0);
        assert_eq!(freed[0].mesh, Some(0));
        assert_eq!(cache.resident_bytes, 0);
    }
    #[test]
    fn missing_visible_page_keeps_its_shared_mesh_without_an_instance_lease() {
        let mut cache = GeometryCache::new([100], [(0, 10), (0, 20)]);
        cache.begin_frame(&[true, false]);
        cache.loaded(0);
        cache.begin_frame(&[false, true]);
        let freed = cache.trim(&[false, true], 0);
        assert_eq!(freed.len(), 1);
        assert_eq!(freed[0].page, Some(0));
        assert_eq!(freed[0].mesh, None);
        assert_eq!(cache.resident_bytes, 100);
        assert_eq!(cache.upload_bytes(1), 20);
        let freed = cache.trim(&[false, false], 0);
        assert_eq!(freed.len(), 1);
        assert_eq!(freed[0].page, None);
        assert_eq!(freed[0].mesh, Some(0));
        assert_eq!(cache.resident_bytes, 0);
        cache.loaded(1);
        assert_eq!(cache.resident_bytes, 120);
    }

    #[test]
    fn sixteen_gib_policy_adapts_to_system_and_device_pressure() {
        let gib = 1024 * 1024 * 1024;
        assert_eq!(cache_target(16 * gib, 8 * gib, 12 * gib), 2 * gib as usize);
        assert_eq!(cache_target(16 * gib, gib, 12 * gib), (gib / 4) as usize);
        assert_eq!(cache_target(16 * gib, 8 * gib, 4 * gib), gib as usize);
        assert_eq!(cache_target(16 * gib, 0, 12 * gib), 0);
    }
}
