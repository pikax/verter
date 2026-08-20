//! Deferred nav requests — Vapor-only prepass state.
//!
//! Not shared with VDOM/SSR/IDE; does not belong in
//! `template::code_gen::types`.

/// Opaque FIFO of [`PendingNavRequest`]. `VaporElementState` holds one;
/// construct/inspect/drain stay `pub(in crate::template::code_gen::vapor)`.
/// Outside `vapor/**` the queue is an opaque default-constructible value.
#[derive(Debug, Default)]
pub(in crate::template::code_gen) struct PendingNavQueue(Vec<PendingNavRequest>);

impl PendingNavQueue {
    /// Empty queue. Reachable from `types` (`VaporElementState::new`)
    /// without naming `PendingNavRequest`.
    pub(in crate::template::code_gen) fn new() -> Self {
        Self(Vec::new())
    }

    /// Clear, retaining allocation. Reachable from `VaporElementState::reset()`.
    pub(in crate::template::code_gen) fn clear(&mut self) {
        self.0.clear();
    }

    pub(in crate::template::code_gen::vapor) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(in crate::template::code_gen::vapor) fn push(&mut self, request: PendingNavRequest) {
        self.0.push(request);
    }

    /// Drain — vapor-only; the `Vec` names `PendingNavRequest`.
    pub(in crate::template::code_gen::vapor) fn take(&mut self) -> Vec<PendingNavRequest> {
        std::mem::take(&mut self.0)
    }
}

/// Deferred nav/operation ref into this scope's container. The scope's own
/// node ref (and any anchor id) cannot be numbered until every direct child
/// has been visited.
///
/// Official rc.3: `transformChildren` completes, then `processDynamicChildren`
/// once — `increaseId()` for the anchor before memoized `reference()`. Eager
/// mint on first request is too early when more than one child needs nav
/// (root `<div>` with if/for gets id 5 instead of 10).
///
/// Private to `vapor::nav_request` via [`PendingNavQueue`].
#[derive(Debug)]
pub(in crate::template::code_gen::vapor) enum PendingNavRequest {
    /// Structural/component/slot child: `_setInsertionState(container[,
    /// anchor|index])`. Statement TEXT POSITION is not deferred — official
    /// only patches `.parent`/`.anchor` on an operation already queued at DFS
    /// visit. Caller reserves the slot immediately; this request remembers
    /// which slot to overwrite once numbers are known.
    Merge {
        dom_child_index: u32,
        has_following: bool,
        /// Index into `child_nav` reserved for the anchor's chained-nav
        /// statement — `Some` only when `has_following`.
        nav_slot: Option<usize>,
        /// Index into `child_statements` reserved for the
        /// `_setInsertionState(...)` statement.
        stmt_slot: usize,
    },
    /// Wrapping element establishing its own ref from this parent:
    /// `const nOwnRef = _child(container)` / chained `_next(prev)`.
    OwnRef {
        own_ref: u32,
        /// Index into `child_nav` reserved for this establishment statement.
        nav_slot: usize,
    },
}
