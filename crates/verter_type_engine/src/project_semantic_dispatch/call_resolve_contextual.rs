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
    owner: Option<crate::semantic_query::ProgramPointId>,
    original: Option<Vec<CallArgument>>,
    /// As in the checker, a callback's parameter context is assigned once,
    /// by the first candidate that types it; every later candidate relates
    /// by that typing (`assignContextualParameters`). Inference journals
    /// are separate.
    typed_arguments: FxHashMap<usize, CallArgument>,
    /// A candidate's OWN contextual typing of a callback, keyed by the
    /// candidate and the argument position: the recipe a rejected candidate
    /// reports names the callback as ITS parameters typed it, not as the
    /// once-assigned context did. Inference journals are separate.
    candidate_typed_arguments: FxHashMap<(SemanticNodeId, usize), CallArgument>,
    arity_candidates: FxHashSet<SemanticNodeId>,
    active: Option<ContextualCandidate>,
    delivery: Option<CallArgument>,
    ready: Option<(SemanticNodeId, Vec<CallArgument>)>,
    /// Parameter contexts remain fixed through final applicability and the
    /// sole candidate's error recovery, even after its collector is abandoned.
    pub(super) completed: Option<ContextualCandidateBindings>,
    arity_fit: bool,
    /// The arities of the candidates whose argument count the call does
    /// not fit, which its arity error reads when it fits none.
    arity: super::ArityEvidence,
    diagnostics_enabled: bool,
    pub(super) diagnostic: Option<crate::semantic_query::CheckerDiagnosticRecipe>,
    pub(in crate::project_semantic_dispatch) resume_at: Option<(RelationKind, SemanticNodeId)>,
}

/// Hide an enclosing call's continuation for the entire nested resolution,
/// including argument acquisition, candidate checks and early refusals.
pub(super) struct ContextualCallIsolation<'a> {
    txn: &'a std::cell::RefCell<super::super::dispatch_txn::CheckerDispatchTransaction>,
    outer: Option<ContextualCallState>,
}

impl Drop for ContextualCallIsolation<'_> {
    fn drop(&mut self) {
        self.txn.borrow_mut().call.contextual = self.outer.take();
    }
}

#[derive(Debug)]
pub(super) struct ContextualCandidateBindings {
    pub(super) signature: SemanticNodeId,
    pub(super) fixed: FxHashMap<SemanticNodeId, SemanticNodeId>,
    pub(super) diagnostic: Option<crate::semantic_query::CheckerDiagnosticRecipe>,
}

impl ContextualCallState {
    /// The suspended candidate collector this continuation holds: its
    /// session and how many deposits it holds.
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::project_semantic_dispatch) fn held_collector_for_tests(
        &self,
    ) -> Option<(u64, usize)> {
        let session = self.active.as_ref()?.session.as_ref()?;
        Some((session.id.0, session.collected_candidates_for_tests()))
    }

    /// The contextual target the held collector waits to relate a typed
    /// callback to.
    pub(in crate::project_semantic_dispatch) fn pending_delivery_target(
        &self,
    ) -> Option<SemanticNodeId> {
        self.active.as_ref()?.pending_target
    }

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
        let original = self.original.as_ref()?;
        // Several candidates fit the call's arity: TS2769, reporting the
        // last one's error below it and at its position.
        let recipe = if self.arity_candidates.len() > 1 {
            self.diagnostic?.as_last_overload()
        } else if self.arity_fit {
            self.diagnostic?
        } else {
            self.arity.recipe(original)?
        };
        Some(ResolveCallFailure::Diagnosed(recipe))
    }
}

