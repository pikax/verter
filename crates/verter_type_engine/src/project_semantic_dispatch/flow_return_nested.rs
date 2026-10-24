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
//! borrows (its parameters, binder environment and slice content) is kept
//! by the drive's [`NestedStore`], and the frames around it are a shared
//! chain, so no evaluator borrows from a native frame of the one around it.
//!
//! A statement suspends the same way at a nested function value in its
//! declarator's initializer, and a call at a function argument its route
//! types (`flow_return_call_stack`). A nested function value evaluated
//! outside a region's statements is prepared, driven and finished in place
//! ([`FlowEvaluator::eval_nested_demand`]); a body inside it is driven
//! from the stack again.

use std::cell::{Cell, OnceCell};
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
/// how many times a return was found circular, and the signatures the
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
    open_recordings: usize,
    signatures: NestedSignatureMemo,
}

impl NestedResolution {
    /// The first position of `function` on the stack, logged for every
    /// recording window open: an evaluation that found it absent reads the
    /// same only while it stays absent.
    fn probe(
        &mut self,
        function: &verter_session_query::function_program::FunctionProgramKey,
    ) -> Option<usize> {
        self.probes.log(function, || function.clone());
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
/// name many times re-checks it once.
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
    /// the innermost does not hold it.
    pub(super) fn log<Q>(&mut self, key: &Q, read: impl FnOnce() -> T)
    where
        T: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let Some((_, seen)) = self.windows.last_mut() else {
            return;
        };
        if seen.contains(key) {
            return;
        }
        let read = read();
        seen.insert(read.clone());
        self.reads.push(read);
    }

    /// Close the innermost window, opened at `start`: each distinct read
    /// made in it, in read order. The log empties once no window is left.
    pub(super) fn finish(&mut self, start: usize) -> Vec<T> {
        self.windows.pop();
        let mut seen = rustc_hash::FxHashSet::default();
        let reads = self
            .reads
            .get(start..)
            .unwrap_or_default()
            .iter()
            .filter(|read| seen.insert(*read))
            .cloned()
            .collect();
        if self.windows.is_empty() {
            self.reads.clear();
        }
        reads
    }
}

/// What a nested function's own evaluator borrows, owned by the drive.
pub(super) struct NestedOwned {
    content: Arc<verter_session_query::flow::slice::SliceContent>,
    params: Vec<SemanticNodeId>,
    binder_env: FlowBinderEnv,
}

/// What a drive's nested evaluators borrow, kept until the drive ends.
/// Each item is appended behind the last and is never moved or dropped
/// before the store is, so a reference to it lives exactly as long as the
/// store's borrow: the borrow checker, not a release protocol, keeps every
/// evaluator borrowing an item from outliving it, on every path out of
/// the drive (a panic unwinding through it included).
///
/// Owner and lifetime: the drive that creates it
/// ([`FlowEvaluator::eval_region`], [`FlowEvaluator::eval_nested_demand_in`]),
/// on its own stack frame; every item is released when that call returns
/// or unwinds, each once.
pub(super) struct NestedStore<'a, T = NestedOwned> {
    first: OnceCell<Box<NestedLink<T>>>,
    last: Cell<Option<&'a NestedLink<T>>>,
    #[cfg(any(test, feature = "test-support"))]
    kept: Cell<usize>,
}

/// One kept item and the items kept after it.
struct NestedLink<T> {
    owned: T,
    next: OnceCell<Box<NestedLink<T>>>,
}

/// The links after this one drop from this loop, each taken off in turn:
/// dropping them link inside link would take a native level per item.
impl<T> Drop for NestedLink<T> {
    fn drop(&mut self) {
        let mut next = self.next.take();
        while let Some(mut link) = next {
            next = link.next.take();
        }
    }
}

impl<'a, T> NestedStore<'a, T> {
    pub(super) fn new() -> Self {
        Self {
            first: OnceCell::new(),
            last: Cell::new(None),
            #[cfg(any(test, feature = "test-support"))]
            kept: Cell::new(0),
        }
    }

