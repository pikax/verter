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
//! THREE channels decide warm-vs-`ReturnOnly` on that one field:
//!
//! 1. the root build's `cache_suppress`
//!    (`ProjectSemanticDispatch::build_flow_return`) — the `FlowReturn`
//!    query's OWN admission;
//! 2. the SCC batch member publish;
//! 3. the sealed consumer entry's cache-read fold
//!    (`ProjectSemanticDispatch::execute_function_return_source`) — the
//!    ENCLOSING composition's admission, folded on every non-clean arm.
//!
//! The family memo's warm READ carries a `debug_assert!` that no degraded
//! success is ever stored; that assertion compiles out in release and is a
//! consistency check, NOT a fourth channel.
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
    /// PRIVATE by construction. Every one of the three admission channels
    /// that reads it — the root build's `cache_suppress`, the SCC batch
    /// publish, and the sealed consumer entry's cache-read fold — decides
    /// warm-vs-`ReturnOnly` on it alone, so a `None` over a value that is
    /// not actually known would be a warm-admitted lie at all three. The
    /// only way to obtain a `FlowReturnResult` is [`Self::new`], which
    /// derives this field from the RESULT NODE rather than accepting a
    /// caller's word for it. See [`FlowReturnDegradation::UnresolvedValue`].
    degradation: Option<FlowReturnDegradation>,
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
        let degradation = degradation.or_else(|| {
            flow_return_value_is_unresolved(graph, return_type)
                .then_some(FlowReturnDegradation::UnresolvedValue)
        });
        Self {
            return_type,
            can_fall_through,
            degradation,
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

    /// The same value with its RETURN TYPE replaced — the only
    /// field-level rebuild (the post-convergence literal widening in the
    /// SCC discharge), and it re-derives the verdict through
    /// [`Self::new`] rather than carrying the old one over.
    #[must_use]
    pub(crate) fn with_return_type(
        &self,
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        return_type: SemanticNodeId,
    ) -> Self {
        Self::new(graph, return_type, self.can_fall_through, self.degradation)
    }
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
