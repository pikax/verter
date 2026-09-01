//! Ordered call/construct applicability executor.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::dispatch_txn::{
    CompletedResolveCallMember, InferenceInfoSetup, InferenceSessionSetup, InferenceSessionState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
    ProvisionalSubstitution, ProvisionalVerdict, RelationStep, ResolveCallPendingState,
    ResolveCallSelection, ReturnDomainMetadata, ReturnEquationFailure, ReturnEquationMember,
    ReturnObligationIdentity, SessionId,
};
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ArgumentLiteralMode, CallArgKey, CallKind, CanonicalTypeSubstitution, ConstParamPolicy,
    ContextualInferenceMode, FreshnessKey, FunctionParam, InferenceCandidatePriority,
    InferencePassKind, NoInferMask, PrimitiveKind, ProjectionReductionContext, QueryError,
    QueryResult, ResolveCallFailure, ResolveCallKey, ResolvedCallResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SemanticQueryValue, SignatureKind, SignatureRef,
    SignatureReturnCarrier, VariancePhase,
};

pub(super) const MAX_CANDIDATES_STARTED: usize = 64;
const MAX_APPLICABILITY_RELATIONS: usize = 1_024;
const MAX_INFERENCE_DEPOSITS: usize = 1_024;
/// Recursion bound for the call-boundary deposit walk: top-level union /
/// intersection constituents plus one-level alias-instantiation
/// expansions. Running out answers NOT-top-level (the deposit widens —
/// the superset direction).
const DEPOSIT_WALK_FUEL: u8 = 16;

#[derive(Debug, Clone)]
pub(crate) enum ResolveCallStep {
    Complete(ResolvedCallResult),
    Hold(Box<ResolveCallKey>),
    Degraded(ResolveCallFailure),
}

enum ResolveCallRootClose {
    Complete(
        ResolvedCallResult,
        Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    ),
    /// A complete result whose mixed component consumed UNPROVEN
    /// flow-member values: the value flows to the caller, the build is
    /// cache-suppressed — never queued, never warm.
    CompleteReturnOnly(ResolvedCallResult),
    Degraded(ResolveCallFailure),
}

enum ResolveCallFramePop {
    Provisional(ResolveCallStep),
    RootClose(ResolveCallRootClose),
}

#[derive(Default)]
pub(super) struct CallResolutionBudget {
    candidates_started: usize,
    applicability_relations: usize,
    inference_deposits: usize,
}

impl CallResolutionBudget {
    fn start_candidate(&mut self) -> bool {
        self.candidates_started += 1;
        self.candidates_started <= MAX_CANDIDATES_STARTED
    }

    fn relation(&mut self) -> bool {
        self.applicability_relations += 1;
        self.applicability_relations <= MAX_APPLICABILITY_RELATIONS
    }

    /// Charge one unit per ACCEPTED deposit — the counter's declared unit.
    /// The count is a delta of the transaction's acceptance-site counter,
    /// taken across one binding-enabled relation.
    fn charge_accepted_deposits(&mut self, accepted: u64) -> bool {
        self.inference_deposits += accepted as usize;
        self.inference_deposits <= MAX_INFERENCE_DEPOSITS
    }
}

#[derive(Clone, Copy)]
struct CallArgument {
    node: SemanticNodeId,
    freshness_origin: SemanticNodeId,
    literal_mode: ArgumentLiteralMode,
    indefinite_spread: bool,
    /// A function-valued argument with at least one un-annotated parameter.
    /// Its provisional type is withheld from the first inference pass.
    context_sensitive: bool,
}

