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
//! by its own activation cell ([`NestedActivationCell`]), and the frames
//! around it are a shared chain of owned snapshots, so no evaluator borrows
//! from a native frame, or the cell, of the one around it.
//!
//! A statement suspends the same way at a nested function value in its
//! declarator's initializer, and a call at a function argument its route
//! types (`flow_return_call_stack`). A nested function value evaluated
//! outside a region's statements is prepared, driven and finished in place
//! ([`FlowEvaluator::eval_nested_demand`]); a body inside it is driven
//! from the stack again.

use std::sync::Arc;
use verter_session_query::source::demand::ExpressionSourceDemand as _;

use super::contextual::ContextualSignature;
use super::FlowDemandDriver;
use super::*;

/// A nested function value demanded where an evaluator suspends: what it
/// is prepared from, owned, so the demand outlives the expression it was
/// read from.
pub(super) struct NestedDemand {
    pub(super) function: verter_session_query::function_program::FunctionProgramKey,
    pub(super) context: Arc<verter_session_query::flow::slice::NestedFlowContext>,
    pub(super) has_declared_return: bool,
    pub(super) extended_captures: Arc<[SkeletonBindingId]>,
    pub(super) declared_evolving_captures:
        Arc<[verter_session_query::function_program::FlowBindingIdentity]>,
    /// The contextual signature it is checked under, if any.
    pub(super) contextual: Option<ContextualSignature>,
    /// The binder environment the function sits in when it is not the
    /// evaluator's own (a class member's is its class's).
    pub(super) outer_env: Option<Arc<FlowBinderEnv>>,
}

impl NestedDemand {
    /// The demand of `expr` when it is a nested function value, and the
    /// flow gap its lowering recorded.
    pub(super) fn of(
        expr: &verter_session_query::flow::slice::SliceExpr,
        contextual: Option<ContextualSignature>,
    ) -> Option<(Self, Option<verter_session_query::flow::policy::FlowGap>)> {
        let verter_session_query::flow::slice::SliceExpr::NestedFunctionValue {
            function,
            context,
            has_declared_return,
            required_parameters: _,
            gap,
            declared_evolving_captures,
            extended_captures,
        } = expr
        else {
            return None;
        };
        Some((
            Self {
                function: function.clone(),
                context: Arc::clone(context),
                has_declared_return: *has_declared_return,
                extended_captures: Arc::clone(extended_captures),
                declared_evolving_captures: Arc::clone(declared_evolving_captures),
                contextual,
                outer_env: None,
            },
            *gap,
        ))
    }
}

/// What the nested function evaluations of one root evaluation share: the
/// checker's return-type resolution stack (`pushTypeResolution`) — the
/// nested function values whose returns are being evaluated, the outermost
/// first, each flagged once a return it depends on is found to read it
/// again, and each function's first position on it, so a re-entry is found
/// without scanning the stack (a scan per nested function cost the square
/// of the nesting) — the functions a recorded evaluation looked up on it,
/// how many times a return was found circular, which recordings read a
/// frame around the evaluation they record, and the signatures the
/// evaluations composed ([`NestedSignatureMemo`]).
#[derive(Default)]
pub(super) struct NestedResolution {
    stack: Vec<(
        verter_session_query::function_program::FunctionProgramKey,
        bool,
    )>,
    first: rustc_hash::FxHashMap<verter_session_query::function_program::FunctionProgramKey, usize>,
    probes: WindowedLog<verter_session_query::function_program::FunctionProgramKey>,
    circular_marks: u64,
    /// For each open recording, the outermost first: the depth of the frame
    /// of the evaluator that began it, and whether an evaluation inside it
    /// read that frame or one around it.
    frame_windows: Vec<(usize, bool)>,
    signatures: NestedSignatureMemo,
}

impl NestedResolution {
    /// Note that an evaluator whose own frame is `reader` deep read the
    /// frame `frame` deep around it: a recording begun by an evaluator
    /// outside the reader, at or inside that frame, recorded a read of
    /// state its inputs do not name.
    pub(super) fn note_frame_read(&mut self, reader: usize, frame: usize) {
        for (depth, outside) in &mut self.frame_windows {
            if frame <= *depth && *depth < reader {
                *outside = true;
            }
        }
    }

    /// The first position of `function` on the stack, logged for every
    /// recording window open: an evaluation that found it absent reads the
    /// same only while it stays absent.
    fn probe(
        &mut self,
        function: &verter_session_query::function_program::FunctionProgramKey,
    ) -> Option<usize> {
        self.probes.log(function, || function.clone(), |_| true);
        self.first.get(function).copied()
    }

    /// Flag every function from `position` up: each reads the re-entered
    /// one's return while it is being resolved.
    fn mark_circular_from(&mut self, position: usize) {
        self.circular_marks += 1;
        for (_, circular) in self.stack[position..].iter_mut() {
            *circular = true;
        }
    }

    fn push(&mut self, function: verter_session_query::function_program::FunctionProgramKey) {
        let position = self.stack.len();
        self.first.entry(function.clone()).or_insert(position);
        self.stack.push((function, false));
    }

    /// Take the innermost function off, with whether it was flagged.
    fn pop(&mut self) -> Option<bool> {
        let (function, circular) = self.stack.pop()?;
        if self.first.get(&function) == Some(&self.stack.len()) {
            self.first.remove(&function);
        }
        Some(circular)
    }
}

/// Reads made while recording windows are open, the innermost window
/// last. A read is logged once per window it is made in: a window that
/// already holds it does not log it again, so an evaluation reading one
/// name many times re-checks it once. A read of a held name that finds
/// something else is logged too, and the windows it is made in are
/// refused when they close.
pub(super) struct WindowedLog<T> {
    reads: Vec<T>,
    windows: Vec<(usize, rustc_hash::FxHashSet<T>)>,
}

impl<T> Default for WindowedLog<T> {
    fn default() -> Self {
        Self {
            reads: Vec::new(),
            windows: Vec::new(),
        }
    }
}

impl<T: Clone + Eq + std::hash::Hash> WindowedLog<T> {
    /// Open a window; its reads are logged from the returned position.
    pub(super) fn begin(&mut self) -> usize {
        let start = self.reads.len();
        self.windows.push((start, rustc_hash::FxHashSet::default()));
        start
    }

