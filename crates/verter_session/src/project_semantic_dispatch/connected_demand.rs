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

use std::cell::Cell;

use crate::resolver_core::ResolverContext;
use crate::semantic_query::PartialReasonSet;

/// Work-unit cap for one connected semantic demand.
pub(super) const MAX_CONNECTED_PROJECTION_WORK: usize = 262_144;
/// Nested query-boundary cap for one connected semantic demand.
pub(super) const MAX_CONNECTED_QUERY_DEPTH: u16 = 24;

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
    query_depth: Cell<u16>,
    query_depth_limit: Cell<u16>,
    tripped: Cell<PartialReasonSet>,
}

impl std::fmt::Debug for ConnectedDemandLedger<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedDemandLedger")
            .field("active", &self.active.get())
            .field("work_used", &self.work_used.get())
            .field("work_limit", &self.work_limit.get())
            .field("query_depth", &self.query_depth.get())
            .field("query_depth_limit", &self.query_depth_limit.get())
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
            query_depth: Cell::new(0),
            query_depth_limit: Cell::new(MAX_CONNECTED_QUERY_DEPTH),
            tripped: Cell::new(PartialReasonSet::empty()),
        }
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
        Ok(())
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
