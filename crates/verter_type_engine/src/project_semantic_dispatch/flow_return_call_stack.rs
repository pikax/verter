//! A call's evaluation from the flow evaluator's explicit stack.
//!
//! A call nests without bound through its callee operand (`new B().m().m()`,
//! an IIFE's function value, a tagged template's tag) and through the
//! frame-lowered arguments its executor route types (`f(f(f(1)))`).
//! [`FlowEvaluator::eval_expr`] evaluates both from its stack: a call
//! suspends while its callee operand evaluates, and its value computation
//! begins with that operand's value in hand. The computation is staged
//! around the call's executor route: its first stage
//! ([`FlowEvaluator::begin_call_value`]) reads and selects the callee and
//! ends at the call's value or at the route, naming the stage that takes
//! the route's answer ([`CallSuffix`]). The route ([`ResolveCallFrame`])
//! types the arguments one at a time and suspends for each lowered one;
//! its answer is folded once and the computation continues
//! ([`FlowEvaluator::continue_call_value`]) where it stopped. Nothing the
//! first stage did runs again: a call's value is computed once.
//!
//! A function-value argument the route types under its parameter's
//! contextual signature (`run(() => run(() => 1))`), and one the
//! executor's second inference pass retypes, is a nested function value
//! the route suspends at too ([`ResolveCallProgress::Function`]): its body
//! evaluates from the stack of evaluators (`flow_return_nested`), not
//! inside the route.
//!
//! A function-value argument the executor's second pass retypes is typed
//! while the executor's contextual continuation holds the candidate's
//! collector. In a flow evaluation that runs as a frame of the continuation
//! runtime, that typing is a window ([`FlowDemandDriver::open_contextual_window`]):
//! a read it makes that the frame must produce first stops the run, and the
//! route is held as it stood before the callback ([`HeldContextualRoute`]),
//! collector included. The frame's next run asks the call again with the
//! same key and resumes the held route there instead of the executor: the
//! callback types over the delivered reads into that same collector.

use std::sync::Arc;
use verter_session_query::source::demand::ExpressionSourceDemand as _;

use super::super::call_resolve::ResolveCallStep;
use super::contextual::{CalleeParameters, FunctionArgumentMark, FunctionArgumentRequest};
use super::FlowDemandDriver;
use super::*;
use verter_session_query::flow::slice::{
    SliceCall, SliceCallArgument, SliceCallArguments, SliceCallSite, SliceExpr,
};

/// The evidence baseline a call's value is recorded against
/// ([`FlowEvaluator::finish_call_evidence`]).
pub(super) struct CallEvidenceMark {
    undecided_relations: u64,
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
}

/// A call being evaluated from [`FlowEvaluator::eval_expr`]'s stack. It
/// travels boxed, so no frame moves it by value.
pub(super) struct CallInFlight<'e> {
    call: &'e SliceCall,
    site: SliceCallSite,
    arguments: &'e SliceCallArguments,
    evidence: CallEvidenceMark,
    /// The callee operand's value, once the stack delivered it.
    operand: Option<Positional<SemanticNodeId>>,
    /// Where the value computation stands.
    stage: CallStage<'e>,
}

/// Where a call's value computation stands between its stages.
enum CallStage<'e> {
    /// Not begun.
    Begin,
    /// Its executor route for `callee` runs; `then` takes the route's answer.
    Route {
        callee: SemanticNodeId,
        then: CallSuffix<'e>,
    },
    /// Taken by the stage that runs now.
    Taken,
}

/// What one stage of a call's value computation concluded.
pub(super) enum CallNext<'e> {
    /// The call's value.
    Value(Positional<CallValue>),
    /// The executor route for `callee`, whose answer `then` takes.
    Route {
        callee: SemanticNodeId,
        then: CallSuffix<'e>,
    },
}

impl<'e> CallNext<'e> {
    /// Continue after the executor route for `callee`, with `then`.
    pub(super) fn route(callee: SemanticNodeId, then: CallSuffix<'e>) -> Self {
        Self::Route { callee, then }
    }

    /// Continue after the executor route for `callee`, else with the
    /// callee type's one call signature.
    pub(super) fn of_callee(callee: SemanticNodeId) -> Self {
        Self::route(callee, CallSuffix::CalleeNode { callee })
    }
}

/// What a call's value computation does with its executor route's answer.
/// An answer is the call's value, but for an optional chain; each variant
/// names what the computation does without one.
pub(super) enum CallSuffix<'e> {
    /// The signature node's own return (an immediately called function
    /// value).
    Signature { node: SemanticNodeId },
    /// The callee type's one call signature.
    CalleeNode { callee: SemanticNodeId },
    /// The typed marker.
    Degrade,
    /// The direct callee's rail, its executor already asked.
    DirectRail(Box<DirectRail<'e>>),
    /// The direct callee's flow or declared return.
    DirectReturn(Box<DirectReturn<'e>>),
    /// The marker when overloaded, else the binding's signature read.
    Binding {
        node: SemanticNodeId,
        overloaded: bool,
    },
    /// The callee's group decision, its executor already asked.
    SymbolicGroup { callee: SemanticNodeId },
    /// The marker when overloaded, else the callee type's signature.
    Group {
        callee: SemanticNodeId,
        overloaded: bool,
    },
    /// The links after the call, read off the answer (or off the callee
    /// type's signature).
    OptionalChain(Box<OptionalChainTail<'e>>),
}

/// A direct call's rail after its lone declared signature's route.
pub(super) struct DirectRail<'e> {
    pub(super) target: &'e verter_session_query::function_program::FunctionProgramKey,
    pub(super) source: Option<verter_type_expr::facts::FunctionReturnSource>,
}

/// A direct call's return read after its generic callee's route.
pub(super) struct DirectReturn<'e> {
    pub(super) target: &'e verter_session_query::function_program::FunctionProgramKey,
    pub(super) source: Option<verter_type_expr::facts::FunctionReturnSource>,
    pub(super) clause: CalleeClause,
}

