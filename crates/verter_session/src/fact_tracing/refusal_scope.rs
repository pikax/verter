//! Typed refusal observation: the per-thread stack of
//! [`RefusalObservationScope`]s that record WHY a compute's basis refused
//! shared admission.
//!
//! The fact tracer records only a boolean plus a propagation — enough to
//! refuse a shared-cache admission, not enough to decide whether the value
//! stays usable for the rest of the request (a fenced serve and a broken
//! decl-body lease set the same boolean and classify differently). So the
//! marking chokepoint ([`super::note_non_cacheable_read_fan_out`]) also
//! records its TYPED reason into every active scope on the thread — the same
//! fan-out shape the tracer stack uses, so an inner producer's refusal is
//! observed by every enclosing scope that consumes its value. The scope is
//! deliberately independent of the fact tracer: a consumer that needs the
//! reason may run without a tracer installed.
//!
//! The classification and dominance rules are pure vocabulary in
//! [`verter_session_query::facts::reuse`]; this module only holds the
//! running thread's scopes and applies a reused value's replay to them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use verter_session_query::facts::reuse::{
    dominant_refusal, NonCacheableReadReason, RefusalReplay, ReuseClass,
};

thread_local! {
    /// The active refusal-observation scopes on this thread, innermost
    /// last. A recorded reason fans out to ALL of them, mirroring the
    /// fact-tracer stack: an inner producer's refusal is part of every
    /// enclosing consumer's basis.
    static ACTIVE_REFUSAL_SCOPES: RefCell<Vec<Rc<Cell<Option<NonCacheableReadReason>>>>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII scope that observes the typed non-cacheable reasons recorded
/// while it is active.
///
/// `!Send + !Sync` by construction (`Rc`): like the fact tracer this is
/// per-compute, per-thread state and must never cross a task boundary.
pub(crate) struct RefusalObservationScope {
    cell: Rc<Cell<Option<NonCacheableReadReason>>>,
}

impl RefusalObservationScope {
    /// Push a fresh scope onto this thread's stack.
    pub(crate) fn enter() -> Self {
        let cell = Rc::new(Cell::new(None));
        ACTIVE_REFUSAL_SCOPES.with(|scopes| scopes.borrow_mut().push(Rc::clone(&cell)));
        Self { cell }
    }

    /// The dominant reason observed so far, if any.
    pub(crate) fn observed(&self) -> Option<NonCacheableReadReason> {
        self.cell.get()
    }
}

impl Drop for RefusalObservationScope {
    fn drop(&mut self) {
        ACTIVE_REFUSAL_SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            // Pop by identity rather than by position: an unwind can drop
            // scopes out of order, and popping the wrong cell would leave
            // a dangling observer collecting another compute's refusals.
            if let Some(index) = scopes
                .iter()
                .rposition(|entry| Rc::ptr_eq(entry, &self.cell))
            {
                scopes.remove(index);
            }
        });
    }
}

/// Record a typed non-cacheable reason into every active scope. Called
/// only from the marking chokepoint so no producer can taint a value
/// without the reason being observable.
#[inline]
pub(super) fn record_refusal(reason: NonCacheableReadReason) {
    ACTIVE_REFUSAL_SCOPES.with(|scopes| {
        for cell in scopes.borrow().iter() {
            let merged = match cell.get() {
                None => reason,
                Some(existing) => dominant_refusal(existing, reason),
            };
            cell.set(Some(merged));
        }
    });
}

/// Re-apply a reused value's refusal to the current thread's tracers and
/// refusal scopes. Called on EVERY return of a reused value — cold, memo
/// hit, singleflight follower; a class that replays nothing is a no-op.
#[inline]
pub(crate) fn replay_reuse_refusal(class: &ReuseClass) {
    match class.refusal_replay() {
        Some(RefusalReplay::Reason(reason)) => super::note_non_cacheable_read_fan_out(reason),
        Some(RefusalReplay::Propagation(propagation)) => {
            super::note_non_cacheable_propagation(propagation);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transient_refusal_dominates_a_deterministic_one_in_either_order() {
        let scope = RefusalObservationScope::enter();
        record_refusal(NonCacheableReadReason::FencedServe);
        record_refusal(NonCacheableReadReason::LeaseMiss);
        assert_eq!(
            scope.observed(),
            Some(NonCacheableReadReason::LeaseMiss),
            "a transient refusal must UPGRADE a deterministic one — otherwise a bundle \
             whose basis includes a recoverable miss would be frozen for the request"
        );
        drop(scope);

        let scope = RefusalObservationScope::enter();
        record_refusal(NonCacheableReadReason::LeaseMiss);
        record_refusal(NonCacheableReadReason::FencedServe);
        assert_eq!(
            scope.observed(),
            Some(NonCacheableReadReason::LeaseMiss),
            "and a deterministic refusal must never DOWNGRADE an already-transient one"
        );
    }

    #[test]
    fn a_refusal_fans_out_to_every_enclosing_scope() {
        let outer = RefusalObservationScope::enter();
        {
            let inner = RefusalObservationScope::enter();
            record_refusal(NonCacheableReadReason::UnrootableRoute);
            assert_eq!(
                inner.observed(),
                Some(NonCacheableReadReason::UnrootableRoute)
            );
        }
        assert_eq!(
            outer.observed(),
            Some(NonCacheableReadReason::UnrootableRoute),
            "an inner producer's refusal is part of every enclosing consumer's basis — \
             observing it only innermost would let an outer compute publish a value built \
             on a refused read"
        );
        drop(outer);
        let after = RefusalObservationScope::enter();
        assert_eq!(
            after.observed(),
            None,
            "a scope that has been dropped must stop collecting — a leaked observer would \
             attribute another compute's refusal to this one"
        );
    }
}