#[allow(clippy::large_enum_variant)]
enum CandidateVerdict {
    Selected(ResolveCallPendingState),
    Mismatch,
    Degraded(ResolveCallFailure),
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// The typed call-resolution authority: reentry hold, validated warm
    /// result, then root-family or inline mixed-obligation execution.
    pub(crate) fn execute_resolve_call(&self, key: ResolveCallKey) -> ResolveCallStep {
        // Per-request dispatch-mask trace, mirroring the cold-build choke point:
        // an INLINE call resolution (one running under an open flow or relation
        // obligation) never funnels through `execute_via_cold_build_helper`, so
        // the family's participation is recorded here — idempotent per tag, no-op
        // without an installed `RequestContext`. Without it a call resolved
        // beneath a `FlowReturn` root leaves no trace of the family that decided
        // its overload, and the audit record under-reports the query families the
        // resolution actually touched.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.record_dispatched_query_tag(
                crate::semantic_query::SemanticQueryKeyTag::ResolveCall,
            );
        }
        let identity = ObligationIdentity::ResolveCall(key.clone());
        {
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(idx) = txn.reentry().find(&identity) {
                txn.obligations.record_assumption(idx);
                return ResolveCallStep::Hold(Box::new(key));
            }
            if let Some(result) = super::dispatch_txn::provisional_resolve_call_result(
                txn.obligations.substitution(),
                &key,
            ) {
                return ResolveCallStep::Complete(result.clone());
            }
        }
        if let Some(result) = self.graph().get_resolve_call_result(self.ctx, &key) {
            return ResolveCallStep::Complete(result);
        }
        if self.dispatch_txn.borrow().obligations.decides_root() {
            self.execute_resolve_call_root(key)
        } else {
            self.execute_resolve_call_inline(key)
        }
    }

    /// Open the transparent executor frame before any call-owned inference
    /// session or argument relation. This keeps root/inline classification in
    /// the generic obligation transaction: a root ResolveCall owns the stack
    /// root, and every relation it starts is necessarily inline beneath it.
    pub(super) fn resolve_call_frame_open(&self, key: &ResolveCallKey) -> usize {
        let wants_inline_flight = !self.dispatch_txn.borrow().obligations.decides_root();
        let inline_flight = wants_inline_flight
            .then(|| self.graph().begin_inline_resolve_call_flight(key))
            .flatten();
        let mut txn = self.dispatch_txn.borrow_mut();
        let watermark = txn.obligations.pending().pending_len();
        let idx = txn.reentry_mut().push_resolve_call(key.clone(), watermark);
        if let Some(state) = txn
            .reentry_mut()
            .frame_mut_for_update(idx)
            .and_then(super::dispatch_txn::ObligationFrame::resolve_call_mut)
        {
            state.inline_flight = inline_flight;
        }
        idx
    }

    fn execute_resolve_call_inline(&self, key: ResolveCallKey) -> ResolveCallStep {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if initial_trip.is_some() {
            return ResolveCallStep::Degraded(ResolveCallFailure::Budget);
        }
        let idx = self.resolve_call_frame_open(&key);
        let outcome = self.run_resolve_call(&key);
        match self.resolve_call_frame_pop(idx, outcome, false) {
            ResolveCallFramePop::Provisional(step) => step,
            ResolveCallFramePop::RootClose(ResolveCallRootClose::Complete(result, _))
            | ResolveCallFramePop::RootClose(ResolveCallRootClose::CompleteReturnOnly(result)) => {
                ResolveCallStep::Complete(result)
            }
            ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure)) => {
                ResolveCallStep::Degraded(failure)
            }
        }
    }

    fn execute_resolve_call_root(&self, key: ResolveCallKey) -> ResolveCallStep {
        let mut publication = None;
        let read = self.execute_via_cold_build_helper_capturing_publication(
            SemanticQueryKey::ResolveCall(Box::new(key.clone())),
            &mut publication,
        );
        let step = match read.value {
            QueryResult::Value(SemanticQueryValue::ResolveCall(result)) => {
                ResolveCallStep::Complete(result.as_ref().clone())
            }
            _ => ResolveCallStep::Degraded(
                self.dispatch_txn
                    .borrow_mut()
                    .call
                    .last_root_failure
                    .take()
                    .unwrap_or(ResolveCallFailure::Undecidable),
            ),
        };
        if let Some(publication) = publication {
            self.resolve_call_drain_completed_members(&key, &publication);
        } else {
            self.relation_abort_completed_members();
        }
        step
    }

    pub(super) fn build_resolve_call(
        &self,
        key: &ResolveCallKey,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let fence = self.project_generation_signature();
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if initial_trip.is_some() {
            self.dispatch_txn.borrow_mut().call.last_root_failure =
                Some(ResolveCallFailure::Budget);
            let mut output: QueryBuildOutput<SemanticQueryValue> =
                (QueryResult::Error(QueryError::Miss), fence).into();
            output.cache_suppress = true;
            return output;
        }
        let idx = self.resolve_call_frame_open(key);
        let outcome = self.run_resolve_call(key);
        #[cfg(any(test, feature = "test-support"))]
        self.inject_unproven_flow_member_for_tests(idx);
        match self.resolve_call_frame_pop(idx, outcome, true) {
            ResolveCallFramePop::RootClose(ResolveCallRootClose::Complete(result, self_roots)) => {
                // A rootless winner has no stable occurrence to key a
                // shared entry on: the caller still receives the result,
                // but the family memo refuses it.
                let admits = crate::semantic_query::AdmissibleCallResult::admits(&result);
                let mut output = QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::ResolveCall(Arc::new(result))),
                    fence,
                ))
                .with_observed_self_roots(self_roots);
                output.cache_suppress |= !admits;
                output
            }
            ResolveCallFramePop::RootClose(ResolveCallRootClose::CompleteReturnOnly(result)) => {
                // ReturnOnly-but-public: the value was composed around a
                // mixed component whose flow members finalized UNPROVEN —
                // the caller receives it, the memo refuses admission, and
                // the frame close already marked the request partial.
                let mut output: QueryBuildOutput<SemanticQueryValue> = (
                    QueryResult::Value(SemanticQueryValue::ResolveCall(Arc::new(result))),
                    fence,
                )
                    .into();
                output.cache_suppress = true;
                output
            }
            ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure)) => {
                self.dispatch_txn.borrow_mut().call.last_root_failure = Some(failure);
                let mut output: QueryBuildOutput<SemanticQueryValue> =
                    (QueryResult::Error(QueryError::Miss), fence).into();
                output.cache_suppress = true;
                output
            }
            ResolveCallFramePop::Provisional(_) => {
                unreachable!("a machinery-root ResolveCall closes its own component")
            }
        }
    }

    /// Whether `key` is an OPEN pending member of the enclosing component
    /// — the only state in which a return-equation hold on it can be
    /// solved. A call that closed its own component has already converged.
    fn resolve_call_is_pending(&self, key: &ResolveCallKey) -> bool {
        self.dispatch_txn
            .borrow()
            .obligations
            .pending()
            .contains(&ObligationIdentity::ResolveCall(key.clone()))
    }

    fn resolve_call_pending_state(
        &self,
        key: &ResolveCallKey,
        selection: ResolveCallSelection,
        concrete_seeds: Vec<SemanticNodeId>,
        holds: Vec<ReturnObligationIdentity>,
        staged_session: Option<SessionId>,
        replay_applicability: bool,
    ) -> ResolveCallPendingState {
        let mut observed_nodes = vec![key.callee];
        observed_nodes.extend(key.receiver);
        observed_nodes.extend(key.explicit_type_args.iter().copied());
        observed_nodes.extend(key.args.iter().filter_map(|arg| match arg {
            CallArgKey::Eager { ty, .. } => Some(*ty),
            CallArgKey::ProgramExpression { .. } => None,
        }));
        let mut self_roots = self.observed_self_roots_from_nodes(observed_nodes);
        if let Some(serve) = self
            .ctx
            .ensure_indexed_ready_serve(key.point.canonical_id.as_ref())
        {
            if !self_roots
                .iter()
                .any(|(canonical, _)| canonical == &key.point.canonical_id)
            {
                self_roots.push((
                    Arc::clone(&key.point.canonical_id),
                    serve.indexed.whole_hash,
                ));
            }
        }
        ResolveCallPendingState {
            selection,
            concrete_seeds,
            holds,
            staged_session,
            replay_applicability,
            inline_flight: None,
            self_roots,
        }
    }

    fn resolve_call_frame_pop(
        &self,
        idx: usize,
        outcome: CandidateVerdict,
        machinery_root: bool,
    ) -> ResolveCallFramePop {
        let popped = self.dispatch_txn.borrow_mut().reentry_mut().pop();
        let self_cycle = popped.assumption_targets.contains(&idx);
        let pending_watermark = popped.pending_watermark;
        let budget_cap = popped.budget_cap;
        let root_key = popped
            .identity
            .as_resolve_call()
            .expect("a call code path pops a call frame")
            .clone();
        let ObligationFrameDomain::ResolveCall(call_state) = popped.domain else {
            unreachable!("a call code path pops a call frame");
        };
        let inline_flight = call_state.inline_flight;
        let outcome = if budget_cap.is_some() {
            if let CandidateVerdict::Selected(state) = &outcome {
                if let Some(session) = state.staged_session {
                    self.abandon_session(session);
                }
            }
            CandidateVerdict::Degraded(ResolveCallFailure::Budget)
        } else {
            outcome
        };
        let is_scc_root = popped.min_open_target.is_none_or(|target| target >= idx);
        if !is_scc_root {
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.propagate_lowlink(popped.min_open_target);
            return match outcome {
                CandidateVerdict::Selected(mut state) => {
                    state.inline_flight = inline_flight;
                    let step = if state.concrete_seeds.is_empty() {
                        ResolveCallStep::Hold(Box::new(root_key.clone()))
                    } else {
                        let return_type = self
                            .intern_normalized_union_or_intersection(&state.concrete_seeds, true);
                        ResolveCallStep::Complete(
                            state.selection.with_return_type(self, return_type),
                        )
                    };
                    txn.obligations.pending_mut().deposit(PendingObligation {
                        identity: ObligationIdentity::ResolveCall(root_key),
                        domain: PendingObligationDomain::ResolveCall(Box::new(state)),
                    });
                    ResolveCallFramePop::Provisional(step)
                }
                CandidateVerdict::Degraded(failure) => {
                    drop(txn);
                    self.resolve_call_abort_inline_flight(inline_flight.as_ref());
                    ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure))
                }
                CandidateVerdict::Mismatch => {
                    unreachable!("candidate mismatch is not a call outcome")
                }
            };
        }

        let mut relation_members = Vec::new();
        let mut flow_members = Vec::new();
        let mut call_members = Vec::new();
        for member in self
            .dispatch_txn
            .borrow_mut()
            .obligations
            .pending_mut()
            .drain_scc(pending_watermark)
        {
            match member.domain {
                PendingObligationDomain::Relate(state) => {
                    let (key, occurrence) = member.identity.expect_relate();
                    relation_members.push(super::relation::DrainedRelationMember {
                        key: key.clone(),
                        occurrence,
                        verdict: state.verdict,
                        session_delta: state.session_delta,
                        opened_session: state.opened_session,
                        inline_flight: state.inline_flight,
                    });
                }
                PendingObligationDomain::FlowReturn(state) => {
                    let key = member
                        .identity
                        .as_flow_return()
                        .expect("flow pending state carries a flow identity")
                        .clone();
                    flow_members.push(super::relation::DrainedFlowReturnMember {
                        key,
                        outcome: state.outcome,
                        plan_refusal: state.plan_refusal,
                        inline_flight: state.inline_flight,
                        holds: state.holds,
                        self_roots: state.self_roots,
                        materialized: state.materialized,
                        fresh_seed: state.fresh_seed,
                        flow_demand: state.flow_demand,
                        discharge: state.discharge,
                        provenance: state.provenance,
                    });
                }
                PendingObligationDomain::ResolveCall(state) => {
                    let state = *state;
                    let key = member
                        .identity
                        .as_resolve_call()
                        .expect("call pending state carries a call identity")
                        .clone();
                    call_members.push((key, state));
                }
            }
        }

        let mut root = match outcome {
            CandidateVerdict::Selected(state) => state,
            CandidateVerdict::Degraded(failure) => {
                self.abort_mixed_call_component(
                    inline_flight.as_ref(),
                    &relation_members,
                    &flow_members,
                    &call_members,
                );
                return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure));
            }
            CandidateVerdict::Mismatch => unreachable!("candidate mismatch is not a call outcome"),
        };
        root.inline_flight = inline_flight;
        let relation_substitution: ProvisionalSubstitution = relation_members
            .iter()
            .map(|member| {
                (
                    ObligationIdentity::Relate {
                        key: member.key.clone(),
                        occurrence: member.occurrence,
                    },
                    ProvisionalVerdict::Relate(super::relation::relation_step_from_pending(
                        &member.verdict,
                    )),
                )
            })
            .collect();
        if root.replay_applicability {
            match self.replay_resolve_call_pending(&root_key, &root, &relation_substitution) {
                Ok(replayed) => root = replayed,
                Err(failure) => {
                    if let Some(session) = root.staged_session {
                        self.abandon_session(session);
                    }
                    self.abort_mixed_call_component(
                        root.inline_flight.as_ref(),
                        &relation_members,
                        &flow_members,
                        &call_members,
                    );
                    return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure));
                }
            }
        }
        for position in 0..call_members.len() {
            if !call_members[position].1.replay_applicability {
                continue;
            }
            let replay = self.replay_resolve_call_pending(
                &call_members[position].0,
                &call_members[position].1,
                &relation_substitution,
            );
            match replay {
                Ok(replayed) => call_members[position].1 = replayed,
                Err(failure) => {
                    if let Some(session) = root.staged_session {
                        self.abandon_session(session);
                    }
                    self.abort_mixed_call_component(
                        root.inline_flight.as_ref(),
                        &relation_members,
                        &flow_members,
                        &call_members,
                    );
                    return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure));
                }
            }
        }

        // The mixed component discharges to ONE joint fixed point: the
        // drained flow members (callee-clause transfer, empty-cycle
        // resurrection, freshness widening) iterate against the call
        // equation — this root included — until the solved returns stop
        // moving. A flow member the discharge cannot resurrect poisons
        // the whole component: a budget failure maps to the call
        // budget, anything else is undecidable.
        let mut call_value_map: rustc_hash::FxHashMap<ResolveCallKey, SemanticNodeId> =
            rustc_hash::FxHashMap::default();
        let bound = 1 + flow_members.len() + call_members.len() + 1;
        let mut root_result = None;
        let mut call_results = Vec::new();
        let mut converged = false;
        // The observed convergence of the joint fixed point: every
        // flow-side pass the discharge runs, accumulated across passes.
        let mut observed_iterations: u32 = 0;
        for _pass in 0..bound {
            if !flow_members.is_empty() {
                let mut entries: Vec<super::dispatch_txn::FlowDischargeEntry> =
                    Vec::with_capacity(flow_members.len());
                for member in flow_members.iter() {
                    entries.push(super::dispatch_txn::FlowDischargeEntry {
                        key: member.key.clone(),
                        outcome: member.outcome.clone(),
                        holds: member.holds.clone(),
                        fresh_seed: member.fresh_seed,
                    });
                }
                let observed =
                    self.discharge_flow_component_to_fixed_point(&mut entries, &call_value_map);
                observed_iterations = observed_iterations.saturating_add(observed.iterations);
                for (member, entry) in flow_members.iter_mut().zip(entries) {
                    member.outcome = entry.outcome;
                }
            }
            for member in &flow_members {
                if let super::dispatch_txn::FlowReturnPendingOutcome::NoValue { failure, .. } =
                    &member.outcome
                {
                    if let Some(session) = root.staged_session {
                        self.abandon_session(session);
                    }
                    self.abort_mixed_call_component(
                        root.inline_flight.as_ref(),
                        &relation_members,
                        &flow_members,
                        &call_members,
                    );
                    let failure = match failure {
                        crate::semantic_query::FlowReturnFailure::Budget(_) => {
                            ResolveCallFailure::Budget
                        }
                        _ => ResolveCallFailure::Undecidable,
                    };
                    return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(failure));
                }
            }

            let flow_overrides: rustc_hash::FxHashMap<
                crate::semantic_query::FlowReturnKey,
                SemanticNodeId,
            > = flow_members
                .iter()
                .filter_map(|member| match &member.outcome {
                    super::dispatch_txn::FlowReturnPendingOutcome::EvaluatedValue(result) => {
                        Some((member.key.clone(), result.return_type()))
                    }
                    super::dispatch_txn::FlowReturnPendingOutcome::NoValue { .. } => None,
                })
                .collect();
            let mut equation = Vec::with_capacity(1 + call_members.len());
            equation.push(ReturnEquationMember {
                fresh_literal_returns: root.selection.fresh_literal_returns().to_vec(),
                identity: ReturnObligationIdentity::ResolveCall(root_key.clone()),
                concrete_seeds: root.concrete_seeds.clone(),
                holds: root.holds.clone(),
                domain: ReturnDomainMetadata::ResolveCall,
            });
            for (key, state) in &call_members {
                equation.push(ReturnEquationMember {
                    fresh_literal_returns: state.selection.fresh_literal_returns().to_vec(),
                    identity: ReturnObligationIdentity::ResolveCall(key.clone()),
                    concrete_seeds: state.concrete_seeds.clone(),
                    holds: state.holds.clone(),
                    domain: ReturnDomainMetadata::ResolveCall,
                });
            }
            let solved = match self.solve_return_equation(&equation, &flow_overrides) {
                Ok(solved) => solved,
                Err(
                    ReturnEquationFailure::EmptyCycle
                    | ReturnEquationFailure::UnresolvedOutsideHold,
                ) => {
                    if let Some(session) = root.staged_session {
                        self.abandon_session(session);
                    }
                    self.abort_mixed_call_component(
                        root.inline_flight.as_ref(),
                        &relation_members,
                        &flow_members,
                        &call_members,
                    );
                    return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(
                        ResolveCallFailure::Undecidable,
                    ));
                }
            };
            let new_root_result = root.selection.with_return_type(self, solved[0]);
            let new_results = call_members
                .iter()
                .zip(solved[1..].iter().copied())
                .map(|((key, state), return_type)| {
                    (
                        key.clone(),
                        state.clone(),
                        state.selection.with_return_type(self, return_type),
                    )
                })
                .collect::<Vec<_>>();
            let mut new_map: rustc_hash::FxHashMap<ResolveCallKey, SemanticNodeId> = new_results
                .iter()
                .map(|(key, _, result)| {
                    (
                        key.clone(),
                        super::return_equation::resolved_call_return_type(result),
                    )
                })
                .collect();
            new_map.insert(
                root_key.clone(),
                super::return_equation::resolved_call_return_type(&new_root_result),
            );
            root_result = Some(new_root_result);
            call_results = new_results;
            if new_map == call_value_map {
                converged = true;
                break;
            }
            call_value_map = new_map;
        }
        if !converged && (!call_members.is_empty() || !flow_members.is_empty()) {
            if let Some(session) = root.staged_session {
                self.abandon_session(session);
            }
            self.abort_mixed_call_component(
                root.inline_flight.as_ref(),
                &relation_members,
                &flow_members,
                &call_members,
            );
            return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(
                ResolveCallFailure::Undecidable,
            ));
        }
        let root_result =
            root_result.expect("the call-root close always solves its own root equation");

        let cyclic = self_cycle
            || !relation_members.is_empty()
            || !flow_members.is_empty()
            || !call_members.is_empty();
        let mut scc_self_roots = root.self_roots.clone();
        for (_, state, _) in &call_results {
            union_self_roots(&mut scc_self_roots, &state.self_roots);
        }
        for member in &flow_members {
            union_self_roots(&mut scc_self_roots, &member.self_roots);
        }
        let relation_nodes = relation_members
            .iter()
            .flat_map(|member| [member.key.source, member.key.target]);
        union_self_roots(
            &mut scc_self_roots,
            &self.observed_self_roots_from_nodes(relation_nodes),
        );
        {
            let txn = self.dispatch_txn.borrow();
            for member in &txn.flow.completed_members {
                union_self_roots(&mut scc_self_roots, &member.self_roots);
            }
            for member in &txn.call.completed_members {
                union_self_roots(&mut scc_self_roots, &member.self_roots);
            }
            let relation_nodes = txn
                .relation
                .completed_members
                .iter()
                .flat_map(|member| [member.key.source, member.key.target]);
            union_self_roots(
                &mut scc_self_roots,
                &self.observed_self_roots_from_nodes(relation_nodes),
            );
        }
        let discharge = self.relation_discharge_and_route(
            false,
            None,
            relation_members,
            flow_members,
            Some((root_key.clone(), root_result.clone(), root.staged_session)),
            call_results,
            cyclic,
            &super::dispatch_txn::flow_obligation_state::ObservedFlowConvergence {
                iterations: observed_iterations,
                stable: true,
            },
        );
        let discharge = match discharge {
            Ok(outcome) => outcome,
            Err(cap) => {
                if let Some(session) = root.staged_session {
                    self.abandon_session(session);
                }
                self.resolve_call_abort_inline_flight(root.inline_flight.as_ref());
                return ResolveCallFramePop::RootClose(ResolveCallRootClose::Degraded(
                    if cap.is_some() {
                        ResolveCallFailure::Budget
                    } else {
                        ResolveCallFailure::Undecidable
                    },
                ));
            }
        };
        if discharge.flow_batch_unproven {
            // The root's return equation consumed UNPROVEN flow-member
            // values (the member batch was refused at the proof gate):
            // the call's value still flows to the caller, but the root is
            // NON-ADMISSIBLE — never queued, never warm — and the
            // enclosing build/request take the same rails the inline
            // flow-root refusal folds.
            self.fold_cache_read_rails(
                true,
                true,
                crate::semantic_query::PartialReasonSet::FLOW_RETURN_UNVERIFIED
                    .union(discharge.flow_batch_partial_reasons),
            );
            if machinery_root {
                return ResolveCallFramePop::RootClose(ResolveCallRootClose::CompleteReturnOnly(
                    root_result,
                ));
            }
            self.resolve_call_abort_inline_flight(root.inline_flight.as_ref());
            return ResolveCallFramePop::Provisional(ResolveCallStep::Complete(root_result));
        }

        if !machinery_root {
            // A rootless winner has no stable occurrence to key a shared
            // entry on: it stays transaction-local, so its inline flight
            // is released instead of queued for publication.
            match crate::semantic_query::AdmissibleCallResult::new(root_result.clone()) {
                Some(result) => self.dispatch_txn.borrow_mut().call.completed_members.push(
                    CompletedResolveCallMember {
                        key: root_key,
                        result,
                        inline_flight: root.inline_flight,
                        self_roots: root.self_roots,
                    },
                ),
                None => self.resolve_call_abort_inline_flight(root.inline_flight.as_ref()),
            }
        }
        if machinery_root {
            ResolveCallFramePop::RootClose(ResolveCallRootClose::Complete(
                root_result,
                scc_self_roots,
            ))
        } else {
            ResolveCallFramePop::Provisional(ResolveCallStep::Complete(root_result))
        }
    }

    pub(super) fn commit_call_sessions(&self, sessions: &[SessionId]) -> bool {
        let mut txn = self.dispatch_txn.borrow_mut();
        let mut unique = sessions.to_vec();
        unique.sort();
        unique.dedup();
        if !unique.iter().all(|session_id| {
            txn.relation
                .sessions
                .iter()
                .find(|session| session.id == *session_id)
                .is_some_and(|session| session.state == InferenceSessionState::StagedDeterministic)
        }) {
            return false;
        }
        for session_id in unique {
            let session = txn
                .relation
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .expect("validated staged call session remains present");
            // The commit is the `StagedDeterministic → CommittedDeterministic`
            // transition itself, so it runs in EVERY build; only its verdict
            // is asserted.
            let committed = session.commit_completed();
            verter_debug_assert!(
                committed,
                "a validated staged call session commits its immutable snapshot"
            );
        }
        true
    }

    pub(super) fn resolve_call_abort_inline_flight(
        &self,
        flight: Option<&crate::semantic_query_memo::InlineResolveCallFlight>,
    ) {
        if let Some(flight) = flight {
            self.graph().abort_inline_resolve_call_flight(flight);
        }
    }

    fn abort_mixed_call_component(
        &self,
        root_flight: Option<&crate::semantic_query_memo::InlineResolveCallFlight>,
        relation_members: &[super::relation::DrainedRelationMember],
        flow_members: &[super::relation::DrainedFlowReturnMember],
        call_members: &[(ResolveCallKey, ResolveCallPendingState)],
    ) {
        self.resolve_call_abort_inline_flight(root_flight);
        for member in relation_members {
            self.relation_abort_inline_flight(member.inline_flight.as_ref());
        }
        self.flow_return_abort_drained_flights(flow_members);
        for (_, member) in call_members {
            self.resolve_call_abort_inline_flight(member.inline_flight.as_ref());
            if let Some(session) = member.staged_session {
                self.abandon_session(session);
            }
        }
    }

    /// Drain the SCC-closed member batch onto this call root's published
    /// carrier through the ONE batched publish, fenced on the root's
    /// admitted candidate. The root's admitted publish is the component's
    /// COMMIT BOUNDARY; every member published here is independently
    /// fenced backfill that stays cold — and recomputes on the next
    /// demand — if its own fence refuses.
    fn resolve_call_drain_completed_members(
        &self,
        root_key: &ResolveCallKey,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
    ) {
        let (relation_members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        self.publish_scc_member_batch(
            crate::semantic_query_memo::SccRootWitness::resolve_call(
                root_key.clone(),
                carrier.admission_seq,
            ),
            carrier,
            relation_members,
            flow_members,
            call_members,
        );
    }

    pub(super) fn replay_resolve_call_pending(
        &self,
        key: &ResolveCallKey,
        previous: &ResolveCallPendingState,
        substitution: &super::dispatch_txn::ProvisionalSubstitution,
    ) -> Result<ResolveCallPendingState, ResolveCallFailure> {
        if let Some(session) = previous.staged_session {
            self.abandon_session(session);
        }
        let saved = self
            .dispatch_txn
            .borrow_mut()
            .obligations
            .replace_substitution(substitution.clone());
        let idx = self.resolve_call_frame_open(key);
        let replay = self.run_resolve_call(key);
        let popped = self.dispatch_txn.borrow_mut().reentry_mut().pop();
        self.dispatch_txn
            .borrow_mut()
            .obligations
            .restore_substitution(saved);
        let ObligationFrameDomain::ResolveCall(frame) = popped.domain else {
            unreachable!("call replay pops its call frame")
        };
        self.resolve_call_abort_inline_flight(frame.inline_flight.as_ref());
        if popped.min_open_target.is_some() {
            if let CandidateVerdict::Selected(state) = &replay {
                if let Some(session) = state.staged_session {
                    self.abandon_session(session);
                }
            }
            self.abort_replay_pending_suffix(popped.pending_watermark);
            self.relation_abort_completed_members();
            return Err(ResolveCallFailure::Undecidable);
        }
        match replay {
            CandidateVerdict::Selected(mut state) if !state.replay_applicability => {
                state.inline_flight = previous.inline_flight.clone();
                for root in &previous.self_roots {
                    if !state
                        .self_roots
                        .iter()
                        .any(|(canonical, _)| canonical == &root.0)
                    {
                        state.self_roots.push(root.clone());
                    }
                }
                verter_debug_assert_eq!(idx, self.dispatch_txn.borrow().reentry().depth());
                Ok(state)
            }
            CandidateVerdict::Selected(_) | CandidateVerdict::Mismatch => {
                self.abort_replay_pending_suffix(popped.pending_watermark);
                self.relation_abort_completed_members();
                Err(ResolveCallFailure::Undecidable)
            }
            CandidateVerdict::Degraded(failure) => {
                self.abort_replay_pending_suffix(popped.pending_watermark);
                self.relation_abort_completed_members();
                Err(failure)
            }
        }
    }

    fn abort_replay_pending_suffix(&self, watermark: usize) {
        let pending = self
            .dispatch_txn
            .borrow_mut()
            .obligations
            .pending_mut()
            .drain_scc(watermark);
        for member in pending {
            match member.domain {
                PendingObligationDomain::Relate(state) => {
                    self.relation_abort_inline_flight(state.inline_flight.as_ref());
                }
                PendingObligationDomain::FlowReturn(state) => {
                    self.flow_return_abort_inline_flight(state.inline_flight.as_ref());
                }
                PendingObligationDomain::ResolveCall(state) => {
                    let state = *state;
                    self.resolve_call_abort_inline_flight(state.inline_flight.as_ref());
                    if let Some(session) = state.staged_session {
                        self.abandon_session(session);
                    }
                }
            }
        }
    }

    fn run_resolve_call(&self, key: &ResolveCallKey) -> CandidateVerdict {
        let graph = self.graph();
        let session_watermark = self.dispatch_txn.borrow().relation.sessions.len();
        let callee = self.substitute_canonical(key.callee, &key.context.substitution);
        let explicit_type_args: Arc<[SemanticNodeId]> = Arc::from(
            key.explicit_type_args
                .iter()
                .map(|argument| self.substitute_canonical(*argument, &key.context.substitution))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        if self.call_callee_is_dynamic_any(callee) {
            let return_type = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
            return CandidateVerdict::Selected(self.resolve_call_pending_state(
                key,
                ResolveCallSelection::DynamicAny,
                vec![return_type],
                Vec::new(),
                None,
                false,
            ));
        }

        // A candidate-set miss under a TRIPPED connected-demand ledger is
        // the budget, never evidence that the callee carries no signature.
        let non_callable = || {
            CandidateVerdict::Degraded(if self.connected_demand_tripped() {
                ResolveCallFailure::Budget
            } else {
                ResolveCallFailure::NotCallable
            })
        };
        let Some(visible) =
            self.acquire_call_candidates(callee, &explicit_type_args, key.context.resolve_env_hash)
        else {
            return non_callable();
        };
        let raw = if key.explicit_type_args.is_empty() {
            Arc::clone(&visible)
        } else {
            match self.acquire_call_candidates(
                callee,
                &Arc::<[SemanticNodeId]>::from([]),
                key.context.resolve_env_hash,
            ) {
                // Explicit type arguments DROP the candidates that cannot
                // accept them, so the raw bucket takes the same drop before
                // it is paired positionally: a ROOTLESS candidate has no
                // occurrence to pair by, and its raw form is the candidate
                // at the same position in the equally-filtered list.
                Some(raw) => Arc::from(
                    raw.iter()
                        .filter(|candidate| {
                            self.instantiate_call_candidate(candidate.node, &explicit_type_args)
                                .is_some()
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                None => return non_callable(),
            }
        };

        // Normalize only the proved ambient `Function.prototype.call`
        // occurrence. User-authored `.call` members remain ordinary methods.
        if key.kind == CallKind::Call {
            if let (Some(extracted), Some(first)) = (key.receiver, key.args.first()) {
                let first_is_spread = match first {
                    CallArgKey::Eager { spread, .. }
                    | CallArgKey::ProgramExpression { spread, .. } => *spread,
                };
                let host = self.ctx.host_for_fact_tracer_install();
                let project = host
                    .resolve_project_for_canonical(key.point.canonical_id.as_ref())
                    .and_then(|project| host.workspace().project_stable_key(project));
                if !first_is_spread
                    && project.is_some_and(|project| {
                        self.prove_prototype_call(project, &visible).is_some()
                    })
                {
                    let mut receiver_key = key.clone();
                    receiver_key.args = Arc::from(vec![first.clone()].into_boxed_slice());
                    let mut first_argument = match self.acquire_call_arguments(&receiver_key) {
                        Ok(arguments) if arguments.len() == 1 => arguments,
                        _ => return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable),
                    };
                    let mut rebased = key.clone();
                    rebased.callee = extracted;
                    rebased.receiver = Some(first_argument.remove(0).node);
                    rebased.args = Arc::from(key.args[1..].to_vec().into_boxed_slice());
                    // The authored type arguments instantiate the AMBIENT
                    // method's own binders. The extracted callable is a
                    // different function with its own (or no) binders, so
                    // the rebased call carries none.
                    rebased.explicit_type_args = Arc::from(Vec::new().into_boxed_slice());
                    let rebased_identity = rebased.clone();
                    return match self.execute_resolve_call(rebased) {
                        ResolveCallStep::Complete(result) => {
                            let return_type =
                                super::return_equation::resolved_call_return_type(&result);
                            // The rebased call is an equation HOLD only while
                            // it is still an open member of this component.
                            // One that closed its own component is already
                            // final, and its value is in hand — holding on a
                            // target the equation cannot read (a rootless
                            // winner never reaches the completed-member
                            // ledger) would degrade the outer call instead.
                            let (concrete_seeds, holds) = if self
                                .resolve_call_is_pending(&rebased_identity)
                            {
                                (
                                    Vec::new(),
                                    vec![ReturnObligationIdentity::ResolveCall(rebased_identity)],
                                )
                            } else {
                                (vec![return_type], Vec::new())
                            };
                            CandidateVerdict::Selected(self.resolve_call_pending_state(
                                key,
                                match result {
                                    ResolvedCallResult::Selected {
                                        selected,
                                        selected_signature,
                                        substitution,
                                        return_type: _,
                                        fresh_literal_returns,
                                    } => ResolveCallSelection::Selected {
                                        selected,
                                        selected_signature:
                                            super::dispatch_txn::SelectedSignature::General(
                                                selected_signature,
                                            ),
                                        substitution,
                                        fresh_literal_returns: fresh_literal_returns.to_vec(),
                                    },
                                    ResolvedCallResult::UnionSelected { selections, .. } => {
                                        ResolveCallSelection::UnionSelected {
                                            arms: selections
                                                .iter()
                                                .map(|arm| {
                                                    super::dispatch_txn::ResolveCallUnionArmSelection {
                                                        selected: arm.selected.clone(),
                                                        selected_signature: arm.selected_signature,
                                                        substitution: arm.substitution.clone(),
                                                    }
                                                })
                                                .collect(),
                                        }
                                    }
                                    ResolvedCallResult::DynamicAny { .. } => {
                                        ResolveCallSelection::DynamicAny
                                    }
                                },
                                concrete_seeds,
                                holds,
                                None,
                                false,
                            ))
                        }
                        ResolveCallStep::Hold(_) => {
                            CandidateVerdict::Degraded(ResolveCallFailure::Undecidable)
                        }
                        ResolveCallStep::Degraded(failure) => CandidateVerdict::Degraded(failure),
                    };
                }
            }
        }
        let arguments = match self.acquire_call_arguments(key) {
            Ok(arguments) => arguments,
            Err(failure) => return CandidateVerdict::Degraded(failure),
        };
        let mut budget = CallResolutionBudget::default();
        let mut saw_bucket = false;

        let consumer = crate::semantic_query::ResolveCallConsumer::witness();
        let bucket_kind = |node: SemanticNodeId| -> Option<SignatureKind> {
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Signature { kind, .. }) => Some(*kind),
                Some(SemanticNodeData::DeferredCallable(callable)) => {
                    Some(callable.parts(&consumer).kind)
                }
                _ => None,
            }
        };
        // A UNION callee is one composite union-signature group: declaration
        // order applies independently WITHIN an arm (first-applicable), arm
        // order carries no overload precedence, and the call succeeds only
        // when EVERY callable arm decides a winner — the arm returns union
        // at the close. Uncertainty in any arm degrades the whole call; an
        // arm with no bucket signature is `NotCallable`; a callable arm
        // with no applicable signature is `NoApplicableOverload`.
        let arm_count = visible
            .iter()
            .map(|candidate| candidate.arm_ordinal)
            .max()
            .map_or(1, |max| max.saturating_add(1));
        let mut arm_states: Vec<Option<ResolveCallPendingState>> =
            (0..arm_count).map(|_| None).collect();
        let mut current_arm: Option<u32> = None;
        let mut arm_saw_bucket = false;
        for (position, candidate) in visible.iter().enumerate() {
            if current_arm != Some(candidate.arm_ordinal) {
                // Close out the previous arm: a callable arm that selected
                // nothing rejects the whole union call.
                if let Some(previous_arm) = current_arm {
                    if arm_states[previous_arm as usize].is_none() {
                        return if arm_saw_bucket {
                            CandidateVerdict::Degraded(ResolveCallFailure::NoApplicableOverload)
                        } else {
                            non_callable()
                        };
                    }
                }
                current_arm = Some(candidate.arm_ordinal);
                arm_saw_bucket = false;
            }
            if arm_states[candidate.arm_ordinal as usize].is_some() {
                // This arm already selected its first-applicable winner.
                continue;
            }
            let Some(kind) = bucket_kind(candidate.node) else {
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            // The bucket filter runs BEFORE the raw pairing: a candidate in
            // the other bucket is not this call site's business, so a
            // pairing failure on it can never decide the call.
            if !call_bucket_matches(key.kind, kind) {
                continue;
            }
            // Pair the (possibly instantiated) candidate with its RAW form
            // through the content-free authored origin — instantiation
            // preserves the occurrence but mints a new graph node, so a
            // node-id pairing would lose the raw type parameters. A
            // ROOTLESS candidate has no occurrence to compare: both lists
            // are the same callee's ordered bucket, so its raw form is the
            // candidate at the same flat position IN THE SAME ARM (an
            // explicit-argument drop can re-order arms between the two
            // lists; a cross-arm pairing would recover the wrong raw
            // binders, so it fails closed as `Undecidable` instead).
            let raw_candidate = match candidate.occurrence.authored() {
                Some(occurrence) => raw.iter().find(|raw| {
                    raw.occurrence.authored() == Some(occurrence)
                        && bucket_kind(raw.node) == Some(kind)
                }),
                None => raw.get(position).filter(|raw| {
                    raw.occurrence.authored().is_none()
                        && bucket_kind(raw.node) == Some(kind)
                        && raw.arm_ordinal == candidate.arm_ordinal
                }),
            };
            let Some(raw_candidate) = raw_candidate else {
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            saw_bucket = true;
            arm_saw_bucket = true;
            if !budget.start_candidate() {
                self.abandon_call_sessions_since(session_watermark);
                return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
            }
            match self.check_call_candidate(key, candidate, raw_candidate, &arguments, &mut budget)
            {
                CandidateVerdict::Selected(result) => {
                    if arm_count == 1 {
                        return CandidateVerdict::Selected(result);
                    }
                    arm_states[candidate.arm_ordinal as usize] = Some(result);
                }
                CandidateVerdict::Mismatch => {}
                CandidateVerdict::Degraded(failure) => {
                    if failure == ResolveCallFailure::Budget {
                        self.abandon_call_sessions_since(session_watermark);
                    }
                    return CandidateVerdict::Degraded(failure);
                }
            }
        }
        // Close out the FINAL arm.
        if let Some(previous_arm) = current_arm {
            if arm_states[previous_arm as usize].is_none() {
                return if arm_saw_bucket {
                    CandidateVerdict::Degraded(ResolveCallFailure::NoApplicableOverload)
                } else {
                    non_callable()
                };
            }
        }
        if !saw_bucket {
            return non_callable();
        }
        if arm_count == 1 {
            // The single-arm loop returns its winner inline; reaching here
            // means no candidate selected.
            return CandidateVerdict::Degraded(ResolveCallFailure::NoApplicableOverload);
        }
        // Every arm decided a winner: merge into ONE composite selection.
        // An arm that saw no bucket candidate at all never reached
        // `arm_states` — that is a non-callable arm.
        let mut arm_selections = Vec::with_capacity(arm_states.len());
        for state in &arm_states {
            if state.is_none() {
                return non_callable();
            }
        }
        let mut merged_seeds = Vec::new();
        let mut merged_holds = Vec::new();
        let mut merged_replay = false;
        let mut merged_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            Vec::new();
        for state in arm_states.into_iter().flatten() {
            let ResolveCallPendingState {
                selection,
                concrete_seeds,
                holds,
                staged_session,
                replay_applicability,
                inline_flight: _,
                self_roots,
            } = state;
            // Per-arm candidate sessions are per-winner scratch: the arm's
            // substitution is already extracted onto its selection, and the
            // composite close owns no per-arm commit — abandon them exactly
            // as a relation-deferred single winner does.
            if let Some(session) = staged_session {
                self.abandon_session(session);
            }
            merged_replay |= replay_applicability;
            merged_seeds.extend(concrete_seeds);
            merged_holds.extend(holds);
            union_self_roots(&mut merged_self_roots, &self_roots);
            let arm_selection = match selection {
                ResolveCallSelection::Selected {
                    selected,
                    selected_signature,
                    substitution,
                    fresh_literal_returns: _,
                } => super::dispatch_txn::ResolveCallUnionArmSelection {
                    selected,
                    selected_signature: match selected_signature {
                        super::dispatch_txn::SelectedSignature::General(node) => node,
                        // Re-intern the sealed carrier: content-addressed
                        // interning recovers the SAME node id the candidate
                        // carried.
                        super::dispatch_txn::SelectedSignature::Deferred(callable) => self
                            .graph()
                            .intern_node(SemanticNodeData::DeferredCallable(*callable)),
                    },
                    substitution,
                },
                // Per-arm winners come only from `check_call_candidate`,
                // which selects concrete candidates.
                ResolveCallSelection::UnionSelected { .. } | ResolveCallSelection::DynamicAny => {
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable)
                }
            };
            arm_selections.push(arm_selection);
        }
        let mut merged = self.resolve_call_pending_state(
            key,
            ResolveCallSelection::UnionSelected {
                arms: arm_selections,
            },
            merged_seeds,
            merged_holds,
            None,
            merged_replay,
        );
        union_self_roots(&mut merged.self_roots, &merged_self_roots);
        CandidateVerdict::Selected(merged)
    }

    fn acquire_call_candidates(
        &self,
        callee: SemanticNodeId,
        type_args: &Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
    ) -> Option<Arc<[SignatureRef]>> {
        match self
            .execute_via_cold_build_helper(SemanticQueryKey::ResolveOverloadSet {
                callee,
                type_args: Arc::clone(type_args),
                context: crate::semantic_query::OverloadSetContext { resolve_env_hash },
            })
            .value
        {
            QueryResult::Value(SemanticQueryValue::OverloadSet(candidates)) => Some(candidates),
            _ => None,
        }
    }

    fn call_callee_is_dynamic_any(&self, mut node: SemanticNodeId) -> bool {
        let mut seen = rustc_hash::FxHashSet::default();
        while seen.insert(node) {
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any)) => return true,
                Some(SemanticNodeData::Alias(target)) => node = *target,
                _ => return false,
            }
        }
        false
    }

    fn acquire_call_arguments(
        &self,
        key: &ResolveCallKey,
    ) -> Result<Vec<CallArgument>, ResolveCallFailure> {
        let mut result = Vec::new();
        for argument in key.args.iter() {
            let context_sensitive = argument.is_context_sensitive();
            let (node, spread, literal_mode) = match argument {
                CallArgKey::Eager {
                    ty,
                    spread,
                    literal_mode,
                    ..
                } => (*ty, *spread, *literal_mode),
                CallArgKey::ProgramExpression {
                    point,
                    spread,
                    literal_mode,
                    ..
                } => {
                    let serve = self
                        .ctx
                        .ensure_indexed_ready_serve(point.canonical_id.as_ref())
                        .ok_or(ResolveCallFailure::Undecidable)?;
                    let memo = serve.indexed.shallow_state.decl_bodies();
                    let indexed_point = verter_type_expr::facts::ProgramExpressionIdentity {
                        canonical_id: Arc::clone(&point.canonical_id),
                        offset: point.offset,
                    };
                    let program_index = memo.function_program_index();
                    let record = program_index
                        .expression(&indexed_point)
                        .ok_or(ResolveCallFailure::Undecidable)?;
                    let expression = memo
                        .indexed_program_expression_ir(record)
                        .ok_or(ResolveCallFailure::Undecidable)?;
                    let node = self
                        .evaluate_indexed_value_expression_node(
                            point.canonical_id.as_ref(),
                            record.locator.contributor.owner,
                            expression.as_ref(),
                        )
                        .ok_or(ResolveCallFailure::Undecidable)?;
                    (node, *spread, *literal_mode)
                }
            };
            let freshness_origin = node;
            let node = self.substitute_canonical(node, &key.context.substitution);
            if !spread {
                result.push(CallArgument {
                    node,
                    freshness_origin,
                    literal_mode,
                    indefinite_spread: false,
                    context_sensitive,
                });
                continue;
            }
            match self.graph().node_data(node).as_deref() {
                // A REST-free tuple spread expands positionally. An
                // OPTIONAL element supplies its `T | undefined` argument
                // and still counts toward the target's maximum — the
                // TypeScript spread-argument rule: the position's value may
                // be absent, so `undefined` reaches the parameter, and a
                // non-nullish parameter rejects it. Only a tuple carrying
                // a rest element (unknown length) stays indefinite.
                Some(SemanticNodeData::Tuple { elements, .. })
                    if elements.iter().all(|element| !element.rest) =>
                {
                    let undefined = self
                        .graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                    result.extend(elements.iter().map(|element| CallArgument {
                        node: if element.optional && element.value != undefined {
                            self.intern_normalized_union_or_intersection(
                                &[element.value, undefined],
                                true,
                            )
                        } else {
                            element.value
                        },
                        freshness_origin,
                        literal_mode,
                        indefinite_spread: false,
                        context_sensitive,
                    }));
                }
                _ => result.push(CallArgument {
                    node,
                    freshness_origin,
                    literal_mode,
                    indefinite_spread: true,
                    context_sensitive,
                }),
            }
        }
        Ok(result)
    }

    fn check_call_candidate(
        &self,
        key: &ResolveCallKey,
        candidate: &SignatureRef,
        raw_candidate: &SignatureRef,
        arguments: &[CallArgument],
        budget: &mut CallResolutionBudget,
    ) -> CandidateVerdict {
        let graph = self.graph();
        let consumer = crate::semantic_query::ResolveCallConsumer::witness();
        let (params, visible_type_params) = match graph.node_data(candidate.node).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                type_parameters,
                ..
            }) => (Arc::clone(params), Arc::clone(type_parameters)),
            // The sealed index-composed carrier: applicability reads its
            // parameters and binders, never a return type — it has none.
            Some(SemanticNodeData::DeferredCallable(callable)) => {
                let parts = callable.parts(&consumer);
                (Arc::clone(parts.params), Arc::clone(parts.type_parameters))
            }
            _ => return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable),
        };
        let params: Arc<[FunctionParam]> = Arc::from(
            params
                .iter()
                .cloned()
                .map(|mut param| {
                    param.ty = self.substitute_canonical(param.ty, &key.context.substitution);
                    param
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let raw_type_params = match graph.node_data(raw_candidate.node).as_deref() {
            Some(SemanticNodeData::Signature {
                type_parameters, ..
            }) => Arc::clone(type_parameters),
            Some(SemanticNodeData::DeferredCallable(callable)) => {
                Arc::clone(callable.parts(&consumer).type_parameters)
            }
            _ => return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable),
        };
        let own_return_function = match &candidate.return_carrier {
            SignatureReturnCarrier::Function(
                verter_type_expr::facts::FunctionReturnSource::Flow(identity),
            ) => Some(self.flow_return_key_for(identity).function),
            _ => None,
        };
        let receiver_source = key
            .receiver
            .map(|receiver| self.substitute_canonical(receiver, &key.context.substitution));
        let (receiver_param, ordinary_params) = crate::semantic_query::split_this_receiver(&params);
        // A receiver-LESS call site supplies `undefined` as its receiver, so a
        // candidate whose authored `this` ACCEPTS `undefined` stays applicable
        // — `this: void` is the canonical "callable without a receiver"
        // annotation, and `unknown` / `any` / `undefined` accept it too. Both
        // receiver gates below run the same typed assignability relation an
        // explicit receiver takes, so a `this` demanding a concrete surface is
        // still rejected.
        let call_receiver = receiver_param.map(|_| {
            receiver_source.unwrap_or_else(|| {
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
            })
        });
        let rest = ordinary_params
            .last()
            .filter(|param| param.rest)
            .map(|param| RestParam {
                param,
                shape: self.rest_shape(param.ty),
            });
        let rest = rest.as_ref();
        // A GENERIC rest is one inference position over the whole trailing
        // argument list: the arguments from the rest slot onward assemble
        // into a single tuple candidate for the parameter itself.
        let generic_rest = rest.and_then(|rest| match rest.shape {
            RestShape::GenericTuple(param) => {
                Some((param, ordinary_params.len().saturating_sub(1)))
            }
            _ => None,
        });
        let (required, maximum, supports_indefinite) = call_candidate_arity(ordinary_params, rest);
        let indefinite = arguments.iter().any(|argument| argument.indefinite_spread);
        if indefinite && !supports_indefinite {
            return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
        }
        if !indefinite && arguments.len() < required {
            return CandidateVerdict::Mismatch;
        }
        if maximum.is_some_and(|maximum| arguments.len() > maximum) {
            return CandidateVerdict::Mismatch;
        }

        #[allow(clippy::useless_conversion)]
        let infer_params: Arc<[InferenceInfoSetup]> = Arc::from(
            visible_type_params
                .iter()
                .map(|decl| {
                    InferenceInfoSetup::for_call(
                        decl.param,
                        Arc::clone(&decl.name),
                        if decl.is_const {
                            ConstParamPolicy::Const
                        } else {
                            ConstParamPolicy::NonConst
                        },
                        decl.constraint.is_some(),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let setup = InferenceSessionSetup::new(
            infer_params,
            VariancePhase::Covariant,
            InferencePassKind::CallApplicability,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        );
        let session_id = self
            .dispatch_txn
            .borrow_mut()
            .push_collecting_session(setup, None);
        let checkpoint = self
            .dispatch_txn
            .borrow()
            .active_session()
            .expect("fresh call session")
            .checkpoint();
        let mut deferred_for_relation_scc = false;

        if let (Some(receiver_param), Some(receiver)) = (receiver_param, call_receiver) {
            let deposits_before = self.accepted_inference_deposits();
            let step = self.call_receiver_relation(receiver, receiver_param.ty, receiver, budget);
            if !budget
                .charge_accepted_deposits(self.accepted_inference_deposits() - deposits_before)
            {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
            }
            match step {
                RelationStep::Assignable { .. } => {}
                RelationStep::NotAssignable => {
                    return self.reject_call_candidate(session_id, &checkpoint)
                }
                RelationStep::Assumed(evidence)
                    if assumption_is_relation_only(&evidence, own_return_function.as_ref()) =>
                {
                    deferred_for_relation_scc = true;
                }
                RelationStep::BudgetExceeded(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
                }
                RelationStep::Unknown | RelationStep::Assumed(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                }
            }
        }

        if let Some((param, rest_start)) = generic_rest {
            let deposits_before = self.accepted_inference_deposits();
            let step = self.generic_rest_relation(arguments, rest_start, param, budget, true);
            if !budget
                .charge_accepted_deposits(self.accepted_inference_deposits() - deposits_before)
            {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
            }
            match step {
                RelationStep::Assignable { .. } => {}
                RelationStep::NotAssignable => {
                    return self.reject_call_candidate(session_id, &checkpoint)
                }
                RelationStep::Assumed(evidence)
                    if assumption_is_relation_only(&evidence, own_return_function.as_ref()) =>
                {
                    deferred_for_relation_scc = true;
                }
                RelationStep::BudgetExceeded(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
                }
                RelationStep::Unknown | RelationStep::Assumed(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                }
            }
        }
        for (index, argument) in arguments.iter().enumerate() {
            if generic_rest.is_some_and(|(_, rest_start)| index >= rest_start) {
                continue;
            }
            let mut canonical_evidence =
                crate::project_semantic_dispatch::canonical_algebra::CanonicalEvidence::default();
            let mapped = mapped_parameter_type(
                ordinary_params,
                rest,
                index,
                arguments.len(),
                *argument,
                graph,
                &mut canonical_evidence,
            );
            self.deposit_canonical_evidence(canonical_evidence);
            let Some(target) = mapped else {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            // A CONTEXT-SENSITIVE argument is withheld from the first
            // inference pass: an un-annotated lambda parameter lowers to
            // `any`, and depositing that `any` would beat every candidate
            // an eager argument contributes. The eager arguments fix the
            // substitution; the post-fixation pass below then checks this
            // argument's applicability under it.
            if argument.context_sensitive {
                continue;
            }
            // Applicability relates the argument's ACTUAL type. Widening is
            // an inference-RESULT rule, applied at the deposit under the
            // inferring parameter's const policy — never to the
            // assignability source.
            let source = argument.node;
            let deposits_before = self.accepted_inference_deposits();
            let step = self.call_argument_relation(
                source,
                target,
                argument.freshness_origin,
                budget,
                argument.literal_mode,
            );
            if !budget
                .charge_accepted_deposits(self.accepted_inference_deposits() - deposits_before)
            {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
            }
            match step {
                RelationStep::Assignable { .. } => {}
                RelationStep::NotAssignable => {
                    return self.reject_call_candidate(session_id, &checkpoint)
                }
                RelationStep::Assumed(evidence)
                    if assumption_is_relation_only(&evidence, own_return_function.as_ref()) =>
                {
                    deferred_for_relation_scc = true;
                }
                RelationStep::BudgetExceeded(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
                }
                RelationStep::Unknown | RelationStep::Assumed(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                }
            }
        }

        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let defaults: FxHashMap<SemanticNodeId, Option<SemanticNodeId>> = raw_type_params
            .iter()
            .map(|decl| (decl.param, decl.default))
            .collect();
        let constraints: FxHashMap<SemanticNodeId, Option<SemanticNodeId>> = raw_type_params
            .iter()
            .map(|decl| (decl.param, decl.constraint))
            .collect();
        let inputs = {
            let txn = self.dispatch_txn.borrow();
            let Some(session) = txn
                .relation
                .sessions
                .iter()
                .find(|session| session.id == session_id)
            else {
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            session.fixation_inputs()
        };
        let Some(inputs) = inputs else {
            self.abandon_session(session_id);
            return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
        };
        // Fixation forms PROVISIONAL bindings for ALL parameters first: an
        // INFERRED parameter combines its winning candidate rung; an
        // UNINFERRED one starts at its default — substituted through the
        // already-fixed prefix (a default references prior siblings only,
        // TS2744) — else `unknown`. The uninferred parameters' CONSTRAINT
        // fallbacks then solve under the FULL substitution: each sweep
        // re-substitutes every uninferred parameter's default and
        // constraint through the complete current solution, so a
        // constraint referencing a FORWARD sibling — or the parameter
        // itself — resolves to that sibling's fixed bound instead of a
        // naked binder (TypeScript's `getInferredType`: `<T extends
        // string>` with nothing to infer from is `string`, not `unknown`;
        // a default that satisfies the constraint still wins). A mutually
        // dependent clause that does not converge within the
        // clause-bounded sweep budget is the typed `Undecidable`. The
        // relation runs with binding DISABLED, so the clamp deposits
        // nothing back into the session.
        let mut fixed: Vec<crate::semantic_query::InferBinding> = Vec::with_capacity(inputs.len());
        let mut uninferred_positions: Vec<usize> = Vec::new();
        let clause_len = inputs.len();
        for (position, input) in inputs.into_iter().enumerate() {
            let bound = if !input.candidates.is_empty() {
                self.relation_combine_candidates(&input.candidates, input.variance)
            } else {
                uninferred_positions.push(position);
                defaults
                    .get(&input.param)
                    .and_then(|default| *default)
                    .map(|default| self.substitute_bindings(default, &fixed))
                    .unwrap_or(unknown)
            };
            fixed.push(crate::semantic_query::InferBinding {
                param: input.param,
                name: input.name,
                bound,
            });
        }
        if !uninferred_positions.is_empty() {
            let mut converged = false;
            // bounded-loop: at most clause-length + 1 constraint-solve sweeps; non-convergence is the typed `Undecidable` below.
            for _ in 0..=clause_len {
                let mut changed = false;
                for &position in &uninferred_positions {
                    let param = fixed[position].param;
                    let fallback = defaults
                        .get(&param)
                        .and_then(|default| *default)
                        .map(|default| self.substitute_bindings(default, &fixed))
                        .unwrap_or(unknown);
                    let bound = match constraints.get(&param).and_then(|bound| *bound) {
                        Some(constraint) => {
                            let constraint = self.substitute_bindings(constraint, &fixed);
                            match decided_call_relation(
                                self.call_relation(
                                    fallback, constraint, fallback, budget, false, false,
                                ),
                                own_return_function.as_ref(),
                            ) {
                                Ok(Some(true)) => fallback,
                                Ok(Some(false)) | Ok(None) => constraint,
                                Err(failure) => {
                                    self.abandon_session(session_id);
                                    return CandidateVerdict::Degraded(failure);
                                }
                            }
                        }
                        None => fallback,
                    };
                    if fixed[position].bound != bound {
                        fixed[position].bound = bound;
                        changed = true;
                    }
                }
                if !changed {
                    converged = true;
                    break;
                }
            }
            if !converged {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            }
        }
        let bindings = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let Some(session) = txn
                .relation
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            else {
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            session.stage_fixation_bindings(fixed)
        };
        let Some(bindings) = bindings else {
            self.abandon_session(session_id);
            return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
        };

        let mut substitution = key.context.substitution.bindings().to_vec();
        for (index, decl) in raw_type_params.iter().enumerate() {
            if let Some(argument) = key.explicit_type_args.get(index) {
                substitution.push((decl.param, *argument));
            } else if !key.explicit_type_args.is_empty() {
                let Some(default) = decl.default else {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                };
                let prefix = CanonicalTypeSubstitution::new(substitution.clone());
                substitution.push((decl.param, self.substitute_canonical(default, &prefix)));
            }
        }
        substitution.extend(
            bindings
                .iter()
                .map(|binding| (binding.param, binding.bound)),
        );
        let substitution = CanonicalTypeSubstitution::new(substitution);
        let substitution = CanonicalTypeSubstitution::new(
            substitution
                .bindings()
                .iter()
                .map(|(param, bound)| (*param, self.substitute_canonical(*bound, &substitution)))
                .collect(),
        );

        for decl in raw_type_params.iter() {
            let Some(bound) = substitution
                .bindings()
                .iter()
                .find_map(|(param, bound)| (*param == decl.param).then_some(*bound))
            else {
                continue;
            };
            if let Some(constraint) = decl.constraint {
                let constraint = self.substitute_canonical(constraint, &substitution);
                match decided_call_relation(
                    self.call_relation(bound, constraint, bound, budget, false, false),
                    own_return_function.as_ref(),
                ) {
                    Ok(Some(true)) => {}
                    Ok(Some(false)) => return self.reject_call_candidate(session_id, &checkpoint),
                    Ok(None) => deferred_for_relation_scc = true,
                    Err(failure) => {
                        self.abandon_session(session_id);
                        return CandidateVerdict::Degraded(failure);
                    }
                }
            }
        }

        if let (Some(receiver_param), Some(receiver)) = (receiver_param, call_receiver) {
            let target = self.substitute_canonical(receiver_param.ty, &substitution);
            match decided_call_relation(
                self.call_relation(receiver, target, receiver, budget, false, false),
                own_return_function.as_ref(),
            ) {
                Ok(Some(true)) => {}
                Ok(Some(false)) => return self.reject_call_candidate(session_id, &checkpoint),
                Ok(None) => deferred_for_relation_scc = true,
                Err(failure) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(failure);
                }
            }
        }
        // The rest parameter is INSTANTIATED for the recheck, so a generic
        // rest that fixation resolved is now an ordinary array / tuple rest
        // and its arguments recheck positionally. Only a rest still
        // uninstantiated after fixation stays one tuple bundle.
        let substituted_rest_param = rest.map(|rest| {
            let mut param = rest.param.clone();
            param.ty = self.substitute_canonical(param.ty, &substitution);
            param
        });
        let substituted_rest = substituted_rest_param.as_ref().map(|param| RestParam {
            param,
            shape: self.rest_shape(param.ty),
        });
        let substituted_rest = substituted_rest.as_ref();
        let deferred_generic_rest = substituted_rest.and_then(|rest| match rest.shape {
            RestShape::GenericTuple(param) => {
                Some((param, ordinary_params.len().saturating_sub(1)))
            }
            _ => None,
        });
        if let Some((param, rest_start)) = deferred_generic_rest {
            match decided_call_relation(
                self.generic_rest_relation(arguments, rest_start, param, budget, false),
                own_return_function.as_ref(),
            ) {
                Ok(Some(true)) => {}
                Ok(Some(false)) => return self.reject_call_candidate(session_id, &checkpoint),
                Ok(None) => deferred_for_relation_scc = true,
                Err(failure) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(failure);
                }
            }
        }
        for (index, argument) in arguments.iter().enumerate() {
            if deferred_generic_rest.is_some_and(|(_, rest_start)| index >= rest_start) {
                continue;
            }
            let mut canonical_evidence =
                crate::project_semantic_dispatch::canonical_algebra::CanonicalEvidence::default();
            let mapped = mapped_parameter_type(
                ordinary_params,
                substituted_rest,
                index,
                arguments.len(),
                *argument,
                graph,
                &mut canonical_evidence,
            );
            self.deposit_canonical_evidence(canonical_evidence);
            let Some(target) = mapped else {
                self.abandon_session(session_id);
                return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
            };
            let target = self.substitute_canonical(target, &substitution);
            let source = argument.node;
            match decided_call_relation(
                self.call_relation(
                    source,
                    target,
                    argument.freshness_origin,
                    budget,
                    false,
                    true,
                ),
                own_return_function.as_ref(),
            ) {
                Ok(Some(true)) => {}
                Ok(Some(false)) => return self.reject_call_candidate(session_id, &checkpoint),
                Ok(None) => deferred_for_relation_scc = true,
                Err(failure) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(failure);
                }
            }
        }

        let ordered_args = raw_type_params
            .iter()
            .map(|decl| {
                substitution
                    .bindings()
                    .iter()
                    .find_map(|(param, bound)| (*param == decl.param).then_some(*bound))
                    .unwrap_or(unknown)
            })
            .collect::<Vec<_>>();
        let selected_node = if ordered_args.is_empty() {
            candidate.node
        } else {
            self.apply_typeof_instantiation_args(raw_candidate.node, &ordered_args)
        };
        // A sealed index-composed winner keeps its deferral until the
        // shared return equation resolves the call's return; the general
        // signature is minted there, never here.
        let selected_signature = match graph.node_data(selected_node).as_deref() {
            Some(SemanticNodeData::DeferredCallable(callable)) => {
                super::dispatch_txn::SelectedSignature::Deferred(Box::new(callable.clone()))
            }
            _ => super::dispatch_txn::SelectedSignature::General(selected_node),
        };
        let mut concrete_seeds = Vec::new();
        let mut holds = Vec::new();
        let mut fresh_literal_returns = Vec::new();
        match &candidate.return_carrier {
            SignatureReturnCarrier::Declared(declared) => {
                // Per-binder freshness at the call boundary (the checker's
                // inference-widening rule, measured against the pinned
                // checker): a FRESH literal deposit widens at the call
                // UNLESS its binder appears at TOP LEVEL of the declared
                // return — the binder itself, or a union / intersection
                // constituent (`dwrap<T>(v: T): { box: T }` widens
                // `dwrap("x")` to `{ box: string }`; `unionD<T>(x: T):
                // T | null` keeps `unionD("x")` as `"x" | null`; a
                // conditional-embedded binder widens at every depth). A
                // kept deposit reaching the return through UNION structure
                // stays FRESH for the caller's value positions, while
                // intersection reduction pins it (`andD<T>(x: T): T & {}`
                // keeps `"x"` pinned everywhere). The declared annotation
                // still carries the binder, so top-levelness is read off
                // the binder occurrence — an authored sibling arm
                // spelling the deposit's literal value never matches.
                self.collect_union_top_level_fresh_bounds(
                    session_id,
                    *declared,
                    &substitution,
                    &mut fresh_literal_returns,
                );
                let widened = self.fresh_widened_substitution_outside_top_level(
                    session_id,
                    &substitution,
                    Some(*declared),
                );
                concrete_seeds.push(
                    self.substitute_canonical(*declared, widened.as_ref().unwrap_or(&substitution)),
                );
            }
            SignatureReturnCarrier::Function(source) => match source {
                verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                    let flow_key = self.flow_return_key_for_instantiation(
                        identity,
                        Arc::from(ordered_args.as_slice()),
                        substitution.clone(),
                    );
                    let pending_before = self
                        .dispatch_txn
                        .borrow()
                        .obligations
                        .pending()
                        .pending_len();
                    match self.execute_flow_return(flow_key.clone()) {
                        // A callee that closed its own SCC independently
                        // is FINAL: its return is a concrete seed of this
                        // call, not an equation edge. A naked flow return
                        // whose substituted value IS a fresh-literal
                        // argument closes on a fresh literal — the
                        // caller's return join widens it, a value
                        // position keeps it.
                        crate::semantic_query::FlowReturnStep::Complete(result)
                            if self
                                .dispatch_txn
                                .borrow()
                                .obligations
                                .pending()
                                .pending_len()
                                == pending_before =>
                        {
                            let seed =
                                self.substitute_canonical(result.return_type(), &substitution);
                            // Per-binder freshness at the call boundary,
                            // exactly as the declared-annotation arm: a
                            // fresh deposit KEPT at top level of the
                            // callee's flow return (the whole return, or
                            // a union / intersection constituent) stays
                            // — the whole-return case is fresh at the
                            // caller's return join, a union-carried one at
                            // the caller's value positions. A deposit
                            // fixed INSIDE the return's structure widens
                            // at the call boundary itself (`wrap("x")` for
                            // `wrap<T>(v: T) { return { box: v } }` is
                            // `{ box: string }` at every observed
                            // position): the callee's flow return is
                            // re-taken under the WIDENED bindings, because
                            // the instantiated flow result carries the
                            // literal with no binder left to substitute.
                            // An `as const` or explicit-type-argument
                            // binding deposits as authored, so neither
                            // path fires for it and the literal stays
                            // pinned.
                            //
                            // Top-levelness is read off the callee's
                            // UNINSTANTIATED flow return — the
                            // substitution-occurrence provenance. The
                            // instantiated result interns literals by
                            // value, so an authored literal in the
                            // callee's own return (`if (c) { return
                            // "error" } return { value: v }` called with
                            // `"error"`) is indistinguishable from the
                            // substituted deposit there; the binder-
                            // bearing structure is the only sound walk
                            // subject. A binder probe that does not close
                            // independently answers None and every fresh
                            // deposit widens — the superset direction —
                            // with the widened re-take below still
                            // keeping the literal seed when it cannot
                            // close. The probe runs ONLY when a fresh
                            // literal deposit exists to adjudicate: with
                            // none, nothing widens and nothing collects,
                            // and a recursive component's callees are
                            // never re-demanded for a question that does
                            // not arise.
                            let has_fresh_literal_deposit =
                                substitution.bindings().iter().any(|(param, bound)| {
                                    matches!(
                                        self.graph().node_data(*bound).as_deref(),
                                        Some(SemanticNodeData::Literal(_))
                                    ) && self.binding_is_fresh_literal_deposit(
                                        session_id, *param, *bound,
                                    )
                                });
                            let binder_structure = if !has_fresh_literal_deposit {
                                None
                            } else {
                                let binder_key = self.flow_return_key_for(identity);
                                let binder_pending_before = self
                                    .dispatch_txn
                                    .borrow()
                                    .obligations
                                    .pending()
                                    .pending_len();
                                match self.execute_flow_return(binder_key) {
                                    crate::semantic_query::FlowReturnStep::Complete(binder)
                                        if self
                                            .dispatch_txn
                                            .borrow()
                                            .obligations
                                            .pending()
                                            .pending_len()
                                            == binder_pending_before =>
                                    {
                                        Some(binder.return_type())
                                    }
                                    _ => None,
                                }
                            };
                            if let Some(binder_structure) = binder_structure {
                                self.collect_union_top_level_fresh_bounds(
                                    session_id,
                                    binder_structure,
                                    &substitution,
                                    &mut fresh_literal_returns,
                                );
                            }
                            // The callee's OWN authored fresh literal
                            // arms (its sealed per-constituent freshness)
                            // stay fresh across the call boundary exactly
                            // as a kept deposit does: a caller value
                            // position widens them, the caller's return
                            // join keeps them, a `const` binding records
                            // them as widening membership. Substitution
                            // never rewrites an authored literal, so the
                            // sealed ids stay valid on the instantiated
                            // seed; the sealed set is already
                            // pinned-wins-folded and top-level-filtered
                            // by its constructor.
                            for arm in result.fresh_literal_arms().iter() {
                                if !fresh_literal_returns.contains(arm) {
                                    fresh_literal_returns.push(*arm);
                                }
                            }
                            if let Some(widened_substitution) = self
                                .fresh_widened_substitution_outside_top_level(
                                    session_id,
                                    &substitution,
                                    binder_structure,
                                )
                            {
                                let widened_args: Vec<SemanticNodeId> = raw_type_params
                                    .iter()
                                    .map(|decl| {
                                        widened_substitution
                                            .bindings()
                                            .iter()
                                            .find_map(|(param, bound)| {
                                                (*param == decl.param).then_some(*bound)
                                            })
                                            .unwrap_or(unknown)
                                    })
                                    .collect();
                                let widened_key = self.flow_return_key_for_instantiation(
                                    identity,
                                    Arc::from(widened_args.as_slice()),
                                    widened_substitution.clone(),
                                );
                                let widened_pending_before = self
                                    .dispatch_txn
                                    .borrow()
                                    .obligations
                                    .pending()
                                    .pending_len();
                                match self.execute_flow_return(widened_key) {
                                    crate::semantic_query::FlowReturnStep::Complete(widened)
                                        if self
                                            .dispatch_txn
                                            .borrow()
                                            .obligations
                                            .pending()
                                            .pending_len()
                                            == widened_pending_before =>
                                    {
                                        concrete_seeds.push(self.substitute_canonical(
                                            widened.return_type(),
                                            &widened_substitution,
                                        ));
                                    }
                                    // The widened instantiation did not
                                    // close independently (a hold, a
                                    // refusal): keep the literal seed —
                                    // never narrower than before.
                                    _ => concrete_seeds.push(seed),
                                }
                            } else {
                                concrete_seeds.push(seed);
                            }
                        }
                        crate::semantic_query::FlowReturnStep::Complete(_)
                        | crate::semantic_query::FlowReturnStep::Hold(_) => {
                            holds.push(ReturnObligationIdentity::FlowReturn(flow_key));
                        }
                        crate::semantic_query::FlowReturnStep::NoValue(
                            crate::semantic_query::FlowReturnFailure::EmptyCycle,
                        ) => {
                            holds.push(ReturnObligationIdentity::FlowReturn(flow_key));
                        }
                        // A tripped work envelope on the callee's return
                        // stays a BUDGET outcome; every other degradation
                        // is undecidable applicability.
                        crate::semantic_query::FlowReturnStep::NoValue(
                            crate::semantic_query::FlowReturnFailure::Budget(_),
                        ) => {
                            self.abandon_session(session_id);
                            return CandidateVerdict::Degraded(ResolveCallFailure::Budget);
                        }
                        crate::semantic_query::FlowReturnStep::NoValue(_) => {
                            self.abandon_session(session_id);
                            return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                        }
                    }
                }
                // A signature with no recoverable return carrier is
                // undecidable applicability. So is an unraised DECLARED
                // locator: an authored return annotation raises at its
                // producer, under the signature's own binder environment,
                // and reaches the executor as
                // [`SignatureReturnCarrier::Declared`] — raising it here
                // would strip the signature's type-parameter bindings.
                verter_type_expr::facts::FunctionReturnSource::Absent
                | verter_type_expr::facts::FunctionReturnSource::Declared(_) => {
                    self.abandon_session(session_id);
                    return CandidateVerdict::Degraded(ResolveCallFailure::Undecidable);
                }
            },
        }
        if deferred_for_relation_scc {
            // The enclosing relation SCC owns re-discharge. Do not leave a
            // provisional call snapshot staged: its re-evaluation opens a
            // fresh collecting session and replays this candidate.
            self.abandon_session(session_id);
        }
        CandidateVerdict::Selected(self.resolve_call_pending_state(
            key,
            ResolveCallSelection::Selected {
                selected: candidate.occurrence.clone(),
                selected_signature,
                substitution,
                fresh_literal_returns,
            },
            concrete_seeds,
            holds,
            (!deferred_for_relation_scc).then_some(session_id),
            deferred_for_relation_scc,
        ))
    }

    /// One applicability relation. `excess_property_check` is the POSITION's
    /// policy: an ARGUMENT position checks a proven-fresh source for excess
    /// properties. A RECEIVER (`this`) position never does — TypeScript
    /// relates the receiver without excess-property checking. Neither does a
    /// post-inference constraint check: the inferred bound IS the fresh
    /// literal, and TypeScript checks a type argument against its constraint
    /// without excess-property checking.
    #[allow(clippy::too_many_arguments)]
    fn call_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut CallResolutionBudget,
        binding_enabled: bool,
        excess_property_check: bool,
    ) -> RelationStep {
        if !budget.relation() {
            self.dispatch_txn.borrow_mut().call.undecided_relations += 1;
            return RelationStep::BudgetExceeded(crate::semantic_query::RecursionOrBudgetCap {
                kind: crate::semantic_query::BudgetExceededKind::CallResolutionBudget,
                limit: MAX_APPLICABILITY_RELATIONS as u32,
            });
        }
        let mut key = self.relate_key_for(source, target);
        key.source_freshness = self.freshness_for_source_node(freshness_origin);
        key.policy.excess_property_check =
            excess_property_check && key.source_freshness == FreshnessKey::Fresh;
        let step = if binding_enabled {
            self.execute_relate(key)
        } else {
            self.dispatch_txn.borrow_mut().begin_binding_disabled();
            let step = self.execute_relate(key);
            self.dispatch_txn.borrow_mut().end_binding_disabled();
            step
        };
        // An undecided relation outcome the call consumed: the flow
        // evaluator's per-call snapshot reads this to refuse claiming a
        // relation obligation whose judgement was never decided.
        if matches!(
            step,
            RelationStep::Unknown | RelationStep::BudgetExceeded(_)
        ) {
            self.dispatch_txn.borrow_mut().call.undecided_relations += 1;
        }
        step
    }

    /// The transaction's monotonic accepted-deposit counter (bumped at
    /// every session-deposit acceptance site). Deltas across one
    /// binding-enabled relation are what the `inference_deposits` fuse
    /// charges.
    fn accepted_inference_deposits(&self) -> u64 {
        self.dispatch_txn
            .borrow()
            .relation
            .accepted_inference_deposits
    }

    fn call_argument_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut CallResolutionBudget,
        literal_mode: ArgumentLiteralMode,
    ) -> RelationStep {
        let top_level = self.top_level_type_param_targets(target);
        self.dispatch_txn
            .borrow_mut()
            .begin_call_argument(Some(literal_mode), top_level);
        let step = self.call_relation(source, target, freshness_origin, budget, true, true);
        self.dispatch_txn.borrow_mut().end_call_argument();
        step
    }

    /// The naked type-parameter positions a call argument's declared
    /// TARGET exposes at TOP LEVEL: the parameter itself, or a union /
    /// intersection arm of it (alias-transparent). A deposit into one of
    /// these positions preserves a primitive-literal candidate — the
    /// constraint is an upper-bound check, never a widening target — while
    /// a NESTED deposit (an array element, an object member) widens under
    /// the parameter's const policy.
    fn top_level_type_param_targets(&self, target: SemanticNodeId) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let mut out = Vec::new();
        let mut stack = vec![target];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::TypeParam { .. }) => out.push(node),
                Some(SemanticNodeData::Alias(inner)) => stack.push(*inner),
                Some(
                    composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                ) => {
                    let members = composite.composite_members().expect("composite arm");
                    stack.extend(members.iter().copied());
                }
                _ => {}
            }
        }
        out
    }

    /// The ONE relation a GENERIC rest parameter contributes: the trailing
    /// arguments assembled into a tuple, related against `target`. The tail
    /// is a single inference position for the parameter itself, so the
    /// parameter collects one tuple candidate rather than one candidate per
    /// argument. An empty tail assembles the empty tuple.
    ///
    /// Widening is applied PER ELEMENT here, not to the bundle: a rest
    /// position infers the tuple shape of its arguments, so the tuple must
    /// survive the deposit (the whole-candidate transform collapses a tuple
    /// to its element array, which is the rule for an authored array-literal
    /// argument, not for a rest bundle). The assembled tuple is therefore
    /// deposited as authored.
    fn generic_rest_relation(
        &self,
        arguments: &[CallArgument],
        rest_start: usize,
        target: SemanticNodeId,
        budget: &mut CallResolutionBudget,
        binding_enabled: bool,
    ) -> RelationStep {
        let graph = self.graph();
        let trailing = arguments.get(rest_start..).unwrap_or(&[]);
        let policy = binding_enabled
            .then(|| {
                self.dispatch_txn
                    .borrow()
                    .active_session()
                    .and_then(|session| session.call_const_policy(target))
            })
            .flatten();
        let element_value = |argument: &CallArgument| match policy {
            Some(policy) if argument.literal_mode == ArgumentLiteralMode::Widened => {
                self.call_inference_candidate(argument.node, policy)
            }
            _ => argument.node,
        };
        let source = graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from(
                trailing
                    .iter()
                    .map(|argument| crate::semantic_query::TupleElement {
                        label: None,
                        value: element_value(argument),
                        optional: false,
                        rest: argument.indefinite_spread,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            readonly: policy == Some(ConstParamPolicy::Const),
        });
        if !binding_enabled {
            return self.call_relation(source, target, source, budget, false, false);
        }
        self.dispatch_txn
            .borrow_mut()
            .begin_call_argument(Some(ArgumentLiteralMode::Literal), Vec::new());
        let step = self.call_relation(source, target, source, budget, true, false);
        self.dispatch_txn.borrow_mut().end_call_argument();
        step
    }

    fn call_receiver_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut CallResolutionBudget,
    ) -> RelationStep {
        self.dispatch_txn
            .borrow_mut()
            .begin_call_argument(None, Vec::new());
        let step = self.call_relation(source, target, freshness_origin, budget, true, false);
        self.dispatch_txn.borrow_mut().end_call_argument();
        step
    }

    fn reject_call_candidate(
        &self,
        session_id: SessionId,
        checkpoint: &super::dispatch_txn::SessionCheckpoint,
    ) -> CandidateVerdict {
        let mut txn = self.dispatch_txn.borrow_mut();
        if let Some(session) = txn
            .relation
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.rollback_to(checkpoint);
            session.abandon();
        }
        CandidateVerdict::Mismatch
    }

    pub(super) fn abandon_session(&self, session_id: SessionId) {
        if let Some(session) = self
            .dispatch_txn
            .borrow_mut()
            .relation
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.abandon();
        }
    }

    fn abandon_call_sessions_since(&self, watermark: usize) {
        for session in self
            .dispatch_txn
            .borrow_mut()
            .relation
            .sessions
            .iter_mut()
            .skip(watermark)
        {
            if session.context_key().pass_kind == InferencePassKind::CallApplicability
                && matches!(
                    session.state,
                    InferenceSessionState::Collecting | InferenceSessionState::StagedDeterministic
                )
            {
                session.abandon();
            }
        }
    }

    fn substitute_bindings(
        &self,
        mut node: SemanticNodeId,
        bindings: &[crate::semantic_query::InferBinding],
    ) -> SemanticNodeId {
        for binding in bindings {
            node = self.substitute_semantic_type_param(node, binding.param, binding.bound);
        }
        node
    }

    pub(crate) fn substitute_canonical(
        &self,
        mut node: SemanticNodeId,
        substitution: &CanonicalTypeSubstitution,
    ) -> SemanticNodeId {
        // bounded-loop: at most one substitution-closure pass per canonical binding.
        for _ in 0..=substitution.bindings().len() {
            let before = node;
            for (param, bound) in substitution.bindings() {
                node = self.substitute_semantic_type_param(node, *param, *bound);
            }
            if node == before {
                break;
            }
        }
        node
    }

    /// Whether one collecting session recorded `(param, bound)` as a
    /// FRESH-preserved literal deposit — the inference-time provenance
    /// distinguishing a bare-literal (or widening-`const`-read) argument
    /// from an authored pin (`as const`, an explicit type argument),
    /// which deposits as authored and never records one.
    fn binding_is_fresh_literal_deposit(
        &self,
        session_id: SessionId,
        param: SemanticNodeId,
        bound: SemanticNodeId,
    ) -> bool {
        self.dispatch_txn
            .borrow()
            .relation
            .sessions
            .iter()
            .any(|session| session.id == session_id && session.fresh_literal_deposit(param, bound))
    }

    /// Whether one fresh-literal deposit is KEPT at the call boundary:
    /// its BINDER appears at TOP LEVEL of the binder-bearing return
    /// structure — the node itself, or a union / intersection
    /// constituent, walked through alias instantiations, recursively.
    /// This is the checker's inference-widening exemption boundary
    /// (measured against the pinned checker): a binder reachable only
    /// through deeper structure — an object member, an array element, a
    /// CONDITIONAL branch at any depth — widens at the call boundary.
    ///
    /// The match is binder identity, NEVER the deposited literal's value:
    /// scalar literals intern by value, so an authored sibling arm
    /// spelling the same literal (`run<T>(v: T): { status: "ok";
    /// value: T } | "error"` called with `"error"`) would otherwise
    /// borrow the deposit's freshness and pin the member (`value:
    /// "error"`) where the checker widens it (`value: string`). Callers
    /// therefore always hand this walk a structure the binder still
    /// occurs in — the declared annotation, or the callee's
    /// uninstantiated flow return — never an instantiated result.
    fn deposit_at_top_level(&self, structure: SemanticNodeId, param: SemanticNodeId) -> bool {
        self.deposit_walk_reaches_binder(structure, param, false, DEPOSIT_WALK_FUEL)
    }

    /// The shared walk behind [`Self::deposit_at_top_level`] and
    /// [`Self::deposit_at_union_top_level`]. `union_only` excludes
    /// intersection constituents (the union half's measured rule). An
    /// alias instantiation at top level is TRANSPARENT: `type UA<T> =
    /// T | undefined` keeps `fu<T>(v: T): UA<T>`'s deposit exactly as
    /// the spelled-out union does (measured), so the walk expands one
    /// level through the shared `Instantiate` demand and continues.
    /// `fuel` bounds alias chains and degenerate structures; running out
    /// answers NOT-top-level, which widens — the superset direction.
    fn deposit_walk_reaches_binder(
        &self,
        structure: SemanticNodeId,
        param: SemanticNodeId,
        union_only: bool,
        fuel: u8,
    ) -> bool {
        if self.nodes_are_same_binder(structure, param) {
            return true;
        }
        let Some(fuel) = fuel.checked_sub(1) else {
            return false;
        };
        match self.graph().node_data(structure).as_deref() {
            Some(SemanticNodeData::Union(members)) => members
                .iter()
                .any(|member| self.deposit_walk_reaches_binder(*member, param, union_only, fuel)),
            Some(SemanticNodeData::Intersection(members)) if !union_only => members
                .iter()
                .any(|member| self.deposit_walk_reaches_binder(*member, param, union_only, fuel)),
            Some(SemanticNodeData::InstantiationRef { .. }) => self
                .expand_alias_instantiation_one_level(structure)
                .is_some_and(|expanded| {
                    self.deposit_walk_reaches_binder(expanded, param, union_only, fuel)
                }),
            _ => false,
        }
    }

    /// Whether `structure` IS the deposit's binder. Node identity first;
    /// otherwise two `TypeParam` nodes naming the SAME declaration slot
    /// are the same binder. The signature environment and the flow-return
    /// environment intern the declaration's parameter under two minting
    /// modes: the signature clause anchors the owning symbol into
    /// `decl_name` while the flow binder env mints the bare parameter
    /// name — so besides full `(decl, param_index)` equality, a flow-env
    /// bare mint (its `decl_name` IS its display name) matches the
    /// signature param when every remaining declaration coordinate
    /// (canonical file, owner, whole hash, clause ordinal, declared
    /// parameter name) agrees. The composed identity is never parsed
    /// back out of the anchored mint, and this is declaration
    /// provenance, never a value match: distinct files, versions, clause
    /// positions, or parameter names never compare equal.
    fn nodes_are_same_binder(&self, structure: SemanticNodeId, param: SemanticNodeId) -> bool {
        if structure == param {
            return true;
        }
        let graph = self.graph();
        let structure_data = graph.node_data(structure);
        let param_data = graph.node_data(param);
        match (structure_data.as_deref(), param_data.as_deref()) {
            (
                Some(SemanticNodeData::TypeParam {
                    decl: structure_decl,
                    param_index: structure_index,
                    display_name: structure_display,
                    ..
                }),
                Some(SemanticNodeData::TypeParam {
                    decl: param_decl,
                    param_index: param_index_value,
                    display_name: param_display,
                    ..
                }),
            ) => {
                structure_index == param_index_value
                    && (structure_decl == param_decl
                        || (structure_decl.canonical_id == param_decl.canonical_id
                            && structure_decl.owner == param_decl.owner
                            && structure_decl.whole_hash == param_decl.whole_hash
                            && structure_display == param_display
                            && structure_decl.decl_name == *structure_display))
            }
            _ => false,
        }
    }

    /// One-level alias-instantiation expansion, through the shared
    /// `Instantiate` demand (the same one-level materialisation the
    /// relation authority uses) — never a private resolver. Serves the
    /// call-boundary deposit walk (an alias to a union is transparent to
    /// the top-level exemption) and the flow return join's union
    /// flattening. `None` when the demand does not produce a distinct
    /// node; callers keep the carrier (the walk answers NOT-top-level
    /// and the deposit widens).
    pub(super) fn expand_alias_instantiation_one_level(
        &self,
        structure: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let graph = self.graph();
        let data = graph.node_data(structure);
        let Some(SemanticNodeData::InstantiationRef { base, args }) = data.as_deref() else {
            return None;
        };
        let oracle_demand = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let owner_canonical = Arc::clone(&base.canonical_id);
        let slot = self.type_slot_for(
            Arc::clone(&base.canonical_id),
            base.owner,
            Arc::clone(&base.decl_name),
        );
        let args = Arc::clone(args);
        let read = self.execute_read(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                args,
                self.instantiate_context_for(&owner_canonical, oracle_demand),
            ),
        ));
        crate::request_context::observe_component_meta_read_suppress(&read);
        match read.value {
            QueryResult::Value(id) if id != structure => Some(id),
            _ => None,
        }
    }

    /// Collect the fresh-literal deposits whose BINDER reaches
    /// `structure`'s top level through UNION structure only (the binder
    /// itself, or a union constituent, walked through alias
    /// instantiations, recursively) into `fresh_literal_returns`. These
    /// stay FRESH on the call's return: a caller's value (member)
    /// position widens them, and the return join widens the whole return
    /// when it IS one of them. Intersection constituents are
    /// deliberately excluded — the checker's intersection reduction pins
    /// the literal (measured: `{ a: andD("x") }` for `andD<T>(x: T):
    /// T & {}` keeps `"x"` where `{ a: unionD("x") }` for
    /// `unionD<T>(x: T): T | null` widens to `string | null`).
    fn collect_union_top_level_fresh_bounds(
        &self,
        session_id: SessionId,
        structure: SemanticNodeId,
        substitution: &CanonicalTypeSubstitution,
        fresh_literal_returns: &mut Vec<SemanticNodeId>,
    ) {
        let graph = self.graph();
        for (param, bound) in substitution.bindings() {
            if !matches!(
                graph.node_data(*bound).as_deref(),
                Some(SemanticNodeData::Literal(_))
            ) {
                continue;
            }
            if !self.binding_is_fresh_literal_deposit(session_id, *param, *bound) {
                continue;
            }
            if self.deposit_at_union_top_level(structure, *param)
                && !fresh_literal_returns.contains(bound)
            {
                fresh_literal_returns.push(*bound);
            }
        }
    }

    /// The UNION-only half of [`Self::deposit_at_top_level`] — the same
    /// binder-identity walk, excluding intersection constituents.
    fn deposit_at_union_top_level(&self, structure: SemanticNodeId, param: SemanticNodeId) -> bool {
        self.deposit_walk_reaches_binder(structure, param, true, DEPOSIT_WALK_FUEL)
    }

    /// The substitution with every FRESH-deposited literal binding the
    /// checker widens at the call boundary widened to its primitive —
    /// every fresh deposit whose binder does NOT appear at top level of
    /// `return_structure` (see [`Self::deposit_at_top_level`]):
    /// `wrap("x")` for `wrap<T>(v: T) { return { box: v } }` reads
    /// `{ box: string }` at every observed position (the caller's return,
    /// a binding initializer, a member value, a member read of the call
    /// expression). Bindings without a fresh deposit — authored pins,
    /// explicit type arguments, outer-context bindings — and deposits
    /// kept at top level stay unchanged; `None` when nothing widens.
    ///
    /// `return_structure` is the BINDER-BEARING structure (the declared
    /// annotation, or the callee's uninstantiated flow return). A caller
    /// with no binder-bearing structure passes `None`: every fresh
    /// deposit then widens — the superset direction, never a value-match
    /// against an instantiated result.
    fn fresh_widened_substitution_outside_top_level(
        &self,
        session_id: SessionId,
        substitution: &CanonicalTypeSubstitution,
        return_structure: Option<SemanticNodeId>,
    ) -> Option<CanonicalTypeSubstitution> {
        let graph = self.graph();
        let mut any_widened = false;
        let widened_bindings: Vec<(SemanticNodeId, SemanticNodeId)> = substitution
            .bindings()
            .iter()
            .map(|(param, bound)| {
                let widened = match graph.node_data(*bound).as_deref() {
                    Some(SemanticNodeData::Literal(value))
                        if self.binding_is_fresh_literal_deposit(session_id, *param, *bound)
                            && !return_structure.is_some_and(|structure| {
                                self.deposit_at_top_level(structure, *param)
                            }) =>
                    {
                        let primitive = match value {
                            verter_type_expr::LiteralValue::String(_) => PrimitiveKind::String,
                            verter_type_expr::LiteralValue::Number(_) => PrimitiveKind::Number,
                            verter_type_expr::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
                            verter_type_expr::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
                        };
                        graph.intern_node(SemanticNodeData::Primitive(primitive))
                    }
                    _ => *bound,
                };
                any_widened |= widened != *bound;
                (*param, widened)
            })
            .collect();
        any_widened.then(|| CanonicalTypeSubstitution::new(widened_bindings))
    }

    pub(super) fn call_inference_candidate(
        &self,
        node: SemanticNodeId,
        policy: ConstParamPolicy,
    ) -> SemanticNodeId {
        self.call_shape_transform(node, policy, &mut FxHashMap::default())
    }

    fn call_shape_transform(
        &self,
        node: SemanticNodeId,
        policy: ConstParamPolicy,
        memo: &mut FxHashMap<SemanticNodeId, SemanticNodeId>,
    ) -> SemanticNodeId {
        if let Some(cached) = memo.get(&node) {
            return *cached;
        }
        let graph = self.graph();
        memo.insert(node, node);
        let result = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(value)) if policy == ConstParamPolicy::NonConst => {
                let primitive = match value {
                    verter_type_expr::LiteralValue::String(_) => PrimitiveKind::String,
                    verter_type_expr::LiteralValue::Number(_) => PrimitiveKind::Number,
                    verter_type_expr::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
                    verter_type_expr::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
                };
                graph.intern_node(SemanticNodeData::Primitive(primitive))
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let values = elements
                    .iter()
                    .map(|element| self.call_shape_transform(element.value, policy, memo))
                    .collect::<Vec<_>>();
                if policy == ConstParamPolicy::Const {
                    let mut mapped = elements.to_vec();
                    for (element, value) in mapped.iter_mut().zip(values) {
                        element.value = value;
                    }
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Tuple {
                            elements: Arc::from(mapped.into_boxed_slice()),
                            readonly: true,
                        },
                    )
                } else {
                    let element = if values.is_empty() {
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                    } else {
                        self.relation_combine_candidates(&values, VariancePhase::Covariant)
                    };
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element,
                            readonly: false,
                        },
                    )
                }
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                let element = self.call_shape_transform(*element, policy, memo);
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Array {
                        element,
                        readonly: policy == ConstParamPolicy::Const,
                    },
                )
            }
            Some(SemanticNodeData::Union(members)) => {
                let members = members
                    .iter()
                    .map(|member| self.call_shape_transform(*member, policy, memo))
                    .collect::<Vec<_>>();
                self.relation_combine_candidates(&members, VariancePhase::Covariant)
            }
            Some(SemanticNodeData::Intersection(members)) => {
                let members = members
                    .iter()
                    .map(|member| self.call_shape_transform(*member, policy, memo))
                    .collect::<Vec<_>>();
                // Order- and scope-preserving rebuild: an intersection
                // reaching call-shape transformation may be an
                // overload-ordered carrier, so the transformed arms keep
                // their declaration order verbatim.
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Intersection(
                        crate::semantic_query::composite::CompositeList::preserving_rebuild(
                            Arc::from(members.into_boxed_slice()),
                        ),
                    ),
                )
            }
            Some(SemanticNodeData::Object(surface)) => {
                let mut members = surface.positive_members().to_vec();
                for member in &mut members {
                    member.value = self.call_shape_transform(member.value, policy, memo);
                    member.readonly = policy == ConstParamPolicy::Const;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(
                        surface
                            .clone()
                            .with_positive_members(Arc::from(members.into_boxed_slice())),
                    ),
                )
            }
            _ => node,
        };
        memo.insert(node, result);
        result
    }
}

