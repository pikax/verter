//! Instantiations on the continuation runtime.
//!
//! An instantiation or a `typeof` evaluated outside any drive is driven:
//! its build runs as a frame whose declaration-body projection, where it
//! would evaluate an instantiation in place, needs it instead, so a chain
//! of instantiations
//! (an alias chain, a reference whose arguments are applications) is a
//! chain of heap-owned frames, never of native calls. A body projection
//! that reaches a value's `typeof` needs it the same way, and the `typeof`
//! of a value typed by its authored value position (an annotation, an
//! inferred initializer, a const-object shape) builds as a frame too,
//! whose projection needs each instantiation and `typeof` it reaches: an
//! interface heritage a hundred levels deep, named by an annotation, is a
//! hundred frames, and so is a chain of a hundred constants each
//! initialized from the one before. Each frame owns what
//! its build owns across its steps — the build's state, its producer lease
//! on the memo, its fact tracer and its build-local taint — and installs
//! the tracer and taint only while one of its steps runs. A delivery
//! carries the answering result's carrier, which the consuming frame
//! replays into its own tracer when it resumes, and its rails, which it
//! folds into its own taint: what a nested evaluation left on the enclosing
//! build's tracer and taint frame when it ran on the native stack.
//!
//! A body projection that reaches a conditional type needs its decision as
//! well. The decision runs as a frame whose reads of instantiations and
//! `typeof`s are answered by the deliveries it received; one it has no
//! answer for becomes its need, and the run that reached it is dropped and
//! runs again once that need is delivered. A chain of conditionals each
//! deciding over the one before is therefore a chain of frames too.
//!
//! A call an initializer's evaluation reaches resolves as a frame too,
//! when its resolution is a machinery root. Its candidate walk stops before
//! an argument relation whose operands need instantiations or `typeof`s the
//! frame has no answer for, holding its candidate's collecting session out
//! of the transaction, and resumes with the same session once each is
//! delivered: a call's inference spans the frame's suspensions.
//!
//! A function's return evaluated outside any drive, whose body calls with a
//! function value among the arguments, is a frame as well, run as a decision
//! is: an instantiation or a `typeof` its own body reads that must be
//! computed becomes the frame's need, and the run that reached it is dropped
//! and runs again once it is delivered. When the read is made while a
//! contextual call in the body types a callback — the call's candidate
//! collector held out of the transaction meanwhile — the call's route is
//! held as it stood before the callback, with its collector, and the next
//! run resumes the held route at that call instead of asking its executor
//! again: the callback types over answered reads, into the same collector.
//!
//! Every other query, and every query a frame's step evaluates in place,
//! keeps the synchronous path; code a step reaches never drives.
//!
//! A frame's cost recording opens at its first step and stays open until
//! it completes: the drive runs one chain, so the recordings nest exactly
//! as the frames do, and the charges a needed demand's entry takes between
//! two steps accrue to the frame that needed it, as they do on the native
//! stack.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::build::{InstantiateBuild, InstantiatePoll, InstantiateStart, TypeofBuild, TypeofStart};
use super::cost_receipt::Nesting;
use super::{BuildLocalTaint, CarrierNormalizationPrelude, ProjectSemanticDispatch};
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::semantic_execution::{EvalStep, External, Outcome, Program, SemanticExecution, Start};
use crate::semantic_query::{
    CacheRead, QueryError, QueryResult, ReadReceipt, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue,
};
use crate::semantic_query_memo::{
    Claim, ClaimAttempt, ExecutionTask, Joined, ProducerLease, ReadCapture, Recursion, Subscription,
};
use verter_session_query::facts::fact_cache::ReadSetSignature;

type ValueRead = CacheRead<QueryResult<SemanticQueryValue>>;

/// What a query answered to the frame that needed it: the read (with the
/// answering result's cost receipt, for the consumer's recording), and the
/// carrier of the result that answered it, for the consumer to replay.
#[derive(Clone)]
pub(super) struct QueryDelivery {
    read: ValueRead,
    carrier: Option<ReadSetSignature>,
}

/// A delivery a frame's build consumes where its evaluation reads it,
/// with how deep the needing frame nests.
pub(super) struct FrameDelivery {
    delivery: QueryDelivery,
    depth: u32,
}

/// The program instantiations run on: the dispatcher they evaluate through
/// — the semantic task's own, holding the request state the task borrowed
/// — the graph its producers and subscriptions are claimed on, and the
/// task that owns every producer the drive claims.
pub(super) struct QueryProgram<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: ProjectSemanticDispatch<'a, C>,
    graph: &'p crate::semantic_query_memo::SemanticGraphStore,
    task: ExecutionTask,
    /// Claims that continue after an outside producer went away.
    retries: FxHashMap<SemanticQueryKey, ClaimAttempt>,
    /// The depth a frame started now nests at: one below the frame whose
    /// step needed it (the drive runs one chain, so a demand starts right
    /// after the step that needed it, or after its wait restarts it).
    child_depth: u32,
    /// Test-only: the demands a call's walk suspends on are claimed by
    /// another task first, and held until released.
    #[cfg(any(test, feature = "test-support"))]
    held_call_needs: Option<Vec<(crate::semantic_query_memo::ExecutionTask, ProducerLease<'p>)>>,
}

/// A suspended semantic computation.
pub(super) enum QueryFrame<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    Instantiate(Box<InstantiateFrame<'p, 'a, C>>),
}

/// The build a frame carries between its steps: an instantiation's, a
/// `typeof`'s whose value's annotation is lowering, a conditional type's
/// decision, a call's resolution, or a function's flow return.
pub(super) enum FrameBuild {
    Instantiate(Box<InstantiateBuild>),
    Typeof(Box<TypeofBuild>),
    Conditional(Box<ConditionalBuild>),
    Call(Box<CallBuild>),
    Flow(Box<FlowBuild>),
}

/// A function's flow return between its steps: the answers to the reads
/// its contextual callbacks made, the one it waits on and the others its
/// last run reached, and the contextual calls it holds.
///
/// A run evaluates the function's body. A read of an instantiation or a
/// `typeof` the body itself makes — not a query it evaluates in place, nor a
/// callee evaluated inline beneath it — that none of the answers holds and
/// that must be computed becomes a need of the frame, and the run is dropped
/// with what it observed: a chain of values the body reads is then a chain
/// of frames, as it is for a synchronous build that drives each. When a
/// contextual call in the body was typing a callback at that read, its
/// candidate's collector held out of the transaction, or relating the typed
/// callback needs it, the call's route is held as it stood before the
/// callback, collector included; the next run resumes the held route where
/// the call is asked again, and the callback types over the delivered
/// answers into that same collector.
#[derive(Default)]
pub(super) struct FlowBuild {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    awaiting: Option<SemanticQueryKey>,
    /// The claim the awaited read entered with, until the runtime takes it.
    claim: Option<ClaimAttempt>,
    pending: std::collections::VecDeque<(SemanticQueryKey, ClaimAttempt)>,
    /// The contextual calls held across the frame's runs, each with its
    /// candidate's collector.
    routes: Vec<super::flow_return::HeldContextualRoute>,
}

/// What the flow evaluation whose frame is stepping reads at its seam.
pub(super) struct FlowSeam {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    /// The reads the run reached that no answer holds, in the order it
    /// reached them, each with the claim it entered with.
    needs: Vec<(SemanticQueryKey, ClaimAttempt)>,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
    /// How many queries the run is evaluating in place now: their reads
    /// evaluate as they would anywhere.
    nested: u32,
    /// How many contextual callbacks are typing now, each under a held
    /// collector: a need made meanwhile holds their call's route.
    windows: u32,
    /// The checker transaction's frame depth the function's own evaluation
    /// runs at: a callee's evaluated inline beneath it is not this frame's.
    frame_depth: usize,
    routes: Vec<super::flow_return::HeldContextualRoute>,
}

/// A call resolution between its steps: the executor frame its walk
/// opened, the walk itself once it stopped — its candidate's collecting
/// session included — and the answers to the operand demands it stopped
/// on, the one it waits on and the others it reached.
///
/// The walk stops before an argument relation whose operands — the
/// argument's type and its parameter's — name instantiations or `typeof`s
/// the frame has no answer for: the frame needs each, and the walk resumes
/// at that argument with the same collector, which then relates over
/// answered operands. Nothing the frame's needs evaluate meanwhile sees the
/// collector: it is out of the transaction until the walk resumes, so a
/// call nested in those evaluations deposits into its own.
#[derive(Default)]
pub(super) struct CallBuild {
    /// The call's executor frame, open from the walk's first step until it
    /// closes.
    frame: Option<usize>,
    suspension: Option<Box<super::call_resolve::CallSuspension>>,
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    awaiting: Option<SemanticQueryKey>,
    /// The claim the awaited demand entered with, until the runtime takes it.
    claim: Option<ClaimAttempt>,
    pending: std::collections::VecDeque<(SemanticQueryKey, ClaimAttempt)>,
}

/// What the walk of the call whose frame is stepping reads at its seam:
/// the walk it resumes, and its operand demands' answers and needs.
pub(super) struct CallSeam {
    key: crate::semantic_query::ResolveCallKey,
    resume: Option<Box<super::call_resolve::CallSuspension>>,
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    needs: Vec<(SemanticQueryKey, ClaimAttempt)>,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
}

/// What a frame's build did in one step.
enum FramePoll {
    Need(SemanticQueryKey),
    Done(Box<crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue>>),
}

impl From<InstantiatePoll> for FramePoll {
    fn from(poll: InstantiatePoll) -> Self {
        match poll {
            InstantiatePoll::Need(key) => Self::Need(key),
            InstantiatePoll::Done(output) => Self::Done(Box::new((*output).into())),
        }
    }
}

/// A conditional type's decision between its steps: the answers to the
/// instantiations and `typeof`s its decision read, the one it waits on and
/// the others its last run reached without an answer.
///
/// The decision procedure is a function of its operands and of those
/// answers. A step runs it with the answers the frame holds; every read
/// none of them answers becomes one of the frame's needs, and the step's
/// run is discarded — its observations with it — to run again once each of
/// them is delivered. No read is ever evaluated in place one native level
/// deeper, so a chain of conditionals each deciding over the one before is
/// a chain of frames, and a decision over a wide union runs again once
/// for its operands, not once per member.
#[derive(Default)]
pub(super) struct ConditionalBuild {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    awaiting: Option<SemanticQueryKey>,
    /// The claim the awaited read entered with, until the runtime takes it.
    claim: Option<ClaimAttempt>,
    /// The other reads the last run reached without an answer, in the order
    /// it reached them, each with the claim it entered with.
    pending: std::collections::VecDeque<(SemanticQueryKey, ClaimAttempt)>,
}

