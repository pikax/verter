//! Admission-only primitive for cooperative get-or-compute over a
//! [`DashMap`]-backed cache.
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
//!     - `validate(&Entry) -> Option<V>`: read-side validation. Runs on the
//!       warm-hit fast path AND on every cooperative joiner that wakes onto
//!       a winner's published entry.
//!     - `compute() -> Option<Entry>`: cold build.
//!     - `project(&Entry) -> V`: value projection from the published
//!       entry. Runs ONLY on the cold winner's own thread after it
//!       publishes.
//!
//!     The `Entry` shape may be richer than the projected `V` — e.g. an
//!     entry can carry dep-signature plus value, while a Value is just
//!     the value.
//!
//! ## Joiner read-side validation
//!
//! A follower that coalesces onto an in-flight cold build is NOT
//! guaranteed to be running under the same view/overlay as the winner:
//! two requests can carry the same cache key while executing under
//! different overlays (a base context and a session/overlay context, or
//! two different overlays). Their results are NOT interchangeable — each
//! must validate against its own content identity.
//!
//! Therefore, when a joiner wakes onto a successfully-published entry it
//! re-reads the published `Arc<Entry>` and runs the caller's `validate`
//! closure — the SAME read-side contract a warm hit runs — NOT `project`.
//!   - `validate` returns `Some(value)` → the winner's entry is valid for
//!     the follower's view; the follower returns that value. `validate`
//!     also performs the caller's fact-bubble side effect, so an outer
//!     cold-compute scope spawning this joiner still observes the entry's
//!     transitive dependency facts.
//!   - `validate` returns `None` → the winner's entry is stale for the
//!     follower's view. The follower removes the exact stale published
//!     entry (a `ptr_eq` guard so a concurrent fresh winner is not
//!     evicted), retires the same in-flight slot (`ptr_eq`-guarded), drops
//!     locks, and re-enters admission so it cold-computes for its OWN
//!     view.
//!
//! ## Removal-side cache cleanup
//!
//! The substrate removes a *published* `Arc<Entry>` in exactly two
//! places: the warm-hit path when `validate` rejects a stale entry, and
//! the joiner-fork path when a cross-view follower rejects the winner's
//! entry. Both removals must trigger the cache's own removal-side
//! bookkeeping — caches with publish-side state (a shared live counter,
//! a per-canonical reverse index) keep that state consistent only if a
//! substrate removal decrements / drains symmetrically with the
//! publish-side increment / registration.
//!
//! The caller therefore supplies a `removal_cleanup` closure — the
//! removal-side counterpart of `post_publish`. The substrate invokes it
//! with the `(key, removed entry)` pair every time it removes a
//! published entry on either path. Caches whose admission path bumps a
//! live counter (in `compute` or in `post_publish`) pass a closure that
//! decrements it and drains any reverse-index registration; caches with
//! no publish-side bookkeeping pass a no-op. `removal_cleanup` is
//! `FnMut` because one cooperative call can remove more than once (a
//! warm-hit reject, then a joiner-fork reject after a re-loop).
//!
//! `removal_cleanup` fires ONLY for removals of an already-published
//! entry. It does NOT fire when a cold compute is skipped without ever
//! publishing (post-compute revalidation rejection, `Failed`,
//! `ReturnOnly`): nothing was inserted, so there is nothing to clean up.
//!
//! `revalidate_after_compute` is NOT used for the joiner path: it is a
//! winner-side publish-race closer that returns only `bool`; `validate`
//! is the read-side contract and produces / bubbles the caller-visible
//! value.
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
//! D3.2 admission-control architecture expects this isolation.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};