    /// Log the read `key` names, made by `read`, when a window is open and
    /// the innermost does not hold it, or holds a read `same` finds
    /// different.
    pub(super) fn log<Q>(
        &mut self,
        key: &Q,
        read: impl FnOnce() -> T,
        same: impl FnOnce(&T) -> bool,
    ) where
        T: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let Some((_, seen)) = self.windows.last_mut() else {
            return;
        };
        match seen.get(key) {
            Some(held) if same(held) => {}
            Some(_) => self.reads.push(read()),
            None => {
                let read = read();
                seen.insert(read.clone());
                self.reads.push(read);
            }
        }
    }

    /// Close the innermost window, opened at `start`: each distinct read
    /// made in it, in read order — `None` when two reads of one name in it
    /// found what `same` tells apart. The log empties once no window is
    /// left.
    pub(super) fn finish(&mut self, start: usize, same: impl Fn(&T, &T) -> bool) -> Option<Vec<T>> {
        self.windows.pop();
        let mut seen: rustc_hash::FxHashSet<&T> = rustc_hash::FxHashSet::default();
        let mut reads = Vec::new();
        let mut consistent = true;
        for read in self.reads.get(start..).unwrap_or_default() {
            match seen.get(read) {
                Some(first) => consistent &= same(first, read),
                None => {
                    seen.insert(read);
                    reads.push(read.clone());
                }
            }
        }
        drop(seen);
        if self.windows.is_empty() {
            self.reads.clear();
        }
        consistent.then_some(reads)
    }
}

/// What a nested function's own evaluator borrows: the immutable inputs
/// of its activation, owned by its [`NestedActivationCell`]. Nothing in it
/// is borrowed from another activation, so the cell moves as a unit.
pub(super) struct NestedOwned {
    content: Arc<verter_session_query::flow::slice::SliceContent>,
    params: Vec<SemanticNodeId>,
    /// The function's own binder environment, shared with the enclosing
    /// frames of the activations nested in it.
    binder_env: Arc<FlowBinderEnv>,
    /// The root evaluation's canonical source, which every evaluator of it
    /// reads under.
    canonical: Arc<str>,
}

/// A nested function's evaluator state and region run, borrowing only
/// their activation's [`NestedOwned`].
pub(super) struct NestedActivationState<'a> {
    evaluator: FlowEvaluatorState<'a>,
    run: RegionRun<'a>,
}

self_cell::self_cell!(
    /// One nested activation: its owned inputs and the evaluator state and
    /// run that borrow them, in one allocation. Each activation owns its
    /// own cell; activations reach one another only through owned handles
    /// (the enclosing frames, the shared memos), never through a borrow of
    /// another cell, and creating one never rebuilds its parent. Dropping
    /// it drops the state before the inputs it borrows.
    pub(super) struct NestedActivationCell {
        owner: NestedOwned,

        #[not_covariant]
        dependent: NestedActivationState,
    }
);

// An activation is exclusively owned: one evaluation advances it, and it
// is never duplicated.
static_assertions::assert_not_impl_any!(NestedActivationCell: Clone);

/// A nested function's body under evaluation by its own evaluator.
pub(super) struct NestedActivation<'d, D: FlowDemandDriver> {
    /// The body's own evaluator state and run, advanced under the drive's
    /// driver.
    cell: NestedActivationCell,
    finish: NestedFinish<'d>,
    driver: std::marker::PhantomData<fn(&'d D)>,
}

/// What composing a nested function's signature needs once its body has
/// been evaluated.
pub(super) struct NestedFinish<'d> {
    circular: bool,
    declared_return_none: bool,
    report_name: Option<(verter_span::Span, Arc<str>)>,
    signature_params: Vec<crate::semantic_query::FunctionParam>,
    type_param_decls: Vec<crate::semantic_query::TypeParamDecl>,
    contextual: Option<ContextualSignature>,
    /// The recording that admits the composed signature to the root
    /// evaluation's [`NestedSignatureMemo`], when its inputs are reusable.
    recording: Option<Box<NestedSignatureRecording<'d>>>,
}

/// One composed nested signature the root evaluation may reuse. It is
/// reused by an evaluation of the same function whose inputs
/// ([`NestedFlowInputs`]) and lexical context are equal, whose enclosing
/// receiver still finds every member this evaluation read off it, and on
/// whose resolution stack every function this evaluation looked up is
/// still absent.
struct NestedSignatureEntry {
    context: Arc<verter_session_query::flow::slice::NestedFlowContext>,
    receiver_reads: Vec<class_expression::ReceiverRead>,
    probes: Vec<verter_session_query::function_program::FunctionProgramKey>,
    signature: SemanticNodeId,
    effects: NestedSignatureEffects,
}

/// What a nested function's evaluation left on its enclosing evaluator and
/// the builds around it, replayed when its signature is reused: its read
/// rails ([`super::super::dispatch_txn::FlowMemberReuse`]), its cost to the
/// connected demand, how it moved the undecided relation count, the first
/// degradation its signature's composition recorded on the enclosing
/// evaluator, and its body's effects.
#[derive(Clone)]
struct NestedSignatureEffects {
    reuse: verter_session_query::facts::reuse::ReuseClass,
    replay: super::super::dispatch_txn::FlowMemberReuse,
    cost: super::super::connected_demand::InlineCost,
    undecided: super::super::dispatch_txn::UndecidedRelationDelta,
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    body: NestedBodyEffects,
}

/// What a nested function's body evaluation left on its enclosing
/// evaluator: its walk, call evidence, heritage roots, checker diagnostics,
/// and the constructive evidence of its value and signature.
#[derive(Clone)]
pub(super) struct NestedBodyEffects {
    walk: ExecutedSliceWalk,
    call_evidence: Vec<FlowCallEvidence>,
    heritage_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    checker_diagnostics: Vec<crate::semantic_query::CheckerDiagnosticOccurrence>,
    constructive_value: Option<(SemanticNodeId, ConstructiveValue)>,
    constructive_signature: bool,
}

/// The nested signatures one root evaluation composed completely, by
/// function and then by inputs. Shared by every nested evaluator of the
/// root evaluation and dropped with it: it lives no longer than the evaluation whose builds its
/// replays fold into. Only the outermost [`NESTED_RECORDING_DEPTH`] nested
/// evaluations around a read record it, so the memo keeps each read a
/// bounded number of times however deep the nest.
#[derive(Default)]
pub(super) struct NestedSignatureMemo {
    by_function: rustc_hash::FxHashMap<
        verter_session_query::function_program::FunctionProgramKey,
        rustc_hash::FxHashMap<verter_identity::identity::InputBasisId, Vec<NestedSignatureEntry>>,
    >,
}

/// How many nested evaluations' recordings may be open around one another.
const NESTED_RECORDING_DEPTH: usize = 4;

