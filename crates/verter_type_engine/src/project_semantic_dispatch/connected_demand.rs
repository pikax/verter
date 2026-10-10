//! The connected-demand operational ledger.
//!
//! One connected semantic demand is the whole tree of structural and query
//! work rooted at an outermost dispatch or direct projector entry. Bounding
//! that tree is an OPERATIONAL responsibility — work units, query-boundary
//! depth, and the request's cancellation signal — with no semantic content:
//! the ledger decides only *whether more work may run*, never *what a type
//! means*.
//!
//! That responsibility lives here rather than on
//! [`ProjectSemanticDispatch`](super::ProjectSemanticDispatch) so a budget
//! consumer can be handed the ledger alone. The accounting cells are private
//! to this module, so no consumer — inside the dispatch tree or outside it —
//! can read or move the counters except through the operations below; and a
//! consumer typed on [`ConnectedDemandLedger`] cannot reach a semantic
//! capability at all, because the ledger holds none. The sole outside signal
//! it observes is [`DemandCancellation`], a one-question seam over the
//! request's cancellation authority.
//!
//! The caps are single-sourced: [`ConnectedDemandLedger::new`] installs the
//! production limits and only the test-only setter replaces them. Beginning a
//! demand resets the COUNTERS, never the caps, so there is exactly one place a
//! limit can come from.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::cost_receipt::{
    BudgetProfileSpec, CostDependency, CostIdentity, CostScope, DemandCostReceipt, LogicalUsage,
    Nesting, ReplayRefusal, COST_MODEL_REVISION,
};

use crate::semantic_query::PartialReasonSet;

/// The work units one structured comparison the checker records costs the
/// relation at most, on the shape its relation-complexity limit is reached
/// by: a union arm scanning a union target of object types charges the
/// alternative and its pair's worklist step (measured: 22,329 units for the
/// 20,100 comparisons of 200 reversed arms).
pub const RELATION_UNITS_PER_COMPARISON: usize = 2;

/// The construction bytes one structured comparison reserves: what a pair
/// the relation relates holds live at its peak — its frame, its memo entry
/// and its proof (measured: 1.9 KB per pair relating 200 reversed object
/// arms). Provisional: the per-pair footprint and this charge are sized
/// together in the combined performance phase.
pub const RELATION_PAIR_BYTES: usize = 2_048;

/// The construction bytes interning one derived node reserves: its payload
/// record, held twice (the arena slot and the dedup index's key), a fixed
/// index entry, and the `payload_bytes` of text and children it owns.
pub(crate) fn derived_node_bytes(payload_bytes: usize) -> usize {
    2 * std::mem::size_of::<crate::semantic_query::SemanticNodeData>() + 32 + payload_bytes
}

/// Work-unit cap for one connected semantic demand: the operational
/// backstop, sized from the checker's relation-complexity envelope
/// ([`checker_policy::RELATION_COMPARISONS`] structured comparisons at
/// [`RELATION_UNITS_PER_COMPARISON`] units each). Work units do not bound
/// memory; the construction-byte rail
/// ([`MAX_CONNECTED_CONSTRUCTION_BYTES`]) does, and on a relation it binds
/// first: at [`RELATION_PAIR_BYTES`] per comparison it stops a relation
/// near 262,000 comparisons as typed incompleteness, until the per-pair
/// footprint shrinks.
///
/// [`checker_policy::RELATION_COMPARISONS`]: crate::semantic_query::checker_policy::RELATION_COMPARISONS
pub const MAX_CONNECTED_PROJECTION_WORK: usize =
    crate::semantic_query::checker_policy::RELATION_COMPARISONS as usize
        * RELATION_UNITS_PER_COMPARISON;
/// Nested query-boundary cap for one connected semantic demand.
pub const MAX_CONNECTED_QUERY_DEPTH: u16 = 24;
/// Construction-byte allowance for one connected semantic demand: the bytes
/// it reserves before it builds, never refunded within the demand, so the
/// allowance bounds what the demand holds at its peak. Provisional: sized
/// with the per-unit footprint in the combined performance phase.
pub(super) const MAX_CONNECTED_CONSTRUCTION_BYTES: usize = 512 << 20;

/// What one connected demand has charged so far: its work units and its
/// construction bytes.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DemandUsage {
    pub work: usize,
    pub bytes: usize,
}

/// Verter's own instantiation budget for one connected semantic demand:
/// how deep instantiations evaluated on the continuation runtime may nest
/// (each one's frame is held on the heap while the one it needs builds).
/// Far past the checker's own depth limit, so Verter answers where the
/// checker gives up; reaching it is the checker's TS2589, reported at
/// Verter's limit ([`checker_policy::instantiation_budget`]). It bounds
/// the frames one chain holds, so an instantiation that never terminates
/// stops in bounded memory; the work ledger bounds its time.
///
/// [`checker_policy::instantiation_budget`]: crate::semantic_query::checker_policy::instantiation_budget
pub(super) const MAX_CONNECTED_INSTANTIATION_DEPTH: u32 = 10_000;