fn call_bucket_matches(call: CallKind, signature: SignatureKind) -> bool {
    matches!(
        (call, signature),
        (CallKind::Call, SignatureKind::Call) | (CallKind::Construct, SignatureKind::Construct)
    )
}

fn decided_call_relation(
    step: RelationStep,
    own_return_function: Option<&crate::semantic_query::FlowFunctionSlotIdentity>,
) -> Result<Option<bool>, ResolveCallFailure> {
    match step {
        RelationStep::Assignable { .. } => Ok(Some(true)),
        RelationStep::NotAssignable => Ok(Some(false)),
        RelationStep::BudgetExceeded(_) => Err(ResolveCallFailure::Budget),
        RelationStep::Assumed(evidence)
            if assumption_is_relation_only(&evidence, own_return_function) =>
        {
            Ok(None)
        }
        RelationStep::Unknown | RelationStep::Assumed(_) => Err(ResolveCallFailure::Undecidable),
    }
}

/// Whether this applicability assumption's dependency closure stays clear of
/// the return SCC owned by the candidate under check.
///
/// The closure is the reentry cycle from the intercepted relation up to the
/// demanding frame, so it always contains the applicability frames the cycle
/// passes through — including the executor's own transparent frame, which is
/// the demander rather than a dependency. Applicability is exactly what the
/// relation SCC root re-discharges and replays, so those frames leave the
/// assumption relation-only. Only the flow return the candidate's own carrier
/// holds puts this call's result inside the cycle, and that is the refusal.
fn assumption_is_relation_only(
    evidence: &super::dispatch_txn::RelationAssumptionEvidence,
    own_return_function: Option<&crate::semantic_query::FlowFunctionSlotIdentity>,
) -> bool {
    own_return_function.is_none_or(|function| !evidence.reaches_flow_function(function))
}

