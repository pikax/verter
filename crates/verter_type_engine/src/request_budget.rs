//! Request-scoped projection budget.
//!
//! This module owns the per-request projection-operation fuse used by
//! component-meta entry points and semantic dispatch. The budget itself
//! is stored on [`crate::request_context::RequestContext`], so scheduler
//! worker propagation uses the same request-context TLS bridge as audit
//! and cache counters.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use rustc_hash::FxHashSet;

use crate::project_semantic_dispatch::cost_receipt::CostIdentity;

/// Request-scoped projection-operation fuse.
///
/// Tracks the per-request aggregate work-op count used to terminate
/// utility / projection / generic-expansion recursion before the call
/// stack exhausts. The cap is constructor-time on
/// `HostConfig::projection_op_budget`; a value of `0` preserves the
/// legacy default of 2000.
///
/// The set of `SemanticQueryKey` kinds that count is the aggregate
/// work-budget gate
/// (`project_semantic_dispatch::semantic_query_counts_toward_projection_budget`):
/// the projection operators (`ProjectMember` / `IndexedAccess` /
/// `ProjectPath` / `KeyOf` / `MappedType`) PLUS `Instantiate` and
/// `Conditional` — the kinds that dominate an open-generic expansion
/// storm. Counting only projection operators left instantiation /
/// conditional storms unbounded; the aggregate gate makes them fail
/// closed too.
///
/// # Default rationale
///
/// The default `2000` is a **fuse threshold**, not a correctness
/// boundary. It is sized so legitimate component-meta resolutions on
/// representative corpora (nuxt-ui, element-plus, primevue, etc.)
/// complete well under the cap with substantial headroom, while
/// pathological projections — recursive `Pick<...>` chains over
/// untyped barrels, deep `Surface[K1][K2]...[Kn]` walks with missing
/// types, generic-helper instantiation storms — exhaust within a
/// few seconds and surface a partial.
///
/// **Semantic contract on exhaustion**: a request that trips the cap
/// returns a *partial* `ComponentMeta` with the same structural
/// invariants as a complete one (well-formed `props` / `emits` /
/// `slots` / `exposed` lists, opaque sentinels for unresolved members) — NOT a
/// malformed payload. The dispatch return carries
/// `cache_suppress=true`, which propagates through the
/// reducer/materializer pipeline (see
/// `MaterializedOutputTypeExpr.cache_suppress` and
/// `RequestContext::request_result_is_partial`) into
/// `ResolvedComponentMetaState.synthesis_should_suppress`. The
/// `ComponentMetaResultDb` admission gate refuses to warm the
/// partial, so a subsequent identical request re-runs the cold
/// compute against fresh budget rather than warm-hitting a
/// poisoned entry.
///
/// **Raising the cap is safe** for users who profile-confirm a
/// legitimate request needs more headroom; lowering it is a way to
/// surface partials earlier on known-pathological inputs. Either
/// direction preserves correctness — the cap only controls when the
/// reducer bails to a partial, not whether the partial is admitted
/// to caches.
///
/// **Warm reads pay too.** A stored result's receipt carries the
/// operations its computation spent; serving it charges every computation
/// in its closure this request has not yet paid
/// ([`Self::admit_replayed_operations`]), so a request trips the fuse at
/// the same point whether its sub-results were computed now, earlier in
/// the request, or by an earlier request.
#[derive(Debug)]
pub struct RequestBudget {
    /// Projection-operation budget for the request.
    pub projection_op_budget: usize,
    projection_ops_executed: AtomicUsize,
    /// The computations whose operations this request has paid: computed
    /// in it, or replayed into it.
    operations_paid: Mutex<PaidOperations>,
}

/// The computations a request has paid the operations of, in the order it
/// paid them, with an order-independent digest of the set: a demand's entry
/// state is read in constant time, and the set it entered at is the prefix
/// of that order.
#[derive(Debug, Default)]
pub(crate) struct PaidOperations {
    set: FxHashSet<CostIdentity>,
    order: Vec<CostIdentity>,
    digest: u64,
}

