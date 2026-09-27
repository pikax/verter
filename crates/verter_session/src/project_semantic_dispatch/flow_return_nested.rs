//! Nested function values evaluated from an explicit stack of evaluators.
//!
//! A nested function value's signature is composed from its body's
//! evaluation, which a new evaluator (its own frame state) performs; a body
//! that returns a function value demands that function's signature in turn.
//! Evaluating each body inside the one around it cost a native level per
//! nested function, about 53 KiB unoptimized. [`FlowEvaluator::eval_region`]
//! instead drives a stack of evaluators: a return statement whose argument
//! reaches a nested function value suspends its region
//! ([`RegionProgress::Nested`]); the function is prepared on the evaluator
//! that suspended ([`FlowEvaluator::prepare_nested`]), its own evaluator is
//! pushed and its body run; when the body completes, the evaluator is popped,
//! the signature composed on the parent ([`FlowEvaluator::finish_nested`]),
//! and the parent's region resumes with it. The data a nested evaluator
//! borrows (its parameters, binder environment and slice content) is owned
//! by the drive's [`NestedArena`], and the frames around it are a shared
//! chain, so no evaluator borrows from a native frame of the one around it.
//!
//! A nested function value in any other position (a call's argument, a
//! variable's initializer) is prepared, driven and finished in place
//! ([`FlowEvaluator::eval_nested_function`]); a return inside it is driven
//! from the stack again.

use std::cell::RefCell;
use std::sync::Arc;

use super::contextual::ContextualSignature;
use super::*;

/// A nested function value demanded where an evaluator suspends.
pub(super) struct NestedDemand<'e> {
    pub(super) function: &'e verter_semantic::analysis::function_program::FunctionProgramKey,
    pub(super) context: &'e Arc<crate::flow_slice_content::NestedFlowContext>,
    pub(super) has_declared_return: bool,
    pub(super) extended_captures: &'e [SkeletonBindingId],
    pub(super) declared_evolving_captures:
        &'e [verter_semantic::analysis::function_program::FlowBindingIdentity],
}

/// What a nested function's own evaluator borrows, owned by the drive.
pub(super) struct NestedOwned {
    content: Arc<crate::flow_slice_content::SliceContent>,
    params: Vec<SemanticNodeId>,
    binder_env: FlowBinderEnv,
}

/// The owned data of the nested evaluators a drive runs, each released as
/// soon as its evaluator is popped. Allocation hands out a reference that
/// stays valid while the item lives: every item is boxed, so the items
/// vector growing never moves one, and an item is only released (the last
/// one, after the evaluator borrowing it has been dropped) by
/// [`NestedArena::release_last`].
pub(super) struct NestedArena<T> {
    items: RefCell<Vec<Box<T>>>,
}

impl<T> NestedArena<T> {
    pub(super) fn new() -> Self {
        Self {
            items: RefCell::new(Vec::new()),
        }
    }

    /// Store `value`; its slot is the one [`Self::release_last`] releases.
    fn alloc(&self, value: T) -> (&T, usize) {
        let boxed = Box::new(value);
        let item: *const T = &*boxed;
        let mut items = self.items.borrow_mut();
        items.push(boxed);
        let slot = items.len() - 1;
        // SAFETY: the box's heap allocation never moves (the vector only
        // moves the `Box` pointer), and it is dropped only by
        // `release_last` — called once the one evaluator borrowing it has
        // been dropped — or with the arena, which outlives every borrow.
        (unsafe { &*item }, slot)
    }

    /// Release the last item, the one at `slot`, whose borrowers have all
    /// been dropped.
    fn release_last(&self, slot: usize) {
        let mut items = self.items.borrow_mut();
        assert_eq!(items.len(), slot + 1, "nested items release in stack order");
        items.pop();
    }
}

/// A nested function's body under evaluation by its own evaluator.
pub(super) struct NestedActivation<'d, 'a> {
    evaluator: Box<FlowEvaluator<'d, 'a>>,
    run: RegionRun<'a>,
    owned: &'a NestedOwned,
    slot: usize,
    finish: NestedFinish,
}

