//! Cooperative cold-compute admission for multi-candidate query-identity
//! caches whose storage is a slot (not a `DashMap<MapK, _>`), with a
//! split publish lifecycle under a retention fence.
//!
//! Query-identity caches store their entries inside a multi-candidate
//! slot where each slot key holds several concurrent candidates. The
//! `cooperative_admit_with_post_publish_by_flight_key` shape cannot serve
//! those caches: its stale-rejection path runs `map.remove_if` on the
//! WHOLE slot, which would evict every OTHER candidate sharing that slot.
//! This adapter reuses the identical winner/joiner state machine (one
//! cold computer, cooperative joiner waits, panic safety, post-compute
//! revalidation, the three-way [`ComputeAdmission`] outcome) but reads
//! through `lookup` and admits through the SPLIT publish lifecycle, so
//! candidate-level stale rejection stays inside the slot.
//!
//! ## Split publish lifecycle (install-before-registration race closure)
//!
//! A query-identity cache with a per-canonical reverse index and a
//! retention budget cannot publish in one step: the candidate-visibility
//! barrier (the slot guard) must cover the install + counter bump +
//! reverse-index registration + retention-admission record together so no
//! reader observes a published candidate before its counter / index
//! registration exists (the install-before-registration race). But the
//! FIFO victim eviction
//! that an over-budget admission triggers RE-ENTERS the slot map and
//! reverse index, so it cannot run under that same guard without
//! self-deadlocking. The lifecycle therefore splits:
//!
//! ```text
//! fence.read
//!   → revalidate_after_compute
//!   → publish_core (under slot/shard guard: install/replace candidate,
//!                   bump counter, register reverse index, record
//!                   retention admission — NON-REENTRANT only;
//!                   returns deferred FIFO victims)
//!   → drop internal slot/shard guard
//!   → evict_deferred(victims)   (re-enters slot map / reverse index —
//!                                guard-free, still under the fence)
//!   → mark flight complete
//! fence drops
//! ```
//!
//! The retention read guard spans revalidation through deferred eviction,
//! so a project-generation `clear` (which holds the matching write guard
//! across its whole map+budget clear) cannot interleave between
//! `publish_core` and `evict_deferred` — `clear` atomicity is preserved.
//!
//! Closures:
//!
//! - `lookup` — read the slot for a candidate valid under the caller's
//!   own view. `Some(v)` is a warm hit (it also performs the caller's
//!   fact-bubble side effect); `None` falls through to the cold path.
//!   `FnMut` because the joiner re-runs it against its own view after
//!   waking onto a winner's publish.
//! - `compute` — the one-winner cold build, returning a three-way
//!   [`ComputeAdmission`].
//! - `project` — projects the caller-visible value from the freshly built
//!   `Entry` (winner-only, on the winner's thread).
//! - `revalidate_after_compute` — the winner-side publish-race closer;
//!   `false` skips the publish (a mutation invalidated the entry
//!   mid-compute). Runs under the fence read guard, atomically with the
//!   publish.
//! - `publish_core` — the non-reentrant publish step under the slot
//!   guard; returns `(slot outcome, deferred victims)`. Winner-only.
//! - `evict_deferred` — evicts the deferred FIFO victims after the slot
//!   guard drops. Winner-only.
//!
//! There is no `removal_cleanup`: this adapter never removes a candidate
//! on a stale read. A stale candidate is left for the slot's own
//! validation to reject (the next `lookup` simply skips it); the slot's
//! removal-side bookkeeping (counter, reverse index, ledger) is the
//! store's responsibility in its own eviction paths, never the
//! singleflight protocol's.
//!
//! `Entry` is moved into `publish_core` by value (the slot takes
//! ownership of the built candidate), so it is not wrapped in `Arc` here
//! — the slot decides how to store it.

use std::hash::Hash;
use std::sync::Arc;

use super::singleflight::{
    retire_slot_if_current, ComputeAdmission, InflightFailureKind, InflightPanicGuard,
    InflightSlot, InflightTable,
};