#[derive(Debug)]
struct ContextualCandidate {
    signature: SignatureRef,
    arguments: Vec<CallArgument>,
    deferred: Vec<(usize, SemanticNodeId)>,
    cursor: usize,
    /// An eager argument already rejected this candidate while a deferred
    /// callback before it is not typed yet: the candidate rejects whatever
    /// its callbacks then relate, and the failure stands as its own.
    eager_mismatch: bool,
    session: Option<InferenceSession>,
    parameters: Arc<[TypeParamDecl]>,
    positions: FxHashMap<SemanticNodeId, usize>,
    /// A parameter's inference can depend on covariant candidates from a
    /// sibling constrained to it. Build these reverse edges once per candidate.
    constrained_siblings: FxHashMap<SemanticNodeId, Vec<SemanticNodeId>>,
    fixed: FxHashMap<SemanticNodeId, SemanticNodeId>,
    pending_target: Option<SemanticNodeId>,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Type properties that produce a marker's receiver binders before the
    /// properties consuming them. Source order breaks ties and cycles.
    pub(in crate::project_semantic_dispatch) fn contextual_object_order(
        &self,
        state: &ContextualCallState,
        names: &[Option<Arc<str>>],
        canonical: &str,
    ) -> Vec<usize> {
        let source_order = || (0..names.len()).collect::<Vec<_>>();
        let Some(progress) = state.active.as_ref() else {
            return source_order();
        };
        let (_, target) = progress.deferred[progress.cursor];
        let Some(surface) = self.resolve_typeinfo_surface_view(
            target,
            ProjectionReductionContext::structural_transit(),
        ) else {
            return source_order();
        };
        let parameters: FxHashSet<_> = progress.parameters.iter().map(|decl| decl.param).collect();
        if parameters.is_empty() {
            return source_order();
        }
        let mut producers: FxHashMap<SemanticNodeId, Vec<usize>> = FxHashMap::default();
        let mut consumers = vec![FxHashSet::default(); names.len()];
        for (index, name) in names.iter().enumerate() {
            let Some(name) = name else { continue };
            let crate::semantic_query::SurfaceKeyProjection::Exact(member) = surface
                .project_known_key(&crate::semantic_query::PropertyKey::identifier(
                    name.as_ref(),
                ))
            else {
                continue;
            };
            let mut pending = vec![member.value];
            let mut seen = FxHashSet::default();
            while let Some(node) = pending.pop() {
                if !seen.insert(node) {
                    continue;
                }
                if parameters.contains(&node) {
                    producers.entry(node).or_default().push(index);
                    continue;
                }
                let Some(data) = self.graph().node_data(node) else {
                    continue;
                };
                if let SemanticNodeData::InstantiationRef { base, args } = &*data {
                    if args.len() == 1
                        && self.names_contextual_this_marker(
                            canonical,
                            &base.canonical_id,
                            base.owner,
                            &base.decl_name,
                        )
                    {
                        let receiver = args[0];
                        consumers[index].extend(
                            parameters
                                .iter()
                                .copied()
                                .filter(|param| self.mentions_node(receiver, *param)),
                        );
                        continue;
                    }
                }
                if matches!(
                    &*data,
                    SemanticNodeData::InstantiationRef { .. } | SemanticNodeData::DeclRef { .. }
                ) {
                    let resolved = self.contextual_carrier_view(node);
                    if resolved != node {
                        pending.push(resolved);
                        continue;
                    }
                }
                if data.for_each_child(|child| pending.push(child))
                    == crate::semantic_query::ChildWalk::Sealed
                {
                    return source_order();
                }
            }
        }
        let dependencies: Vec<FxHashSet<usize>> = consumers
            .iter()
            .enumerate()
            .map(|(index, params)| {
                params
                    .iter()
                    .filter_map(|param| producers.get(param))
                    .flatten()
                    .copied()
                    .filter(|producer| *producer != index)
                    .collect()
            })
            .collect();
        let mut remaining: Vec<_> = dependencies.iter().map(FxHashSet::len).collect();
        let mut followers = vec![Vec::new(); names.len()];
        let mut ready = std::collections::BinaryHeap::new();
        for (index, dependencies) in dependencies.iter().enumerate() {
            if dependencies.is_empty() {
                ready.push(std::cmp::Reverse(index));
            }
            for &dependency in dependencies {
                followers[dependency].push(index);
            }
        }
        let mut ordered = Vec::with_capacity(names.len());
        let mut completed = vec![false; names.len()];
        while let Some(std::cmp::Reverse(next)) = ready.pop() {
            completed[next] = true;
            ordered.push(next);
            for &follower in &followers[next] {
                remaining[follower] -= 1;
                if remaining[follower] == 0 {
                    ready.push(std::cmp::Reverse(follower));
                }
            }
        }
        ordered.extend((0..names.len()).filter(|index| !completed[*index]));
        ordered
    }

