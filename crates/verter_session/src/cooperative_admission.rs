//! Admission-only primitive for cooperative get-or-compute over a
//! [`DashMap`]-backed cache.
//!
//! Plan §3 D3.2 sub-task 3.2.0 (architectural-debt-closure revision 10).
//!
//! ## Contract
//!
//! `cooperative_get_or_insert` provides:
//!   - **Exactly-one-computer guarantee.** When N threads call concurrently
//!     for the same key on a cold miss, exactly ONE thread runs `compute()`.
//!     Other threads block on a per-key condvar until the winner publishes
//!     (success) or fails (panic / `compute()` returned `None`).
//!   - **Cooperative wait.** Joiners do NOT busy-spin; they wait on a
//!     `parking_lot::Condvar` and wake on publish or fail.
//!   - **Panic safety.** If `compute()` panics, the winner's RAII guard
//!     marks the slot `failed = true` and notifies waiters; subsequent
//!     callers retry the cold path.
//!   - **Post-compute revalidation.** After `compute()` returns, the caller-
//!     supplied `revalidate_after_compute` runs against the freshly-built
//!     `Entry` BEFORE the entry is inserted. This catches the race where
//!     a file mutation occurred during the cold compute window: the entry's
//!     dep-signature is no longer valid against host state, so the publish
//!     is skipped and waiters fall through to retry.
//!   - **Value projection.** Three callbacks separate concerns:
//!     - `validate(&Entry) -> Option<V>`: warm-hit validation.
//!     - `compute() -> Option<Entry>`: cold build.
//!     - `project(&Entry) -> V`: value projection from the published
//!       entry.
//!
//!     The `Entry` shape may be richer than the projected `V` — e.g. an
//!     entry can carry dep-signature plus value, while a Value is just
//!     the value.
//!
//! ## What this primitive deliberately does NOT do
//!
//! Strictly decoupled from semantic-specific concerns. Callers compose
//! these on top of the admission core:
//!   - **Recursion sentinels** (e.g. same-path re-entry detection):
//!     callers track this in their own thread-local stack.
//!   - **Stats / metrics**: callers maintain their own counters around
//!     the call site.
//!   - **Request-context cache events**: callers record events as needed
//!     via `verter_scheduler::request_context`.
//!   - **Retry budgets**: when a publish is skipped (post-compute
//!     revalidation fails), the caller decides whether to retry, sleep,
//!     or give up.
//!   - **Invalidation TOCTOU**: per-cache invalidation policy lives in the
//!     caller's `validate` and `revalidate_after_compute` closures.
//!
//! ## Per-cache inflight tables
//!
//! Each typed cache owns an [`InflightTable<K>`] (this module's wrapper
//! around `Mutex<HashMap<K, Arc<InflightSlot>>>`) so that contention on
//! one cache does not stall threads operating on a different cache. The
//! plan's D3.2 admission-control architecture expects this isolation.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};

/// Per-key in-flight slot. The winner publishes via `state.completed`;
/// joiners wait on `ready` until publish or fail.
struct InflightSlot {
    state: Mutex<InflightSlotState>,
    ready: Condvar,
}

#[derive(Default)]
struct InflightSlotState {
    /// `true` once a thread has claimed ownership of the cold build.
    /// Subsequent threads see `claimed == true` and wait on `ready`.
    claimed: bool,
    /// `true` once the winner has finished — successfully or otherwise.
    completed: bool,
    /// `true` if the winner's `compute()` returned `None`, panicked, OR
    /// `revalidate_after_compute` rejected the freshly-built entry.
    /// Joiners observing `failed = true` return `None`; subsequent calls
    /// retry the cold path.
    failed: bool,
}

impl InflightSlot {
    fn new() -> Self {
        Self {
            state: Mutex::new(InflightSlotState::default()),
            ready: Condvar::new(),
        }
    }
}

/// Per-cache in-flight table. Each typed cache DB owns one of these so
/// admission control is isolated across caches.
pub struct InflightTable<K>
where
    K: Hash + Eq + Clone,
{
    table: Mutex<HashMap<K, Arc<InflightSlot>>>,
}

impl<K> Default for InflightTable<K>
where
    K: Hash + Eq + Clone,
{
    fn default() -> Self {
        Self {
            table: Mutex::new(HashMap::new()),
        }
    }
}

impl<K> InflightTable<K>
where
    K: Hash + Eq + Clone,
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only count of live in-flight slots. Test-only / observability.
    #[cfg(test)]
    pub fn live_count(&self) -> usize {
        self.table.lock().len()
    }
}

/// RAII guard that fails the in-flight slot if the cold build panics or
/// returns early. Without this, a panic inside `compute()` would leave
/// `claimed = true, completed = false` forever — joiners would block on
/// the condvar with no possible publish to wake them.
struct InflightPanicGuard<'a, K>
where
    K: Hash + Eq + Clone,
{
    slot: Arc<InflightSlot>,
    table: &'a Mutex<HashMap<K, Arc<InflightSlot>>>,
    key: K,
    finished: bool,
}