/// What composing a nested function's signature needs once its body has
/// been evaluated.
pub(super) struct NestedFinish {
    circular: bool,
    declared_return_none: bool,
    signature_params: Vec<crate::semantic_query::FunctionParam>,
    type_param_decls: Vec<crate::semantic_query::TypeParamDecl>,
    contextual: Option<ContextualSignature>,
}

/// The locals of a nested function's signature evaluation its body's
/// evaluator is built from.
pub(super) struct NestedChildParts {
    pub(super) params: Vec<SemanticNodeId>,
    pub(super) signature_params: Vec<crate::semantic_query::FunctionParam>,
    pub(super) type_param_decls: Vec<crate::semantic_query::TypeParamDecl>,
    pub(super) binder_env: FlowBinderEnv,
    pub(super) captured_products: FlowProductStore,
    pub(super) declared_capture_types: rustc_hash::FxHashMap<
        verter_semantic::analysis::function_program::FlowBindingIdentity,
        SemanticNodeId,
    >,
    pub(super) execution_selection: Arc<super::super::flow_solve::FlowExecutionSelection>,
    pub(super) plan: Option<Arc<super::super::flow_solve::FlowDemandPlan>>,
    pub(super) bound: crate::cache_runtime::flow_slice_node::BoundFlowGraph,
    pub(super) skeleton: Arc<FunctionBodySkeleton>,
}

/// A nested function's signature evaluation up to its body's evaluation.
pub(super) enum NestedSignatureStep {
    /// The signature, composed without evaluating the body.
    Done(SemanticNodeId),
    /// The body's evaluator is to be built from these.
    Child(Box<NestedChildParts>),
}

/// A nested function prepared for evaluation.
pub(super) enum NestedPrepared<'d, 'a> {
    /// Its signature, with nothing to evaluate.
    Value(SemanticNodeId),
    /// Its body, to evaluate on its own evaluator.
    Child(Box<NestedActivation<'d, 'a>>),
}

/// How a drive ends.
enum DriveEnd {
    /// The region it was started on completed.
    Region((Result<Vec<FlowContribution>, FlowReturnFailure>, bool)),
    /// The nested function it was started on is composed.
    Nested(SemanticNodeId),
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Evaluate a region, driving the nested functions its return
    /// statements reach from the stack (see the module docs).
    pub(super) fn eval_region(
        &mut self,
        region: &crate::flow_slice_content::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        let arena = NestedArena::new();
        let run = self.start_region_run(region);
        match self.drive(&arena, Some(run), Vec::new()) {
            DriveEnd::Region(outcome) => outcome,
            DriveEnd::Nested(_) => unreachable!("a region drive ends with its region"),
        }
    }