    /// A checked property advances the suspended call's own collector. The
    /// collector is installed only for inference, never while a body runs.
    pub(in crate::project_semantic_dispatch) fn contextual_object_member(
        &self,
        state: &mut ContextualCallState,
        name: &str,
        value: Option<SemanticNodeId>,
    ) -> Result<Option<SemanticNodeId>, ResolveCallFailure> {
        let Some(progress) = state.active.as_mut() else {
            return Ok(None);
        };
        let (_, target) = progress.deferred[progress.cursor];
        let id = progress.session.as_ref().expect("suspended collector").id;
        let declared = Arc::clone(&progress.parameters);
        self.restore_collecting_session(&mut progress.session);
        let mut budget = CallResolutionBudget::new(&self.connected_demand);
        let result = (|| {
            let target = self.resolved_reduction_view(target);
            let surface = self
                .resolve_typeinfo_surface_view(
                    target,
                    ProjectionReductionContext::structural_transit(),
                )
                .ok_or(ResolveCallFailure::Undecidable)?;
            let crate::semantic_query::SurfaceKeyProjection::Exact(member) =
                surface.project_known_key(&crate::semantic_query::PropertyKey::identifier(name))
            else {
                return Ok(None);
            };
            if let Some(node) = value {
                let argument = CallArgument {
                    node,
                    first_pass: None,
                    const_view: None,
                    indefinite_spread: false,
                    literal_mode: ArgumentLiteralMode::Literal,
                    context_sensitive: false,
                    freshness_origin: node,
                    // A member read off an argument is reported by no
                    // diagnostic of its own.
                    authored: u32::MAX,
                    literal_provenance: None,
                };
                match self.collect_contextual_argument(
                    &argument,
                    member.value,
                    &mut budget,
                    &declared,
                    false,
                ) {
                    Ok(true) => {}
                    Ok(false) | Err(ContextualCandidateFailure::Mismatch) => {
                        return Err(ResolveCallFailure::Undecidable)
                    }
                    Err(ContextualCandidateFailure::Degraded(failure)) => return Err(failure),
                }
                Ok(None)
            } else {
                self.contextual_candidate_mapper(progress, member.value, id, &mut budget)
                    .map(|(contextual, _)| Some(contextual))
            }
        })();
        progress.session = Some(self.take_collecting_session(id));
        result
    }
    pub(super) fn isolate_contextual_call(
        &self,
        key: &ResolveCallKey,
    ) -> Option<ContextualCallIsolation<'_>> {
        let mut txn = self.dispatch_txn.borrow_mut();
        if txn
            .call
            .contextual
            .as_ref()
            .is_some_and(|state| state.owner.as_ref() != Some(&key.point))
        {
            Some(ContextualCallIsolation {
                txn: &self.dispatch_txn,
                outer: txn.call.contextual.take(),
            })
        } else {
            None
        }
    }

    /// Exercise ordinary executor reentry while a real contextual candidate is
    /// suspended, including preservation of the enclosing continuation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_call_under_suspended_context_for_tests(
        &self,
        outer_key: ResolveCallKey,
        inner_key: ResolveCallKey,
    ) -> ResolveCallStep {
        let mut state = ContextualCallState::default();
        assert!(matches!(
            self.execute_contextual_call(outer_key, &mut state).0,
            ResolveCallStep::Degraded(ResolveCallFailure::ContextSensitiveInference {
                contextual: Some(_)
            })
        ));
        let before = format!("{state:?}");
        let outer = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .replace(state);
        let result = self.execute_resolve_call(inner_key);
        let state = self.dispatch_txn.borrow_mut().call.contextual.take();
        self.dispatch_txn.borrow_mut().call.contextual = outer;
        assert_eq!(
            format!("{:?}", state.expect("enclosing continuation")),
            before
        );
        result
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_contextual_call_for_tests(&self, key: ResolveCallKey) -> ResolveCallStep {
        self.execute_contextual_call(key, &mut ContextualCallState::default())
            .0
    }

    /// The same call authority, with a route-owned contextual continuation.
    /// Nesting restores the enclosing route's state after the inner call.
    pub(in crate::project_semantic_dispatch) fn execute_contextual_call(
        &self,
        key: ResolveCallKey,
        state: &mut ContextualCallState,
    ) -> (ResolveCallStep, Option<DeclaredCallResult>) {
        state.owner = Some(key.point.clone());
        let outer = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .replace(std::mem::take(state));
        let (step, proof) = self.execute_resolve_call_with_declared_result(key);
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
        (step, proof)
    }

    /// A callback delivery advances only its suspended candidate. Full call
    /// acquisition and publication run again only on completion or rejection.
    pub(in crate::project_semantic_dispatch) fn resume_contextual_call(
        &self,
        key: &mut ResolveCallKey,
        args: &[CallArgKey],
        position: usize,
        state: &mut ContextualCallState,
    ) -> (ResolveCallStep, Option<DeclaredCallResult>) {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if initial_trip.is_some() {
            state.active = None;
            return (ResolveCallStep::Degraded(ResolveCallFailure::Budget), None);
        }
        let Some(candidate) = state
            .active
            .as_ref()
            .map(|progress| progress.signature.clone())
        else {
            // Spread and explicitly instantiated calls use the executor's
            // ordinary contextual recheck, without a suspended collector.
            record_argument_work(args.len());
            key.args = Arc::from(args);
            return self.execute_contextual_call(key.clone(), state);
        };
        let Some(argument) = args.get(position) else {
            return (
                ResolveCallStep::Degraded(ResolveCallFailure::Undecidable),
                None,
            );
        };
        let mut delivery_key = key.clone();
        delivery_key.args = Arc::from([argument.clone()]);
        let delivery = match self.acquire_call_arguments(&delivery_key) {
            // The delivery is acquired alone; it is the call's argument at
            // `position`.
            Ok(mut arguments) if arguments.len() == 1 => arguments.pop().map(|mut delivery| {
                delivery.authored = u32::try_from(position).unwrap_or(u32::MAX);
                delivery
            }),
            Ok(_) => {
                return (
                    ResolveCallStep::Degraded(ResolveCallFailure::Undecidable),
                    None,
                )
            }
            Err(failure) => return (ResolveCallStep::Degraded(failure), None),
        };
        state.delivery = delivery;
        let resume_at = state.resume_at;
        let outer_applicability = self.dispatch_txn.borrow().call.applicability;
        if let Some((applicability, _)) = resume_at {
            self.dispatch_txn.borrow_mut().call.applicability = applicability;
        }
        let outer = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .replace(std::mem::take(state));
        let result = self.prepare_contextual_candidate(
            key,
            &candidate,
            &[],
            &mut CallResolutionBudget::new(&self.connected_demand),
            false,
        );
        *state = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .take()
            .expect("installed contextual call");
        self.dispatch_txn.borrow_mut().call.contextual = outer;
        self.dispatch_txn.borrow_mut().call.applicability = outer_applicability;
        match result {
            Err(ContextualCandidateFailure::Degraded(failure)) => {
                (ResolveCallStep::Degraded(failure), None)
            }
            completed => {
                if let Ok(arguments) = completed {
                    state.ready = Some((candidate.node, arguments));
                }
                state.resume_at = resume_at;
                record_argument_work(args.len());
                key.args = Arc::from(args);
                self.execute_contextual_call(key.clone(), state)
            }
        }
    }

    /// Type each deferred argument exactly once for this candidate and pass.
    /// The final applicability check still belongs to `check_call_candidate`.
    /// `last_visible`: whether `candidate` is the last of the walk's visible
    /// candidates — the one whose own error the checker reports when every
    /// candidate rejects.
    pub(super) fn prepare_contextual_candidate(
        &self,
        key: &ResolveCallKey,
        candidate: &SignatureRef,
        arguments: &[CallArgument],
        budget: &mut CallResolutionBudget<'_>,
        last_visible: bool,
    ) -> Result<Vec<CallArgument>, ContextualCandidateFailure> {
        let mut state = self
            .dispatch_txn
            .borrow_mut()
            .call
            .contextual
            .take()
            .expect("contextual route");
        let result = self.prepare_contextual_candidate_inner(
            key,
            candidate,
            arguments,
            budget,
            last_visible,
            &mut state,
        );
        self.dispatch_txn.borrow_mut().call.contextual = Some(state);
        result
    }

    fn prepare_contextual_candidate_inner(
        &self,
        key: &ResolveCallKey,
        candidate: &SignatureRef,
        arguments: &[CallArgument],
        budget: &mut CallResolutionBudget<'_>,
        last_visible: bool,
        state: &mut ContextualCallState,
    ) -> Result<Vec<CallArgument>, ContextualCandidateFailure> {
        if state
            .ready
            .as_ref()
            .is_some_and(|(signature, _)| *signature == candidate.node)
        {
            return Ok(state.ready.take().expect("completed candidate").1);
        }
        let resumed = state.active.take();
        let original = state.original.get_or_insert_with(|| {
            record_argument_work(arguments.len());
            arguments.to_vec()
        });
        if resumed.is_none()
            && (!original.iter().any(|argument| argument.context_sensitive)
                || !key.explicit_type_args.is_empty()
                || key.args.iter().any(|arg| match arg {
                    CallArgKey::Eager { spread, .. }
                    | CallArgKey::ProgramExpression { spread, .. } => *spread,
                }))
        {
            return Ok(arguments.to_vec());
        }
        state.diagnostics_enabled = true;
        let graph = self.graph();
        let (params, type_parameters) = if resumed.is_none() {
            let Some(data) = graph.node_data(candidate.node) else {
                return Ok(arguments.to_vec());
            };
            let Some(header) =
                data.callable_header(&crate::semantic_query::ResolveOverloadSetConsumer::witness())
            else {
                return Ok(arguments.to_vec());
            };
            (
                Arc::clone(header.params),
                Arc::clone(header.type_parameters),
            )
        } else {
            (Arc::from([]), Arc::from([]))
        };
        let (receiver, ordinary) = crate::semantic_query::split_this_receiver(&params);
        let rest = ordinary
            .last()
            .filter(|param| param.rest)
            .map(|param| RestParam {
                param,
                shape: self.rest_shape(param.ty),
            });
        let generic_rest = rest.as_ref().and_then(|rest| match rest.shape {
            RestShape::GenericTuple(param) => Some((param, ordinary.len() - 1)),
            _ => None,
        });
        if resumed.is_none() {
            let (required, maximum, _) = call_candidate_arity(ordinary, rest.as_ref());
            if original.len() < required || maximum.is_some_and(|max| original.len() > max) {
                state.arity.add(
                    required,
                    maximum.unwrap_or(ordinary.len()),
                    maximum.is_none(),
                    original.len(),
                );
                return Err(ContextualCandidateFailure::Mismatch);
            }
            state.arity_fit = true;
            state.arity_candidates.insert(candidate.node);
            // A callback in the generic tail has no positional template yet.
            // Keep the ordinary executor's contextual recheck for that shape.
            if generic_rest.is_some_and(|(_, start)| {
                original
                    .get(start..)
                    .unwrap_or(&[])
                    .iter()
                    .any(|arg| arg.context_sensitive)
            }) {
                return Ok(arguments.to_vec());
            }
        }
        let (mut progress, session_id) = if let Some(progress) = resumed {
            verter_debug_assert_eq!(progress.signature.node, candidate.node);
            let id = progress.session.as_ref().expect("suspended collector").id;
            (progress, id)
        } else {
            state.completed = None;
            state.diagnostic = None;
            let setup = InferenceSessionSetup::new(
                self.graph(),
                self.call_inference_infos(&type_parameters),
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
            let session = self.take_collecting_session(id);
            (
                ContextualCandidate {
                    signature: candidate.clone(),
                    arguments: {
                        record_argument_work(original.len());
                        original.clone()
                    },
                    deferred: Vec::new(),
                    cursor: 0,
                    eager_mismatch: false,
                    session: Some(session),
                    positions: type_parameters
                        .iter()
                        .enumerate()
                        .map(|(i, decl)| (decl.param, i))
                        .collect(),
                    constrained_siblings: {
                        let mut siblings: FxHashMap<_, Vec<_>> = FxHashMap::default();
                        for decl in type_parameters.iter() {
                            if let Some(constraint) =
                                decl.constraint.filter(|&node| node != decl.param)
                            {
                                siblings.entry(constraint).or_default().push(decl.param);
                            }
                        }
                        siblings
                    },
                    parameters: type_parameters,
                    fixed: FxHashMap::default(),
                    pending_target: None,
                },
                id,
            )
        };
        let declared = Arc::clone(&progress.parameters);
        self.restore_collecting_session(&mut progress.session);
        let result = (|| {
            if progress.pending_target.is_some() {
                let (position, _) = progress.deferred[progress.cursor];
                let typed = state
                    .delivery
                    .take()
                    .or_else(|| arguments.get(position).cloned())
                    .filter(|arg| !arg.context_sensitive)
                    .ok_or(ContextualCandidateFailure::Degraded(
                        ResolveCallFailure::Undecidable,
                    ))?;
                let _target = progress.pending_target.take().expect("pending callback");
                // The delivery is THIS candidate's own contextual typing of
                // the argument; the first typing of a position is also its
                // once-assigned context, which every later candidate relates
                // by. The deferred loop below decides the relation.
                state
                    .candidate_typed_arguments
                    .entry((candidate.node, position))
                    .or_insert(typed.clone());
                state.typed_arguments.entry(position).or_insert(typed);
            } else {
                // Eager arguments infer before a context-sensitive callback is
                // visited. Annotated callbacks participate in this pass too.
                let own_return_function = match &candidate.return_carrier {
                    SignatureReturnCarrier::Function(
                        verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                    ) => Some(self.flow_return_key_for(identity).function),
                    _ => None,
                };
                if let Some((param, start)) = generic_rest {
                    self.dispatch_txn
                        .borrow_mut()
                        .active_session_mut()
                        .expect("contextual collector")
                        .set_implied_arity(param, original.len().saturating_sub(start));
                }
                if let Some(receiver) = receiver {
                    let source = key.receiver.unwrap_or_else(|| {
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void))
                    });
                    let before = self.accepted_inference_deposits();
                    let relation_source = self.receiver_relation_source(source, receiver.ty);
                    let step =
                        self.call_receiver_relation(relation_source, receiver.ty, source, budget);
                    if !budget.charge_accepted_deposits(self.accepted_inference_deposits() - before)
                    {
                        return Err(ContextualCandidateFailure::Degraded(
                            ResolveCallFailure::Budget,
                        ));
                    }
                    if decided_call_relation(step, own_return_function.as_ref())
                        .map_err(ContextualCandidateFailure::Degraded)?
                        == Some(false)
                    {
                        return Err(ContextualCandidateFailure::Mismatch);
                    }
                }
                if let Some((param, start)) = generic_rest {
                    let before = self.accepted_inference_deposits();
                    let step = self.generic_rest_relation(original, start, param, budget, true);
                    if !budget.charge_accepted_deposits(self.accepted_inference_deposits() - before)
                    {
                        return Err(ContextualCandidateFailure::Degraded(
                            ResolveCallFailure::Budget,
                        ));
                    }
                    if decided_call_relation(step, own_return_function.as_ref())
                        .map_err(ContextualCandidateFailure::Degraded)?
                        == Some(false)
                    {
                        return Err(ContextualCandidateFailure::Mismatch);
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
                    if generic_rest.is_some_and(|(_, start)| position >= start) {
                        continue;
                    }
                    let mut evidence =
                        super::super::canonical_algebra::CanonicalEvidence::default();
                    let target = mapped_parameter_type(
                        ordinary,
                        rest.as_ref(),
                        position,
                        original.len(),
                        argument.clone(),
                        graph,
                        self.construction_meter(),
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
                                // The flow pass already applied mutable-slot
                                // widening. Preserve pins in checked returns.
                                literal_mode: ArgumentLiteralMode::Literal,
                                ..argument.clone()
                            };
                            match self
                                .collect_contextual_argument(&view, target, budget, &declared, true)
                            {
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
                    let mut argument = argument.clone();
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
                        generic_arguments.push((argument, signature, target, position));
                        continue;
                    }
                    if !self
                        .collect_contextual_argument(&argument, target, budget, &declared, false)?
                    {
                        if !self.hold_eager_failure(
                            &mut state.diagnostic,
                            &mut progress,
                            argument.authored,
                            argument.node,
                            self.declared_report_target(ordinary, position, target),
                            target,
                            last_visible,
                        ) {
                            return Err(ContextualCandidateFailure::Mismatch);
                        }
                        break;
                    }
                }
                if !progress.eager_mismatch {
                    for (mut argument, signature, target, position) in generic_arguments {
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
                        if !self.collect_contextual_argument(
                            &argument, target, budget, &declared, false,
                        )? {
                            if !self.hold_eager_failure(
                                &mut state.diagnostic,
                                &mut progress,
                                argument.authored,
                                argument.node,
                                self.declared_report_target(ordinary, position, target),
                                target,
                                last_visible,
                            ) {
                                return Err(ContextualCandidateFailure::Mismatch);
                            }
                            break;
                        }
                    }
                }
            }
            while let Some(&(position, target)) = progress.deferred.get(progress.cursor) {
                let (contextual, inference_target) = self
                    .contextual_candidate_mapper(&mut progress, target, session_id, budget)
                    .map_err(ContextualCandidateFailure::Degraded)?;
                // The once-assigned context decides the candidate's
                // applicability (`assignContextualParameters`: a callback's
                // parameter context is assigned once, by the first candidate
                // that types it, and later candidates relate by that typing).
                if let Some(assigned) = state.typed_arguments.get(&position).cloned() {
                    if self.collect_contextual_argument(
                        &assigned,
                        inference_target,
                        budget,
                        &declared,
                        false,
                    )? {
                        progress.arguments[position] = assigned;
                        progress.cursor += 1;
                        continue;
                    }
                    // Rejected through its once-assigned context. The
                    // checker's reported failure is the candidate's OWN
                    // typing of the callback — the last overload's error
                    // elaborates the callback under its own parameters — so
                    // the recipe names it where it fails, and nothing where
                    // it relates (an unexplained rejection reports no
                    // position).
                    if let Some(own) = state
                        .candidate_typed_arguments
                        .get(&(candidate.node, position))
                        .cloned()
                    {
                        let reportable = own.node == assigned.node
                            || !self.collect_contextual_argument(
                                &own,
                                inference_target,
                                budget,
                                &declared,
                                false,
                            )?;
                        if reportable {
                            state.diagnostic = Some(super::argument_not_assignable(
                                own.authored,
                                own.node,
                                inference_target,
                            ));
                        }
                        state.resume_at = None;
                        return Err(ContextualCandidateFailure::Mismatch);
                    }
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
            if progress.eager_mismatch {
                // Every deferred callback typed and held; the eager failure
                // stands as the candidate's own.
                return Err(ContextualCandidateFailure::Mismatch);
            }
            state.resume_at = None;
            Ok(std::mem::take(&mut progress.arguments))
        })();
        progress.session = Some(self.take_collecting_session(session_id));
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
            state.completed = Some(ContextualCandidateBindings {
                signature: candidate.node,
                fixed: progress.fixed,
                diagnostic: state.diagnostic,
            });
            // Nothing holds the finished collector any more: it returns to
            // the transaction abandoned, as a rejected candidate's does.
            self.restore_collecting_session(&mut progress.session);
            self.abandon_session(session_id);
        }
        result
    }

    /// Record an eager argument's rejection of `candidate`: `false` when
    /// the candidate rejects on it now, `true` when the rejection is held
    /// for the deferred loop to complete. The LAST candidate's reported
    /// failure types a callback that precedes the eager argument under its
    /// own parameters first, so the recipe the checker reports may be that
    /// callback's, not this argument's. An eager failure against a target
    /// still reading the candidate's own type parameters is an
    /// inference-order reading, not the checker's verdict — the checker
    /// fixes its parameters before applicability — so a candidate so
    /// rejects with no recipe and the recovery reading answers; so does
    /// any candidate but the last, whose own error the checker never
    /// reports.
    fn hold_eager_failure(
        &self,
        diagnostic: &mut Option<crate::semantic_query::CheckerDiagnosticRecipe>,
        progress: &mut ContextualCandidate,
        authored: u32,
        node: SemanticNodeId,
        report_target: SemanticNodeId,
        target: SemanticNodeId,
        last_visible: bool,
    ) -> bool {
        let deferred_before = progress
            .deferred
            .iter()
            .any(|&(deferred, _)| deferred <= authored as usize);
        let inference_order = progress
            .parameters
            .iter()
            .any(|decl| self.mentions_node(target, decl.param));
        if deferred_before && (inference_order || !last_visible) {
            return false;
        }
        *diagnostic = Some(super::argument_not_assignable(
            authored,
            node,
            report_target,
        ));
        if !deferred_before {
            return false;
        }
        progress.eager_mismatch = true;
        true
    }

    /// Relate one argument to `target` collecting its candidates;
    /// `first_pass` when the argument is a context-sensitive literal's
    /// first-pass reading.
    fn collect_contextual_argument(
        &self,
        argument: &CallArgument,
        target: SemanticNodeId,
        budget: &mut CallResolutionBudget<'_>,
        declared: &[TypeParamDecl],
        first_pass: bool,
    ) -> Result<bool, ContextualCandidateFailure> {
        let before = self.accepted_inference_deposits();
        let step = self.call_argument_relation(
            argument.node,
            target,
            argument.freshness_origin,
            budget,
            argument.literal_mode,
            argument.literal_provenance.as_deref(),
            declared,
            first_pass,
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

    /// Release the suspended candidate collector a contextual continuation
    /// holds, which no delivery will advance: it returns to the transaction
    /// abandoned, as a rejected candidate's does.
    pub(in crate::project_semantic_dispatch) fn release_contextual_collector(
        &self,
        state: &mut ContextualCallState,
    ) {
        let Some(session) = state.active.take().and_then(|progress| progress.session) else {
            return;
        };
        let id = session.id;
        self.restore_collecting_session(&mut Some(session));
        self.abandon_session(id);
    }

    /// Take a collecting session out of the transaction, for its candidate's
    /// continuation to hold while something that must not deposit into it
    /// runs.
    pub(super) fn take_collecting_session(&self, id: SessionId) -> InferenceSession {
        let mut txn = self.dispatch_txn.borrow_mut();
        let position = txn
            .relation
            .session_position(id)
            .expect("contextual collector");
        txn.relation.sessions.remove(position)
    }

    /// Return a held collecting session to the transaction, in session order.
    pub(super) fn restore_collecting_session(&self, session: &mut Option<InferenceSession>) {
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
                if let Some(siblings) = progress.constrained_siblings.get(&node) {
                    stack.extend(siblings);
                }
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
        let retained_substitution = CanonicalTypeSubstitution::new(self.graph(), retained.clone());
        let declarations: Vec<_> = positions
            .iter()
            .map(|&position| {
                let mut declaration = progress.parameters[position].copy();
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
        let fixed = CanonicalTypeSubstitution::new(self.graph(), fixed);
        let fixed = self
            .fresh_widened_substitution_outside_top_level(id, &fixed, None)?
            .unwrap_or(fixed);
        let inferred_params: FxHashSet<_> = inferred.iter().map(|(param, _)| *param).collect();
        let inference_mapper = CanonicalTypeSubstitution::new(
            self.graph(),
            fixed
                .bindings()
                .iter()
                .copied()
                .filter(|(param, _)| inferred_params.contains(param))
                .collect(),
        );
        let contextual_mapper = CanonicalTypeSubstitution::new(
            self.graph(),
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