/// What a rest parameter's type proves about arity and about each trailing
/// argument's target.
enum RestShape {
    /// The rest parameter is still an uninstantiated type parameter
    /// (`...xs: A`). It is ONE inference position over the whole trailing
    /// argument list: those arguments assemble into a tuple candidate for
    /// the parameter itself, and the declaration-site constraint is checked
    /// against that assembled tuple. The constraint decides neither the
    /// per-argument target nor the arity.
    GenericTuple(SemanticNodeId),
    /// A resolved array rest — every trailing argument targets the element.
    Array(SemanticNodeId),
    /// A resolved tuple rest — trailing arguments target its elements
    /// positionally.
    Tuple(Arc<[crate::semantic_query::TupleElement]>),
    /// A shape the resolution could not classify. It proves nothing, so it
    /// caps nothing.
    Unresolved,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Classify one rest parameter's type. Reference carriers and alias
    /// chains of ANY authored depth resolve through the shared cycle-safe
    /// structural unwrap before classification, so a named alias
    /// (`...xs: ArgsAlias`) is seen as the tuple it is; an uninstantiated
    /// type parameter is reported AS the parameter, never through its
    /// constraint. Only a genuinely unresolvable shape (a carrier cycle,
    /// an unresolved reference) is `Unresolved` — which caps no arity but
    /// supplies no per-argument target either.
    fn rest_shape(&self, ty: SemanticNodeId) -> RestShape {
        let graph = self.graph();
        let resolved = match self.unwrap_identity_carrier_for_relation(ty) {
            super::relation::IdentityCarrierUnwrap::Concrete(resolved) => resolved,
            _ => return RestShape::Unresolved,
        };
        match graph.node_data(resolved).as_deref() {
            Some(SemanticNodeData::TypeParam { .. }) => RestShape::GenericTuple(resolved),
            Some(SemanticNodeData::Array { element, .. }) => RestShape::Array(*element),
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                RestShape::Tuple(Arc::clone(elements))
            }
            _ => RestShape::Unresolved,
        }
    }
}

