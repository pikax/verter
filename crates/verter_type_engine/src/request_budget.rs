//! Request-scoped projection budget.
//!
//! This module owns the per-request projection-operation fuse used by
//! component-meta entry points and semantic dispatch. The budget itself
//! is stored on [`crate::request_context::RequestContext`], so scheduler
//! worker propagation uses the same request-context TLS bridge as audit
//! and cache counters.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use crate::project_semantic_dispatch::cost_receipt::{CostIdentity, CostIdentitySet};

/// Request-scoped projection-operation fuse.
///
/// Tracks the per-request aggregate work-op count used to terminate
/// utility / projection / generic-expansion recursion before the call
/// stack exhausts. The cap is constructor-time on
/// `HostConfig::projection_op_budget`; a value of `0` preserves the
/// default, [`DEFAULT_PROJECTION_OP_BUDGET`].
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
/// The default is a **fuse threshold**, not a correctness boundary: the
/// checker's own instantiation allowance
/// ([`checker_policy::INSTANTIATION_COUNT`]). One operation is one counted
/// query computed, coarser than the checker's instantiation: a distributive
/// conditional repeating an `infer` costs 3 operations per union member
/// where the checker instantiates 7, and a recursive conditional over an
/// `as const` route tree 13 per parent where the checker instantiates 219.
/// So a finite computation the checker answers within its allowance does
/// not trip the fuse, and a smaller allowance would refuse where the
/// checker answers. Pathological projections — recursive `Pick<...>` chains over
/// untyped barrels, deep `Surface[K1][K2]...[Kn]` walks with missing
/// types, generic-helper instantiation storms — still exhaust the finite
/// allowance and surface a partial.
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
///
/// [`checker_policy::INSTANTIATION_COUNT`]: crate::semantic_query::checker_policy::INSTANTIATION_COUNT
#[derive(Debug)]
pub struct RequestBudget {
    /// Projection-operation budget for the request.
    pub projection_op_budget: usize,
    projection_ops_executed: AtomicUsize,
    /// The computations whose operations this request has paid: computed
    /// in it, or replayed into it.
    operations_paid: Mutex<PaidOperations>,
}

/// Default finite headroom for projection operations, shared by host
/// configuration and the zero-config fallback: the checker's instantiation
/// allowance (see [`RequestBudget`]'s default rationale).
pub const DEFAULT_PROJECTION_OP_BUDGET: usize =
    crate::semantic_query::checker_policy::INSTANTIATION_COUNT as usize;

/// The computations a request has paid the operations of, in the order it
/// paid them, with an order-independent digest of the set: a demand's entry
/// state is read in constant time, and the set it entered at is the prefix
/// of that order.
#[derive(Debug, Default)]
pub(crate) struct PaidOperations {
    /// The paid computations, in the order they were paid.
    set: CostIdentitySet,
    digest: u64,
}

impl PaidOperations {
    pub(crate) fn contains(&self, identity: &CostIdentity) -> bool {
        self.set.contains(identity)
    }

