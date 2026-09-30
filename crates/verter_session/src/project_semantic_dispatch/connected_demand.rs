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
    CostDependency, CostIdentity, CostScope, DemandCostReceipt, LogicalUsage, ReplayRefusal,
};

use crate::resolver_core::ResolverContext;
use crate::semantic_query::PartialReasonSet;

/// The work units one structured comparison the checker records costs the
/// relation at most, on the shape its relation-complexity limit is reached
/// by: a union arm scanning a union target of object types charges the
/// alternative and its pair's worklist step (measured: 22,329 units for the
/// 20,100 comparisons of 200 reversed arms).
pub(super) const RELATION_UNITS_PER_COMPARISON: usize = 2;

/// The construction bytes one structured comparison reserves: what a pair
/// the relation relates holds live at its peak — its frame, its memo entry
/// and its proof (measured: 1.9 KB per pair relating 200 reversed object
/// arms). Provisional: the per-pair footprint and this charge are sized
/// together in the combined performance phase.
pub(super) const RELATION_PAIR_BYTES: usize = 2_048;

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
pub(super) const MAX_CONNECTED_PROJECTION_WORK: usize =
    crate::semantic_query::checker_policy::RELATION_COMPARISONS as usize
        * RELATION_UNITS_PER_COMPARISON;
/// Nested query-boundary cap for one connected semantic demand.
pub(super) const MAX_CONNECTED_QUERY_DEPTH: u16 = 24;
/// Construction-byte allowance for one connected semantic demand: the bytes
/// it reserves before it builds, never refunded within the demand, so the
/// allowance bounds what the demand holds at its peak. Provisional: sized
/// with the per-unit footprint in the combined performance phase.
pub(super) const MAX_CONNECTED_CONSTRUCTION_BYTES: usize = 512 << 20;

/// What one connected demand has charged so far: its work units and its
/// construction bytes.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DemandUsage {
    pub(crate) work: usize,
    pub(crate) bytes: usize,
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