/// One candidate's rest parameter: the authored parameter plus its
/// classified shape.
struct RestParam<'p> {
    param: &'p FunctionParam,
    shape: RestShape,
}

fn call_candidate_arity(
    params: &[FunctionParam],
    rest: Option<&RestParam<'_>>,
) -> (usize, Option<usize>, bool) {
    let fixed = params
        .iter()
        .filter(|param| !param.rest)
        .collect::<Vec<_>>();
    // TypeScript's required arity is the LAST required position + 1, not
    // the count of non-optional parameters: an optional / initializer
    // parameter BEFORE a required one still occupies a required position
    // (`(a = 0, b: string)` requires TWO arguments — TS2554).
    let fixed_required = fixed
        .iter()
        .rposition(|param| !param.optional)
        .map_or(0, |position| position + 1);
    let Some(rest) = rest else {
        return (fixed_required, Some(params.len()), false);
    };
    match &rest.shape {
        RestShape::Tuple(elements) => {
            let tuple_required = elements
                .iter()
                .filter(|element| !element.optional && !element.rest)
                .count();
            // A required tuple element sits AFTER every fixed parameter,
            // so its presence makes every fixed parameter required by
            // position (`(a?: number, ...rest: [string])` requires two).
            let required = if tuple_required > 0 {
                fixed.len() + tuple_required
            } else {
                fixed_required
            };
            // A rest element at ANY tuple position opens arity — the
            // embedded run (`[...string[], number]`) accepts any number of
            // arguments at its own position, exactly like a trailing one.
            let has_rest = elements.iter().any(|element| element.rest);
            (
                required,
                (!has_rest).then_some(fixed.len() + elements.len()),
                has_rest,
            )
        }
        // A generic, an array, or an UNCLASSIFIED rest fails OPEN. Arity is
        // capped only by a PROVEN fixed-length shape: a generic rest is one
        // open inference position, an array rest is unbounded, and a shape
        // the resolution could not classify proves nothing — capping it at
        // the fixed-parameter count would reject every rest argument.
        RestShape::GenericTuple(_) | RestShape::Array(_) | RestShape::Unresolved => {
            (fixed_required, None, true)
        }
    }
}