    /// Add `identity`: `true` when it was not paid yet.
    pub(crate) fn insert(&mut self, identity: CostIdentity) -> bool {
        let hash = identity.hash_value();
        if !self.set.insert(identity) {
            return false;
        }
        self.digest = self.digest.wrapping_add(spread(hash));
        if let Some(paid) = self.set.members().last() {
            attribute_paid(paid);
        }
        true
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

impl RequestEntryState {
    /// How many computations were already paid when the demand entered.
    #[must_use]
    pub(crate) fn paid_len(&self) -> usize {
        self.paid_len
    }

    /// Order-independent digest of the paid set at entry.
    #[must_use]
    pub(crate) fn paid_digest(&self) -> u64 {
        self.paid_digest
    }

    /// Projection operations already spent when the demand entered.
    #[must_use]
    pub(crate) fn operations(&self) -> usize {
        self.operations
    }
}

/// The request state a component-meta compute entered at, including the
/// paid-set order. A later observation accepts a spend only when that
/// order is still the prefix: an insertion ahead of it means the spend
/// is not this compute's.
#[derive(Clone)]
pub(crate) struct RequestEntrySnapshot {
    state: RequestEntryState,
    paid: Box<[CostIdentity]>,
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
    /// The bytes a retained copy of this spend keeps: both paid lists'
    /// backings and every identity they name, once per `aliases`. A paid
    /// identity can outlive the receipt that minted it, so a retained
    /// spend counts what it keeps reachable.
    pub(crate) fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use verter_session_query::retention::allocation::heap_block_bytes;
        let list =
            |identities: &[CostIdentity],
             aliases: &mut verter_session_query::retention::allocation::AliasSet| {
                heap_block_bytes(std::mem::size_of_val(identities))
                    + identities
                        .iter()
                        .map(|identity| identity.owned_heap_bytes(aliases))
                        .sum::<usize>()
            };
        list(&self.entry_paid, aliases) + list(&self.paid, aliases)
    }

    /// A spend that names these identities and nothing else. Retention tests
    /// use it to vary the stored prefix without a live request.
    #[cfg(test)]
    pub(crate) fn from_identities_for_test(
        entry_paid: Vec<CostIdentity>,
        paid: Vec<CostIdentity>,
    ) -> Self {
        Self {
            entry: RequestEntryState {
                operations: 0,
                paid_len: entry_paid.len(),
                paid_digest: 0,
            },
            entry_paid: entry_paid.into_boxed_slice(),
            operations: 0,
            paid: paid.into_boxed_slice(),
        }
    }
}

/// Operations and paid identities recorded on this thread while a
/// component-meta compute owns the stack. Admission reads the copy taken
/// at the end of that compute, not this stack.
#[derive(Default)]
struct ThreadSpendLog {
    operations: usize,
    paid: Vec<CostIdentity>,
}

thread_local! {
    static THREAD_SPEND_LOGS: RefCell<Vec<Rc<RefCell<ThreadSpendLog>>>> =
        const { RefCell::new(Vec::new()) };
}

/// Pushes a spend log for the current thread. Nested computes each keep
/// their own log, and a spend is recorded on every log still on the stack
/// so an outer compute includes the inner one's request effects.
pub(crate) struct ThreadSpendGuard {
    log: Rc<RefCell<ThreadSpendLog>>,
}

impl ThreadSpendGuard {
    pub(crate) fn enter() -> Self {
        let log = Rc::new(RefCell::new(ThreadSpendLog::default()));
        THREAD_SPEND_LOGS.with(|logs| logs.borrow_mut().push(Rc::clone(&log)));
        Self { log }
    }

    pub(crate) fn finish(self) -> (usize, Vec<CostIdentity>) {
        let out = {
            let log = self.log.borrow();
            (log.operations, log.paid.clone())
        };
        drop(self);
        out
    }
}

impl Drop for ThreadSpendGuard {
    fn drop(&mut self) {
        THREAD_SPEND_LOGS.with(|logs| {
            logs.borrow_mut().retain(|log| !Rc::ptr_eq(log, &self.log));
        });
    }
}

/// Capture the request operations and paid computations `body` spends, as
/// a component-meta compute's spend log records them.
#[cfg(any(test, feature = "test-support"))]
pub fn with_request_spend_capture_for_tests<R>(body: impl FnOnce() -> R) -> (R, usize, usize) {
    let guard = ThreadSpendGuard::enter();
    let result = body();
    let (operations, paid) = guard.finish();
    (result, operations, paid.len())
}

/// The request spend of a semantic task, which may be polled on a thread
/// other than the one that lent it its request: one log standing for every
/// log on the stack where it was lent (none when none was). Each poll
/// installs it as the polling thread's only log, and the task's return
/// records what it spent on the logs it was lent under.
#[derive(Default)]
pub(crate) struct TaskSpendCarry {
    open: bool,
    operations: usize,
    paid: Vec<CostIdentity>,
}

impl TaskSpendCarry {
    /// The carry of a task lent here: open exactly when a log is.
    pub(crate) fn lend() -> Self {
        Self {
            open: THREAD_SPEND_LOGS.with(|logs| !logs.borrow().is_empty()),
            operations: 0,
            paid: Vec::new(),
        }
    }

    /// Install the task's log on this thread for one poll; the returned
    /// guard restores the thread's own logs and takes the task's back.
    pub(crate) fn install(&mut self) -> TaskSpendInstall<'_> {
        let task = if self.open {
            vec![Rc::new(RefCell::new(ThreadSpendLog {
                operations: std::mem::take(&mut self.operations),
                paid: std::mem::take(&mut self.paid),
            }))]
        } else {
            Vec::new()
        };
        let thread = THREAD_SPEND_LOGS.with(|logs| logs.replace(task));
        TaskSpendInstall {
            carry: self,
            thread,
        }
    }

    /// Record what the task spent on every log on this thread's stack, the
    /// one that lent it, in the order it spent it.
    pub(crate) fn return_to_lender(self) {
        attribute_operations(self.operations);
        for identity in &self.paid {
            attribute_paid(identity);
        }
    }
}

/// A task's spend log installed for one poll ([`TaskSpendCarry::install`]).
pub(crate) struct TaskSpendInstall<'c> {
    carry: &'c mut TaskSpendCarry,
    thread: Vec<Rc<RefCell<ThreadSpendLog>>>,
}