#[cfg(test)]
thread_local! {
    /// Test-only rendezvous: a hook fired by the cold winner AFTER
    /// `publish_core` returns its deferred FIFO victims but BEFORE
    /// `evict_deferred` removes them — i.e. inside the `publish_fence`
    /// read-guard region, at the exact gap where a project-generation
    /// `clear` must not be able to interleave between the core publish and
    /// the deferred eviction.
    ///
    /// A race test installs a hook that parks the winner there, runs a
    /// concurrent `clear` on another thread, and asserts the `clear`'s
    /// `retention_gate.write()` is blocked (the winner already holds the
    /// read guard). It is a deterministic rendezvous — the hook IS the
    /// synchronisation point — not a timing sleep. The hook is
    /// thread-local so it only affects the installing test's winner
    /// thread; production fires nothing.
    static POST_PUBLISH_CORE_PRE_EVICT_HOOK: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: install a hook fired by the cold winner between
/// `publish_core` and `evict_deferred`, inside the `publish_fence`
/// read-guard region. Returns a guard that clears the hook on drop.
#[cfg(test)]
pub(crate) fn install_post_publish_core_pre_evict_hook(
    hook: Box<dyn Fn()>,
) -> PostPublishCorePreEvictHookGuard {
    POST_PUBLISH_CORE_PRE_EVICT_HOOK.with(|h| *h.borrow_mut() = Some(hook));
    PostPublishCorePreEvictHookGuard
}

/// RAII guard that clears the thread-local post-publish-core pre-evict
/// hook.
#[cfg(test)]
pub(crate) struct PostPublishCorePreEvictHookGuard;

#[cfg(test)]
impl Drop for PostPublishCorePreEvictHookGuard {
    fn drop(&mut self) {
        POST_PUBLISH_CORE_PRE_EVICT_HOOK.with(|h| *h.borrow_mut() = None);
    }
}

/// Fire the test-only post-publish-core / pre-evict rendezvous hook (a
/// no-op in production — the thread-local is always `None` unless a test
/// installed one).
#[inline]
fn fire_post_publish_core_pre_evict_hook() {
    #[cfg(test)]
    POST_PUBLISH_CORE_PRE_EVICT_HOOK.with(|h| {
        if let Some(hook) = h.borrow().as_ref() {
            hook();
        }
    });
}

/// Cooperative cold-compute admission over a multi-candidate slot store,
/// with the split publish lifecycle under a retention fence. See the
/// module docs for the full contract.
///
/// `project_unadmitted` is the winner-side projection for a freshly
/// computed `Cacheable` entry whose admission was REFUSED by
/// `revalidate_after_compute`. `Some(value)` returns the COMPUTED value
/// to the winner as a non-cacheable `ReturnOnly`-style outcome (no
/// publish; joiners fork and cold-recompute for their own view) — the
/// honest shape: the winner computed a complete value and substituting
/// anything else would make the caller-visible result a function of
/// admission timing. `None` keeps failure semantics for the winner
/// (`AdmissionRejected`; the winner returns `None` and its joiners fork).
///
/// `allow(dead_code)`: the query-identity `query::lookup` entry point
/// lowers here; it is exercised by the `cache_runtime` tests and is the
/// substrate the query-identity cache families route through.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn cooperative_admit_with_lookup_publish<
    FlightK,
    Entry,
    V,
    Victims,
    Lookup,
    Compute,
    Project,
    ProjectUnadmitted,
    Revalidate,
    PublishCore,
    EvictDeferred,
>(
    inflight: &InflightTable<FlightK>,
    flight_key: FlightK,
    mut lookup: Lookup,
    compute: Compute,
    project: Project,
    project_unadmitted: ProjectUnadmitted,
    mut revalidate_after_compute: Revalidate,
    publish_core: PublishCore,
    evict_deferred: EvictDeferred,
    publish_fence: Option<&parking_lot::RwLock<()>>,
) -> Option<V>
where
    FlightK: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    V: Clone,
    Lookup: FnMut() -> Option<V>,
    Compute: FnOnce() -> ComputeAdmission<V, Entry>,
    Project: FnOnce(&Entry) -> V,
    ProjectUnadmitted: FnOnce(&Entry) -> Option<V>,
    Revalidate: FnMut(&Entry) -> bool,
    PublishCore: FnOnce(Entry) -> Victims,
    EvictDeferred: FnOnce(Victims),
{
    // `compute`, `project`, `publish_core`, and `evict_deferred` are
    // logically one-shot but the joiner re-validation loop may iterate.
    // Carry them in `Option`s and `take()` them only on the iteration
    // where THIS caller becomes the cold winner.
    let mut compute = Some(compute);
    let mut project = Some(project);
    let mut project_unadmitted = Some(project_unadmitted);
    let mut publish_core = Some(publish_core);
    let mut evict_deferred = Some(evict_deferred);
    loop {
        // Warm-hit read-side validation through the slot's own lookup.
        if let Some(value) = lookup() {
            return Some(value);
        }

        // Claim the inflight slot or join an in-progress build.
        let slot = {
            let mut table = inflight.table.lock();
            table
                .entry(flight_key.clone())
                .or_insert_with(|| Arc::new(InflightSlot::new()))
                .clone()
        };

        let mut state = slot.state.lock();
        if state.claimed {
            // Joiner — wait for the winner to publish or fail.
            slot.ready.wait_while(&mut state, |s| !s.completed);
            match state.failure {
                // Deterministic compute failure — surface `None` (the
                // caller's fallback owns retry policy).
                Some(InflightFailureKind::ComputeFailed) => return None,
                // The winner produced NO completeness evidence (panic),
                // or computed a value its own view could not admit
                // (admission rejection). Neither outcome is consumable by
                // this joiner as a value — fork and cold-recompute for
                // this joiner's OWN view, exactly like the cross-view
                // stale-reject fork below. A panicked winner's bug
                // surfaces as the joiner's own panic on recompute, never
                // as a fabricated value-bearing result.
                Some(InflightFailureKind::WinnerPanicked)
                | Some(InflightFailureKind::AdmissionRejected) => {
                    drop(state);
                    retire_slot_if_current(&inflight.table, &flight_key, &slot);
                    drop(slot);
                    continue;
                }
                None => {}
            }
            // A `ReturnOnly` winner published nothing the joiner can
            // view-validate against its own view, so it forks and
            // cold-recomputes.
            let non_cacheable_winner = state.non_cacheable_winner;
            drop(state);
            if non_cacheable_winner {
                retire_slot_if_current(&inflight.table, &flight_key, &slot);
                drop(slot);
                continue;
            }
            // Cacheable winner — re-read the slot via `lookup` against
            // THIS follower's view. A hit returns the validated value (and
            // bubbles facts); a miss means the winner's candidate is stale
            // for the follower's view, so the follower forks and
            // cold-recomputes. The slot keeps the winner's candidate for
            // other views; this adapter never evicts it.
            if let Some(value) = lookup() {
                return Some(value);
            }
            retire_slot_if_current(&inflight.table, &flight_key, &slot);
            drop(slot);
            continue;
        }
        state.claimed = true;
        drop(state);

        // Cold winner runs compute under a panic guard.
        let mut panic_guard =
            InflightPanicGuard::new(Arc::clone(&slot), &inflight.table, flight_key.clone());

        let admission = compute
            .take()
            .expect("compute is taken exactly once by the cold winner")();

        let value = match admission {
            ComputeAdmission::Cacheable(entry) => {
                // Hold the cache's retention gate (shared read) across the
                // WHOLE publish lifecycle — `revalidate_after_compute`, the
                // non-reentrant `publish_core`, AND the `evict_deferred`.
                // The revalidation is the last generation / dep-signature
                // check before the candidate lands, so it must be atomic
                // with the publish; and `clear` (write guard) must not be
                // able to interleave between `publish_core` and
                // `evict_deferred`. A cache with no retention budget passes
                // `None` — the lifecycle then runs unfenced.
                let _retention = publish_fence.map(parking_lot::RwLock::read);
                // Post-compute revalidation BEFORE admitting the candidate.
                // A mutation landing in the cold window rejects the entry
                // here and no candidate is published. The COMPUTED value is
                // still the winner's honest result: when the caller opts in
                // (`project_unadmitted` returns `Some`), return it
                // `ReturnOnly`-style — non-cacheable, nothing published,
                // joiners fork — instead of discarding it (a discarded
                // complete value forces the caller to fabricate a
                // substitute, making the caller-visible result a function
                // of admission timing). A `None` opt-out keeps failure
                // semantics (`AdmissionRejected`; joiners fork).
                if !revalidate_after_compute(&entry) {
                    let unadmitted = project_unadmitted
                        .take()
                        .expect("project_unadmitted is taken exactly once by the cold winner")(
                        &entry,
                    );
                    {
                        let mut state = slot.state.lock();
                        state.completed = true;
                        if unadmitted.is_some() {
                            state.non_cacheable_winner = true;
                        } else {
                            state.failure = Some(InflightFailureKind::AdmissionRejected);
                        }
                    }
                    slot.ready.notify_all();
                    panic_guard.mark_finished();
                    drop(panic_guard);
                    retire_slot_if_current(&inflight.table, &flight_key, &slot);
                    return unadmitted;
                }
                let value = project
                    .take()
                    .expect("project is taken exactly once by the cold winner")(
                    &entry
                );
                // Non-reentrant publish step under the store's internal
                // slot/shard guard: install/replace the candidate, bump the
                // counter, register the reverse index, record the retention
                // admission. Returns the FIFO victims for deferred eviction.
                // No reader can observe the published candidate before its
                // counter / reverse-index registration exists (the
                // install-before-registration race closure).
                let victims = publish_core
                    .take()
                    .expect("publish_core is taken exactly once by the cold winner")(
                    entry
                );
                // Test-only rendezvous fired inside the `publish_fence`
                // read-guard region, AFTER `publish_core` and BEFORE
                // `evict_deferred` — the exact gap a project-generation
                // `clear` must not be able to interleave. Production fires
                // nothing.
                fire_post_publish_core_pre_evict_hook();
                // Deferred eviction of the FIFO victims. Runs AFTER the
                // internal slot/shard guard has dropped (still under the
                // retention read guard) so a budgeted eviction that
                // re-enters the slot map / reverse index cannot
                // self-deadlock on the publish-core guard.
                evict_deferred
                    .take()
                    .expect("evict_deferred is taken exactly once by the cold winner")(
                    victims
                );
                {
                    let mut state = slot.state.lock();
                    state.completed = true;
                    // Joiners fall through to the `lookup()` path and
                    // validate the published candidate against their own
                    // view.
                }
                slot.ready.notify_all();
                Some(value)
            }
            ComputeAdmission::ReturnOnly { value, reason } => {
                crate::cache_runtime::admission::propagate_non_admission(reason);
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
                    state.failure = Some(InflightFailureKind::ComputeFailed);
                }
                slot.ready.notify_all();
                None
            }
        };

        panic_guard.mark_finished();
        drop(panic_guard);
        retire_slot_if_current(&inflight.table, &flight_key, &slot);
        return value;
    }
}

#[cfg(test)]
#[path = "lookup_publish_tests.rs"]
mod tests;
