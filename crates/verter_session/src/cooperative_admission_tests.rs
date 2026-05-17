//! Cooperative-admission substrate tests — extracted sibling of
//! `cooperative_admission.rs` (kept under the file-size guard cap).
//!
//! Included as a child `mod` of `cooperative_admission` via
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
/// Stabilisation note: the original 5 ms `thread::sleep` between
/// winner-spawn and joiner-spawn under-budgeted scheduler latency
/// under workspace-parallel test load on Windows. When the OS
/// scheduled the winner to panic + the panic guard's `Drop` to
/// remove the inflight slot BEFORE the joiner reached
/// `cooperative_get_or_insert`, the joiner found no inflight slot,
/// claimed a fresh one itself, and ran its own `compute` (returning
/// `Some("never reached")`) instead of waking on the panicked-winner
/// condvar with `None`. The fix replaces the timed sleep with a
/// `mpsc::sync_channel(0)` rendezvous: the winner's compute sends
/// `()` AFTER `state.claimed = true` (claim happens unconditionally
/// before `compute()` runs in `cooperative_get_or_insert`) and
/// BEFORE its own pre-panic sleep. Main blocks on `recv()` before
/// spawning the joiner, so the joiner is guaranteed to enter
/// `cooperative_get_or_insert` while the winner is still inside
/// compute. The assertion is unchanged (joiner returns `None`), so
/// this is not a Stub Prevention violation — only the timing
/// primitive changed.
#[test]
fn cooperative_admission_panic_wakes_waiters() {
    use std::sync::mpsc;

    // Use a dedicated map per scenario to avoid cross-test races.
    let map: DashMap<u32, Arc<String>> = DashMap::new();
    let inflight: InflightTable<u32> = InflightTable::default();
    let map = Arc::new(map);
    let inflight = Arc::new(inflight);

    // Joiner that arrives second; will block on the panicking
    // winner's slot.
    let joiner_done = Arc::new(AtomicUsize::new(0));

    // Rendezvous channel — the winner's compute() signals AFTER
    // claim (i.e. once `state.claimed = true` in the inflight slot)
    // and BEFORE its pre-panic sleep. Main blocks on `recv()`
    // before spawning the joiner so the joiner cannot race ahead
    // of the winner's claim.
    let (claimed_tx, claimed_rx) = mpsc::sync_channel::<()>(0);

    // Winner thread that panics inside compute.
    let map_w = Arc::clone(&map);
    let inflight_w = Arc::clone(&inflight);
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
                    // Slack window so the joiner has time to enter
                    // `cooperative_get_or_insert` and acquire the
                    // existing inflight slot reference before the
                    // panic guard's `Drop` removes the slot from
                    // the inflight table. Sized for headroom under
                    // workspace-parallel test load on Windows.
                    thread::sleep(Duration::from_millis(50));
                    panic!("simulated compute panic");
                },
                |entry: &String| entry.clone(),
                |_entry: &String| true,
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
        );
        joiner_done_j.fetch_add(1, Ordering::SeqCst);
        result
    });

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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
         `crates/verter_session/src/cooperative_admission.rs` joiner branch."
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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
            |_entry_arc: &Arc<String>, _k: &u32| {},
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