impl Drop for TaskSpendInstall<'_> {
    fn drop(&mut self) {
        let task = THREAD_SPEND_LOGS.with(|logs| logs.replace(std::mem::take(&mut self.thread)));
        if let Some(log) = task.first() {
            let mut log = log.borrow_mut();
            self.carry.operations = std::mem::take(&mut log.operations);
            self.carry.paid = std::mem::take(&mut log.paid);
        }
    }
}

fn attribute_operations(n: usize) {
    if n == 0 {
        return;
    }
    THREAD_SPEND_LOGS.with(|logs| {
        for log in logs.borrow().iter() {
            let mut slot = log.borrow_mut();
            slot.operations = slot.operations.saturating_add(n);
        }
    });
}

fn attribute_paid(identity: &CostIdentity) {
    THREAD_SPEND_LOGS.with(|logs| {
        for log in logs.borrow().iter() {
            log.borrow_mut().paid.push(identity.clone());
        }
    });
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
        attribute_operations(operations);
        true
    }

    /// The request's state and paid-set order, for a compute that will
    /// later ask what it spent.
    pub(crate) fn capture_entry(&self) -> RequestEntrySnapshot {
        let paid = self.operations_paid.lock();
        RequestEntrySnapshot {
            state: RequestEntryState {
                operations: self.projection_ops_executed.load(Ordering::Relaxed),
                paid_len: paid.set.len(),
                paid_digest: paid.digest,
            },
            paid: paid.set.members().to_vec().into_boxed_slice(),
        }
    }

    /// What changed since `snapshot` when the paid-set order still begins
    /// with the order captured there. Direct projection-op checks count:
    /// this is the counter delta, not only effects a connected demand
    /// recorded. `None` when that prefix was replaced or the counter
    /// moved backwards. Component-meta admission does not use it: a counter
    /// delta includes sibling spends.
    #[cfg(test)]
    pub(crate) fn observed_spend_since(
        &self,
        snapshot: &RequestEntrySnapshot,
    ) -> Option<RequestSpent> {
        let paid = self.operations_paid.lock();
        if paid.set.members().len() < snapshot.paid.len()
            || paid.set.members()[..snapshot.paid.len()] != snapshot.paid[..]
        {
            return None;
        }
        let now = self.projection_ops_executed.load(Ordering::Relaxed);
        let operations = now.checked_sub(snapshot.state.operations)?;
        Some(RequestSpent {
            entry: snapshot.state.clone(),
            entry_paid: snapshot.paid.clone(),
            operations,
            paid: paid.set.members()[snapshot.paid.len()..]
                .to_vec()
                .into_boxed_slice(),
        })
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
            && paid.set.members().len() == entry.paid_len.checked_add(effects.paid)?;
        alone.then(|| RequestSpent {
            entry_paid: paid.set.members()[..entry.paid_len]
                .iter()
                .cloned()
                .collect(),
            paid: paid.set.members()[entry.paid_len..]
                .iter()
                .cloned()
                .collect(),
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
        attribute_operations(spent.operations);
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
        let spent = tripped.saturating_sub(before);
        attribute_operations(spent);
        spent
    }

    /// Increment the projection-op counter and return `true` when the
    /// request has exceeded its cap.
    pub fn check_projection_op_count(&self) -> bool {
        let current = self
            .projection_ops_executed
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        attribute_operations(1);
        current > self.effective_projection_op_budget()
    }

    /// This thread's recorded spend since `snapshot`, and whether that spend
    /// is the request's whole change. A sibling's operations or paid
    /// identities make the second flag false; the spend is still only what
    /// this thread recorded.
    pub(crate) fn owned_spend(
        &self,
        snapshot: &RequestEntrySnapshot,
        operations: usize,
        paid: &[CostIdentity],
    ) -> (RequestSpent, bool) {
        let effects = crate::project_semantic_dispatch::connected_demand::RequestEffects {
            operations,
            paid: paid.len(),
        };
        if let Some(spent) = self.spent_alone_since(snapshot.state.clone(), effects) {
            return (spent, true);
        }
        (
            RequestSpent {
                entry: snapshot.state.clone(),
                entry_paid: snapshot.paid.clone(),
                operations,
                paid: paid.to_vec().into_boxed_slice(),
            },
            false,
        )
    }

    /// Return the configured cap after applying the legacy default.
    #[must_use]
    pub fn effective_projection_op_budget(&self) -> usize {
        if self.projection_op_budget == 0 {
            DEFAULT_PROJECTION_OP_BUDGET
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
    use super::{RequestBudget, DEFAULT_PROJECTION_OP_BUDGET};

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

    /// A zero cap falls back to the finite default, the checker's whole
    /// instantiation allowance: a smaller one would refuse inputs the
    /// checker answers.
    #[test]
    fn request_budget_zero_cap_falls_back_to_finite_default() {
        assert_eq!(
            DEFAULT_PROJECTION_OP_BUDGET,
            crate::semantic_query::checker_policy::INSTANTIATION_COUNT as usize
        );
        let budget = RequestBudget::new(0);
        assert!(budget.admit_replayed_operations(
            (DEFAULT_PROJECTION_OP_BUDGET - 1) as u64,
            budget.operations_paid(),
            Vec::new(),
        ));
        assert!(!budget.check_projection_op_count(), "last call at cap");
        assert!(
            budget.check_projection_op_count(),
            "next call exceeds default"
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

    fn identity(name: &str) -> super::CostIdentity {
        super::CostIdentity::new(name.as_bytes().to_vec())
    }

    fn effects(
        operations: usize,
        paid: usize,
    ) -> crate::project_semantic_dispatch::connected_demand::RequestEffects {
        crate::project_semantic_dispatch::connected_demand::RequestEffects { operations, paid }
    }

    /// An evaluation's spending is its own only when nothing else spent in
    /// its request while it ran: a sibling's operation or paid computation
    /// makes the request differ from entry plus the evaluation's effects.
    #[test]
    fn spending_is_an_evaluations_own_only_when_it_ran_alone() {
        let alone = RequestBudget::new(100);
        let entry = alone.entry_state();
        alone.check_projection_op_count();
        alone.mark_operations_paid(std::iter::once(identity("own")));
        let spent = alone
            .spent_alone_since(entry, effects(1, 1))
            .expect("the request moved only by the evaluation's own effects");
        assert_eq!(spent.operations, 1);
        assert_eq!(spent.paid.len(), 1);
        assert!(spent.entry_paid.is_empty());

        let shared = RequestBudget::new(100);
        let entry = shared.entry_state();
        shared.check_projection_op_count();
        // A sibling root spends an operation and pays a computation.
        shared.check_projection_op_count();
        shared.mark_operations_paid([identity("own"), identity("sibling")]);
        assert!(
            shared.spent_alone_since(entry, effects(1, 1)).is_none(),
            "a sibling's spending is never recorded as the evaluation's"
        );
    }

    /// Two refusals entered at the same request state each check and apply
    /// their charges in one step: once one has moved the request, the other
    /// is no longer at its entry and changes nothing, so their charges never
    /// collapse into the larger of the two.
    #[test]
    fn a_refusal_applies_only_at_the_state_it_entered_at() {
        let budget = RequestBudget::new(100);
        let entry = budget.entry_state();
        let refusal = |operations: usize, paid: &str| super::RequestSpent {
            entry: entry.clone(),
            entry_paid: Box::from([]),
            operations,
            paid: Box::from([identity(paid)]),
        };
        let first = refusal(3, "a");
        let second = refusal(5, "b");
        assert!(budget.apply_refusal(&first));
        assert_eq!(budget.projection_ops_executed_count(), 3);
        assert!(
            !budget.apply_refusal(&second),
            "the request is no longer where the second refusal entered it"
        );
        assert_eq!(
            budget.projection_ops_executed_count(),
            3,
            "a refused application spends nothing"
        );
        assert!(!budget.operations_paid().contains(&identity("b")));

        let raced = RequestBudget::new(100);
        let entry = raced.entry_state();
        // A sibling spends after the refusal's entry was read.
        raced.check_projection_op_count();
        assert!(!raced.apply_refusal(&super::RequestSpent {
            entry,
            entry_paid: Box::from([]),
            operations: 3,
            paid: Box::from([]),
        }));
        assert_eq!(raced.projection_ops_executed_count(), 1);
    }

    #[test]
    fn observed_spend_counts_direct_checks_and_rejects_a_replaced_prefix() {
        let budget = RequestBudget::new(100);
        let snapshot = budget.capture_entry();
        assert!(!budget.check_projection_op_count());
        budget.mark_operations_paid(std::iter::once(identity("own")));
        let spent = budget
            .observed_spend_since(&snapshot)
            .expect("an append leaves the captured prefix in place");
        assert_eq!(spent.operations, 1);
        assert_eq!(spent.paid.len(), 1);
        let fresh = RequestBudget::new(100);
        assert!(fresh.apply_refusal(&spent));
        assert_eq!(fresh.projection_ops_executed_count(), 1);
        assert!(fresh.operations_paid().contains(&identity("own")));

        let replaced = RequestBudget::new(100);
        replaced.mark_operations_paid(std::iter::once(identity("real")));
        let foreign = super::RequestEntrySnapshot {
            state: replaced.entry_state(),
            paid: Box::from([identity("other")]),
        };
        assert!(
            replaced.observed_spend_since(&foreign).is_none(),
            "a paid prefix that is not the captured order is not this compute's spend"
        );
    }
}