/// A nested function's evaluation being recorded for the memo, from its
/// slice content's read to its signature's composition.
pub(super) struct NestedSignatureRecording<'d> {
    function: verter_session_query::function_program::FunctionProgramKey,
    inputs: verter_identity::identity::InputBasisId,
    context: Arc<verter_session_query::flow::slice::NestedFlowContext>,
    reads: super::super::build::TransactionReadRecording<'d>,
    /// The enclosing receiver the body reads `this` through, and where
    /// its read log stood when the recording began.
    receiver: Option<(std::rc::Rc<class_expression::ClassReceiver>, usize)>,
    /// Where the resolution stack's lookup log stood, and how many returns
    /// had been found circular, when the recording began.
    probes_start: usize,
    circular_marks: u64,
    /// The window measuring how the undecided relation count moves.
    undecided: super::super::dispatch_txn::UndecidedRelationWindow<'d>,
    /// The measurement of its cost to the connected demand.
    cost: super::super::connected_demand::InlineCostRecording<'d>,
    /// The enclosing evaluator's degradation when the recording began,
    /// given back, first-recorded first, when it ends.
    degradation_before: Option<crate::semantic_query::FlowReturnDegradation>,
}

/// A nested signature reused from the memo, its effects replayed.
pub(super) struct ReusedNestedSignature {
    pub(super) signature: SemanticNodeId,
    pub(super) constructive: bool,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// How many nested function bodies this thread evaluated; test-only.
    static NESTED_BODY_EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many nested function bodies this thread evaluated so far
/// (test-only): a signature reused from the memo evaluates none.
#[cfg(any(test, feature = "test-support"))]
pub fn nested_body_evaluations_for_tests() -> usize {
    NESTED_BODY_EVALUATIONS.with(std::cell::Cell::get)
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// How many nested signatures this thread reused; test-only.
    static NESTED_SIGNATURE_REUSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many nested signatures this thread reused from the memo so far
/// (test-only).
#[cfg(any(test, feature = "test-support"))]
pub fn nested_signature_reuses_for_tests() -> usize {
    NESTED_SIGNATURE_REUSES.with(std::cell::Cell::get)
}

/// The locals of a nested function's signature evaluation its body's
/// evaluator is built from.
pub(super) struct NestedChildParts {
    pub(super) params: Vec<SemanticNodeId>,
    pub(super) signature_params: Vec<crate::semantic_query::FunctionParam>,
    pub(super) type_param_decls: Vec<crate::semantic_query::TypeParamDecl>,
    pub(super) binder_env: FlowBinderEnv,
    pub(super) captured_products: FlowProductStore,
    /// The product authority `captured_products` advances under.
    pub(super) captured_execution: super::super::flow_return_products::FlowFrameExecution,
    pub(super) declared_capture_types: rustc_hash::FxHashMap<
        verter_session_query::function_program::FlowBindingIdentity,
        SemanticNodeId,
    >,
    /// The captures that entered the body extended, which stay extended
    /// for the functions the body creates.
    pub(super) extended_capture_identities:
        rustc_hash::FxHashSet<verter_session_query::function_program::FlowBindingIdentity>,
    pub(super) execution_selection: Arc<super::super::flow_solve::FlowExecutionSelection>,
    pub(super) plan: Option<Arc<super::super::flow_solve::FlowDemandPlan>>,
    pub(super) bound: verter_session_query::flow::bundle::BoundFlowGraph,
    pub(super) skeleton: verter_session_query::flow::bundle::SkeletonRef,
}

/// A nested function's signature evaluation up to its body's evaluation.
pub(super) enum NestedSignatureStep {
    /// The signature, composed without evaluating the body.
    Done(SemanticNodeId),
    /// The body's evaluator is to be built from these.
    Child(Box<NestedChildParts>),
}

/// A nested function prepared for evaluation.
pub(super) enum NestedPrepared<'d, D: FlowDemandDriver> {
    /// Its signature, with nothing to evaluate.
    Value(SemanticNodeId),
    /// Its body, to evaluate on its own evaluator.
    Child(Box<NestedActivation<'d, D>>),
}

/// How a drive ends.
enum DriveEnd {
    /// The region it was started on completed.
    Region((Result<Vec<FlowContribution>, FlowReturnFailure>, bool)),
    /// The nested function it was started on is composed.
    Nested(SemanticNodeId),
}

impl<'s, 'd, 'b, D: FlowDemandDriver> FlowEvaluator<'s, 'd, 'b, D> {
    /// Evaluate a region, driving the nested functions its return
    /// statements reach from the stack (see the module docs).
    pub(super) fn eval_region(
        &mut self,
        region: &verter_session_query::flow::slice::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        let run = self.start_region_run(region);
        match self.drive(Some(run), Vec::new()) {
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
    pub(super) fn eval_nested_demand(&mut self, demand: NestedDemand) -> SemanticNodeId {
        let own_env = demand.outer_env.clone();
        let frame_env: &FlowBinderEnv = self.st.binder_env;
        self.eval_nested_demand_in(demand, own_env.as_deref().unwrap_or(frame_env))
    }

    /// [`Self::eval_nested_demand`] under `outer_env`, the binder
    /// environment the function sits in (a class member's is its class's).
    pub(super) fn eval_nested_demand_in(
        &mut self,
        demand: NestedDemand,
        outer_env: &FlowBinderEnv,
    ) -> SemanticNodeId {
        let prepared = self.prepare_nested(demand, outer_env);
        let node = match prepared {
            NestedPrepared::Value(node) => node,
            NestedPrepared::Child(child) => match self.drive(None, vec![*child]) {
                DriveEnd::Nested(node) => node,
                DriveEnd::Region(_) => unreachable!("a nested drive ends with its function"),
            },
        };
        node
    }

    /// Run `root` (this evaluator's region) and the nested activations on
    /// `stack` until the root region completes, or, with no root, until the
    /// bottom nested function is composed on this evaluator. Each
    /// activation advances inside its own cell, under this drive's driver.
    fn drive<'r>(
        &mut self,
        mut root: Option<RegionRun<'r>>,
        mut stack: Vec<NestedActivation<'d, D>>,
    ) -> DriveEnd {
        let dispatch = self.dispatch;
        let mut delivered: Option<RegionDelivery> = None;
        loop {
            // The evaluator on top runs; a nested function it suspends at is
            // prepared on it.
            let prepared = match stack.last_mut() {
                Some(top) => {
                    let delivery = delivered.take();
                    let step =
                        top.cell.with_dependent_mut(|_, active| {
                            match FlowEvaluator::over(dispatch, &mut active.evaluator)
                                .run_region(&mut active.run, delivery)
                            {
                                RegionProgress::Nested(demand) => {
                                    let own_env = demand.outer_env.clone();
                                    let frame_env: &FlowBinderEnv = active.evaluator.binder_env;
                                    let outer_env = own_env.as_deref().unwrap_or(frame_env);
                                    Ok(FlowEvaluator::over(dispatch, &mut active.evaluator)
                                        .prepare_nested(demand, outer_env))
                                }
                                RegionProgress::Done(outcome) => Err(outcome),
                            }
                        });
                    match step {
                        Ok(prepared) => Some(prepared),
                        Err(outcome) => {
                            let finished = stack.pop().expect("the evaluator on top");
                            let node = match stack.last_mut() {
                                Some(parent) => parent.cell.with_dependent_mut(|_, active| {
                                    FlowEvaluator::over(dispatch, &mut active.evaluator)
                                        .finish_nested(finished, outcome)
                                }),
                                None => {
                                    let node = self.finish_nested(finished, outcome);
                                    if root.is_none() {
                                        return DriveEnd::Nested(node);
                                    }
                                    node
                                }
                            };
                            delivered = Some(RegionDelivery::Nested(node));
                            None
                        }
                    }
                }
                None => {
                    let run = root
                        .as_mut()
                        .expect("a drive with no nested evaluator runs its root");
                    match self.run_region(run, delivered.take()) {
                        RegionProgress::Nested(demand) => {
                            let own_env = demand.outer_env.clone();
                            let frame_env: &FlowBinderEnv = self.st.binder_env;
                            let outer_env = own_env.as_deref().unwrap_or(frame_env);
                            Some(self.prepare_nested(demand, outer_env))
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
    /// evaluator, in its own activation cell with the data it borrows.
    pub(super) fn prepare_nested(
        &mut self,
        demand: NestedDemand,
        outer_env: &FlowBinderEnv,
    ) -> NestedPrepared<'d, D> {
        let NestedDemand {
            function,
            context,
            has_declared_return,
            extended_captures,
            declared_evolving_captures,
            contextual,
            outer_env: _,
        } = demand;
        self.st.constructive_signatures.remove(&function);
        let function = &function;
        let context = &context;
        let extended_captures: &[SkeletonBindingId] = &extended_captures;
        let declared_evolving_captures: &[verter_session_query::function_program::FlowBindingIdentity] =
            &declared_evolving_captures;
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: verter_type_expr::OwnedStr::from(self.st.canonical),
                owner: function.declaration.owner,
                symbol: function.declaration.name.clone(),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: function.part.clone(),
            overload_ordinal: function.overload_ordinal,
        };
        let key = self.dispatch.flow_return_key_for(&identity);
        let Ok(site) = self.dispatch.flow_slice_demand_site(&key) else {
            return NestedPrepared::Value(self.unresolved_nested_value());
        };
        let index = site.source_demand.function_program_index();
        let Some(matched) = index.get(function) else {
            return NestedPrepared::Value(self.unresolved_nested_value());
        };
        let anchor = matched.entry().span().start;
        let (Some(skeleton), Some(bound)) = (
            self.dispatch.flow_slice_skeleton(&site.slice_key_function),
            self.dispatch.flow_bound_graph_for(&site.slice_key_function),
        ) else {
            return NestedPrepared::Value(self.unresolved_nested_value());
        };
        let planned_and_selection = if has_declared_return {
            None
        } else {
            let Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::Planned(planned)) =
                self.dispatch.flow_slice_hash(site.slice_key.clone())
            else {
                return NestedPrepared::Value(self.unresolved_nested_value());
            };
            let Some(lowered) = self.dispatch.flow_slice_lowered(
                crate::cache_runtime::flow_slice_node::FlowSliceLoweredKey {
                    hash_key: site.slice_key.clone(),
                    slice_hash: planned.hash(),
                },
                &planned,
            ) else {
                return NestedPrepared::Value(self.unresolved_nested_value());
            };
            Some((
                planned,
                verter_session_query::flow::slice::FlowSliceSelection::from_slice_ir(&lowered),
            ))
        };
        // A body evaluated from its slice, outside a contextual window (where
        // a read may stop the run instead of completing), and not re-entered
        // while its return resolves is reused on equal inputs: they are read
        // here, before its slice content, without changing any frame — a
        // capture whose reading would change one keeps the function from the
        // memo and is read in its place below.
        let mut recording = None;
        let mut precomputed = None;
        if let Some((planned, _)) = planned_and_selection
            .as_ref()
            .filter(|_| !self.dispatch.contextual_window_open())
        {
            let captures = self.prepare_nested_capture_inputs(
                &FlowProductInputs::for_bound_graph(&bound),
                planned,
                context,
                declared_evolving_captures,
                extended_captures,
                true,
            );
            let inputs = captures.as_ref().map(|captures| {
                self.nested_flow_inputs(
                    &captures.capture_basis,
                    outer_env,
                    context,
                    contextual.as_ref(),
                )
            });
            let reentered = self
                .st
                .resolving_functions
                .borrow_mut()
                .probe(function)
                .is_some();
            if let Some(inputs) = inputs.as_ref().filter(|_| !reentered) {
                if let Some(reused) = self.reuse_nested_signature(function, inputs, context) {
                    if reused.constructive {
                        self.st
                            .constructive_signatures
                            .insert(function.clone(), reused.signature);
                    }
                    return NestedPrepared::Value(reused.signature);
                }
                recording =
                    self.begin_nested_signature_recording(function, inputs.clone(), context);
            }
            precomputed = captures.zip(inputs);
        }
        #[cfg(any(test, feature = "test-support"))]
        let content_withheld = self
            .dispatch
            .observers
            .flow
            .fail_nested_slice_content
            .load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(any(test, feature = "test-support")))]
        let content_withheld = false;
        let content = (!content_withheld)
            .then(|| {
                site.source_demand.flow_slice_content_with_context(
                    matched,
                    planned_and_selection
                        .as_ref()
                        .map(|(_, selection)| selection.clone()),
                    &bound,
                    Some(Arc::clone(context)),
                    self.policy(),
                )
            })
            .flatten();
        let planned = planned_and_selection.map(|(planned, _)| planned);
        let Some(content) = content else {
            self.end_nested_signature_recording(recording);
            return NestedPrepared::Value(self.unresolved_nested_value());
        };
        if content.budget_failure.is_some() {
            self.end_nested_signature_recording(recording);
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
        let reentered = self.st.resolving_functions.borrow_mut().probe(function);
        if let Some(position) = reentered {
            self.st
                .resolving_functions
                .borrow_mut()
                .mark_circular_from(position);
        }
        let circular = reentered.is_some() && content.declared_return.is_none();
        if !circular {
            self.st
                .resolving_functions
                .borrow_mut()
                .push(function.clone());
        }
        let step = self.nested_signature_prefix(
            precomputed,
            &content.params,
            &content.type_parameters,
            context,
            declared_evolving_captures,
            content.declared_return.as_ref(),
            content.declared_predicate.as_ref(),
            &content.bindings,
            skeleton,
            bound,
            planned.as_deref().map(|planned| &**planned),
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
                self.end_nested_signature_recording(recording);
                let (value, occurrence) = self.nested_function_postlude(
                    circular,
                    declared_return_none,
                    false,
                    signature,
                    matched.entry().report_name(),
                );
                if let Some(occurrence) = occurrence {
                    self.record_checker_diagnostic(occurrence);
                }
                return NestedPrepared::Value(value);
            }
            NestedSignatureStep::Child(parts) => *parts,
        };
        let NestedChildParts {
            params,
            signature_params,
            type_param_decls,
            binder_env,
            captured_products,
            captured_execution,
            declared_capture_types,
            extended_capture_identities,
            execution_selection,
            plan,
            bound,
            skeleton,
        } = parts;
        let enclosing_receiver = self.enclosing_receiver_of(context);
        #[cfg(any(test, feature = "test-support"))]
        NESTED_BODY_EVALUATIONS.with(|evaluations| evaluations.set(evaluations.get() + 1));
        let enclosing_frames = self.st.enclosing_frames.with(
            self.st.bindings.function().clone(),
            Arc::clone(self.st.binder_env),
            self.st.products.clone(),
            std::rc::Rc::clone(&self.st.frame_execution),
        );
        let owned = NestedOwned {
            content,
            params,
            binder_env: Arc::new(binder_env),
            canonical: Arc::from(self.st.canonical),
        };
        let capture_context = context;
        let dispatch = self.dispatch;
        let cell = NestedActivationCell::new(owned, |owned| {
            let mut evaluator = FlowEvaluatorState {
                independent_members: None,
                independent_returns: None,
                constructive_values: rustc_hash::FxHashMap::default(),
                constructive_signatures: rustc_hash::FxHashMap::default(),
                call_receivers: rustc_hash::FxHashMap::default(),
                call_arguments: Arc::clone(&owned.content.call_arguments),
                self_slot: None,
                resolving_functions: std::rc::Rc::clone(&self.st.resolving_functions),
                this_free_values: std::rc::Rc::clone(&self.st.this_free_values),
                declared_type_reads: std::rc::Rc::clone(&self.st.declared_type_reads),
                carrier_unwraps: std::rc::Rc::clone(&self.st.carrier_unwraps),
                discriminant_members: std::rc::Rc::clone(&self.st.discriminant_members),
                guard_relations: std::rc::Rc::clone(&self.st.guard_relations),
                arm_indexes: std::rc::Rc::clone(&self.st.arm_indexes),
                canonical: &owned.canonical,
                owner: self.st.owner,
                nullability: self.st.nullability,
                no_implicit_any: self.st.no_implicit_any,
                use_unknown_in_catch_variables: self.st.use_unknown_in_catch_variables,
                no_implicit_this: self.st.no_implicit_this,
                contextual_object_this: None,
                contextual_object_receiver: None,
                inherits_contextual_object_receiver: false,
                object_first_pass_context: ObjectFirstPassContext::default(),
                params: &owned.params,
                param_names: &owned.content.params,
                binder_env: &owned.binder_env,
                enclosing_frames,
                bindings: owned.content.bindings.clone(),
                skeleton: skeleton.clone(),
                flow_graph: bound.graph_ref(),
                execution_selection,
                plan,
                anchor,
                products: captured_products,
                frame_execution: std::rc::Rc::new(std::cell::RefCell::new(captured_execution)),
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
                extended_capture_identities,
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
                call_diagnostic_sites: rustc_hash::FxHashMap::default(),
                circular_return: std::cell::Cell::new(false),
                frame_declares_return: !declared_return_none,
                bare_self_return: None,
                declared_reads: false,
                dead_writes: false,
                dead_written: rustc_hash::FxHashSet::default(),
                dead_written_log: Vec::new(),
                receiver: enclosing_receiver,
            };
            let body = &owned.content.body;
            if owned.content.declared_this.is_none()
                && !capture_context.this_is_lexical()
                && matches!(
                    capture_context.this(),
                    Some(verter_session_query::flow::slice::SliceThis::Receiver)
                )
            {
                if let Some(binder) = contextual.as_ref().and_then(|signature| signature.receiver) {
                    evaluator.receiver = Some(std::rc::Rc::new(
                        class_expression::ClassReceiver::of_type(binder, Some(binder)).explicit(),
                    ));
                }
            }
            let run = {
                let mut child = FlowEvaluator::over(dispatch, &mut evaluator);
                child.bind_explicit_this(owned.content.declared_this.as_ref());
                child.seed_hoisted_var_declarations(body);
                child.start_region_run(body)
            };
            NestedActivationState { evaluator, run }
        });
        NestedPrepared::Child(Box::new(NestedActivation {
            cell,
            driver: std::marker::PhantomData,
            finish: NestedFinish {
                circular,
                declared_return_none,
                report_name: matched.entry().report_name().cloned(),
                signature_params,
                type_param_decls,
                contextual,
                recording,
            },
        }))
    }

    /// Compose a nested function's signature on this evaluator (its parent)
    /// from its body's evaluation, `outcome`, and release its activation.
    fn finish_nested(
        &mut self,
        activation: NestedActivation<'d, D>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> SemanticNodeId {
        let NestedActivation {
            mut cell,
            finish,
            driver: _,
        } = activation;
        cell.with_dependent_mut(|owned, active| {
            // The body's run is over: its frames go before the signature is
            // composed.
            drop(std::mem::take(&mut active.run.frames));
            self.finish_nested_body(owned, &mut active.evaluator, finish, outcome)
        })
    }

    /// [`Self::finish_nested`] over the activation's owned inputs and its
    /// evaluator state.
    fn finish_nested_body(
        &mut self,
        owned: &NestedOwned,
        nested_evaluator: &mut FlowEvaluatorState<'_>,
        finish: NestedFinish<'d>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> SemanticNodeId {
        let NestedFinish {
            circular,
            declared_return_none,
            report_name,
            signature_params,
            type_param_decls,
            contextual,
            recording,
        } = finish;
        let function = nested_evaluator.bindings.function().clone();
        let constructive_signature;
        // What the recording keeps beside the signature: whether the body
        // completed with nothing held, degraded or aborted, and the effects
        // its evaluation left on this evaluator.
        let mut body_complete = false;
        let mut recorded_effects: Option<NestedBodyEffects> = None;
        let mut constructive_value = None;
        let signature = {
            let graph = self.dispatch.graph();
            let can_fall_through = owned.content.can_fall_through;
            let empty_completion = owned.content.empty_completion;
            let binder_env = &owned.binder_env;
            let skeleton = nested_evaluator.skeleton.clone();
            let nested_holds;
            let nested_degradation;
            let nested_observations;
            let nested_yield_contributions;
            let nested_inferred_predicate;
            let mut constructive_return = None;
            let mut independently_typed_return = false;
            let (contributors, nested_body_falls_through) = {
                let (outcome, nested_body_falls_through) = outcome;
                FlowEvaluator::over(self.dispatch, nested_evaluator)
                    .promote_pending_statement_gap();
                nested_yield_contributions =
                    std::mem::take(&mut nested_evaluator.yield_contributions);
                nested_inferred_predicate = nested_evaluator.inferred_predicate;
                #[cfg(any(test, feature = "test-support"))]
                if self
                    .dispatch
                    .observers
                    .flow
                    .short_nested_execution_ledger
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    nested_evaluator.executed_walk.aborted = true;
                }
                nested_holds = nested_evaluator.holds.clone();
                let source_return = match owned.content.body.statements.as_ref() {
                    [verter_session_query::flow::slice::SliceStatement::Return {
                        argument: Some(argument),
                        ..
                    }] => Some(argument),
                    _ => None,
                };
                let source_complete = outcome.as_ref().is_ok_and(|values| values.len() == 1)
                    && !nested_body_falls_through
                    && nested_holds.is_empty()
                    && !nested_evaluator.executed_walk.aborted
                    && !nested_evaluator.executed_walk.local_aborted
                    && !crate::request_context::current_cold_compute_completeness().is_partial()
                    && crate::request_context::current_execution_abort().is_none()
                    && source_return.is_some();
                let independent_return = outcome
                    .as_ref()
                    .ok()
                    .filter(|values| values.len() == 1)
                    .and_then(|values| {
                        nested_evaluator
                            .constructive_values
                            .get(&values[0].node)
                            .cloned()
                            .map(|proof| (values[0].node, proof))
                    })
                    .filter(|_| source_complete);
                let finished = nested_evaluator
                .frame_execution
                .borrow_mut()
                .finish(nested_evaluator.plan.as_deref())
                .and_then(|evidence| {
                    #[cfg(any(test, feature = "test-support"))]
                    let evidence = if self.dispatch.observers.flow
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
                            verter_session_query::flow::policy::FlowGap::UnmodeledExpression,
                        ))
                    }
                });
                let finished_ok = finished.is_ok();
                if let Err(failure) = finished {
                    if matches!(failure, FlowProductFailure::BudgetExceeded(_)) {
                        self.st.frame_execution.borrow_mut().failure = Some(failure);
                        nested_evaluator.executed_walk.aborted = true;
                    }
                    FlowEvaluator::over(self.dispatch, nested_evaluator).record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                    );
                }
                if finished_ok && !nested_evaluator.executed_walk.aborted {
                    independently_typed_return = source_complete
                        && source_return.is_some_and(super::independent_return_expression);
                    if let Some((node, proof)) = independent_return {
                        constructive_return = Some(node);
                        if recording.is_some() {
                            constructive_value = Some((node, proof.clone()));
                        }
                        self.st.constructive_values.insert(node, proof);
                    }
                }
                nested_degradation = nested_evaluator.degradation;
                nested_observations = nested_evaluator.observations;
                if recording.is_some() {
                    body_complete = outcome.is_ok()
                        && finished_ok
                        && nested_holds.is_empty()
                        && nested_degradation.is_none()
                        && !nested_evaluator.executed_walk.aborted
                        && !nested_evaluator.executed_walk.local_aborted
                        && !crate::request_context::current_cold_compute_completeness()
                            .is_partial()
                        && crate::request_context::current_execution_abort().is_none();
                    recorded_effects = Some(NestedBodyEffects {
                        walk: nested_evaluator.executed_walk,
                        call_evidence: nested_evaluator.call_evidence.clone(),
                        heritage_self_roots: nested_evaluator.heritage_self_roots.clone(),
                        checker_diagnostics: nested_evaluator.checker_diagnostics.clone(),
                        constructive_value: constructive_value.take(),
                        constructive_signature: false,
                    });
                }
                self.st.holds.append(&mut nested_evaluator.holds);
                // A call the NESTED body evaluated is still an evaluated call
                // of this evaluation run: the evidence rides the enclosing
                // ledger exactly as the nested holds do — and so does the
                // nested walk (a nested abort shortens THIS run's ledger).
                self.st
                    .call_evidence
                    .append(&mut nested_evaluator.call_evidence);
                self.st.executed_walk.absorb(nested_evaluator.executed_walk);
                // A heritage hop the NESTED body's `instanceof` narrowing read
                // is still a cross-file observation of THIS run: it rides the
                // enclosing self-roots exactly as the nested holds do.
                for root in nested_evaluator.heritage_self_roots.drain(..) {
                    if !self.st.heritage_self_roots.contains(&root) {
                        self.st.heritage_self_roots.push(root);
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
                    self.st.nullability,
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
                            self.st.nullability,
                        ),
                    );
                    wrapped.return_type()
                }
                Err(_) => {
                    body_complete = false;
                    self.unmodeled_position()
                }
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
            constructive_signature = signature_params.is_empty()
                && type_param_decls.is_empty()
                && predicate.is_none()
                && nested_degradation.is_none()
                && skeleton.kind == verter_session_query::flow::skeleton::FunctionBodyKind::Plain
                && (constructive_return == Some(return_type) || independently_typed_return);
            graph.intern_node(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Call,
                params: crate::semantic_query::PayloadArray::from(
                    signature_params.into_boxed_slice(),
                ),
                return_type,
                type_parameters: crate::semantic_query::PayloadArray::from(
                    type_param_decls.into_boxed_slice(),
                ),
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
        FlowEvaluator::over(self.dispatch, nested_evaluator)
            .report_receiver_reads(owned.content.receiver_reads.as_ref());
        for diagnostic in nested_evaluator.checker_diagnostics.drain(..) {
            self.record_checker_diagnostic(diagnostic);
        }
        let found_in_body = nested_evaluator.circular_return.get();
        let (value, occurrence) = self.nested_function_postlude(
            circular,
            declared_return_none,
            found_in_body,
            signature,
            report_name.as_ref(),
        );
        if let Some(occurrence) = occurrence {
            if let Some(effects) = recorded_effects.as_mut() {
                effects.checker_diagnostics.push(occurrence.clone());
            }
            self.record_checker_diagnostic(occurrence);
        }
        if let Some(recording) = recording {
            self.admit_nested_signature(
                *recording,
                (body_complete && (value == signature || found_in_body)).then_some(value),
                recorded_effects.map(|effects| NestedBodyEffects {
                    constructive_signature,
                    ..effects
                }),
            );
        }
        #[cfg(any(test, feature = "test-support"))]
        if self
            .dispatch
            .observers
            .flow
            .partial_after_nested_signature
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            crate::request_context::mark_request_result_partial();
        }
        if constructive_signature && value == signature {
            self.st.constructive_signatures.insert(function, signature);
        }
        value
    }