    /// Keep `owned` until the store drops.
    fn keep(&'a self, owned: T) -> &'a T {
        let cell = match self.last.get() {
            Some(link) => &link.next,
            None => &self.first,
        };
        let link: &'a NestedLink<T> = cell.get_or_init(|| {
            Box::new(NestedLink {
                owned,
                next: OnceCell::new(),
            })
        });
        self.last.set(Some(link));
        #[cfg(any(test, feature = "test-support"))]
        self.kept.set(self.kept.get() + 1);
        &link.owned
    }

    /// How many items the store keeps.
    #[cfg(test)]
    pub(super) fn kept(&self) -> usize {
        self.kept.get()
    }
}

/// A nested function's body under evaluation by its own evaluator.
pub(super) struct NestedActivation<'d, 'a, D: FlowDemandDriver> {
    evaluator: Box<FlowEvaluator<'d, 'a, D>>,
    run: RegionRun<'a>,
    owned: &'a NestedOwned,
    finish: NestedFinish<'d>,
}

/// What composing a nested function's signature needs once its body has
/// been evaluated.
pub(super) struct NestedFinish<'d> {
    circular: bool,
    declared_return_none: bool,
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
    inputs: verter_identity::identity::InputBasisId,
    context: Arc<verter_session_query::flow::slice::NestedFlowContext>,
    receiver_reads: Vec<class_expression::ReceiverRead>,
    probes: Vec<verter_session_query::function_program::FunctionProgramKey>,
    signature: SemanticNodeId,
    effects: NestedSignatureEffects,
}

/// What a nested function's evaluation left on its enclosing evaluator and
/// the builds around it, replayed when its signature is reused: its read
/// rails ([`super::super::dispatch_txn::FlowMemberReuse`]), the first
/// degradation its signature's composition
/// recorded on the enclosing evaluator, and its body's effects.
struct NestedSignatureEffects {
    reuse: verter_session_query::facts::reuse::ReuseClass,
    replay: super::super::dispatch_txn::FlowMemberReuse,
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    body: NestedBodyEffects,
}

/// What a nested function's body evaluation left on its enclosing
/// evaluator: its walk, call evidence, heritage roots, checker diagnostics,
/// the undecided relation outcomes its calls consumed, and the
/// constructive evidence of its value and signature.
pub(super) struct NestedBodyEffects {
    walk: ExecutedSliceWalk,
    call_evidence: Vec<FlowCallEvidence>,
    heritage_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    checker_diagnostics: Vec<crate::semantic_query::CheckerDiagnosticOccurrence>,
    undecided_relations: u64,
    constructive_value: Option<(SemanticNodeId, ConstructiveValue)>,
    constructive_signature: bool,
}