/// First-class outcome of a cooperative-admission cold compute.
///
/// `compute` closures return one of three variants:
///
/// - `Cacheable(Entry)` — the result is valid AND cacheable. The
///   admission flow inserts the entry into the map; joiners re-read the
///   freshly-published entry and run `validate` against their own view.
/// - `ReturnOnly(V)` — the result is valid but NOT cacheable (e.g.
///   the producer's fact-signature tracer overflowed). The admission
///   flow does NOT insert into the map. A `ReturnOnly` outcome carries
///   no `Entry` and no dep-signature carrier, so it CANNOT be
///   view-validated against a cooperative joiner's own view. It is
///   therefore non-shareable: the winner alone receives the `V`, and
///   every joiner is forced to fork and cold-recompute for its own
///   view. The next cold-miss recomputes from scratch.
/// - `Failed` — compute observed a fatal condition (panic substitute,
///   missing dep, parse error). Joiners wake to a failed slot and
///   surface `None`; the next cold-miss retries.
///
/// **Why three variants.** Before the carrier consolidation the
/// "valid-but-non-cacheable" case was modelled with a stack-local
/// `RefCell<Option<...>>` side channel inside the winner's compute
/// closure, so cooperative joiners on the same key saw an empty
/// side channel and returned a Tainted outcome even when the winner
/// computed a valid result. `ReturnOnly` lifts that case into the
/// admission contract: the winner observes its valid outcome, and
/// joiners — which cannot validate a carrier-less value against their
/// own view — fork and recompute rather than inherit a possibly
/// wrong-view result.
pub enum ComputeAdmission<V, Entry> {
    /// Result is valid AND cacheable. Cache admits the entry; joiners
    /// re-read the map and run `validate`.
    Cacheable(Entry),
    /// Result is valid but NOT cacheable. Cache does NOT admit. The
    /// winner receives the `V` directly; joiners cannot view-validate a
    /// carrier-less value and therefore fork + cold-recompute.
    ReturnOnly(V),
    /// Cold-compute failed (panic substitute, missing dep, etc.).
    /// Joiners surface `None`; subsequent callers retry the cold path.
    Failed,
}

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
    /// `true` when the winner's compute returned a valid-but-non-cacheable
    /// outcome (`ComputeAdmission::ReturnOnly`). The map stays empty for
    /// such a winner. A joiner that wakes observing `non_cacheable_winner
    /// == true` (with `failed == false`) has no published entry to
    /// validate against its own view, so it forks and cold-recomputes.
    /// `false` for cacheable outcomes and for failures.
    non_cacheable_winner: bool,
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
    #[allow(dead_code)]
    pub fn live_count(&self) -> usize {
        self.table.lock().len()
    }

    /// Strong-count of the in-flight slot `Arc` currently registered
    /// under `key`, or `None` if no slot is registered. Test-only —
    /// drives the deterministic joiner rendezvous: a thread is a
    /// confirmed cooperative joiner once it has cloned its own `Arc`
    /// to the winner's in-flight slot, observable as a step up in this
    /// count. Reading the count through the table's shard guard does
    /// NOT itself bump the count.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn slot_strong_count(&self, key: &K) -> Option<usize> {
        self.table.lock().get(key).map(Arc::strong_count)
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
        // caller starts a fresh build. `ptr_eq`-guarded: a cross-view
        // joiner that forked may already have installed a fresh
        // `InflightSlot` for the same key; an unconditional remove would
        // evict that fresh slot.
        let mut table = self.table.lock();
        if table
            .get(&self.key)
            .is_some_and(|existing| Arc::ptr_eq(existing, &self.slot))
        {
            table.remove(&self.key);
        }
    }
}

/// Remove the in-flight slot from the per-cache table iff the table
/// still holds the SAME `Arc` this caller owned. A cross-view joiner
/// that forked may already have installed a fresh slot for the same
/// key; an unconditional remove would evict that fresh slot.
fn retire_slot_if_current<K>(
    table: &Mutex<HashMap<K, Arc<InflightSlot>>>,
    key: &K,
    slot: &Arc<InflightSlot>,
) where
    K: Eq + Hash + Clone,
{
    let mut table = table.lock();
    if table
        .get(key)
        .is_some_and(|existing| Arc::ptr_eq(existing, slot))
    {
        table.remove(key);
    }
}

