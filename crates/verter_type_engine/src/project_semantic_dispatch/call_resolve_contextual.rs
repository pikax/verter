//! Candidate-local contextual inference. A flow route lends the executor its
//! continuation while asking it, and owns the suspended collector while a
//! callback body evaluates. No collector is active during that body demand.

use super::super::inference::session::InferenceSession;
use super::*;
use crate::semantic_query::{InferBinding, RelationKind, TypeParamDecl};
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy)]
pub(super) enum ContextualCandidateFailure {
    Mismatch,
    Degraded(ResolveCallFailure),
}

impl From<ContextualCandidateFailure> for CandidateVerdict {
    fn from(failure: ContextualCandidateFailure) -> Self {
        match failure {
            ContextualCandidateFailure::Mismatch => Self::Mismatch,
            ContextualCandidateFailure::Degraded(failure) => Self::Degraded(failure),
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::project_semantic_dispatch) struct ContextualCallState {
    original: Option<Vec<CallArgument>>,
    /// As in the checker, a callback's parameter context is assigned once,
    /// by the first candidate that reaches it. Inference journals are separate.
    typed_arguments: FxHashMap<usize, CallArgument>,
    arity_candidates: FxHashSet<SemanticNodeId>,
    active: Option<ContextualCandidate>,
    arity_fit: bool,
    diagnostics_enabled: bool,
    pub(super) diagnostic: Option<crate::semantic_query::CheckerDiagnosticCode>,
    pub(in crate::project_semantic_dispatch) resume_at: Option<(RelationKind, SemanticNodeId)>,
}

impl ContextualCallState {
    pub(super) fn was_context_sensitive(&self, index: usize) -> bool {
        self.original
            .as_ref()
            .and_then(|args| args.get(index))
            .is_some_and(|arg| arg.context_sensitive)
    }

    pub(super) fn failure(&self) -> Option<ResolveCallFailure> {
        if !self.diagnostics_enabled {
            return None;
        }
        if !self
            .original
            .as_ref()?
            .iter()
            .any(|arg| arg.context_sensitive)
        {
            return None;
        }
        Some(ResolveCallFailure::Diagnosed(
            crate::semantic_query::CheckerDiagnostic {
                code: if self.arity_candidates.len() > 1 {
                    crate::semantic_query::CheckerDiagnosticCode::NoMatchingOverload
                } else if self.arity_fit {
                    self.diagnostic.unwrap_or(
                        crate::semantic_query::CheckerDiagnosticCode::ArgumentNotAssignable,
                    )
                } else {
                    crate::semantic_query::CheckerDiagnosticCode::ArgumentCount
                },
                operation: crate::semantic_query::CheckerDiagnosticOperation::CallResolution,
            },
        ))
    }
}

#[derive(Debug)]
struct ContextualCandidate {
    signature: SemanticNodeId,
    arguments: Vec<CallArgument>,
    deferred: Vec<(usize, SemanticNodeId)>,
    cursor: usize,
    session: Option<InferenceSession>,
    parameters: Arc<[TypeParamDecl]>,
    positions: FxHashMap<SemanticNodeId, usize>,
    fixed: FxHashMap<SemanticNodeId, SemanticNodeId>,
    pending_target: Option<SemanticNodeId>,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    pub(super) fn contextual_argument_failure(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        budget: &mut CallResolutionBudget<'_>,
    ) -> crate::semantic_query::CheckerDiagnosticCode {
        use crate::semantic_query::CheckerDiagnosticCode;
        let single_return = |node| {
            let super::super::signature_discovery::SharedSignatureNodes::Nodes(nodes) =
                self.shared_signature_nodes(node, SignatureKind::Call)
            else {
                return None;
            };
            let [node] = nodes.as_slice() else {
                return None;
            };
            match self.graph().node_data(*node).as_deref() {
                Some(SemanticNodeData::Signature { return_type, .. }) => Some(*return_type),
                _ => None,
            }
        };
        if let (Some(from), Some(into)) = (single_return(source), single_return(target)) {
            if matches!(
                self.call_relation(from, into, from, budget, false, false),
                RelationStep::NotAssignable
            ) {
                return CheckerDiagnosticCode::TypeNotAssignable;
            }
        }
        CheckerDiagnosticCode::ArgumentNotAssignable
    }
    /// The same call authority, with a route-owned contextual continuation.
    /// Nesting restores the enclosing route's state after the inner call.
    pub(in crate::project_semantic_dispatch) fn execute_contextual_call(
        &self,
        key: ResolveCallKey,
        state: &mut ContextualCallState,
    ) -> ResolveCallStep {
        let outer = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .replace(std::mem::take(state));
        let step = self.execute_resolve_call(key);
        *state = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .take()
            .expect("installed contextual call");
        self.dispatch_txn.borrow_mut().call.contextual = outer;
        if !matches!(
            step,
            ResolveCallStep::Degraded(ResolveCallFailure::ContextSensitiveInference {
                contextual: Some(_)
            })
        ) {
            state.active = None;
            state.resume_at = None;
        }
        step
    }

    /// Type each deferred argument exactly once for this candidate and pass.
    /// The final applicability check still belongs to `check_call_candidate`.
    pub(super) fn prepare_contextual_candidate(
        &self,
        key: &ResolveCallKey,
        candidate: &SignatureRef,
        arguments: &[CallArgument],
        budget: &mut CallResolutionBudget<'_>,
    ) -> Result<Vec<CallArgument>, ContextualCandidateFailure> {
        let mut state = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .take()
            .expect("contextual route");
        let result =
            self.prepare_contextual_candidate_inner(key, candidate, arguments, budget, &mut state);
        self.dispatch_txn.borrow_mut().call.contextual = Some(state);
        result
    }

    fn prepare_contextual_candidate_inner(
        &self,
        key: &ResolveCallKey,
        candidate: &SignatureRef,
        arguments: &[CallArgument],
        budget: &mut CallResolutionBudget<'_>,
        state: &mut ContextualCallState,
    ) -> Result<Vec<CallArgument>, ContextualCandidateFailure> {
        let original = state.original.get_or_insert_with(|| arguments.to_vec());
        if !original.iter().any(|argument| argument.context_sensitive)
            || !key.explicit_type_args.is_empty()
            || key.args.iter().any(|arg| match arg {
                CallArgKey::Eager { spread, .. } | CallArgKey::ProgramExpression { spread, .. } => {
                    *spread
                }
            })
        {
            return Ok(arguments.to_vec());
        }
        state.diagnostics_enabled = true;
        let graph = self.graph();
        let Some(SemanticNodeData::Signature {
            params,
            type_parameters,
            ..
        }) = graph.node_data(candidate.node).as_deref().cloned()
        else {
            return Ok(arguments.to_vec());
        };
        let (receiver, ordinary) = crate::semantic_query::split_this_receiver(&params);
        let rest = ordinary
            .last()
            .filter(|param| param.rest)
            .map(|param| RestParam {
                param,
                shape: self.rest_shape(param.ty),
            });
        let (required, maximum, _) = call_candidate_arity(ordinary, rest.as_ref());
        if original.len() < required || maximum.is_some_and(|max| original.len() > max) {
            return Err(ContextualCandidateFailure::Mismatch);
        }
        state.arity_fit = true;
        state.arity_candidates.insert(candidate.node);
        let resumed = state.active.take();
        let (mut progress, session_id) = if let Some(progress) = resumed {
            verter_debug_assert_eq!(progress.signature, candidate.node);
            let id = progress.session.as_ref().expect("suspended collector").id;
            (progress, id)
        } else {
            let setup = InferenceSessionSetup::new(
                type_parameters
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
                    .collect(),
                VariancePhase::Covariant,
                InferencePassKind::CallApplicability,
                InferenceCandidatePriority::Argument,
                NoInferMask::empty(),
                ConstParamPolicy::NonConst,
                ContextualInferenceMode::None,
            );
            let id = self
                .dispatch_txn
                .borrow_mut()
                .push_collecting_session(setup, None);
            let session = self.take_contextual_session(id);
            (
                ContextualCandidate {
                    signature: candidate.node,
                    arguments: original.clone(),
                    deferred: Vec::new(),
                    cursor: 0,
                    session: Some(session),
                    positions: type_parameters
                        .iter()
                        .enumerate()
                        .map(|(i, decl)| (decl.param, i))
                        .collect(),
                    parameters: type_parameters,
                    fixed: FxHashMap::default(),
                    pending_target: None,
                },
                id,
            )
        };
        self.restore_contextual_session(&mut progress.session);
        let result = (|| {
            if progress.pending_target.is_some() {
                let (position, _) = progress.deferred[progress.cursor];
                let typed = arguments
                    .get(position)
                    .filter(|arg| !arg.context_sensitive)
                    .ok_or(ContextualCandidateFailure::Degraded(
                        ResolveCallFailure::Undecidable,
                    ))?;
                let target = progress.pending_target.take().expect("pending callback");
                state.typed_arguments.insert(position, *typed);
                if !self.collect_contextual_argument(typed, target, budget)? {
                    state.diagnostic =
                        Some(self.contextual_argument_failure(typed.node, target, budget));
                    state.resume_at = None;
                    return Err(ContextualCandidateFailure::Mismatch);
                }
                progress.arguments[position] = *typed;
                progress.cursor += 1;
            } else {
                // Eager arguments infer before a context-sensitive callback is
                // visited. Annotated callbacks participate in this pass too.
                if let Some(receiver) = receiver {
                    let source = key.receiver.unwrap_or_else(|| {
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void))
                    });
                    let step = self.call_receiver_relation(source, receiver.ty, source, budget);
                    if matches!(
                        step,
                        RelationStep::Unknown
                            | RelationStep::BudgetExceeded(_)
                            | RelationStep::Assumed(_)
                    ) {
                        return Err(ContextualCandidateFailure::Degraded(
                            ResolveCallFailure::Undecidable,
                        ));
                    }
                }
                let const_params: Vec<_> = progress
                    .parameters
                    .iter()
                    .filter(|decl| decl.is_const)
                    .map(|decl| decl.param)
                    .collect();
                let mut generic_arguments = Vec::new();
                for (position, argument) in original.iter().enumerate() {
                    let mut evidence =
                        super::super::canonical_algebra::CanonicalEvidence::default();
                    let target = mapped_parameter_type(
                        ordinary,
                        rest.as_ref(),
                        position,
                        original.len(),
                        *argument,
                        graph,
                        &mut evidence,
                    )
                    .ok_or(ContextualCandidateFailure::Degraded(
                        ResolveCallFailure::Undecidable,
                    ))?;
                    self.deposit_canonical_evidence(evidence);
                    if argument.context_sensitive {
                        progress.deferred.push((position, target));
                        if let Some(first_pass) = argument.first_pass {
                            let checkpoint = self.relation_session_checkpoint();
                            let view = CallArgument {
                                node: first_pass,
                                ..*argument
                            };
                            match self.collect_contextual_argument(&view, target, budget) {
                                Ok(true) => {}
                                Ok(false)
                                | Err(ContextualCandidateFailure::Degraded(
                                    ResolveCallFailure::Undecidable,
                                )) => self.relation_session_rollback(&checkpoint),
                                Err(failure) => return Err(failure),
                            }
                        }
                        continue;
                    }
                    let mut argument = *argument;
                    if let Some(view) = argument.const_view {
                        argument.node = if const_params.contains(&target) {
                            view
                        } else {
                            self.nested_const_context_view(
                                argument.node,
                                view,
                                target,
                                &const_params,
                            )
                        };
                    }
                    if let Some(signature) = self.generic_function_against(argument.node, target) {
                        generic_arguments.push((argument, signature, target));
                        continue;
                    }
                    self.collect_contextual_argument(&argument, target, budget)?;
                }
                for (mut argument, signature, target) in generic_arguments {
                    argument.node = self
                        .generic_function_in_context(
                            session_id,
                            &progress.parameters,
                            (signature, target),
                            None,
                            budget,
                            None,
                        )
                        .map_err(ContextualCandidateFailure::Degraded)?
                        .unwrap_or(argument.node);
                    self.collect_contextual_argument(&argument, target, budget)?;
                }
            }
            while let Some(&(position, target)) = progress.deferred.get(progress.cursor) {
                let (contextual, inference_target) = self
                    .contextual_candidate_mapper(&mut progress, target, session_id, budget)
                    .map_err(ContextualCandidateFailure::Degraded)?;
                if let Some(typed) = state.typed_arguments.get(&position) {
                    if !self.collect_contextual_argument(typed, inference_target, budget)? {
                        state.resume_at = None;
                        return Err(ContextualCandidateFailure::Mismatch);
                    }
                    progress.arguments[position] = *typed;
                    progress.cursor += 1;
                    continue;
                }
                progress.pending_target = Some(inference_target);
                state.resume_at = Some((
                    self.dispatch_txn.borrow().call.applicability,
                    candidate.node,
                ));
                return Err(ContextualCandidateFailure::Degraded(
                    ResolveCallFailure::ContextSensitiveInference {
                        contextual: u32::try_from(position)
                            .ok()
                            .map(|position| (position, contextual)),
                    },
                ));
            }
            state.resume_at = None;
            Ok(progress.arguments.clone())
        })();
        progress.session = Some(self.take_contextual_session(session_id));
        if matches!(
            result,
            Err(ContextualCandidateFailure::Degraded(
                ResolveCallFailure::ContextSensitiveInference {
                    contextual: Some(_)
                }
            ))
        ) {
            state.active = Some(progress);
        } else {
            progress.session.as_mut().expect("collector").abandon();
        }
        result
    }

