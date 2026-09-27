//! A call's evaluation from the flow evaluator's explicit stack.
//!
//! A call nests without bound through its callee operand (`new B().m().m()`,
//! an IIFE's function value, a tagged template's tag) and through the
//! frame-lowered arguments its executor route types (`f(f(f(1)))`).
//! [`FlowEvaluator::eval_expr`] evaluates both from its stack: a call
//! suspends while its callee operand evaluates, and its value computation
//! ([`FlowEvaluator::eval_call_value`]) runs with that operand's value in
//! hand. When the computation takes the executor route over a lowered
//! argument, it stops there ([`CallDrive::demand`]), the route
//! ([`ResolveCallFrame`]) types the arguments one at a time and suspends
//! for each lowered one, and the computation runs again with the route's
//! answer in hand. Everything the computation does before its executor
//! route reads the graph and the frame, and records nothing, so running
//! it again reaches the route exactly as the first run did.

use std::sync::Arc;

use super::super::call_resolve::ResolveCallStep;
use super::*;
use crate::flow_slice_content::{
    SliceCall, SliceCallArgument, SliceCallArguments, SliceCallSite, SliceExpr,
};

/// What a call's value computation reads from the stack instead of
/// evaluating in place, and the executor route it stopped at.
pub(super) struct CallDrive {
    span: verter_span::Span,
    /// The callee operand's value.
    operand: Option<Positional<SemanticNodeId>>,
    /// Each executor route's answer, by callee.
    answers: Vec<(SemanticNodeId, Option<Positional<CallValue>>)>,
    /// The callee of the executor route the computation stopped at.
    demand: Option<SemanticNodeId>,
    /// Each lowered argument's value, by ordinal, once a route evaluated
    /// it: a second route over the same arguments (a callee's group asked
    /// again after its frame-rooted route did not decide) reads them from
    /// here instead of evaluating the nest inside them again.
    argument_values: Vec<Option<Positional<SemanticNodeId>>>,
}

/// The evidence baseline a call's value is recorded against
/// ([`FlowEvaluator::eval_call`]).
pub(super) struct CallEvidenceMark {
    undecided_relations: u64,
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
}

/// A call being evaluated from [`FlowEvaluator::eval_expr`]'s stack.
pub(super) struct CallInFlight<'e> {
    call: &'e SliceCall,
    site: SliceCallSite,
    arguments: &'e SliceCallArguments,
    evidence: CallEvidenceMark,
    drive: CallDrive,
}

/// A call waiting on a lowered argument its executor route asked for.
pub(super) struct CallArgumentWait<'e> {
    flight: CallInFlight<'e>,
    callee: SemanticNodeId,
    route: ResolveCallFrame<'e>,
    pub(super) lowered: &'e SliceExpr,
}

/// What a call begun from the stack needs next.
pub(super) enum CallStep<'e> {
    /// Its callee operand's value.
    Operand(CallInFlight<'e>, &'e SliceExpr),
    /// A lowered argument's value, for its executor route.
    Argument(Box<CallArgumentWait<'e>>),
    /// Nothing: the call's value.
    Done(Positional<SemanticNodeId>),
}

/// A call's executor route ([`FlowEvaluator::resolve_call_step`]) between
/// its arguments: the arguments typed so far.
pub(super) struct ResolveCallFrame<'e> {
    _serve: crate::host_manage::prepared_decl::IndexedReadyServe,
    callee: SemanticNodeId,
    arguments: &'e SliceCallArguments,
    indexed: Arc<crate::decl_body_memo::IndexedFlowCallExpression>,
    frame_arguments: Option<Arc<[SliceCallArgument]>>,
    args: Vec<crate::semantic_query::CallArgKey>,
    function_arguments: Vec<Option<SliceExpr>>,
    /// The binding of the argument whose lowered value is awaited.
    awaiting: Option<FlowIndexedArgumentBinding>,
}