    /// A nested function's signature, once evaluated: its frame leaves the
    /// resolving stack, and a signature whose return was read while it was
    /// being resolved (a circular reference) returns `any`.
    fn nested_function_postlude(
        &mut self,
        circular: bool,
        declared_return_none: bool,
        found_in_body: bool,
        signature: SemanticNodeId,
        report_name: Option<&(verter_span::Span, Arc<str>)>,
    ) -> (
        SemanticNodeId,
        Option<crate::semantic_query::CheckerDiagnosticOccurrence>,
    ) {
        let found_circular = !circular
            && self
                .st
                .resolving_functions
                .borrow_mut()
                .pop()
                .unwrap_or(false)
            || found_in_body;
        if found_circular && declared_return_none {
            return (
                self.signature_returning_any(signature),
                report_name.and_then(|name| self.circular_return_occurrence(name)),
            );
        }
        (signature, None)
    }

    /// The signature the root evaluation composed for `function` on
    /// `inputs` in `context`, when every member its evaluation read off the
    /// enclosing receiver reads the same now, every function it looked up
    /// on the resolution stack is still absent from it, and the demand can
    /// pay for it: its effects replayed on this evaluator and the builds
    /// around it, exactly as evaluating the body again would leave them.
    /// `None` once the computation is aborted or partial, where evaluating
    /// the body again is what reports it.
    pub(super) fn reuse_nested_signature(
        &mut self,
        function: &verter_session_query::function_program::FunctionProgramKey,
        inputs: &verter_identity::identity::InputBasisId,
        context: &Arc<verter_session_query::flow::slice::NestedFlowContext>,
    ) -> Option<ReusedNestedSignature> {
        #[cfg(any(test, feature = "test-support"))]
        if self
            .dispatch
            .observers
            .flow
            .skip_nested_signature_reuse
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        if crate::request_context::current_execution_abort().is_some()
            || crate::request_context::current_cold_compute_completeness().is_partial()
        {
            return None;
        }
        let receiver = self.enclosing_receiver_of(context);
        let resolution = std::rc::Rc::clone(&self.st.resolving_functions);
        let mut resolution = resolution.borrow_mut();
        let entry = resolution
            .signatures
            .by_function
            .get(function)?
            .get(inputs)?
            .iter()
            .find(|entry| {
                (Arc::ptr_eq(&entry.context, context) || entry.context == *context)
                    && entry.receiver_reads.iter().all(|read| {
                        receiver.as_deref().is_some_and(|receiver| {
                            self.receiver_member_read(receiver, &read.name) == read.read
                        })
                    })
                    && entry
                        .probes
                        .iter()
                        .all(|probe| !resolution.first.contains_key(probe))
            })?;
        let receiver_reads = entry.receiver_reads.clone();
        let probes = entry.probes.clone();
        let signature = entry.signature;
        let NestedSignatureEffects {
            reuse,
            replay,
            cost,
            undecided,
            degradation,
            body,
        } = entry.effects.clone();
        // A body evaluated again would take its cost and move the
        // undecided relation count again; where the demand lacks the room
        // for either, evaluating it again is what meets the allowance.
        if self.dispatch.call_undecided_relations() < undecided.depth
            || !self.dispatch.replay_inline_cost(&cost)
        {
            return None;
        }
        // What the reuse stands on is read by the evaluation around it too:
        // a recording around this one keeps it.
        for probe in &probes {
            resolution.probes.log(probe, || probe.clone(), |_| true);
        }
        drop(resolution);
        if let Some(receiver) = receiver.as_deref() {
            for read in &receiver_reads {
                receiver.reads.log(&read.name, read.read);
            }
        }
        if let Some(degradation) = degradation {
            self.record_degradation(degradation);
        }
        self.dispatch.replay_transaction_read(&reuse, &replay);
        let replayed = self.dispatch.replay_undecided_relation_delta(undecided);
        verter_debug_assert!(replayed, "the count was checked to hold the delta's depth");
        self.st
            .call_evidence
            .extend(body.call_evidence.iter().copied());
        self.st.executed_walk.absorb(body.walk);
        for root in body.heritage_self_roots {
            if !self.st.heritage_self_roots.contains(&root) {
                self.st.heritage_self_roots.push(root);
            }
        }
        for diagnostic in body.checker_diagnostics {
            self.record_checker_diagnostic(diagnostic);
        }
        if let Some((node, proof)) = body.constructive_value {
            self.st.constructive_values.insert(node, proof);
        }
        #[cfg(any(test, feature = "test-support"))]
        NESTED_SIGNATURE_REUSES.with(|reuses| reuses.set(reuses.get() + 1));
        Some(ReusedNestedSignature {
            signature,
            constructive: body.constructive_signature,
        })
    }

