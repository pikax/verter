//! Opt-in relation explanations: request-owned capture of why the relation
//! authority decided a judgement, leasing the operands it names.
//!
//! The module exists only under the default-off `semantic-observe` feature
//! (an OPTIONAL explanation tree in `docs/arch/semantic-observe.md`). A
//! default build compiles away the module, the dispatch's capture buffer and
//! every explanation construction; the relation answer, its bindings, the
//! recursion footprint and the transaction's assumption / discharge state
//! never depend on it.
//!
//! **Ownership.** An explanation belongs to the request that computed the
//! judgement — the [`ProjectSemanticDispatch`] capture buffer, drained by
//! [`ProjectSemanticDispatch::take_relation_explanations`] — never to the
//! shared [`crate::semantic_query_memo::SemanticGraphStore`]. The store keeps
//! no proof or key table, so a published relation's residency is its memo
//! candidate alone.
//!
//! **Leases.** Each [`LeasedOperand`] holds its operand's interned payload
//! `Arc`, so a captured explanation stays readable after the memo evicts the
//! judgement and after a document close tombstones the operand's arena slot.
//! The lease ends when the explanation drops.
//!
//! **Cold only.** A warm hit computes nothing and captures nothing: an
//! uncaptured warm answer is complete on its own, and capture never
//! manufactures history for it.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    RecursionOrBudgetCap, RelateMemoKey, RelationKind, RelationOutcome, SemanticNodeData,
    SemanticNodeId,
};

/// One relation operand, held by its interned payload rather than by a
/// store-relative id alone.
#[derive(Debug, Clone)]
pub struct LeasedOperand {
    /// The operand's id at capture time. After a document close the id may
    /// read as released in the store; [`Self::payload`] does not.
    pub id: SemanticNodeId,
    /// The operand's payload, leased for the explanation's lifetime. `None`
    /// only for an id the store never handed out.
    pub payload: Option<Arc<SemanticNodeData>>,
}

/// The two leased operands of one relation and the axis they relate on.
#[derive(Debug, Clone)]
pub struct LeasedRelatePair {
    pub source: LeasedOperand,
    pub target: LeasedOperand,
    pub relation: RelationKind,
}

/// Why the relation authority decided one judgement.
#[derive(Debug, Clone)]
pub enum RelationExplanation {
    /// The pair relates outside any coinductive cycle.
    Assignable { pair: LeasedRelatePair },
    /// The pair provably does not relate.
    NotAssignable { pair: LeasedRelatePair },
    /// A budget or recursion cap stopped the relation before it decided.
    BudgetExceeded { cap: RecursionOrBudgetCap },
    /// The pair relates because its strongly connected component discharged
    /// together under a coinductive assumption; `members` are every pair the
    /// component discharged, `pair` included.
    CoinductiveCycle {
        pair: LeasedRelatePair,
        members: Arc<[LeasedRelatePair]>,
    },
}

impl RelationExplanation {
    /// The public outcome this explanation accounts for.
    #[must_use]
    pub fn outcome(&self) -> RelationOutcome {
        match self {
            Self::Assignable { .. } | Self::CoinductiveCycle { .. } => RelationOutcome::Assignable,
            Self::NotAssignable { .. } => RelationOutcome::NotAssignable,
            Self::BudgetExceeded { cap } => RelationOutcome::BudgetExceeded(cap.kind),
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Lease `id`'s current payload.
    fn lease_operand(&self, id: SemanticNodeId) -> LeasedOperand {
        LeasedOperand {
            id,
            payload: self.graph().node_data(id),
        }
    }

    /// Lease both operands of `key`.
    pub(super) fn lease_relate_pair(&self, key: &RelateMemoKey) -> LeasedRelatePair {
        LeasedRelatePair {
            source: self.lease_operand(key.source),
            target: self.lease_operand(key.target),
            relation: key.relation,
        }
    }

    /// Append one cold judgement's explanation to this request's buffer.
    pub(super) fn record_relation_explanation(&self, explanation: RelationExplanation) {
        self.relation_explanations.borrow_mut().push(explanation);
    }

    /// Drain the explanations this request captured, in decision order.
    pub fn take_relation_explanations(&self) -> Vec<RelationExplanation> {
        std::mem::take(&mut *self.relation_explanations.borrow_mut())
    }
}