impl<'a, K> InflightPanicGuard<'a, K>
where
    K: Hash + Eq + Clone,
{
    fn new(
        slot: Arc<InflightSlot>,
        table: &'a Mutex<HashMap<K, Arc<InflightSlot>>>,
        key: K,
    ) -> Self {
        Self {
            slot,
            table,
            key,
            finished: false,
        }
    }

    fn mark_finished(&mut self) {
        self.finished = true;
    }
}

impl<'a, K> Drop for InflightPanicGuard<'a, K>
where
    K: Hash + Eq + Clone,
{
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Panic / early-return path — mark the slot failed so joiners
        // wake and retry rather than waiting forever on a condvar that
        // will never be signalled.
        {
            let mut state = self.slot.state.lock();
            if !state.completed {
                state.completed = true;
                state.failed = true;
            }
        }
        self.slot.ready.notify_all();
        // Retire the in-flight slot from the per-cache table so the next
        // caller starts a fresh build.
        let mut table = self.table.lock();
        table.remove(&self.key);
    }
}

/// Cooperative get-or-compute over a [`DashMap`]-backed cache. See module
/// docs for the full contract.
///
/// ## Type parameters
///
/// - `K`: cache key — `Eq + Hash + Clone`.
/// - `Entry`: cache value, wrapped in `Arc<Entry>` inside the `DashMap`.
///   Carries everything needed to validate (e.g. dep-signature) and to
///   project (e.g. payload).
/// - `V`: projected value — what the caller actually wants. Cheap to
///   clone (`V: Clone`).
///
/// ## Closures
///
/// - `validate`: runs on warm-hit; returns `Some(V)` if the entry is
///   still valid for the caller's view of host state (typically a
///   dep-signature check), `None` to fall through and remove the stale
///   entry.
/// - `compute`: runs on cold-miss for exactly one thread. Returns
///   `Some(Entry)` on success, `None` on observable failure (e.g. dep
///   missing, parse error). A panic also classifies as failure via the
///   RAII guard.
/// - `project`: extracts the projected value from a published entry.
///   Called by both the winner (after publish) and joiners (after
///   waking from the condvar).
/// - `revalidate_after_compute`: after `compute()` returns successfully,
///   this re-validates the entry against current host state. Returns
///   `false` if the entry is now stale (e.g. file mutated mid-compute);
///   the publish is skipped and waiters fall through.
///
/// ## Returns
///
/// - `Some(V)` on warm-hit, on cold success + valid post-compute, or on
///   joiner success.
/// - `None` if `compute()` returns `None`, panics, post-compute
///   revalidation fails, or the joiner observes a failed winner.
pub fn cooperative_get_or_insert<K, Entry, V, Validate, Compute, Project, Revalidate>(
    map: &DashMap<K, Arc<Entry>>,
    inflight: &InflightTable<K>,
    key: K,
    validate: Validate,
    compute: Compute,
    project: Project,
    revalidate_after_compute: Revalidate,
) -> Option<V>
where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Validate: FnOnce(&Entry) -> Option<V>,
    Compute: FnOnce() -> Option<Entry>,
    Project: FnOnce(&Entry) -> V,
    Revalidate: FnOnce(&Entry) -> bool,
{
    cooperative_get_or_insert_with_post_publish(
        map,
        inflight,
        key,
        validate,
        compute,
        project,
        revalidate_after_compute,
        |_, _| {},
    )
}

/// Plan §1.5 / §10.1 — extension of [`cooperative_get_or_insert`]
/// with a `post_publish` callback that fires AFTER `entries.insert`
/// AND AFTER successful `revalidate_after_compute`. The callback
/// receives the published `Arc<Entry>` and the key.
///
/// **Race-closure contract.** post_publish is NOT inside the
/// inflight slot's state lock. Synchronisation against concurrent
/// invalidation comes from `revalidate_after_compute`'s
/// dep-signature check, which sees the host's CURRENT state. If
/// invalidation happened during compute, the entry is stale and
/// revalidation fails BEFORE post_publish runs.
///
/// **Eventually-consistent reverse-index window.** A concurrent
/// invalidator that drains a canonical's reverse-index between
/// `entries.insert` and `post_publish` would miss the
/// registration. The orphan registration is caught by the next
/// peek's stale-check (the entry references the invalidated
/// canonical with its old dep_signature; the host has the new
/// state) and proactively removed. The orphan window is bounded
/// per edit-cycle.
///
/// **Compute closure synchronicity contract (plan §4.20).** The
/// `compute` closure runs SYNCHRONOUSLY on the caller's thread.
/// Future maintainers MUST preserve this invariant; it underpins
/// borrow-capture safety in callers (e.g.,
/// `RefCycleResultDb::get_or_compute` borrows `&VerterHost`
/// directly without `'static` bounds or thread-hop dispatch).
///
/// **Halt grep before each commit modifying this function**
/// (production-only per R8-4 — strips `#[cfg(test)]` regions before
/// grep so the existing test-only `thread::spawn` calls at lines
/// 434/511/557 do not false-trigger):
/// ```text
/// awk '/^#\[cfg\(test\)\]\s*$/{intest=1} !intest{print} /^}\s*$/&&intest{intest=0}' \
///     crates/verter_session/src/cooperative_admission.rs \
///   | grep -n "thread::spawn\|rayon\|tokio::spawn" \
///   || echo "OK: no production thread-spawn"
/// ```
/// Must produce "OK: no production thread-spawn" (or zero match
/// lines from grep).
#[allow(clippy::too_many_arguments)]
pub fn cooperative_get_or_insert_with_post_publish<
    K,
    Entry,
    V,
    Validate,
    Compute,
    Project,
    Revalidate,
    PostPublish,
