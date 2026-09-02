//! [`FlowReturnResult`] — the SUCCESS carrier of a `FlowReturn` query, and
//! the derivation of its degradation verdict.
//!
//! STRUCTURAL CONFINEMENT. The two DECIDING fields — `return_type` and
//! `degradation` — are PRIVATE to THIS module, so
//! [`FlowReturnResult::new`] is the only way to set them anywhere in the
//! crate, the parent `semantic_query` module included. (`can_fall_through`
//! is `pub`: it is a plain reachability bit no admission channel reads,
//! and nothing derives from it.) The confinement matters because the
//! constructor does not merely store the caller's degradation: it folds
//! [`FlowReturnDegradation::UnresolvedValue`] in when the value reaches a
//! semantic-miss carrier the evaluation could not attribute to a position.
//!
//! Warm-vs-`ReturnOnly` admission has ONE positive authority: the flow
//! finalizer's `CompleteFlowResult` proof token
//! ([`crate::project_semantic_dispatch::flow_solve`]). A degraded verdict
//! never seals (`FlowSealError::DegradedValue`), so no proof mints for it;
//! the root build's finalizer-outcome adapter translates that refusal ONCE
//! into the build's `result_is_partial` / `cache_suppress` / `partial_reasons`
//! rails, and the universal read funnel (`fold_cache_read_rails`) propagates
//! those rails into every enclosing composition. SCC members are proof-typed
//! at the publish boundary, so an unproven member is unrepresentable in the
//! batch. The family memo's `FlowReturn` proof gate
//! (`semantic_query_memo`) is a NEGATIVE veto fence over that design: it
//! refuses a build whose `flow_completion` token is absent or mismatched —
//! and marks the refused read partial/suppressed so no parent warms around
//! the veto — but it can never PROMOTE a proofless result.
//!
//! The family memo's warm READ carries a `debug_assert!` that no degraded
//! success is ever stored; that assertion compiles out in release and is a
//! consistency check, not an admission channel.
//!
//! A struct literal is unrepresentable outside this file (`E0451`), and a
//! post-construction `result.return_type = …` is unrepresentable too
//! (`E0616`) — the one rebuild,
//! [`FlowReturnResult::with_return_type`], re-derives.

use super::{FlowReturnDegradation, SemanticNodeId};

/// The SUCCESS carrier of a `FlowReturn` query — including DEGRADED
/// successes. A no-value failure and a usable degraded value are
/// different public outcomes: a degraded success (a usable result whose
/// evaluation substituted a modeled-`any` for something it could not
/// model) returns through THIS carrier with its typed
/// [`FlowReturnDegradation`] reason and defaults to `ReturnOnly` — it
/// never warms the family memo (a later explicit fact-rooted
/// admission-table row is the only thing that could change that; none
/// exists). Only a COMPLETE, non-degraded evaluation is admitted warm;
/// unsupported, missing, budgeted, cyclic-empty, torn, or otherwise
/// NO-VALUE results are typed `FlowReturnFailure`s through
/// `ReturnOnly`. The whole-return node is canonical and
/// carrier-preserving; consumers project or publish it afterward under
/// their own mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowReturnResult {
    /// The whole-function return type (widened join of every contributor).
    ///
    /// PRIVATE for the same reason as [`Self::degradation`]: the
    /// verdict is derived FROM this node, so a post-construction
    /// reassignment would leave a stale verdict attached to a value it
    /// was never taken over. [`Self::with_return_type`] is the one
    /// rebuild, and it re-derives.
    return_type: SemanticNodeId,
    /// Whether execution can reach the end of the body without a return.
    pub can_fall_through: bool,
    /// The typed degradation reason, when the evaluation produced a
    /// USABLE result through a modeled-`any` substitution, OR when the
    /// value carries an unresolved semantic carrier. `Some` gates the
    /// result to `ReturnOnly`; `None` is the warm-admissible arm.
    ///
    /// PRIVATE by construction. The verdict gates warm admission through
    /// the proof layer alone: a `Some` here can never seal
    /// (`FlowSealError::DegradedValue`), so no `CompleteFlowResult` mints
    /// and the result is `ReturnOnly` at every admission boundary — the
    /// root build's finalizer-outcome adapter, the proof-typed SCC member
    /// batch, and the family memo's proof gate. A `None` over a value that
    /// is not actually known would therefore be a warm-admitted lie at all
    /// three. The only way to obtain a `FlowReturnResult` is [`Self::new`],
    /// which derives this field from the RESULT NODE rather than accepting
    /// a caller's word for it. See [`FlowReturnDegradation::UnresolvedValue`].
    degradation: Option<FlowReturnDegradation>,
    /// The FRESH (widening) literal constituents kept at the return's
    /// top level: the function's own authored fresh literal arms (a bare
    /// literal return, a widening-`const` read, a fresh ternary arm) and
    /// the fresh values calls it returns carried through, after the
    /// pinned-wins fold (a same-value `as const` / annotated sibling
    /// anywhere in the join cancels the value) — the checker's
    /// fresh/regular literal-type split on the inferred return. A caller
    /// VALUE position (an object member, an array element, a mutable
    /// declaration) widens exactly these; the caller's return join and a
    /// `const` binding keep them, the `const` recording them as its
    /// widening membership. Always a subset of the return's top-level
    /// literal constituents — [`Self::new`] filters by literal VALUE, so
    /// a widened or substituted rebuild can never list a value the
    /// return no longer carries.
    fresh_literal_arms: std::sync::Arc<[SemanticNodeId]>,
}

