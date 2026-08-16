//! Deferred nav/operation reference requests — Vapor-owned prepass state.
//!
//! Kept private to `vapor/**` (constructed and consumed only by the Vapor
//! backend's element-state bookkeeping): no other codegen backend (VDOM,
//! SSR) or IDE path has a use for this, so it does not belong in the shared
//! `template::code_gen::types` module those backends all depend on.

/// Opaque FIFO queue of [`PendingNavRequest`]s. The shared
/// `VaporElementState` (defined in the sibling `template::code_gen::types`
/// module, alongside the VDOM/SSR/IDE backends' own state types) holds one
/// of these, but every operation that can construct, inspect, or drain a
/// [`PendingNavRequest`] variant is scoped `pub(in
/// crate::template::code_gen::vapor)` — visible only within this backend.
/// No module outside `vapor/**` can ever name `PendingNavRequest` itself or
/// observe what this queue holds; `types.rs` sees only an opaque value it
/// can default-construct and clear, matching `VaporElementState`'s own
/// pooled-recycling lifecycle.
#[derive(Debug, Default)]
pub(in crate::template::code_gen) struct PendingNavQueue(Vec<PendingNavRequest>);

impl PendingNavQueue {
    /// An empty queue. Reachable from `template::code_gen::types` (the
    /// state-pool's `VaporElementState::new()`), which is OUTSIDE
    /// `vapor/**` — this is the one operation that must be, and it reveals
    /// nothing about `PendingNavRequest`'s own shape.
    pub(in crate::template::code_gen) fn new() -> Self {
        Self(Vec::new())
    }

    /// Clears the queue while retaining its allocation. Reachable from
    /// `VaporElementState::reset()` for the same reason as [`Self::new`].
    pub(in crate::template::code_gen) fn clear(&mut self) {
        self.0.clear();
    }

    pub(in crate::template::code_gen::vapor) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(in crate::template::code_gen::vapor) fn push(&mut self, request: PendingNavRequest) {
        self.0.push(request);
    }

    /// Drains the queue, handing the caller the raw requests to resolve —
    /// vapor-only, like [`Self::push`]: the returned `Vec` names
    /// `PendingNavRequest` directly.
    pub(in crate::template::code_gen::vapor) fn take(&mut self) -> Vec<PendingNavRequest> {
        std::mem::take(&mut self.0)
    }
}

/// A deferred request to establish a nav/operation reference into THIS
/// scope's own container — queued instead of resolved immediately, because
/// this scope's own node ref (and any anchor id) cannot be assigned its real
/// number until ALL of this scope's direct children have been visited.
///
/// Confirmed directly against the vendored rc.3 `@vue/compiler-vapor`
/// source: `transformChildren`'s children loop runs to completion FIRST,
/// and only THEN does `processDynamicChildren(context)` run once — for each
/// direct child needing an anchor/parent reference, it allocates a fresh
/// anchor id (`context.increaseId()`) BEFORE the container's own memoized
/// `context.reference()` — never eagerly on the first child that asks. A
/// single-pass walker that resolves a scope's own ref the moment a child
/// requests it mints it too early whenever that scope has more than one
/// direct child needing navigation — the id ends up far too low (e.g. a
/// root `<div>` with an `if`/`for` pair getting id 5 instead of the
/// correct 10, since the trailing `<ul>` subtree's ids 5-8 hadn't been
/// consumed yet). This deferred-request queue is what avoids that.
///
/// Private to `vapor::nav_request` — only reachable through
/// [`PendingNavQueue`]'s own `pub(in crate::template::code_gen::vapor)`
/// operations, never named directly outside this backend.
#[derive(Debug)]
pub(in crate::template::code_gen::vapor) enum PendingNavRequest {
    /// A structural/component/slot-outlet child merging into this scope:
    /// `_setInsertionState(container[, anchor|index])`.
    ///
    /// The STATEMENT'S TEXT POSITION in `child_statements`/`child_nav` is
    /// NOT deferred — official's real `processDynamicChildren` only patches
    /// the numeric `.parent`/`.anchor` FIELDS on an operation object whose
    /// POSITION in `context.block.operation` was already fixed at
    /// transform-registration time (DFS visit order), confirmed directly
    /// against the vendored rc.3 source. So the caller reserves a
    /// placeholder slot at the CORRECT position immediately, and this
    /// request just remembers which slot(s) to overwrite once the real
    /// numbers are known.
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
    /// A plain wrapping element (e.g. `<header>` around a `<slot>`)
    /// establishing ITS OWN already-known ref as reachable from this
    /// (its parent) scope — `const nOwnRef = _child(container)` / chained
    /// `_next(prev)`, matching official's real single-argument nav runtime.
    OwnRef {
        own_ref: u32,
        /// Index into `child_nav` reserved for this establishment statement.
        nav_slot: usize,
    },
}