    fn collect_contextual_argument(
        &self,
        argument: &CallArgument,
        target: SemanticNodeId,
        budget: &mut CallResolutionBudget<'_>,
    ) -> Result<bool, ContextualCandidateFailure> {
        let before = self.accepted_inference_deposits();
        let step = self.call_argument_relation(
            argument.node,
            target,
            argument.freshness_origin,
            budget,
            argument.literal_mode,
        );
        if !budget.charge_accepted_deposits(self.accepted_inference_deposits() - before) {
            return Err(ContextualCandidateFailure::Degraded(
                ResolveCallFailure::Budget,
            ));
        }
        match step {
            RelationStep::Assignable { .. } => Ok(true),
            RelationStep::NotAssignable => Ok(false),
            RelationStep::BudgetExceeded(_) => Err(ContextualCandidateFailure::Degraded(
                ResolveCallFailure::Budget,
            )),
            RelationStep::Unknown | RelationStep::Assumed(_) => Err(
                ContextualCandidateFailure::Degraded(ResolveCallFailure::Undecidable),
            ),
        }
    }

    fn take_contextual_session(&self, id: SessionId) -> InferenceSession {
        let mut txn = self.dispatch_txn.borrow_mut();
        let position = txn
            .relation
            .session_position(id)
            .expect("contextual collector");
        txn.relation.sessions.remove(position)
    }