impl FlowReturnResult {
    /// THE construction point of a flow-return value.
    ///
    /// `degradation` is the evaluation's OWN observed reason (a
    /// modeled-`any` substitution it made knowingly). It is not the whole
    /// verdict: this constructor additionally inspects `return_type` and
    /// folds [`FlowReturnDegradation::UnresolvedValue`] in whenever the
    /// value REACHES a semantic-miss carrier
    /// ([`flow_return_value_is_unresolved`]). The evaluation's own reason
    /// wins when both apply (first-observed reason, deterministic).
    ///
    /// The evaluation is the AUTHORITY on its own degradation: a position
    /// whose resolver is a named downstream block contributes the typed
    /// unresolved marker and records
    /// [`FlowReturnDegradation::UnmodeledPosition`] where it stands. This
    /// fold is the BACKSTOP for the residue — a leaf lowering that answered
    /// a miss carrier inside the structure it handed back, which no
    /// position observed.
    #[must_use]
    pub(crate) fn new(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
        can_fall_through: bool,
        degradation: Option<FlowReturnDegradation>,
    ) -> Self {
        Self::new_with_fresh_literal_arms(graph, return_type, can_fall_through, degradation, &[])
    }

    /// [`Self::new`] with the join's surviving fresh literal arms. The
    /// list is FILTERED here, by literal value, to the constituents the
    /// return actually keeps at top level — the constructor owns the
    /// subset invariant exactly as it owns the degradation fold, so no
    /// caller can attach freshness to a value the join absorbed.
    #[must_use]
    pub(crate) fn new_with_fresh_literal_arms(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
        can_fall_through: bool,
        degradation: Option<FlowReturnDegradation>,
        fresh_literal_arms: &[SemanticNodeId],
    ) -> Self {
        let degradation = degradation.or_else(|| {
            flow_return_value_is_unresolved(graph, return_type)
                .then_some(FlowReturnDegradation::UnresolvedValue)
        });
        Self {
            return_type,
            can_fall_through,
            degradation,
            fresh_literal_arms: retained_fresh_literal_arms(graph, return_type, fresh_literal_arms),
        }
    }

    /// The whole-function return type.
    #[must_use]
    pub fn return_type(&self) -> SemanticNodeId {
        self.return_type
    }

    /// The typed degradation reason, or `None` for the warm-admissible arm.
    #[must_use]
    pub fn degradation(&self) -> Option<FlowReturnDegradation> {
        self.degradation
    }

    /// The surviving FRESH literal constituents at the return's top
    /// level (see the field). Empty means every kept literal is pinned.
    #[must_use]
    pub(crate) fn fresh_literal_arms(&self) -> &std::sync::Arc<[SemanticNodeId]> {
        &self.fresh_literal_arms
    }

    /// The same value with its RETURN TYPE replaced — the only
    /// field-level rebuild (the post-convergence literal widening in the
    /// SCC discharge), and it re-derives the verdict through
    /// [`Self::new_with_fresh_literal_arms`] rather than carrying the
    /// old one over. The fresh arms carry forward and re-filter against
    /// the NEW return: a widened literal or an absorbed constituent
    /// drops out of the list with the value it named.
    #[must_use]
    pub(crate) fn with_return_type(
        &self,
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
    ) -> Self {
        Self::new_with_fresh_literal_arms(
            graph,
            return_type,
            self.can_fall_through,
            self.degradation,
            &self.fresh_literal_arms,
        )
    }
}

/// The subset of `fresh_literal_arms` the return actually keeps at top
/// level, matched by literal VALUE (node data): scalar literals can
/// intern to distinct ids across lowering arenas, so an id comparison
/// would silently drop a constituent respelled by normalization.
fn retained_fresh_literal_arms(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    return_type: SemanticNodeId,
    fresh_literal_arms: &[SemanticNodeId],
) -> std::sync::Arc<[SemanticNodeId]> {
    use crate::semantic_query::SemanticNodeData;
    if fresh_literal_arms.is_empty() {
        return std::sync::Arc::from([]);
    }
    let mut top_level: Vec<SemanticNodeId> = Vec::new();
    match graph.node_data(return_type).as_deref() {
        Some(SemanticNodeData::Literal(_)) => top_level.push(return_type),
        Some(SemanticNodeData::Union(members)) => {
            for member in members.iter() {
                if matches!(
                    graph.node_data(*member).as_deref(),
                    Some(SemanticNodeData::Literal(_))
                ) {
                    top_level.push(*member);
                }
            }
        }
        _ => {}
    }
    let mut retained: Vec<SemanticNodeId> = Vec::new();
    for kept in top_level {
        let kept_data = graph.node_data(kept);
        let is_fresh = fresh_literal_arms
            .iter()
            .any(|arm| graph.node_data(*arm) == kept_data);
        if is_fresh && !retained.contains(&kept) {
            retained.push(kept);
        }
    }
    std::sync::Arc::from(retained.into_boxed_slice())
}

/// Whether a flow-return VALUE reaches a semantic-miss carrier — a node
/// whose payload says "the represented type is not known".
///
/// This is the BACKSTOP, not the authority. The authority is the
/// EVALUATION: every position whose resolver is a named downstream block
/// contributes the typed unresolved marker and records
/// [`FlowReturnDegradation::UnmodeledPosition`] at the position, so the
/// evaluation knows its own degradation without inspecting the value it
/// produced. What survives here is the case the evaluation genuinely
/// cannot attribute to a position — a leaf lowering that answered a miss
/// carrier inside the structure it handed back.
///
/// The verdict itself is
/// [`SemanticGraphStore::node_reaches_unresolved`](crate::semantic_query_memo::SemanticGraphStore::node_reaches_unresolved):
/// a memoized inductive bit over the immutable hash-consed graph, decided
/// once per node id. It carries NO budget, so it cannot report a fully
/// known value as unresolved.
fn flow_return_value_is_unresolved(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    root: SemanticNodeId,
) -> bool {
    graph.node_reaches_unresolved(root)
}
