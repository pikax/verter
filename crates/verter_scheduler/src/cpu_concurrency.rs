//! Per-call CPU-concurrency limiter.
//!
//! [`CpuConcurrencySemaphore`] is a hand-rolled counting semaphore built
//! from a `parking_lot::Mutex<usize>` (the count of free permits) plus a
//! `parking_lot::Condvar`. It LAYERS on top of the scheduler pool size +
//! DAG capacity: a submission holds a semaphore handle on each of its CPU
//! work nodes, and a worker acquires a *fresh* [`CpuConcurrencyPermit`]
//! per CPU task immediately before executing it. The permit releases on
//! `Drop` (RAII), so the slot is returned even if the task panics during
//! execution.
//!
//! # Why hand-rolled, not `parking_lot::Semaphore`
//!
//! The project forbids `parking_lot::Semaphore` (enforced by
//! `tests/cases/no_parking_lot_semaphore.rs`). A hand-rolled
//! `Mutex<usize>` + `Condvar` counter gives us exactly the permit
//! semantics we need with no second accounting truth:
//!
//! - [`CpuConcurrencyPermit`] is `#[must_use]` and non-`Clone` — a permit
//!   represents *one* held slot and cannot be duplicated.
//! - Release is RAII: the permit's `Drop` increments the free count and
//!   notifies one waiter. Unwinding on panic runs `Drop`, so a panicking
//!   holder still frees its slot (no leak, no deadlock of the recovery
//!   acquire).
//! - There is a single source of truth for available permits — the
//!   `Mutex<usize>` count — so the limiter never disagrees with itself.

use parking_lot::{Condvar, Mutex};

/// A hand-rolled counting semaphore limiting concurrent CPU tasks.
///
/// `acquire()` blocks until a permit is free and returns an RAII
/// [`CpuConcurrencyPermit`]; dropping the permit (normally or on unwind)
/// returns the slot and wakes one waiter.
pub struct CpuConcurrencySemaphore {
    /// Count of currently-available permits. Guarded by the mutex; the
    /// condvar is notified whenever a permit is returned.
    available: Mutex<usize>,
    /// Notified (one waiter) each time a permit becomes available.
    permit_returned: Condvar,
}

impl CpuConcurrencySemaphore {
    /// Creates a semaphore with `capacity` permits.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0`. A zero-capacity CPU-concurrency cap is a
    /// caller-contract violation: every `acquire()` would block forever
    /// (no permit can ever become free), deadlocking the CPU pool. The
    /// assert is RELEASE-ACTIVE — the cap is configured once at pool
    /// construction, so the check is off the hot path and a misconfigured
    /// cap must fail loudly in release builds, not silently deadlock.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity >= 1,
            "CpuConcurrencySemaphore capacity must be >= 1; a 0-permit \
             semaphore would deadlock every acquire (no permit can ever \
             become available)"
        );
        Self {
            available: Mutex::new(capacity),
            permit_returned: Condvar::new(),
        }
    }

    /// Blocks until a permit is available, then takes it and returns an
    /// RAII [`CpuConcurrencyPermit`]. The permit is released when dropped.
    ///
    /// `&self` so the semaphore is shared behind an `Arc`; many threads
    /// may call `acquire` concurrently.
    #[must_use = "the returned CpuConcurrencyPermit holds a slot until it is dropped"]
    pub fn acquire(&self) -> CpuConcurrencyPermit<'_> {
        let mut available = self.available.lock();
        while *available == 0 {
            // Park until a returned permit notifies us. `parking_lot`'s
            // `Condvar::wait` is not subject to spurious wakeups across
            // platforms the way std's can be, but the `while` loop
            // re-checks the predicate regardless, so a spurious wake is
            // harmless.
            self.permit_returned.wait(&mut available);
        }
        *available -= 1;
        CpuConcurrencyPermit { semaphore: self }
    }

    /// Returns one permit to the pool and wakes one waiter. Called only
    /// by [`CpuConcurrencyPermit::drop`].
    fn release(&self) {
        let mut available = self.available.lock();
        *available += 1;
        // Notify exactly one waiter — one returned permit satisfies at
        // most one blocked `acquire`.
        self.permit_returned.notify_one();
    }
}

/// An RAII guard representing one held CPU-concurrency permit.
///
/// Non-`Clone` (a permit is exactly one held slot) and `#[must_use]`
/// (dropping it immediately would defeat the limit). The permit borrows
/// its semaphore, so it cannot outlive it. `Drop` returns the slot — on
/// both the normal path and stack-unwind on panic.
#[must_use = "dropping the permit immediately releases the slot, defeating the limit"]
pub struct CpuConcurrencyPermit<'sem> {
    semaphore: &'sem CpuConcurrencySemaphore,
}

impl Drop for CpuConcurrencyPermit<'_> {
    fn drop(&mut self) {
        self.semaphore.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn acquire_decrements_release_restores() {
        let sem = CpuConcurrencySemaphore::new(1);
        {
            let _p = sem.acquire();
            assert_eq!(*sem.available.lock(), 0, "permit taken");
        }
        assert_eq!(*sem.available.lock(), 1, "permit returned on drop");
    }

    #[test]
    #[should_panic(expected = "capacity must be >= 1")]
    fn new_zero_capacity_panics() {
        // A 0-permit semaphore would deadlock every acquire; `new` must
        // reject it loudly (RELEASE-ACTIVE assert), not construct a
        // permanently-blocking limiter.
        let _ = CpuConcurrencySemaphore::new(0);
    }

    #[test]
    fn full_capacity_concurrent() {
        let sem = CpuConcurrencySemaphore::new(3);
        let _a = sem.acquire();
        let _b = sem.acquire();
        let _c = sem.acquire();
        assert_eq!(*sem.available.lock(), 0);
    }

    #[test]
    fn over_capacity_blocks_then_proceeds() {
        let sem = Arc::new(CpuConcurrencySemaphore::new(1));
        let p = sem.acquire();

        let proceeded = Arc::new(AtomicUsize::new(0));
        let sem2 = Arc::clone(&sem);
        let flag = Arc::clone(&proceeded);
        let h = thread::spawn(move || {
            let _p2 = sem2.acquire();
            flag.store(1, Ordering::SeqCst);
        });

        thread::sleep(Duration::from_millis(50));
        assert_eq!(proceeded.load(Ordering::SeqCst), 0, "second acquire blocks");

        drop(p);
        let start = Instant::now();
        while proceeded.load(Ordering::SeqCst) == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "blocked acquire never proceeded"
            );
            thread::sleep(Duration::from_millis(5));
        }
        h.join().unwrap();
    }
}