/// An optional chain after its call: the callee, whether a strip removed
/// nullish arms, the `undefined` a short-circuit reads as, and the links
/// read off the call's value.
pub(super) struct OptionalChainTail<'e> {
    pub(super) callee: SemanticNodeId,
    pub(super) stripped: bool,
    pub(super) undefined: SemanticNodeId,
    pub(super) after: &'e [(Arc<str>, bool)],
    pub(super) after_spans: &'e [verter_session_query::flow::frame_span::FrameSpan],
}

/// A call's callee operand value: the stack delivers it before the call's
/// value computation begins.
pub(super) fn delivered_operand(
    operand: Option<Positional<SemanticNodeId>>,
) -> Positional<SemanticNodeId> {
    operand.expect("a call's operand is delivered before its value computes")
}

/// A call waiting on a lowered argument its executor route asked for.
pub(super) struct CallArgumentWait<'e> {
    flight: Box<CallInFlight<'e>>,
    route: ResolveCallFrame<'e>,
    pub(super) lowered: &'e SliceExpr,
}

/// A call waiting on the signature of a function-value argument its
/// executor route types.
pub(super) struct CallFunctionWait<'e> {
    flight: Box<CallInFlight<'e>>,
    route: RouteAt<'e>,
    mark: FunctionArgumentMark,
    demand: Option<NestedDemand>,
    /// Whether the argument types in a contextual window.
    window: bool,
}

impl CallFunctionWait<'_> {
    /// The function-value argument to evaluate, taken once.
    pub(super) fn take_demand(&mut self) -> NestedDemand {
        self.demand
            .take()
            .expect("a function argument is evaluated once")
    }
}

/// What a call begun from the stack needs next.
pub(super) enum CallStep<'e> {
    /// Its callee operand's value.
    Operand(Box<CallInFlight<'e>>, &'e SliceExpr),
    /// A lowered argument's value, for its executor route.
    Argument(Box<CallArgumentWait<'e>>),
    /// A function-value argument's signature, for its executor route.
    Function(Box<CallFunctionWait<'e>>),
    /// Nothing: the call's value.
    Done(Positional<SemanticNodeId>),
}

// A call's step, stage and suffix sit on the run's stack in every frame a
// call nests through; the flight they carry stays boxed.
static_assertions::const_assert!(std::mem::size_of::<CallStep<'static>>() <= 24);
static_assertions::const_assert!(std::mem::size_of::<CallNext<'static>>() <= 48);
static_assertions::const_assert!(std::mem::size_of::<CallSuffix<'static>>() <= 24);

/// A call's executor route ([`FlowEvaluator::resolve_call_step`]) between
/// its arguments: the arguments typed so far.
pub(super) struct ResolveCallFrame<'e> {
    _serve: verter_session_query::inputs::indexed::IndexedInputServe,
    /// The call's whole span.
    span: verter_span::Span,
    callee: SemanticNodeId,
    /// Lazily prepared once; only complete discovery results are retained.
    contextual_parameters: Option<Option<CalleeParameters>>,
    arguments: &'e SliceCallArguments,
    indexed: Arc<verter_session_query::source::indexed_call::IndexedFlowCallExpression>,
    frame_arguments: Option<Arc<[SliceCallArgument]>>,
    args: Vec<crate::semantic_query::CallArgKey>,
    function_arguments: Vec<Option<SliceExpr>>,
    argument_observations: Vec<Option<super::super::flow_return_fact::ObservedRead<()>>>,
    /// The binding of the argument whose lowered value is awaited.
    awaiting: Option<FlowIndexedArgumentBinding>,
    /// The typed function value of the argument being typed, once its
    /// signature is evaluated.
    delivered_function: Option<Option<SemanticNodeId>>,
}

/// A call's executor route once every argument is typed: the executor
/// asked, and asked again for each context-sensitive argument its second
/// inference pass retypes.
pub(super) struct FinishRoute {
    _serve: verter_session_query::inputs::indexed::IndexedInputServe,
    /// The key the call was asked with first.
    asked: crate::semantic_query::ResolveCallKey,
    key: crate::semantic_query::ResolveCallKey,
    args: Vec<crate::semantic_query::CallArgKey>,
    function_arguments: Vec<Option<SliceExpr>>,
    argument_observations: Vec<Option<super::super::flow_return_fact::ObservedRead<()>>>,
    step_observations: super::super::flow_return_fact::ObservedRead<()>,
    /// Relation observations owned by this executor attempt, excluding arguments.
    undecided_relations: u64,
    declared_result: Option<super::super::call_resolve::DeclaredCallResult>,
    step: Positional<ResolveCallStep>,
    /// The call itself: its executor answers, and its contextual
    /// continuation between rounds.
    call: Box<super::super::helper_continuation::CallRead>,
    retyped: bool,
    /// The argument being retyped.
    position: usize,
}

/// A call's executor route held across its flow frame's suspension: the
/// route as it stood before the round whose callback stopped the run, its
/// call's contextual continuation holding the candidate's collector.
pub(in crate::project_semantic_dispatch) struct HeldContextualRoute {
    finish: Box<FinishRoute>,
}

impl HeldContextualRoute {
    /// The key the call was asked with first: the run that asks it again
    /// with this key resumes the route.
    pub(in crate::project_semantic_dispatch) fn asked(
        &self,
    ) -> &crate::semantic_query::ResolveCallKey {
        &self.finish.asked
    }

    /// Release a route no run resumes: what it observed is dropped and its
    /// held call returns its collector to the transaction abandoned.
    pub(in crate::project_semantic_dispatch) fn release<
        C: crate::resolver_core::ResolverCapabilities,
    >(
        self,
        dispatch: &super::super::ProjectSemanticDispatch<'_, C>,
    ) {
        let FinishRoute {
            argument_observations,
            step_observations,
            call,
            ..
        } = *self.finish;
        for observation in argument_observations.into_iter().flatten() {
            let _ = observation.discard();
        }
        let _ = step_observations.discard();
        dispatch.release_call_read(*call);
    }

    /// The held collector's session and how many deposits it holds.
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::project_semantic_dispatch) fn held_collector_for_tests(
        &mut self,
    ) -> Option<(u64, usize)> {
        self.finish.call.contextual_mut().held_collector_for_tests()
    }
}

/// One retyping round's outcome: the route's progress, or a
/// context-sensitive literal to type in place before the next round.
enum RetypeRound<'e> {
    Progress(Box<ResolveCallProgress<'e>>),
    InPlace(Box<FinishRoute>),
}

/// Where a call's executor route suspends at a function-value argument.
pub(super) enum RouteAt<'e> {
    /// Typing its arguments.
    Typing(Box<ResolveCallFrame<'e>>),
    /// Retyping a context-sensitive argument.
    Finishing(Box<FinishRoute>),
}

/// A call's executor route suspended at a function-value argument.
pub(super) struct RouteFunction<'e> {
    pub(super) at: RouteAt<'e>,
    pub(super) request: FunctionArgumentRequest,
}

/// What a call's executor route needs next.
pub(super) enum ResolveCallProgress<'e> {
    /// The value of a lowered argument.
    Argument(ResolveCallFrame<'e>, &'e SliceExpr),
    /// The signature of a function-value argument.
    Function(Box<RouteFunction<'e>>),
    /// Nothing: the route's step.
    Done(Option<Positional<ResolveCallStep>>),
}

/// One argument's type, and how it was typed.
struct TypedArgument {
    node: Option<SemanticNodeId>,
    fresh_call: bool,
    const_view: Option<SemanticNodeId>,
    /// The regular literals the argument nests, when its value is the
    /// indexed program's own in owner scope.
    literal_provenance: Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>>,
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
        | SliceCall::OnElement {
            object: operand, ..
        }
        | SliceCall::Construct(operand)
        | SliceCall::TaggedTemplate(operand)
        | SliceCall::OptionalChain { root: operand, .. } => Some(operand),
        _ => None,
    }
}