/// The nested signatures one root evaluation composed completely, by
/// function. Shared by every nested evaluator of the root evaluation and
/// dropped with it: it lives no longer than the evaluation whose builds its
/// replays fold into. Only the outermost [`NESTED_RECORDING_DEPTH`] nested
/// evaluations around a read record it, so the memo keeps each read a
/// bounded number of times however deep the nest.
#[derive(Default)]
pub(super) struct NestedSignatureMemo {
    by_function: rustc_hash::FxHashMap<
        verter_session_query::function_program::FunctionProgramKey,
        Vec<NestedSignatureEntry>,
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
    /// The call resolutions' undecided relation count when it began.
    undecided_relations: u64,
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
pub(super) enum NestedPrepared<'d, 'a, D: FlowDemandDriver> {
    /// Its signature, with nothing to evaluate.
    Value(SemanticNodeId),
    /// Its body, to evaluate on its own evaluator.
    Child(Box<NestedActivation<'d, 'a, D>>),
}

/// How a drive ends.
enum DriveEnd {
    /// The region it was started on completed.
    Region((Result<Vec<FlowContribution>, FlowReturnFailure>, bool)),
    /// The nested function it was started on is composed.
    Nested(SemanticNodeId),
}

impl<'d, 'b, D: FlowDemandDriver> FlowEvaluator<'d, 'b, D> {
    /// Evaluate a region, driving the nested functions its return
    /// statements reach from the stack (see the module docs).
    pub(super) fn eval_region(
        &mut self,
        region: &verter_session_query::flow::slice::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        let store = NestedStore::new();
        let run = self.start_region_run(region);
        match self.drive(&store, Some(run), Vec::new()) {
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
        let frame_env = self.binder_env;
        self.eval_nested_demand_in(demand, own_env.as_deref().unwrap_or(frame_env))
    }

    /// [`Self::eval_nested_demand`] under `outer_env`, the binder
    /// environment the function sits in (a class member's is its class's).
    pub(super) fn eval_nested_demand_in(
        &mut self,
        demand: NestedDemand,
        outer_env: &FlowBinderEnv,
    ) -> SemanticNodeId {
        let store = NestedStore::new();
        let prepared = self.prepare_nested(demand, outer_env, &store);
        let node = match prepared {
            NestedPrepared::Value(node) => node,
            NestedPrepared::Child(child) => match self.drive(&store, None, vec![*child]) {
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
        store: &'a NestedStore<'a>,
        mut root: Option<RegionRun<'r>>,
        mut stack: Vec<NestedActivation<'d, 'a, D>>,
    ) -> DriveEnd
    where
        'b: 'a,
        'd: 'a,
        'd: 'r,
    {
        let mut delivered: Option<RegionDelivery> = None;
        loop {
            // The evaluator on top runs; a nested function it suspends at is
            // prepared on it.
            let prepared = match stack.last_mut() {
                Some(top) => match top.evaluator.run_region(&mut top.run, delivered.take()) {
                    RegionProgress::Nested(demand) => {
                        let own_env = demand.outer_env.clone();
                        let outer_env = own_env.as_deref().unwrap_or(top.evaluator.binder_env);
                        Some(top.evaluator.prepare_nested(demand, outer_env, store))
                    }
                    RegionProgress::Done(outcome) => {
                        let finished = stack.pop().expect("the evaluator on top");
                        let node = match stack.last_mut() {
                            Some(parent) => parent.evaluator.finish_nested(finished, outcome),
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
                },
                None => {
                    let run = root
                        .as_mut()
                        .expect("a drive with no nested evaluator runs its root");
                    match self.run_region(run, delivered.take()) {
                        RegionProgress::Nested(demand) => {
                            let own_env = demand.outer_env.clone();
                            let frame_env = self.binder_env;
                            let outer_env = own_env.as_deref().unwrap_or(frame_env);
                            Some(self.prepare_nested(demand, outer_env, store))
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
    /// evaluator, whose borrowed data `store` keeps.
    pub(super) fn prepare_nested<'a>(
        &mut self,
        demand: NestedDemand,
        outer_env: &FlowBinderEnv,
        store: &'a NestedStore<'a>,
    ) -> NestedPrepared<'d, 'a, D>
    where
        'b: 'a,
    {
        let NestedDemand {
            function,
            context,
            has_declared_return,
            extended_captures,
            declared_evolving_captures,
            contextual,
            outer_env: _,
        } = demand;
        self.constructive_signatures.remove(&function);
        let function = &function;
        let context = &context;
        let extended_captures: &[SkeletonBindingId] = &extended_captures;
        let declared_evolving_captures: &[verter_session_query::function_program::FlowBindingIdentity] =
            &declared_evolving_captures;
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: verter_type_expr::OwnedStr::from(self.canonical),
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
        let mut precomputed_captures = None;
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
                .resolving_functions
                .borrow_mut()
                .probe(function)
                .is_some();
            if let Some(inputs) = inputs.filter(|_| !reentered) {
                if let Some(reused) = self.reuse_nested_signature(function, &inputs, context) {
                    if reused.constructive {
                        self.constructive_signatures
                            .insert(function.clone(), reused.signature);
                    }
                    return NestedPrepared::Value(reused.signature);
                }
                recording = self.begin_nested_signature_recording(function, inputs, context);
            }
            precomputed_captures = captures;
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
        let reentered = self.resolving_functions.borrow_mut().probe(function);
        if let Some(position) = reentered {
            self.resolving_functions
                .borrow_mut()
                .mark_circular_from(position);
        }
        let circular = reentered.is_some() && content.declared_return.is_none();
        if !circular {
            self.resolving_functions.borrow_mut().push(function.clone());
        }
        let step = self.nested_signature_prefix(
            precomputed_captures,
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
                return NestedPrepared::Value(self.nested_function_postlude(
                    circular,
                    declared_return_none,
                    signature,
                ));
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
            extended_capture_identities,
            execution_selection,
            plan,
            bound,
            skeleton,
        } = parts;
        let enclosing_receiver = self.enclosing_receiver_of(context);
        #[cfg(any(test, feature = "test-support"))]
        NESTED_BODY_EVALUATIONS.with(|evaluations| evaluations.set(evaluations.get() + 1));
        let enclosing_frames = self.enclosing_frames.with(
            self.bindings.function().clone(),
            self.binder_env,
            self.products.clone(),
        );
        let owned = store.keep(NestedOwned {
            content,
            params,
            binder_env,
        });
        let capture_context = context;
        let mut evaluator = Box::new(FlowEvaluator {
            independent_members: None,
            independent_returns: None,
            constructive_values: rustc_hash::FxHashMap::default(),
            constructive_signatures: rustc_hash::FxHashMap::default(),
            dispatch: self.dispatch,
            call_receivers: rustc_hash::FxHashMap::default(),
            call_arguments: Arc::clone(&owned.content.call_arguments),
            self_slot: None,
            resolving_functions: std::rc::Rc::clone(&self.resolving_functions),
            this_free_values: std::rc::Rc::clone(&self.this_free_values),
            declared_type_reads: std::rc::Rc::clone(&self.declared_type_reads),
            carrier_unwraps: std::rc::Rc::clone(&self.carrier_unwraps),
            discriminant_members: std::rc::Rc::clone(&self.discriminant_members),
            guard_relations: std::rc::Rc::clone(&self.guard_relations),
            arm_indexes: std::rc::Rc::clone(&self.arm_indexes),
            canonical: self.canonical,
            owner: self.owner,
            nullability: self.nullability,
            no_implicit_any: self.no_implicit_any,
            use_unknown_in_catch_variables: self.use_unknown_in_catch_variables,
            no_implicit_this: self.no_implicit_this,
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
            declared_reads: false,
            dead_writes: false,
            dead_written: rustc_hash::FxHashSet::default(),
            dead_written_log: Vec::new(),
            receiver: enclosing_receiver,
        });
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
                    class_expression::ClassReceiver::of_type(binder, Some(binder)),
                ));
            }
        }
        evaluator.bind_explicit_this(owned.content.declared_this.as_ref());
        evaluator.seed_hoisted_var_declarations(body);
        let run = evaluator.start_region_run(body);
        NestedPrepared::Child(Box::new(NestedActivation {
            evaluator,
            run,
            owned,
            finish: NestedFinish {
                circular,
                declared_return_none,
                signature_params,
                type_param_decls,
                contextual,
                recording,
            },
        }))
    }

    /// Compose a nested function's signature on this evaluator (its parent)
    /// from its body's evaluation, `outcome`, and release its evaluator.
    fn finish_nested(
        &mut self,
        activation: NestedActivation<'d, '_, D>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> SemanticNodeId {
        let NestedActivation {
            evaluator: mut nested_evaluator,
            run,
            owned,
            finish,
        } = activation;
        drop(run);
        let NestedFinish {
            circular,
            declared_return_none,
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
                nested_evaluator.promote_pending_statement_gap();
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
                .products
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
                        self.products.execution.borrow_mut().failure = Some(failure);
                        nested_evaluator.executed_walk.aborted = true;
                    }
                    nested_evaluator.record_degradation(
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
                        self.constructive_values.insert(node, proof);
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
                        && crate::request_context::current_execution_abort().is_none();
                    recorded_effects = Some(NestedBodyEffects {
                        walk: nested_evaluator.executed_walk,
                        call_evidence: nested_evaluator.call_evidence.clone(),
                        heritage_self_roots: nested_evaluator.heritage_self_roots.clone(),
                        checker_diagnostics: nested_evaluator.checker_diagnostics.clone(),
                        undecided_relations: 0,
                        constructive_value: constructive_value.take(),
                        constructive_signature: false,
                    });
                }
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
        drop(nested_evaluator);
        let value = self.nested_function_postlude(circular, declared_return_none, signature);
        if let Some(recording) = recording {
            self.admit_nested_signature(
                *recording,
                (body_complete && value == signature).then_some(signature),
                recorded_effects.map(|effects| NestedBodyEffects {
                    constructive_signature,
                    ..effects
                }),
            );
        }
        if constructive_signature && value == signature {
            self.constructive_signatures.insert(function, signature);
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
        signature: SemanticNodeId,
    ) -> SemanticNodeId {
        let found_circular =
            !circular && self.resolving_functions.borrow_mut().pop().unwrap_or(false);
        if found_circular && declared_return_none {
            return self.signature_returning_any(signature);
        }
        signature
    }

    /// The signature the root evaluation composed for `function` on
    /// `inputs` in `context`, when every member its evaluation read off the
    /// enclosing receiver reads the same now and every function it looked
    /// up on the resolution stack is still absent from it: its effects
    /// replayed on this evaluator and the builds around it, exactly as
    /// evaluating the body again would leave them.
    pub(super) fn reuse_nested_signature(
        &mut self,
        function: &verter_session_query::function_program::FunctionProgramKey,
        inputs: &verter_identity::identity::InputBasisId,
        context: &Arc<verter_session_query::flow::slice::NestedFlowContext>,
    ) -> Option<ReusedNestedSignature> {
        let receiver = self.enclosing_receiver_of(context);
        let resolution = std::rc::Rc::clone(&self.resolving_functions);
        let mut resolution = resolution.borrow_mut();
        let entry = resolution
            .signatures
            .by_function
            .get(function)?
            .iter()
            .find(|entry| {
                entry.inputs == *inputs
                    && (Arc::ptr_eq(&entry.context, context) || entry.context == *context)
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
            degradation,
            body,
        } = &entry.effects;
        let (reuse, replay, degradation) = (*reuse, replay.clone(), *degradation);
        let body = NestedBodyEffects {
            walk: body.walk,
            call_evidence: body.call_evidence.clone(),
            heritage_self_roots: body.heritage_self_roots.clone(),
            checker_diagnostics: body.checker_diagnostics.clone(),
            undecided_relations: body.undecided_relations,
            constructive_value: body.constructive_value.clone(),
            constructive_signature: body.constructive_signature,
        };
        // What the reuse stands on is read by the evaluation around it too:
        // a recording around this one keeps it.
        for probe in &probes {
            resolution.probes.log(probe, || probe.clone());
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
        self.dispatch
            .restore_call_relation_observations(body.undecided_relations);
        self.call_evidence
            .extend(body.call_evidence.iter().copied());
        self.executed_walk.absorb(body.walk);
        for root in body.heritage_self_roots {
            if !self.heritage_self_roots.contains(&root) {
                self.heritage_self_roots.push(root);
            }
        }
        for diagnostic in body.checker_diagnostics {
            self.record_checker_diagnostic(diagnostic);
        }
        if let Some((node, proof)) = body.constructive_value {
            self.constructive_values.insert(node, proof);
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
        if self.resolving_functions.borrow().open_recordings >= NESTED_RECORDING_DEPTH {
            return None;
        }
        let reads = dispatch.begin_transaction_read()?;
        let receiver = self.enclosing_receiver_of(context).map(|receiver| {
            let start = receiver.reads.begin();
            (receiver, start)
        });
        let (probes_start, circular_marks) = {
            let mut resolution = self.resolving_functions.borrow_mut();
            resolution.open_recordings += 1;
            (resolution.probes.begin(), resolution.circular_marks)
        };
        Some(Box::new(NestedSignatureRecording {
            function: function.clone(),
            inputs,
            context: Arc::clone(context),
            reads,
            receiver,
            probes_start,
            circular_marks,
            undecided_relations: dispatch.call_undecided_relations(),
            degradation_before: self.degradation.take(),
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
    /// receiver ahead of its evaluation, found no return circular, gave back
    /// no undecided relation outcome it did not consume, and its reads are
    /// replayable.
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
            undecided_relations,
            degradation_before,
        } = recording;
        let degradation = self.degradation.take();
        self.degradation = degradation_before.or(degradation);
        let receiver_reads = receiver
            .map(|(receiver, start)| receiver.reads.finish(start))
            .unwrap_or_default();
        let (probes, marks_now) = {
            let mut resolution = self.resolving_functions.borrow_mut();
            resolution.open_recordings -= 1;
            (
                resolution.probes.finish(probes_start),
                resolution.circular_marks,
            )
        };
        let undecided_now = self.dispatch.call_undecided_relations();
        let body = body
            .filter(|_| undecided_now >= undecided_relations)
            .map(|body| NestedBodyEffects {
                undecided_relations: undecided_now - undecided_relations,
                ..body
            });
        let admissible = signature.is_some()
            && body.is_some()
            && marks_now == circular_marks
            && receiver_reads
                .iter()
                .all(|read| read.read != class_expression::ReceiverMemberRead::Forward);
        let replay = self.dispatch.finish_transaction_read(reads, admissible);
        let (Some(signature), Some(body), Some((reuse, replay)), true) =
            (signature, body, replay, admissible)
        else {
            return;
        };
        self.resolving_functions
            .borrow_mut()
            .signatures
            .by_function
            .entry(function)
            .or_default()
            .push(NestedSignatureEntry {
                inputs,
                context,
                receiver_reads,
                probes,
                signature,
                effects: NestedSignatureEffects {
                    reuse,
                    replay,
                    degradation,
                    body,
                },
            });
    }
}

#[cfg(test)]
mod nested_store_tests {
    use super::NestedStore;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// An item that records its own release in `released`.
    struct Tracked {
        id: usize,
        released: Rc<RefCell<Vec<usize>>>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.released.borrow_mut().push(self.id);
        }
    }

    fn tracked(id: usize, released: &Rc<RefCell<Vec<usize>>>) -> Tracked {
        Tracked {
            id,
            released: Rc::clone(released),
        }
    }

    /// Every kept item stays readable through the reference `keep` handed
    /// out while later items are kept, and is released once when the store
    /// drops.
    #[test]
    fn a_store_keeps_every_item_until_it_drops_and_releases_each_once() {
        let released = Rc::new(RefCell::new(Vec::new()));
        {
            let store = NestedStore::new();
            let first = store.keep(tracked(0, &released));
            let kept: Vec<&Tracked> = (1..100)
                .map(|id| store.keep(tracked(id, &released)))
                .collect();
            assert_eq!(first.id, 0);
            assert!(kept
                .iter()
                .enumerate()
                .all(|(index, item)| item.id == index + 1));
            assert_eq!(store.kept(), 100);
            assert!(released.borrow().is_empty());
        }
        let mut released = released.borrow().clone();
        released.sort_unstable();
        assert_eq!(released, (0..100).collect::<Vec<_>>());
    }

    /// A panic while items are kept and borrowed unwinds through the store,
    /// which releases every item once.
    #[test]
    fn a_panic_holding_kept_items_releases_each_once_on_unwind() {
        let released = Rc::new(RefCell::new(Vec::new()));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let store = NestedStore::new();
            let items: Vec<&Tracked> = (0..10)
                .map(|id| store.keep(tracked(id, &released)))
                .collect();
            assert_eq!(items.len(), 10);
            panic!("unwinding with {} kept items", store.kept());
        }));
        assert!(caught.is_err());
        let mut released = released.borrow().clone();
        released.sort_unstable();
        assert_eq!(released, (0..10).collect::<Vec<_>>());
    }

    /// The items drop from a loop: a store keeping 1,000,000 items (one per
    /// nested function of a drive) releases them on a 64 KiB thread.
    #[test]
    fn a_million_kept_items_release_on_a_small_stack() {
        let released = std::thread::Builder::new()
            .stack_size(64 << 10)
            .spawn(|| {
                let released = Rc::new(RefCell::new(Vec::new()));
                {
                    let store = NestedStore::new();
                    for id in 0..1_000_000 {
                        store.keep(tracked(id, &released));
                    }
                }
                let count = released.borrow().len();
                count
            })
            .expect("spawn the releasing thread")
            .join()
            .expect("the store releases its items");
        assert_eq!(released, 1_000_000);
    }
}