/// Verter's own budget for one tail run of a conditional type: how many
/// tail steps (each the conditional over its next arguments, evaluated in
/// place) one run may take. Far past the checker's own tail limit, so
/// Verter answers where the checker gives up; reaching it is the checker's
/// TS2589 ([`ConditionalTail`]). A run holds one step's arguments at a
/// time, and the arguments it has seen for its recurrence check.
///
/// [`ConditionalTail`]: crate::semantic_query::checker_policy::ConditionalTail
pub(super) const MAX_CONNECTED_TAIL_STEPS: u32 = 10_000;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// A lower instantiation budget for the ledgers this thread installs,
    /// set by [`InstantiationBudgetForTests`].
    static INSTANTIATION_DEPTH_FOR_TESTS: Cell<Option<u32>> = const { Cell::new(None) };
    /// A lower tail budget for the ledgers this thread installs, set by
    /// [`TailBudgetForTests`].
    static TAIL_STEPS_FOR_TESTS: Cell<Option<u32>> = const { Cell::new(None) };
    /// A lower work budget for the ledgers this thread installs, set by
    /// [`WorkBudgetForTests`].
    static WORK_FOR_TESTS: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Install `limit` as the connected-work budget of every ledger this
/// thread creates until the guard drops — the production budget's code
/// path at an amount of work a test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct WorkBudgetForTests {
    previous: Option<usize>,
}

