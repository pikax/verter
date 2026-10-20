//! Candidate collection: the one place a relation deposits an inference
//! candidate into a collecting session, brackets an alternative with a
//! checkpoint, and rolls a losing alternative back. Pure assignability
//! (`execute_relate_pair`) never reaches these entries — it runs behind a
//! binding barrier — so a relation deposits only when the inference owner
//! opened the session it collects into ([`ProjectSemanticDispatch::relate_collecting`],
//! a call's applicability relation, or a relation root over an `infer`
//! pattern).

use super::super::dispatch_txn::{InferenceOccurrence, RelationStep};
use super::super::relation::IdentityCarrierUnwrap;
use super::super::relation_predicates::is_deferred;
use super::super::ProjectSemanticDispatch;
use super::session::{InferenceSession, SessionCheckpoint};
use crate::semantic_query::{InferenceCandidatePriority, SemanticNodeData, SemanticNodeId};

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Relate `source` to `target` collecting into the innermost session
    /// the caller opened: the inference half of a relation, where
    /// [`Self::execute_relate_pair`] is the pure half.
    pub(in crate::project_semantic_dispatch) fn relate_collecting(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        self.execute_relate(self.relate_key_for(source, target))
    }

    /// Deposit an inference candidate into the active session (a
    /// session-local delta — the deposit itself is ReturnOnly, never
    /// published). The current top frame records the delta flag ONLY when
    /// the session belongs to an OUTER frame (admission row 7); the
    /// binding root's own deposits into its OWN session do not suppress
    /// its publish (its payload carries the session's fixed bindings).
    pub(in crate::project_semantic_dispatch) fn relation_deposit(
        &self,
        param_node: SemanticNodeId,
        mut bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        // A context-sensitive literal's first-pass reading relates, but is
        // no candidate for a naked type parameter: that parameter keeps its
        // constraint for the second pass's contextual typing.
        if self
            .dispatch_txn
            .borrow()
            .call_argument_withholds(param_node, bound)
        {
            return true;
        }
        let (call_policy, deposit_is_top_level, context_keeps) = {
            let txn = self.dispatch_txn.borrow();
            (
                txn.active_session()
                    .and_then(|session| session.call_const_policy(param_node))
                    .zip(txn.call_argument_literal_mode()),
                txn.call_argument_target_is_top_level(param_node)
                    || txn.call_argument_is_whole(bound),
                txn.call_argument_context_keeps(param_node),
            )
        };
        if let Some((policy, literal_mode)) = call_policy {
            // A bare literal argument's candidate widens under the
            // inferring parameter's own const policy; an argument whose
            // authored form already pins its type deposits as authored.
            // A NAKED top-level inference position preserves a primitive
            // literal (the constraint is an upper-bound check, not a
            // widening target: `cstr<T extends string>("a")` is `"a"`);
            // nested positions — an array element, an object member —
            // widen unless the argument's contextual typing kept them — the
            // authored expression holding the literal has a literal context
            // for its contextual type (`isLiteralOfContextualType`: `{ v: "x" }`
            // against `{ v: T }` keeps `"x"` for `T extends string`), as does an object
            // argument deposited whole at a naked position whose constraint
            // gives the member a literal context.
            if literal_mode == crate::semantic_query::ArgumentLiteralMode::Widened {
                // A fresh union (a call's result whose literal members are
                // fresh, `h(1)` over `h<T>(x: T): T | undefined`) is kept
                // whole at a naked position as a fresh literal is.
                let fresh_literals: Vec<SemanticNodeId> =
                    match self.graph().node_data(bound).as_deref() {
                        Some(SemanticNodeData::Literal(_)) => vec![bound],
                        Some(SemanticNodeData::Union(members)) => members
                            .iter()
                            .copied()
                            .filter(|member| {
                                matches!(
                                    self.graph().node_data(*member).as_deref(),
                                    Some(SemanticNodeData::Literal(_))
                                )
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                let preserve_top_literal = deposit_is_top_level
                    && policy == crate::semantic_query::ConstParamPolicy::NonConst
                    && !fresh_literals.is_empty();
                if !preserve_top_literal {
                    bound = if policy == crate::semantic_query::ConstParamPolicy::NonConst
                        && !context_keeps.is_empty()
                    {
                        self.contextual_literal_candidate(bound, &context_keeps, policy)
                    } else {
                        self.call_inference_candidate(bound, policy)
                    };
                    if policy == crate::semantic_query::ConstParamPolicy::Const
                        && deposit_is_top_level
                    {
                        bound = self.const_tuple_in_context(param_node, bound);
                    }
                } else {
                    // A preserved literal at a naked position is FRESH
                    // provenance for an unconstrained parameter (the note
                    // is a no-op for a constrained one, whose preserved
                    // literal is regular).
                    if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                        for literal in fresh_literals {
                            session.note_fresh_literal_deposit(param_node, literal);
                        }
                    }
                }
            }
        }
        self.relation_deposit_candidate(param_node, bound, occurrence)
    }

    /// Deposit `bound` as it stands: the session write every candidate
    /// deposit ends in.
    pub(in crate::project_semantic_dispatch) fn relation_deposit_candidate(
        &self,
        param_node: SemanticNodeId,
        bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit(param_node, bound, occurrence.priority, occurrence.variance)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    pub(in crate::project_semantic_dispatch) fn relation_projection_target(
        &self,
        node: SemanticNodeId,
    ) -> bool {
        self.dispatch_txn
            .borrow()
            .active_session()
            .is_some_and(|session| session.is_projection_target(node))
    }

    /// Deposit the assembled reverse candidate through the same frame/session
    /// ownership gate as ordinary and projection candidates. A nested frame
    /// mutating an outer session is a session-local delta and therefore cannot
    /// publish an otherwise context-free relation payload.
    pub(in crate::project_semantic_dispatch) fn relation_reverse_aggregate_deposit(
        &self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
    ) -> bool {
        // A call argument's literals widen as the argument's own members
        // would under the inferring parameter's const policy: the mapped
        // template gives them no literal context.
        let policy = {
            let txn = self.dispatch_txn.borrow();
            txn.active_session()
                .and_then(|session| session.call_const_policy(param_node))
                .filter(|_| {
                    txn.call_argument_literal_mode()
                        == Some(crate::semantic_query::ArgumentLiteralMode::Widened)
                })
        };
        // A homomorphic mapped type is a tuple context: a tuple's reverse
        // mapping stays a tuple, its elements widened.
        let candidate = match policy {
            Some(policy) => match self.graph().node_data(candidate).as_deref() {
                Some(SemanticNodeData::Tuple { .. }) => self.widened_slice(candidate, policy),
                _ => self.call_inference_candidate(candidate, policy),
            },
            None => candidate,
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit_reverse_aggregate(param_node, candidate, priority)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    /// Deposit into a registered reverse projection. The indexed access is
    /// only a projection target; it never becomes an `Infer` declaration.
    pub(in crate::project_semantic_dispatch) fn relation_projection_deposit(
        &self,
        projection: SemanticNodeId,
        bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let bound = match self.unwrap_identity_carrier_for_relation(bound) {
            IdentityCarrierUnwrap::Concrete(bound) => bound,
            IdentityCarrierUnwrap::Unresolvable => return false,
        };
        if self.relation_subtree_contains_semantically_unresolved(bound)
            || super::super::raise::node_is_unknown_materializing_failure(self, bound)
            || super::super::raise::node_contains_semantic_miss_with_dispatch(self, bound)
                != Some(false)
        {
            return false;
        }
        let Some(bound_data) = self.graph().node_data(bound) else {
            return false;
        };
        if is_deferred(&bound_data)
            || matches!(
                bound_data.as_ref(),
                SemanticNodeData::TypeParam { .. }
                    | SemanticNodeData::Infer { .. }
                    | SemanticNodeData::InferRef { .. }
            )
        {
            return false;
        }
        drop(bound_data);
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let deposited = txn.active_session_mut().is_some_and(|session| {
            session.deposit_projection(projection, bound, occurrence.priority, occurrence.variance)
        });
        if !deposited {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    /// Whether `node` is a type parameter the active call session infers
    /// (a call's applicability rechecks what such an inference fixes).
    pub(in crate::project_semantic_dispatch) fn call_session_infers_bare(
        &self,
        node: SemanticNodeId,
    ) -> bool {
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::TypeParam { .. })
        ) && self
            .dispatch_txn
            .borrow()
            .active_session()
            .is_some_and(|session| session.call_const_policy(node).is_some())
    }

    /// Whether an inference session is currently active.
    pub(in crate::project_semantic_dispatch) fn relation_session_active(&self) -> bool {
        self.dispatch_txn.borrow().active_session().is_some()
    }

    /// Checkpoint the ACTIVE inference session's deposits (`None` when no
    /// session is active). The alternative-scoping half of the
    /// losing-alternative rule: a first-match loop over overload /
    /// signature-group alternatives brackets each alternative with a
    /// checkpoint and rolls back on failure, so a LOSING alternative's
    /// deposits never reach fixation (`{ (a: number, b: number): void;
    /// (a: string, b: string): void } extends (a: infer U, b: string) =>
    /// void` fixes `U := string`, never `number ∧ string`).
    pub(in crate::project_semantic_dispatch) fn relation_session_checkpoint(
        &self,
    ) -> Option<SessionCheckpoint> {
        self.dispatch_txn
            .borrow()
            .active_session()
            .map(InferenceSession::checkpoint)
    }

    /// Roll the ACTIVE session's deposits back to `checkpoint` (no-op when
    /// no session is active or no checkpoint was taken).
    pub(in crate::project_semantic_dispatch) fn relation_session_rollback(
        &self,
        checkpoint: &Option<SessionCheckpoint>,
    ) {
        if let Some(checkpoint) = checkpoint {
            if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                session.rollback_to(checkpoint);
            }
        }
    }
}