#[cfg(test)]
thread_local! {
    /// A lower instantiation budget for the ledgers this thread installs,
    /// set by [`InstantiationBudgetForTests`].
    static INSTANTIATION_DEPTH_FOR_TESTS: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Install `limit` as the instantiation budget of every ledger this
/// thread creates until the guard drops — the production budget's code
/// path at a depth a test can reach quickly. Test-only.
#[cfg(test)]
pub(super) struct InstantiationBudgetForTests {
    previous: Option<u32>,
}

#[cfg(test)]
impl InstantiationBudgetForTests {
    pub(super) fn install(limit: u32) -> Self {
        Self {
            previous: INSTANTIATION_DEPTH_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(test)]
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
    ctx: &'a dyn ResolverContext,
}

impl<'a> DemandCancellation<'a> {
    /// Narrow the request's resolver context down to its cancellation signal.
    pub(super) fn from_context(ctx: &'a dyn ResolverContext) -> Self {
        Self { ctx }
    }

    /// Cheap cancellation checkpoint, consulted at every charge boundary.
    #[cfg_attr(feature = "test-support", track_caller)]
    fn is_cancelled(&self) -> bool {
        self.ctx.is_cancelled()
    }
}

/// Operational budget ledger for one dispatcher's connected demands.
///
/// Holds the live counters plus the configured caps. Every field is private
/// to this module: the ledger's invariants (a counter advances only inside an
/// active demand; a trip is sticky for the rest of the demand; caps change
/// only outside a demand) are enforced by the operations, not by convention.
pub(crate) struct ConnectedDemandLedger<'a> {
    cancellation: DemandCancellation<'a>,
    active: Cell<bool>,
    work_used: Cell<usize>,
    work_limit: Cell<usize>,
    bytes_used: Cell<usize>,
    bytes_limit: Cell<usize>,
    query_depth: Cell<u16>,
    query_depth_limit: Cell<u16>,
    instantiation_depth_limit: u32,
    tripped: Cell<PartialReasonSet>,
    /// The cold computations recording their exclusive cost, innermost
    /// last; `scope_open` mirrors "the stack is not empty" so a charge
    /// taken outside any recording pays one flag read.
    scopes: RefCell<Vec<CostScope>>,
    scope_open: Cell<bool>,
    /// The computations whose receipt, and its whole prerequisite closure,
    /// this connected demand has paid.
    paid: RefCell<FxHashSet<CostIdentity>>,
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
            work_limit: Cell::new(MAX_CONNECTED_PROJECTION_WORK),
            bytes_used: Cell::new(0),
            bytes_limit: Cell::new(MAX_CONNECTED_CONSTRUCTION_BYTES),
            query_depth: Cell::new(0),
            query_depth_limit: Cell::new(MAX_CONNECTED_QUERY_DEPTH),
            #[cfg(not(test))]
            instantiation_depth_limit: MAX_CONNECTED_INSTANTIATION_DEPTH,
            #[cfg(test)]
            instantiation_depth_limit: INSTANTIATION_DEPTH_FOR_TESTS
                .with(Cell::get)
                .unwrap_or(MAX_CONNECTED_INSTANTIATION_DEPTH),
            tripped: Cell::new(PartialReasonSet::empty()),
            scopes: RefCell::new(Vec::new()),
            scope_open: Cell::new(false),
            paid: RefCell::new(FxHashSet::default()),
        }
    }

    /// Whether an instantiation nested `depth` deep on the continuation
    /// runtime is within Verter's instantiation budget
    /// ([`MAX_CONNECTED_INSTANTIATION_DEPTH`]).
    pub(crate) fn instantiation_within_budget(&self, depth: u32) -> bool {
        depth <= self.instantiation_depth_limit
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
    pub(crate) fn charge(&self) -> Result<(), PartialReasonSet> {
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
    pub(crate) fn charge_units(&self, units: usize) -> Result<(), PartialReasonSet> {
        let available = self.work_available()?;
        if units > available {
            return Err(self.record_trip(PartialReasonSet::PROJECTION_WORK_LIMIT));
        }
        self.commit(units);
        Ok(())
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
    pub(crate) fn reserve_bytes(&self, bytes: usize) -> Result<(), PartialReasonSet> {
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
    pub(super) fn limit_report(&self, reasons: PartialReasonSet) -> (usize, u64, &'static str) {
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
    #[cfg(test)]
    pub(super) fn set_limits_for_tests(&self, work: usize, depth: u16) {
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
    pub(super) fn set_byte_limit_for_tests(&self, bytes: usize) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.bytes_limit.set(bytes);
    }

    /// The construction bytes the last connected demand reserved. Test-only:
    /// kept after the demand ends, and reset when the next one enters.
    #[cfg(test)]
    pub(super) fn bytes_used_for_tests(&self) -> usize {
        self.bytes_used.get()
    }

    /// The work units the last connected demand charged. Test-only: kept
    /// after the demand ends, and reset when the next one enters.
    #[cfg(test)]
    pub(super) fn work_used_for_tests(&self) -> usize {
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
/// exclusive cost and admitting warm results by replaying their receipts.
/// The memo and continuation carriers consume them; until those carriers
/// are threaded, only the substrate's own tests do.
#[cfg_attr(not(test), allow(dead_code))]
impl ConnectedDemandLedger<'_> {
    /// Open the recording of a cold computation of `identity`: until it is
    /// sealed or abandoned, every charge the demand takes is its exclusive
    /// cost, except the charges of computations opened above it.
    pub(crate) fn open_cost_scope(&self, identity: CostIdentity) {
        self.scopes.borrow_mut().push(CostScope {
            identity,
            exclusive: LogicalUsage::default(),
            prerequisites: Vec::new(),
        });
        self.scope_open.set(true);
    }

    /// Record that the open computation consumed the result `receipt`
    /// costs, at `nesting` (1 when entered as a nested demand, 0 when read
    /// in place). Recorded however the result arrived — computed, served
    /// or already paid — so the receipt being built owes it in full.
    pub(crate) fn record_prerequisite(&self, receipt: &Arc<DemandCostReceipt>, nesting: u16) {
        if let Some(scope) = self.scopes.borrow_mut().last_mut() {
            scope.prerequisites.push(CostDependency {
                receipt: Arc::clone(receipt),
                nesting,
            });
        }
    }

    /// Seal the open computation's recording: its receipt, now paid for
    /// this demand together with its closure, which it consumed paid.
    /// `None` when no computation is recording.
    pub(crate) fn seal_cost_scope(&self) -> Option<Arc<DemandCostReceipt>> {
        let scope = {
            let mut scopes = self.scopes.borrow_mut();
            let scope = scopes.pop()?;
            self.scope_open.set(!scopes.is_empty());
            scope
        };
        let receipt = DemandCostReceipt::new(scope.identity, scope.exclusive, scope.prerequisites);
        self.paid.borrow_mut().insert(receipt.identity().clone());
        Some(receipt)
    }

    /// Drop the open computation's recording without a receipt: it did not
    /// complete, so nothing it charged may be served warm as paid.
    pub(crate) fn abandon_cost_scope(&self) {
        let mut scopes = self.scopes.borrow_mut();
        scopes.pop();
        self.scope_open.set(!scopes.is_empty());
    }

    /// Whether this demand has paid `identity`'s receipt and its closure.
    pub(crate) fn is_paid(&self, identity: &CostIdentity) -> bool {
        self.paid.borrow().contains(identity)
    }

    /// Admit serving the result `receipt` costs without computing it:
    /// charge, in one admission, the exclusive usage of every computation
    /// in its closure this demand has not paid, and mark them paid. The
    /// receipt's nesting is checked against the remaining query depth even
    /// when it is paid. A refusal charges and marks nothing: the caller
    /// computes the demand instead, so its refusal names the operation that
    /// fails. Replayed charges belong to no open recording — the consumer
    /// records the receipt as a prerequisite instead.
    pub(crate) fn replay_admit(
        &self,
        receipt: &Arc<DemandCostReceipt>,
    ) -> Result<(), ReplayRefusal> {
        let tripped = self.tripped.get();
        if !tripped.is_empty() || self.cancellation.is_cancelled() {
            return Err(ReplayRefusal::Tripped);
        }
        if u32::from(self.query_depth.get()) + u32::from(receipt.depth())
            > u32::from(self.query_depth_limit.get())
        {
            return Err(ReplayRefusal::Depth);
        }
        let (unpaid, total) = {
            let paid = self.paid.borrow();
            let mut unpaid: Vec<&CostIdentity> = Vec::new();
            let mut seen: FxHashSet<&CostIdentity> = FxHashSet::default();
            let mut total = LogicalUsage::default();
            let mut stack: Vec<&Arc<DemandCostReceipt>> = vec![receipt];
            while let Some(next) = stack.pop() {
                let identity = next.identity();
                if paid.contains(identity) || !seen.insert(identity) {
                    continue;
                }
                total = total
                    .checked_add(next.exclusive())
                    .ok_or(ReplayRefusal::Work)?;
                unpaid.push(identity);
                stack.extend(
                    next.prerequisites()
                        .iter()
                        .rev()
                        .map(|dependency| &dependency.receipt),
                );
            }
            let unpaid: Vec<CostIdentity> = unpaid.into_iter().cloned().collect();
            (unpaid, total)
        };
        let work_room = self.work_limit.get().saturating_sub(self.work_used.get()) as u64;
        if total.work > work_room {
            return Err(ReplayRefusal::Work);
        }
        let byte_room = self.bytes_limit.get().saturating_sub(self.bytes_used.get()) as u64;
        if total.bytes > byte_room {
            return Err(ReplayRefusal::Bytes);
        }
        self.work_used
            .set(self.work_used.get() + total.work as usize);
        self.bytes_used
            .set(self.bytes_used.get() + total.bytes as usize);
        self.paid.borrow_mut().extend(unpaid);
        Ok(())
    }
}

/// Panic-safe lifetime of one connected semantic demand. The outermost
/// dispatch or direct projector installs the demand; nested query and worklist
/// entries join it without holding a `RefCell` borrow across semantic work.
pub(super) struct ConnectedDemandGuard<'g> {
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
