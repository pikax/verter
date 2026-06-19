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
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_scheduler::cpu_concurrency::{CpuConcurrencyPermit, CpuConcurrencySemaphore};

/// Capacity cap: with capacity N, the (N+1)th `acquire()` BLOCKS until a
/// permit is released. This test is deterministic and discriminating — it
/// proves blocking via channel handshakes, NOT via sleep-timing
/// inference:
///
/// - The spawned acquirer sends on `intent_tx` IMMEDIATELY before calling
///   `acquire()`, then sends on `acquired_tx` ONLY AFTER `acquire()`
///   returns.
/// - The main thread waits for the intent signal (the spawned thread is
///   about to enter `acquire`), then asserts `acquired_rx.recv_timeout`
///   times out: a correctly-BLOCKING `acquire` cannot have returned yet,
///   so no `acquired` message can exist within the window. A broken
///   NON-blocking semaphore would have already sent on `acquired_tx`,
///   tripping this assertion.
/// - The main thread then releases one held permit and asserts the
///   spawned `acquire` completes (the `acquired` message now arrives).
///
/// Because the discriminator is "did the `acquired` message arrive BEFORE
/// any permit was freed", a non-blocking impl fails it regardless of
/// thread-scheduling timing — the window only ever shrinks the chance of
/// a false PASS for the real impl, never produces a false FAIL for a
/// broken one.
#[test]
fn capacity_cap_blocks_until_permit_released() {
    const N: usize = 2;
    /// Generous relative to scheduling jitter, short relative to the test
    /// (the real impl never sends `acquired` in this window because no
    /// permit is freed yet; only a broken non-blocking impl would).
    const BLOCK_OBSERVATION_WINDOW: Duration = Duration::from_millis(300);

    let sem = Arc::new(CpuConcurrencySemaphore::new(N));

    // Fill capacity on the main thread: N live permits, zero free.
    let held: Vec<CpuConcurrencyPermit<'_>> = (0..N).map(|_| sem.acquire()).collect();
    assert_eq!(held.len(), N, "main thread holds all {N} permits");

    let (intent_tx, intent_rx) = mpsc::channel::<()>();
    let (acquired_tx, acquired_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();

    let sem2 = Arc::clone(&sem);
    let extra = thread::spawn(move || {
        // Signal intent IMMEDIATELY before the (expected-blocking) acquire.
        intent_tx.send(()).expect("intent send");
        let _permit = sem2.acquire();
        // Reached ONLY after acquire() returns — i.e. after a permit is
        // free. A non-blocking impl reaches this immediately; the real
        // impl reaches it only after the main thread drops a permit.
        acquired_tx.send(()).expect("acquired send");
        // Keep the permit live until the test tells us to finish, so the
        // `acquired` send above is observed before this thread (and its
        // sender) tears down. `_permit` drops at end of scope.
        let _ = release_rx.recv();
    });

    // The spawned thread is about to call `acquire()`.
    intent_rx
        .recv()
        .expect("spawned thread signalled acquire intent");

    // DISCRIMINATOR: capacity is exhausted, so `acquire()` MUST still be
    // blocked — no `acquired` message can arrive within the window. A
    // broken non-blocking semaphore would already have sent.
    match acquired_rx.recv_timeout(BLOCK_OBSERVATION_WINDOW) {
        Err(RecvTimeoutError::Timeout) => { /* correct: still blocked */ }
        Ok(()) => panic!(
            "(N+1)th acquire returned while capacity ({N}) was fully held — \
             the semaphore did NOT block; a non-blocking/broken impl",
        ),
        Err(RecvTimeoutError::Disconnected) => {
            panic!("acquirer thread dropped its sender before acquiring — test wiring bug")
        }
    }

    // Free exactly one slot; the blocked acquire must now proceed.
    drop(held.into_iter().next().expect("at least one held permit"));

    acquired_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("(N+1)th acquire proceeded after a permit was released");

    // Let the acquirer release its permit and exit.
    release_tx.send(()).expect("release send");
    extra.join().expect("extra-acquirer thread joined");
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