impl PaidOperations {
    pub(crate) fn contains(&self, identity: &CostIdentity) -> bool {
        self.set.contains(identity)
    }

    /// Add `identity`: `true` when it was not paid yet.
    pub(crate) fn insert(&mut self, identity: CostIdentity) -> bool {
        let hash = identity.hash_value();
        if self.set.insert(identity.clone()) {
            self.order.push(identity);
            self.digest = self.digest.wrapping_add(spread(hash));
            true
        } else {
            false
        }
    }

    /// Add `identities`: how many were not paid yet.
    pub(crate) fn extend(&mut self, identities: impl IntoIterator<Item = CostIdentity>) -> usize {
        identities
            .into_iter()
            .filter(|identity| self.insert(identity.clone()))
            .count()
    }
}

/// Mix one identity hash before it joins the set digest, so the digest of
/// a set is not the plain sum of its members' hashes.
fn spread(hash: u64) -> u64 {
    xxhash_rust::xxh3::xxh3_64(&hash.to_le_bytes())
}

/// What a request had spent when a demand entered it: the operations, and
/// the computations whose operations it had paid. Two demands entered at
/// equal states are charged identically from there on.
#[derive(Debug, Clone)]
pub struct RequestEntryState {
    operations: usize,
    paid_len: usize,
    paid_digest: u64,
}

/// What one evaluation did to a request it ran in alone: the state it
/// entered the request at, and its own effects from there — the operations
/// it spent and the computations it paid.
#[derive(Debug, Clone)]
pub struct RequestSpent {
    pub(crate) entry: RequestEntryState,
    /// The computations paid when the evaluation entered.
    pub(crate) entry_paid: Box<[CostIdentity]>,
    /// The operations the evaluation spent.
    pub(crate) operations: usize,
    /// The computations the evaluation paid, in the order it paid them.
    pub(crate) paid: Box<[CostIdentity]>,
}

impl RequestSpent {
    /// The computations it names, entered at and paid.
    pub(crate) fn identities(&self) -> usize {
        self.entry_paid.len() + self.paid.len()
    }
}

impl RequestBudget {
    /// Construct a new per-request budget with a zeroed counter and the
    /// supplied cap.
    #[must_use]
    pub fn new(projection_op_budget: usize) -> Arc<Self> {
        Arc::new(Self {
            projection_op_budget,
            projection_ops_executed: AtomicUsize::new(0),
            operations_paid: Mutex::new(PaidOperations::default()),
        })
    }