>(
    map: &DashMap<K, Arc<Entry>>,
    inflight: &InflightTable<K>,
    key: K,
    validate: Validate,
    compute: Compute,
    project: Project,
    revalidate_after_compute: Revalidate,
    post_publish: PostPublish,
) -> Option<V>
where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Validate: FnOnce(&Entry) -> Option<V>,
    Compute: FnOnce() -> Option<Entry>,
    Project: FnOnce(&Entry) -> V,
    Revalidate: FnOnce(&Entry) -> bool,
    PostPublish: FnOnce(&Arc<Entry>, &K),
{
    // Phase 1: warm-hit + validation.
    if let Some(entry_arc) = map.get(&key).map(|e| e.clone()) {
        if let Some(value) = validate(&entry_arc) {
            return Some(value);
        }
        // Stale entry; remove. DashMap::remove is idempotent under races.
        map.remove(&key);
    }

    // Phase 2: claim the inflight slot or join an in-progress build.
    let slot = {
        let mut table = inflight.table.lock();
        table
            .entry(key.clone())
            .or_insert_with(|| Arc::new(InflightSlot::new()))
            .clone()
    };

    let mut state = slot.state.lock();
    if state.claimed {
        // Joiner — wait for the winner to publish or fail.
        slot.ready.wait_while(&mut state, |s| !s.completed);
        if state.failed {
            return None;
        }
        // Winner succeeded; entry is in the map. Re-read and project.
        drop(state);
        let entry_arc = map.get(&key).map(|e| e.clone())?;
        return Some(project(&entry_arc));
    }
    state.claimed = true;
    drop(state);

    // Phase 3: cold winner runs compute under a panic guard.
    let mut panic_guard = InflightPanicGuard::new(Arc::clone(&slot), &inflight.table, key.clone());

    let computed = compute();

    let value = match computed {
        Some(entry) => {
            // Post-compute revalidation. If a mutation invalidated the
            // entry's dep-signature during the cold window, skip publish
            // and signal failure to waiters.
            if !revalidate_after_compute(&entry) {
                {
                    let mut state = slot.state.lock();
                    state.completed = true;
                    state.failed = true;
                }
                slot.ready.notify_all();
                panic_guard.mark_finished();
                drop(panic_guard);
                inflight.table.lock().remove(&key);
                return None;
            }

            let entry_arc = Arc::new(entry);
            let value = project(&entry_arc);
            map.insert(key.clone(), Arc::clone(&entry_arc));

            // Plan §1.5 / §10.1 post_publish: fires AFTER
            // entries.insert AND AFTER successful revalidate.
            // Reverse-index registration lives here. NOT inside
            // the inflight slot's state lock — the race-closure
            // is via the revalidate_after_compute check above
            // (eventually-consistent for the reverse index per
            // the function-level docs).
            post_publish(&entry_arc, &key);

            // Mark slot completed; wake joiners.
            {
                let mut state = slot.state.lock();
                state.completed = true;
            }
            slot.ready.notify_all();

            Some(value)
        }
        None => {
            // Compute returned None — failure.
            {
                let mut state = slot.state.lock();
                state.completed = true;
                state.failed = true;
            }
            slot.ready.notify_all();
            None
        }
    };

    panic_guard.mark_finished();
    drop(panic_guard);

    // Retire the inflight slot. Future callers either hit the warm map
    // or start a fresh inflight if the publish was skipped.
    inflight.table.lock().remove(&key);

    value
}

// ============================================================================
// Sub-task 3.2.0 gating tests (5 required by plan §3 D3.2)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    /// Plan D3.2 test 1: 100 threads racing on the same key — exactly
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

    /// Plan D3.2 test 2: winner panics in compute → waiters wake with
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

    /// Plan D3.2 test 3: post-compute revalidation returns false →
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

    /// Plan D3.2 test 4: simulated invalidation during compute — first
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

    /// Plan D3.2 test 5: same Entry projects to different Value types
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
}
