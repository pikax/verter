//! Cooperative-admission substrate tests — extracted sibling of
//! `singleflight.rs` (kept under the file-size guard cap).
//!
//! Included as a child `mod` of `singleflight` via
//! `#[path]`, so `use super::*` reaches the substrate's private
//! `InflightSlot` / `InflightTable` internals the thread-coordinated
//! discriminators poll.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// D3.2 test 1: 100 threads racing on the same key — exactly
/// ONE compute call observed; all return same value.
#[test]
fn cooperative_admission_one_winner_others_wait() {
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();
    let compute_count = Arc::new(AtomicUsize::new(0));

    let map = Arc::new(map);
    let inflight = Arc::new(inflight);

    let handles: Vec<_> = (0..100)
        .map(|_| {
            let map = Arc::clone(&map);
            let inflight = Arc::clone(&inflight);
            let compute_count = Arc::clone(&compute_count);
            thread::spawn(move || {
                cooperative_get_or_insert(
                    &map,
                    &inflight,
                    42u32,
                    |entry: &String| Some(entry.clone()),
                    || {
                        compute_count.fetch_add(1, Ordering::SeqCst);
                        // Hold long enough for other threads to enter
                        // the joiner branch.
                        thread::sleep(Duration::from_millis(20));
                        Some("winner".to_string())
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    |_k: &u32, _e: &Arc<String>| {},
                )
            })
        })
        .collect();

    let results: Vec<Option<String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        1,
        "exactly one thread must run compute under admission control"
    );
    for r in &results {
        assert_eq!(r.as_deref(), Some("winner"));
    }
}

/// D3.2 test 2: winner panics in compute → waiters wake with
/// None; subsequent calls retry.
///
/// Deterministic rendezvous: a fixed sleep between the winner's
/// claim and its panic does NOT prove the joiner has acquired its
/// own `Arc` on the in-flight slot — on a slow worker the panic
/// guard's `Drop` can retire the slot before the joiner reaches it,
/// the joiner then claims a fresh slot and runs its own `compute`,
/// and the test no longer exercises the wake-on-panic path it
/// claims to cover. The rendezvous instead has two stages:
///   * `claimed_tx`/`claimed_rx` `sync_channel(0)` — the winner's
///     `compute` signals AFTER `state.claimed = true`; main blocks
///     on `recv()` before spawning the joiner so the joiner cannot
///     race ahead of the winner's claim.
///   * `release_barrier` — the winner's `compute` blocks on a
///     `Barrier::new(2)` AFTER signalling claim; the test driver
///     polls the slot strong count and crosses the barrier ONLY
///     once the joiner has its own `Arc` on the slot (count `>= 4`:
///     table + winner.slot + winner.panic_guard.slot + joiner.slot).
///
/// The winner therefore panics only after the joiner is a proven
/// slot waiter, so the panic guard's `Drop` `notify_all` wakes the
/// joiner with `failed` on every run regardless of worker speed.
#[test]
fn cooperative_admission_panic_wakes_waiters() {
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::Instant;

    // Use a dedicated map per scenario to avoid cross-test races.
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();
    let map = Arc::new(map);
    let inflight = Arc::new(inflight);

    // Joiner that arrives second; will block on the panicking
    // winner's slot.
    let joiner_done = Arc::new(AtomicUsize::new(0));

    // Rendezvous channel — the winner's compute() signals AFTER
    // claim (i.e. once `state.claimed = true` in the inflight slot).
    // Main blocks on `recv()` before spawning the joiner so the
    // joiner cannot race ahead of the winner's claim.
    let (claimed_tx, claimed_rx) = mpsc::sync_channel::<()>(0);
    // Release barrier — the winner's compute blocks here after
    // signalling claim; the driver crosses it only after the joiner
    // has acquired its own slot Arc.
    let release_barrier = Arc::new(Barrier::new(2));

    // Winner thread that panics inside compute.
    let map_w = Arc::clone(&map);
    let inflight_w = Arc::clone(&inflight);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        // We use catch_unwind manually so the test process doesn't
        // abort on the panic; the production cooperative caller
        // doesn't care, but the test harness does.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cooperative_get_or_insert(
                &map_w,
                &inflight_w,
                7u32,
                |entry: &String| Some(entry.clone()),
                || -> Option<String> {
                    // `compute` runs only AFTER the winner has set
                    // `state.claimed = true` inside
                    // `cooperative_get_or_insert`, so signalling
                    // here is the contract-correct hook for "winner
                    // has claimed the inflight slot".
                    claimed_tx
                        .send(())
                        .expect("rendezvous receiver must outlive winner's compute");
                    // Block until the driver has confirmed the
                    // joiner holds its own `Arc` on the in-flight
                    // slot — only then is the joiner a proven slot
                    // waiter and the panic guaranteed to wake it.
                    release_barrier_w.wait();
                    panic!("simulated compute panic");
                },
                |entry: &String| entry.clone(),
                |_entry: &String| true,
                |_k: &u32, _e: &Arc<String>| {},
            )
        }));
    });

    // Block until the winner has claimed the inflight slot. Once
    // this returns, the joiner spawn is guaranteed to race against
    // a winner that is already in compute, not a winner that has
    // not yet claimed.
    claimed_rx
        .recv()
        .expect("winner's compute must signal claim before panicking");

    // Joiner — should wake with None when winner's RAII guard fires.
    let map_j = Arc::clone(&map);
    let inflight_j = Arc::clone(&inflight);
    let joiner_done_j = Arc::clone(&joiner_done);
    let joiner = thread::spawn(move || {
        let result = cooperative_get_or_insert(
            &map_j,
            &inflight_j,
            7u32,
            |entry: &String| Some(entry.clone()),
            || Some("never reached".to_string()),
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
        );
        joiner_done_j.fetch_add(1, Ordering::SeqCst);
        result
    });

    // Deterministic wait: poll the inflight table until the joiner
    // has acquired its own `Arc` on the slot. While the winner is
    // parked at the release barrier the strong count is 3 (table +
    // winner.slot + winner.panic_guard.slot); the joiner bumps it to
    // 4 once it clones its slot Arc, past which it deterministically
    // reaches the cooperative joiner wait branch.
    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if inflight
            .slot_strong_count(&7u32)
            .is_some_and(|count| count >= 4)
        {
            break;
        }
        assert!(
            Instant::now() < poll_deadline,
            "joiner failed to acquire inflight slot Arc within 10s — \
             the deterministic panic-wake rendezvous is broken"
        );
        std::hint::spin_loop();
    }
    // Joiner is a proven slot waiter — release the winner so it
    // panics; the panic guard's `Drop` wakes the joiner with `failed`.
    release_barrier.wait();

    winner.join().unwrap();
    let joiner_result = joiner.join().unwrap();
    assert_eq!(
        joiner_done.load(Ordering::SeqCst),
        1,
        "joiner must finish after winner panics"
    );
    assert_eq!(
        joiner_result, None,
        "joiner observing a panicked winner must return None"
    );

    // Subsequent call retries cold path successfully.
    let retry_result = cooperative_get_or_insert(
        &map,
        &inflight,
        7u32,
        |entry: &String| Some(entry.clone()),
        || Some("retry succeeded".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| true,
        |_k: &u32, _e: &Arc<String>| {},
    );
    assert_eq!(retry_result.as_deref(), Some("retry succeeded"));
}

