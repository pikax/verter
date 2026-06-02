//! `CpuConcurrencySemaphore` capacity + RAII + panic-release guards.
//!
//! Native-only: `cpu_concurrency` is gated `#[cfg(not(target_arch =
//! "wasm32"))]` in the crate root (the limiter caps the SCHEDULER CPU pool,
//! which is native-only — wasm runs the scheduler inline). This test file
//! therefore compiles only on native targets.
//!
//! The semaphore is a hand-rolled `parking_lot::Mutex<usize>` + `Condvar`
//! permit counter (NOT `parking_lot::Semaphore`). It LAYERS on top of the
//! scheduler pool size + DAG capacity: workers acquire a fresh permit per
//! CPU task at dispatch and release it (RAII) at task completion. These
//! tests pin the three load-bearing properties:
//!
//! 1. Capacity cap: N concurrent `acquire()` succeed, the (N+1)th blocks
//!    until a permit is released.
//! 2. RAII release on normal drop: dropping a permit frees the slot so a
//!    blocked acquirer proceeds.
//! 3. Panic-release: a permit released during stack-unwind on panic still
//!    frees the slot (the RAII `Drop` runs on unwind). A non-RAII
//!    implementation would leak the slot and deadlock the recovery
//!    acquire.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_scheduler::cpu_concurrency::{CpuConcurrencyPermit, CpuConcurrencySemaphore};

/// Capacity cap: with capacity 2, two `acquire()` calls succeed
/// immediately and a third blocks until one of the first two permits is
/// dropped. We observe the block by asserting the third acquire does not
/// complete while both permits are held, then completes promptly after a
/// release.
#[test]
fn capacity_cap_blocks_until_permit_released() {
    let sem = Arc::new(CpuConcurrencySemaphore::new(2));

    let p1 = sem.acquire();
    let p2 = sem.acquire();

    // A third acquire on another thread must block while both permits live.
    let acquired_third = Arc::new(AtomicUsize::new(0));
    let sem2 = Arc::clone(&sem);
    let flag = Arc::clone(&acquired_third);
    let handle = thread::spawn(move || {
        let _p3 = sem2.acquire();
        flag.store(1, Ordering::SeqCst);
        // Hold briefly so the main thread can observe the store.
        thread::sleep(Duration::from_millis(20));
    });

    // Give the spawned thread time to attempt the (blocked) acquire.
    thread::sleep(Duration::from_millis(80));
    assert_eq!(
        acquired_third.load(Ordering::SeqCst),
        0,
        "third acquire must block while capacity (2) is exhausted",
    );

    // Release one permit; the blocked acquire must now proceed.
    drop(p1);

    let start = Instant::now();
    while acquired_third.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "third acquire did not proceed after a permit was released — \
             RAII release or condvar notify is broken",
        );
        thread::sleep(Duration::from_millis(5));
    }

    drop(p2);
    handle.join().expect("third-acquire thread joined");
}

/// RAII normal-drop release: a single-permit semaphore is fully
/// serialised; dropping the permit unblocks the next acquire.
#[test]
fn raii_drop_releases_permit() {
    let sem = Arc::new(CpuConcurrencySemaphore::new(1));

    {
        let _p = sem.acquire();
        // Capacity exhausted inside this scope.
    } // permit drops here, releasing the slot

    // If release did NOT happen on drop, this acquire would block forever.
    let got = Arc::new(AtomicUsize::new(0));
    let sem2 = Arc::clone(&sem);
    let flag = Arc::clone(&got);
    let h = thread::spawn(move || {
        let _p = sem2.acquire();
        flag.store(1, Ordering::SeqCst);
    });

    let start = Instant::now();
    while got.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "acquire after scope-drop blocked — RAII release on normal drop is broken",
        );
        thread::sleep(Duration::from_millis(5));
    }
    h.join().expect("acquire thread joined");
}

/// Panic-release: a thread that panics WHILE holding a permit must still
/// release the slot during stack unwind (RAII `Drop` runs on panic). A
/// non-RAII counter (e.g. an explicit `release()` only on the happy path)
/// would leak the permit and deadlock the recovery acquire below.
#[test]
fn permit_released_on_panic_unwind() {
    let sem = Arc::new(CpuConcurrencySemaphore::new(1));

    let sem_panic = Arc::clone(&sem);
    let panicker = thread::spawn(move || {
        let _permit: CpuConcurrencyPermit = sem_panic.acquire();
        // Panic while holding the only permit. Unwinding must drop
        // `_permit` and release the slot back to the semaphore.
        panic!("worker panicked while holding a CPU permit");
    });
    // The spawned thread is expected to panic; joining yields Err.
    assert!(
        panicker.join().is_err(),
        "panicker thread should have panicked",
    );

    // Capacity must have recovered: this acquire must succeed promptly.
    let recovered = Arc::new(AtomicUsize::new(0));
    let sem2 = Arc::clone(&sem);
    let flag = Arc::clone(&recovered);
    let h = thread::spawn(move || {
        let _p = sem2.acquire();
        flag.store(1, Ordering::SeqCst);
    });

    let start = Instant::now();
    while recovered.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "capacity did not recover after a permit-holder panicked — \
             release is NOT RAII (it leaked the slot on unwind)",
        );
        thread::sleep(Duration::from_millis(5));
    }
    h.join().expect("recovery acquire thread joined");
}

/// Full-capacity concurrency: with capacity N, exactly N permits can be
/// held simultaneously. Acquire N permits on the main thread without
/// blocking, then confirm all N are live.
#[test]
fn n_permits_held_concurrently() {
    const N: usize = 4;
    let sem = CpuConcurrencySemaphore::new(N);
    let permits: Vec<CpuConcurrencyPermit> = (0..N).map(|_| sem.acquire()).collect();
    assert_eq!(
        permits.len(),
        N,
        "all {N} permits acquired without blocking"
    );
    drop(permits);
}