    /// Begin recording the evaluation of `function` on `inputs` in
    /// `context`. `None` when the read could not be replayed (see
    /// [`super::super::ProjectSemanticDispatch::begin_transaction_read`]).
    fn begin_nested_signature_recording(
        &mut self,
        function: &verter_session_query::function_program::FunctionProgramKey,
        inputs: verter_identity::identity::InputBasisId,
        context: &Arc<verter_session_query::flow::slice::NestedFlowContext>,
    ) -> Option<Box<NestedSignatureRecording<'d>>> {
        let dispatch: &'d FlowCx<'d, D> = self.dispatch;
        // Every fact a body reads reaches each recording open around it, so
        // only the outermost few nested evaluations around a read record it:
        // a nest of any depth keeps each read a bounded number of times.
        if self.st.resolving_functions.borrow().frame_windows.len() >= NESTED_RECORDING_DEPTH {
            return None;
        }
        let reads = dispatch.begin_transaction_read()?;
        let receiver = self.enclosing_receiver_of(context).map(|receiver| {
            let start = receiver.reads.begin();
            (receiver, start)
        });
        let (probes_start, circular_marks) = {
            let mut resolution = self.st.resolving_functions.borrow_mut();
            resolution
                .frame_windows
                .push((self.st.enclosing_frames.len(), false));
            (resolution.probes.begin(), resolution.circular_marks)
        };
        let undecided = dispatch.open_undecided_relation_window();
        let cost = dispatch.open_inline_cost();
        Some(Box::new(NestedSignatureRecording {
            function: function.clone(),
            inputs,
            context: Arc::clone(context),
            reads,
            receiver,
            probes_start,
            circular_marks,
            undecided,
            cost,
            degradation_before: self.st.degradation.take(),
        }))
    }

    /// End `recording` (when one began) without keeping a signature.
    fn end_nested_signature_recording(
        &mut self,
        recording: Option<Box<NestedSignatureRecording<'d>>>,
    ) {
        if let Some(recording) = recording {
            self.admit_nested_signature(*recording, None, None);
        }
    }

    /// A nested function value whose slice could not be read: the
    /// unresolved value at its position.
    fn unresolved_nested_value(&mut self) -> SemanticNodeId {
        self.record_degradation(crate::semantic_query::FlowReturnDegradation::UnresolvedValue);
        self.unmodeled_position()
    }

    /// End `recording` and keep its `signature` for reuse when the body
    /// completed (`signature` is `Some`), read no member of the enclosing
    /// receiver ahead of its evaluation nor one member two ways, read no
    /// frame around it, found no return circular, took no more undecided
    /// relation outcomes off the count than it held, and its reads and cost
    /// are replayable.
    fn admit_nested_signature(
        &mut self,
        recording: NestedSignatureRecording<'d>,
        signature: Option<SemanticNodeId>,
        body: Option<NestedBodyEffects>,
    ) {
        let NestedSignatureRecording {
            function,
            inputs,
            context,
            reads,
            receiver,
            probes_start,
            circular_marks,
            undecided,
            cost,
            degradation_before,
        } = recording;
        let degradation = self.st.degradation.take();
        self.st.degradation = degradation_before.or(degradation);
        let cost = cost.close();
        let undecided = undecided.close();
        let receiver_reads = match receiver {
            Some((receiver, start)) => receiver.reads.finish(start),
            None => Some(Vec::new()),
        };
        let (probes, marks_now, frames_read) = {
            let mut resolution = self.st.resolving_functions.borrow_mut();
            let (_, frames_read) = resolution.frame_windows.pop().unwrap_or((0, true));
            (
                resolution.probes.finish(probes_start, |_, _| true),
                resolution.circular_marks,
                frames_read,
            )
        };
        let admissible = signature.is_some()
            && body.is_some()
            && cost.is_some()
            && undecided.is_some()
            && probes.is_some()
            && !frames_read
            && marks_now == circular_marks
            && receiver_reads.as_ref().is_some_and(|reads| {
                reads
                    .iter()
                    .all(|read| read.read != class_expression::ReceiverMemberRead::Forward)
            });
        let replay = self.dispatch.finish_transaction_read(reads, admissible);
        let (
            Some(signature),
            Some(body),
            Some(cost),
            Some(undecided),
            Some(probes),
            Some(receiver_reads),
            Some((reuse, replay)),
            true,
        ) = (
            signature,
            body,
            cost,
            undecided,
            probes,
            receiver_reads,
            replay,
            admissible,
        )
        else {
            return;
        };
        self.st
            .resolving_functions
            .borrow_mut()
            .signatures
            .by_function
            .entry(function)
            .or_default()
            .entry(inputs)
            .or_default()
            .push(NestedSignatureEntry {
                context,
                receiver_reads,
                probes,
                signature,
                effects: NestedSignatureEffects {
                    reuse,
                    replay,
                    cost,
                    undecided,
                    degradation,
                    body,
                },
            });
    }
}