fn mapped_parameter_type(
    params: &[FunctionParam],
    rest: Option<&RestParam<'_>>,
    index: usize,
    argument_count: usize,
    argument: CallArgument,
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    evidence: &mut crate::project_semantic_dispatch::canonical_algebra::CanonicalEvidence,
) -> Option<SemanticNodeId> {
    // An INDEFINITE spread supplies an unknown-length tail, so it never
    // maps onto a positional slot — it relates against the rest parameter's
    // own declared type.
    if !argument.indefinite_spread {
        if let Some(param) = params.get(index) {
            if !param.rest {
                return Some(optional_parameter_target(param, graph, evidence));
            }
        }
    }
    let rest = rest?;
    // The argument's offset INTO the rest parameter: the arguments before
    // the rest slot are consumed by the ordinary parameters.
    let offset = index.saturating_sub(params.len().saturating_sub(1));
    match &rest.shape {
        // A generic rest resolves as ONE tuple bundle, never per argument;
        // an unclassified rest supplies no target at all.
        RestShape::GenericTuple(_) | RestShape::Unresolved => None,
        // An indefinite spread supplies an unknown-length TAIL, so it
        // relates against the rest parameter's own declared type — for a
        // tuple rest, against the SUFFIX still unfilled at this offset,
        // never against the whole tuple whose fixed prefix the preceding
        // positional arguments already covered.
        RestShape::Array(_) if argument.indefinite_spread => Some(rest.param.ty),
        RestShape::Tuple(elements) if argument.indefinite_spread => {
            tuple_suffix_target(elements, offset, rest.param.ty, graph)
        }
        RestShape::Array(element) => Some(*element),
        // A POSITIONAL argument maps around the tuple's rest element, which
        // may sit at ANY position: elements BEFORE the rest map by offset,
        // the LAST `post` arguments map onto the post-rest elements FROM
        // THE END, and everything between lands on the rest element's
        // ELEMENT type, never the rest element's own array type.
        RestShape::Tuple(elements) => {
            let Some(rest_position) = elements.iter().position(|element| element.rest) else {
                // A rest-free tuple maps purely by offset; arity already
                // capped the argument count at the element count.
                return elements.get(offset).map(|element| element.value);
            };
            if offset < rest_position {
                return elements.get(offset).map(|element| element.value);
            }
            let post = elements.len() - rest_position - 1;
            let tuple_argument_count =
                argument_count.saturating_sub(params.len().saturating_sub(1));
            let from_end = tuple_argument_count.saturating_sub(offset);
            if from_end >= 1 && from_end <= post {
                return elements
                    .get(elements.len() - from_end)
                    .map(|element| element.value);
            }
            let rest_element = elements.get(rest_position)?;
            match graph.node_data(rest_element.value).as_deref() {
                Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                _ => Some(rest_element.value),
            }
        }
    }
}