#[cfg(any(test, feature = "test-support"))]
impl WorkBudgetForTests {
    pub fn install(limit: usize) -> Self {
        Self {
            previous: WORK_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for WorkBudgetForTests {
    fn drop(&mut self) {
        WORK_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// Install `limit` as the tail budget of every ledger this thread creates
/// until the guard drops — the production budget's code path at a length a
/// test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct TailBudgetForTests {
    previous: Option<u32>,
}

#[cfg(any(test, feature = "test-support"))]
impl TailBudgetForTests {
    pub fn install(limit: u32) -> Self {
        Self {
            previous: TAIL_STEPS_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TailBudgetForTests {
    fn drop(&mut self) {
        TAIL_STEPS_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// Install `limit` as the instantiation budget of every ledger this
/// thread creates until the guard drops — the production budget's code
/// path at a depth a test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct InstantiationBudgetForTests {
    previous: Option<u32>,
}

#[cfg(any(test, feature = "test-support"))]
impl InstantiationBudgetForTests {
    pub fn install(limit: u32) -> Self {
        Self {
            previous: INSTANTIATION_DEPTH_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for InstantiationBudgetForTests {
    fn drop(&mut self) {
        INSTANTIATION_DEPTH_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// The one signal the ledger observes from outside its own accounting:
/// whether the owning request has been cancelled.
///
/// A newtype rather than the resolver context itself, and its field is
/// private to this module: the ledger — and therefore every consumer typed on
/// the ledger — can ask the single question below and reach nothing else. No
/// host caches, no store view, and above all no route back into semantic
/// dispatch.
#[derive(Clone, Copy)]
pub(crate) struct DemandCancellation<'a> {
    checkpoint: crate::resolver_core::request_ports::CancellationCheckpoint,
    _request: std::marker::PhantomData<&'a ()>,
}

impl<'a> DemandCancellation<'a> {
    /// Narrow the request snapshot's flag handles down to the cancellation
    /// signal. The dispatch passes the snapshot it borrowed once at request
    /// admission, so every charge-boundary checkpoint below is a plain
    /// field read.
    pub(super) fn from_flags(
        flags: &'a crate::resolver_core::resolver_context::RequestFlags,
    ) -> Self {
        Self {
            checkpoint: flags.cancellation_checkpoint(),
            _request: std::marker::PhantomData,
        }
    }

    /// Cheap cancellation checkpoint, consulted at every charge boundary.
    #[cfg_attr(feature = "test-support", track_caller)]
    fn is_cancelled(&self) -> bool {
        self.checkpoint.is_cancelled()
    }
}

/// Operational budget ledger for one dispatcher's connected demands.
///
/// Holds the live counters plus the configured caps. Every field is private
/// to this module: the ledger's invariants (a counter advances only inside an
/// active demand; a trip is sticky for the rest of the demand; caps change
/// only outside a demand) are enforced by the operations, not by convention.
pub struct ConnectedDemandLedger<'a> {
    cancellation: DemandCancellation<'a>,
    active: Cell<bool>,
    work_used: Cell<usize>,
    work_limit: Cell<usize>,
    bytes_used: Cell<usize>,
    bytes_limit: Cell<usize>,
    query_depth: Cell<u16>,
    query_depth_limit: Cell<u16>,
    instantiation_depth_limit: u32,
    tail_steps_limit: u32,
    tripped: Cell<PartialReasonSet>,
    /// The cold computations recording their exclusive cost, innermost
    /// last; `scope_open` mirrors "the stack is not empty" so a charge
    /// taken outside any recording pays one flag read.
    scopes: RefCell<Vec<CostScope>>,
    scope_open: Cell<bool>,
    /// The computations whose receipt, and its whole prerequisite closure,
    /// this connected demand has paid.
    paid: RefCell<FxHashSet<CostIdentity>>,
    /// What this connected demand itself did to its request: the request
    /// operations it spent and the computations it was first to pay.
    request_effects: Cell<RequestEffects>,
    /// Whether one of this demand's reads was answered with a recursion
    /// carrier another task's schedule decided — a cross-task wait cycle,
    /// or a joined producer's own re-entry: the demand went on without work
    /// it alone would have done, so where it stopped is not its own.
    schedule_cut: Cell<bool>,
}

/// What one connected demand did to its request
/// ([`ConnectedDemandLedger::request_effects`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RequestEffects {
    /// The request operations the demand spent.
    pub(crate) operations: usize,
    /// The computations the demand added to the request's paid set.
    pub(crate) paid: usize,
}

impl std::fmt::Debug for ConnectedDemandLedger<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedDemandLedger")
            .field("active", &self.active.get())
            .field("work_used", &self.work_used.get())
            .field("work_limit", &self.work_limit.get())
            .field("bytes_used", &self.bytes_used.get())
            .field("bytes_limit", &self.bytes_limit.get())
            .field("query_depth", &self.query_depth.get())
            .field("query_depth_limit", &self.query_depth_limit.get())
            .field("instantiation_depth_limit", &self.instantiation_depth_limit)
            .field("tail_steps_limit", &self.tail_steps_limit)
            .field("tripped", &self.tripped.get())
            .finish()
    }
}

impl<'a> ConnectedDemandLedger<'a> {
    /// Install a ledger at the production caps, observing `cancellation`.
    pub(super) fn new(cancellation: DemandCancellation<'a>) -> Self {
        Self {
            cancellation,
            active: Cell::new(false),
            work_used: Cell::new(0),
            #[cfg(not(any(test, feature = "test-support")))]
            work_limit: Cell::new(MAX_CONNECTED_PROJECTION_WORK),
            #[cfg(any(test, feature = "test-support"))]
            work_limit: Cell::new(
                WORK_FOR_TESTS
                    .with(Cell::get)
                    .unwrap_or(MAX_CONNECTED_PROJECTION_WORK),
            ),
            bytes_used: Cell::new(0),
            bytes_limit: Cell::new(MAX_CONNECTED_CONSTRUCTION_BYTES),
            query_depth: Cell::new(0),
            query_depth_limit: Cell::new(MAX_CONNECTED_QUERY_DEPTH),
            #[cfg(not(any(test, feature = "test-support")))]
            instantiation_depth_limit: MAX_CONNECTED_INSTANTIATION_DEPTH,
            #[cfg(any(test, feature = "test-support"))]
            instantiation_depth_limit: INSTANTIATION_DEPTH_FOR_TESTS
                .with(Cell::get)
                .unwrap_or(MAX_CONNECTED_INSTANTIATION_DEPTH),
            #[cfg(not(any(test, feature = "test-support")))]
            tail_steps_limit: MAX_CONNECTED_TAIL_STEPS,
            #[cfg(any(test, feature = "test-support"))]
            tail_steps_limit: TAIL_STEPS_FOR_TESTS
                .with(Cell::get)
                .unwrap_or(MAX_CONNECTED_TAIL_STEPS),
            tripped: Cell::new(PartialReasonSet::empty()),
            scopes: RefCell::new(Vec::new()),
            scope_open: Cell::new(false),
            paid: RefCell::new(FxHashSet::default()),
            request_effects: Cell::new(RequestEffects::default()),
            schedule_cut: Cell::new(false),
        }
    }

    /// Whether an instantiation nested `depth` deep on the continuation
    /// runtime is within Verter's instantiation budget
    /// ([`MAX_CONNECTED_INSTANTIATION_DEPTH`]).
    pub(crate) fn instantiation_within_budget(&self, depth: u32) -> bool {
        depth <= self.instantiation_depth_limit
    }

    /// Take one more step of the tail run `tail` under this demand's tail
    /// budget, recording the steps the run has taken on the open recording:
    /// its receipt then names the tail allowance its cold run needed.
    pub(crate) fn tail_step(
        &self,
        tail: &mut crate::semantic_query::checker_policy::ConditionalTail,
        operation: crate::semantic_query::CheckerDiagnosticOperation,
    ) -> Result<(), crate::semantic_query::checker_policy::OperationRefusal> {
        tail.step(self.tail_steps_limit, operation)?;
        if self.scope_open.get() {
            if let Some(scope) = self.scopes.borrow_mut().last_mut() {
                scope.tail_steps = scope.tail_steps.max(tail.steps());
            }
        }
        Ok(())
    }

    /// Join the active connected demand, or install a fresh one when this is
    /// the outermost entry. Returns the panic-safe guard plus the trip that
    /// forbids the caller's work — `Some` when the ledger had already tripped,
    /// when the request is cancelled, or when `query_boundary` would exceed
    /// the nested-query depth cap.
    pub(super) fn enter(
        &self,
        query_boundary: bool,
    ) -> (ConnectedDemandGuard<'_>, Option<PartialReasonSet>) {
        let root = !self.active.get();
        if root {
            self.work_used.set(0);
            self.bytes_used.set(0);
            self.paid.borrow_mut().clear();
            self.request_effects.set(RequestEffects::default());
            self.schedule_cut.set(false);
            self.scopes.borrow_mut().clear();
            self.scope_open.set(false);
            self.query_depth.set(0);
            self.tripped.set(PartialReasonSet::empty());
            self.active.set(true);
        }
        if self.cancellation.is_cancelled() {
            self.tripped
                .set(self.tripped.get().union(PartialReasonSet::CANCELLED));
        }
        let mut entered_query_depth = false;
        let tripped = self.tripped.get();
        let trip = if !tripped.is_empty() {
            Some(tripped)
        } else if query_boundary && self.query_depth.get() >= self.query_depth_limit.get() {
            let tripped = tripped.union(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT);
            self.tripped.set(tripped);
            Some(tripped)
        } else {
            if query_boundary {
                self.query_depth.set(self.query_depth.get() + 1);
                entered_query_depth = true;
            }
            None
        };
        (
            ConnectedDemandGuard {
                ledger: self,
                root,
                entered_query_depth,
            },
            trip,
        )
    }

    /// The trip that refuses one more natively nested inline evaluation
    /// when `nesting` evaluations are already open on the native stack —
    /// `None` when it may run. Nesting takes the same cap as nested query
    /// boundaries: an evaluation that re-enters inline, without a query
    /// boundary, would otherwise grow the native stack unbounded. The
    /// refusal is the depth rail's typed incompleteness, sticky like every
    /// trip.
    pub(super) fn nesting_trip(&self, nesting: usize) -> Option<PartialReasonSet> {
        if nesting < usize::from(self.query_depth_limit.get()) {
            return None;
        }
        Some(self.record_trip(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT))
    }

    /// Whether this demand has ALREADY tripped. A sub-query suppressed under a
    /// tripped ledger is never evidence about the queried surface — the caller
    /// must report the budget, not a semantic verdict.
    pub(crate) fn has_tripped(&self) -> bool {
        !self.tripped.get().is_empty()
    }

    /// The trip of the ACTIVE demand, if any. `None` outside a demand, and
    /// `None` inside an untripped one.
    pub(crate) fn active_trip(&self) -> Option<PartialReasonSet> {
        self.active
            .get()
            .then(|| self.tripped.get())
            .filter(|tripped| !tripped.is_empty())
    }

    /// Charge one work unit. `Err` carries the sticky trip set that now
    /// forbids further work.
    pub fn charge(&self) -> Result<(), PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "connected work must be charged inside a connected-demand guard"
        );
        if self.cancellation.is_cancelled() {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        let work_used = self.work_used.get();
        if work_used >= self.work_limit.get() {
            let tripped = tripped.union(PartialReasonSet::PROJECTION_WORK_LIMIT);
            self.tripped.set(tripped);
            return Err(tripped);
        }
        self.work_used.set(work_used + 1);
        self.accrue(1, 0);
        Ok(())
    }

    /// Charge `units` work units at once — the work a planned stage costs,
    /// paid before it runs. `Err` carries the sticky trip set when the
    /// demand cannot pay for all of it.
    pub fn charge_units(&self, units: usize) -> Result<(), PartialReasonSet> {
        let available = self.work_available()?;
        if units > available {
            return Err(self.record_trip(PartialReasonSet::PROJECTION_WORK_LIMIT));
        }
        self.commit(units);
        Ok(())
    }

    /// Add one structured relation comparison to the open recording, if
    /// any: a comparison the computation on top recorded itself.
    #[inline(always)]
    pub(crate) fn accrue_comparison(&self) {
        if !self.scope_open.get() {
            return;
        }
        if let Some(scope) = self.scopes.borrow_mut().last_mut() {
            scope.exclusive.comparisons = scope.exclusive.comparisons.saturating_add(1);
        }
    }

    /// Add `work` units and `bytes` to the open recording, if any: the cost
    /// the computation on top of the scope stack performs itself.
    #[inline(always)]
    fn accrue(&self, work: usize, bytes: usize) {
        if !self.scope_open.get() {
            return;
        }
        if let Some(scope) = self.scopes.borrow_mut().last_mut() {
            scope.exclusive.work = scope.exclusive.work.saturating_add(work as u64);
            scope.exclusive.bytes = scope.exclusive.bytes.saturating_add(bytes as u64);
        }
    }

    /// Snapshot the remaining work available to a query-free terminal run. The
    /// caller commits exactly the units it consumes before any nested semantic
    /// dispatch.
    #[inline(always)]
    pub(crate) fn work_available(&self) -> Result<usize, PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "connected work must be observed inside a connected-demand guard"
        );
        if self.cancellation.is_cancelled() {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        Ok(self.work_limit.get().saturating_sub(self.work_used.get()))
    }

    /// Commit work consumed against a snapshot taken by
    /// [`Self::work_available`].
    #[inline(always)]
    pub(crate) fn commit(&self, consumed: usize) {
        if consumed == 0 {
            return;
        }
        verter_debug_assert!(self.active.get());
        let work_used = self.work_used.get();
        verter_debug_assert!(work_used.saturating_add(consumed) <= self.work_limit.get());
        self.work_used.set(work_used + consumed);
        self.accrue(consumed, 0);
    }

    /// Reserve `bytes` of construction before building what they hold.
    /// `Err` carries the sticky trip set when the demand cannot pay for them
    /// ([`PartialReasonSet::CONNECTED_MEMORY_LIMIT`] when this reservation
    /// is the one refused).
    pub fn reserve_bytes(&self, bytes: usize) -> Result<(), PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "construction bytes must be reserved inside a connected-demand guard"
        );
        if self.cancellation.is_cancelled() {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        let used = self.bytes_used.get();
        match used.checked_add(bytes) {
            Some(total) if total <= self.bytes_limit.get() => {
                self.bytes_used.set(total);
                self.accrue(0, bytes);
                Ok(())
            }
            _ => Err(self.record_trip(PartialReasonSet::CONNECTED_MEMORY_LIMIT)),
        }
    }

    /// What the active demand has charged so far.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn usage(&self) -> DemandUsage {
        DemandUsage {
            work: self.work_used.get(),
            bytes: self.bytes_used.get(),
        }
    }

    /// The `(limit, actual, rail)` triple describing why the demand tripped,
    /// for the budget-exceeded carrier the dispatcher reports. Purely
    /// operational reporting — the ledger owns the numbers, so no consumer
    /// reads the counters directly.
    pub fn limit_report(&self, reasons: PartialReasonSet) -> (usize, u64, &'static str) {
        if !self.active.get() {
            return (0, 0, "unknown");
        }
        if reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT) {
            (
                usize::from(self.query_depth_limit.get()),
                u64::from(self.query_depth.get()),
                "connected-query-depth",
            )
        } else if reasons.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT) {
            (
                self.bytes_limit.get(),
                self.bytes_used.get() as u64,
                "construction-bytes",
            )
        } else {
            (
                self.work_limit.get(),
                self.work_used.get() as u64,
                "projection-work",
            )
        }
    }

    /// Replace the caps. Test-only, and refused inside an active demand so one
    /// run can never observe two different caps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_limits_for_tests(&self, work: usize, depth: u16) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.work_limit.set(work);
        self.query_depth_limit.set(depth);
    }

    /// Replace the construction-byte allowance. Test-only, and refused inside
    /// an active demand.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_byte_limit_for_tests(&self, bytes: usize) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.bytes_limit.set(bytes);
    }