#[cfg(test)]
mod activation_cell_contracts {
    /// A nested activation borrows nothing outside its own cell: its
    /// evaluator state and run point only into the inputs the cell owns,
    /// so the cell can be kept past any one call of the drive.
    #[test]
    fn a_nested_activation_cell_owns_everything_it_borrows() {
        fn owns_everything<T: 'static>() {}
        owns_everything::<super::NestedActivationCell>();
        owns_everything::<super::NestedOwned>();
    }
}

#[cfg(test)]
mod windowed_log_tests {
    use super::WindowedLog;

    /// A read of member `name` that found `found`; reads compare by name.
    #[derive(Clone, Debug)]
    struct Read {
        name: u8,
        found: u8,
    }

    impl PartialEq for Read {
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name
        }
    }

    impl Eq for Read {}

    impl std::hash::Hash for Read {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.name.hash(state);
        }
    }

    impl std::borrow::Borrow<u8> for Read {
        fn borrow(&self) -> &u8 {
            &self.name
        }
    }

    fn log(reads: &mut WindowedLog<Read>, name: u8, found: u8) {
        reads.log(&name, || Read { name, found }, |held| held.found == found);
    }

    fn finish(reads: &mut WindowedLog<Read>, start: usize) -> Option<Vec<(u8, u8)>> {
        reads
            .finish(start, |first, later| first.found == later.found)
            .map(|reads| reads.iter().map(|read| (read.name, read.found)).collect())
    }

    /// A window keeps each name once, however often it is read the same
    /// way; one that finds a name two ways — inside it, or once in it and
    /// once in a window inside it — is refused, while the inner window,
    /// which found it one way, closes clean.
    #[test]
    fn a_name_found_two_ways_refuses_every_window_holding_both_reads() {
        let mut reads = WindowedLog::default();
        let window = reads.begin();
        log(&mut reads, 3, 1);
        log(&mut reads, 3, 1);
        assert_eq!(finish(&mut reads, window), Some(vec![(3, 1)]));

        let window = reads.begin();
        log(&mut reads, 2, 5);
        log(&mut reads, 2, 6);
        assert_eq!(finish(&mut reads, window), None);

        let outer = reads.begin();
        log(&mut reads, 1, 10);
        let inner = reads.begin();
        log(&mut reads, 1, 11);
        assert_eq!(finish(&mut reads, inner), Some(vec![(1, 11)]));
        assert_eq!(finish(&mut reads, outer), None);

        log(&mut reads, 4, 0);
        let window = reads.begin();
        assert_eq!(
            finish(&mut reads, window),
            Some(Vec::new()),
            "no window held the read"
        );
    }
}
