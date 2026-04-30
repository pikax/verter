//! `MacroFieldGraphState` lazy-lowering scaffold + dispatch lower counter.
//!
//! Phase 11a domain 3 — Plan §4.10 / K1 lazy-lowering scaffold +
//! test-only `DISPATCH_LOWER_COUNTER` instrumentation.
//!
//! Per §4.10, the macro field-type rewrite path inside
//! `materialize_component_meta_field_types` is migrating from TypeExpr-walking
//! predicates to graph-native `_node` predicates. K1 introduces the field-state
//! scaffold; K2 migrates the predicate call sites; K3 ensures raise-once-at-
//! publish (lower count ≤ 2 per field).
//!
//! `DISPATCH_LOWER_COUNTER` is incremented every time a `MacroFieldGraphState`
//! performs a TypeExpr → SemanticNodeId lowering. K3's TDD test asserts this
//! stays ≤ 2 per field after the predicate-call migration.
//!
//! `node_rewrite_dirty` distinguishes lazy-lowering (for predicate inspection)
//! from graph-native rewrites that produce a NEW current_node. Per §4.10 /
//! Codex2 P1 #6, `publish()` raises ONLY when dirty=true.

#[cfg(test)]
thread_local! {
    /// Plan §4.10 / K3 — instrumentation counter for "this field-state
    /// triggered a TypeExpr -> SemanticNodeId lowering". Incremented on every
    /// `raw_node()` / `current_node()` call that actually performs a lower.
    ///
    /// Test-only; production builds elide the counter entirely (tracking
    /// adds no overhead off the test path).
    pub(crate) static DISPATCH_LOWER_COUNTER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn dispatch_lower_counter_reset() {
    DISPATCH_LOWER_COUNTER.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) fn dispatch_lower_counter_get() -> usize {
    DISPATCH_LOWER_COUNTER.with(|c| c.get())
}

#[cfg(test)]
fn dispatch_lower_counter_increment() {
    DISPATCH_LOWER_COUNTER.with(|c| c.set(c.get() + 1));
}

/// Plan §4.10 — lazy-lowering field state for the macro field-type rewrite
/// path. Carries the canonical `published_type` (TypeExpr), a memoised
/// `raw_node` for the field's original raw type, a memoised `current_node`
/// for the post-mutation state, and a `node_rewrite_dirty` flag
/// distinguishing lazy lowering from graph-native rewrites.
///
/// Lifecycle (per K1 / K2 / K3 / §4.10):
///
/// 1. Construct from `field.r#type`'s clone — `MacroFieldGraphState::new`.
/// 2. `raw_node(&raw_expr)` lazy-lowers the field's raw TypeExpr (for
///    predicates like `expr_needs_projection_rescue` that consult the raw).
/// 3. `current_node()` lazy-lowers the current `published_type` for
///    predicate inspection. Does NOT set `node_rewrite_dirty`.
/// 4. `set_current_node_rewrite(node)` records a graph-native rewrite. Sets
///    `node_rewrite_dirty = true` so `publish()` will raise on exit.
/// 5. `set_current_type(ty)` records a TypeExpr-side mutation (legacy paths
///    that haven't migrated). Invalidates the cached `current_node` and
///    clears the dirty flag (the new TypeExpr is canonical).
/// 6. `publish()` returns the final TypeExpr. When `node_rewrite_dirty`,
///    raises `current_node` back to TypeExpr; otherwise returns
///    `published_type` unchanged.
pub(crate) struct MacroFieldGraphState<'a> {
    /// Memoised lowering of the field's raw type (for predicates that
    /// inspect the original raw TypeExpr). Lazy.
    #[cfg_attr(not(test), allow(dead_code, reason = "K1 scaffold; wired in K2"))]
    raw_node: Option<crate::semantic_query::SemanticNodeId>,
    /// Memoised lowering of `published_type`. Lazy.
    current_node: Option<crate::semantic_query::SemanticNodeId>,
    /// Plan §4.10 / Codex2 P1 #6 — distinct from "current_node was lowered".
    /// Set TRUE only when a graph-native rewrite (via
    /// `set_current_node_rewrite`) produced a NEW `current_node`.
    /// `publish()` raises ONLY when this flag is set; lazy lowering for
    /// predicate inspection does not flip the flag.
    node_rewrite_dirty: bool,
    /// Canonical TypeExpr state. Updated by `set_current_type`; written
    /// back to the field via `publish()` at scope exit.
    published_type: verter_semantic::analysis::type_expr::TypeExpr,
    /// Owner scope used when lowering through dispatch.
    #[cfg_attr(not(test), allow(dead_code, reason = "K1 scaffold; wired in K2"))]
    scope: &'a str,
    /// Borrowed dispatch handle for lower / raise calls.
    dispatch: &'a crate::project_semantic_dispatch::ProjectSemanticDispatch<'a>,
}