    /// The construction bytes the last connected demand reserved. Test-only:
    /// kept after the demand ends, and reset when the next one enters.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bytes_used_for_tests(&self) -> usize {
        self.bytes_used.get()
    }

    /// The work units the last connected demand charged. Test-only: kept
    /// after the demand ends, and reset when the next one enters.
    #[cfg(any(test, feature = "test-support"))]
    pub fn work_used_for_tests(&self) -> usize {
        self.work_used.get()
    }

    /// Fold `reason` into the sticky trip set and return the widened set.
    pub(super) fn record_trip(&self, reason: PartialReasonSet) -> PartialReasonSet {
        verter_debug_assert!(
            self.active.get(),
            "an operational limit can trip only inside a connected demand"
        );
        let tripped = self.tripped.get().union(reason);
        self.tripped.set(tripped);
        tripped
    }
}

/// The cost-receipt operations: recording each cold computation's
/// exclusive cost and admitting stored results by replaying their receipts.
impl ConnectedDemandLedger<'_> {
    /// Open the recording of a cold computation of `identity`: until it is
    /// sealed or abandoned, every charge the demand takes is its exclusive
    /// cost, except the charges of computations opened above it.
    pub fn open_cost_scope(&self, identity: CostIdentity) {
        self.resume_cost_scope(CostScope {
            identity,
            exclusive: LogicalUsage::default(),
            prerequisites: Vec::new(),
            tail_steps: 0,
        });
    }

    /// Refresh the mirror of "a recording is open".
    fn sync_top(&self, scopes: &[CostScope]) {
        self.scope_open.set(!scopes.is_empty());
    }

    /// Take the open computation's recording off the stack without ending
    /// it: a suspended frame keeps its recording between its steps, so a
    /// sibling's charges never accrue to it. `None` when none is open.
    pub(crate) fn suspend_cost_scope(&self) -> Option<CostScope> {
        let mut scopes = self.scopes.borrow_mut();
        let scope = scopes.pop();
        self.sync_top(&scopes);
        scope
    }

    /// Put a suspended recording back on top: its frame runs again.
    pub(crate) fn resume_cost_scope(&self, scope: CostScope) {
        let mut scopes = self.scopes.borrow_mut();
        scopes.push(scope);
        self.sync_top(&scopes);
    }

    /// Spend one of `request`'s operations for the computation on top: the
    /// per-request projection fuse admits one more operator build. `true`
    /// when the request has now passed its cap.
    pub(crate) fn spend_request_operation(
        &self,
        request: &crate::request_budget::RequestBudget,
    ) -> bool {
        self.note_request_effects(1, 0);
        if self.scope_open.get() {
            if let Some(scope) = self.scopes.borrow_mut().last_mut() {
                scope.exclusive.operations = scope.exclusive.operations.saturating_add(1);
            }
        }
        request.check_projection_op_count()
    }

    /// Record `operations` request operations this demand spent and `paid`
    /// computations it was first to pay in its request.
    pub(crate) fn note_request_effects(&self, operations: usize, paid: usize) {
        let effects = self.request_effects.get();
        self.request_effects.set(RequestEffects {
            operations: effects.operations.saturating_add(operations),
            paid: effects.paid.saturating_add(paid),
        });
    }

    /// What this connected demand itself has done to its request.
    pub(crate) fn request_effects(&self) -> RequestEffects {
        self.request_effects.get()
    }

    /// Record that one of this demand's reads was answered with a recursion
    /// carrier another task's schedule decided.
    pub(crate) fn note_schedule_cut(&self) {
        if self.active.get() {
            self.schedule_cut.set(true);
        }
    }

    /// Whether another task's schedule has cut this demand's evaluation:
    /// what it answered depends on that schedule.
    pub(crate) fn was_schedule_cut(&self) -> bool {
        self.schedule_cut.get()
    }

    /// Mark `identity` paid in `request`, counting it as this demand's
    /// effect when the demand is the first to pay it there.
    fn mark_request_paid(
        &self,
        request: &crate::request_budget::RequestBudget,
        identity: CostIdentity,
    ) {
        let added = request.mark_operations_paid(std::iter::once(identity));
        self.note_request_effects(0, added);
    }

    /// Record that the open computation consumed the result `receipt`
    /// costs, the way `nesting` names. Recorded however the result arrived
    /// — computed, served or already paid — so the receipt being built owes
    /// it in full.
    pub fn record_prerequisite(&self, receipt: &Arc<DemandCostReceipt>, nesting: Nesting) {
        if !self.scope_open.get() {
            return;
        }
        if let Some(scope) = self.scopes.borrow_mut().last_mut() {
            scope.prerequisites.push(CostDependency {
                receipt: Arc::clone(receipt),
                nesting,
            });
        }
    }

    /// Seal the open computation's recording: its receipt, now paid for
    /// this demand (and its request operations for `request`) together
    /// with its closure, which it consumed paid. `None` when no computation
    /// is recording.
    pub fn seal_cost_scope(
        &self,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        let scope = self.suspend_cost_scope()?;
        self.paid.borrow_mut().insert(scope.identity.clone());
        if let Some(request) = request {
            self.mark_request_paid(request, scope.identity.clone());
        }
        Some(DemandCostReceipt::with_tail_steps(
            scope.identity,
            scope.exclusive,
            scope.prerequisites,
            scope.tail_steps,
        ))
    }

    /// How many computations are recording: the depth of the open
    /// recording on top.
    pub(crate) fn recording_depth(&self) -> usize {
        self.scopes.borrow().len()
    }

    /// Drop the open computation's recording without a receipt: it did not
    /// complete, so nothing about it may be served warm as paid. What it
    /// charged stays spent, and is its consumer's own cost: the consumer
    /// paid for a computation it then had to do without.
    pub fn abandon_cost_scope(&self) {
        if let Some(scope) = self.suspend_cost_scope() {
            self.fold_into_consumer(scope);
        }
    }

    /// Fold an ended recording into the recording below it, if any: the
    /// usage it charged, and the prerequisites it consumed.
    fn fold_into_consumer(&self, scope: CostScope) {
        if let Some(consumer) = self.scopes.borrow_mut().last_mut() {
            consumer.exclusive = consumer.exclusive.saturating_add(scope.exclusive);
            consumer.prerequisites.extend(scope.prerequisites);
            consumer.tail_steps = consumer.tail_steps.max(scope.tail_steps);
        }
    }

    /// Whether this demand has paid `identity`'s receipt and its closure.
    pub fn is_paid(&self, identity: &CostIdentity) -> bool {
        self.paid.borrow().contains(identity)
    }

    /// The allowances this ledger answers to under `request`'s operation
    /// allowance: the spec its interned profile names.
    pub(crate) fn profile_spec(
        &self,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> BudgetProfileSpec {
        BudgetProfileSpec {
            work: self.work_limit.get(),
            bytes: self.bytes_limit.get(),
            query_depth: self.query_depth_limit.get(),
            instantiation_depth: self.instantiation_depth_limit,
            tail_steps: self.tail_steps_limit,
            request_operations: request
                .map_or(0, |request| request.effective_projection_op_budget()),
            relation_comparisons: crate::semantic_query::checker_policy::relation_comparisons(),
            cost_model_revision: COST_MODEL_REVISION,
        }
    }

    /// Admit serving the result `receipt` costs without computing it, read
    /// the way `nesting` names: charge, in one admission, the exclusive
    /// usage of every computation in its closure this demand has not paid
    /// (and the request operations of every one `request` has not paid),
    /// and mark them paid. The receipt's footprint is checked against the
    /// reader's allowances even when its cost is paid: its nesting against
    /// the remaining query depth, its instantiation frames against the
    /// instantiation allowance at the reader's frame depth, its tail runs
    /// against the tail allowance. A refusal charges and marks nothing,
    /// and leaves the stored result untouched: this caller computes the
    /// result itself, and another demand with room may still serve it.
    /// Construction bytes are charged but never refuse: a complete result
    /// is not rejected at handoff for the bytes it took to build. Replayed
    /// charges belong to no open recording — the consumer records the
    /// receipt as a prerequisite instead. The unpaid closure's structured
    /// comparisons must fit a fresh relation check's whole allowance.
    pub fn replay_admit(
        &self,
        receipt: &Arc<DemandCostReceipt>,
        request: Option<&crate::request_budget::RequestBudget>,
        nesting: Nesting,
    ) -> Result<(), ReplayRefusal> {
        self.replay_admit_within(
            receipt,
            request,
            nesting,
            crate::semantic_query::checker_policy::relation_comparisons(),
        )
        .map(|_| ())
    }

    /// [`Self::replay_admit`] for a reader whose relation check, if any,
    /// has `comparison_room` structured comparisons left: the unpaid
    /// closure's comparisons — what its cold run would have recorded into
    /// that check — must fit it. `Ok` carries those comparisons, which the
    /// reader's check records as its cold run would have.
    pub(crate) fn replay_admit_within(
        &self,
        receipt: &Arc<DemandCostReceipt>,
        request: Option<&crate::request_budget::RequestBudget>,
        nesting: Nesting,
        comparison_room: u32,
    ) -> Result<u32, ReplayRefusal> {
        let tripped = self.tripped.get();
        if !tripped.is_empty() || self.cancellation.is_cancelled() {
            return Err(ReplayRefusal::Tripped);
        }
        let footprint = receipt.footprint();
        if u32::from(self.query_depth.get()) + u32::from(footprint.query_depth)
            > u32::from(self.query_depth_limit.get())
        {
            return Err(ReplayRefusal::Depth);
        }
        let first_frame = match nesting {
            Nesting::Frame { depth } => Some(depth),
            Nesting::Drive => Some(1),
            Nesting::InPlace | Nesting::Entered => None,
        };
        if footprint.instantiation_required > self.instantiation_depth_limit
            || first_frame.is_some_and(|depth| {
                depth.saturating_add(u32::from(footprint.instantiation_height))
                    > self.instantiation_depth_limit
            })
        {
            return Err(ReplayRefusal::InstantiationDepth);
        }
        if footprint.tail_steps >= self.tail_steps_limit && footprint.tail_steps > 0 {
            return Err(ReplayRefusal::TailSteps);
        }
        let work_room = self.work_limit.get().saturating_sub(self.work_used.get()) as u64;
        let operation_room = request.map_or(u64::MAX, |request| {
            request
                .effective_projection_op_budget()
                .saturating_sub(request.projection_ops_executed_count()) as u64
        });
        let operations_paid = request.map(|request| request.operations_paid());
        let (unpaid, unpaid_operations, total) = {
            let paid = self.paid.borrow();
            let mut unpaid: Vec<&CostIdentity> = Vec::new();
            let mut unpaid_operations: Vec<&CostIdentity> = Vec::new();
            let mut seen: FxHashSet<&CostIdentity> = FxHashSet::default();
            let mut total = LogicalUsage::default();
            let mut stack: Vec<&Arc<DemandCostReceipt>> = vec![receipt];
            while let Some(next) = stack.pop() {
                let identity = next.identity();
                let demand_paid = paid.contains(identity);
                let request_paid = operations_paid
                    .as_ref()
                    .is_none_or(|operations| operations.contains(identity));
                if (demand_paid && request_paid) || !seen.insert(identity) {
                    continue;
                }
                let exclusive = next.exclusive();
                let owed = LogicalUsage {
                    work: if demand_paid { 0 } else { exclusive.work },
                    bytes: if demand_paid { 0 } else { exclusive.bytes },
                    operations: if request_paid {
                        0
                    } else {
                        exclusive.operations
                    },
                    comparisons: if demand_paid {
                        0
                    } else {
                        exclusive.comparisons
                    },
                };
                total = total.checked_add(owed).ok_or(ReplayRefusal::Work)?;
                // The rest of the closure can only add to what is owed.
                if total.work > work_room {
                    return Err(ReplayRefusal::Work);
                }
                if total.operations > operation_room {
                    return Err(ReplayRefusal::Operations);
                }
                if total.comparisons > u64::from(comparison_room) {
                    return Err(ReplayRefusal::RelationComparisons);
                }
                if !demand_paid {
                    unpaid.push(identity);
                }
                if !request_paid {
                    unpaid_operations.push(identity);
                }
                stack.extend(
                    next.prerequisites()
                        .iter()
                        .rev()
                        .map(|dependency| &dependency.receipt),
                );
            }
            (
                unpaid.into_iter().cloned().collect::<Vec<_>>(),
                unpaid_operations.into_iter().cloned().collect::<Vec<_>>(),
                total,
            )
        };
        if let (Some(request), Some(operations_paid)) = (request, operations_paid) {
            let newly_paid = unpaid_operations.len();
            if !request.admit_replayed_operations(
                total.operations,
                operations_paid,
                unpaid_operations,
            ) {
                return Err(ReplayRefusal::Operations);
            }
            self.note_request_effects(
                usize::try_from(total.operations).unwrap_or(usize::MAX),
                newly_paid,
            );
        }
        self.work_used
            .set(self.work_used.get() + total.work as usize);
        self.bytes_used
            .set(self.bytes_used.get().saturating_add(total.bytes as usize));
        self.paid.borrow_mut().extend(unpaid);
        // Within `comparison_room`, so within `u32`.
        Ok(u32::try_from(total.comparisons).unwrap_or(u32::MAX))
    }
}