    /// The computations whose operations this request has paid, held for
    /// one replay's admission.
    pub(crate) fn operations_paid(&self) -> MutexGuard<'_, PaidOperations> {
        self.operations_paid.lock()
    }

    /// Record `identities` as paid: their computations ran in this request.
    /// Returns how many were not paid yet.
    pub(crate) fn mark_operations_paid(
        &self,
        identities: impl IntoIterator<Item = CostIdentity>,
    ) -> usize {
        self.operations_paid.lock().extend(identities)
    }

    /// Admit a replay that owes `operations` for the computations `unpaid`:
    /// `true`, with the operations spent and the computations paid, when
    /// the request has room for all of them; `false`, charging and marking
    /// nothing, when it does not. `paid` is the guard the replay read the
    /// paid set under, so the decision and the marking are one step.
    pub(crate) fn admit_replayed_operations(
        &self,
        operations: u64,
        mut paid: MutexGuard<'_, PaidOperations>,
        unpaid: Vec<CostIdentity>,
    ) -> bool {
        let operations = usize::try_from(operations).unwrap_or(usize::MAX);
        if operations > 0 {
            let cap = self.effective_projection_op_budget();
            let admitted = self.projection_ops_executed.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |executed| {
                    executed
                        .checked_add(operations)
                        .filter(|total| *total <= cap)
                },
            );
            if admitted.is_err() {
                return false;
            }
        }
        paid.extend(unpaid);
        true
    }

    /// The request's state now, as a demand entering it would see it.
    pub(crate) fn entry_state(&self) -> RequestEntryState {
        let paid = self.operations_paid.lock();
        RequestEntryState {
            operations: self.projection_ops_executed.load(Ordering::Relaxed),
            paid_len: paid.set.len(),
            paid_digest: paid.digest,
        }
    }

    /// What the evaluation that entered at `entry` and itself did `effects`
    /// left the request at — `None` unless the request is exactly `entry`
    /// plus those effects: anything else that spent in the request while
    /// the evaluation ran (a sibling root, a worker of the same request)
    /// makes where the evaluation stopped depend on it, so it is not the
    /// evaluation's alone.
    pub(crate) fn spent_alone_since(
        &self,
        entry: RequestEntryState,
        effects: crate::project_semantic_dispatch::connected_demand::RequestEffects,
    ) -> Option<RequestSpent> {
        let paid = self.operations_paid.lock();
        let alone = self.projection_ops_executed.load(Ordering::Relaxed)
            == entry.operations.checked_add(effects.operations)?
            && paid.order.len() == entry.paid_len.checked_add(effects.paid)?;
        alone.then(|| RequestSpent {
            entry_paid: paid.order[..entry.paid_len].iter().cloned().collect(),
            paid: paid.order[entry.paid_len..].iter().cloned().collect(),
            entry,
            operations: effects.operations,
        })
    }

    /// Leave this request as `spent`'s evaluation left it — its operations
    /// spent and its computations paid — if the request is exactly where
    /// that evaluation entered it: the same operations spent and the same
    /// computations paid. The check and the application are one step under
    /// the paid set's lock, with the operation counter moved by a
    /// compare-exchange from the entered value, so no spending between them
    /// can be absorbed: `false`, changing nothing, when the request is
    /// anywhere else. The digest rejects in constant time; equal digests
    /// compare the sets in full.
    pub(crate) fn apply_refusal(&self, spent: &RequestSpent) -> bool {
        let entry = &spent.entry;
        let mut paid = self.operations_paid.lock();
        if paid.set.len() != entry.paid_len
            || paid.digest != entry.paid_digest
            || spent.entry_paid.len() != entry.paid_len
            || !spent
                .entry_paid
                .iter()
                .all(|identity| paid.contains(identity))
        {
            return false;
        }
        let Some(left) = entry.operations.checked_add(spent.operations) else {
            return false;
        };
        if self
            .projection_ops_executed
            .compare_exchange(entry.operations, left, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        paid.extend(spent.paid.iter().cloned());
        true
    }

    /// Spend the rest of the request's operations: a replay this request
    /// could not pay for ends it as the operation that passed the cap
    /// would have, so every later entry sees the fuse tripped. Returns the
    /// operations it spent.
    pub(crate) fn exhaust(&self) -> usize {
        let tripped = self.effective_projection_op_budget().saturating_add(1);
        let before = self
            .projection_ops_executed
            .fetch_max(tripped, Ordering::Relaxed);
        tripped.saturating_sub(before)
    }

    /// Increment the projection-op counter and return `true` when the
    /// request has exceeded its cap.
    pub fn check_projection_op_count(&self) -> bool {
        let current = self
            .projection_ops_executed
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        current > self.effective_projection_op_budget()
    }

    /// Return the configured cap after applying the legacy default.
    #[must_use]
    pub fn effective_projection_op_budget(&self) -> usize {
        if self.projection_op_budget == 0 {
            2000
        } else {
            self.projection_op_budget
        }
    }

    /// Read-only view of the executed projection-op counter.
    #[must_use]
    pub fn projection_ops_executed_count(&self) -> usize {
        self.projection_ops_executed.load(Ordering::Relaxed)
    }

    /// Peek-only test for budget exhaustion. Returns `true` when the
    /// already-executed projection-op count strictly exceeds the cap,
    /// i.e. when a fresh [`Self::check_projection_op_count`] would also
    /// return `true` *without* the prior incrementing call having been
    /// the one to trip the fuse.
    ///
    /// The dispatcher's `execute_via_cold_build_helper` consults this
    /// peek BEFORE entering the cooperative-admission machinery so that,
    /// once a request trips its fuse, every subsequent projection-op
    /// query short-circuits at the dispatch entry without paying the
    /// `execute_cooperative` admission cost (in-flight table mutex,
    /// joiner-condvar entry, fact-tracer install, per-key warm probe).
    /// Without this gate a runaway request keeps spending μs-per-call
    /// on admission overhead for each rejected MappedType / KeyOf /
    /// ProjectPath dispatch — the empirically-observed 250K rejected
    /// builds on `ChatMessages.vue` translate to ~250 wall-clock
    /// seconds of materialisation-lane time spent past the fuse trip,
    /// none of which makes progress because every call returns
    /// `BudgetExceeded(cache_suppress=true)`.
    ///
    /// Non-incrementing on purpose: the cooperative-admission build
    /// closure remains the single site that bumps the executed
    /// counter via [`Self::check_projection_op_count`], so the trip
    /// point and the reported `BudgetExceededFailure.actual` value
    /// stay invariant across this fast-path peek.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.projection_ops_executed_count() > self.effective_projection_op_budget()
    }
}