    /// Evaluate a nested function value's signature: bind its OWN type
    /// parameters in scope (the SAME binder environment the root
    /// evaluation uses), lower its parameters, evaluate its body in a
    /// fresh frame seeded with the CAPTURED enclosing bindings (holds the
    /// nested evaluation met ride the outer frame's hold set), and
    /// compose the `Signature` node.
    ///
    /// Closure capture uses the selected graph's exact captured identities.
    /// Each input comes from the enclosing continuation at the function
    /// value's own position. The child has its own indexed graph, input
    /// basis, and execution capability; its local bindings cannot alias an
    /// enclosing slot merely because their names or numeric IDs match.
    ///
    /// It is prepared on this evaluator, its body driven from the stack, and
    /// its signature composed here.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn eval_nested_function(
        &mut self,
        function: &verter_semantic::analysis::function_program::FunctionProgramKey,
        context: &Arc<crate::flow_slice_content::NestedFlowContext>,
        has_declared_return: bool,
        outer_env: &FlowBinderEnv,
        extended_captures: &[SkeletonBindingId],
        declared_evolving_captures: &[verter_semantic::analysis::function_program::FlowBindingIdentity],
        contextual: Option<&ContextualSignature>,
    ) -> SemanticNodeId {
        let arena = NestedArena::new();
        let demand = NestedDemand {
            function,
            context,
            has_declared_return,
            extended_captures,
            declared_evolving_captures,
        };
        let prepared = self.prepare_nested(demand, outer_env, contextual.cloned(), &arena);
        let node = match prepared {
            NestedPrepared::Value(node) => node,
            NestedPrepared::Child(child) => match self.drive(&arena, None, vec![*child]) {
                DriveEnd::Nested(node) => node,
                DriveEnd::Region(_) => unreachable!("a nested drive ends with its function"),
            },
        };
        node
    }

    /// Run `root` (this evaluator's region) and the nested evaluators on
    /// `stack` until the root region completes, or, with no root, until the
    /// bottom nested function is composed on this evaluator.
    fn drive<'r, 'a>(
        &mut self,
        arena: &'a NestedArena<NestedOwned>,
        mut root: Option<RegionRun<'r>>,
        mut stack: Vec<NestedActivation<'d, 'a>>,
    ) -> DriveEnd
    where
        'b: 'a,
    {
        let mut delivered: Option<RegionDelivery> = None;
        loop {
            // The evaluator on top runs; a nested function it suspends at is
            // prepared on it.
            let prepared = match stack.last_mut() {
                Some(top) => match top.evaluator.run_region(&mut top.run, delivered.take()) {
                    RegionProgress::Nested(demand) => {
                        let outer_env = top.evaluator.binder_env;
                        Some(top.evaluator.prepare_nested(demand, outer_env, None, arena))
                    }
                    RegionProgress::Done(outcome) => {
                        let finished = stack.pop().expect("the evaluator on top");
                        let node = match stack.last_mut() {
                            Some(parent) => {
                                parent.evaluator.finish_nested(finished, outcome, arena)
                            }
                            None => {
                                let node = self.finish_nested(finished, outcome, arena);
                                if root.is_none() {
                                    return DriveEnd::Nested(node);
                                }
                                node
                            }
                        };
                        delivered = Some(RegionDelivery::Nested(node));
                        None
                    }
                },
                None => {
                    let run = root
                        .as_mut()
                        .expect("a drive with no nested evaluator runs its root");
                    match self.run_region(run, delivered.take()) {
                        RegionProgress::Nested(demand) => {
                            let outer_env = self.binder_env;
                            Some(self.prepare_nested(demand, outer_env, None, arena))
                        }
                        RegionProgress::Done(outcome) => return DriveEnd::Region(outcome),
                    }
                }
            };
            match prepared {
                Some(NestedPrepared::Value(node)) => delivered = Some(RegionDelivery::Nested(node)),
                Some(NestedPrepared::Child(child)) => stack.push(*child),
                None => {}
            }
        }
    }

    /// Prepare a nested function value on this evaluator: its slice content
    /// and signature up to its body's evaluation, and the body's own
    /// evaluator, whose borrowed data `arena` owns.
    pub(super) fn prepare_nested<'a>(
        &mut self,
        demand: NestedDemand<'_>,
        outer_env: &FlowBinderEnv,
        contextual: Option<ContextualSignature>,
        arena: &'a NestedArena<NestedOwned>,
    ) -> NestedPrepared<'d, 'a>
    where
        'b: 'a,
    {
        let NestedDemand {
            function,
            context,
            has_declared_return,
            extended_captures,
            declared_evolving_captures,
        } = demand;
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from(self.canonical),
                owner: function.declaration.owner,
                symbol: Arc::clone(&function.declaration.name),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: function.part.clone(),
            overload_ordinal: function.overload_ordinal,
        };
        let key = self.dispatch.flow_return_key_for(&identity);
        let prepared = (|| {
            let site = self.dispatch.flow_slice_demand_site(&key).ok()?;
            let flow_slice = self.dispatch.ctx.project_type_store().flow_slice();
            let index = site
                .indexed
                .shallow_state
                .decl_bodies()
                .function_program_index();
            let entry = index.get(function)?.entry();
            let skeleton = flow_slice.skeleton_for(&site.slice_key_function, self.dispatch.ctx)?;
            let bound = flow_slice.bound_graph_for(&site.slice_key_function)?;
            let planned_and_selection = if has_declared_return {
                None
            } else {
                let crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::Planned(planned) =
                    crate::cache_runtime::lookup(
                        flow_slice.hash_node(),
                        site.slice_key.clone(),
                        self.dispatch.ctx,
                    )?
                else {
                    return None;
                };
                let lowered = crate::cache_runtime::lookup(
                    flow_slice.lowered_node(),
                    crate::cache_runtime::flow_slice_node::FlowSliceLoweredKey {
                        hash_key: site.slice_key.clone(),
                        slice_hash: planned.hash(),
                    },
                    self.dispatch.ctx,
                )?;
                Some((
                    planned,
                    crate::flow_slice_content::FlowSliceSelection::from_slice_ir(&lowered),
                ))
            };
            let content = site
                .indexed
                .shallow_state
                .decl_bodies()
                .flow_slice_content_with_context(
                    entry,
                    planned_and_selection
                        .as_ref()
                        .map(|(_, selection)| selection.clone()),
                    &bound,
                    Some(Arc::clone(context)),
                    self.policy(),
                )?;
            Some((
                content,
                skeleton,
                bound,
                planned_and_selection.map(|(planned, _)| planned),
                entry.span.start,
            ))
        })();
        let Some((content, skeleton, bound, planned, anchor)) = prepared else {
            self.record_degradation(crate::semantic_query::FlowReturnDegradation::UnresolvedValue);
            return NestedPrepared::Value(self.unmodeled_position());
        };
        if content.budget_failure.is_some() {
            self.record_degradation(
                crate::semantic_query::FlowReturnDegradation::UnmodeledPosition,
            );
            return NestedPrepared::Value(self.unmodeled_position());
        }
        // A return read again while it is being resolved (a local
        // function reached from its own body other than as a bare tail
        // call, or mutually recursive declarations) is the checker's
        // circular return: every return from the re-entered one inward is
        // `any` (`getReturnTypeOfSignature` when `popTypeResolution`
        // fails). A declared return is never resolved from the body.
        let reentered = self
            .resolving_functions
            .borrow()
            .iter()
            .position(|(resolving, _)| resolving == function);
        if let Some(position) = reentered {
            for (_, circular) in self.resolving_functions.borrow_mut()[position..].iter_mut() {
                *circular = true;
            }
        }
        let circular = reentered.is_some() && content.declared_return.is_none();
        if !circular {
            self.resolving_functions
                .borrow_mut()
                .push((function.clone(), false));
        }
        let step = self.nested_signature_prefix(
            &content.params,
            &content.type_parameters,
            context,
            declared_evolving_captures,
            content.declared_return.as_ref(),
            content.declared_predicate.as_ref(),
            &content.bindings,
            skeleton,
            bound,
            planned.as_deref(),
            anchor,
            &key,
            outer_env,
            extended_captures,
            circular,
            contextual.as_ref(),
        );
        let declared_return_none = content.declared_return.is_none();
        let parts = match step {
            NestedSignatureStep::Done(signature) => {
                return NestedPrepared::Value(self.nested_function_postlude(
                    circular,
                    declared_return_none,
                    signature,
                ))
            }
            NestedSignatureStep::Child(parts) => *parts,
        };
        let NestedChildParts {
            params,
            signature_params,
            type_param_decls,
            binder_env,
            captured_products,
            declared_capture_types,
            execution_selection,
            plan,
            bound,
            skeleton,
        } = parts;
        let enclosing_frames = self.enclosing_frames.with(
            self.bindings.function().clone(),
            self.binder_env,
            self.products.clone(),
        );
        let (owned, slot) = arena.alloc(NestedOwned {
            content,
            params,
            binder_env,
        });
        let capture_context = context;
        let mut evaluator = Box::new(FlowEvaluator {
            dispatch: self.dispatch,
            call_arguments: Arc::clone(&owned.content.call_arguments),
            self_slot: None,
            resolving_functions: std::rc::Rc::clone(&self.resolving_functions),
            canonical: self.canonical,
            owner: self.owner,
            nullability: self.nullability,
            no_implicit_any: self.no_implicit_any,
            use_unknown_in_catch_variables: self.use_unknown_in_catch_variables,
            no_implicit_this: self.no_implicit_this,
            params: &owned.params,
            param_names: &owned.content.params,
            binder_env: &owned.binder_env,
            enclosing_frames,
            bindings: Arc::clone(&owned.content.bindings),
            skeleton: Arc::clone(&skeleton),
            flow_graph: Arc::clone(&bound.bundle().graph),
            execution_selection,
            plan,
            anchor,
            products: captured_products,
            narrowing_writes: Vec::new(),
            observations: BodyCompletionObservations::none(),
            // A nested function value always evaluates its WHOLE
            // return (its signature's return type) — the member
            // filter is a top-level demand axis.
            member_filter: None,
            holds: Vec::new(),
            yield_contributions: Vec::new(),
            degradation: None,
            pending_statement_gap: None,
            pattern_write_definition: None,
            correlated_groups: Vec::new(),
            guard_aliases: rustc_hash::FxHashMap::default(),
            auto_typed_locals: rustc_hash::FxHashSet::default(),
            inferred_declared_locals: rustc_hash::FxHashMap::default(),
            declared_capture_types,
            evolving_locals: rustc_hash::FxHashSet::default(),
            circular_inferred: rustc_hash::FxHashSet::default(),
            unwidened_views: rustc_hash::FxHashMap::default(),
            regular_right_operands: rustc_hash::FxHashSet::default(),
            declared_return: None,
            call_fresh_literal_returns: Vec::new(),
            break_exits: Vec::new(),
            return_edges: Vec::new(),
            throw_points: Vec::new(),
            collect_throw_points: false,
            scope_shadows: Vec::new(),
            call_evidence: Vec::new(),
            call_drive: None,
            expression_write_nodes: rustc_hash::FxHashMap::default(),
            capture_write_lookahead: rustc_hash::FxHashMap::default(),
            executed_walk: ExecutedSliceWalk::default(),
            heritage_self_roots: Vec::new(),
            inferred_predicate: None,
            checker_diagnostics: Vec::new(),
            declared_reads: false,
            // A class expression member (or an arrow it creates) runs
            // against the receiver the class binds.
            receiver: matches!(
                capture_context.this(),
                Some(crate::flow_slice_content::SliceThis::Receiver)
            )
            .then(|| self.receiver.clone())
            .flatten(),
        });
        let body = &owned.content.body;
        evaluator.seed_hoisted_var_declarations(body);
        let run = evaluator.start_region_run(body);
        NestedPrepared::Child(Box::new(NestedActivation {
            evaluator,
            run,
            owned,
            slot,
            finish: NestedFinish {
                circular,
                declared_return_none,
                signature_params,
                type_param_decls,
                contextual,
            },
        }))
    }

    /// Compose a nested function's signature on this evaluator (its parent)
    /// from its body's evaluation, `outcome`, and release its evaluator.
    fn finish_nested<'a>(
        &mut self,
        activation: NestedActivation<'d, 'a>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
        arena: &'a NestedArena<NestedOwned>,
    ) -> SemanticNodeId {
        let NestedActivation {
            evaluator: mut nested_evaluator,
            run,
            owned,
            slot,
            finish,
        } = activation;
        drop(run);
        let NestedFinish {
            circular,
            declared_return_none,
            signature_params,
            type_param_decls,
            contextual,
        } = finish;
        let signature = {
            let graph = self.dispatch.graph();
            let can_fall_through = owned.content.can_fall_through;
            let empty_completion = owned.content.empty_completion;
            let binder_env = &owned.binder_env;
            let skeleton = Arc::clone(&nested_evaluator.skeleton);
            let nested_holds;
            let nested_degradation;
            let nested_observations;
            let nested_yield_contributions;
            let nested_inferred_predicate;
            let (contributors, nested_body_falls_through) = {
                let (outcome, nested_body_falls_through) = outcome;
                nested_evaluator.promote_pending_statement_gap();
                nested_yield_contributions =
                    std::mem::take(&mut nested_evaluator.yield_contributions);
                nested_inferred_predicate = nested_evaluator.inferred_predicate;
                #[cfg(test)]
                if self
                    .dispatch
                    .ctx
                    .host_for_fact_tracer_install()
                    .flow_fault_injection
                    .short_nested_execution_ledger
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    nested_evaluator.executed_walk.aborted = true;
                }
                nested_holds = nested_evaluator.holds.clone();
                let finished = nested_evaluator
                .products
                .finish(nested_evaluator.plan.as_deref())
                .and_then(|evidence| {
                    #[cfg(test)]
                    let evidence = if self.dispatch.ctx.host_for_fact_tracer_install().flow_fault_injection
                        .drop_binding_domain_product.load(std::sync::atomic::Ordering::Relaxed) {
                        None
                    } else { evidence };
                    let Some(plan) = nested_evaluator.plan.as_deref() else {
                        return Ok(evidence);
                    };
                    use super::super::dispatch_txn::flow_obligation_state::FlowObligationBasis;
                    let products_complete = plan.obligation_specs().iter().all(|spec| {
                        let subject = match spec.basis() {
                            // A local function declaration's value is the
                            // declaration itself: it has no slot product.
                            FlowObligationBasis::Binding { slot, .. }
                                if binding_is_local_function_declaration(
                                    &slot.binding,
                                    &nested_evaluator.skeleton,
                                ) =>
                            {
                                return true;
                            }
                            FlowObligationBasis::Binding { slot, .. } => Some(slot.binding.clone()),
                            FlowObligationBasis::CapturedBinding { node, identity, demand, .. } => {
                                let completed = nested_evaluator.executed_walk
                                    .completed_selection(plan.structural_selection())
                                    .is_some_and(|selection| selection.is_selected(*node));
                                if !completed {
                                    return false;
                                }
                                if *demand == super::super::dispatch_txn::flow_obligation_state::FlowCaptureDemand::Effect {
                                    return true;
                                }
                                Some(
                                nested_evaluator
                                    .bindings
                                    .local(identity)
                                    .map(FlowProductSubject::Local)
                                    .unwrap_or_else(|| {
                                        FlowProductSubject::Captured(identity.clone())
                                    }),
                                )
                            },
                            _ => return true,
                        };
                        subject.is_some_and(|subject| {
                            binding_product_evidence(
                                &subject,
                                evidence.as_ref(),
                                &nested_evaluator.products,
                                plan,
                            )
                        })
                    });
                    if products_complete {
                        Ok(evidence)
                    } else {
                        Err(FlowProductFailure::Gap(
                            crate::semantic_query::FlowGap::UnmodeledExpression,
                        ))
                    }
                });
                if let Err(failure) = finished {
                    if matches!(failure, FlowProductFailure::BudgetExceeded(_)) {
                        self.products.execution.borrow_mut().failure = Some(failure);
                    }
                    nested_evaluator.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                    );
                }
                nested_degradation = nested_evaluator.degradation;
                nested_observations = nested_evaluator.observations;
                self.holds.append(&mut nested_evaluator.holds);
                // A call the NESTED body evaluated is still an evaluated call
                // of this evaluation run: the evidence rides the enclosing
                // ledger exactly as the nested holds do — and so does the
                // nested walk (a nested abort shortens THIS run's ledger).
                self.call_evidence
                    .append(&mut nested_evaluator.call_evidence);
                self.executed_walk.absorb(nested_evaluator.executed_walk);
                // A heritage hop the NESTED body's `instanceof` narrowing read
                // is still a cross-file observation of THIS run: it rides the
                // enclosing self-roots exactly as the nested holds do.
                for root in nested_evaluator.heritage_self_roots.drain(..) {
                    if !self.heritage_self_roots.contains(&root) {
                        self.heritage_self_roots.push(root);
                    }
                }
                // A nested body's calls are calls of THIS function's text:
                // their diagnostics ride this frame's value.
                for diagnostic in nested_evaluator.checker_diagnostics.drain(..) {
                    self.record_checker_diagnostic(diagnostic);
                }
                (outcome, nested_body_falls_through)
            };
            // A degraded nested body degrades the enclosing value that
            // embeds its signature.
            if let Some(degradation) = nested_degradation {
                self.record_degradation(degradation);
            }
            // A nested body's OWN frame-level failure — an unmodelled control
            // surface, an empty hold-only cycle — is a fact about the NESTED
            // function's return position, not about the frame that embeds its
            // signature. Propagating it outward is what deleted
            // `{ label: "x", go: (n) => { while (…) { return n } return 0 } }`
            // whole, where the checker publishes
            // `{ label: string; go: (n: number) => number }`. The signature
            // survives with its parameters intact and the typed marker in its
            // RETURN position.
            //
            // A nested function value's body is its own join; its holds ride
            // the OUTER frame's component, so no fixed point closes here and
            // the freshness bit has no later consumer.
            let mut predicate = None;
            // The one fresh literal every contributor of the body return is.
            let lone_fresh_literal = contributors.as_ref().ok().and_then(|contributors| {
                let (first, rest) = contributors.split_first()?;
                (contributors
                    .iter()
                    .all(|contribution| contribution.fresh_literal)
                    && rest
                        .iter()
                        .all(|contribution| contribution.node == first.node))
                .then_some(first.node)
            });
            let return_type = match contributors.and_then(|contributors| {
                self.dispatch.join_flow_return_contributors(
                    contributors,
                    NormalCompletion::minted(
                        can_fall_through.reaches_end(CompletionDischarge::EvaluatorRegionWalk)
                            && nested_body_falls_through,
                        CompletionConstruction::NestedBodyRefinement,
                    ),
                    nested_observations,
                    &nested_holds,
                    nested_degradation,
                    empty_completion,
                    self.nullability,
                )
            }) {
                // The nested signature's return IS the function-kind-wrapped
                // body join (an async arrow's type is `() => Promise<T>`), so
                // the wrap attaches and MATERIALIZES here — the nested body has
                // no equation fixed point of its own ("no fixed point closes
                // here"), and the signature consumes the node directly. A
                // predicate inferred from the body rides beside that return.
                Ok((result, _fresh_seed)) => {
                    let result = self
                        .dispatch
                        .attach_inferred_predicate(result, nested_inferred_predicate);
                    predicate = result.inferred_predicate();
                    let wrapped = self.dispatch.materialize_flow_return_wrap(
                        self.dispatch.attach_function_kind_wrap(
                            result,
                            skeleton.kind,
                            &nested_yield_contributions,
                            binder_env,
                            self.nullability,
                        ),
                    );
                    wrapped.return_type()
                }
                Err(_) => self.unmodeled_position(),
            };
            // A body return that is one fresh literal widens, unless the
            // contextual return is a literal context for it
            // (`getWidenedLiteralLikeTypeForContextualReturnTypeIfNeeded`):
            // `(v) => 1` passed for `(v: string) => T` with `T extends number`
            // returns `1`.
            let return_type = match (contextual, lone_fresh_literal) {
                (Some(contextual), Some(literal))
                    if self.literal_of_contextual_type(literal, contextual.return_type) =>
                {
                    literal
                }
                _ => return_type,
            };
            graph.intern_node(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Call,
                params: Arc::from(signature_params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_param_decls.into_boxed_slice()),
                signature_span: None,
                return_type_span: None,
                // A nested function value's synthesized signature has no
                // authored occurrence anchor and no served position: the
                // return carrier is the interned node itself.
                occurrence: None,
                return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(
                    return_type,
                ),
                predicate,
                is_abstract: false,
            })
        };
        drop(nested_evaluator);
        arena.release_last(slot);
        self.nested_function_postlude(circular, declared_return_none, signature)
    }

    /// A nested function's signature, once evaluated: its frame leaves the
    /// resolving stack, and a signature whose return was read while it was
    /// being resolved (a circular reference) returns `any`.
    fn nested_function_postlude(
        &mut self,
        circular: bool,
        declared_return_none: bool,
        signature: SemanticNodeId,
    ) -> SemanticNodeId {
        let found_circular = !circular
            && self
                .resolving_functions
                .borrow_mut()
                .pop()
                .is_some_and(|(_, circular)| circular);
        if found_circular && declared_return_none {
            return self.signature_returning_any(signature);
        }
        signature
    }
}