/// D3.2 test 3: post-compute revalidation returns false →
/// publish skipped; waiters fall through; no entry in map.
#[test]
fn cooperative_admission_post_compute_revalidation_drops_stale() {
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();

    let result = cooperative_get_or_insert(
        &map,
        &inflight,
        13u32,
        |entry: &String| Some(entry.clone()),
        || Some("computed but stale".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| false, // post-compute revalidation FAILS
        |_k: &u32, _e: &Arc<String>| {},
    );

    assert_eq!(
        result, None,
        "post-compute revalidation rejection must yield None"
    );
    assert!(
        map.get(&13u32).is_none(),
        "rejected entries must NOT be inserted into the map"
    );
}

/// D3.2 test 4: simulated invalidation during compute — first
/// call returns None due to revalidation rejection; second call
/// runs fresh compute and succeeds when revalidation passes.
#[test]
fn cooperative_admission_invalidation_during_compute_retries() {
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();
    let attempt = AtomicUsize::new(0);

    // First attempt: compute succeeds but revalidation rejects.
    let first = cooperative_get_or_insert(
        &map,
        &inflight,
        21u32,
        |entry: &String| Some(entry.clone()),
        || {
            attempt.fetch_add(1, Ordering::SeqCst);
            Some("first".to_string())
        },
        |entry: &String| entry.clone(),
        |_entry: &String| false,
        |_k: &u32, _e: &Arc<String>| {},
    );
    assert_eq!(first, None, "first attempt must drop on revalidation");

    // Second attempt: post-mutation, revalidation passes.
    let second = cooperative_get_or_insert(
        &map,
        &inflight,
        21u32,
        |entry: &String| Some(entry.clone()),
        || {
            attempt.fetch_add(1, Ordering::SeqCst);
            Some("second".to_string())
        },
        |entry: &String| entry.clone(),
        |_entry: &String| true,
        |_k: &u32, _e: &Arc<String>| {},
    );
    assert_eq!(second.as_deref(), Some("second"));
    assert_eq!(
        attempt.load(Ordering::SeqCst),
        2,
        "both attempts must run compute (no spurious cache reuse)"
    );
}

/// D3.2 test 5: same Entry projects to different Value types
/// per call site. Demonstrates the projection-isolation contract.
#[test]
fn cooperative_admission_value_projection_isolated() {
    // Entry carries TWO fields; different call sites project
    // different scalars from the same entry.
    struct Entry {
        length: usize,
        label: String,
    }
    let map: DashMap<u32, Arc<Entry>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();

    // First call site: project the length.
    let length: Option<usize> = cooperative_get_or_insert(
        &map,
        &inflight,
        55u32,
        |entry: &Entry| Some(entry.length),
        || {
            Some(Entry {
                length: 7,
                label: "hello".to_string(),
            })
        },
        |entry: &Entry| entry.length,
        |_entry: &Entry| true,
        |_k: &u32, _e: &Arc<Entry>| {},
    );
    assert_eq!(length, Some(7));

    // Second call site (warm hit): project the label from the same
    // cached Entry.
    let label: Option<String> = cooperative_get_or_insert(
        &map,
        &inflight,
        55u32,
        |entry: &Entry| Some(entry.label.clone()),
        || -> Option<Entry> { panic!("must not run compute on warm hit") },
        |entry: &Entry| entry.label.clone(),
        |_entry: &Entry| true,
        |_k: &u32, _e: &Arc<Entry>| {},
    );
    assert_eq!(label.as_deref(), Some("hello"));
}

/// Joiners run `validate` (not `project`) on their own thread.
///
/// A cacheable winner does NOT broadcast a value to joiners through
/// the inflight slot; joiners fall through to `map.get(&key)` and
/// run the caller's `validate` closure on their OWN thread. In
/// production, `validate` both (a) view-checks the entry against the
/// joiner's own view and (b) runs the caller's fact-bubble side
/// effect — `entry.read_set_signature.bubble(ctx)` — delivering the
/// cached entry's facts into the joiner thread's active outer fact
/// tracer.
///
/// Discriminating signal: a per-thread `validate_count` atomic. The
/// JOINER thread's `validate` count must increment exactly once.
/// Pre-fix the joiner ran `project`, not `validate`, on the joiner
/// path (and a `ReturnOnly`-style broadcast skipped even that), so
/// the joiner's `validate` count was zero.
#[test]
fn cacheable_joiner_runs_validate_on_its_own_thread() {
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};
    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<u32>> = Arc::new(InflightTable::default());

    // Per-thread validate counters. The winner is a cold miss: its
    // warm-hit probe finds an empty map so `validate` is never
    // called, and the winner does not re-validate its own published
    // entry (it runs `project`). The winner's validate count is
    // therefore 0. The joiner runs `validate` exactly once against
    // the winner's published entry on the joiner branch.
    let winner_validate_count = Arc::new(AtomicUsize::new(0));
    let joiner_validate_count = Arc::new(AtomicUsize::new(0));

    // Deterministic synchronisation:
    //
    //   * `tx_winner_in_compute` — winner signals it has claimed
    //     the inflight slot and is inside `compute()`. Joiner is
    //     spawned only after this fires, so the joiner cannot
    //     race ahead of the winner's claim.
    //   * `release_barrier` — a `Barrier::new(2)` between winner's
    //     `compute()` body and the test driver. The winner blocks
    //     at the barrier AFTER signalling claim, so it CANNOT
    //     publish (return Cacheable) until the test driver also
    //     crosses the barrier.
    //
    // Between `rx_winner_in_compute.recv()` and the barrier release
    // the test polls the inflight table for the moment the joiner
    // has acquired its own `Arc<InflightSlot>` (table refcount +
    // winner refcount + winner.panic_guard refcount + joiner
    // refcount = 4). Once the joiner holds an Arc to the existing
    // slot, the winner may retire the table entry without changing
    // the joiner's view of `state.claimed`/`completed`, and the
    // joiner deterministically reaches the `map.get(&key) +
    // validate(&entry_arc)` branch that the discriminator measures.
    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let release_barrier = Arc::new(Barrier::new(2));

    let winner_map = Arc::clone(&map);
    let winner_inflight = Arc::clone(&inflight);
    let winner_vc = Arc::clone(&winner_validate_count);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*winner_map,
            &*winner_inflight,
            42u32,
            |_entry: &String| -> Option<String> {
                winner_vc.fetch_add(1, Ordering::SeqCst);
                None // no warm hit
            },
            || -> ComputeAdmission<String, String> {
                // Signal we are inside compute (claimed). The
                // joiner can now enter and acquire the inflight
                // slot Arc.
                tx_winner_in_compute.send(()).expect("signal in-compute");
                // Block until the test driver crosses the
                // release barrier — i.e. until the test driver
                // has confirmed the joiner has acquired its own
                // Arc on the inflight slot.
                release_barrier_w.wait();
                ComputeAdmission::Cacheable("payload".to_string())
            },
            |entry: &String| -> String { entry.clone() },
            |_entry: &String| -> bool { true },
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    // Wait for the winner to enter compute (claimed but not yet
    // published).
    rx_winner_in_compute
        .recv()
        .expect("winner must signal claim before joiner spawn");

    let joiner_map = Arc::clone(&map);
    let joiner_inflight = Arc::clone(&inflight);
    let joiner_vc = Arc::clone(&joiner_validate_count);
    let joiner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*joiner_map,
            &*joiner_inflight,
            42u32,
            |entry: &String| -> Option<String> {
                // The joiner runs `validate` against the winner's
                // published entry on its OWN thread. Accept the
                // entry (same-view coalesce) so the joiner returns
                // the winner's value.
                joiner_vc.fetch_add(1, Ordering::SeqCst);
                Some(entry.clone())
            },
            || -> ComputeAdmission<String, String> {
                // The joiner MUST NOT execute compute; if this
                // panics the test isn't exercising the joiner
                // branch (which means the deterministic sync
                // below is broken, not the production code).
                panic!("joiner must not run compute");
            },
            |entry: &String| -> String { entry.clone() },
            |_entry: &String| -> bool { true },
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    // Deterministic wait: poll the inflight table until the
    // joiner has acquired its own `Arc<InflightSlot>` for the
    // key. Strong-count layout for the still-claimed slot:
    //
    //   * 1 — table entry holds the slot Arc
    //   * 1 — winner's `slot` local inside
    //         `cooperative_admit_with_post_publish`
    //   * 1 — winner's `panic_guard.slot`, created by
    //         `InflightPanicGuard::new(Arc::clone(&slot), ...)`
    //         AFTER `state.claimed = true` (i.e. before the
    //         winner enters its `compute()` body)
    //   * 1 — joiner's `slot` local AFTER it executes the
    //         `table.entry(key).or_insert_with(...).clone()`
    //         block
    //
    // The winner is parked inside its `compute()` body at the
    // release barrier, so winner.slot and winner.panic_guard.slot
    // both stay alive — the baseline strong count is 3 before
    // the joiner arrives and exactly 4 once the joiner has
    // acquired its Arc on the existing slot. We poll for `>= 4`
    // so the release barrier crosses only after the joiner has
    // bumped the refcount.
    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let table_guard = inflight.table.lock();
        if let Some(slot) = table_guard.get(&42u32) {
            if Arc::strong_count(slot) >= 4 {
                break;
            }
        }
        drop(table_guard);
        if Instant::now() >= poll_deadline {
            panic!(
                "joiner failed to acquire inflight slot Arc within 10s — \
                 the deterministic-sync poll below the release barrier is broken"
            );
        }
        std::hint::spin_loop();
    }

    // Release the winner via the barrier. The winner returns
    // Cacheable, publishes, inserts into the map, sets
    // `state.completed = true`, notifies the joiner, and retires
    // the inflight table entry. The joiner is past the
    // slot-acquisition point so it observes the existing slot via
    // its own Arc and falls through to `map.get(&key) +
    // validate(&entry_arc)` — bumping the joiner validate count by
    // exactly one.
    release_barrier.wait();

    let winner_result = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");

    assert_eq!(winner_result.as_deref(), Some("payload"));
    assert_eq!(joiner_result.as_deref(), Some("payload"));
    assert_eq!(
        winner_validate_count.load(Ordering::SeqCst),
        0,
        "winner thread is a cold miss: its warm-hit probe finds an \
         empty map so `validate` is never called, and the winner does \
         not re-validate its own published entry (it runs `project`)"
    );
    // Pre-fix this is 0 (the joiner ran `project`, not `validate`,
    // on the joiner path). Post-fix this is 1 (the joiner runs
    // `validate(&entry_arc)` on its own thread, which is where
    // production caches both view-check the entry and run their
    // fact-bubble side effect — `entry.read_set_signature.bubble(ctx)`).
    assert_eq!(
        joiner_validate_count.load(Ordering::SeqCst),
        1,
        "joiner thread's validate count must be exactly 1. If 0, the \
         joiner is not running the caller's `validate` closure on the \
         joiner path — it is skipping read-side view validation and \
         the fact-bubble side effect. See \
         `crates/verter_session/src/cache_runtime/singleflight.rs` joiner branch."
    );
}

