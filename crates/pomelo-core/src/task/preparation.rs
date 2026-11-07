//! Process-wide CPU permits for bounded geometry preparation.
use super::{CancellationToken, Cancelled};
use std::{
    sync::{Condvar, Mutex, OnceLock},
    time::Duration,
};

fn cpu_limit(cpus: usize) -> usize {
    let reserve = if cpus <= 2 {
        1
    } else if cpus <= 8 {
        2
    } else {
        cpus.div_ceil(4)
    };
    cpus.saturating_sub(reserve).clamp(1, 4)
}
/// Leave CPU capacity for input/rendering, including on machines with few cores.
pub fn worker_limit() -> usize {
    cpu_limit(std::thread::available_parallelism().map_or(1, usize::from))
}
#[derive(Default)]
struct State {
    active: usize,
    scratch: usize,
    solo: bool,
}
struct Pool {
    state: Mutex<State>,
    changed: Condvar,
}
static POOL: OnceLock<Pool> = OnceLock::new();
/// Releases its CPU slot and scratch admission on error, cancellation or unwinding.
pub struct PreparationPermit {
    pool: &'static Pool,
    scratch: usize,
    solo: bool,
}
impl PreparationPermit {
    /// Reserve a CPU slot; the caller already accounts for its memory independently.
    pub fn acquire(cancel: &CancellationToken) -> Result<Self, Cancelled> {
        Self::with_scratch(cancel, 0, usize::MAX)
    }
    /// Admit conservative temporary storage across documents. A job larger than
    /// the soft physical-memory allowance runs alone, preserving serial behavior.
    pub fn with_scratch(
        cancel: &CancellationToken,
        scratch: usize,
        allowance: usize,
    ) -> Result<Self, Cancelled> {
        let pool = POOL.get_or_init(|| Pool {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        let solo = wait_for(pool, worker_limit(), scratch, allowance, cancel)?;
        Ok(Self {
            pool,
            scratch,
            solo,
        })
    }
}
fn wait_for(
    pool: &Pool,
    slots: usize,
    scratch: usize,
    allowance: usize,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let mut state = pool.state.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        cancel.check()?;
        let total = state.scratch.checked_add(scratch);
        let memory = !state.solo && (total.is_some_and(|v| v <= allowance) || state.active == 0);
        if state.active < slots && memory {
            state.active += 1;
            state.scratch += scratch;
            let solo = scratch > allowance;
            state.solo = solo;
            return Ok(solo);
        }
        state = pool
            .changed
            .wait_timeout(state, Duration::from_millis(10))
            .unwrap_or_else(|p| p.into_inner())
            .0;
    }
}
impl Drop for PreparationPermit {
    fn drop(&mut self) {
        let mut state = self.pool.state.lock().unwrap_or_else(|p| p.into_inner());
        state.active -= 1;
        state.scratch -= self.scratch;
        if self.solo {
            state.solo = false;
        }
        self.pool.changed.notify_all();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserves_cpu_headroom_on_small_and_large_machines() {
        assert_eq!(
            [1, 2, 3, 4, 8, 12, 24, 64].map(cpu_limit),
            [1, 1, 1, 2, 4, 4, 4, 4]
        );
    }
    #[test]
    fn cancelled_wait_does_not_consume_a_permit() {
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(PreparationPermit::acquire(&cancel).is_err());
        assert!(PreparationPermit::acquire(&CancellationToken::default()).is_ok());
    }
    #[test]
    fn a_cancelled_waiter_does_not_take_a_saturated_cpu_slot() {
        let pool = Pool {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        };
        let token = CancellationToken::default();
        assert!(!wait_for(&pool, 1, 60, 100, &token).unwrap());
        std::thread::scope(|scope| {
            let (started, ready) = std::sync::mpsc::channel();
            let waiter_pool = &pool;
            let waiter_token = &token;
            let thread = scope.spawn(move || {
                started.send(()).unwrap();
                wait_for(waiter_pool, 1, 30, 100, waiter_token)
            });
            ready.recv().unwrap();
            token.cancel();
            assert!(thread.join().unwrap().is_err());
        });
        let state = pool.state.lock().unwrap();
        assert_eq!((state.active, state.scratch), (1, 60));
    }
    #[test]
    fn an_oversized_workspace_requires_exclusive_admission() {
        let pool = Pool {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        };
        let token = CancellationToken::default();
        assert!(wait_for(&pool, 4, 200, 100, &token).unwrap());
        let (started, ready) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let thread = scope.spawn(|| {
                started.send(()).unwrap();
                wait_for(&pool, 4, 0, usize::MAX, &token)
            });
            ready.recv().unwrap();
            token.cancel();
            assert!(thread.join().unwrap().is_err());
        });
        let state = pool.state.lock().unwrap();
        assert_eq!((state.active, state.scratch, state.solo), (1, 200, true));
    }
}