/// What a call's executor route needs next.
pub(super) enum ResolveCallProgress<'e> {
    /// The value of a lowered argument.
    Argument(ResolveCallFrame<'e>, &'e SliceExpr),
    /// Nothing: the route's step.
    Done(Option<Positional<ResolveCallStep>>),
}

/// One argument's type, and how it was typed.
struct TypedArgument {
    node: Option<SemanticNodeId>,
    fresh_call: bool,
    const_view: Option<SemanticNodeId>,
}

/// The operand a call's value computation evaluates first, if it has one.
fn call_operand(call: &SliceCall) -> Option<&SliceExpr> {
    match call {
        SliceCall::Nested(operand)
        | SliceCall::Member {
            receiver: operand, ..
        }
        | SliceCall::OnValue {
            object: operand, ..
        }
        | SliceCall::Construct(operand)
        | SliceCall::TaggedTemplate(operand) => Some(operand),
        _ => None,
    }
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Begin evaluating a call from [`Self::eval_expr`]'s stack. A call is a
    /// throw point: an enclosing `try`'s catch / finally can be entered
    /// from HERE, with the state as it stands BEFORE the call.
    pub(super) fn begin_call<'e>(
        &mut self,
        call: &'e SliceCall,
        site: SliceCallSite,
        arguments: &'e SliceCallArguments,
    ) -> CallStep<'e> {
        self.capture_throw_point();
        let flight = CallInFlight {
            call,
            site,
            arguments,
            evidence: self.call_evidence_mark(),
            drive: CallDrive {
                span: site.span(),
                operand: None,
                answers: Vec::new(),
                demand: None,
                argument_values: Vec::new(),
            },
        };
        match call_operand(call) {
            Some(operand) => CallStep::Operand(flight, operand),
            None => self.drive_call(flight),
        }
    }

    /// Continue a call with its callee operand's value.
    pub(super) fn resume_call_operand<'e>(
        &mut self,
        mut flight: CallInFlight<'e>,
        value: Positional<SemanticNodeId>,
    ) -> CallStep<'e> {
        flight.drive.operand = Some(value);
        self.drive_call(flight)
    }

    /// Continue a call with the value of the lowered argument its executor
    /// route asked for.
    pub(super) fn resume_call_argument<'e>(
        &mut self,
        wait: CallArgumentWait<'e>,
        value: Positional<SemanticNodeId>,
    ) -> CallStep<'e> {
        let CallArgumentWait {
            mut flight,
            callee,
            route,
            lowered,
        } = wait;
        let ordinal = route.args.len();
        if flight.drive.argument_values.len() <= ordinal {
            flight.drive.argument_values.resize(ordinal + 1, None);
        }
        flight.drive.argument_values[ordinal] = Some(value.clone());
        let progress = self.deliver_resolve_call_argument(route, lowered, value);
        self.continue_call_route(flight, callee, progress)
    }

    /// Run a call's value computation with what the stack has delivered;
    /// stopped at an executor route over a lowered argument, start that
    /// route.
    fn drive_call<'e>(&mut self, mut flight: CallInFlight<'e>) -> CallStep<'e> {
        let enclosing = self.call_drive.replace(flight.drive);
        let value = self.eval_call_value(flight.call, flight.site, flight.arguments);
        flight.drive = std::mem::replace(&mut self.call_drive, enclosing)
            .expect("the call's drive stays installed while its value computes");
        if let Some(callee) = flight.drive.demand.take() {
            let progress = self.start_resolve_call(callee, flight.site, flight.arguments);
            return self.continue_call_route(flight, callee, progress);
        }
        let value = self.finish_call_evidence(flight.site, flight.evidence, value);
        CallStep::Done(match value {
            Positional::Value(value) => Positional::Value(value.into_node()),
            Positional::Hold => Positional::Hold,
            Positional::Unmodeled => Positional::Unmodeled,
        })
    }

    /// Continue a call whose executor route for `callee` made `progress`:
    /// suspend for the argument it asks for, or run the value computation
    /// again with its answer.
    fn continue_call_route<'e>(
        &mut self,
        mut flight: CallInFlight<'e>,
        callee: SemanticNodeId,
        mut progress: ResolveCallProgress<'e>,
    ) -> CallStep<'e> {
        loop {
            let (route, lowered) = match progress {
                ResolveCallProgress::Argument(route, lowered) => (route, lowered),
                ResolveCallProgress::Done(step) => {
                    let answer = self.fold_resolve_call_step(step, flight.site);
                    flight.drive.answers.push((callee, answer));
                    return self.drive_call(flight);
                }
            };
            let evaluated = flight
                .drive
                .argument_values
                .get(route.args.len())
                .cloned()
                .flatten();
            match evaluated {
                Some(value) => {
                    progress = self.deliver_resolve_call_argument(route, lowered, value);
                }
                None => {
                    return CallStep::Argument(Box::new(CallArgumentWait {
                        flight,
                        callee,
                        route,
                        lowered,
                    }))
                }
            }
        }
    }

    /// The value of a call's callee operand: the one the stack delivered,
    /// or evaluated in place for a call evaluated outside the stack.
    pub(super) fn call_operand_value(
        &mut self,
        operand: &SliceExpr,
        site: SliceCallSite,
    ) -> Positional<SemanticNodeId> {
        let delivered = self
            .call_drive
            .as_ref()
            .filter(|drive| drive.span == site.span())
            .and_then(|drive| drive.operand.clone());
        match delivered {
            Some(value) => value,
            None => self.eval_expr(operand),
        }
    }

    /// The answer of the executor route for `callee` the stack already
    /// took, or, for a call on the stack with lowered arguments, a stop
    /// there: `Some` either way, which every route site returns at once.
    /// `None` when the route runs in place.
    pub(super) fn driven_call_route(
        &mut self,
        callee: SemanticNodeId,
        site: SliceCallSite,
        arguments: &SliceCallArguments,
    ) -> Option<Option<Positional<CallValue>>> {
        let drive = self
            .call_drive
            .as_mut()
            .filter(|drive| drive.span == site.span())?;
        if let Some((_, answer)) = drive
            .answers
            .iter()
            .find(|(answered, _)| *answered == callee)
        {
            return Some(answer.clone());
        }
        if arguments.iter().next().is_none() {
            return None;
        }
        drive.demand = Some(callee);
        // A placeholder the value computation returns at once; the stack
        // discards it and takes the route.
        Some(Some(Positional::Hold))
    }

    /// The evidence baseline of a call about to evaluate.
    pub(super) fn call_evidence_mark(&self) -> CallEvidenceMark {
        CallEvidenceMark {
            undecided_relations: self.dispatch.dispatch_txn.borrow().call.undecided_relations,
            degradation: self.degradation,
        }
    }

    /// Record a call's evidence against `mark`: a call that evaluates to a
    /// value or a coinductive hold, without minting a fresh degradation,
    /// deposits its span (plus whether every relation outcome the
    /// resolution consumed was decided). A call whose evaluation minted the
    /// frame's FIRST degradation did not decide its occurrence (an
    /// already-degraded frame never seals, so evidence accuracy past the
    /// first degradation cannot affect admission).
    pub(super) fn finish_call_evidence(
        &mut self,
        site: SliceCallSite,
        mark: CallEvidenceMark,
        value: Positional<CallValue>,
    ) -> Positional<CallValue> {
        let newly_degraded = mark.degradation.is_none() && self.degradation.is_some();
        if !matches!(value, Positional::Unmodeled) && !newly_degraded {
            let relations_decided = self.dispatch.dispatch_txn.borrow().call.undecided_relations
                == mark.undecided_relations;
            self.call_evidence.push(FlowCallEvidence {
                span: site.span(),
                relations_decided,
            });
        }
        value
    }

    /// Begin a call's executor route: the authored call re-read from the
    /// retained snapshot, its arguments typed from the first.
    pub(super) fn start_resolve_call<'e>(
        &mut self,
        callee: SemanticNodeId,
        site: SliceCallSite,
        arguments: &'e SliceCallArguments,
    ) -> ResolveCallProgress<'e> {
        let Some(serve) = self.dispatch.ctx.ensure_indexed_ready_serve(self.canonical) else {
            return ResolveCallProgress::Done(None);
        };
        let memo = serve.indexed.shallow_state.decl_bodies();
        // The arguments this frame lowered and evaluates itself need no
        // indexed record of their own.
        let frame_lowered: Arc<[bool]> = (0..arguments.len())
            .map(|ordinal| arguments.get(ordinal).is_some())
            .collect();
        let Some(indexed) = memo.indexed_call_expression_over_frame_at(site.span(), frame_lowered)
        else {
            return ResolveCallProgress::Done(None);
        };
        let frame_arguments = self.call_arguments.get(&site.span()).cloned();
        let count = indexed.call.args.len();
        let route = ResolveCallFrame {
            _serve: serve,
            callee,
            arguments,
            indexed,
            frame_arguments,
            args: Vec::with_capacity(count),
            function_arguments: Vec::with_capacity(count),
            awaiting: None,
        };
        self.advance_resolve_call(route, None)
    }

    /// Continue a call's executor route with the value of the lowered
    /// argument it asked for.
    pub(super) fn deliver_resolve_call_argument<'e>(
        &mut self,
        route: ResolveCallFrame<'e>,
        lowered: &SliceExpr,
        value: Positional<SemanticNodeId>,
    ) -> ResolveCallProgress<'e> {
        let typed = match value {
            Positional::Value(node) => TypedArgument {
                node: Some(node),
                fresh_call: self.is_fresh_call_value(lowered, node),
                const_view: None,
            },
            Positional::Hold => return ResolveCallProgress::Done(Some(Positional::Hold)),
            Positional::Unmodeled => TypedArgument {
                node: None,
                fresh_call: false,
                const_view: None,
            },
        };
        self.advance_resolve_call(route, Some(typed))
    }

    /// Type a call's arguments from the next one, `delivered` being the
    /// awaited argument's; suspend at a lowered argument, and ask the
    /// executor once every argument is typed.
    fn advance_resolve_call<'e>(
        &mut self,
        mut route: ResolveCallFrame<'e>,
        mut delivered: Option<TypedArgument>,
    ) -> ResolveCallProgress<'e> {
        let indexed = Arc::clone(&route.indexed);
        let arguments: &'e SliceCallArguments = route.arguments;
        while let Some(argument) = indexed.call.args.get(route.args.len()) {
            let ordinal = route.args.len();
            let (binding, typed) = match delivered.take() {
                Some(typed) => (
                    route
                        .awaiting
                        .take()
                        .expect("the awaited argument's binding"),
                    typed,
                ),
                None => {
                    let Some(root) = indexed.argument_roots.get(ordinal) else {
                        return ResolveCallProgress::Done(None);
                    };
                    let binding = self.indexed_argument_binding(*root);
                    let frame_arguments = route.frame_arguments.clone();
                    // An argument that is itself a call, or a member read,
                    // evaluates through this frame's carriers, against the
                    // frame's bindings: a hold on its callee holds this call
                    // too. Any other argument that is no bare binding read
                    // is a value THIS frame computes (`c ? a : b`,
                    // `twin(c)`, `[twin(c)]`): its frame lowering reads the
                    // frame's own bindings, which the indexed program
                    // resolves in owner scope and cannot.
                    // A literal argument this frame computes is also
                    // evaluated in its const context: the value a `const`
                    // type parameter it is passed to infers from.
                    route.function_arguments.push(
                        arguments
                            .get(ordinal)
                            .or_else(|| {
                                frame_arguments
                                    .as_deref()
                                    .and_then(|frame| frame.get(ordinal))
                                    .map(|frame| &frame.value)
                            })
                            .filter(|expr| matches!(expr, SliceExpr::NestedFunctionValue { .. }))
                            .cloned(),
                    );
                    let mut const_view = None;
                    // Whether the argument is a call whose result is wholly
                    // the fresh literal its inference kept: a fresh literal
                    // source, as a bare literal is.
                    let mut fresh_call = false;
                    // A function value whose parameters are all annotated is
                    // typed with the call's first pass, its body return read
                    // under the parameter's contextual return type.
                    let function_value = match route
                        .function_arguments
                        .last()
                        .and_then(Option::as_ref)
                    {
                        Some(expr) if !argument.context_sensitive => {
                            self.eval_function_argument_under_parameter(expr, route.callee, ordinal)
                        }
                        _ => None,
                    };
                    let node = match (function_value, arguments.get(ordinal)) {
                        (Some(node), _) => Some(node),
                        (None, Some(lowered)) => {
                            route.awaiting = Some(binding);
                            return ResolveCallProgress::Argument(route, lowered);
                        }
                        (None, None) => {
                            let frame_value = match (&binding, frame_arguments.as_deref()) {
                                (
                                    FlowIndexedArgumentBinding::NonBindingExpression,
                                    Some(frame_arguments),
                                ) => frame_arguments.get(ordinal).and_then(|frame_argument| {
                                    let value =
                                        self.eval_frame_call_argument(&frame_argument.value)?;
                                    const_view = frame_argument
                                        .const_context
                                        .as_ref()
                                        .and_then(|expr| self.eval_frame_call_argument(expr));
                                    Some(value)
                                }),
                                _ => None,
                            };
                            frame_value.or_else(|| {
                                let value = self
                                    .eval_indexed_call_argument(&argument.expression, &binding)?;
                                fresh_call = value.fresh;
                                Some(value.node)
                            })
                        }
                    };
                    (
                        binding,
                        TypedArgument {
                            node,
                            fresh_call,
                            const_view,
                        },
                    )
                }
            };
            let Some(ty) = typed.node else {
                // An argument this substrate cannot type leaves
                // applicability without its evidence: the executor refuses
                // as surely, and degrading here is the same typed marker
                // with one less hop.
                return ResolveCallProgress::Done(None);
            };
            // A read of a WIDENING-literal `const` is a FRESH literal source
            // exactly as a bare literal argument is (the checker widens
            // `wrap(a)` for `const a = "x"` identically to `wrap("x")`); the
            // indexed lowering classifies every reference as pinned because
            // only this frame knows the binding's widening membership.
            let reads_widening_local = match &binding {
                FlowIndexedArgumentBinding::ValueRead(binding) => self.widening_of(binding),
                // A value declared outside the frame (`const c = 1` read as
                // `c`) widens by its declared type.
                FlowIndexedArgumentBinding::Free => match &argument.expression {
                    verter_type_expr::IndexedValueExpression::Value(
                        verter_type_expr::TypeExpr::TypeOf(value),
                    ) => {
                        value.type_args.is_empty()
                            && !self.top_level_literal_nodes(ty).is_empty()
                            && self.dispatch.value_read_widens(
                                self.canonical,
                                self.owner,
                                &value.path,
                            )
                    }
                    _ => false,
                },
                _ => false,
            };
            route.args.push(crate::semantic_query::CallArgKey::Eager {
                ty,
                spread: argument.spread,
                context_sensitive: argument.context_sensitive,
                const_view: typed.const_view,
                literal_mode: indexed_argument_literal_mode(
                    argument.literal_mode,
                    reads_widening_local || typed.fresh_call,
                ),
            });
        }
        ResolveCallProgress::Done(self.finish_resolve_call(route))
    }

    /// Ask the executor for a call whose arguments are typed: its explicit
    /// type arguments and receiver ride the key, and the checker's second
    /// inference pass retypes a context-sensitive argument the executor
    /// names.
    fn finish_resolve_call(
        &mut self,
        route: ResolveCallFrame<'_>,
    ) -> Option<Positional<ResolveCallStep>> {
        let ResolveCallFrame {
            _serve,
            callee,
            indexed,
            mut args,
            mut function_arguments,
            ..
        } = route;
        let call = &indexed.call;
        let mut explicit_type_args = Vec::with_capacity(call.explicit_type_args.len());
        for argument in call.explicit_type_args.iter() {
            let node = self.dispatch.lower_type_expr_in_owner_scope_with_mode(
                self.canonical,
                self.owner,
                argument,
                crate::semantic_query::ProjectionMode::Navigate,
            )?;
            explicit_type_args.push(node);
        }
        // A member call's receiver rides the key: `.call` / `.apply`
        // rebase and `this`-typed methods read it — the same indexed
        // lowering the callee came from, evaluated in the same scope.
        let receiver = match call.receiver.as_deref() {
            Some(receiver) => {
                let root = indexed.receiver_root?;
                let receiver_binding = self.indexed_argument_binding(root);
                Some(
                    self.eval_indexed_call_argument(receiver, &receiver_binding)?
                        .node,
                )
            }
            None => None,
        };
        let mut key = crate::semantic_query::ResolveCallKey {
            point: crate::semantic_query::ProgramPointId {
                canonical_id: Arc::from(self.canonical),
                offset: call.point,
            },
            callee,
            kind: match call.kind {
                verter_type_expr::IndexedValueCallKind::Call => {
                    crate::semantic_query::CallKind::Call
                }
                verter_type_expr::IndexedValueCallKind::Construct => {
                    crate::semantic_query::CallKind::Construct
                }
            },
            receiver,
            args: Arc::from(args.clone().into_boxed_slice()),
            explicit_type_args: Arc::from(explicit_type_args.into_boxed_slice()),
            flow: crate::semantic_query::FlowNarrowingKey::empty(),
            context: self.dispatch.resolve_call_context_for(self.canonical),
        };
        let mut step = Positional::Value(self.dispatch.execute_resolve_call(key.clone()));
        // The checker's second inference pass: a context-sensitive argument
        // the executor names is typed under the contextual type it hands
        // back, and the call is asked again with that argument's type. Each
        // round types one argument that no round types again.
        let mut retyped = false;
        // bounded-loop: at most one round per argument — each round retypes one context-sensitive argument, which is no longer context-sensitive after it.
        for _ in 0..args.len() {
            let Some((position, contextual)) = Self::contextual_argument_request(&step) else {
                break;
            };
            let Some(Some(expr)) = function_arguments.get(position).cloned() else {
                break;
            };
            let Some(ty) = self.eval_function_argument_in_context(&expr, contextual) else {
                break;
            };
            let Some(crate::semantic_query::CallArgKey::Eager { spread, .. }) = args.get(position)
            else {
                break;
            };
            retyped = true;
            args[position] = crate::semantic_query::CallArgKey::Eager {
                ty,
                spread: *spread,
                context_sensitive: false,
                const_view: None,
                literal_mode: crate::semantic_query::ArgumentLiteralMode::Literal,
            };
            function_arguments[position] = None;
            key.args = Arc::from(args.clone().into_boxed_slice());
            step = Positional::Value(self.dispatch.execute_resolve_call(key.clone()));
        }
        // A call asked again after a retyped argument that still does not
        // decide answers no uninferred parameter's fallback either.
        if retyped
            && matches!(
                step,
                Positional::Value(ResolveCallStep::Degraded(
                    crate::semantic_query::ResolveCallFailure::Undecidable
                        | crate::semantic_query::ResolveCallFailure::Budget
                ))
            )
        {
            step = Positional::Value(ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::ContextSensitiveInference {
                    contextual: None,
                },
            ));
        }
        Some(step)
    }
}
