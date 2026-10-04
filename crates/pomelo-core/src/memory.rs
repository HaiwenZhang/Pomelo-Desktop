//! Shared byte reservations for simultaneously resident and temporary buffers.
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug)]
struct State {
    used: usize,
    limit: usize,
    peak: usize,
}

/// Clones share one ceiling; a reservation returns its bytes when dropped.
#[derive(Debug, Clone)]
pub struct MemoryBudget(Arc<Mutex<State>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("MEMORY_BUDGET_LIMIT actual={actual} limit={limit}")]
pub struct MemoryLimit {
    pub actual: usize,
    pub limit: usize,
}

#[derive(Debug)]
pub struct MemoryReservation {
    budget: MemoryBudget,
    bytes: usize,
}

impl MemoryBudget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(State {
            used: 0,
            limit,
            peak: 0,
        })))
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    pub fn used(&self) -> usize {
        self.state().used
    }
    pub fn peak(&self) -> usize {
        self.state().peak
    }
    pub fn limit(&self) -> usize {
        self.state().limit
    }
    pub fn available(&self) -> usize {
        let state = self.state();
        state.limit.saturating_sub(state.used)
    }

    /// A lower ceiling leaves live reservations valid but blocks additional growth.
    pub fn set_limit(&self, limit: usize) {
        self.state().limit = limit;
    }

    pub fn reserve(&self, bytes: usize) -> Result<MemoryReservation, MemoryLimit> {
        self.acquire(bytes)?;
        Ok(MemoryReservation {
            budget: self.clone(),
            bytes,
        })
    }

    fn acquire(&self, bytes: usize) -> Result<(), MemoryLimit> {
        // Zero-byte work is allowed even while live allocations exceed a lowered ceiling.
        if bytes == 0 {
            return Ok(());
        }
        let mut state = self.state();
        let limit = state.limit;
        let actual = state.used.checked_add(bytes).ok_or(MemoryLimit {
            actual: usize::MAX,
            limit,
        })?;
        if actual > limit {
            return Err(MemoryLimit { actual, limit });
        }
        state.used = actual;
        state.peak = state.peak.max(actual);
        Ok(())
    }
}

impl MemoryReservation {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    /// Grow before allocation and shrink after old buffers have been released.
    /// Failed growth leaves the original reservation unchanged.
    pub fn resize(&mut self, bytes: usize) -> Result<(), MemoryLimit> {
        if bytes > self.bytes {
            self.budget.acquire(bytes - self.bytes)?;
        } else {
            self.budget.state().used -= self.bytes - bytes;
        }
        self.bytes = bytes;
        Ok(())
    }
}
impl Drop for MemoryReservation {
    fn drop(&mut self) {
        self.budget.state().used -= self.bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stages_share_capacity_and_failed_growth_preserves_live_reservations() {
        let budget = MemoryBudget::new(100);
        let mut scene = budget.reserve(60).unwrap();
        let scratch = budget.clone().reserve(30).unwrap();
        assert!(scene.resize(80).is_err());
        assert_eq!(scene.bytes(), 60);
        assert_eq!(budget.used(), 90);
        drop(scratch);
        scene.resize(80).unwrap();
        assert_eq!(budget.peak(), 90);
        drop(scene);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn pressure_blocks_growth_but_allows_release_and_zero_byte_work() {
        let budget = MemoryBudget::new(100);
        let mut live = budget.reserve(80).unwrap();
        budget.set_limit(40);
        assert!(budget.reserve(1).is_err());
        assert!(budget.reserve(0).is_ok());
        live.resize(20).unwrap();
        assert_eq!(budget.available(), 20);
        assert!(budget.reserve(21).is_err());
    }
    #[test]
    fn simultaneous_workers_cannot_overdraw_one_shared_ceiling() {
        let budget = MemoryBudget::new(64);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let budget = &budget;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        budget.reserve(16)
                    })
                })
                .collect();
            let reservations: Vec<_> = handles
                .into_iter()
                .filter_map(|handle| handle.join().unwrap().ok())
                .collect();
            assert_eq!(reservations.len(), 4);
            assert_eq!(budget.used(), 64);
        });
        assert_eq!(budget.used(), 0);
    }
}