impl<'a> ConnectedDemandLedger<'a> {
    /// Begin recording the materialized prefixes of the computation
    /// recording at `depth`: what it charged before now stays its own, and
    /// each [`PrefixRecording::seal_prefix`] seals what it charged since
    /// the previous prefix as that prefix's receipt. `None` when no
    /// computation is recording at `depth`.
    pub(crate) fn record_prefixes(&self, depth: usize) -> Option<PrefixRecording<'_, 'a>> {
        let mut scopes = self.scopes.borrow_mut();
        if depth == 0 || scopes.len() != depth {
            return None;
        }
        let scope = scopes.last_mut()?;
        let held = CostScope {
            identity: scope.identity.clone(),
            exclusive: std::mem::take(&mut scope.exclusive),
            prerequisites: std::mem::take(&mut scope.prerequisites),
            tail_steps: std::mem::take(&mut scope.tail_steps),
        };
        Some(PrefixRecording {
            ledger: self,
            depth,
            held: Some(held),
        })
    }
}

/// The prefixes one computation materializes on its way to its result,
/// each sealed as a receipt of its own ([`ConnectedDemandLedger::record_prefixes`]).
/// What the computation charged before the first prefix stays its own, and
/// is put back on its recording when this ends, however it ends.
pub(crate) struct PrefixRecording<'l, 'a> {
    ledger: &'l ConnectedDemandLedger<'a>,
    depth: usize,
    held: Option<CostScope>,
}