/// The relation target of an OPTIONAL parameter's position: its declared
/// type WITH `undefined` — under strict null semantics an optional (`?`)
/// or defaulted parameter accepts an explicit `undefined` argument, so the
/// position's check type is `T | undefined` (a defaulted parameter takes
/// its default there). A declared type already carrying the `undefined`
/// arm keeps its own node.
fn optional_parameter_target(
    param: &FunctionParam,
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    evidence: &mut crate::project_semantic_dispatch::canonical_algebra::CanonicalEvidence,
) -> SemanticNodeId {
    if !param.optional {
        return param.ty;
    }
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    if param.ty == undefined {
        return param.ty;
    }
    // Canonical construction: the optional-parameter relation target
    // (`T | undefined`) routes through the one authority (which also
    // flattens a union-typed `T`); the evidence threads to the caller's
    // disposition boundary.
    let composite = crate::project_semantic_dispatch::canonical_algebra::canonical_union(
        graph,
        &[param.ty, undefined],
    );
    evidence.absorb(composite.evidence);
    composite.node
}

/// The relation target for an indefinite spread landing at `offset` inside
/// a tuple rest parameter: the tuple suffix from that offset on.
///
/// At offset zero the suffix IS the declared tuple (`declared`). A suffix
/// that is exactly the trailing rest element is that element's own array
/// type, which an indefinite spread relates against directly. Any other
/// suffix re-interns the remaining elements as their own tuple.
fn tuple_suffix_target(
    elements: &Arc<[crate::semantic_query::TupleElement]>,
    offset: usize,
    declared: SemanticNodeId,
    graph: &crate::semantic_query_memo::SemanticGraphStore,
) -> Option<SemanticNodeId> {
    if offset == 0 {
        return Some(declared);
    }
    let suffix = elements.get(offset..)?;
    match suffix {
        [] => None,
        [only] if only.rest => Some(only.value),
        _ => Some(graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from(suffix.to_vec().into_boxed_slice()),
            readonly: false,
        })),
    }
}

/// Union `source`'s self-roots into `target`, one root per canonical.
pub(super) fn union_self_roots(
    target: &mut Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    source: &[crate::semantic_query_memo::ObservedGraphSelfRoot],
) {
    for root in source {
        if !target.iter().any(|(canonical, _)| canonical == &root.0) {
            target.push(root.clone());
        }
    }
}