/// What a running conditional decision's reads are answered from: its
/// frame's answers, and the reads they do not answer.
pub(super) struct DecisionSeam {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    /// The reads the run reached that no answer holds, in the order it
    /// reached them, each with the claim it entered with for the runtime
    /// to produce it.
    needs: Vec<(SemanticQueryKey, ClaimAttempt)>,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
    /// How many queries the decision reached are evaluating in place now.
    /// Inside one, a read the answers hold is still answered by them, but
    /// any other evaluates as it would anywhere: a query evaluated in place
    /// keeps what it builds, so it never reads a gap.
    nested: u32,
}

/// A query the running decision reached, evaluating in place; the seam is
/// the decision's own again when it returns.
pub(super) struct NestedDecisionQuery<'d> {
    seam: Option<&'d std::cell::RefCell<Option<DecisionSeam>>>,
    flow: Option<&'d std::cell::RefCell<Option<FlowSeam>>>,
}

impl Drop for NestedDecisionQuery<'_> {
    fn drop(&mut self) {
        if let Some(cell) = self.seam {
            if let Some(seam) = cell.borrow_mut().as_mut() {
                seam.nested -= 1;
            }
        }
        if let Some(cell) = self.flow {
            if let Some(seam) = cell.borrow_mut().as_mut() {
                seam.nested -= 1;
            }
        }
    }
}

/// An instantiation's — or a `typeof`'s — build between its steps. It
/// names the request state it entered (the declaration it made active, the
/// cost recording it opened, a request-local completion it holds open) and
/// the program releases each when the frame is abandon
/// ([`QueryProgram::abandon`]).
pub(super) struct InstantiateFrame<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    key: SemanticQueryKey,
    producer: Option<FrameProducer<'p>>,
    tracer: Option<crate::fact_signature_helpers::StepwiseFactTracer<'a, C::Clocks>>,
    taint: BuildLocalTaint,
    build: Option<FrameBuild>,
    /// The declaration and arguments the build entered as active, until it
    /// leaves them.
    active: Option<(
        super::InstantiateIdentity,
        Arc<[crate::semantic_query::SemanticNodeId]>,
    )>,
    /// How deep this instantiation nests in the drive's chain: the root's
    /// is 1.
    depth: u32,
    /// Whether this frame's cost recording is open on the ledger.
    recording: bool,
    /// The edit clocks the frame's build computes under.
    computed_at: super::WorkspaceEdit,
}

/// What produces a frame's demand: the family memo's producer lease, or —
/// for a cheap intermediate nested in another computation
/// ([`super::completes_request_locally`]) — the request's own completion of
/// the claim its lookup missed, open on the dispatcher until it completes.
pub(super) enum FrameProducer<'p> {
    Lease(ProducerLease<'p>),
    /// The request-local completion of the frame's key, which the
    /// dispatcher's request-local open set holds open until the frame
    /// completes or is abandoned.
    RequestLocal {
        attempt: ClaimAttempt,
    },
}

impl FrameProducer<'_> {
    fn cost_identity(&self) -> super::cost_receipt::CostIdentity {
        match self {
            Self::Lease(lease) => lease.cost_identity(),
            Self::RequestLocal { attempt, .. } => attempt.cost_identity(),
        }
    }
}

/// A subscription to another task's producer, with the claim it continues.
pub(super) struct PendingClaim<'p> {
    key: SemanticQueryKey,
    subscription: Subscription<'p>,
    /// Taken when the subscription resolves, for the claim to continue.
    attempt: Option<ClaimAttempt>,
    /// How the claim's result is read: the needed key's frame depth.
    nesting: Nesting,
}

/// One semantic query's evaluation as a task: the execution of its frames,
/// the program they run on — with the dispatcher that holds the request
/// state the task borrowed ([`ProjectSemanticDispatch::lend_semantic_task`])
/// — the construction owner that roots what its polls construct, and the
/// validity its polls observe.
///
/// Everything a suspended task needs is its own: its frames, their builds
/// and leases, its outside subscription, the checker transaction with its
/// inference sessions, its active instantiations, cost recordings and taint
/// stack, the counted roots of every node it constructed, the scheduler
/// job whose cancellation it observes, and the component-meta charges,
/// request spend and refusals it records for the computes it was lent
/// under. It reaches the request only through immutable shared inputs. So
/// the task is `Send` and one exclusive poller may resume it on any thread:
/// each poll installs its construction owner, its execution task, its
/// validity, its job cancellation, its accounting and its bound request
/// context on the polling thread for the poll's extent and restores the
/// thread's own after it.
pub struct SemanticTask<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> {
    execution: SemanticExecution<QueryProgram<'p, 'a, C>>,
    /// Taken when the task gives its request state back: by
    /// [`ProjectSemanticDispatch::return_semantic_task`], or by its drop
    /// through `home`.
    program: Option<QueryProgram<'p, 'a, C>>,
    construction: Option<crate::semantic_query_memo::node_roots::TaskConstructionOwner>,
    validity: crate::request_context::TaskExecutionValidity,
    /// The scheduler job the task was lent under: its aggregate token
    /// decides cancellation on every thread the task is polled on.
    job_cancellation: Option<verter_execution::cancellation::CancellationToken>,
    accounting: TaskAccounting,
    /// Where a task dropped without being returned leaves its request state
    /// for the dispatcher that lent it.
    home: Arc<parking_lot::Mutex<Option<UnreturnedTask<'a, C>>>>,
    /// The root the first poll opens.
    root: Option<SemanticQueryKey>,
}

/// The per-thread accounting a task records for the computes it was lent
/// under, carried between its polls.
#[derive(Default)]
struct TaskAccounting {
    spans: super::connected_demand::TaskSpanCarry,
    spend: crate::request_budget::TaskSpendCarry,
    refusals: crate::fact_tracing::refusal_scope::TaskRefusalCarry,
    completeness: crate::request_context::TaskCompletenessCarry,
}

impl TaskAccounting {
    fn lend() -> Self {
        Self {
            spans: super::connected_demand::TaskSpanCarry::lend(),
            spend: crate::request_budget::TaskSpendCarry::lend(),
            refusals: crate::fact_tracing::refusal_scope::TaskRefusalCarry::lend(),
            completeness: crate::request_context::TaskCompletenessCarry::lend(),
        }
    }

    fn return_to_lender(self) {
        self.spans.return_to_lender();
        self.spend.return_to_lender();
        self.refusals.return_to_lender();
        self.completeness.return_to_lender();
    }
}

/// What a task dropped without being returned leaves for its lender: the
/// request state its dispatcher borrowed, its construction ownership, the
/// abort its polls observed and the accounting it recorded.
struct UnreturnedTask<'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: ProjectSemanticDispatch<'a, C>,
    validity: crate::request_context::TaskExecutionValidity,
    accounting: TaskAccounting,
    construction: crate::semantic_query_memo::node_roots::TaskConstructionOwner,
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> SemanticTask<'p, 'a, C> {
    /// Advance the task as far as it can go: `Ready` with its root's
    /// delivery (`Err` when a stop abandoned it), or `Pending` once it
    /// waits on a producer outside it that will wake `waker`. Cancelling
    /// the request wakes nothing: the stop is seen at the next poll, so the
    /// executor re-polls or drops a pending task its request cancelled
    /// ([`crate::semantic_execution::SemanticExecution::poll`]).
    pub(super) fn poll(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<Result<QueryDelivery, ()>> {
        let program = self
            .program
            .as_mut()
            .expect("a task holds its program until it gives its state back");
        let _construction = self
            .construction
            .as_mut()
            .expect("a task holds its construction owner until it gives its state back")
            .install();
        let _task = program.graph.install_execution(&program.task);
        let _validity = self.validity.install();
        let _job = verter_execution::cancellation::JobCancellationGuard::replace(
            self.job_cancellation.clone(),
        );
        let _spans = self.accounting.spans.install();
        let _spend = self.accounting.spend.install();
        let _refusals = self.accounting.refusals.install();
        let _completeness = self.accounting.completeness.install();
        self.execution.poll(program, self.root.take(), waker)
    }

    /// Poll the task to completion on this thread.
    pub(super) fn drive(&mut self) -> Result<QueryDelivery, ()> {
        crate::semantic_execution::block_on(|waker| self.poll(waker))
    }

    /// [`Self::poll`], reporting what the root answered.
    #[cfg(any(test, feature = "test-support"))]
    pub fn poll_for_tests(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<TaskOutcomeForTests> {
        self.poll(waker).map(|outcome| match outcome {
            Ok(delivery) => TaskOutcomeForTests::Answered(delivery.read.value),
            Err(()) => TaskOutcomeForTests::Stopped,
        })
    }

    /// The frames the task holds suspended.
    #[cfg(any(test, feature = "test-support"))]
    pub fn live_frames_for_tests(&self) -> usize {
        self.execution.live_frames()
    }

    /// The free-type-variable summaries the task's dispatcher holds — the
    /// request-scoped summary population it inherited from its lender
    /// ([`ProjectSemanticDispatch::free_type_variable_summaries_for_tests`])
    /// plus whatever its own drives have composed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn free_type_variable_summaries_for_tests(&self) -> usize {
        self.program.as_ref().map_or(0, |program| {
            program.dispatch.free_type_variable_summaries_for_tests()
        })
    }

    /// From now on, have another task produce each demand a call's walk
    /// suspends on before this task claims it, so the walk waits on it with
    /// its collector held, until [`Self::release_held_call_needs_for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_call_needs_for_tests(&mut self) {
        if let Some(program) = self.program.as_mut() {
            program.held_call_needs.get_or_insert_with(Vec::new);
        }
    }

    /// Let the other task's producers of the held demands go, without a
    /// result: the walk's waits wake, and the task produces them itself.
    #[cfg(any(test, feature = "test-support"))]
    pub fn release_held_call_needs_for_tests(&mut self) -> usize {
        self.program
            .as_mut()
            .and_then(|program| program.held_call_needs.take())
            .map_or(0, |held| held.len())
    }
}

/// What a semantic task's root answered.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub enum TaskOutcomeForTests {
    Answered(QueryResult<SemanticQueryValue>),
    /// A stop abandoned it.
    Stopped,
}

