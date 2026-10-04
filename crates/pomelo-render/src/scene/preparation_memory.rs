//! Adaptive shared CPU allowances for native geometry preparation.
use crate::tracks::PrepareError;
use pomelo_core::memory::{MemoryBudget, MemoryReservation};

pub struct PreparationMemory {
    budget: MemoryBudget,
    system: sysinfo::System,
}

/// Geometry whose reservation follows its allocation through every Arc owner.
/// Only native preparation outputs implement this sealed trait.
pub trait PreparedGeometry: sealed::Sealed {}

mod sealed {
    use super::MemoryReservation;
    pub trait Sealed {
        fn attach_reservation(&mut self, reservation: MemoryReservation);
    }
}

macro_rules! tracked_geometry {
    ($($geometry:ty),+ $(,)?) => { $(
        impl sealed::Sealed for $geometry {
            fn attach_reservation(&mut self, reservation: MemoryReservation) {
                self.memory_reservation = Some(reservation);
            }
        }
        impl PreparedGeometry for $geometry {}
    )+ };
}
tracked_geometry!(
    crate::tracks::PreparedTracks,
    crate::copper::PreparedCopper,
    crate::pads::PreparedPads
);
impl sealed::Sealed for crate::drills::PreparedDrills {
    fn attach_reservation(&mut self, reservation: MemoryReservation) {
        sealed::Sealed::attach_reservation(&mut self.geometry, reservation);
    }
}
impl PreparedGeometry for crate::drills::PreparedDrills {}

// Leave half the physical memory outside this managed pool and a quarter of
// currently available memory for untracked application/OS work. Missing resource
// telemetry retains a bounded fallback rather than treating zero as infinite RAM.
fn adaptive_limit(used: usize, total: u64, available: u64) -> usize {
    if total == 0 {
        return used
            .saturating_add(512 * 1024 * 1024)
            .min(2 * 1024 * 1024 * 1024);
    }
    let total = usize::try_from(total).unwrap_or(usize::MAX);
    let available = usize::try_from(available).unwrap_or(usize::MAX).min(total);
    (total / 2).min(used.saturating_add(available - available / 4))
}

impl PreparationMemory {
    pub fn prepare_tracks(
        &mut self,
        build: impl FnOnce(
            crate::tracks::TraceLimits,
        ) -> Result<crate::tracks::PreparedTracks, PrepareError>,
    ) -> Result<crate::tracks::PreparedTracks, PrepareError> {
        self.prepare(
            |bytes| {
                build(crate::tracks::TraceLimits {
                    max_instances: u32::MAX as usize,
                    max_bytes: bytes,
                })
            },
            crate::tracks::PreparedTracks::allocation_bytes,
        )
    }
    pub fn prepare_copper(
        &mut self,
        build: impl FnOnce(
            crate::copper::CopperLimits,
        ) -> Result<crate::copper::PreparedCopper, PrepareError>,
    ) -> Result<crate::copper::PreparedCopper, PrepareError> {
        self.prepare(
            |bytes| {
                build(crate::copper::CopperLimits {
                    max_vertices: u32::MAX as usize,
                    max_indices: u32::MAX as usize,
                    max_zones: u32::MAX as usize,
                    max_bytes: bytes,
                })
            },
            crate::copper::PreparedCopper::allocation_bytes,
        )
    }
    pub fn prepare_pads(
        &mut self,
        build: impl FnOnce(crate::pads::PadLimits) -> Result<crate::pads::PreparedPads, PrepareError>,
    ) -> Result<crate::pads::PreparedPads, PrepareError> {
        self.prepare(
            |bytes| {
                build(crate::pads::PadLimits {
                    max_pads: u32::MAX as usize,
                    max_bytes: bytes,
                })
            },
            crate::pads::PreparedPads::allocation_bytes,
        )
    }
    pub fn new(budget: MemoryBudget) -> Self {
        Self {
            budget,
            system: sysinfo::System::new(),
        }
    }

    pub fn prepare<T: PreparedGeometry>(
        &mut self,
        build: impl FnOnce(usize) -> Result<T, PrepareError>,
        allocation_bytes: impl FnOnce(&T) -> usize,
    ) -> Result<T, PrepareError> {
        self.system.refresh_memory();
        self.budget.set_limit(adaptive_limit(
            self.budget.used(),
            self.system.total_memory(),
            self.system.available_memory(),
        ));
        self.prepare_reserved(build, allocation_bytes)
    }