/// A cooperative joiner whose `validate` closure REJECTS the
/// winner's published entry (simulating a follower running under a
/// different view/overlay) must NOT inherit the winner's value — it
/// forks and cold-computes its OWN value.
///
/// Discrimination: the joiner's `validate` closure returns `None`
/// for the winner's value (`"winner-view"`) but `Some` for the
/// joiner's own freshly-computed value (`"joiner-view"`). Pre-fix
/// the joiner ran `project` and returned the winner's value
/// verbatim with no `validate` call, so the joiner's compute closure
/// never ran and the joiner observed `"winner-view"`. Post-fix the
/// joiner runs `validate`, gets `None`, forks, cold-computes, and
/// observes `"joiner-view"`.
#[test]
fn cooperative_get_or_insert_joiner_validate_reject_forks() {
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<u32>> = Arc::new(InflightTable::default());

    let winner_compute_count = Arc::new(AtomicUsize::new(0));
    let joiner_compute_count = Arc::new(AtomicUsize::new(0));

    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let release_barrier = Arc::new(Barrier::new(2));

    let winner_map = Arc::clone(&map);
    let winner_inflight = Arc::clone(&inflight);
    let winner_cc = Arc::clone(&winner_compute_count);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        cooperative_get_or_insert(
            &*winner_map,
            &*winner_inflight,
            99u32,
            // The winner's view accepts the winner value.
            |entry: &String| {
                if entry.as_str() == "winner-view" {
                    Some(entry.clone())
                } else {
                    None
                }
            },
            || -> Option<String> {
                winner_cc.fetch_add(1, Ordering::SeqCst);
                tx_winner_in_compute.send(()).expect("signal in-compute");
                release_barrier_w.wait();
                Some("winner-view".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
        )
    });

    rx_winner_in_compute
        .recv()
        .expect("winner must signal claim before joiner spawn");

    let joiner_map = Arc::clone(&map);
    let joiner_inflight = Arc::clone(&inflight);
    let joiner_cc = Arc::clone(&joiner_compute_count);
    let joiner = thread::spawn(move || {
        cooperative_get_or_insert(
            &*joiner_map,
            &*joiner_inflight,
            99u32,
            // The joiner's view REJECTS the winner value and only
            // accepts its own. This simulates a follower running
            // under a different overlay: the winner's entry is not
            // valid for the follower's view.
            |entry: &String| {
                if entry.as_str() == "joiner-view" {
                    Some(entry.clone())
                } else {
                    None
                }
            },
            || -> Option<String> {
                joiner_cc.fetch_add(1, Ordering::SeqCst);
                Some("joiner-view".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
        )
    });

    // Poll until the joiner has acquired its own Arc on the slot
    // (table + winner.slot + winner.panic_guard.slot + joiner.slot).
    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let table_guard = inflight.table.lock();
        if let Some(slot) = table_guard.get(&99u32) {
            if Arc::strong_count(slot) >= 4 {
                break;
            }
        }
        drop(table_guard);
        if Instant::now() >= poll_deadline {
            panic!("joiner failed to acquire inflight slot Arc within 10s");
        }
        std::hint::spin_loop();
    }

    release_barrier.wait();

    let winner_result = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");

    assert_eq!(
        winner_result.as_deref(),
        Some("winner-view"),
        "winner returns its own cold-computed value"
    );
    // Discriminator: pre-fix the joiner ran `project` and returned
    // the winner's `"winner-view"` value with no `validate` call.
    // Post-fix the joiner runs `validate` (rejects `"winner-view"`),
    // forks, cold-computes, and returns `"joiner-view"`.
    assert_eq!(
        joiner_result.as_deref(),
        Some("joiner-view"),
        "a joiner whose `validate` rejects the winner's entry must \
         fork and cold-compute its OWN value, not inherit the winner's"
    );
    assert_eq!(
        winner_compute_count.load(Ordering::SeqCst),
        1,
        "winner runs compute exactly once"
    );
    assert_eq!(
        joiner_compute_count.load(Ordering::SeqCst),
        1,
        "joiner whose validate rejected the winner must run its OWN \
         cold compute exactly once (the fork)"
    );
}

/// Same discriminator as
/// `cooperative_get_or_insert_joiner_validate_reject_forks` but for
/// the `cooperative_admit_with_post_publish` function: a joiner
/// whose `validate` rejects a `Cacheable` winner's published entry
/// forks and cold-computes its own value.
#[test]
fn cooperative_admit_joiner_validate_reject_forks() {
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<u32>> = Arc::new(InflightTable::default());

    let winner_compute_count = Arc::new(AtomicUsize::new(0));
    let joiner_compute_count = Arc::new(AtomicUsize::new(0));

    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let release_barrier = Arc::new(Barrier::new(2));

    let winner_map = Arc::clone(&map);
    let winner_inflight = Arc::clone(&inflight);
    let winner_cc = Arc::clone(&winner_compute_count);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*winner_map,
            &*winner_inflight,
            7u32,
            |entry: &String| {
                if entry.as_str() == "winner-view" {
                    Some(entry.clone())
                } else {
                    None
                }
            },
            || -> ComputeAdmission<String, String> {
                winner_cc.fetch_add(1, Ordering::SeqCst);
                tx_winner_in_compute.send(()).expect("signal in-compute");
                release_barrier_w.wait();
                ComputeAdmission::Cacheable("winner-view".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    rx_winner_in_compute
        .recv()
        .expect("winner must signal claim before joiner spawn");

    let joiner_map = Arc::clone(&map);
    let joiner_inflight = Arc::clone(&inflight);
    let joiner_cc = Arc::clone(&joiner_compute_count);
    let joiner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*joiner_map,
            &*joiner_inflight,
            7u32,
            |entry: &String| {
                if entry.as_str() == "joiner-view" {
                    Some(entry.clone())
                } else {
                    None
                }
            },
            || -> ComputeAdmission<String, String> {
                joiner_cc.fetch_add(1, Ordering::SeqCst);
                ComputeAdmission::Cacheable("joiner-view".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let table_guard = inflight.table.lock();
        if let Some(slot) = table_guard.get(&7u32) {
            if Arc::strong_count(slot) >= 4 {
                break;
            }
        }
        drop(table_guard);
        if Instant::now() >= poll_deadline {
            panic!("joiner failed to acquire inflight slot Arc within 10s");
        }
        std::hint::spin_loop();
    }

    release_barrier.wait();

    let winner_result = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");

    assert_eq!(winner_result.as_deref(), Some("winner-view"));
    assert_eq!(
        joiner_result.as_deref(),
        Some("joiner-view"),
        "a `cooperative_admit_with_post_publish` joiner whose `validate` \
         rejects the Cacheable winner's entry must fork and cold-compute"
    );
    assert_eq!(winner_compute_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        joiner_compute_count.load(Ordering::SeqCst),
        1,
        "the forking joiner must run its own cold compute exactly once"
    );
}

/// A `ComputeAdmission::ReturnOnly` winner must NOT be broadcast to
/// cooperative joiners. A `ReturnOnly` value carries no `Entry` and
/// no dep-signature carrier, so it cannot be view-validated against
/// a joiner's own view — every joiner forks and cold-recomputes.
///
/// Discrimination: the winner's compute returns
/// `ReturnOnly("winner-returnonly")`; the joiner's compute returns
/// `Cacheable("joiner-own")`. Pre-fix the winner stored the value in
/// the slot's `return_only` channel and the joiner downcast it and
/// returned `"winner-returnonly"` WITHOUT running its own compute.
/// Post-fix the joiner observes `non_cacheable_winner`, forks,
/// runs its own compute, and returns `"joiner-own"`.
#[test]
fn return_only_winner_not_broadcast_cross_view_joiner_forks() {
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<u32>> = Arc::new(InflightTable::default());

    let winner_compute_count = Arc::new(AtomicUsize::new(0));
    let joiner_compute_count = Arc::new(AtomicUsize::new(0));

    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let release_barrier = Arc::new(Barrier::new(2));

    let winner_map = Arc::clone(&map);
    let winner_inflight = Arc::clone(&inflight);
    let winner_cc = Arc::clone(&winner_compute_count);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*winner_map,
            &*winner_inflight,
            3u32,
            |_entry: &String| -> Option<String> { None },
            || -> ComputeAdmission<String, String> {
                winner_cc.fetch_add(1, Ordering::SeqCst);
                tx_winner_in_compute.send(()).expect("signal in-compute");
                release_barrier_w.wait();
                // Valid but non-cacheable outcome.
                ComputeAdmission::ReturnOnly("winner-returnonly".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    rx_winner_in_compute
        .recv()
        .expect("winner must signal claim before joiner spawn");

    let joiner_map = Arc::clone(&map);
    let joiner_inflight = Arc::clone(&inflight);
    let joiner_cc = Arc::clone(&joiner_compute_count);
    let joiner = thread::spawn(move || {
        cooperative_admit_with_post_publish(
            &*joiner_map,
            &*joiner_inflight,
            3u32,
            |entry: &String| {
                if entry.as_str() == "joiner-own" {
                    Some(entry.clone())
                } else {
                    None
                }
            },
            || -> ComputeAdmission<String, String> {
                joiner_cc.fetch_add(1, Ordering::SeqCst);
                ComputeAdmission::Cacheable("joiner-own".to_string())
            },
            |entry: &String| entry.clone(),
            |_entry: &String| true,
            |_k: &u32, _e: &Arc<String>| {},
            |_entry_arc: &Arc<String>, _k: &u32| {},
            // No retention budget on this test cache — no publish fence.
            None,
        )
    });

    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let table_guard = inflight.table.lock();
        if let Some(slot) = table_guard.get(&3u32) {
            if Arc::strong_count(slot) >= 4 {
                break;
            }
        }
        drop(table_guard);
        if Instant::now() >= poll_deadline {
            panic!("joiner failed to acquire inflight slot Arc within 10s");
        }
        std::hint::spin_loop();
    }

    release_barrier.wait();

    let winner_result = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");

    // The winner still receives its valid-but-non-cacheable value.
    assert_eq!(
        winner_result.as_deref(),
        Some("winner-returnonly"),
        "the ReturnOnly winner receives its own valid value"
    );
    // Discriminator: pre-fix the joiner downcast the broadcast
    // `return_only` value and observed `"winner-returnonly"` without
    // running compute. Post-fix the joiner observes
    // `non_cacheable_winner`, forks, and runs its own compute.
    assert_eq!(
        joiner_result.as_deref(),
        Some("joiner-own"),
        "a ReturnOnly winner must NOT be broadcast to a cross-view \
         joiner — the joiner forks and cold-computes its own value"
    );
    assert_eq!(winner_compute_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        joiner_compute_count.load(Ordering::SeqCst),
        1,
        "the joiner of a ReturnOnly winner must run its OWN cold \
         compute (ReturnOnly is non-shareable across joiners)"
    );
}

/// `ComputeAdmission::Failed` is constructible. The three-variant
/// contract requires a `Failed` case alongside `Cacheable` and
/// `ReturnOnly`; this test exercises construction so the variant
/// cannot be dead-removed by future refactors.
#[test]
fn compute_admission_failed_variant_is_constructible() {
    let admission: ComputeAdmission<(), ()> = ComputeAdmission::Failed;
    assert!(
        matches!(admission, ComputeAdmission::Failed),
        "ComputeAdmission::Failed must be constructible"
    );
}

// ===========================================================================
// New-adapter discriminators: by_flight_key (MapK != FlightK) +
// with_lookup_publish (slot-delegated storage).
// ===========================================================================

/// `cooperative_admit_with_post_publish_by_flight_key` publishes into
/// the map under `MapK` while coalescing the flight on `FlightK`.
///
/// Two threads race on the SAME map key but carry DIFFERENT flight keys
/// (the model of two overlays on one cache key, both cold). They must
/// NOT coalesce: each runs its own cold compute. A `Barrier` holds both
/// inside `compute` until both have passed the warm-miss check and
/// claimed their (distinct) flight slots, so neither observes the
/// other's publish on the warm path.
///
/// Discriminating: against the unified-key primitive (where flight key =
/// map key) the two threads WOULD claim the SAME flight slot and only
/// ONE compute would run; the other would be a cooperative joiner.
/// Distinct flight keys force two concurrent cold computes. The
/// published map stays keyed by the shared `MapK` (the compat-token
/// never enters the map), so exactly one map entry survives.
#[test]
fn by_flight_key_keys_map_on_mapk_and_coalesces_on_flightk() {
    use std::sync::Barrier;

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<(u32, u8)>> = Arc::new(InflightTable::default());
    let compute_count = Arc::new(AtomicUsize::new(0));
    // Both cold computes rendezvous here so each passes its warm-miss
    // check and claims its own flight slot before either publishes.
    let both_in_compute = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [0u8, 1u8]
        .into_iter()
        .map(|flight_token| {
            let map = Arc::clone(&map);
            let inflight = Arc::clone(&inflight);
            let count = Arc::clone(&compute_count);
            let barrier = Arc::clone(&both_in_compute);
            thread::spawn(move || {
                cooperative_admit_with_post_publish_by_flight_key(
                    &map,
                    &inflight,
                    42u32,
                    (42u32, flight_token),
                    |entry: &String| Some(entry.clone()),
                    move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        // Hold until BOTH threads are inside compute so
                        // neither sees the other's publish on the warm
                        // path — proving distinct flight lanes do not
                        // coalesce.
                        barrier.wait();
                        ComputeAdmission::Cacheable(format!("flight-{flight_token}"))
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    |_k: &u32, _e: &Arc<String>| {},
                    |_e: &Arc<String>, _k: &u32| {},
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Distinct flight lanes ⇒ both cold computes ran (no coalescing).
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        2,
        "distinct flight keys must NOT coalesce — each runs its own cold compute"
    );
    for r in &results {
        assert!(r.is_some(), "each flight returns its own computed value");
    }
    // The published map is keyed by the shared MapK (42), so there is
    // exactly ONE map entry (the second publish overwrote the first
    // under the same map key). The compat-token never enters the map.
    assert_eq!(
        map.len(),
        1,
        "the published map must be keyed by MapK only — the flight token is NOT a map key"
    );

    // A subsequent caller under a THIRD flight token now hits the warm
    // map under MapK 42 and does NOT recompute — confirming the map key
    // is the shared MapK, independent of flight token.
    let warm = cooperative_admit_with_post_publish_by_flight_key(
        &map,
        &inflight,
        42u32,
        (42u32, 9u8),
        |entry: &String| Some(entry.clone()),
        || {
            compute_count.fetch_add(1, Ordering::SeqCst);
            ComputeAdmission::Cacheable("should-not-run".to_string())
        },
        |entry: &String| entry.clone(),
        |_entry: &String| true,
        |_k: &u32, _e: &Arc<String>| {},
        |_e: &Arc<String>, _k: &u32| {},
        None,
    );
    assert!(
        warm.is_some(),
        "warm map hit under MapK must return a value"
    );
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        2,
        "a warm map hit under MapK must NOT recompute regardless of flight token"
    );
}

/// When two cold winners on DIFFERENT flight lanes publish under the SAME
/// map key, the second `map.insert` replaces the first entry — and the
/// replaced entry must receive `removal_cleanup` so a cache that bumps a
/// live counter in `post_publish` and decrements it in `removal_cleanup`
/// stays balanced.
///
/// The `by_flight_key` adapter separates the in-flight coalescing identity
/// (`FlightK`) from the published map key (`MapK`). Two requests on
/// distinct flight lanes (e.g. two overlays) for the same cache key both
/// cold-compute and both publish under the shared `MapK`; the second
/// publish overwrites the first. Without cleaning up the overwritten
/// entry, every overwrite leaks one live-counter increment — the artifact
/// node's `post_publish` ran twice while `removal_cleanup` never ran for
/// the displaced entry.
///
/// A `Barrier` holds both winners inside `compute` until each has passed
/// its warm-miss check and claimed its own flight slot, so neither
/// coalesces onto the other and both reach the publish path.
///
/// Discriminating: with the overwrite path leaking, `post_publish` fires
/// twice and `removal_cleanup` zero times, so the net live count is 2.
/// With the displaced entry cleaned up under the publish fence, the net
/// live count is 1 (two publishes, one removal of the overwritten entry).
#[test]
fn by_flight_key_overwrite_cleans_up_displaced_entry() {
    use std::sync::Barrier;

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<(u32, u8)>> = Arc::new(InflightTable::default());
    // Models a cache's live-entry counter: bumped in post_publish, drained
    // in removal_cleanup. A leaked overwrite leaves this above the number
    // of entries actually held in the map.
    let post_publish_count = Arc::new(AtomicUsize::new(0));
    let removal_cleanup_count = Arc::new(AtomicUsize::new(0));
    // Both cold computes rendezvous so each claims its own flight slot
    // before either publishes — forcing two concurrent winners that both
    // publish under the same MapK.
    let both_in_compute = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [0u8, 1u8]
        .into_iter()
        .map(|flight_token| {
            let map = Arc::clone(&map);
            let inflight = Arc::clone(&inflight);
            let pub_count = Arc::clone(&post_publish_count);
            let rm_count = Arc::clone(&removal_cleanup_count);
            let barrier = Arc::clone(&both_in_compute);
            thread::spawn(move || {
                cooperative_admit_with_post_publish_by_flight_key(
                    &map,
                    &inflight,
                    42u32,
                    (42u32, flight_token),
                    |entry: &String| Some(entry.clone()),
                    move || {
                        barrier.wait();
                        ComputeAdmission::Cacheable(format!("flight-{flight_token}"))
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    move |_k: &u32, _e: &Arc<String>| {
                        rm_count.fetch_add(1, Ordering::SeqCst);
                    },
                    move |_e: &Arc<String>, _k: &u32| {
                        pub_count.fetch_add(1, Ordering::SeqCst);
                    },
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    for r in &results {
        assert!(r.is_some(), "each cold winner returns its own value");
    }
    // Both winners published under the shared MapK.
    assert_eq!(
        post_publish_count.load(Ordering::SeqCst),
        2,
        "two cold winners on distinct flight lanes each publish under the shared MapK"
    );
    // The second publish overwrote the first; the displaced entry must
    // have received removal_cleanup exactly once.
    assert_eq!(
        removal_cleanup_count.load(Ordering::SeqCst),
        1,
        "the entry displaced by the second publish must receive removal_cleanup"
    );
    // Net live accounting (post_publish bumps − removal_cleanup drains) ==
    // 1, matching the single surviving map entry.
    let net_live =
        post_publish_count.load(Ordering::SeqCst) - removal_cleanup_count.load(Ordering::SeqCst);
    assert_eq!(
        net_live, 1,
        "net live-entry accounting must equal the one surviving map entry, \
         not leak the overwritten entry's increment"
    );
    assert_eq!(
        map.len(),
        1,
        "exactly one map entry survives under the shared MapK"
    );
}

/// The displacing publisher's `removal_cleanup` of an overwritten entry
/// must run STRICTLY AFTER that overwritten entry's own `post_publish`
/// completes. This is the linearization the aggregate-count test above
/// CANNOT see: two publishes netting to one cleanup proves the count is
/// balanced eventually, not that the cleanup observed the displaced
/// entry's `post_publish` as already done.
///
/// Two cold winners on distinct flight lanes (A on token 0, B on token 1)
/// publish under the same `MapK`. A inserts first; B's compute blocks
/// until A has entered its `post_publish`, so B is always the displacer
/// and A is always the displaced entry.
///
/// The live counter is modelled as a SIGNED `AtomicI64`: `post_publish`
/// bumps it, `removal_cleanup` drains it. A correct linearization keeps it
/// `>= 0` at every observed point — the displaced entry's bump always
/// precedes its drain. Without per-`MapK` linearization the displacing
/// publisher (B) can run `removal_cleanup` (`fetch_sub`) for A while A's
/// `post_publish` (`fetch_add`) has not yet run, driving the counter to
/// `-1` (a `usize` counter would wrap to a catastrophic huge value). A
/// `fetch_min`-tracked low-water mark and a per-decrement pre-condition
/// check both witness the underflow.
///
/// An ORDER LOG (a `Mutex<Vec<Op>>`) records `post_publish` / `cleanup`
/// events. Post-fix, A's whole publish — insert + `post_publish` — runs
/// inside A's shard write guard, and B's insert blocks on that same shard
/// guard until A releases it, so the log always reads
/// `[APostPublish, .., BCleanupOfA, ..]`. Pre-fix, A's `map.insert` holds
/// no guard across `post_publish`, so B's insert + `removal_cleanup` can
/// land in the gap and the log shows `BCleanupOfA` before `APostPublish`.
///
/// Discriminating: post-fix the low-water mark is `0`, the per-decrement
/// pre-condition holds, and the order log shows A's `post_publish` before
/// B's cleanup-of-A — all THREE deterministically. Pre-fix at least one
/// fails (low-water `-1`, the pre-condition panic, or the inverted order),
/// reliably exposed by the documented window-widener below.
#[test]
fn by_flight_key_displaced_cleanup_runs_after_its_own_post_publish() {
    use std::sync::atomic::AtomicI64;
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::sync::Mutex;

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    enum Op {
        APostPublish,
        BCleanupOfA,
        BPostPublish,
    }

    let map: Arc<DashMap<u32, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<(u32, u8)>> = Arc::new(InflightTable::default());
    // Signed so an underflow is observable as a negative value rather than
    // a usize wrap. Models a cache's live-entry counter.
    let live = Arc::new(AtomicI64::new(0));
    // Low-water mark: a correct linearization never lets `live` dip below
    // zero, so this stays `0`. A pre-fix underflow records `-1`.
    let low_water = Arc::new(AtomicI64::new(0));
    let order_log = Arc::new(Mutex::new(Vec::<Op>::new()));
    // Both cold computes rendezvous so each claims its own flight slot and
    // both reach the publish path under the shared MapK.
    let both_winners = Arc::new(Barrier::new(2));
    // A signals from inside its `post_publish` that it has begun
    // publishing; B's compute blocks on this so B inserts AFTER A and is
    // always the displacer. The receiver is consumed only by B (token 1).
    let (tx_a_publishing, rx_a_publishing) = mpsc::channel::<()>();
    let tx_a_publishing = Arc::new(Mutex::new(Some(tx_a_publishing)));
    let rx_a_publishing = Arc::new(Mutex::new(Some(rx_a_publishing)));

    let handles: Vec<_> = [0u8, 1u8]
        .into_iter()
        .map(|flight_token| {
            let map = Arc::clone(&map);
            let inflight = Arc::clone(&inflight);
            let live = Arc::clone(&live);
            let low_water = Arc::clone(&low_water);
            let order_log = Arc::clone(&order_log);
            let both_winners = Arc::clone(&both_winners);
            let tx_a_publishing = Arc::clone(&tx_a_publishing);
            // Only B (token 1) pulls the receiver; A (token 0) leaves it.
            let rx_a_publishing = if flight_token == 1 {
                rx_a_publishing.lock().unwrap().take()
            } else {
                None
            };
            thread::spawn(move || {
                cooperative_admit_with_post_publish_by_flight_key(
                    &map,
                    &inflight,
                    42u32,
                    (42u32, flight_token),
                    |entry: &String| Some(entry.clone()),
                    move || {
                        // Both threads become winners on distinct flight
                        // lanes, then B (token 1) waits until A (token 0)
                        // has entered its publish, so A inserts first.
                        both_winners.wait();
                        if let Some(rx) = &rx_a_publishing {
                            rx.recv()
                                .expect("A must signal it is publishing before B publishes");
                        }
                        ComputeAdmission::Cacheable(format!("flight-{flight_token}"))
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    {
                        let live = Arc::clone(&live);
                        let low_water = Arc::clone(&low_water);
                        let order_log = Arc::clone(&order_log);
                        move |_k: &u32, _e: &Arc<String>| {
                            // Displacing publisher draining the overwritten
                            // entry. Pre-condition: the overwritten entry's
                            // `post_publish` bump MUST already be visible —
                            // i.e. `live >= 1` before this decrement. Under
                            // a correct per-MapK linearization this always
                            // holds; pre-fix it can be `0`.
                            let before = live.load(Ordering::SeqCst);
                            assert!(
                                before >= 1,
                                "removal_cleanup of a displaced entry ran before that \
                                 entry's own post_publish bump was visible (live={before}) \
                                 — the per-MapK publish lifecycle is not linearized"
                            );
                            order_log.lock().unwrap().push(Op::BCleanupOfA);
                            let after = live.fetch_sub(1, Ordering::SeqCst) - 1;
                            low_water.fetch_min(after, Ordering::SeqCst);
                        }
                    },
                    {
                        let live = Arc::clone(&live);
                        let order_log = Arc::clone(&order_log);
                        let tx_a_publishing = Arc::clone(&tx_a_publishing);
                        move |_e: &Arc<String>, _k: &u32| {
                            if flight_token == 0 {
                                // A: announce that publishing has begun so
                                // B may now insert and displace A. Pre-fix
                                // this runs with NO shard guard held, so B
                                // can race in and clean A up before the
                                // bump below. The sleep widens that pre-fix
                                // window to make the race deterministic; it
                                // is NOT a correctness synchroniser — post
                                // fix A holds the shard write guard across
                                // this whole closure, so B's insert blocks
                                // until A releases regardless of the sleep.
                                if let Some(tx) = tx_a_publishing.lock().unwrap().take() {
                                    tx.send(()).expect("A signals publishing");
                                }
                                thread::sleep(Duration::from_millis(50));
                                order_log.lock().unwrap().push(Op::APostPublish);
                            } else {
                                order_log.lock().unwrap().push(Op::BPostPublish);
                            }
                            live.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    for r in &results {
        assert!(r.is_some(), "each cold winner returns its own value");
    }
    // The live counter never dipped below zero — the displaced entry's
    // bump always preceded its drain.
    assert_eq!(
        low_water.load(Ordering::SeqCst),
        0,
        "live counter underflowed: a displaced entry was cleaned up before its \
         own post_publish ran"
    );
    // Net live accounting settles at exactly the one surviving map entry.
    assert_eq!(
        live.load(Ordering::SeqCst),
        1,
        "net live-entry accounting must equal the one surviving map entry"
    );
    assert_eq!(map.len(), 1, "exactly one map entry survives");
    // The order log proves A's post_publish completed before B cleaned A
    // up — not merely that the counts balanced.
    let log = order_log.lock().unwrap().clone();
    let a_pp = log
        .iter()
        .position(|op| *op == Op::APostPublish)
        .expect("A must run post_publish");
    let b_cleanup = log
        .iter()
        .position(|op| *op == Op::BCleanupOfA)
        .expect("B must clean up the displaced entry A");
    assert!(
        a_pp < b_cleanup,
        "A's post_publish must be ordered before B's cleanup of A, got {log:?}"
    );
}

/// A cold cacheable node computes exactly ONCE for two concurrent
/// joiners on the same flight key (H14 singleflight). Exercised through
/// the by_flight_key adapter — the artifact `lookup` path lowers here.
#[test]
fn cold_cacheable_node_computes_once_for_two_joiners() {
    use std::sync::Barrier;

    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<(u32, u8)> = InflightTable::default();
    let map = Arc::new(map);
    let inflight = Arc::new(inflight);
    let compute_count = Arc::new(AtomicUsize::new(0));
    // Hold the winner in compute until the joiner is a proven slot
    // waiter, so the joiner cannot race ahead and start its own compute.
    let release = Arc::new(Barrier::new(1));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let map = Arc::clone(&map);
            let inflight = Arc::clone(&inflight);
            let compute_count = Arc::clone(&compute_count);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                cooperative_admit_with_post_publish_by_flight_key(
                    &map,
                    &inflight,
                    7u32,
                    (7u32, 0u8),
                    |entry: &String| Some(entry.clone()),
                    move || {
                        compute_count.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(20));
                        let _ = release;
                        ComputeAdmission::Cacheable("one-winner".to_string())
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    |_k: &u32, _e: &Arc<String>| {},
                    |_e: &Arc<String>, _k: &u32| {},
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        1,
        "exactly one cold cacheable compute must run for two joiners on the same flight key"
    );
    for r in &results {
        assert_eq!(r.as_deref(), Some("one-winner"));
    }
}

/// A `ReturnOnly` winner over the by_flight_key adapter publishes
/// NOTHING: the map stays empty and `post_publish` (the reverse-index /
/// persistence hook) is never invoked. The winner receives its value
/// directly; the next caller cold-recomputes.
///
/// Discriminating: a `Cacheable` outcome inserts into the map and fires
/// `post_publish`. The `ReturnOnly` arm must do neither — it carries no
/// validatable carrier, so it is winner-only and non-persisting.
#[test]
fn return_only_does_not_publish_reverse_index_or_persist() {
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<(u32, u8)> = InflightTable::default();
    let post_publish_count = Arc::new(AtomicUsize::new(0));
    let post_publish_count_cl = Arc::clone(&post_publish_count);

    let v = cooperative_admit_with_post_publish_by_flight_key(
        &map,
        &inflight,
        5u32,
        (5u32, 0u8),
        |entry: &String| Some(entry.clone()),
        || ComputeAdmission::<String, String>::ReturnOnly("winner-only".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| true,
        |_k: &u32, _e: &Arc<String>| {},
        move |_e: &Arc<String>, _k: &u32| {
            post_publish_count_cl.fetch_add(1, Ordering::SeqCst);
        },
        None,
    );

    assert_eq!(
        v.as_deref(),
        Some("winner-only"),
        "the ReturnOnly winner receives its value directly"
    );
    assert_eq!(
        map.len(),
        0,
        "a ReturnOnly outcome must NOT insert into the published map"
    );
    assert_eq!(
        post_publish_count.load(Ordering::SeqCst),
        0,
        "a ReturnOnly outcome must NOT fire post_publish (no reverse-index / persist)"
    );
}

/// A budgeted cache driven through the CACHE-KEY-IS-FLIGHT-KEY entry point
/// (`cooperative_admit_with_post_publish`) must not self-deadlock when its
/// `post_publish` hook re-enters the published map to FIFO-evict a victim
/// during publication.
///
/// This is the exact shape of the two budgeted unified consumers
/// (`MaterializeStructureDb` / `RefCycleResultDb`): their `post_publish`
/// runs `register_post_publish` → `evict_budget_victim` →
/// `entries.remove_if(victim)` on the SAME `DashMap` the entry was just
/// published into. If the publish holds the map's shard WRITE guard across
/// `post_publish` (as a per-map-key linearization does), and the FIFO
/// victim hashes to the same shard as the just-published key, the
/// re-entrant `remove_if` blocks forever on a write guard this very thread
/// already holds — a non-reentrant same-shard self-deadlock.
///
/// The repro is deterministic, not a race: every key has a CONSTANT
/// `Hash` (distinct under `Eq`, so K1 and K2 are separate map entries, but
/// they hash identically), so they always land on the same `DashMap`
/// shard, and the publish runs entirely on one worker thread. With cap = 1,
/// publishing K2 while K1 is resident evicts K1 from inside K2's
/// `post_publish`; under the buggy linearized publish K2's shard guard is
/// held across that eviction, so `remove_if(K1)` (same shard)
/// self-deadlocks the worker.
///
/// Deadlock-freedom is asserted with a WATCHDOG: the publish sequence runs
/// on a worker thread that signals completion over an `mpsc` channel; the
/// test waits with `recv_timeout(5s)`. Pre-fix the worker hangs and the
/// timeout fires → the test FAILS. Post-fix the cache-key-is-flight-key
/// publish uses `map.insert` (transient guard released before
/// `post_publish`), so the re-entrant eviction acquires the now-free shard
/// lock and the worker completes in well under the timeout. The test also
/// asserts the eviction actually fired (one victim removed, K2 sole
/// survivor) so it discriminates on eviction-DURING-publication, not merely
/// on "did not hang". (Pre-fix the deadlocked worker thread is abandoned;
/// the test process exits after the failure, so the leak is bounded.)
#[test]
fn unified_budgeted_post_publish_eviction_does_not_self_deadlock() {
    use std::collections::VecDeque;
    use std::hash::{Hash, Hasher};
    use std::sync::mpsc;
    use std::sync::Mutex;

    // A key that is DISTINCT under `Eq` (so K1 and K2 occupy separate map
    // slots) but hashes to a CONSTANT (so every key lands on the same
    // `DashMap` shard under the map's `RandomState`). This forces K2's
    // publish and the eviction of victim K1 onto the SAME shard lock — the
    // precise condition the buggy linearized publish self-deadlocks on —
    // without needing a custom map hasher (the substrate hardcodes the
    // default `RandomState`).
    #[derive(PartialEq, Eq, Clone, Copy, Debug)]
    struct CollidingKey(u32);
    impl Hash for CollidingKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            // Constant — every value hashes identically, so all keys share
            // one shard. `Eq` still distinguishes them.
            state.write_u8(0);
        }
    }

    let map: Arc<DashMap<CollidingKey, Arc<String>>> = Arc::new(DashMap::new());
    let inflight: Arc<InflightTable<CollidingKey>> = Arc::new(InflightTable::default());
    // FIFO retention budget, cap = 1 — models the unified consumers'
    // `retention_budget.record_admission` returning the oldest key as a
    // victim once the cap is exceeded.
    let fifo: Arc<Mutex<VecDeque<CollidingKey>>> = Arc::new(Mutex::new(VecDeque::new()));
    const CAP: usize = 1;
    let evictions = Arc::new(AtomicUsize::new(0));

    let map_w = Arc::clone(&map);
    let inflight_w = Arc::clone(&inflight);
    let fifo_w = Arc::clone(&fifo);
    let evictions_w = Arc::clone(&evictions);

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        // Publish two distinct keys in sequence on ONE thread. K1 fills the
        // cap; K2 overflows it and must evict K1 from inside K2's
        // `post_publish` while (pre-fix) holding K2's shard guard.
        for n in [1u32, 2u32] {
            let key = CollidingKey(n);
            let map_inner = Arc::clone(&map_w);
            let fifo_inner = Arc::clone(&fifo_w);
            let evictions_inner = Arc::clone(&evictions_w);
            let v = cooperative_admit_with_post_publish(
                &map_w,
                &inflight_w,
                key,
                |entry: &String| Some(entry.clone()),
                move || ComputeAdmission::Cacheable(format!("v{n}")),
                |entry: &String| entry.clone(),
                |_entry: &String| true,
                |_k: &CollidingKey, _e: &Arc<String>| {},
                // post_publish: record the admission in the FIFO, then
                // FIFO-evict any overflow by removing the victim from the
                // SAME map — exactly `evict_budget_victim`'s `remove_if`.
                move |_e: &Arc<String>, published_key: &CollidingKey| {
                    let victims: Vec<CollidingKey> = {
                        let mut q = fifo_inner.lock().unwrap();
                        q.push_back(*published_key);
                        let mut v = Vec::new();
                        while q.len() > CAP {
                            if let Some(victim) = q.pop_front() {
                                v.push(victim);
                            }
                        }
                        v
                    };
                    for victim in victims {
                        // Re-entrant removal on the just-published map. With
                        // a held shard guard (pre-fix) and a same-shard
                        // victim this is the self-deadlock.
                        if map_inner.remove_if(&victim, |_, _| true).is_some() {
                            evictions_inner.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                },
                None,
            );
            assert_eq!(v.as_deref(), Some(format!("v{n}").as_str()));
        }
        done_tx.send(()).expect("worker signals completion");
    });

    // Watchdog: a hung publish never sends, so the timeout fires and the
    // test fails. A healthy publish completes near-instantly.
    match done_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(()) => {
            worker.join().expect("worker thread joins cleanly");
        }
        Err(_) => panic!(
            "unified budgeted publish DEADLOCKED: post_publish re-entered the published map to \
             evict a same-shard victim while the publish held that shard's write guard"
        ),
    }

    // Eviction actually ran during publication — the test discriminates on
    // re-entrant-eviction-during-publish, not merely on deadlock-freedom.
    assert_eq!(
        evictions.load(Ordering::SeqCst),
        1,
        "publishing K2 over a cap-1 budget must evict K1 from inside K2's post_publish"
    );
    assert_eq!(map.len(), 1, "exactly the surviving entry (K2) remains");
    assert!(
        map.get(&CollidingKey(2)).is_some() && map.get(&CollidingKey(1)).is_none(),
        "K1 was evicted; K2 survives"
    );
}