/// The request state a dispatcher holds open on behalf of evaluations in
/// progress.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRequestStateForTests {
    pub active_instantiations: usize,
    pub open_cost_scopes: usize,
    pub request_local_open: usize,
    /// Executor and relation frames open on the checker transaction.
    pub open_obligation_frames: usize,
    /// Inference sessions that may still collect: collecting on the checker
    /// transaction, or held out of it by a suspended continuation.
    pub collecting_sessions: usize,
}

#[cfg(any(test, feature = "test-support"))]
impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// [`Self::drive_query`]'s entry for `key`, handing the semantic task it
    /// lends this dispatcher's request state to `drive` in place of the
    /// blocking drive. `drive` may poll it, move it between threads while it
    /// is pending, or take and drop it; a task it leaves is returned. `None`
    /// when the entry answered without a task.
    pub fn with_semantic_task_for_tests<'s, R>(
        &'s self,
        key: SemanticQueryKey,
        drive: impl FnOnce(&mut Option<SemanticTask<'s, 'a, C>>) -> R,
    ) -> Option<R> {
        let (_connected_guard, _) = self.enter_connected_demand(false);
        let (attempt, _query_depth_guard) = self.enter_query(&key, None, Nesting::Drive).ok()?;
        let mut lent = LentTask::new(self, self.lend_semantic_task(key, attempt));
        Some(drive(&mut lent.task))
    }

    /// The request state this dispatcher holds open now.
    pub fn open_request_state_for_tests(&self) -> OpenRequestStateForTests {
        OpenRequestStateForTests {
            active_instantiations: self.instantiate_active.borrow().len(),
            open_cost_scopes: self.connected_demand.open_cost_scopes_for_tests(),
            request_local_open: self.request_local_open.borrow().len(),
            open_obligation_frames: self.dispatch_txn.borrow().reentry().depth(),
            collecting_sessions: self.dispatch_txn.borrow().collecting_sessions_for_tests(),
        }
    }

    /// How many rooted nodes this dispatcher holds a free-type-variable
    /// summary for: the request-scoped population a lent semantic task
    /// inherits with the rest of the request state and returns with it.
    pub fn free_type_variable_summaries_for_tests(&self) -> usize {
        self.free_type_variables.borrow().len()
    }
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> Drop for SemanticTask<'p, 'a, C> {
    fn drop(&mut self) {
        // A task released while pending releases its frames through the
        // program, which holds the state they entered; a task nobody
        // returns leaves the request state it borrowed for its lender.
        let Some(mut program) = self.program.take() else {
            return;
        };
        self.execution.release(&mut program);
        let QueryProgram { dispatch, .. } = program;
        *self.home.lock() = Some(UnreturnedTask {
            dispatch,
            validity: std::mem::take(&mut self.validity),
            accounting: std::mem::take(&mut self.accounting),
            construction: self
                .construction
                .take()
                .expect("an unreturned task retains its construction owner"),
        });
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Begin evaluating `key` as a semantic task that borrows this
    /// dispatcher's request state until it is returned
    /// ([`Self::return_semantic_task`]): its claim continues `attempt`, it
    /// runs as the execution task installed here (or a fresh one), and
    /// every poll runs under the request context, the scheduler job and the
    /// component-meta accounting active here.
    pub(super) fn lend_semantic_task(
        &self,
        key: SemanticQueryKey,
        attempt: ClaimAttempt,
    ) -> SemanticTask<'_, 'a, C> {
        let dispatch = self.task_dispatcher();
        self.exchange_request_state(&dispatch);
        let graph = self.graph();
        let mut execution = SemanticExecution::new();
        if let Some(context) = verter_execution::request_context::current_context() {
            execution.bind_request_context(context);
        }
        let mut program = QueryProgram {
            dispatch,
            graph,
            task: graph.execution_task(),
            retries: FxHashMap::default(),
            child_depth: 1,
            #[cfg(any(test, feature = "test-support"))]
            held_call_needs: None,
        };
        program.retries.insert(key.clone(), attempt);
        SemanticTask {
            execution,
            program: Some(program),
            construction: Some(
                crate::semantic_query_memo::node_roots::TaskConstructionOwner::new(
                    self.binding.construction.arena(),
                ),
            ),
            validity: crate::request_context::TaskExecutionValidity::inherit(),
            job_cancellation: verter_execution::cancellation::current_job_cancellation_token(),
            accounting: TaskAccounting::lend(),
            home: Arc::new(parking_lot::Mutex::new(None)),
            root: Some(key),
        }
    }

    /// Take back the request state `task` borrowed, with the abort its
    /// polls observed, the accounting it recorded, the nodes it constructed
    /// (now rooted by the construction owner open here) and the frames it
    /// still holds, which it releases first.
    pub(super) fn return_semantic_task<'p>(&self, mut task: SemanticTask<'p, 'a, C>)
    where
        'a: 'p,
    {
        let mut program = task.program.take().expect("a task is returned once");
        task.execution.release(&mut program);
        self.exchange_request_state(&program.dispatch);
        std::mem::take(&mut task.accounting).return_to_lender();
        task.validity.bubble();
        drop(task.construction.take());
    }

    /// Take back the request state a task this dispatcher lent left in
    /// `home` when it was dropped without being returned.
    fn reclaim_unreturned_task(&self, home: &parking_lot::Mutex<Option<UnreturnedTask<'a, C>>>) {
        if let Some(unreturned) = home.lock().take() {
            self.exchange_request_state(&unreturned.dispatch);
            unreturned.accounting.return_to_lender();
            unreturned.validity.bubble();
            // Only the lender's thread may absorb these roots. Keep them
            // until its request state has been restored, just as on return.
            drop(unreturned.construction);
        }
    }
}

/// A semantic task lent the request state of the dispatcher evaluating it on
/// this thread, which takes the state back when the task is done — or when
/// the drive unwinds, or from where the task left it when it was dropped.
struct LentTask<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> {
    lender: &'p ProjectSemanticDispatch<'a, C>,
    task: Option<SemanticTask<'p, 'a, C>>,
    home: Arc<parking_lot::Mutex<Option<UnreturnedTask<'a, C>>>>,
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> LentTask<'p, 'a, C> {
    fn new(lender: &'p ProjectSemanticDispatch<'a, C>, task: SemanticTask<'p, 'a, C>) -> Self {
        Self {
            lender,
            home: Arc::clone(&task.home),
            task: Some(task),
        }
    }
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> Drop for LentTask<'p, 'a, C> {
    fn drop(&mut self) {
        match self.task.take() {
            Some(task) => self.lender.return_semantic_task(task),
            None => self.lender.reclaim_unreturned_task(&self.home),
        }
    }
}

impl<'p, 'a, C: crate::resolver_core::ResolverCapabilities> QueryProgram<'p, 'a, C> {
    /// Claim `key` as another task's producer, when the test holds the
    /// demands a call's walk suspends on.
    #[cfg(any(test, feature = "test-support"))]
    fn hold_call_need_for_tests(&mut self, key: &SemanticQueryKey) {
        let Some(held) = self.held_call_needs.as_mut() else {
            return;
        };
        let dispatch = &self.dispatch;
        let other = self.graph.task_registry_for_tests().register_task();
        let mut capture = ReadCapture::default();
        let Ok(mut attempt) = self.graph.begin_query_claim(
            dispatch.ctx,
            dispatch.snapshot.flags(),
            key.clone(),
            &mut capture,
        ) else {
            return;
        };
        if let Claim::Produce(lease) = self.graph.claim_query(
            dispatch.ctx,
            dispatch.snapshot.flags(),
            &mut attempt,
            &other,
            &mut capture,
        ) {
            held.push((other, lease));
        }
    }

    /// The frame that produces `key` one level below the needing frame.
    fn producing_frame(
        &self,
        key: &SemanticQueryKey,
        producer: FrameProducer<'p>,
    ) -> QueryFrame<'p, 'a, C> {
        QueryFrame::Instantiate(Box::new(InstantiateFrame {
            key: key.clone(),
            producer: Some(producer),
            tracer: None,
            taint: BuildLocalTaint {
                produces: super::instantiation_frame_identity(key),
                ..BuildLocalTaint::default()
            },
            build: None,
            active: None,
            depth: self.child_depth,
            recording: false,
            computed_at: self.dispatch.workspace_edit(),
        }))
    }
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> Program
    for QueryProgram<'p, 'a, C>
{
    type Demand = SemanticQueryKey;
    type Frame = QueryFrame<'p, 'a, C>;
    type Value = QueryDelivery;
    type Failure = ();
    type Role = ();
    type Subscription = PendingClaim<'p>;

    fn start(&mut self, key: &SemanticQueryKey) -> Start<Self> {
        let dispatch = &self.dispatch;
        // The needed key's own frame runs one level below the needing one.
        let nesting = Nesting::Frame {
            depth: self.child_depth,
        };
        let mut attempt = match self.retries.remove(key) {
            Some(attempt) => attempt,
            None => match dispatch.enter_query_frame(key, self.child_depth) {
                Ok(attempt) => attempt,
                Err(delivery) => return Start::Answer(Ok(*delivery)),
            },
        };
        // A result this dispatcher published answers before any claim: a
        // claim installs a flight other tasks may join, so it is taken only
        // to produce.
        if let Some((read, carrier)) = dispatch.own_published_read(key, nesting) {
            return Start::Answer(Ok(QueryDelivery {
                read: dispatch.attribute_query_read(
                    key,
                    false,
                    read,
                    &CarrierNormalizationPrelude::none(),
                ),
                carrier: Some(carrier),
            }));
        }
        // A cheap intermediate nested in the drive completes for this
        // request alone, as it does nested in a synchronous build: its
        // frame produces it without the family memo's claim.
        if dispatch.request_local_is_open(key) {
            return Start::Answer(Ok(dispatch.recursion_delivery(key, Recursion::SamePath)));
        }
        if dispatch.completes_nested_request_locally(key) {
            dispatch.request_local_open.borrow_mut().insert(key.clone());
            return Start::Produce(
                self.producing_frame(key, FrameProducer::RequestLocal { attempt }),
            );
        }
        let (claim, carrier) = loop {
            let mut carrier = None;
            let claim = {
                let mut capture = ReadCapture::deferring_carrier(&mut carrier);
                self.graph.claim_query(
                    dispatch.ctx,
                    dispatch.snapshot.flags(),
                    &mut attempt,
                    &self.task,
                    &mut capture,
                )
            };
            // A warm result this demand cannot pay for is computed instead.
            if let Claim::Read(CacheRead {
                receipt: ReadReceipt::Priced(receipt),
                ..
            }) = &claim
            {
                if !attempt.is_recomputing()
                    && matches!(
                        dispatch.admit_served(receipt, nesting),
                        super::ServedAdmission::Recompute(_)
                    )
                {
                    attempt.recompute();
                    continue;
                }
                // Admitted (or tripped): `answered` records it once more,
                // which charges nothing further.
            }
            break (claim, carrier);
        };
        match claim {
            Claim::Read(read) => Start::Answer(Ok(dispatch.answered(key, read, carrier, nesting))),
            Claim::Recursive(recursion) => {
                Start::Answer(Ok(dispatch.recursion_delivery(key, recursion)))
            }
            Claim::Subscribed(subscription) => Start::Subscribe(PendingClaim {
                key: key.clone(),
                subscription,
                attempt: Some(attempt),
                nesting,
            }),
            Claim::Produce(lease) => {
                Start::Produce(self.producing_frame(key, FrameProducer::Lease(lease)))
            }
        }
    }

    fn step(&mut self, frame: &mut Self::Frame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        match frame {
            QueryFrame::Instantiate(frame) => {
                self.child_depth = frame.depth + 1;
                let step = self.dispatch.step_instantiate_frame(frame, delivery);
                // A decision's need entered already: its claim continues
                // when the runtime starts it.
                if let EvalStep::Need { demand, .. } = &step {
                    let claim = match frame.build.as_mut() {
                        Some(FrameBuild::Conditional(build)) => build.claim.take(),
                        Some(FrameBuild::Call(build)) => {
                            #[cfg(any(test, feature = "test-support"))]
                            self.hold_call_need_for_tests(demand);
                            build.claim.take()
                        }
                        Some(FrameBuild::Flow(build)) => {
                            // Held only while a contextual call's route waits
                            // on it, collector included.
                            #[cfg(any(test, feature = "test-support"))]
                            if !build.routes.is_empty() {
                                self.hold_call_need_for_tests(demand);
                            }
                            build.claim.take()
                        }
                        _ => None,
                    };
                    if let Some(claim) = claim {
                        self.retries.insert(demand.clone(), claim);
                    }
                }
                step
            }
        }
    }

    fn close_cycle(&mut self, key: &SemanticQueryKey, _role: ()) -> Outcome<Self> {
        // The open demand's producer is this task's own: the memo answers
        // the claim exactly as it answers a synchronous re-entry — a warm
        // result where one validates, else the same-path carrier.
        let dispatch = &self.dispatch;
        let nesting = Nesting::Frame {
            depth: self.child_depth,
        };
        let mut carrier = None;
        let mut capture = ReadCapture::deferring_carrier(&mut carrier);
        let graph = self.graph;
        let claim = match graph.begin_query_claim(
            dispatch.ctx,
            dispatch.snapshot.flags(),
            key.clone(),
            &mut capture,
        ) {
            Err(read) => return Ok(dispatch.answered(key, read, carrier, nesting)),
            Ok(mut attempt) => graph.claim_query(
                dispatch.ctx,
                dispatch.snapshot.flags(),
                &mut attempt,
                &self.task,
                &mut capture,
            ),
        };
        Ok(match claim {
            Claim::Read(read) => dispatch.answered(key, read, carrier, nesting),
            Claim::Recursive(recursion) => dispatch.recursion_delivery(key, recursion),
            Claim::Subscribed(_) | Claim::Produce(_) => {
                dispatch.recursion_delivery(key, Recursion::SamePath)
            }
        })
    }

    fn reusable(&self, _key: &SemanticQueryKey, delivery: &QueryDelivery) -> bool {
        // Exactly what the memo would serve warm: a complete, cacheable
        // result. A partial or non-cacheable one is evaluated again, as a
        // synchronous re-entry would evaluate it.
        !delivery.read.cache_suppress && !delivery.read.result_is_partial()
    }

    fn poll_subscription(
        &mut self,
        pending: &mut PendingClaim<'p>,
        waker: &std::task::Waker,
    ) -> std::task::Poll<External<Self>> {
        let dispatch = &self.dispatch;
        let mut carrier = None;
        let joined = {
            let mut capture = ReadCapture::deferring_carrier(&mut carrier);
            match pending.subscription.poll(
                dispatch.ctx,
                dispatch.snapshot.flags(),
                pending
                    .attempt
                    .as_mut()
                    .expect("a pending subscription holds its claim"),
                &mut capture,
                waker,
            ) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(joined) => joined,
            }
        };
        let key = pending.key.clone();
        let nesting = pending.nesting;
        // The runtime drops a resolved subscription; its claim continues
        // in `retries` when the demand starts again.
        let mut attempt = pending
            .attempt
            .take()
            .expect("a pending subscription holds its claim");
        // A joined result this demand cannot pay for is computed instead.
        if let Joined::Read(CacheRead {
            receipt: ReadReceipt::Priced(receipt),
            ..
        }) = &joined
        {
            if !attempt.is_recomputing()
                && matches!(
                    dispatch.admit_served(receipt, nesting),
                    super::ServedAdmission::Recompute(_)
                )
            {
                attempt.recompute();
                self.retries.insert(key, attempt);
                return std::task::Poll::Ready(External::Restart);
            }
        }
        std::task::Poll::Ready(match joined {
            Joined::Read(read) => {
                External::Complete(Ok(dispatch.answered(&key, read, carrier, nesting)))
            }
            Joined::Recursive(recursion) => {
                External::Complete(Ok(dispatch.recursion_delivery(&key, recursion)))
            }
            Joined::Retry => {
                self.retries.insert(key, attempt);
                External::Restart
            }
        })
    }

    fn stop(&self) -> Option<()> {
        self.dispatch.cancellation.is_cancelled().then_some(())
    }

    fn abandon(&mut self, frame: QueryFrame<'p, 'a, C>) {
        let QueryFrame::Instantiate(mut frame) = frame;
        // A call stopped between its steps releases the collector its walk
        // held and closes the executor frame it opened.
        if let Some(FrameBuild::Call(build)) = frame.build.as_mut() {
            if let Some(idx) = build.frame.take() {
                self.dispatch
                    .abandon_resolve_call_build(idx, build.suspension.take());
            }
        }
        // A flow return stopped between its runs releases the contextual
        // calls it held, each collector abandoned, and aborts the members
        // its runs completed, which no publication of it will drain.
        if let Some(FrameBuild::Flow(build)) = frame.build.as_mut() {
            self.dispatch
                .release_held_contextual_routes(std::mem::take(&mut build.routes));
            self.dispatch.relation_abort_completed_members();
        }
        // A build stopped between its steps leaves the declaration it
        // entered; its producer lease aborts its flight as it drops.
        if let Some((identity, args)) = frame.active.take() {
            self.dispatch.leave_instantiate_active(&identity, &args);
        }
        // A frame stopped before it completed leaves no receipt; what it
        // charged is its consumer's.
        if std::mem::take(&mut frame.recording) {
            self.dispatch.connected_demand.abandon_cost_scope();
        }
        // A request-local completion it held open closes, so a later read
        // of the key is not taken for a same-path re-entry.
        if let Some(FrameProducer::RequestLocal { .. }) = frame.producer.take() {
            self.dispatch
                .request_local_open
                .borrow_mut()
                .remove(&frame.key);
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether a demand of `key` a frame needs completes for this request
    /// alone, outside the family memo's claim and admission: exactly when a
    /// read of it nested in a synchronous build does
    /// ([`super::completes_request_locally`]) — outside every checker
    /// obligation and every forced operand, and not already produced on this
    /// task's path.
    fn completes_nested_request_locally(&self, key: &SemanticQueryKey) -> bool {
        super::completes_request_locally(key)
            && self.active_operand_evidence.borrow().is_empty()
            && self.dispatch_txn.borrow().obligations.decides_root()
            && !self.graph().is_same_path_claim(key)
    }

    /// Whether `key` runs on the continuation runtime when it is evaluated
    /// outside any drive: an instantiation or a `typeof` read for its value
    /// alone. A `typeof` whose value's type lowers from its authored value
    /// position (its annotation, its inferred initializer, its const-object
    /// shape) builds as a frame, so a chain of values each typed by the one
    /// before is a chain of frames; any other `typeof` completes in the
    /// frame's first step.
    pub(super) fn drives_on_the_runtime(&self, key: &SemanticQueryKey) -> bool {
        let family = match key {
            SemanticQueryKey::Instantiate(_) | SemanticQueryKey::TypeOf { .. } => true,
            // A function's return evaluated as a machinery root, outside
            // every open obligation, whose body types callbacks under a held
            // collector. Any other evaluates synchronously, its reads each
            // driven where it makes them.
            SemanticQueryKey::FlowReturn(flow) => {
                self.dispatch_txn.borrow().obligations.decides_root()
                    && self.flow_body_types_callbacks(flow)
            }
            _ => false,
        };
        family
            && !crate::semantic_execution::drive_running()
            && self.active_operand_evidence.borrow().is_empty()
    }

    /// Evaluate `key` on the continuation runtime: the synchronous entry
    /// of an instantiation or a `typeof`. A warm result answers without the
    /// runtime.
    pub(super) fn drive_query(&self, key: SemanticQueryKey) -> ValueRead {
        // The connected demand spans the whole drive; the entry itself is
        // charged exactly as a synchronous entry is.
        let (connected_guard, _) = self.enter_connected_demand(false);
        // An isolated root whose refusal is sealed answers from it.
        let isolated_root = self.isolated_root_identity(&connected_guard, &key);
        if let Some(read) = isolated_root
            .as_ref()
            .and_then(|root| self.answer_from_sealed_refusal(root))
        {
            return read;
        }
        // The drive runs on this native stack: its entry is one nested
        // query level, as a synchronous entry is.
        let (attempt, _query_depth_guard) = match self.enter_query(&key, None, Nesting::Drive) {
            Ok(entered) => entered,
            Err(read) => return self.finish_driven_read(&key, read, None, &connected_guard, None),
        };
        // The drive runs as a semantic task that borrows this dispatcher's
        // request state and gives it back when it is done.
        let outcome = {
            let mut lent = LentTask::new(self, self.lend_semantic_task(key.clone(), attempt));
            lent.task.as_mut().expect("the task is lent").drive()
        };
        match outcome {
            Ok(QueryDelivery { read, carrier }) => {
                if let Some(receipt) = read.receipt.priced() {
                    self.connected_demand
                        .record_prerequisite(receipt, Nesting::Drive);
                }
                let traced = self.driven_root_traced.borrow_mut().take();
                self.finish_driven_read(
                    &key,
                    read,
                    carrier,
                    &connected_guard,
                    isolated_root.zip(traced),
                )
            }
            Err(()) => self.finish_driven_read(
                &key,
                crate::semantic_query_memo::cancelled_cache_read(),
                None,
                &connected_guard,
                None,
            ),
        }
    }

    /// The synchronous entry's tail: replay the answering result's carrier
    /// into the tracers active here, and fold the read's rails into the
    /// enclosing build, as every synchronous read does.
    fn finish_driven_read(
        &self,
        key: &SemanticQueryKey,
        mut read: ValueRead,
        carrier: Option<ReadSetSignature>,
        connected_guard: &super::connected_demand::ConnectedDemandGuard<'_>,
        refusal: Option<(super::IsolatedRoot, super::TracedFacts)>,
    ) -> ValueRead {
        if let Some(carrier) = carrier {
            carrier.bubble_via_tls();
        }
        if connected_guard.is_root() {
            if let Some(reasons) = self.connected_demand_trip() {
                self.append_connected_limit_diagnostics(key, reasons, &mut read);
            }
        }
        if let Some((identity, traced)) = refusal {
            self.seal_root_refusal(identity, &read, traced);
        }
        self.fold_cache_read_rails(
            read.result_is_partial(),
            read.cache_suppress,
            read.partial_reason_classes(),
        );
        self.deposit_operation_refusals(read.walker_diagnostics.iter());
        read
    }

    /// A need's entry: the dispatch it replaces is counted, charged and
    /// looked up exactly as a synchronous entry would be. `Err` answers it.
    fn enter_query_frame(
        &self,
        key: &SemanticQueryKey,
        depth: u32,
    ) -> Result<ClaimAttempt, Box<QueryDelivery>> {
        self.record_dispatch_intent_counters(key);
        #[cfg(any(test, feature = "test-support"))]
        super::raise::DISPATCH_TRACE.with(|t| {
            t.borrow_mut()
                .push(super::raise::query_key_discriminant(key))
        });
        #[cfg(any(test, feature = "test-support"))]
        super::raise::record_dispatch_key(key);
        let mut carrier = None;
        // The runtime holds this entry on the heap: it enters no nested
        // native query level.
        let entered = self.enter_query(key, Some(&mut carrier), Nesting::Frame { depth });
        entered
            .map(|(attempt, _)| attempt)
            .map_err(|read| Box::new(QueryDelivery { read, carrier }))
    }

    /// Count, charge and look up one query entry, exactly as the
    /// synchronous entry does. `carrier`, when given, receives a warm
    /// result's carrier instead of the tracers active now; `nesting` is how
    /// the entry is reached — a drive's root enters one nested native query
    /// level, held by the returned guard. `Err` is the read that answers
    /// the entry.
    fn enter_query(
        &self,
        key: &SemanticQueryKey,
        carrier: Option<&mut Option<ReadSetSignature>>,
        nesting: Nesting,
    ) -> Result<
        (
            ClaimAttempt,
            Option<super::connected_demand::ConnectedDemandGuard<'_>>,
        ),
        ValueRead,
    > {
        crate::request_context::with_current_request_context(|ctx| {
            ctx.record_dispatched_query_tag(key.tag())
        });
        let (_connected_guard, preexisting_trip) = self.enter_connected_demand(false);
        let query_depth_guard =
            self.charge_query_entry(key, preexisting_trip, nesting.query_levels() > 0)?;
        let mut capture = match carrier {
            Some(slot) => ReadCapture::deferring_carrier(slot),
            None => ReadCapture::default(),
        };
        let attempt = match self.graph().begin_query_claim(
            self.ctx,
            self.snapshot.flags(),
            key.clone(),
            &mut capture,
        ) {
            Ok(attempt) => attempt,
            Err(read) => {
                match self.served_read(key, read, nesting) {
                    Ok(read) => {
                        return Err(self.attribute_query_read(
                            key,
                            false,
                            read,
                            &CarrierNormalizationPrelude::none(),
                        ))
                    }
                    // A warm result this demand cannot pay for is computed.
                    Err(_) => self
                        .graph()
                        .begin_query_recompute(self.snapshot.flags(), key.clone())?,
                }
            }
        };
        Ok((attempt, query_depth_guard))
    }

    /// A result another producer, or the memo, answered with: admitted by
    /// replaying its receipt into the needing frame's demand. A result the
    /// demand cannot pay for even after computing it is the demand's trip.
    fn answered(
        &self,
        key: &SemanticQueryKey,
        read: ValueRead,
        carrier: Option<ReadSetSignature>,
        nesting: Nesting,
    ) -> QueryDelivery {
        let read = match self.served_read(key, read, nesting) {
            Ok(read) => read,
            Err(refusal) => self.refused_replay_read(key, refusal),
        };
        QueryDelivery {
            read: self.attribute_query_read(key, false, read, &CarrierNormalizationPrelude::none()),
            carrier,
        }
    }

    /// The recursion carrier answering `key`.
    fn recursion_delivery(&self, key: &SemanticQueryKey, recursion: Recursion) -> QueryDelivery {
        if recursion == Recursion::WaitCycle {
            self.connected_demand.note_schedule_cut();
        }
        let read = self
            .graph()
            .recursion_read(recursion, self.query_sentinel(key));
        QueryDelivery {
            read: self.attribute_query_read(key, false, read, &CarrierNormalizationPrelude::none()),
            carrier: None,
        }
    }

    /// The node a same-path re-entry of `key` answers with: an
    /// instantiation's recursive reference, else a miss.
    pub(super) fn query_sentinel(&self, key: &SemanticQueryKey) -> SemanticNodeId {
        if let SemanticQueryKey::Instantiate(k) = key {
            return self
                .graph()
                .intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                    name: k.base().merged_symbol_name.clone(),
                    args: Arc::clone(k.args()).into(),
                }));
        }
        self.graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::Miss))
    }

    /// Consume a delivery in the consuming frame's step: replay its
    /// carrier into the frame's tracer, fold its rails into the frame's
    /// taint, and read the instantiated node as a synchronous
    /// instantiation's reader does.
    /// [`Self::consume_delivery`] for a delivery a build consumes itself.
    pub(super) fn consume_frame_delivery(
        &self,
        delivery: FrameDelivery,
    ) -> super::locator_view_worklist::ProjectionDelivery {
        self.consume_delivery(delivery.delivery, delivery.depth)
    }

    /// The read a delivery a build consumes itself answers it with
    /// ([`Self::read_delivery`]).
    pub(super) fn read_frame_delivery(&self, delivery: FrameDelivery) -> ValueRead {
        self.read_delivery(
            delivery.delivery,
            Nesting::Frame {
                depth: delivery.depth,
            },
        )
    }

    /// The node a delivery answers its consumer with, and whether it
    /// answered a value: a read of no value delivers the shared miss.
    fn consume_delivery(
        &self,
        delivery: QueryDelivery,
        depth: u32,
    ) -> super::locator_view_worklist::ProjectionDelivery {
        let read = self.read_delivery(delivery, Nesting::Frame { depth });
        match read.value {
            QueryResult::Value(SemanticQueryValue::TypeNode(node)) => {
                super::locator_view_worklist::ProjectionDelivery {
                    node,
                    answered: Some(read.fact_status),
                }
            }
            _ => super::locator_view_worklist::ProjectionDelivery {
                node: self.opaque(QueryError::Miss),
                answered: None,
            },
        }
    }

    /// The read a delivery answers its consumer with: its carrier replayed
    /// into the tracers active now, its receipt recorded as the consumer's
    /// prerequisite read the way `nesting` names, its rails folded into the
    /// consumer's taint — what the read's own entry leaves on its reader.
    fn read_delivery(&self, delivery: QueryDelivery, nesting: Nesting) -> ValueRead {
        let QueryDelivery { read, carrier } = delivery;
        if let Some(carrier) = carrier {
            carrier.bubble_via_tls();
        }
        if let Some(receipt) = read.receipt.priced() {
            self.connected_demand.record_prerequisite(receipt, nesting);
        }
        self.fold_cache_read_rails(
            read.result_is_partial(),
            read.cache_suppress,
            read.partial_reason_classes(),
        );
        self.deposit_operation_refusals(read.walker_diagnostics.iter());
        read
    }

    /// Carry a conditional decision's frame on: record a delivery as the
    /// answer to the read it waited on, then run the decision with every
    /// answer the frame holds. A read none of them answers is the frame's
    /// next need, and the run that reached it is dropped with what it
    /// observed; a run every read of which was answered is the decision.
    fn drain_conditional(
        &self,
        key: &SemanticQueryKey,
        build: &mut ConditionalBuild,
        delivery: Option<QueryDelivery>,
        depth: u32,
    ) -> InstantiatePoll {
        if let Some(delivery) = delivery {
            let answered = build
                .awaiting
                .take()
                .expect("a delivery answers the read its decision waited on");
            build.answers.insert(answered, delivery);
        }
        // The last run reached more reads without an answer: each is needed
        // before the decision runs again.
        if let Some((need, claim)) = build.pending.pop_front() {
            build.awaiting = Some(need.clone());
            build.claim = Some(claim);
            return InstantiatePoll::Need(need);
        }
        let SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } = key
        else {
            unreachable!("a decision frame produces a conditional")
        };
        let installed = self.decision_seam.replace(Some(DecisionSeam {
            answers: std::mem::take(&mut build.answers),
            needs: Vec::new(),
            depth,
            nested: 0,
        }));
        verter_debug_assert!(installed.is_none(), "one decision runs per step");
        let run = self.observe_read(|| {
            self.build_conditional(
                *check,
                *extends,
                *true_branch,
                *false_branch,
                *distributive,
                pending.clone(),
            )
        });
        let seam = self
            .decision_seam
            .take()
            .expect("the decision's seam stays installed for its run");
        build.answers = seam.answers;
        let mut needs = seam.needs.into_iter();
        let output = match needs.next() {
            Some((need, claim)) => match run.discard() {
                // An aborted run is the decision's answer: its abort is
                // observed, never discarded.
                Err(abort) => self.aborted_build_output(abort),
                Ok(_) => {
                    build.awaiting = Some(need.clone());
                    build.claim = Some(claim);
                    build.pending = needs.collect();
                    return InstantiatePoll::Need(need);
                }
            },
            None => run
                .adopt(self)
                .unwrap_or_else(|abort| self.aborted_build_output(abort)),
        };
        InstantiatePoll::Done(Box::new(output))
    }

    fn aborted_build_output(
        &self,
        abort: crate::semantic_query::ExecutionAbort,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        (
            QueryResult::Error(QueryError::ExecutionAborted(abort)),
            self.project_generation_signature(),
        )
            .into()
    }

    /// Answer a read at the running decision's seam. An instantiation or a
    /// `typeof` the decision's frame holds the answer to reads it. Any other
    /// one the decision reads itself enters as the frame's need would: a
    /// result the memo serves warm answers it at once, and joins the
    /// frame's answers; one that must be computed becomes one of the frame's
    /// needs, its claim kept for the runtime to produce it, and reads as a
    /// typed gap nothing built from it keeps. A query the decision evaluates in place reads only the frame's
    /// answers here and evaluates anything else as it would anywhere.
    /// `None` when no decision runs here, or for any other query.
    pub(super) fn read_at_decision_seam(&self, key: &SemanticQueryKey) -> Option<ValueRead> {
        if !matches!(
            key,
            SemanticQueryKey::Instantiate(_) | SemanticQueryKey::TypeOf { .. }
        ) {
            return None;
        }
        // The answer, or what the read does without one. An answer was
        // computed in a frame of its own, the needed demand's, so it reads
        // as that frame's result wherever the decision's run reads it.
        enum Seam {
            Answered(QueryDelivery),
            Waiting,
            Enter,
        }
        let (seam, depth) = {
            let installed = self.decision_seam.borrow();
            let seam = installed.as_ref()?;
            let answered = seam.answers.get(key).cloned();
            let read = match answered {
                Some(delivery) => Seam::Answered(delivery),
                None if seam.nested > 0 => return None,
                None if seam.needs.iter().any(|(needed, _)| needed == key) => Seam::Waiting,
                None => Seam::Enter,
            };
            (read, seam.depth)
        };
        let nesting = Nesting::Frame { depth };
        Some(match seam {
            Seam::Answered(delivery) => self.read_delivery(delivery, nesting),
            Seam::Waiting => self.suspended_decision_read(),
            Seam::Enter => match self.enter_query_frame(key, depth) {
                Err(delivery) => {
                    self.with_decision_seam(|seam| {
                        seam.answers.insert(key.clone(), (*delivery).clone());
                    });
                    self.read_delivery(*delivery, nesting)
                }
                Ok(claim) => {
                    self.with_decision_seam(|seam| seam.needs.push((key.clone(), claim)));
                    self.suspended_decision_read()
                }
            },
        })
    }

    fn with_decision_seam(&self, update: impl FnOnce(&mut DecisionSeam)) {
        update(
            self.decision_seam
                .borrow_mut()
                .as_mut()
                .expect("the decision's seam stays installed for its run"),
        );
    }

    /// The gap a read the running decision waits on reads as.
    fn suspended_decision_read(&self) -> ValueRead {
        let reasons = crate::semantic_query::PartialReasonSet::PROPAGATED;
        self.fold_local_partial_completeness(reasons);
        CacheRead {
            lease: Default::default(),
            value: QueryResult::Error(QueryError::IncompleteSemanticOperand { reasons }),
            dep_signature: super::empty_signature(),
            walker_diagnostics: crate::semantic_query_memo::empty_diagnostics(),
            cache_suppress: true,
            fact_status: crate::semantic_query::FactStatus::unavailable(reasons),
            receipt: ReadReceipt::Unpriced,
        }
    }

    /// Whether the running decision waits on a read: what remains of its
    /// run is dropped, so it stops where it can.
    pub(super) fn decision_waits(&self) -> bool {
        self.decision_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| !seam.needs.is_empty())
    }

    /// Mark a query the running decision reached as evaluating in place:
    /// until it returns, reads the decision's answers do not hold evaluate
    /// as they would anywhere.
    pub(super) fn nested_decision_query(&self) -> NestedDecisionQuery<'_> {
        let mut seam = self.decision_seam.borrow_mut();
        let mut flow = self.flow_seam.borrow_mut();
        NestedDecisionQuery {
            seam: seam.as_mut().map(|seam| {
                seam.nested += 1;
                &self.decision_seam
            }),
            flow: flow.as_mut().map(|seam| {
                seam.nested += 1;
                &self.flow_seam
            }),
        }
    }

    /// Whether a conditional decision runs here as its frame's own run, not
    /// inside a query it evaluates in place.
    pub(super) fn deciding_on_the_runtime(&self) -> bool {
        self.decision_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| seam.nested == 0)
    }

    /// One step of an instantiation's frame: its taint frame and tracer
    /// installed, it begins or resumes its build and runs it until the
    /// build needs another instantiation or completes.
    fn step_instantiate_frame<'p>(
        &self,
        frame: &mut InstantiateFrame<'p, 'a, C>,
        delivery: Option<Outcome<QueryProgram<'p, 'a, C>>>,
    ) -> EvalStep<QueryProgram<'p, 'a, C>>
    where
        'a: 'p,
    {
        if !frame.recording {
            let identity = frame
                .producer
                .as_ref()
                .expect("a stepping frame owns its producer")
                .cost_identity();
            self.connected_demand.open_cost_scope(identity);
            frame.recording = true;
        }
        let tracer = frame.tracer.get_or_insert_with(|| {
            crate::fact_signature_helpers::StepwiseFactTracer::new(
                crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx),
            )
        });
        let taint = super::StepTaint::install(&self.build_local_taint, &mut frame.taint);
        let installed = tracer.install();
        let delivery = delivery.map(|outcome| match outcome {
            Ok(delivery) => delivery,
            Err(()) => unreachable!("a stopped drive delivers nothing"),
        });
        let poll: FramePoll = match frame.build.as_mut() {
            Some(FrameBuild::Instantiate(build)) => self
                .drain_instantiate(
                    build,
                    delivery.map(|delivery| self.consume_delivery(delivery, frame.depth + 1)),
                )
                .into(),
            // A typeof build consumes its delivery where its evaluation
            // reads it.
            Some(FrameBuild::Typeof(build)) => self
                .drain_typeof(
                    build,
                    delivery.map(|delivery| FrameDelivery {
                        delivery,
                        depth: frame.depth + 1,
                    }),
                )
                .into(),
            // A decision keeps its deliveries as answers to its reads.
            Some(FrameBuild::Conditional(build)) => self
                .drain_conditional(&frame.key, build, delivery, frame.depth + 1)
                .into(),
            // So does a call's walk, for its operands.
            Some(FrameBuild::Call(build)) => {
                self.drain_call(&frame.key, build, delivery, frame.depth + 1)
            }
            // A flow return keeps its deliveries as answers to the reads its
            // contextual callbacks made.
            Some(FrameBuild::Flow(build)) => {
                self.drain_flow(&frame.key, build, delivery, frame.depth + 1)
            }
            None => {
                self.open_cold_build(None);
                // A frame produces its key as a synchronous build does.
                #[cfg(any(test, feature = "test-support"))]
                if let Some(hook) = self.graph().produce_hook_for_tests() {
                    hook(&frame.key, &|nested| {
                        let _ = self.execute_via_cold_build_helper(nested);
                    });
                }
                match &frame.key {
                    SemanticQueryKey::Instantiate(key) => {
                        let refusal = self
                            .cold_build_budget_refusal(&frame.key)
                            .or_else(|| self.instantiation_budget_refusal(frame.depth));
                        let begun = match refusal {
                            Some(refusal) => InstantiateStart::Done(Box::new(refusal)),
                            None => self.begin_instantiate(
                                key.base(),
                                key.args(),
                                key.source(),
                                key.context(),
                                true,
                            ),
                        };
                        match begun {
                            InstantiateStart::Done(output) => InstantiatePoll::Done(output).into(),
                            InstantiateStart::Build(build) => {
                                frame.active =
                                    Some((build.active_identity.clone(), Arc::clone(&build.args)));
                                let FrameBuild::Instantiate(build) =
                                    frame.build.insert(FrameBuild::Instantiate(build))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_instantiate(build, None).into()
                            }
                        }
                    }
                    SemanticQueryKey::TypeOf {
                        value_root,
                        path,
                        context,
                    } => {
                        let begun = match self.cold_build_budget_refusal(&frame.key) {
                            Some(refusal) => TypeofStart::Done(Box::new(refusal)),
                            None => self.begin_typeof(
                                &value_root.root,
                                path,
                                context.projection_reduction,
                            ),
                        };
                        match begun {
                            TypeofStart::Done(output) => InstantiatePoll::Done(output).into(),
                            TypeofStart::Build(build) => {
                                let FrameBuild::Typeof(build) =
                                    frame.build.insert(FrameBuild::Typeof(build))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_typeof(build, None).into()
                            }
                        }
                    }
                    SemanticQueryKey::Conditional { .. } => {
                        match self.cold_build_budget_refusal(&frame.key) {
                            Some(refusal) => InstantiatePoll::Done(Box::new(refusal)).into(),
                            None => {
                                let FrameBuild::Conditional(build) =
                                    frame.build.insert(FrameBuild::Conditional(Box::default()))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_conditional(&frame.key, build, None, frame.depth + 1)
                                    .into()
                            }
                        }
                    }
                    SemanticQueryKey::ResolveCall(_) => {
                        match self.cold_build_budget_refusal(&frame.key) {
                            Some(refusal) => InstantiatePoll::Done(Box::new(refusal)).into(),
                            None => {
                                let FrameBuild::Call(build) =
                                    frame.build.insert(FrameBuild::Call(Box::default()))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_call(&frame.key, build, None, frame.depth + 1)
                            }
                        }
                    }
                    SemanticQueryKey::FlowReturn(_) => {
                        let FrameBuild::Flow(build) =
                            frame.build.insert(FrameBuild::Flow(Box::default()))
                        else {
                            unreachable!("the build was just inserted")
                        };
                        self.drain_flow(&frame.key, build, None, frame.depth + 1)
                    }
                    _ => unreachable!(
                        "a frame produces an instantiation, a typeof, a conditional decision, \
                         a call resolution or a flow return"
                    ),
                }
            }
        };
        drop(installed);
        drop(taint);
        match poll {
            FramePoll::Need(key) => EvalStep::Need {
                demand: key,
                role: (),
            },
            FramePoll::Done(output) => {
                // Finishing the build left the declaration it entered.
                frame.active = None;
                EvalStep::Complete(self.complete_instantiate_frame(frame, *output))
            }
        }
    }

    /// An instantiation nested `depth` deep past Verter's instantiation
    /// budget: the checker's TS2589 recovery, reported at Verter's limit, as a
    /// resource partial. Whether the budget is reached depends on the chain
    /// that needed the instantiation, not on the instantiation alone, so
    /// neither the recovery nor anything evaluated from it is kept.
    fn instantiation_budget_refusal(
        &self,
        depth: u32,
    ) -> Option<crate::project_semantic_dispatch::walk::QueryBuildOutput> {
        let refusal = crate::semantic_query::checker_policy::instantiation_budget(
            self.connected_demand.instantiation_within_budget(depth),
        )
        .err()?;
        let recovery = self.recover_at_operation_budget(refusal, None);
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Value(recovery),
            self.project_generation_signature(),
        )
            .into();
        output.cache_suppress = true;
        Some(output)
    }

    /// Settle, admit and complete a finished build's producer, exactly as
    /// the synchronous entry settles a cold build: the build's taint and
    /// tracer verdict fold into its output first.
    fn complete_instantiate_frame(
        &self,
        frame: &mut InstantiateFrame<'_, 'a, C>,
        output: crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue>,
    ) -> QueryDelivery {
        let finalise = frame
            .tracer
            .take()
            .expect("a stepped frame owns its tracer")
            .finish();
        let (output, rooting) = self.close_cold_build(
            output,
            std::mem::take(&mut frame.taint),
            finalise,
            &CarrierNormalizationPrelude::none(),
        );
        let producer = frame
            .producer
            .take()
            .expect("a producing frame owns its producer");
        if frame.depth == 1 {
            // The drive's root: its entry seals a refusal on these facts.
            *self.driven_root_traced.borrow_mut() = super::TracedFacts::of_output(&output, rooting);
        }
        let request = crate::request_context::current_request_budget();
        let receipt = if std::mem::take(&mut frame.recording) {
            if output.completeness.is_partial() {
                self.connected_demand.abandon_cost_scope();
                None
            } else {
                self.connected_demand.seal_cost_scope(request.as_deref())
            }
        } else {
            None
        };
        let mut carrier = None;
        let lease = match producer {
            FrameProducer::Lease(lease) => lease,
            // A request-local completion settles as a producer's would and
            // enters no family memo; the dispatcher keeps what it may serve
            // again.
            FrameProducer::RequestLocal { attempt } => {
                let (read, kept) = self.graph().complete_request_local(
                    attempt,
                    self.snapshot.flags(),
                    output,
                    receipt,
                    &mut ReadCapture::deferring_carrier(&mut carrier),
                );
                if let Some(kept) = kept {
                    self.keep_published(&frame.key, &read, kept, frame.computed_at);
                }
                let delivery = QueryDelivery {
                    read: self.attribute_query_read(
                        &frame.key,
                        true,
                        read,
                        &CarrierNormalizationPrelude::none(),
                    ),
                    carrier,
                };
                self.request_local_open.borrow_mut().remove(&frame.key);
                return delivery;
            }
        };
        let mut publication = None;
        let read = {
            let mut capture =
                ReadCapture::deferring_carrier(&mut carrier).and_publication(&mut publication);
            match lease.settle(self.ctx, self.snapshot.flags(), output, receipt.clone()) {
                Err(read) => read,
                Ok(mut settled) => {
                    match settled.admit(self.ctx, self.snapshot.flags(), &mut capture) {
                        Err(read) => read,
                        Ok(()) => {
                            let published = settled.published_carrier();
                            let read = settled.complete(self.ctx, &mut capture);
                            if let Some(carrier) = published {
                                self.keep_published(&frame.key, &read, carrier, frame.computed_at);
                            }
                            read
                        }
                    }
                }
            }
        };
        // A call's admitted publication is its component's commit boundary:
        // the members its walk completed publish against it, or are
        // aborted with it.
        if let SemanticQueryKey::ResolveCall(call) = &frame.key {
            self.dispatch_txn.borrow_mut().call.last_root_failure = None;
            match &publication {
                Some(publication) => self.resolve_call_drain_completed_members(call, publication),
                None => self.relation_abort_completed_members(),
            }
        }
        // So is a flow return's, for the members its evaluation completed.
        if let SemanticQueryKey::FlowReturn(flow) = &frame.key {
            match &publication {
                Some(publication) => self.flow_return_drain_completed_members(flow, publication),
                None => self.relation_abort_completed_members(),
            }
        }
        QueryDelivery {
            read: self.attribute_query_read(
                &frame.key,
                true,
                read,
                &CarrierNormalizationPrelude::none(),
            ),
            carrier,
        }
    }

    /// Carry a call resolution's frame on: record a delivery as the answer
    /// to the operand demand its walk waited on, need the next one its walk
    /// reached, or run the walk — resuming it where it stopped — until it
    /// stops again or closes.
    fn drain_call(
        &self,
        key: &SemanticQueryKey,
        build: &mut CallBuild,
        delivery: Option<QueryDelivery>,
        depth: u32,
    ) -> FramePoll {
        let SemanticQueryKey::ResolveCall(call) = key else {
            unreachable!("a call frame produces a call resolution")
        };
        if let Some(delivery) = delivery {
            let answered = build
                .awaiting
                .take()
                .expect("a delivery answers the demand the walk waited on");
            build.answers.insert(answered, delivery);
        }
        if let Some((need, claim)) = build.pending.pop_front() {
            build.awaiting = Some(need.clone());
            build.claim = Some(claim);
            return FramePoll::Need(need);
        }
        let idx = match build.frame {
            Some(idx) => idx,
            None => match self.open_resolve_call_build(call) {
                Ok(idx) => *build.frame.insert(idx),
                Err(refusal) => return FramePoll::Done(refusal),
            },
        };
        let installed = self.call_seam.replace(Some(CallSeam {
            key: (**call).clone(),
            resume: build.suspension.take(),
            answers: std::mem::take(&mut build.answers),
            needs: Vec::new(),
            depth,
        }));
        verter_debug_assert!(installed.is_none(), "one call walk runs per step");
        let step = self.step_resolve_call_build(call, idx);
        let seam = self
            .call_seam
            .take()
            .expect("the walk's seam stays installed for its run");
        build.answers = seam.answers;
        match step {
            super::call_resolve::CallWalkStep::Suspended(suspension) => {
                #[cfg(any(test, feature = "test-support"))]
                CALL_SUSPENSIONS_FOR_TESTS.with(|observed| {
                    observed.borrow_mut().push(CallSuspensionForTests {
                        needs: seam.needs.iter().map(|(need, _)| need.clone()).collect(),
                        collected_candidates: suspension.collected_candidates(),
                    })
                });
                build.suspension = Some(suspension);
                let mut needs = seam.needs.into_iter();
                let (need, claim) = needs.next().expect("a suspended walk waits on a demand");
                build.awaiting = Some(need.clone());
                build.claim = Some(claim);
                build.pending = needs.collect();
                FramePoll::Need(need)
            }
            super::call_resolve::CallWalkStep::Done(output) => {
                build.frame = None;
                FramePoll::Done(output)
            }
        }
    }

    /// Whether the walk of `key` is the one its frame is stepping, which
    /// stops on its operands' demands instead of evaluating them in place.
    pub(super) fn call_walk_suspends(&self, key: &crate::semantic_query::ResolveCallKey) -> bool {
        self.call_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| &seam.key == key)
    }

    /// The walk of `key` its frame suspended, to resume where it stopped.
    pub(super) fn take_call_resume(
        &self,
        key: &crate::semantic_query::ResolveCallKey,
    ) -> Option<Box<super::call_resolve::CallSuspension>> {
        self.call_seam
            .borrow_mut()
            .as_mut()
            .filter(|seam| &seam.key == key)
            .and_then(|seam| seam.resume.take())
    }

    /// Demand what an argument relation over `operands` unwraps — as the
    /// checker instantiates an argument's and its parameter's types before
    /// relating them — at the walk's seam: an instantiation or a `typeof`
    /// the frame's answers hold, or the memo serves warm, reads at once;
    /// one that must be computed becomes one of the frame's needs. What the
    /// demand observed is the relation's to observe, so it is dropped.
    /// Whether the walk must stop for any.
    pub(super) fn call_operands_wait(&self, operands: [SemanticNodeId; 2]) -> bool {
        let (answers, depth) = {
            let mut seam = self.call_seam.borrow_mut();
            let seam = seam
                .as_mut()
                .expect("an operand demand runs inside its frame's walk");
            (std::mem::take(&mut seam.answers), seam.depth)
        };
        let outer = self.decision_seam.replace(Some(DecisionSeam {
            answers,
            needs: Vec::new(),
            depth,
            nested: 0,
        }));
        let demanded = self.observe_read(|| {
            for operand in operands {
                let _ = self.unwrap_identity_carrier_for_relation(operand);
            }
        });
        let _ = demanded.discard();
        let DecisionSeam { answers, needs, .. } = self
            .decision_seam
            .replace(outer)
            .expect("the operand demand's seam stays installed for its run");
        let waits = !needs.is_empty();
        let mut seam = self.call_seam.borrow_mut();
        let seam = seam
            .as_mut()
            .expect("an operand demand runs inside its frame's walk");
        seam.answers = answers;
        seam.needs = needs;
        waits
    }

    /// Carry a flow return's frame on: record a delivery as the answer to
    /// the read it waited on, need the next one its last run reached, or
    /// run the function's evaluation with every answer the frame holds and
    /// every contextual call it holds. A run that reached a read none of
    /// them answers is dropped with what it observed, the contextual call
    /// that read it held for the next run; a run that reached none is the
    /// build.
    fn drain_flow(
        &self,
        key: &SemanticQueryKey,
        build: &mut FlowBuild,
        delivery: Option<QueryDelivery>,
        depth: u32,
    ) -> FramePoll {
        let SemanticQueryKey::FlowReturn(flow) = key else {
            unreachable!("a flow frame produces a flow return")
        };
        if let Some(delivery) = delivery {
            let answered = build
                .awaiting
                .take()
                .expect("a delivery answers the read its run waited on");
            build.answers.insert(answered, delivery);
        }
        if let Some((need, claim)) = build.pending.pop_front() {
            build.awaiting = Some(need.clone());
            build.claim = Some(claim);
            return FramePoll::Need(need);
        }
        // The function's own evaluation runs one frame below the depth the
        // transaction has now.
        let frame_depth = self.dispatch_txn.borrow().reentry().depth() + 1;
        let installed = self.flow_seam.replace(Some(FlowSeam {
            answers: std::mem::take(&mut build.answers),
            needs: Vec::new(),
            depth,
            nested: 0,
            windows: 0,
            frame_depth,
            routes: std::mem::take(&mut build.routes),
        }));
        verter_debug_assert!(installed.is_none(), "one flow evaluation runs per step");
        let run = self.observe_read(|| self.build_flow_return(flow));
        let seam = self
            .flow_seam
            .take()
            .expect("the flow evaluation's seam stays installed for its run");
        build.answers = seam.answers;
        build.routes = seam.routes;
        let mut needs = seam.needs.into_iter();
        let output = match needs.next() {
            Some((need, claim)) => match run.discard() {
                // An aborted run is the build's answer: its abort is
                // observed, never discarded.
                Err(abort) => self.aborted_build_output(abort).into(),
                Ok(_) => {
                    // The dropped run's typed failure is the next run's to
                    // report.
                    self.dispatch_txn.borrow_mut().flow.last_root_failure = None;
                    build.awaiting = Some(need.clone());
                    build.claim = Some(claim);
                    build.pending = needs.collect();
                    return FramePoll::Need(need);
                }
            },
            None => run
                .adopt(self)
                .unwrap_or_else(|abort| self.aborted_build_output(abort).into()),
        };
        // A completed build holds no contextual call any more: one its run
        // did not reach again is released.
        self.release_held_contextual_routes(std::mem::take(&mut build.routes));
        FramePoll::Done(Box::new(output))
    }

    /// Answer a read the function's own body makes at the seam of the flow
    /// evaluation whose frame is stepping: an instantiation or a `typeof`
    /// the frame holds the answer to reads it; one the memo serves warm
    /// answers at once and joins the answers; one that must be computed
    /// becomes one of the frame's needs and reads as a typed gap nothing
    /// built from it keeps. `None` everywhere else — a query the body
    /// evaluates in place, a callee evaluated inline beneath it — where the
    /// read evaluates in place.
    pub(super) fn read_at_flow_seam(&self, key: &SemanticQueryKey) -> Option<ValueRead> {
        if !matches!(
            key,
            SemanticQueryKey::Instantiate(_) | SemanticQueryKey::TypeOf { .. }
        ) {
            return None;
        }
        enum Seam {
            Answered(QueryDelivery),
            Waiting,
            Enter,
        }
        let (seam, depth) = {
            let installed = self.flow_seam.borrow();
            let seam = installed.as_ref()?;
            if !self.flow_seam_is_open(seam) {
                return None;
            }
            let read = match seam.answers.get(key).cloned() {
                Some(delivery) => Seam::Answered(delivery),
                None if seam.needs.iter().any(|(needed, _)| needed == key) => Seam::Waiting,
                None => Seam::Enter,
            };
            (read, seam.depth)
        };
        let nesting = Nesting::Frame { depth };
        Some(match seam {
            Seam::Answered(delivery) => self.read_delivery(delivery, nesting),
            Seam::Waiting => self.suspended_decision_read(),
            Seam::Enter => match self.enter_query_frame(key, depth) {
                Err(delivery) => {
                    self.with_flow_seam(|seam| {
                        seam.answers.insert(key.clone(), (*delivery).clone());
                    });
                    self.read_delivery(*delivery, nesting)
                }
                Ok(claim) => {
                    self.with_flow_seam(|seam| seam.needs.push((key.clone(), claim)));
                    self.suspended_decision_read()
                }
            },
        })
    }

    /// Whether the flow evaluation's own body is evaluating here: not a
    /// query it evaluates in place, nor a callee evaluated inline beneath
    /// it.
    fn flow_seam_is_open(&self, seam: &FlowSeam) -> bool {
        seam.nested == 0 && self.dispatch_txn.borrow().reentry().depth() == seam.frame_depth
    }

    fn with_flow_seam(&self, update: impl FnOnce(&mut FlowSeam)) {
        update(
            self.flow_seam
                .borrow_mut()
                .as_mut()
                .expect("the flow evaluation's seam stays installed for its run"),
        );
    }

    /// Open a window in which a contextual callback of the stepping flow
    /// evaluation's own body types while its call's collector is held: a
    /// need made meanwhile holds the call's route for the next run. Whether
    /// the window opened — not outside a flow frame's run, nor once the run
    /// waits on a need already, when what is left of it is dropped.
    pub(super) fn open_contextual_window(&self) -> bool {
        let mut installed = self.flow_seam.borrow_mut();
        let Some(seam) = installed.as_mut() else {
            return false;
        };
        if !seam.needs.is_empty() || !self.flow_seam_is_open(seam) {
            return false;
        }
        seam.windows += 1;
        true
    }

    /// Close a window [`Self::open_contextual_window`] opened.
    pub(super) fn close_contextual_window(&self) {
        if let Some(seam) = self.flow_seam.borrow_mut().as_mut() {
            seam.windows = seam.windows.saturating_sub(1);
        }
    }

    /// Whether the stepping flow evaluation's run waits on a need: what is
    /// left of it is dropped.
    pub(super) fn contextual_window_waits(&self) -> bool {
        self.flow_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| !seam.needs.is_empty())
    }

    /// Demand what relating a typed callback to the contextual target its
    /// call's held collector waits on unwraps — the operands, and a
    /// signature's parameter and return types, as the relation instantiates
    /// them before relating — at the stepping flow evaluation's seam: one
    /// the answers hold or the memo serves warm reads at once, one that
    /// must be computed becomes one of the frame's needs. What the demand
    /// observed is the relation's to observe, so it is dropped. Whether the
    /// run must stop for any; never outside a flow frame's own body.
    pub(super) fn contextual_delivery_waits(
        &self,
        state: &super::call_resolve::ContextualCallState,
        typed: SemanticNodeId,
    ) -> bool {
        let Some(target) = state.pending_delivery_target() else {
            return false;
        };
        if !self.open_contextual_window() {
            return false;
        }
        let graph = self.graph();
        let mut operands = Vec::new();
        for node in [typed, target] {
            operands.push(node);
            if let Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) = graph.node_data(node).as_deref()
            {
                operands.extend(params.iter().map(|param| param.ty));
                operands.push(*return_type);
            }
        }
        let demanded = self.observe_read(|| {
            for operand in operands {
                let _ = self.unwrap_identity_carrier_for_relation(operand);
            }
        });
        let _ = demanded.discard();
        self.close_contextual_window();
        self.contextual_window_waits()
    }

    /// Hold a contextual call's route, collector included, for the stepping
    /// flow evaluation's next run.
    pub(super) fn hold_contextual_route(&self, route: super::flow_return::HeldContextualRoute) {
        #[cfg(any(test, feature = "test-support"))]
        if let Some((session, collected)) = route.held_collector_for_tests() {
            self.record_contextual_route_for_tests(ContextualRouteEventForTests::Held {
                session,
                collected,
            });
        }
        let mut installed = self.flow_seam.borrow_mut();
        match installed.as_mut() {
            Some(seam) => seam.routes.push(route),
            None => {
                drop(installed);
                self.release_held_contextual_routes(vec![route]);
            }
        }
    }

    /// The route the stepping flow evaluation's last run held for the call
    /// it asks with `asked`, to resume instead of asking its executor.
    pub(super) fn take_held_contextual_route(
        &self,
        asked: &crate::semantic_query::ResolveCallKey,
    ) -> Option<super::flow_return::HeldContextualRoute> {
        let route = {
            let mut installed = self.flow_seam.borrow_mut();
            let seam = installed.as_mut()?;
            if !self.flow_seam_is_open(seam) {
                return None;
            }
            let position = seam
                .routes
                .iter()
                .position(|route| route.asked() == asked)?;
            seam.routes.remove(position)
        };
        #[cfg(any(test, feature = "test-support"))]
        if let Some((session, collected)) = route.held_collector_for_tests() {
            self.record_contextual_route_for_tests(ContextualRouteEventForTests::Resumed {
                session,
                collected,
            });
        }
        Some(route)
    }

    /// Release contextual routes no run resumes: each held collector returns
    /// to the transaction abandoned, and what the route observed is dropped.
    pub(super) fn release_held_contextual_routes(
        &self,
        routes: Vec<super::flow_return::HeldContextualRoute>,
    ) {
        for route in routes {
            #[cfg(any(test, feature = "test-support"))]
            if let Some((session, collected)) = route.held_collector_for_tests() {
                self.record_contextual_route_for_tests(ContextualRouteEventForTests::Released {
                    session,
                    collected,
                });
            }
            route.release(self);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn record_contextual_route_for_tests(&self, event: ContextualRouteEventForTests) {
        self.binding
            .observers
            .flow
            .contextual_routes
            .lock()
            .expect("the contextual-route observer is never poisoned")
            .push(event);
    }
}

/// What happened to a contextual call a flow frame held. Test-only.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextualRouteEventForTests {
    /// Held across the frame's suspension, its collector holding
    /// `collected` deposits.
    Held { session: u64, collected: usize },
    /// Resumed by the frame's next run.
    Resumed { session: u64, collected: usize },
    /// Released unresumed, its collector abandoned.
    Released { session: u64, collected: usize },
}

/// What a call frame's walk held when it suspended. Test-only.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
pub struct CallSuspensionForTests {
    /// The demands the walk stopped on.
    pub needs: Vec<SemanticQueryKey>,
    /// The deposits its candidate's collector held.
    pub collected_candidates: usize,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static CALL_SUSPENSIONS_FOR_TESTS: std::cell::RefCell<Vec<CallSuspensionForTests>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Take the suspensions call frames' walks made on this thread.
#[cfg(any(test, feature = "test-support"))]
pub fn take_call_suspensions_for_tests() -> Vec<CallSuspensionForTests> {
    CALL_SUSPENSIONS_FOR_TESTS.with(|observed| std::mem::take(&mut *observed.borrow_mut()))
}