    fn prepare_reserved<T: PreparedGeometry>(
        &mut self,
        build: impl FnOnce(usize) -> Result<T, PrepareError>,
        allocation_bytes: impl FnOnce(&T) -> usize,
    ) -> Result<T, PrepareError> {
        let allowance = self.budget.available();
        // Hold the grant through construction, including temporary allocations.
        // A failed/cancelled builder automatically returns its whole grant.
        let mut reservation =
            self.budget
                .reserve(allowance)
                .map_err(|error| PrepareError::Limit {
                    actual: error.actual,
                    limit: error.limit,
                })?;
        let mut prepared = build(allowance)?;
        reservation
            .resize(allocation_bytes(&prepared))
            .map_err(|error| PrepareError::Limit {
                actual: error.actual,
                limit: error.limit,
            })?;
        prepared.attach_reservation(reservation);
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    struct Allocation {
        bytes: usize,
        memory_reservation: Option<MemoryReservation>,
    }
    impl Allocation {
        fn new(bytes: usize) -> Self {
            Self {
                bytes,
                memory_reservation: None,
            }
        }
    }
    tracked_geometry!(Allocation);
    #[test]
    fn resource_allowance_grows_with_headroom_and_shrinks_under_pressure() {
        assert_eq!(adaptive_limit(100, 1000, 800), 500);
        assert_eq!(adaptive_limit(100, 1000, 200), 250);
        assert_eq!(adaptive_limit(400, 1000, 0), 400);
        assert_eq!(adaptive_limit(600, 1000, 100), 500);
        assert!(adaptive_limit(0, 0, 0) <= 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn native_geometry_remains_charged_until_the_last_pending_frame_releases_it() {
        use bytemuck::Zeroable;
        let budget = MemoryBudget::new(128);
        let mut controller = PreparationMemory::new(budget.clone());
        let source = controller
            .prepare_reserved(
                |_| {
                    Ok(crate::tracks::PreparedTracks {
                        instances: vec![crate::tracks::TraceInstance::zeroed()],
                        batches: vec![],
                        memory_reservation: None,
                    })
                },
                crate::tracks::PreparedTracks::allocation_bytes,
            )
            .unwrap();
        let document_source = Arc::new(source);
        let pending_frame = Arc::clone(&document_source);
        drop(controller);
        drop(document_source);
        assert_eq!(budget.used(), 128);
        assert!(budget.reserve(1).is_err());
        drop(pending_frame);
        assert_eq!(budget.used(), 0);
        assert!(budget.reserve(128).is_ok());
    }

    #[test]
    fn document_residency_survives_stage_failure_and_returns_on_close() {
        let budget = MemoryBudget::new(100);
        let mut first = PreparationMemory::new(budget.clone());
        let first_geometry = first
            .prepare_reserved(
                |allowance| {
                    assert_eq!(allowance, 100);
                    Ok(Allocation::new(60))
                },
                |value| value.bytes,
            )
            .unwrap();
        let mut second = PreparationMemory::new(budget.clone());
        assert!(matches!(
            second.prepare_reserved::<Allocation>(
                |allowance| {
                    assert_eq!(allowance, 40);
                    Err(PrepareError::Cancelled)
                },
                |value| value.bytes
            ),
            Err(PrepareError::Cancelled)
        ));
        assert_eq!(budget.used(), 60);
        let second_geometry = second
            .prepare_reserved(|_| Ok(Allocation::new(30)), |value| value.bytes)
            .unwrap();
        assert_eq!(budget.used(), 90);
        drop(first);
        assert_eq!(budget.used(), 90);
        let document_geometry = Arc::new(first_geometry);
        let pending_frame = Arc::clone(&document_geometry);
        drop(document_geometry);
        assert_eq!(budget.used(), 90);
        drop(pending_frame);
        assert_eq!(budget.used(), 30);
        drop(second);
        assert_eq!(budget.used(), 30);
        drop(second_geometry);
        assert_eq!(budget.used(), 0);
    }
}