impl<'a> MacroFieldGraphState<'a> {
    /// Construct a new field-state from a field's current `r#type` value.
    pub(crate) fn new(
        published_type: verter_semantic::analysis::type_expr::TypeExpr,
        scope: &'a str,
        dispatch: &'a crate::project_semantic_dispatch::ProjectSemanticDispatch<'a>,
    ) -> Self {
        Self {
            raw_node: None,
            current_node: None,
            node_rewrite_dirty: false,
            published_type,
            scope,
            dispatch,
        }
    }

    /// Read-only view of the canonical TypeExpr state (for callers that
    /// still consume TypeExpr via predicates not yet migrated to `_node`).
    pub(crate) fn published_type(&self) -> &verter_semantic::analysis::type_expr::TypeExpr {
        &self.published_type
    }

    /// Lazy-lower the field's raw TypeExpr to a `SemanticNodeId` in
    /// `Navigate` mode. Memoised — lowering happens at most once per state.
    /// Does NOT set `node_rewrite_dirty`.
    #[cfg_attr(not(test), allow(dead_code, reason = "K1 scaffold; wired in K2"))]
    pub(crate) fn raw_node(
        &mut self,
        raw_expr: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        if self.raw_node.is_none() {
            #[cfg(test)]
            dispatch_lower_counter_increment();
            self.raw_node = self.dispatch.lower_type_expr_in_scope_with_mode(
                self.scope,
                raw_expr,
                crate::semantic_query::ProjectionMode::Navigate,
            );
        }
        self.raw_node
    }

    /// Lazy-lower `published_type` to a `SemanticNodeId` in `Navigate`
    /// mode. Memoised — lowering happens at most once per
    /// `published_type` revision. Does NOT set `node_rewrite_dirty` — this
    /// is purely "lower for predicate inspection" lowering.
    #[cfg_attr(not(test), allow(dead_code, reason = "K1 scaffold; wired in K2"))]
    pub(crate) fn current_node(&mut self) -> Option<crate::semantic_query::SemanticNodeId> {
        if self.current_node.is_none() {
            #[cfg(test)]
            dispatch_lower_counter_increment();
            self.current_node = self.dispatch.lower_type_expr_in_scope_with_mode(
                self.scope,
                &self.published_type,
                crate::semantic_query::ProjectionMode::Navigate,
            );
        }
        self.current_node
    }

    /// Record a graph-native rewrite that produced a NEW `current_node`.
    /// Sets `node_rewrite_dirty = true` so `publish()` will raise on
    /// exit. Used by K2 callers after a graph-native operation produces
    /// a fresh node id.
    #[cfg_attr(not(test), allow(dead_code, reason = "Wired in K2"))]
    pub(crate) fn set_current_node_rewrite(&mut self, node: crate::semantic_query::SemanticNodeId) {
        self.current_node = Some(node);
        self.node_rewrite_dirty = true;
    }

    /// Record a TypeExpr-side mutation. Invalidates the cached
    /// `current_node` (the previously lowered node is now stale) and
    /// clears the `node_rewrite_dirty` flag (the new TypeExpr is
    /// canonical — `publish()` should NOT raise from a stale node).
    pub(crate) fn set_current_type(&mut self, ty: verter_semantic::analysis::type_expr::TypeExpr) {
        self.published_type = ty;
        self.current_node = None;
        self.node_rewrite_dirty = false;
    }

    /// Final exit. Returns the canonical TypeExpr. When
    /// `node_rewrite_dirty`, raises `current_node` back to TypeExpr;
    /// otherwise returns `published_type` unchanged.
    pub(crate) fn publish(self) -> verter_semantic::analysis::type_expr::TypeExpr {
        if self.node_rewrite_dirty {
            if let Some(node) = self.current_node {
                if let Some(raised) = self.dispatch.raise_node_to_type_expr(node) {
                    return raised;
                }
            }
        }
        self.published_type
    }
}