#[cfg(test)]
mod tests {
    use super::RequestBudget;

    #[test]
    fn request_budget_check_increments_until_cap_then_returns_true() {
        let budget = RequestBudget::new(3);
        assert!(!budget.check_projection_op_count(), "1st call (1 of 3)");
        assert!(!budget.check_projection_op_count(), "2nd call (2 of 3)");
        assert!(!budget.check_projection_op_count(), "3rd call (3 of 3)");
        assert!(budget.check_projection_op_count(), "4th call exceeds 3");
        assert_eq!(
            budget.projection_ops_executed_count(),
            4,
            "counter must persist past the trip; the trip should not silently reset"
        );
    }

    #[test]
    fn request_budget_zero_cap_falls_back_to_default_2000() {
        let budget = RequestBudget::new(0);
        for _ in 0..1999 {
            assert!(!budget.check_projection_op_count());
        }
        assert!(!budget.check_projection_op_count(), "2000th call at cap");
        assert!(
            budget.check_projection_op_count(),
            "2001st call exceeds default"
        );
    }

    #[test]
    fn request_budget_is_exhausted_tracks_post_trip_state_without_incrementing() {
        let budget = RequestBudget::new(2);
        assert!(
            !budget.is_exhausted(),
            "fresh budget reports !exhausted before any call"
        );
        assert!(!budget.check_projection_op_count(), "1st call (1 of 2)");
        assert!(
            !budget.is_exhausted(),
            "within-budget call leaves !exhausted"
        );
        assert!(!budget.check_projection_op_count(), "2nd call (2 of 2)");
        assert!(
            !budget.is_exhausted(),
            "at-cap call leaves !exhausted (the cap is inclusive)"
        );
        assert!(budget.check_projection_op_count(), "3rd call exceeds 2");
        assert!(
            budget.is_exhausted(),
            "post-trip peek reports exhausted so the dispatcher early-exits"
        );
        // Crucial property: the peek must NOT increment. Without this
        // invariant the dispatcher's fast-path early-exit would inflate
        // `BudgetExceededFailure.actual` past the production value and
        // skew the per-request audit.
        let executed_before = budget.projection_ops_executed_count();
        assert!(budget.is_exhausted());
        assert!(budget.is_exhausted());
        assert!(budget.is_exhausted());
        assert_eq!(
            budget.projection_ops_executed_count(),
            executed_before,
            "is_exhausted is peek-only — it must not bump the executed counter"
        );
    }
}