impl PrefixRecording<'_, '_> {
    /// Seal what the computation charged since the previous prefix — with
    /// that prefix's receipt as its prerequisite — as the receipt of the
    /// prefix `identity` it just materialized, paid for this demand and
    /// `request` exactly as a computed result is; the computation goes on
    /// with that receipt as its prerequisite. `None` when the computation
    /// is no longer the recording on top.
    pub(crate) fn seal_prefix(
        &self,
        identity: CostIdentity,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        let ledger = self.ledger;
        let receipt = {
            let mut scopes = ledger.scopes.borrow_mut();
            if scopes.len() != self.depth {
                return None;
            }
            let scope = scopes.last_mut()?;
            let receipt = DemandCostReceipt::with_tail_steps(
                identity.clone(),
                std::mem::take(&mut scope.exclusive),
                std::mem::take(&mut scope.prerequisites),
                std::mem::take(&mut scope.tail_steps),
            );
            scope.prerequisites.push(CostDependency {
                receipt: Arc::clone(&receipt),
                nesting: Nesting::InPlace,
            });
            receipt
        };
        ledger.paid.borrow_mut().insert(identity.clone());
        if let Some(request) = request {
            ledger.mark_request_paid(request, identity);
        }
        Some(receipt)
    }
}

impl Drop for PrefixRecording<'_, '_> {
    fn drop(&mut self) {
        let Some(held) = self.held.take() else {
            return;
        };
        let mut scopes = self.ledger.scopes.borrow_mut();
        if let Some(scope) = scopes
            .get_mut(self.depth - 1)
            .filter(|scope| scope.identity == held.identity)
        {
            scope.exclusive = scope.exclusive.saturating_add(held.exclusive);
            scope.prerequisites.extend(held.prerequisites);
            scope.tail_steps = scope.tail_steps.max(held.tail_steps);
        }
    }
}