impl<'s, 'd, 'b, D: FlowDemandDriver> FlowEvaluator<'s, 'd, 'b, D> {
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
        let flight = Box::new(CallInFlight {
            call,
            site,
            arguments,
            evidence: self.call_evidence_mark(),
            operand: None,
            stage: CallStage::Begin,
        });
        match call_operand(call) {
            Some(operand) => CallStep::Operand(flight, operand),
            None => self.drive_call(flight),
        }
    }

    /// Continue a call with its callee operand's value.
    pub(super) fn resume_call_operand<'e>(
        &mut self,
        mut flight: Box<CallInFlight<'e>>,
        value: Positional<SemanticNodeId>,
    ) -> CallStep<'e> {
        flight.operand = Some(value);
        self.drive_call(flight)
    }

    /// Continue a call with the value of the lowered argument its executor
    /// route asked for.
    pub(super) fn resume_call_argument<'e>(
        &mut self,
        wait: Box<CallArgumentWait<'e>>,
        value: Positional<SemanticNodeId>,
    ) -> CallStep<'e> {
        let CallArgumentWait {
            flight,
            route,
            lowered,
        } = *wait;
        let progress = self.deliver_resolve_call_argument(route, lowered, value);
        self.continue_call_route(flight, progress)
    }

    /// Continue a call with the signature of the function-value argument
    /// its executor route asked for.
    pub(super) fn resume_call_function<'e>(
        &mut self,
        wait: Box<CallFunctionWait<'e>>,
        value: Positional<SemanticNodeId>,
    ) -> CallStep<'e> {
        let CallFunctionWait {
            flight,
            route,
            mark,
            window,
            ..
        } = *wait;
        if window {
            self.dispatch.close_contextual_window();
        }
        let progress = self.deliver_resolve_call_function(route, mark, value, window);
        self.continue_call_route(flight, progress)
    }

    /// Open the contextual window a function-value argument types in, when
    /// its route retypes it under the executor's held collector.
    pub(super) fn open_function_argument_window(&self, at: &RouteAt<'_>) -> bool {
        matches!(at, RouteAt::Finishing(_)) && self.dispatch.open_contextual_window()
    }

    /// Continue a call's executor route suspended at a function-value
    /// argument with that argument's evaluated signature. A callback typed
    /// in a contextual `window` whose run now waits on a need — one its
    /// typing read, or one relating it to the held collector's target
    /// unwraps — holds the route as it stood before it, collector included,
    /// for the run that resumes it; this run's call goes undecided.
    pub(super) fn deliver_resolve_call_function<'e>(
        &mut self,
        at: RouteAt<'e>,
        mark: FunctionArgumentMark,
        value: Positional<SemanticNodeId>,
        window: bool,
    ) -> ResolveCallProgress<'e> {
        let typed = self.function_argument_value(mark, value);
        match at {
            RouteAt::Typing(mut route) => {
                route.delivered_function = Some(typed);
                self.advance_resolve_call(*route, None)
            }
            RouteAt::Finishing(mut finish) => {
                // A finishing route types a callback its call named, so the
                // call lends its continuation.
                let holds = window
                    && (self.dispatch.contextual_window_waits()
                        || typed.is_some_and(|typed| {
                            self.dispatch
                                .contextual_delivery_waits(finish.call.contextual_mut(), typed)
                        }));
                if holds {
                    self.dispatch
                        .hold_contextual_route(HeldContextualRoute { finish });
                    ResolveCallProgress::Done(None)
                } else {
                    self.retype_resolve_call(finish, Some(typed))
                }
            }
        }
    }

    /// Begin a call's value computation with what the stack delivered, and
    /// take it on to its value or its executor route.
    fn drive_call<'e>(&mut self, mut flight: Box<CallInFlight<'e>>) -> CallStep<'e> {
        self.dispatch.graph().record_call_value_run();
        let (call, site, arguments) = (flight.call, flight.site, flight.arguments);
        let operand = flight.operand.take();
        let driver = self.dispatch;
        let read = driver.observe_read(|| self.begin_call_value(call, site, arguments, operand));
        // The call is a required value dependency, including a no-surface
        // result represented by this position's gap. Selected siblings own
        // separate value observations and need not adopt this dependency.
        let next = driver
            .adopt_read(read)
            .unwrap_or(CallNext::Value(Positional::Unmodeled));
        self.advance_call(flight, next)
    }

    /// Take a call's value computation on from `next`: to its value, or
    /// through its executor route to the stage after it.
    fn advance_call<'e>(
        &mut self,
        mut flight: Box<CallInFlight<'e>>,
        mut next: CallNext<'e>,
    ) -> CallStep<'e> {
        loop {
            let (callee, then) = match next {
                CallNext::Value(value) => {
                    let CallInFlight { site, evidence, .. } = *flight;
                    return self.finish_call(site, evidence, value);
                }
                CallNext::Route { callee, then } => (callee, then),
            };
            flight.stage = CallStage::Route { callee, then };
            let progress = self.start_resolve_call(callee, flight.site, flight.arguments);
            match self.call_route_progress(flight, progress) {
                Ok(step) => return step,
                Err((back, following)) => {
                    flight = back;
                    next = following;
                }
            }
        }
    }

    /// Continue a call whose executor route made `progress`.
    fn continue_call_route<'e>(
        &mut self,
        flight: Box<CallInFlight<'e>>,
        progress: ResolveCallProgress<'e>,
    ) -> CallStep<'e> {
        match self.call_route_progress(flight, progress) {
            Ok(step) => step,
            Err((flight, next)) => self.advance_call(flight, next),
        }
    }

    /// A call's executor route's `progress`: a suspension at the argument
    /// it asks for, or (`Err`) the call's next stage, taken with the route's
    /// answer folded once.
    fn call_route_progress<'e>(
        &mut self,
        mut flight: Box<CallInFlight<'e>>,
        progress: ResolveCallProgress<'e>,
    ) -> Result<CallStep<'e>, (Box<CallInFlight<'e>>, CallNext<'e>)> {
        match progress {
            ResolveCallProgress::Argument(route, lowered) => {
                Ok(CallStep::Argument(Box::new(CallArgumentWait {
                    flight,
                    route,
                    lowered,
                })))
            }
            ResolveCallProgress::Function(function) => {
                let RouteFunction { at, request } = *function;
                let window = self.open_function_argument_window(&at);
                Ok(CallStep::Function(Box::new(CallFunctionWait {
                    flight,
                    route: at,
                    mark: request.mark,
                    demand: Some(request.demand),
                    window,
                })))
            }
            ResolveCallProgress::Done(step) => {
                let CallStage::Route { callee, then } =
                    std::mem::replace(&mut flight.stage, CallStage::Taken)
                else {
                    unreachable!("a route answers the call that started it");
                };
                let site = flight.site;
                let answer = self.fold_resolve_call_step(step, callee, site);
                // The next stage runs under its own observation even when
                // it only hands the answer back: an abort the route observed
                // leaves the call unmodelled.
                let driver = self.dispatch;
                let read = driver.observe_read(|| self.continue_call_value(site, then, answer));
                let next = driver
                    .adopt_read(read)
                    .unwrap_or(CallNext::Value(Positional::Unmodeled));
                Err((flight, next))
            }
        }
    }

    /// Record a call's evidence and hand its value to the stack.
    fn finish_call<'e>(
        &mut self,
        site: SliceCallSite,
        evidence: CallEvidenceMark,
        value: Positional<CallValue>,
    ) -> CallStep<'e> {
        let value = self.finish_call_evidence(site, evidence, value);
        CallStep::Done(match value {
            Positional::Value(value) => Positional::Value(value.into_node()),
            Positional::Hold => Positional::Hold,
            Positional::Unmodeled => Positional::Unmodeled,
        })
    }

    /// The evidence baseline of a call about to evaluate.
    pub(super) fn call_evidence_mark(&self) -> CallEvidenceMark {
        CallEvidenceMark {
            undecided_relations: self.dispatch.call_undecided_relations(),
            degradation: self.st.degradation,
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
        let newly_degraded = mark.degradation.is_none() && self.st.degradation.is_some();
        if !matches!(value, Positional::Unmodeled) && !newly_degraded {
            #[cfg(any(test, feature = "test-support"))]
            if self.st.self_slot.is_none()
                && self
                    .dispatch
                    .observers
                    .flow
                    .undecided_nested_call_relations
                    .load(std::sync::atomic::Ordering::Relaxed)
            {
                self.dispatch.restore_call_relation_observations(1);
            }
            let relations_decided =
                self.dispatch.call_undecided_relations() == mark.undecided_relations;
            #[cfg(any(test, feature = "test-support"))]
            if self
                .dispatch
                .observers
                .flow
                .observe_call_evidence
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                if let Ok(mut observed) = self.dispatch.observers.flow.call_evidence.lock() {
                    observed.push(relations_decided);
                }
            }
            self.st.call_evidence.push(FlowCallEvidence {
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
        let Some((serve, source_demand)) = self
            .dispatch
            .source
            .indexed_expression_source(self.st.canonical)
        else {
            return ResolveCallProgress::Done(None);
        };
        // The arguments this frame lowered and evaluates itself need no
        // indexed record of their own.
        let frame_lowered: Arc<[bool]> = (0..arguments.len())
            .map(|ordinal| arguments.get(ordinal).is_some())
            .collect();
        let Some(indexed) =
            source_demand.indexed_call_expression_over_frame_at(site.span(), frame_lowered)
        else {
            return ResolveCallProgress::Done(None);
        };
        let frame_arguments = self.st.call_arguments.get(&site.span()).cloned();
        let count = indexed.call.args.len();
        let route = ResolveCallFrame {
            _serve: serve,
            span: site.span(),
            callee,
            contextual_parameters: None,
            arguments,
            indexed,
            frame_arguments,
            args: Vec::with_capacity(count),
            function_arguments: Vec::with_capacity(count),
            argument_observations: (0..count).map(|_| None).collect(),
            awaiting: None,
            delivered_function: None,
        };
        self.advance_resolve_call(route, None)
    }

    /// Continue a call's executor route with the value of the lowered
    /// argument it asked for.
    pub(super) fn deliver_resolve_call_argument<'e>(
        &mut self,
        mut route: ResolveCallFrame<'e>,
        lowered: &SliceExpr,
        value: Positional<SemanticNodeId>,
    ) -> ResolveCallProgress<'e> {
        let typed = match value {
            Positional::Value(node) => TypedArgument {
                node: Some(node),
                fresh_call: self.is_fresh_call_value(lowered, node),
                const_view: None,
                literal_provenance: None,
            },
            Positional::Hold => {
                self.adopt_argument_observations(&mut route.argument_observations);
                return ResolveCallProgress::Done(Some(Positional::Hold));
            }
            Positional::Unmodeled => TypedArgument {
                node: None,
                fresh_call: false,
                const_view: None,
                literal_provenance: None,
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
            let (binding, mut typed) = match delivered.take() {
                Some(typed) => (
                    route
                        .awaiting
                        .take()
                        .expect("the awaited argument's binding"),
                    typed,
                ),
                None => {
                    let Some(root) = indexed.argument_roots.get(ordinal) else {
                        self.adopt_argument_observations(&mut route.argument_observations);
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
                    if route.function_arguments.len() == ordinal {
                        route.function_arguments.push(
                            arguments
                                .get(ordinal)
                                .or_else(|| {
                                    frame_arguments
                                        .as_deref()
                                        .and_then(|frame| frame.get(ordinal))
                                        .map(|frame| &frame.value)
                                })
                                .filter(|expr| {
                                    matches!(expr, SliceExpr::NestedFunctionValue { .. })
                                        || (argument.context_sensitive
                                            && matches!(
                                                expr,
                                                SliceExpr::Object { .. } | SliceExpr::Array { .. }
                                            ))
                                })
                                .cloned(),
                        );
                    }
                    let mut const_view = None;
                    // Whether the argument is a call whose result is wholly
                    // the fresh literal its inference kept: a fresh literal
                    // source, as a bare literal is.
                    let mut fresh_call = false;
                    // A function value whose parameters are all annotated is
                    // typed with the call's first pass, its body return read
                    // under the parameter's contextual return type.
                    // Its body evaluates from the stack of evaluators: the
                    // route suspends until the signature is delivered.
                    let function_value = match route.delivered_function.take() {
                        Some(delivered) => delivered,
                        None => {
                            let request =
                                match route.function_arguments.last().and_then(Option::as_ref) {
                                    Some(
                                        expr @ SliceExpr::NestedFunctionValue { gap: None, .. },
                                    ) if !argument.context_sensitive => {
                                        if route.contextual_parameters.is_none() {
                                            route.contextual_parameters =
                                                self.callee_parameter_types(route.callee).ok();
                                        }
                                        let parameter = route
                                            .contextual_parameters
                                            .as_mut()
                                            .and_then(Option::as_mut)
                                            .and_then(|parameters| {
                                                parameters.at(self.dispatch, ordinal)
                                            });
                                        self.function_argument_request_under_parameter(
                                            expr, parameter,
                                        )
                                    }
                                    _ => None,
                                };
                            if let Some(request) = request {
                                return ResolveCallProgress::Function(Box::new(RouteFunction {
                                    at: RouteAt::Typing(Box::new(route)),
                                    request,
                                }));
                            }
                            None
                        }
                    };
                    let node = match (function_value, arguments.get(ordinal)) {
                        (Some(node), _) => Some(node),
                        (None, Some(lowered)) => {
                            route.awaiting = Some(binding);
                            return ResolveCallProgress::Argument(route, lowered);
                        }
                        (None, None) => {
                            let driver = self.dispatch;
                            let read = driver.observe_read(|| {
                                let frame_value = match (&binding, frame_arguments.as_deref()) {
                                    (
                                        FlowIndexedArgumentBinding::NonBindingExpression,
                                        Some(frame_arguments),
                                    ) => frame_arguments.get(ordinal).and_then(|frame_argument| {
                                        let authored_literal = argument.literal_mode
                                            == verter_type_expr::IndexedValueLiteralMode::Widened;
                                        let body_return =
                                            indexed_literal_holds_body_return(&argument.expression);
                                        let value = self.eval_frame_call_argument(
                                            &frame_argument.value,
                                            authored_literal,
                                            body_return,
                                        )?;
                                        const_view = frame_argument
                                            .const_context
                                            .as_ref()
                                            .and_then(|expr| {
                                                self.eval_frame_call_argument(
                                                    expr,
                                                    authored_literal,
                                                    body_return,
                                                )
                                            });
                                        Some(value)
                                    }),
                                    _ => None,
                                };
                                frame_value.or_else(|| {
                                    let value = self.eval_indexed_call_argument(
                                        &argument.expression,
                                        &binding,
                                    )?;
                                    fresh_call = value.fresh;
                                    Some(value.node)
                                })
                            });
                            let Ok((node, observations)) = read.split() else {
                                self.adopt_argument_observations(&mut route.argument_observations);
                                return ResolveCallProgress::Done(None);
                            };
                            if argument.context_sensitive && node.is_some() {
                                route.argument_observations[ordinal] = Some(observations);
                            } else {
                                let _ = self.dispatch.adopt_read(observations);
                            }
                            node
                        }
                    };
                    (
                        binding,
                        TypedArgument {
                            node,
                            fresh_call,
                            const_view,
                            literal_provenance: None,
                        },
                    )
                }
            };
            // The regular literals the argument nests, whichever route typed
            // it: its reads resolve in this frame's scope. Whole binding
            // reads also carry their top-level mixed membership.
            if typed.node.is_some()
                && !matches!(binding, FlowIndexedArgumentBinding::SourceTypeQuery(_))
            {
                typed.literal_provenance =
                    self.frame_argument_literal_provenance(argument.literal_provenance.as_deref());
            }
            let Some(ty) = typed.node else {
                // An argument this substrate cannot type leaves
                // applicability without its evidence: the executor refuses
                // as surely, and degrading here is the same typed marker
                // with one less hop.
                self.adopt_argument_observations(&mut route.argument_observations);
                return ResolveCallProgress::Done(None);
            };
            // Indexed provenance records nested positions only. For a whole
            // mixed binding read, supply the equivalent root record from
            // this frame's reaching membership and actual read type.
            if let FlowIndexedArgumentBinding::ValueRead(binding) = &binding {
                if let Some(WideningMembership::Partial(fresh)) = self.membership_of(binding) {
                    let graph = self.dispatch.graph();
                    let members = match graph.node_data(ty).as_deref() {
                        Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
                        _ => vec![ty],
                    };
                    let regular = members
                        .into_iter()
                        .filter(|literal| {
                            let data = graph.node_data(*literal);
                            !fresh.iter().any(|value| graph.node_data(*value) == data)
                        })
                        .collect::<Vec<_>>();
                    typed.literal_provenance =
                        Some(Arc::new(crate::semantic_query::ArgumentLiteralProvenance {
                            positions: Arc::from([
                                crate::semantic_query::ArgumentRegularPosition {
                                    path: Arc::from([]),
                                    literals: Some(Arc::from(regular)),
                                },
                            ]),
                            fresh_reads: Arc::from([Arc::from([])]),
                        }));
                }
            }
            // A read of a WIDENING-literal `const` is a FRESH literal source
            // exactly as a bare literal argument is (the checker widens
            // `wrap(a)` for `const a = "x"` identically to `wrap("x")`); the
            // indexed lowering classifies every reference as pinned because
            // only this frame knows the binding's widening membership.
            let reads_widening_local = match &binding {
                FlowIndexedArgumentBinding::ValueRead(binding) => {
                    self.membership_of(binding).is_some()
                }
                // A value declared outside the frame (`const c = 1` read as
                // `c`) widens by its declared type.
                FlowIndexedArgumentBinding::Free => match &argument.expression {
                    verter_type_expr::IndexedValueExpression::Value(
                        verter_type_expr::TypeExpr::TypeOf(value),
                    ) => {
                        value.type_args.is_empty()
                            && !self.top_level_literal_nodes(ty).is_empty()
                            && self.dispatch.value_read_widens(
                                self.st.canonical,
                                self.st.owner,
                                &value.path,
                            )
                    }
                    _ => false,
                },
                _ => false,
            };
            // A context-sensitive object literal also rides as the first
            // inference pass reads it: its context-sensitive members as the
            // non-inferring `any`.
            let first_pass = match route
                .function_arguments
                .get(ordinal)
                .and_then(Option::as_ref)
            {
                Some(SliceExpr::Object { entries, offset }) if argument.context_sensitive => {
                    if route.contextual_parameters.is_none() {
                        route.contextual_parameters =
                            self.callee_parameter_types(route.callee).ok();
                    }
                    let contextual = route
                        .contextual_parameters
                        .as_mut()
                        .and_then(Option::as_mut)
                        .and_then(|parameters| parameters.at(self.dispatch, ordinal));
                    match self.eval_object_literal_first_pass(entries, *offset, contextual) {
                        Positional::Value(node) => Some(node),
                        Positional::Hold | Positional::Unmodeled => None,
                    }
                }
                _ => None,
            };
            route.args.push(crate::semantic_query::CallArgKey::Eager {
                ty,
                spread: argument.spread,
                context_sensitive: argument.context_sensitive,
                const_view: typed.const_view,
                first_pass,
                literal_mode: indexed_argument_literal_mode(
                    argument.literal_mode,
                    reads_widening_local || typed.fresh_call,
                ),
                literal_provenance: typed.literal_provenance,
            });
        }
        self.finish_resolve_call(route)
    }

    /// Ask the executor for a call whose arguments are typed: its explicit
    /// type arguments and receiver ride the key, and the checker's second
    /// inference pass retypes a context-sensitive argument the executor
    /// names.
    fn finish_resolve_call<'e>(&mut self, route: ResolveCallFrame<'_>) -> ResolveCallProgress<'e> {
        match self.ask_resolve_call(route) {
            Some(finish) => self.retype_resolve_call(finish, None),
            None => ResolveCallProgress::Done(None),
        }
    }

    /// Mint a typed call's key and ask the executor once.
    fn ask_resolve_call(&mut self, route: ResolveCallFrame<'_>) -> Option<Box<FinishRoute>> {
        let ResolveCallFrame {
            _serve,
            span,
            callee,
            indexed,
            args,
            function_arguments,
            mut argument_observations,
            ..
        } = route;
        let call = &indexed.call;
        self.st.call_diagnostic_sites.insert(
            span,
            crate::semantic_query::CheckerDiagnosticSite {
                operation_start: Some(call.report_point),
                operation_end: Some(call.report_end),
                arguments: call
                    .args
                    .iter()
                    .map(|argument| argument.report.clone())
                    .collect(),
            },
        );
        let mut explicit_type_args = Vec::with_capacity(call.explicit_type_args.len());
        for argument in call.explicit_type_args.iter() {
            // Explicit arguments belong to the calling body's lexical clause,
            // including a permutation of that clause's still-generic binders.
            let node = self.lower_body_type(argument);
            explicit_type_args.push(node);
        }
        // A member call's receiver rides the key: `.call` / `.apply`
        // rebase and `this`-typed methods read it — the same indexed
        // lowering the callee came from, evaluated in the same scope.
        // A receiver this frame evaluated for the call (a call's result,
        // which the indexed record reads as its callee's object) is its
        // value.
        let evaluated_receiver = self.st.call_receivers.get(&span).copied();
        let receiver = match call.receiver.as_deref() {
            _ if evaluated_receiver.is_some() => evaluated_receiver,
            Some(receiver) => {
                let Some(root) = indexed.receiver_root else {
                    self.adopt_argument_observations(&mut argument_observations);
                    return None;
                };
                let receiver_binding = self.indexed_argument_binding(root);
                let Some(value) = self.eval_indexed_call_argument(receiver, &receiver_binding)
                else {
                    self.adopt_argument_observations(&mut argument_observations);
                    return None;
                };
                Some(value.node)
            }
            None => None,
        };
        // The callee's polymorphic `this` is the receiver it is read
        // through.
        let callee = receiver.map_or(callee, |receiver| {
            self.dispatch.bind_callee_receiver(callee, receiver)
        });
        let key = crate::semantic_query::ResolveCallKey {
            point: crate::semantic_query::ProgramPointId {
                canonical_id: verter_type_expr::OwnedStr::from(self.st.canonical),
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
            context: self.dispatch.resolve_call_context_for(self.st.canonical),
        };
        // A route the frame's last run held for this call resumes here, its
        // collector as it stood: the executor is not asked again.
        if let Some(held) = self.dispatch.take_held_contextual_route(&key) {
            return Some(self.resume_held_route(held, argument_observations));
        }
        let mut call = Box::new(super::super::helper_continuation::CallRead::begin(
            key.clone(),
            super::super::call_resolve::ContextualCallState::default(),
        ));
        let relations_before = self.dispatch.call_undecided_relations();
        let driver = self.dispatch;
        let read = driver.observe_read(|| driver.begin_call(&mut call));
        let undecided_relations = self
            .dispatch
            .call_undecided_relations()
            .saturating_sub(relations_before);
        let complete = read.observations_are_complete();
        let Ok(((step, declared_result), step_observations)) = read.split() else {
            self.dispatch.release_call(call);
            self.adopt_argument_observations(&mut argument_observations);
            return None;
        };
        let step = Positional::Value(step);
        Some(Box::new(FinishRoute {
            _serve,
            asked: key.clone(),
            key,
            args,
            function_arguments,
            argument_observations,
            step_observations,
            undecided_relations,
            declared_result: declared_result.filter(|_| complete),
            step,
            call,
            retyped: false,
            position: 0,
        }))
    }

    /// A held route resumed by the run that asks its call again. This run
    /// typed the call's arguments itself, so their observations are this
    /// run's, but for an argument the held route already retyped; the
    /// undecided relations its executor attempt observed count again, as
    /// they would have had the run asked it.
    fn resume_held_route(
        &mut self,
        held: HeldContextualRoute,
        mut argument_observations: Vec<Option<super::super::flow_return_fact::ObservedRead<()>>>,
    ) -> Box<FinishRoute> {
        let mut finish = held.finish;
        for (held, kept) in finish
            .argument_observations
            .iter_mut()
            .zip(argument_observations.iter_mut())
        {
            match held.take() {
                Some(stale) => {
                    let _ = stale.discard();
                    *held = kept.take();
                }
                None => {
                    if let Some(retyped) = kept.take() {
                        let _ = retyped.discard();
                    }
                }
            }
        }
        self.dispatch
            .restore_call_relation_observations(finish.undecided_relations);
        finish
    }

    /// The checker's second inference pass: a context-sensitive argument
    /// the executor names is typed under the contextual type it hands back,
    /// and the call is asked again with that argument's type, `delivered`
    /// being the retyped argument's. Each round types one argument that no
    /// round types again, so there are at most as many rounds as arguments.
    fn retype_resolve_call<'e>(
        &mut self,
        finish: Box<FinishRoute>,
        delivered: Option<Option<SemanticNodeId>>,
    ) -> ResolveCallProgress<'e> {
        let (mut finish, mut delivered) = (finish, delivered);
        loop {
            match self.retype_resolve_call_round(finish, delivered) {
                RetypeRound::Progress(progress) => return *progress,
                // A context-sensitive literal retyped in place, under the
                // contextual type the executor's step names: its value is
                // the next round's delivery.
                RetypeRound::InPlace(mut next) => {
                    let expr = next
                        .function_arguments
                        .get(next.position)
                        .cloned()
                        .flatten();
                    let contextual =
                        Self::contextual_argument_request(&next.step).map(|(_, ty)| ty);
                    delivered = Some(match (expr, contextual) {
                        (Some(expr), Some(contextual)) => {
                            match self.eval_contextual_object_argument(
                                &expr,
                                contextual,
                                next.call.contextual_mut(),
                            ) {
                                Positional::Value(node) => Some(node),
                                Positional::Hold | Positional::Unmodeled => None,
                            }
                        }
                        _ => None,
                    });
                    finish = next;
                }
            }
        }
    }

    /// One round of [`Self::retype_resolve_call`]: the delivered retyped
    /// argument applied and the executor asked again, then the next
    /// argument to retype.
    fn retype_resolve_call_round<'e>(
        &mut self,
        mut finish: Box<FinishRoute>,
        delivered: Option<Option<SemanticNodeId>>,
    ) -> RetypeRound<'e> {
        if let Some(typed) = delivered {
            let position = finish.position;
            let retyped = typed.and_then(|ty| match finish.args.get(position) {
                Some(crate::semantic_query::CallArgKey::Eager { spread, .. }) => {
                    Some((ty, *spread))
                }
                _ => None,
            });
            let Some((ty, spread)) = retyped else {
                return RetypeRound::Progress(Box::new(ResolveCallProgress::Done(
                    self.settle_retyped_call(*finish),
                )));
            };
            finish.retyped = true;
            finish.args[position] = crate::semantic_query::CallArgKey::Eager {
                ty,
                spread,
                context_sensitive: false,
                const_view: None,
                first_pass: None,
                literal_mode: crate::semantic_query::ArgumentLiteralMode::Literal,
                literal_provenance: None,
            };
            if let Some(old) = finish.argument_observations[position].take() {
                let _ = old.discard();
            }
            self.dispatch
                .discard_call_relation_observations(finish.undecided_relations);
            let relations_before = self.dispatch.call_undecided_relations();
            // The call keeps its own key and sets its arguments on the
            // rounds that execute it again; the route's copy takes them on
            // every round. Only a declared result reads the copy's
            // arguments, and only a round that executed the call again
            // supplies one, so the two never disagree where it is read.
            let args: Arc<[crate::semantic_query::CallArgKey]> = Arc::from(finish.args.as_slice());
            finish.key.args = Arc::clone(&args);
            let driver = self.dispatch;
            let read = driver.observe_read(|| driver.resume_call(&mut finish.call, args));
            finish.undecided_relations = self
                .dispatch
                .call_undecided_relations()
                .saturating_sub(relations_before);
            let complete = read.observations_are_complete();
            let Ok(((step, proof), observations)) = read.split() else {
                return RetypeRound::Progress(Box::new(ResolveCallProgress::Done(
                    self.settle_retyped_call(*finish),
                )));
            };
            let old = std::mem::replace(&mut finish.step_observations, observations);
            let _ = old.discard();
            finish.step = Positional::Value(step);
            finish.declared_result = proof.filter(|_| complete);
        }
        match self.next_retyped_argument(&finish) {
            Some((position, Some(request))) => {
                finish.position = position;
                RetypeRound::Progress(Box::new(ResolveCallProgress::Function(Box::new(
                    RouteFunction {
                        at: RouteAt::Finishing(finish),
                        request,
                    },
                ))))
            }
            Some((position, None)) => {
                finish.position = position;
                RetypeRound::InPlace(finish)
            }
            None => RetypeRound::Progress(Box::new(ResolveCallProgress::Done(
                self.settle_retyped_call(*finish),
            ))),
        }
    }

    /// The context-sensitive argument the executor's last step names for
    /// retyping, while a round is left: its position, and the request to
    /// type it under the contextual type the step hands back — `None` for
    /// an object or array literal, which is typed in place.
    fn next_retyped_argument(
        &mut self,
        finish: &FinishRoute,
    ) -> Option<(usize, Option<FunctionArgumentRequest>)> {
        let (position, contextual) = Self::contextual_argument_request(&finish.step)?;
        let expr = finish.function_arguments.get(position).cloned().flatten()?;
        if matches!(expr, SliceExpr::Object { .. } | SliceExpr::Array { .. }) {
            return Some((position, None));
        }
        if !matches!(expr, SliceExpr::NestedFunctionValue { gap: None, .. }) {
            return None;
        }
        let signature = self.contextual_signature(contextual, &expr)?;
        let request = self.function_argument_request(&expr, Some(signature))?;
        Some((position, Some(request)))
    }

    /// The executor's step for a call once no argument is retyped again. The
    /// call is released first: an answered call holds nothing, and one whose
    /// callback goes untyped returns its collector abandoned.
    fn settle_retyped_call(&mut self, finish: FinishRoute) -> Option<Positional<ResolveCallStep>> {
        let FinishRoute {
            step,
            retyped,
            mut argument_observations,
            step_observations,
            undecided_relations,
            declared_result,
            key,
            call,
            ..
        } = finish;
        drop(call);
        self.adopt_argument_observations(&mut argument_observations);
        // A declined executor does not own the fixed declared result read by
        // the direct-call route. Its failed applicability probes are not that
        // route's evidence; authored argument observations remain adopted.
        let declined = !retyped
            && matches!(
                step,
                Positional::Value(ResolveCallStep::Degraded(
                    crate::semantic_query::ResolveCallFailure::Undecidable
                        | crate::semantic_query::ResolveCallFailure::Budget
                ))
            )
            && !self.undecided_call_return_reads_clause(key.callee);
        let validity = if declined {
            self.dispatch
                .discard_call_relation_observations(undecided_relations);
            step_observations.discard()
        } else {
            self.dispatch.adopt_read(step_observations)
        };
        if validity.is_err() {
            return None;
        }
        if let Some(proof) = declared_result {
            if key.kind == crate::semantic_query::CallKind::Call {
                if let (
                    Some((_, result)),
                    Positional::Value(ResolveCallStep::Complete(
                        crate::semantic_query::ResolvedCallResult::Selected {
                            substitution, ..
                        },
                    )),
                ) = (proof.result(&key), &step)
                {
                    let sources: Vec<_> = self
                        .st
                        .constructive_values
                        .iter()
                        .filter_map(|(node, value)| match value {
                            super::ConstructiveValue::Class {
                                instance,
                                declaration,
                            } => Some((*node, *instance, declaration.clone())),
                            _ => None,
                        })
                        .collect();
                    for (source, instance, declaration) in sources {
                        let substituted = self.dispatch.substitute_canonical(source, substitution);
                        if substituted == result {
                            let instance =
                                self.dispatch.substitute_canonical(instance, substitution);
                            self.st.heritage_self_roots.extend_from_slice(proof.roots());
                            self.st.constructive_values.insert(
                                result,
                                super::ConstructiveValue::Class {
                                    instance,
                                    declaration,
                                },
                            );
                            break;
                        }
                    }
                }
            }
            if key.kind == crate::semantic_query::CallKind::Construct
                && key.args.is_empty()
                && key.explicit_type_args.is_empty()
            {
                if let (Some((_, result)), Some(super::ConstructiveValue::Class { instance, .. })) = (
                    proof.result(&key),
                    self.st.constructive_values.get(&key.callee),
                ) {
                    if result == *instance {
                        self.st.heritage_self_roots.extend_from_slice(proof.roots());
                        self.st
                            .constructive_values
                            .insert(result, super::ConstructiveValue::Instance);
                    }
                }
            }
        }
        let mut step = step;
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

    fn adopt_argument_observations(
        &self,
        observations: &mut [Option<super::super::flow_return_fact::ObservedRead<()>>],
    ) {
        for observation in observations.iter_mut().filter_map(Option::take) {
            let _ = self.dispatch.adopt_read(observation);
        }
    }
}