    fn restore_contextual_session(&self, session: &mut Option<InferenceSession>) {
        let mut active = session.take().expect("suspended collector");
        let mut txn = self.dispatch_txn.borrow_mut();
        active.opened_at_depth = txn.reentry().depth();
        let position = txn
            .relation
            .sessions
            .partition_point(|other| other.id < active.id);
        txn.relation.sessions.insert(position, active);
    }

    fn contextual_candidate_mapper(
        &self,
        progress: &mut ContextualCandidate,
        target: SemanticNodeId,
        id: SessionId,
        budget: &mut CallResolutionBudget<'_>,
    ) -> Result<(SemanticNodeId, SemanticNodeId), ResolveCallFailure> {
        // Walk the callback template, stopping at a binder. Fixed binders are
        // read by identity; their (possibly deeply nested) images are not walked.
        let mut stack = vec![target];
        let mut seen = FxHashSet::default();
        let mut positions = Vec::new();
        let mut retained = Vec::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if let Some(&bound) = progress.fixed.get(&node) {
                retained.push((node, bound));
                continue;
            }
            if let Some(&position) = progress.positions.get(&node) {
                positions.push(position);
                let decl = &progress.parameters[position];
                stack.extend(decl.constraint);
                stack.extend(decl.default);
                continue;
            }
            if let Some(data) = self.graph().node_data(node) {
                if data.for_each_child(|child| stack.push(child))
                    == crate::semantic_query::ChildWalk::Sealed
                {
                    return Err(ResolveCallFailure::Undecidable);
                }
            } else {
                return Err(ResolveCallFailure::Undecidable);
            }
        }
        positions.sort_unstable();
        let retained_substitution = CanonicalTypeSubstitution::new(retained.clone());
        let declarations: Vec<_> = positions
            .iter()
            .map(|&position| {
                let mut declaration = progress.parameters[position].clone();
                declaration.constraint = declaration
                    .constraint
                    .map(|bound| self.substitute_canonical(bound, &retained_substitution));
                declaration.default = declaration
                    .default
                    .map(|bound| self.substitute_canonical(bound, &retained_substitution));
                declaration
            })
            .collect();
        let params: Vec<_> = declarations.iter().map(|decl| decl.param).collect();
        let inputs = self
            .dispatch_txn
            .borrow()
            .relation
            .session(id)
            .and_then(|session| session.fixation_inputs_for(&params))
            .ok_or(ResolveCallFailure::Undecidable)?;
        let (bindings, uninferred) =
            self.fix_inference_inputs(inputs, &declarations, |this, from, to| {
                decided_call_relation(
                    this.call_relation(from, to, from, budget, false, false),
                    None,
                )
            })?;
        let mut fixed = retained.clone();
        let mut inferred = retained;
        for (position, InferBinding { param, bound, .. }) in bindings.iter().enumerate() {
            fixed.push((*param, *bound));
            if !uninferred.contains(&position) {
                inferred.push((*param, *bound));
            }
        }
        let fixed = CanonicalTypeSubstitution::new(fixed);
        let fixed = self
            .fresh_widened_substitution_outside_top_level(id, &fixed, None)?
            .unwrap_or(fixed);
        let inferred_params: FxHashSet<_> = inferred.iter().map(|(param, _)| *param).collect();
        let inference_mapper = CanonicalTypeSubstitution::new(
            fixed
                .bindings()
                .iter()
                .copied()
                .filter(|(param, _)| inferred_params.contains(param))
                .collect(),
        );
        let contextual_mapper = CanonicalTypeSubstitution::new(
            fixed
                .bindings()
                .iter()
                .map(|&(param, bound)| {
                    if inferred_params.contains(&param) {
                        (param, bound)
                    } else {
                        let constraint = progress
                            .positions
                            .get(&param)
                            .and_then(|&position| progress.parameters[position].constraint);
                        (
                            param,
                            self.contextual_uninferred_parameter(param, constraint),
                        )
                    }
                })
                .collect(),
        );
        let contextual = self.contextual_argument_type(target, &fixed, &contextual_mapper);
        let inference_target = self.contextual_argument_type(target, &fixed, &inference_mapper);
        // Only parameter-position binders become fixed by contextual typing;
        // the return's binder still collects what this callback returns.
        if let super::super::signature_discovery::SharedSignatureNodes::Nodes(signatures) =
            self.shared_signature_nodes(target, SignatureKind::Call)
        {
            for signature in signatures {
                if let Some(SemanticNodeData::Signature { params, .. }) =
                    self.graph().node_data(signature).as_deref()
                {
                    for &(param, bound) in fixed.bindings() {
                        if params.iter().any(|slot| self.mentions_node(slot.ty, param)) {
                            progress.fixed.insert(param, bound);
                        }
                    }
                }
            }
        }
        Ok((contextual, inference_target))
    }
}