/// The recording of one cold computation, open until its build ends. A
/// recording dropped without [`Self::finish`] — a panic, an early return —
/// is abandoned, so its charges stay its consumer's.
pub(crate) struct CostRecording<'l, 'a> {
    ledger: &'l ConnectedDemandLedger<'a>,
    open: bool,
}

impl ConnectedDemandLedger<'_> {
    /// Open the recording of a cold computation of `identity`.
    pub(crate) fn record_cost(&self, identity: CostIdentity) -> CostRecording<'_, '_> {
        self.open_cost_scope(identity);
        CostRecording {
            ledger: self,
            open: true,
        }
    }

    /// Whether a connected demand is active.
    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    /// The work units and construction bytes the active demand charged.
    pub(crate) fn charged(&self) -> (usize, usize) {
        (self.work_used.get(), self.bytes_used.get())
    }

    /// Leave the active demand as a sealed refusal's evaluation left it:
    /// its work and bytes charged and `trip` tripped.
    pub(crate) fn apply_refusal(&self, work: usize, bytes: usize, trip: PartialReasonSet) {
        self.work_used
            .set(self.work_used.get().saturating_add(work));
        self.bytes_used
            .set(self.bytes_used.get().saturating_add(bytes));
        self.record_trip(trip);
    }

    /// Whether no computation is recording.
    pub(crate) fn is_unrecorded(&self) -> bool {
        !self.scope_open.get()
    }
}