#[cfg(test)]
thread_local! {
    /// Test-only rendezvous: a hook fired by
    /// [`remove_published_entry_with_cleanup`] AFTER `map.remove_if`
    /// succeeds but BEFORE the caller's `removal_cleanup` runs.
    ///
    /// This is the precise interleaving point a removal-cleanup race
    /// needs: a test installs a hook that cold-publishes a fresh entry
    /// under the same key (the work a concurrent caller would do while
    /// the removing caller is preempted), then lets the real
    /// `removal_cleanup` run. It is a deterministic rendezvous — the
    /// hook IS the synchronisation point — not a timing sleep. The
    /// hook is thread-local so it only affects the installing test's
    /// thread, and it fires the production removal path otherwise
    /// unchanged.
    static REMOVAL_CLEANUP_PRE_HOOK: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: install a hook fired between `map.remove_if` and
/// `removal_cleanup` inside [`remove_published_entry_with_cleanup`].
/// Returns a guard that clears the hook on drop so it cannot leak into
/// a later test on the same worker thread.
#[cfg(test)]
pub(crate) fn install_removal_cleanup_pre_hook(hook: Box<dyn Fn()>) -> RemovalCleanupPreHookGuard {
    REMOVAL_CLEANUP_PRE_HOOK.with(|h| *h.borrow_mut() = Some(hook));
    RemovalCleanupPreHookGuard
}

/// RAII guard that clears the thread-local removal-cleanup pre-hook.
#[cfg(test)]
pub(crate) struct RemovalCleanupPreHookGuard;

#[cfg(test)]
impl Drop for RemovalCleanupPreHookGuard {
    fn drop(&mut self) {
        REMOVAL_CLEANUP_PRE_HOOK.with(|h| *h.borrow_mut() = None);
    }
}

/// Remove the exact published `Arc<Entry>` the caller validated — a
/// `ptr_eq` guard so a concurrent fresh winner's entry is not evicted —
/// and, on a real removal, run the caller's `removal_cleanup` so the
/// cache's removal-side bookkeeping (live counter, reverse index) stays
/// symmetric with its publish-side bookkeeping.
///
/// Used by both substrate removal sites: the warm-hit path when
/// `validate` rejects a stale entry, and the joiner-fork path when a
/// cross-view follower rejects the winner's entry. A raw `map.remove_if`
/// in either site would skip the cleanup and leave caches with
/// publish-side counters / reverse indexes out of sync after a removal.
fn remove_published_entry_with_cleanup<K, Entry, RemovalCleanup>(
    map: &DashMap<K, Arc<Entry>>,
    key: &K,
    entry_arc: &Arc<Entry>,
    removal_cleanup: &mut RemovalCleanup,
) where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    RemovalCleanup: FnMut(&K, &Arc<Entry>),
{
    if let Some((removed_key, removed_entry)) =
        map.remove_if(key, |_, existing| Arc::ptr_eq(existing, entry_arc))
    {
        // Test-only rendezvous: a fresh re-publish under the same key
        // can land here, between the `remove_if` and the cleanup. This
        // is the exact window the removal-cleanup identity-check guards.
        #[cfg(test)]
        REMOVAL_CLEANUP_PRE_HOOK.with(|h| {
            if let Some(hook) = h.borrow().as_ref() {
                hook();
            }
        });
        removal_cleanup(&removed_key, &removed_entry);
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
/// - `validate`: read-side validation. Runs on the warm-hit fast path
///   AND on every cooperative joiner that wakes onto a winner's
///   published entry. Returns `Some(V)` if the entry is still valid for
///   the caller's view of host state (typically a dep-signature check),
///   `None` to fall through, remove the stale entry, and cold-compute.
///   Because a joiner runs `validate`, the bound is `FnMut`: one call
///   can reject a stale warm hit and a later call (after the joiner
///   wakes) can validate the joined winner's entry.
/// - `compute`: runs on cold-miss for exactly one thread. Returns
///   `Some(Entry)` on success, `None` on observable failure (e.g. dep
///   missing, parse error). A panic also classifies as failure via the
///   RAII guard. Logically one-shot — the loop carries it in an
///   `Option` and `take()`s it only when this caller becomes the cold
///   winner.
/// - `project`: extracts the projected value from a published entry.
///   Called ONLY by the cold winner on its own thread after it
///   publishes. Joiners do NOT call `project` — they call `validate`.
/// - `revalidate_after_compute`: after `compute()` returns successfully,
///   this re-validates the entry against current host state. Returns
///   `false` if the entry is now stale (e.g. file mutated mid-compute);
///   the publish is skipped and waiters fall through.
/// - `removal_cleanup`: the removal-side counterpart of `post_publish`.
///   Runs whenever the substrate removes an already-published entry —
///   on the warm-hit reject path AND on the joiner-fork reject path —
///   so a cache that bumps a live counter / reverse index on publish
///   can decrement / drain symmetrically. `FnMut`: one cooperative call
///   can remove more than once. A cache with no publish-side
///   bookkeeping passes a no-op.
///
/// ## Returns
///
/// - `Some(V)` on warm-hit, on cold success + valid post-compute, or on
///   joiner success (joiner's own `validate` accepted the winner's
///   entry).
/// - `None` if `compute()` returns `None`, panics, post-compute
///   revalidation fails, or the joiner observes a failed winner.
#[allow(clippy::too_many_arguments)]
pub fn cooperative_get_or_insert<
    K,
    Entry,
    V,
    Validate,
    Compute,
    Project,
    Revalidate,
    RemovalCleanup,
>(
    map: &DashMap<K, Arc<Entry>>,
    inflight: &InflightTable<K>,
    key: K,
    validate: Validate,
    compute: Compute,
    project: Project,
    revalidate_after_compute: Revalidate,
    removal_cleanup: RemovalCleanup,
) -> Option<V>
where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Validate: FnMut(&Entry) -> Option<V>,
    Compute: FnOnce() -> Option<Entry>,
    Project: FnOnce(&Entry) -> V,
    Revalidate: FnMut(&Entry) -> bool,
    RemovalCleanup: FnMut(&K, &Arc<Entry>),
{
    cooperative_get_or_insert_with_post_publish(
        map,
        inflight,
        key,
        validate,
        compute,
        project,
        revalidate_after_compute,
        removal_cleanup,
        |_, _| {},
    )
}

/// Extension of [`cooperative_get_or_insert`]
/// with a `post_publish` callback that fires AFTER `entries.insert`
/// AND AFTER successful `revalidate_after_compute`. The callback
/// receives the published `Arc<Entry>` and the key.
///
/// `post_publish` is winner-only — it fires exactly once, on the cold
/// winner's thread after a successful publish. Cooperative joiners run
/// `validate` against the published entry; they never re-run
/// `post_publish`, so reverse-index registration and live counters are
/// not duplicated.
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
/// **Compute closure synchronicity contract.** The
/// `compute` closure runs SYNCHRONOUSLY on the caller's thread.
/// Future maintainers MUST preserve this invariant; it underpins
/// borrow-capture safety in callers (e.g.,
/// `RefCycleResultDb::get_or_compute` borrows `&VerterHost`
/// directly without `'static` bounds or thread-hop dispatch).
///
/// **Halt grep before each commit modifying this function**
/// (production-only per R8-4 — strips `#[cfg(test)]` regions before
/// grep so the existing test-only `thread::spawn` calls do not
/// false-trigger):
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
    RemovalCleanup,
    PostPublish,
>(
    map: &DashMap<K, Arc<Entry>>,
    inflight: &InflightTable<K>,
    key: K,
    mut validate: Validate,
    compute: Compute,
    project: Project,
    mut revalidate_after_compute: Revalidate,
    mut removal_cleanup: RemovalCleanup,
    post_publish: PostPublish,
) -> Option<V>
where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Validate: FnMut(&Entry) -> Option<V>,
    Compute: FnOnce() -> Option<Entry>,
    Project: FnOnce(&Entry) -> V,
    Revalidate: FnMut(&Entry) -> bool,
    RemovalCleanup: FnMut(&K, &Arc<Entry>),
    PostPublish: FnOnce(&Arc<Entry>, &K),
{
    // `compute` and `project` are logically one-shot but the joiner
    // re-validation loop may iterate (a follower whose `validate`
    // rejects the winner's entry forks and re-enters admission). Carry
    // them in `Option`s and `take()` them only on the iteration where
    // THIS caller becomes the cold winner.
    let mut compute = Some(compute);
    let mut project = Some(project);
    loop {
        // Warm-hit + read-side validation.
        if let Some(entry_arc) = map.get(&key).map(|e| e.clone()) {
            if let Some(value) = validate(&entry_arc) {
                return Some(value);
            }
            // Stale entry; remove the exact `Arc` we validated so a
            // concurrent fresh winner's entry is not evicted, and run
            // the cache's removal-side cleanup so its live counter /
            // reverse index stay symmetric with the publish-side
            // bookkeeping.
            remove_published_entry_with_cleanup(map, &key, &entry_arc, &mut removal_cleanup);
        }

        // Claim the inflight slot or join an in-progress build.
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
            // Winner succeeded. Drop the slot lock, then re-read the
            // published entry and run the caller's `validate` closure
            // against THIS follower's view — NOT `project`. A follower
            // joining this in-flight build is not guaranteed to be
            // running under the same view/overlay as the winner, so the
            // winner's entry must validate against the follower's own
            // content identity (the same read-side contract a warm hit
            // runs). `validate` returning `Some` also performs the
            // caller's fact-bubble side effect.
            drop(state);
            if let Some(entry_arc) = map.get(&key).map(|e| e.clone()) {
                if let Some(value) = validate(&entry_arc) {
                    return Some(value);
                }
                // The winner's entry is stale for the follower's view.
                // Remove the exact stale entry (`ptr_eq`-guarded) AND run
                // the cache's removal-side cleanup so its live counter /
                // reverse index stay consistent, retire the same
                // in-flight slot (`ptr_eq`-guarded), and re-enter
                // admission so the follower cold-computes for its own
                // view.
                remove_published_entry_with_cleanup(map, &key, &entry_arc, &mut removal_cleanup);
            }
            retire_slot_if_current(&inflight.table, &key, &slot);
            drop(slot);
            continue;
        }
        state.claimed = true;
        drop(state);

        // Cold winner runs compute under a panic guard.
        let mut panic_guard =
            InflightPanicGuard::new(Arc::clone(&slot), &inflight.table, key.clone());

        let computed = compute
            .take()
            .expect("compute is taken exactly once by the cold winner")();

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
                    retire_slot_if_current(&inflight.table, &key, &slot);
                    return None;
                }

                let entry_arc = Arc::new(entry);
                let value = project
                    .take()
                    .expect("project is taken exactly once by the cold winner")(
                    &entry_arc
                );
                map.insert(key.clone(), Arc::clone(&entry_arc));

                // post_publish: fires AFTER
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
        retire_slot_if_current(&inflight.table, &key, &slot);

        return value;
    }
}

/// Cooperative cold-compute admission with a first-class
/// `ComputeAdmission` outcome. Generalises
/// [`cooperative_get_or_insert_with_post_publish`] by lifting the
/// "valid-but-non-cacheable" case (overflowed fact signature, e.g.)
/// into the admission contract via [`ComputeAdmission::ReturnOnly`].
///
/// **Three-way outcome contract.**
///
/// - `Cacheable(Entry)` — insert into the map, call `post_publish`.
///   Joiners re-read the published entry and run `validate` against
///   their own view.
/// - `ReturnOnly(V)` — do NOT insert; do NOT call `post_publish`. A
///   `ReturnOnly` value carries no `Entry` and no dep-signature
///   carrier, so it cannot be view-validated against a joiner's own
///   view. The winner alone receives the `V`; joiners observe the
///   non-cacheable-winner flag and fork + cold-recompute for their own
///   view. The cache stays empty so the next cold-miss recomputes.
/// - `Failed` — mark the slot failed; joiners surface `None`.
///
/// **Joiner contract.** Joiners wake on the slot's condvar. If
/// `state.failed`, they return `None`. If `state.non_cacheable_winner`,
/// the winner emitted a carrier-less `ReturnOnly` outcome that cannot
/// be view-validated — the joiner forks and cold-recomputes. Otherwise
/// the winner inserted an entry into the map; the joiner re-reads the
/// map and runs `validate` against its own view (forking if the entry
/// is stale for that view).
///
/// `post_publish` is winner-only and fires exactly once. Joiners run
/// `validate`, never `post_publish`.
#[allow(clippy::too_many_arguments)]
pub fn cooperative_admit_with_post_publish<
    K,
    Entry,
    V,
    Validate,
    Compute,
    Project,
    Revalidate,
    RemovalCleanup,
    PostPublish,
>(
    map: &DashMap<K, Arc<Entry>>,
    inflight: &InflightTable<K>,
    key: K,
    mut validate: Validate,
    compute: Compute,
    project: Project,
    mut revalidate_after_compute: Revalidate,
    mut removal_cleanup: RemovalCleanup,
    post_publish: PostPublish,
) -> Option<V>
where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Validate: FnMut(&Entry) -> Option<V>,
    Compute: FnOnce() -> ComputeAdmission<V, Entry>,
    Project: FnOnce(&Entry) -> V,
    Revalidate: FnMut(&Entry) -> bool,
    RemovalCleanup: FnMut(&K, &Arc<Entry>),
    PostPublish: FnOnce(&Arc<Entry>, &K),
{
    // `compute` and `project` are logically one-shot; the joiner
    // re-validation loop carries them in `Option`s (see
    // `cooperative_get_or_insert_with_post_publish` for the rationale).
    let mut compute = Some(compute);
    let mut project = Some(project);
    loop {
        // Warm-hit + read-side validation. Same shape as the
        // `cooperative_get_or_insert_with_post_publish` warm path —
        // including the removal-side cleanup so the cache's live
        // counter / reverse index stay symmetric with publish.
        if let Some(entry_arc) = map.get(&key).map(|e| e.clone()) {
            if let Some(value) = validate(&entry_arc) {
                return Some(value);
            }
            remove_published_entry_with_cleanup(map, &key, &entry_arc, &mut removal_cleanup);
        }

        // Claim the inflight slot or join an in-progress build.
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
            // A `ReturnOnly` winner left the map empty and published no
            // dep-signature carrier. The joiner cannot view-validate a
            // carrier-less value against its own view — `ReturnOnly` is
            // non-shareable across joiners — so it forks and
            // cold-recomputes for its own view.
            let non_cacheable_winner = state.non_cacheable_winner;
            drop(state);
            if non_cacheable_winner {
                retire_slot_if_current(&inflight.table, &key, &slot);
                drop(slot);
                continue;
            }
            // Cacheable winner — re-read the published entry and run the
            // caller's `validate` closure against THIS follower's view
            // (NOT `project`). A follower joining this in-flight build is
            // not guaranteed to be running under the same view/overlay
            // as the winner. `validate` returning `Some` also performs
            // the caller's fact-bubble side effect.
            if let Some(entry_arc) = map.get(&key).map(|e| e.clone()) {
                if let Some(value) = validate(&entry_arc) {
                    return Some(value);
                }
                // The winner's entry is stale for the follower's view.
                // Fork: remove the stale entry AND run the cache's
                // removal-side cleanup so its live counter / reverse
                // index stay consistent, retire the slot, re-enter
                // admission so the follower cold-computes for its view.
                remove_published_entry_with_cleanup(map, &key, &entry_arc, &mut removal_cleanup);
            }
            retire_slot_if_current(&inflight.table, &key, &slot);
            drop(slot);
            continue;
        }
        state.claimed = true;
        drop(state);

        // Cold winner runs compute under a panic guard.
        let mut panic_guard =
            InflightPanicGuard::new(Arc::clone(&slot), &inflight.table, key.clone());

        let admission = compute
            .take()
            .expect("compute is taken exactly once by the cold winner")();

        let value = match admission {
            ComputeAdmission::Cacheable(entry) => {
                if !revalidate_after_compute(&entry) {
                    {
                        let mut state = slot.state.lock();
                        state.completed = true;
                        state.failed = true;
                    }
                    slot.ready.notify_all();
                    panic_guard.mark_finished();
                    drop(panic_guard);
                    retire_slot_if_current(&inflight.table, &key, &slot);
                    return None;
                }
                let entry_arc = Arc::new(entry);
                let value = project
                    .take()
                    .expect("project is taken exactly once by the cold winner")(
                    &entry_arc
                );
                map.insert(key.clone(), Arc::clone(&entry_arc));
                post_publish(&entry_arc, &key);
                {
                    let mut state = slot.state.lock();
                    state.completed = true;
                    // Cacheable winner: joiners fall through to the
                    // `map.get(&key) + validate(&entry_arc)` path so each
                    // joiner thread runs `validate` against its own view.
                    // `validate` both view-checks the entry and runs the
                    // caller's fact-bubble side effect (e.g.
                    // `entry.read_set_signature.bubble(ctx)` for the
                    // materialiser) on the joiner's own thread, delivering
                    // the cached entry's facts into the joiner's outer
                    // fact tracer.
                    //
                    // The map entry persists past slot retirement
                    // (`map.insert` above happens before the slot is
                    // removed from the inflight table), so a slow-waking
                    // joiner still observes the entry through `map.get`.
                }
                slot.ready.notify_all();
                Some(value)
            }
            ComputeAdmission::ReturnOnly(value) => {
                // Valid result but not cacheable. The map stays empty and
                // no dep-signature carrier is published, so a joiner has
                // nothing to view-validate against its own view. Mark the
                // slot `non_cacheable_winner` so every joiner forks and
                // cold-recomputes for its own view; the winner alone
                // receives `value`.
                {
                    let mut state = slot.state.lock();
                    state.completed = true;
                    state.non_cacheable_winner = true;
                }
                slot.ready.notify_all();
                Some(value)
            }
            ComputeAdmission::Failed => {
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
        // (Cacheable path) or start a fresh inflight (ReturnOnly /
        // Failed paths leave the map empty).
        retire_slot_if_current(&inflight.table, &key, &slot);

        return value;
    }
}
// ============================================================================
// D3.2 admission-control gating tests + joiner view-validation discriminators.
//
// Extracted to the sibling `cooperative_admission_tests.rs` (kept as a
// child `mod` via `#[path]` so the thread-coordinated discriminators
// reach the substrate's private `InflightSlot` / `InflightTable`
// internals). Splitting keeps this file under the file-size guard cap.
// ============================================================================

#[cfg(test)]
#[path = "cooperative_admission_tests.rs"]
mod tests;