impl CostRecording<'_, '_> {
    /// End the recording: a complete computation seals its receipt (paid
    /// for this demand, its operations for `request`); an incomplete one is
    /// abandoned and leaves none.
    pub(crate) fn finish(
        mut self,
        complete: bool,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        self.open = false;
        if complete {
            self.ledger.seal_cost_scope(request)
        } else {
            self.ledger.abandon_cost_scope();
            None
        }
    }
}

impl Drop for CostRecording<'_, '_> {
    fn drop(&mut self) {
        if self.open {
            self.ledger.abandon_cost_scope();
        }
    }
}

/// Panic-safe lifetime of one connected semantic demand. The outermost
/// dispatch or direct projector installs the demand; nested query and worklist
/// entries join it without holding a `RefCell` borrow across semantic work.
pub struct ConnectedDemandGuard<'g> {
    ledger: &'g ConnectedDemandLedger<'g>,
    root: bool,
    entered_query_depth: bool,
}

impl ConnectedDemandGuard<'_> {
    pub(super) fn is_root(&self) -> bool {
        self.root
    }
}

impl Drop for ConnectedDemandGuard<'_> {
    fn drop(&mut self) {
        if self.entered_query_depth {
            self.ledger
                .query_depth
                .set(self.ledger.query_depth.get().saturating_sub(1));
        }
        if self.root {
            verter_debug_assert!(
                self.ledger.query_depth.get() == 0,
                "connected-demand root dropped while a nested query boundary remained active"
            );
            self.ledger.active.set(false);
        }
    }
}
