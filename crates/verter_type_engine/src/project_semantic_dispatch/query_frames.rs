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
//! well. The decision runs as a frame whose reads of instantiations,
//! `typeof`s and nested conditionals are answered by the deliveries it received; one it has no
//! answer for becomes its need. The unfinished stage runs again once that
//! need is delivered; completed operand preparation, relation selection, distribution members
//! and their observations stay on the frame. A chain of conditionals each
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

use crate::semantic_query::checker_policy::TupleSplicePosition;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::build::{InstantiateBuild, InstantiatePoll, InstantiateStart, TypeofBuild, TypeofStart};
use super::cost_receipt::Nesting;
use super::family_continuation::{FamilyBuild, FamilyStep};
use super::{BuildLocalTaint, CarrierNormalizationPrelude, ProjectSemanticDispatch};
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::resolver_core::request_ports::{IndexedLoad, IndexedLoadOutcome, IndexedReadiness};
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

const ROOT_FRAME_DEPTH: u32 = 1;

/// Where each relation operand's carrier walk stopped in a decision's last
/// run, by operand and checking site: the point whose step reached a read
/// without an answer, with the instantiations the walk took to reach it.
/// Every node before it settled from answers the frame keeps, so the next
/// run's operand demand resumes there, still counting toward the
/// instantiation budget the walk from the operand would reach.
type OperandResumes =
    FxHashMap<(SemanticNodeId, TupleSplicePosition), super::relation::CarrierWalkPoint>;

/// What a query answered to the frame that needed it: the read (with the
/// answering result's cost receipt, for the consumer's recording), and the
/// carrier of the result that answered it, for the consumer to replay.
#[derive(Clone)]
pub(super) struct QueryDelivery {
    site: TupleSplicePosition,
    read: ValueRead,
    carrier: Option<ReadSetSignature>,
}

/// A delivery a frame's build consumes where its evaluation reads it,
/// with how deep the needing frame nests.
pub(super) struct FrameDelivery {
    delivery: QueryDelivery,
    depth: u32,
}

/// One demand of a semantic task: the query and the checking site it is
/// evaluated at. A query evaluated at one site recovers differently from
/// the same query at the other (TS2799 against TS2800), so the two are two
/// computations: a result completed at one site never answers a need at
/// the other, and a need at one site never closes a cycle through the
/// other's open demand.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct QueryDemand {
    key: SemanticQueryKey,
    site: TupleSplicePosition,
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
    retries: FxHashMap<QueryDemand, ClaimAttempt>,
    /// The depth a frame started now nests at: one below the frame whose
    /// step needed it (the drive runs one chain, so a demand starts right
    /// after the step that needed it, or after its wait restarts it).
    child_depth: u32,
    /// Whether a frame waits on a file's load as an outside producer: a
    /// production poll's drive. The synchronous entry's drive loads the
    /// file where the frame reads it, as its external driver.
    suspends_loads: bool,
    /// The pending drive's wait on the cancellation that stops it, so the
    /// cancellation wakes the drive instead of a timer re-polling it.
    stop_wake: Option<verter_execution::cancellation::CancellationSubscription>,
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
/// decision, a call's resolution, a function's flow return, or a family
/// build written as a continuation, between two of its stages.
pub(super) enum FrameBuild {
    Instantiate(Box<InstantiateBuild>),
    Typeof(Box<TypeofBuild>),
    Conditional(Box<ConditionalBuild>),
    Call(Box<CallBuild>),
    Flow(Box<FlowBuild>),
    Family(Box<FamilyBuild>),
    /// A family build waiting on its file's load, to read that file next
    /// from the load's terminal outcome.
    FamilySource(Box<FamilyBuild>, Arc<str>, LoadEnded),
}

/// Where a load's terminal outcome is kept for the frame that waited on it:
/// the frame consumes it once, and never begins the same load again.
pub(super) type LoadEnded = Arc<std::sync::OnceLock<IndexedLoadOutcome>>;

/// The load of a file a frame waits on, and where its terminal outcome goes
/// for the frame that consumes it.
pub(super) struct LoadWait<'p> {
    load: Box<dyn IndexedLoad + Send + 'p>,
    ended: LoadEnded,
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
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
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
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
    /// The reads the run reached that no answer holds, in the order it
    /// reached them, each with the claim it entered with.
    needs: SeamNeeds,
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
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
    resumes: OperandResumes,
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
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
    resumes: OperandResumes,
    needs: SeamNeeds,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
}

/// What a frame's build did in one step.
enum FramePoll<'a> {
    Need(SemanticQueryKey),
    /// The build waits on a file's load before it reads the file.
    Await(LoadWait<'a>),
    Done(Box<crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue>>),
}

impl From<InstantiatePoll> for FramePoll<'_> {
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
/// answers. A step advances its sequential state with the answers the frame
/// holds. A stage that reaches an unanswered read discards that stage's
/// observations and resumes after delivery. Completed operand preparation
/// and its observations remain owned by the frame. No read is ever
/// evaluated in place one native level deeper, so a chain of conditionals each deciding over the one before is
/// a chain of frames, and a decision over a wide union runs again once
/// for its operands, not once per member.
#[derive(Default)]
pub(super) struct ConditionalBuild {
    decision: super::conditional_decision::ConditionalDecisionState,
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
    resumes: OperandResumes,
    awaiting: Option<SemanticQueryKey>,
    /// The claim the awaited read entered with, until the runtime takes it.
    claim: Option<ClaimAttempt>,
    /// The other reads the last run reached without an answer, in the order
    /// it reached them, each with the claim it entered with.
    pending: std::collections::VecDeque<(SemanticQueryKey, ClaimAttempt)>,
}

/// The reads a seam's run reached that no answer holds, in the order it
/// reached them, each with the claim it entered with, and an index of them
/// by demand. A run over a wide union reaches a read per member; asking
/// whether one of them is already needed compares it with the needs that
/// share its demand's hash, not with every need before it. Each need's key
/// is stored once, in `reached`, and a membership question borrows the key
/// it asks about.
#[derive(Default)]
pub(super) struct SeamNeeds {
    reached: Vec<(SemanticQueryKey, ClaimAttempt)>,
    /// Positions in `reached`, by the hash of each need's demand.
    demands: hashbrown::HashTable<usize>,
}

/// Whether the reached need `(need, claim)` is the demand of `key` at
/// `site`: the one comparison a membership question makes with a stored
/// need, counted for the growth tests.
fn is_need(
    (need, claim): &(SemanticQueryKey, ClaimAttempt),
    key: &SemanticQueryKey,
    site: TupleSplicePosition,
) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    SEAM_NEED_PROBES.with(|probes| probes.set(probes.get() + 1));
    claim.checking_site() == site && need == key
}

/// The hash a need's demand — its key at its checking site — is indexed by.
fn demand_hash(key: &SemanticQueryKey, site: TupleSplicePosition) -> u64 {
    std::hash::BuildHasher::hash_one(&rustc_hash::FxBuildHasher, (key, site))
}

impl SeamNeeds {
    fn push(&mut self, key: SemanticQueryKey, claim: ClaimAttempt) {
        let hash = demand_hash(&key, claim.checking_site());
        let position = self.reached.len();
        self.reached.push((key, claim));
        let reached = &self.reached;
        self.demands.insert_unique(hash, position, |&stored| {
            let (key, claim) = &reached[stored];
            demand_hash(key, claim.checking_site())
        });
    }

    /// Whether `key`, at `site`, is one of the needs.
    fn contains(&self, key: &SemanticQueryKey, site: TupleSplicePosition) -> bool {
        if self.reached.is_empty() {
            return false;
        }
        #[cfg(any(test, feature = "test-support"))]
        SEAM_NEED_QUESTIONS.with(|questions| questions.set(questions.get() + 1));
        let reached = &self.reached;
        self.demands
            .find(demand_hash(key, site), |&stored| {
                is_need(&reached[stored], key, site)
            })
            .is_some()
    }

    fn is_empty(&self) -> bool {
        self.reached.is_empty()
    }

    fn len(&self) -> usize {
        self.reached.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn iter(&self) -> std::slice::Iter<'_, (SemanticQueryKey, ClaimAttempt)> {
        self.reached.iter()
    }
}

impl IntoIterator for SeamNeeds {
    type Item = (SemanticQueryKey, ClaimAttempt);
    type IntoIter = std::vec::IntoIter<(SemanticQueryKey, ClaimAttempt)>;

    fn into_iter(self) -> Self::IntoIter {
        self.reached.into_iter()
    }
}

/// What a running conditional decision's reads are answered from: its
/// frame's answers, and the reads they do not answer.
pub(super) struct DecisionSeam {
    answers: FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
    /// Where the operand demands of the decision's last run stopped.
    resumes: OperandResumes,
    /// The reads the run reached that no answer holds, in the order it
    /// reached them, each with the claim it entered with for the runtime
    /// to produce it.
    needs: SeamNeeds,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
    /// How many queries the decision reached are evaluating in place now.
    /// Inside one, a read the answers hold is still answered by them, but
    /// any other evaluates as it would anywhere: a query evaluated in place
    /// keeps what it builds, so it never reads a gap.
    nested: u32,
    /// Only an explicit member, inferred winner or final union reduction
    /// supplies a materialized child. Operand and relation probes retain
    /// temporary checker context and cannot transfer it through this seam.
    materialized_children: bool,
}

/// The seam whose run made a read that joins another task's producer
/// ([`ProjectSemanticDispatch::join_seam_for`]).
#[derive(Clone, Copy)]
pub(super) enum JoinSeam {
    Decision,
    Flow,
}

pub(super) struct ConditionalChildQueries<'d> {
    restore: Option<(
        &'d verter_execution::sync_cell::SyncRefCell<Option<DecisionSeam>>,
        bool,
    )>,
}

impl Drop for ConditionalChildQueries<'_> {
    fn drop(&mut self) {
        if let Some((cell, previous)) = self.restore {
            if let Some(seam) = cell.borrow_mut().as_mut() {
                seam.materialized_children = previous;
            }
        }
    }
}

/// A query the running decision reached, evaluating in place; the seam is
/// the decision's own again when it returns.
pub(super) struct NestedDecisionQuery<'d> {
    seam: Option<&'d verter_execution::sync_cell::SyncRefCell<Option<DecisionSeam>>>,
    flow: Option<&'d verter_execution::sync_cell::SyncRefCell<Option<FlowSeam>>>,
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
    site: TupleSplicePosition,
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
    /// The files the frame waited on before its build began
    /// ([`ProjectSemanticDispatch::frame_source_load`]), each with how its
    /// load ended until the frame took that outcome.
    awaited_loads: Vec<(Arc<str>, Option<LoadEnded>)>,
    /// The awaited loads that ended without integrating their file and that
    /// the frame's build has not yet consumed: each answers the build's first
    /// load of its file, as the synchronous build's own failed load answers.
    failed_loads: crate::resolver_core::request_ports::ConsumedLoadEnds,
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

/// What a drive's chain waits on outside its execution: another task's
/// producer of a demand, or the load of a file a frame reads next.
#[allow(
    clippy::large_enum_variant,
    reason = "an execution holds at most one outside wait, inline beside its frames; boxing the claim would allocate on every wait for another task's producer"
)]
pub(super) enum PendingWait<'p> {
    Claim(PendingClaim<'p>),
    /// Resolved, the waiting frame steps again and reads the file.
    Load(LoadWait<'p>),
}

/// A subscription to another task's producer, with the claim it continues.
pub(super) struct PendingClaim<'p> {
    demand: QueryDemand,
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
    /// Stop registrations travel with the task just like its flight wait.
    /// An ownerless job can acquire owners while the task is suspended, so
    /// keep its subscription even while the request decides cancellation.
    job_cancellation_wake: Option<verter_execution::cancellation::CancellationSubscription>,
    request_cancellation_wake: Option<verter_execution::cancellation::CancellationSubscription>,
    accounting: TaskAccounting,
    /// Where a task dropped without being returned leaves its request state
    /// for the dispatcher that lent it.
    home: Arc<parking_lot::Mutex<Option<UnreturnedTask<'a, C>>>>,
    /// The root the first poll opens.
    root: Option<QueryDemand>,
    /// The root's delivery (`Err` when a stop abandoned it), kept from the
    /// poll that completed the task until the entry that lent it reads it.
    finished: Option<Result<QueryDelivery, ()>>,
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
    /// Advance the task as far as it can go, on whichever thread its one
    /// exclusive poller runs: `Ready` once its root completed or a stop
    /// abandoned it, or `Pending` once it waits on a producer outside it
    /// that will wake `waker`. The selected request or shared job's
    /// cancellation also wakes the task, so a stop does not depend on the
    /// outside producer completing or a timer tick.
    ///
    /// Polling is the task's only way forward: it has no blocking driver
    /// and no handle on its dispatcher or resolver. Its root's answer is
    /// read by the entry that lent it ([`SemanticQueryPoll`]).
    pub fn poll(&mut self, waker: &std::task::Waker) -> std::task::Poll<()> {
        if self.finished.is_none() {
            match self.advance(waker) {
                std::task::Poll::Ready(done) => self.finished = Some(done),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
        std::task::Poll::Ready(())
    }

    /// [`Self::poll`], handing over the root's delivery once it is ready.
    fn poll_delivery(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<Result<QueryDelivery, ()>> {
        match self.finished.take() {
            Some(done) => std::task::Poll::Ready(done),
            None => self.advance(waker),
        }
    }

    fn advance(&mut self, waker: &std::task::Waker) -> std::task::Poll<Result<QueryDelivery, ()>> {
        // Aggregate ownership is monotonic. Once a job has accepted owners,
        // their aggregate decides: an individual request cannot stop shared
        // work, and need not wake it. Readiness is still checked by the
        // execution's cancellation checkpoint under its bound context.
        if self
            .job_cancellation
            .as_ref()
            .is_some_and(|job| job.has_registered_owners())
        {
            self.request_cancellation_wake = None;
        }
        if let Some(stop) = &mut self.job_cancellation_wake {
            let _ = stop.poll(waker);
        }
        if let Some(stop) = &mut self.request_cancellation_wake {
            let _ = stop.poll(waker);
        }
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
        let poll = self.execution.poll(program, self.root.take(), waker);
        if poll.is_ready() {
            self.job_cancellation_wake = None;
            self.request_cancellation_wake = None;
        }
        // Each frame step keeps its partiality to its own frame, reaching a
        // consumer through the delivery it reads. The lender consumes the
        // root's: its partiality returns to the lender with the task.
        if let std::task::Poll::Ready(Ok(delivery)) = &poll {
            if delivery.read.result_is_partial() {
                crate::request_context::mark_request_result_partial_from_read_with(
                    delivery.read.partial_reason_classes(),
                );
            }
        }
        poll
    }

    /// [`Self::poll`], reporting what the root answered.
    #[cfg(any(test, feature = "test-support"))]
    pub fn poll_for_tests(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<TaskOutcomeForTests> {
        self.poll_read_for_tests(waker)
            .map(|outcome| match outcome {
                Ok(read) => TaskOutcomeForTests::Answered(read.value),
                Err(()) => TaskOutcomeForTests::Stopped,
            })
    }

    /// The terminal read, including its fact status, diagnostics and receipt.
    #[cfg(any(test, feature = "test-support"))]
    pub fn poll_read_for_tests(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<Result<CacheRead<QueryResult<SemanticQueryValue>>, ()>> {
        self.poll_delivery(waker)
            .map(|outcome| outcome.map(|delivery| delivery.read))
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
        let mut lent = LentTask::new(self, self.lend_semantic_task(key, attempt, true));
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

    /// Run `body` while a request-local completion of `key` is held open at
    /// the type site (or the expression site when `opened_at_expression`),
    /// with every demand `body` makes evaluated at the expression site when
    /// `read_at_expression`.
    pub fn with_request_local_open_for_tests<R>(
        &self,
        key: &SemanticQueryKey,
        opened_at_expression: bool,
        read_at_expression: bool,
        body: impl FnOnce() -> R,
    ) -> R {
        let site = |expression| {
            if expression {
                TupleSplicePosition::Expression
            } else {
                TupleSplicePosition::Type
            }
        };
        let _open = super::RequestLocalOpen::enter(
            &self.request_local_open,
            key,
            site(opened_at_expression),
        );
        let _site = self.checking_site_scope(site(read_at_expression));
        body()
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
        suspends_loads: bool,
    ) -> SemanticTask<'_, 'a, C> {
        let dispatch = self.task_dispatcher();
        self.exchange_request_state(&dispatch);
        let graph = self.graph();
        let mut execution = SemanticExecution::new();
        if let Some(context) = verter_execution::request_context::current_context() {
            execution.bind_request_context(context);
        }
        let root = QueryDemand {
            key,
            site: attempt.checking_site(),
        };
        let mut program = QueryProgram {
            dispatch,
            graph: graph.store(),
            task: graph.execution_task(),
            retries: FxHashMap::default(),
            child_depth: ROOT_FRAME_DEPTH,
            suspends_loads,
            stop_wake: None,
            #[cfg(any(test, feature = "test-support"))]
            held_call_needs: None,
        };
        program.retries.insert(root.clone(), attempt);
        let job_cancellation = verter_execution::cancellation::current_job_cancellation_token();
        let job_cancellation_wake = job_cancellation.as_ref().map(|job| job.subscribe());
        let request_cancellation_wake =
            crate::request_context::current_request_cancellation_token()
                .filter(|_| {
                    !job_cancellation
                        .as_ref()
                        .is_some_and(|job| job.has_registered_owners())
                })
                .map(|request| request.subscribe());
        SemanticTask {
            execution,
            program: Some(program),
            construction: Some(
                crate::semantic_query_memo::node_roots::TaskConstructionOwner::new(
                    self.binding.construction.arena(),
                ),
            ),
            validity: crate::request_context::TaskExecutionValidity::inherit(),
            job_cancellation,
            job_cancellation_wake,
            request_cancellation_wake,
            accounting: TaskAccounting::lend(),
            home: Arc::new(parking_lot::Mutex::new(None)),
            root: Some(root),
            finished: None,
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

/// A root query on the continuation runtime, from its entry to its
/// delivery: the one implementation the synchronous entry
/// ([`ProjectSemanticDispatch::drive_query`]) blocks on and the production
/// poll ([`SemanticQueryPoll`]) advances.
///
/// Its fields drop in order: the task gives its request state back before
/// the entry's query level and connected demand close, as they close after
/// the delivery on the synchronous entry's stack.
struct DrivenRoot<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> {
    lent: Option<LentTask<'d, 'a, C>>,
    _query_depth: Option<super::connected_demand::ConnectedDemandGuard<'d>>,
    isolated_root: Option<super::IsolatedRoot>,
    connected: super::connected_demand::ConnectedDemandGuard<'d>,
    lender: &'d ProjectSemanticDispatch<'a, C>,
    key: SemanticQueryKey,
}

/// How a root's entry begins: answered at its entry, or a lent task.
#[allow(
    clippy::large_enum_variant,
    reason = "returned once to the entry that starts the root: the synchronous entry keeps the root on its stack as it kept the lent task, and the polled entry boxes it"
)]
enum RootStart<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> {
    Answered(ValueRead),
    Driven(DrivenRoot<'d, 'a, C>),
}

impl<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> DrivenRoot<'d, 'a, C> {
    fn start(
        lender: &'d ProjectSemanticDispatch<'a, C>,
        key: SemanticQueryKey,
        polled: bool,
    ) -> RootStart<'d, 'a, C> {
        // The connected demand spans the whole drive; the entry itself is
        // charged exactly as a synchronous entry is.
        let (connected, _) = lender.enter_connected_demand(false);
        // An isolated root whose refusal is sealed answers from it.
        let isolated_root = lender.isolated_root_identity(&connected, &key);
        if let Some(read) = isolated_root
            .as_ref()
            .and_then(|root| lender.answer_from_sealed_refusal(root))
        {
            return RootStart::Answered(read);
        }
        // The entry is one nested query level, as a synchronous entry is.
        let (attempt, query_depth) = match lender.enter_query(&key, None, Nesting::Drive) {
            Ok(entered) => entered,
            Err(read) => {
                return RootStart::Answered(
                    lender.finish_driven_read(&key, read, None, &connected, None),
                );
            }
        };
        // The root runs as a semantic task that borrows the lender's
        // request state and gives it back when it is done.
        let lent = LentTask::new(
            lender,
            lender.lend_semantic_task(key.clone(), attempt, polled),
        );
        RootStart::Driven(Self {
            lent: Some(lent),
            _query_depth: query_depth,
            isolated_root,
            connected,
            lender,
            key,
        })
    }

    /// The lent task, while the root has not delivered.
    fn task(&mut self) -> Option<&mut SemanticTask<'d, 'a, C>> {
        self.lent.as_mut().and_then(|lent| lent.task.as_mut())
    }

    /// Advance the root's task; once it is ready, take back the request
    /// state it borrowed and deliver its answer here.
    fn poll(&mut self, waker: &std::task::Waker) -> std::task::Poll<ValueRead> {
        let Some(task) = self.task() else {
            panic!("a root is not polled after it delivered")
        };
        let outcome = match task.poll_delivery(waker) {
            std::task::Poll::Ready(outcome) => outcome,
            std::task::Poll::Pending => return std::task::Poll::Pending,
        };
        drop(self.lent.take());
        let lender = self.lender;
        std::task::Poll::Ready(match outcome {
            Ok(QueryDelivery { read, carrier, .. }) => {
                if let Some(receipt) = read.receipt.priced() {
                    lender
                        .connected_demand
                        .record_prerequisite(receipt, Nesting::Drive);
                }
                let traced = lender.driven_root_traced.borrow_mut().take();
                lender.finish_driven_read(
                    &self.key,
                    read,
                    carrier,
                    &self.connected,
                    self.isolated_root.take().zip(traced),
                )
            }
            Err(()) => lender.finish_driven_read(
                &self.key,
                crate::semantic_query_memo::cancelled_cache_read(),
                None,
                &self.connected,
                None,
            ),
        })
    }
}

/// A root semantic query evaluated by polling: the production entry a
/// consumer that must not block — a metadata or compile root suspended on
/// its scheduler — drives a query through. It reads, in the query's own
/// value domain, what the synchronous full-value entry
/// ([`ProjectSemanticDispatch::execute_via_cold_build_helper`]) reads,
/// through the same implementation. An instantiation, a `typeof`, a
/// machinery-root conditional or call, or a callback-typing machinery-root
/// flow return runs as a semantic task whose frames wait on
/// another task's producer by returning `Pending`. Every other query enters
/// the same cold-query admission and family-memo claim as the synchronous
/// entry; where that claim meets another task's producer of the query, the
/// entry returns `Pending` with the claim kept, instead of waiting it out.
///
/// The entry holds its dispatcher exclusively
/// ([`super::ThreadDispatch::poll_query`]) and stays on its thread. Its task
/// ([`Self::task`]) is `Send` and may be polled to readiness on another
/// thread; the entry delivers the answer here. Dropping the entry while it
/// is pending releases every frame, flight and wait it holds and gives the
/// request state back.
pub struct SemanticQueryPoll<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> {
    state: RootPollState<'d, 'a, C>,
    lender: &'d ProjectSemanticDispatch<'a, C>,
}

enum RootPollState<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> {
    Unstarted(SemanticQueryKey),
    /// A driven root, and — for every root but a flow return's, whose
    /// executor drives it before entering any execution — the dispatcher
    /// operation it runs as. The operation spans the root's polls: its
    /// state moves with the request state the root's task borrows, so it
    /// closes only after the task gives that state back. Declared after
    /// the root, so it closes after the root's entry does.
    Driven {
        root: Box<DrivenRoot<'d, 'a, C>>,
        operation: Option<super::DispatchOperation<'d>>,
    },
    /// A root off the continuation runtime: its admitted cold entry and its
    /// family-memo claim, kept across polls while the claim waits on
    /// another task's producer, and the operation it runs as.
    Cold {
        root: Box<ColdRoot<'d>>,
        operation: super::DispatchOperation<'d>,
    },
    Delivered,
}

/// A root off the continuation runtime between its polls. Its claim drops
/// before its entry, so a released root withdraws its wait before its query
/// level and connected demand close.
struct ColdRoot<'d> {
    claim: super::ColdClaim<'d>,
    entry: super::ColdEntry<'d>,
    /// A flow return evaluated off the runtime is a machinery root: its
    /// publication is captured for it to drain its members onto.
    publication: Option<Option<crate::semantic_query_memo::PublishedMemoCandidate>>,
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Run `body` under the thread's scopes of one execution
    /// (`execution_thread_scopes!`): a polled root holds its operation
    /// across its polls — the operation's state moves with the request
    /// state its task borrows — and enters these on each.
    fn within_execution_scopes<R>(
        &self,
        #[cfg_attr(not(feature = "test-support"), allow(unused_variables))]
        family: crate::semantic_query::SemanticQueryKeyTag,
        body: impl FnOnce() -> R,
    ) -> R {
        execution_thread_scopes!(self, family);
        body()
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> super::ThreadDispatch<'a, C> {
    /// Begin evaluating `key` as a polled root query ([`SemanticQueryPoll`]).
    pub fn poll_query(&mut self, key: SemanticQueryKey) -> SemanticQueryPoll<'_, 'a, C> {
        SemanticQueryPoll {
            state: RootPollState::Unstarted(key),
            lender: &self.dispatch,
        }
    }
}

impl<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> SemanticQueryPoll<'d, 'a, C> {
    /// Advance the query: `Ready` with what the synchronous full-value
    /// entry reads, in the query's own value domain, or `Pending` once it
    /// waits on a producer outside it that will wake `waker`.
    pub fn poll(&mut self, waker: &std::task::Waker) -> std::task::Poll<ValueRead> {
        if let RootPollState::Unstarted(_) = &self.state {
            let RootPollState::Unstarted(key) =
                std::mem::replace(&mut self.state, RootPollState::Delivered)
            else {
                unreachable!("the state was just matched")
            };
            #[cfg(any(test, feature = "test-support"))]
            super::raise::DISPATCH_TRACE.with(|t| {
                t.borrow_mut()
                    .push(super::raise::query_key_discriminant(&key))
            });
            if let Some(read) = self.start(key) {
                return std::task::Poll::Ready(read);
            }
        }
        match &self.state {
            RootPollState::Driven { .. } => self.poll_driven(waker),
            RootPollState::Cold { .. } => self.poll_cold(waker),
            _ => panic!("a polled root query is not polled after it delivered"),
        }
    }

    /// [`Self::poll`], read as a type node: the convenience adapter for a
    /// consumer of a node-domain query. A query answering another value
    /// domain reads as the domain mismatch the synchronous node entry
    /// ([`ProjectSemanticDispatch::execute_read`]) reports.
    pub fn poll_node(
        &mut self,
        waker: &std::task::Waker,
    ) -> std::task::Poll<CacheRead<QueryResult<SemanticNodeId>>> {
        self.poll(waker).map(super::narrow_value_cache_read)
    }

    /// Admit the root: a driven root's task, or a cold root's entry. `Some`
    /// when the root is answered at its admission.
    fn start(&mut self, key: SemanticQueryKey) -> Option<ValueRead> {
        let lender = self.lender;
        if lender.drives_on_the_runtime(&key) {
            let operation = (!matches!(key, SemanticQueryKey::FlowReturn(_)))
                .then(|| lender.enter_operation(false));
            let start = match operation {
                Some(_) => lender
                    .within_execution_scopes(key.tag(), || DrivenRoot::start(lender, key, true)),
                None => DrivenRoot::start(lender, key, true),
            };
            return match start {
                RootStart::Answered(read) => {
                    let scoped = operation.is_some();
                    drop(operation);
                    Some(self.deliver(read, scoped))
                }
                RootStart::Driven(root) => {
                    self.state = RootPollState::Driven {
                        root: Box::new(root),
                        operation,
                    };
                    None
                }
            };
        }
        // Off the runtime the root is the synchronous entry's cold query:
        // the same operation, scopes, admission and claim, entered once.
        let flow = matches!(key, SemanticQueryKey::FlowReturn(_));
        let operation = lender.enter_operation(false);
        let begun = lender.within_execution_scopes(key.tag(), || {
            if let Some(read) = lender.refuse_relation_without_inference_context(&key) {
                return super::ColdBegin::Answered(read);
            }
            let _nested = lender.nested_decision_query();
            lender.begin_cold_query(key, !flow)
        });
        match begun {
            super::ColdBegin::Answered(read) => {
                drop(operation);
                let read = self.deliver(read, true);
                if flow {
                    lender.relation_abort_completed_members();
                }
                Some(read)
            }
            super::ColdBegin::Entered(entry) => {
                self.state = RootPollState::Cold {
                    root: Box::new(ColdRoot {
                        claim: super::ColdClaim::default(),
                        entry,
                        publication: flow.then_some(None),
                    }),
                    operation,
                };
                None
            }
        }
    }

    /// Advance a driven root's task; once it delivers, close its entry
    /// within the execution's scopes, then its operation.
    fn poll_driven(&mut self, waker: &std::task::Waker) -> std::task::Poll<ValueRead> {
        let lender = self.lender;
        let (family, scoped) = match &self.state {
            RootPollState::Driven { root, operation } => (root.key.tag(), operation.is_some()),
            _ => unreachable!("a driven root is polled as one"),
        };
        let state = &mut self.state;
        let mut operation = None;
        let mut advance = || {
            let RootPollState::Driven { root, .. } = &mut *state else {
                unreachable!("the state was just matched")
            };
            let poll = root.poll(waker);
            // A delivered root closes its entry — its query level and its
            // connected demand — where the synchronous entry closes it:
            // within the execution's scopes, before its operation closes and
            // its read is delivered.
            if poll.is_ready() {
                if let RootPollState::Driven {
                    operation: open, ..
                } = std::mem::replace(&mut *state, RootPollState::Delivered)
                {
                    operation = open;
                }
            }
            poll
        };
        let poll = if scoped {
            lender.within_execution_scopes(family, advance)
        } else {
            advance()
        };
        drop(operation);
        poll.map(|read| self.deliver(read, scoped))
    }

    /// Advance a cold root's claim; once it read, deliver the entry as the
    /// synchronous entry does — within the execution's scopes, then its
    /// operation closes, the read is re-rooted where it is delivered, and a
    /// machinery-root flow return drains or aborts the members its
    /// evaluation completed.
    fn poll_cold(&mut self, waker: &std::task::Waker) -> std::task::Poll<ValueRead> {
        let lender = self.lender;
        let RootPollState::Cold { root, .. } = &mut self.state else {
            unreachable!("a cold root is polled as one")
        };
        let family = root.entry.key().tag();
        let claimed = {
            let ColdRoot {
                claim,
                entry,
                publication,
            } = &mut **root;
            lender.within_execution_scopes(family, || {
                let _nested = lender.nested_decision_query();
                lender.claim_cold_query(entry, claim, publication.as_mut(), None, Some(waker))
            })
        };
        let std::task::Poll::Ready(claimed) = claimed else {
            return std::task::Poll::Pending;
        };
        let RootPollState::Cold { root, operation } =
            std::mem::replace(&mut self.state, RootPollState::Delivered)
        else {
            unreachable!("the state was just matched")
        };
        let ColdRoot {
            claim,
            entry,
            publication,
        } = *root;
        drop(claim);
        let flow = match entry.key() {
            SemanticQueryKey::FlowReturn(flow) => Some((**flow).clone()),
            _ => None,
        };
        let read = lender.within_execution_scopes(family, || {
            let _nested = lender.nested_decision_query();
            lender.finish_cold_query(entry, claimed)
        });
        drop(operation);
        let read = self.deliver(read, true);
        if let Some(flow) = flow {
            match publication.flatten() {
                Some(publication) => {
                    lender.flow_return_drain_completed_members(&flow, &publication)
                }
                None => lender.relation_abort_completed_members(),
            }
        }
        std::task::Poll::Ready(read)
    }

    /// The query's semantic task, while it runs on the continuation runtime
    /// and has not delivered: `Send`, so its one exclusive poller may
    /// advance it on another thread.
    pub fn task(&mut self) -> Option<&mut SemanticTask<'d, 'a, C>> {
        match &mut self.state {
            RootPollState::Driven { root, .. } => root.task(),
            _ => None,
        }
    }

    /// What the synchronous entry delivers for a root's read: re-rooted in
    /// the scope it is delivered in, when the root ran as an operation.
    fn deliver(&self, read: ValueRead, scoped: bool) -> ValueRead {
        if scoped {
            self.lender.redeliver_read(read)
        } else {
            read
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> Drop for SemanticQueryPoll<'_, '_, C> {
    fn drop(&mut self) {
        // A root released while pending gives its request state back — its
        // task releasing every frame, flight and wait it holds, or its claim
        // withdrawing its wait — within the execution's scopes, then closes
        // its operation.
        match std::mem::replace(&mut self.state, RootPollState::Delivered) {
            RootPollState::Driven { root, operation } => match operation {
                Some(operation) => {
                    self.lender
                        .within_execution_scopes(root.key.tag(), move || drop(root));
                    drop(operation);
                }
                None => drop(root),
            },
            RootPollState::Cold { root, operation } => {
                let flow = matches!(root.entry.key(), SemanticQueryKey::FlowReturn(_));
                self.lender
                    .within_execution_scopes(root.entry.key().tag(), move || drop(root));
                drop(operation);
                // A released machinery root publishes nothing for the
                // members its evaluation completed.
                if flow {
                    self.lender.relation_abort_completed_members();
                }
            }
            RootPollState::Unstarted(_) | RootPollState::Delivered => {}
        }
    }
}

// The entry is never structurally pinned: polling it moves nothing it owns.
impl<C: crate::resolver_core::ResolverCapabilities> Unpin for SemanticQueryPoll<'_, '_, C> {}

impl<'d, 'a: 'd, C: crate::resolver_core::ResolverCapabilities> std::future::Future
    for SemanticQueryPoll<'d, 'a, C>
{
    type Output = ValueRead;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        SemanticQueryPoll::poll(self.get_mut(), cx.waker())
    }
}

impl<'p, 'a, C: crate::resolver_core::ResolverCapabilities> QueryProgram<'p, 'a, C> {
    /// Claim `key` as another task's producer, when the test holds the
    /// demands a call's walk suspends on.
    #[cfg(any(test, feature = "test-support"))]
    fn hold_call_need_for_tests(&mut self, key: &SemanticQueryKey, site: TupleSplicePosition) {
        let Some(held) = self.held_call_needs.as_mut() else {
            return;
        };
        let dispatch = &self.dispatch;
        let other = self.graph.task_registry_for_tests().register_task();
        let mut capture = ReadCapture::default();
        capture.set_checking_site(site);
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
            site: self.dispatch.checking_site.get(),
            key: key.clone(),
            producer: Some(producer),
            tracer: None,
            taint: BuildLocalTaint {
                produces: super::FrameProduction::of(key, self.dispatch.checking_site.get()),
                ..BuildLocalTaint::default()
            },
            build: None,
            active: None,
            depth: self.child_depth,
            recording: false,
            computed_at: self.dispatch.workspace_edit(),
            awaited_loads: Vec::new(),
            failed_loads: Vec::new(),
        }))
    }
}

impl<'p, 'a: 'p, C: crate::resolver_core::ResolverCapabilities> Program
    for QueryProgram<'p, 'a, C>
{
    type Demand = QueryDemand;
    type Frame = QueryFrame<'p, 'a, C>;
    type Value = QueryDelivery;
    type Failure = ();
    type Role = ();
    type Subscription = PendingWait<'p>;

    fn start(&mut self, demand: &QueryDemand) -> Start<Self> {
        let dispatch = &self.dispatch;
        let key = &demand.key;
        let _site = dispatch.checking_site_scope(demand.site);
        // The needed key's own frame runs one level below the needing one.
        let nesting = Nesting::Frame {
            depth: self.child_depth,
        };
        let mut attempt = match self.retries.remove(demand) {
            Some(attempt) => attempt,
            None => match dispatch.enter_query_frame(key, self.child_depth) {
                Ok(attempt) => attempt,
                Err(delivery) => return Start::Answer(Ok(*delivery)),
            },
        };
        // A result the request already completed answers before any claim: a
        // claim installs a flight other tasks may join, so it is taken only
        // to produce.
        if let Some((read, carrier)) = dispatch.own_published_read(key, nesting) {
            return Start::Answer(Ok(QueryDelivery {
                site: dispatch.checking_site.get(),
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
        // Only dependencies can complete outside the family memo. A root
        // retains the canonical candidate its synchronous entry would publish.
        // A relation keeps its component at its own synchronous root. A key
        // another task is producing is joined through the claim below, never
        // computed beside it, as its synchronous entry joins it.
        if self.child_depth > ROOT_FRAME_DEPTH
            && !matches!(key, SemanticQueryKey::Relate { .. })
            && dispatch.completes_nested_request_locally(key)
            && !self.graph.has_flight(&attempt)
        {
            dispatch
                .request_local_open
                .borrow_mut()
                .insert((key.clone(), demand.site));
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
            Claim::Subscribed(subscription) => Start::Subscribe(PendingWait::Claim(PendingClaim {
                demand: demand.clone(),
                subscription,
                attempt: Some(attempt),
                nesting,
            })),
            Claim::Produce(lease) => {
                Start::Produce(self.producing_frame(key, FrameProducer::Lease(lease)))
            }
        }
    }

    fn step(&mut self, frame: &mut Self::Frame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        match frame {
            QueryFrame::Instantiate(frame) => {
                self.child_depth = frame.depth + 1;
                let mut step =
                    self.dispatch
                        .step_instantiate_frame(frame, delivery, self.suspends_loads);
                // The site the need is checked at is part of its identity:
                // the claim its read entered with when there is one, else
                // the site the needing evaluation checks at.
                if let EvalStep::Need { demand, .. } = &mut step {
                    let mut site = match &frame.build {
                        Some(FrameBuild::Typeof(build)) => build.checking_site(),
                        _ if matches!(frame.key, SemanticQueryKey::TypeOf { .. }) => {
                            TupleSplicePosition::Type
                        }
                        _ => frame.site,
                    };
                    if matches!(demand.key, SemanticQueryKey::ResolveCall(_)) {
                        site = TupleSplicePosition::Expression;
                    }
                    // A decision's need entered already: its claim continues
                    // when the runtime starts it.
                    let claim = match frame.build.as_mut() {
                        Some(FrameBuild::Conditional(build)) => build.claim.take(),
                        Some(FrameBuild::Call(build)) => {
                            let claim = build.claim.take();
                            if let Some(claim) = &claim {
                                site = claim.checking_site();
                            }
                            #[cfg(any(test, feature = "test-support"))]
                            self.hold_call_need_for_tests(&demand.key, site);
                            claim
                        }
                        Some(FrameBuild::Flow(build)) => {
                            let claim = build.claim.take();
                            if let Some(claim) = &claim {
                                site = claim.checking_site();
                            }
                            // Held only while a contextual call's route waits
                            // on it, collector included.
                            #[cfg(any(test, feature = "test-support"))]
                            if !build.routes.is_empty() {
                                self.hold_call_need_for_tests(&demand.key, site);
                            }
                            claim
                        }
                        _ => None,
                    };
                    if let Some(claim) = &claim {
                        site = claim.checking_site();
                    }
                    demand.site = site;
                    if let Some(claim) = claim {
                        self.retries.insert(demand.clone(), claim);
                    }
                }
                step
            }
        }
    }

    fn close_cycle(&mut self, demand: &QueryDemand, _role: ()) -> Outcome<Self> {
        // The open demand's producer is this task's own: the memo answers
        // the claim exactly as it answers a synchronous re-entry — a warm
        // result where one validates, else the same-path carrier. The claim
        // is the open demand's, at its site.
        let dispatch = &self.dispatch;
        let key = &demand.key;
        let _site = dispatch.checking_site_scope(demand.site);
        let nesting = Nesting::Frame {
            depth: self.child_depth,
        };
        let mut carrier = None;
        let mut capture = ReadCapture::deferring_carrier(&mut carrier);
        capture.set_checking_site(demand.site);
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

    fn reusable(&self, demand: &QueryDemand, delivery: &QueryDelivery) -> bool {
        // Every answer to a demand is produced at the demand's site, so a
        // kept result answers only needs checked where it was computed.
        verter_debug_assert!(
            delivery.site == demand.site,
            "a demand is answered at its own checking site"
        );
        // Exactly what the memo would serve warm: a complete, cacheable
        // result naming no retired kernel epoch. A partial or non-cacheable
        // one is evaluated again, as a synchronous re-entry would evaluate
        // it; one whose epoch was retired after it completed is read again,
        // as the request's kept completions are.
        !delivery.read.cache_suppress
            && !delivery.read.result_is_partial()
            && self
                .dispatch
                .graph()
                .serves_stored_value(&delivery.read.value)
    }

    fn poll_subscription(
        &mut self,
        pending: &mut PendingWait<'p>,
        waker: &std::task::Waker,
    ) -> std::task::Poll<External<Self>> {
        let pending = match pending {
            PendingWait::Claim(pending) => pending,
            // A loaded file is read by the frame that waited on it.
            PendingWait::Load(wait) => {
                return wait.load.poll(waker).map(|outcome| {
                    let set = wait.ended.set(outcome);
                    verter_debug_assert!(set.is_ok(), "a load ends once");
                    External::Restart
                })
            }
        };
        let dispatch = &self.dispatch;
        verter_debug_assert!(
            pending
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.checking_site() == pending.demand.site),
            "a pending claim continues at its demand's site"
        );
        let _site = dispatch.checking_site_scope(pending.demand.site);
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
        let demand = pending.demand.clone();
        let key = &demand.key;
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
                self.retries.insert(demand, attempt);
                return std::task::Poll::Ready(External::Restart);
            }
        }
        std::task::Poll::Ready(match joined {
            Joined::Read(read) => {
                External::Complete(Ok(dispatch.answered(key, read, carrier, nesting)))
            }
            Joined::Recursive(recursion) => {
                External::Complete(Ok(dispatch.recursion_delivery(key, recursion)))
            }
            Joined::Retry => {
                self.retries.insert(demand, attempt);
                External::Restart
            }
        })
    }

    fn stop(&self) -> Option<()> {
        self.dispatch.cancellation.is_cancelled().then_some(())
    }

    fn wake_on_stop(&mut self, waker: &std::task::Waker) {
        // The token `stop` selects on this poll's thread, with the task's job
        // and request installed: the same selection decides the stop.
        let Some(token) =
            crate::resolver_core::request_ports::CancellationCheckpoint::selected_token()
        else {
            self.stop_wake = None;
            return;
        };
        let subscription = match &mut self.stop_wake {
            Some(held) if held.token().same_source(&token) => held,
            slot => slot.insert(token.subscribe()),
        };
        if subscription.poll(waker).is_ready() {
            waker.wake_by_ref();
        }
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
                .remove(&(frame.key.clone(), frame.site));
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether a read of `key` nested in another computation — a demand a
    /// frame needs, or a read nested in a synchronous build — completes for
    /// this request alone, outside the family memo's claim and admission
    /// ([`super::completes_request_locally`]): outside every forced operand,
    /// outside every checker obligation unless the family completes inside
    /// one ([`super::completes_request_locally_in_obligations`]), and not
    /// already produced on this task's path.
    pub(super) fn completes_nested_request_locally(&self, key: &SemanticQueryKey) -> bool {
        super::completes_request_locally(key)
            && self.active_operand_evidence.borrow().is_empty()
            && (super::completes_request_locally_in_obligations(key)
                || self.dispatch_txn.borrow().obligations.decides_root())
            && !self
                .graph()
                .is_same_path_claim_at(key, self.checking_site.get())
    }

    /// Whether a build's dependency read of `key` can be its frame's need:
    /// a key the synchronous entry admits as it is. An entry that first
    /// normalizes its subject — a carrier, a scope-less subject read through
    /// its apparent wrapper or its apparent signatures — or canonicalizes
    /// the key itself keeps the prelude that entry records, which a frame's
    /// need does not carry; such a read is made where the synchronous build
    /// makes it.
    pub(super) fn reads_as_frame_need(&self, key: &SemanticQueryKey) -> bool {
        !matches!(
            key,
            SemanticQueryKey::Relate { .. }
                | SemanticQueryKey::ProjectMember { .. }
                | SemanticQueryKey::IndexedAccess { .. }
                | SemanticQueryKey::ReduceUnion { .. }
        ) && !self.key_subject_is_carrier(key)
            && !self.key_reads_apparent_wrapper(key)
            && !self.key_reads_apparent_signatures(key)
    }

    /// Whether `key` runs on the continuation runtime when it is evaluated
    /// outside any drive: an instantiation or a `typeof` read for its value
    /// alone, a conditional/call evaluated as a machinery root, or a family
    /// built as a continuation (a macro payload, an intersection reduction)
    /// read outside every open obligation. A `typeof`
    /// whose value's type lowers from its authored value
    /// position (its annotation, its inferred initializer, its const-object
    /// shape) builds as a frame, so a chain of values each typed by the one
    /// before is a chain of frames; any other `typeof` completes in the
    /// frame's first step.
    pub(super) fn drives_on_the_runtime(&self, key: &SemanticQueryKey) -> bool {
        let family = match key {
            SemanticQueryKey::Instantiate(_) | SemanticQueryKey::TypeOf { .. } => true,
            // A family built as a continuation drives from an entry outside
            // every open checker obligation, as a machinery root does; read
            // inside one, it is built in place through the same
            // continuation, its reads made where the obligation stands.
            key @ (SemanticQueryKey::ResolveMacroPayload { .. }
            | SemanticQueryKey::ReduceIntersection { .. }
            | SemanticQueryKey::ResolveOverloadSet { .. }
            | SemanticQueryKey::ApparentType { .. }
            | SemanticQueryKey::ResolveClassSurface { .. }) => {
                super::family_continuation::builds_as_continuation(key)
                    && self.reads_as_frame_need(key)
                    && self
                        .dispatch_txn
                        .try_borrow()
                        .is_ok_and(|txn| txn.obligations.decides_root())
            }
            SemanticQueryKey::Conditional { .. } | SemanticQueryKey::ResolveCall(_) => {
                self.dispatch_txn.borrow().obligations.decides_root()
            }
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
    /// of an instantiation or a `typeof`, which blocks this thread on the
    /// same root the production poll ([`SemanticQueryPoll`]) advances. A
    /// warm result answers without the runtime.
    ///
    /// Never inlined: the root and its lent task live in this frame alone,
    /// off the stack of every nested query entry that does not drive.
    #[inline(never)]
    pub(super) fn drive_query(&self, key: SemanticQueryKey) -> ValueRead {
        match DrivenRoot::start(self, key, false) {
            RootStart::Answered(read) => read,
            RootStart::Driven(mut root) => {
                crate::semantic_execution::block_on(|waker| root.poll(waker))
            }
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
        let mut carrier = None;
        // The runtime holds this entry on the heap: it enters no nested
        // native query level.
        let entered = self.enter_query(key, Some(&mut carrier), Nesting::Frame { depth });
        entered.map(|(attempt, _)| attempt).map_err(|read| {
            Box::new(QueryDelivery {
                read,
                carrier,
                site: self.checking_site.get(),
            })
        })
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
        crate::observe_request!(C::Observe => on self.observation.request.as_deref() => |ctx| { ctx.record_dispatched_query_tag(key.tag()) });
        let (_connected_guard, preexisting_trip) = self.enter_connected_demand(false);
        let query_depth_guard =
            self.charge_query_entry(key, preexisting_trip, nesting.query_levels() > 0)?;
        let mut capture = match carrier {
            Some(slot) => ReadCapture::deferring_carrier(slot),
            None => ReadCapture::default(),
        };
        capture.set_checking_site(self.checking_site.get());
        let mut attempt = match self.graph().begin_query_claim(
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
        attempt.set_checking_site(self.checking_site.get());
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
            site: self.checking_site.get(),
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
            site: self.checking_site.get(),
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
        let QueryDelivery { read, carrier, .. } = delivery;
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
    /// answer to the read it waited on, then advance the decision's retained
    /// stage with every answer the frame holds. A read none of them answers
    /// is the frame's next need; only its unfinished stage is discarded.
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
            build.answers.insert((answered, delivery.site), delivery);
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
            resumes: std::mem::take(&mut build.resumes),
            needs: SeamNeeds::default(),
            depth,
            nested: 0,
            materialized_children: false,
        }));
        verter_debug_assert!(installed.is_none(), "one decision runs per step");
        let output = self.build_conditional_with_state(
            *check,
            *extends,
            *true_branch,
            *false_branch,
            *distributive,
            pending.clone(),
            &mut build.decision,
        );
        let seam = self
            .decision_seam
            .take()
            .expect("the decision's seam stays installed for its run");
        build.answers = seam.answers;
        build.resumes = seam.resumes;
        // Each decision stage owns its speculative observation scope. A
        // finished stage has already joined this frame's retained taint;
        // discarding the whole step here would lose that evidence.
        if !matches!(
            &output.result,
            QueryResult::Error(QueryError::ExecutionAborted(_))
        ) {
            let mut needs = seam.needs.into_iter();
            if let Some((need, claim)) = needs.next() {
                build.awaiting = Some(need.clone());
                build.claim = Some(claim);
                build.pending = needs.collect();
                return InstantiatePoll::Need(need);
            }
        }
        InstantiatePoll::Done(Box::new(output))
    }

    pub(super) fn aborted_build_output(
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
    /// Nested conditional decisions, including distributed members and
    /// inferred winners, and the final union reduction use the same delivery
    /// boundary. Reduction enters once as a child demand, including when its
    /// producer is already held; consuming that delivery does not re-enter
    /// cold-query admission.
    /// `None` when no decision runs here, or for any other query.
    pub(super) fn read_at_decision_seam(&self, key: &SemanticQueryKey) -> Option<ValueRead> {
        if !matches!(
            key,
            SemanticQueryKey::Instantiate(_)
                | SemanticQueryKey::TypeOf { .. }
                | SemanticQueryKey::Conditional { .. }
                | SemanticQueryKey::ReduceUnion { .. }
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
            if matches!(
                key,
                SemanticQueryKey::Conditional { .. } | SemanticQueryKey::ReduceUnion { .. }
            ) && !seam.materialized_children
            {
                return None;
            }
            let answered = seam
                .answers
                .get(&(key.clone(), self.checking_site.get()))
                .cloned();
            let read = match answered {
                Some(delivery) => Seam::Answered(delivery),
                None if seam.nested > 0 => return None,
                None if seam.needs.contains(key, self.checking_site.get()) => Seam::Waiting,
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
                        seam.answers
                            .insert((key.clone(), delivery.site), (*delivery).clone());
                    });
                    self.read_delivery(*delivery, nesting)
                }
                Ok(claim) => {
                    self.with_decision_seam(|seam| seam.needs.push(key.clone(), claim));
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

    /// The caller supplies the complete materialized key of a member,
    /// inferred winner or final union reduction. Other probes must retain
    /// their checker context in their owning continuation before suspending.
    pub(super) fn conditional_child_queries(&self) -> ConditionalChildQueries<'_> {
        let restore = self.decision_seam.borrow_mut().as_mut().map(|seam| {
            let previous = std::mem::replace(&mut seam.materialized_children, true);
            (&self.decision_seam, previous)
        });
        ConditionalChildQueries { restore }
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

    /// The seam a read of `key` made here joins another task's producer
    /// through: the running decision's or flow evaluation's own read of a
    /// query outside the families its seam answers. Such a read meeting
    /// another task's producer becomes the frame's need instead of waiting
    /// the producer out on the polling thread; a read it evaluates in place
    /// is unaffected. `None` for a read in place, outside any seam, or of a
    /// family a frame owns: an instantiation, a `typeof`, a conditional — a
    /// probe of one keeps its temporary checker context — a call or a flow
    /// return.
    pub(super) fn join_seam_for(&self, key: &SemanticQueryKey) -> Option<JoinSeam> {
        if matches!(
            key,
            SemanticQueryKey::Instantiate(_)
                | SemanticQueryKey::TypeOf { .. }
                | SemanticQueryKey::Conditional { .. }
                | SemanticQueryKey::ResolveCall(_)
                | SemanticQueryKey::FlowReturn(_)
        ) {
            return None;
        }
        if self.deciding_on_the_runtime() {
            Some(JoinSeam::Decision)
        } else if self.flow_seam_reads_here() {
            Some(JoinSeam::Flow)
        } else {
            None
        }
    }

    /// The read of `key` at `seam` the frame already settled: the answer a
    /// delivery holds, or the typed gap while it is still one of the run's
    /// needs. `None` when the run has neither, and the read claims.
    pub(super) fn read_joined_at_seam(
        &self,
        seam: JoinSeam,
        key: &SemanticQueryKey,
    ) -> Option<ValueRead> {
        let site = self.checking_site.get();
        let probe =
            |answers: &FxHashMap<(SemanticQueryKey, TupleSplicePosition), QueryDelivery>,
             needs: &SeamNeeds| {
                (
                    answers.get(&(key.clone(), site)).cloned(),
                    needs.contains(key, site),
                )
            };
        let (answer, waiting) = match seam {
            JoinSeam::Decision => {
                let installed = self.decision_seam.borrow();
                let seam = installed.as_ref()?;
                probe(&seam.answers, &seam.needs)
            }
            JoinSeam::Flow => {
                let installed = self.flow_seam.borrow();
                let seam = installed.as_ref()?;
                probe(&seam.answers, &seam.needs)
            }
        };
        match answer {
            // The read is the one nested query level its synchronous entry
            // enters; only who produced it differs.
            Some(delivery) => Some(self.read_delivery(delivery, Nesting::Entered)),
            None if waiting => Some(self.suspended_decision_read()),
            None => None,
        }
    }

    /// Make a read that met another task's producer the need of the frame
    /// whose run made it, its claim kept for the runtime to join again, and
    /// read the typed gap nothing built from it keeps.
    pub(super) fn suspend_joined_read(
        &self,
        seam: JoinSeam,
        key: SemanticQueryKey,
        claim: ClaimAttempt,
    ) -> ValueRead {
        match seam {
            JoinSeam::Decision => self.with_decision_seam(|seam| seam.needs.push(key, claim)),
            JoinSeam::Flow => self.with_flow_seam(|seam| seam.needs.push(key, claim)),
        }
        self.suspended_decision_read()
    }

    /// Begin a family's build in its frame: as a continuation, its first
    /// stage, or — for a family that builds in one step — its whole build.
    /// The build is refused as the synchronous producer refuses it when the
    /// request's budget is spent.
    #[inline(never)]
    fn start_family_frame(
        &self,
        key: &SemanticQueryKey,
        build: &mut Option<FrameBuild>,
        suspends_loads: bool,
    ) -> FramePoll<'a> {
        if !super::family_continuation::builds_as_continuation(key) {
            return FramePoll::Done(Box::new(self.build_cold_query_family(key)));
        }
        if let Some(refusal) = self.cold_build_budget_refusal(key) {
            return FramePoll::Done(Box::new(self.family_value_output(key, refusal)));
        }
        let step = self
            .begin_family_build(key)
            .expect("a continuation family begins its build");
        self.advance_family_frame(key, build, step, suspends_loads)
    }

    /// Resume a family's build with the delivery of the read it waited on.
    #[inline(never)]
    fn resume_family_frame(
        &self,
        key: &SemanticQueryKey,
        build: &mut Option<FrameBuild>,
        depth: u32,
        delivery: QueryDelivery,
        suspends_loads: bool,
    ) -> FramePoll<'a> {
        let Some(FrameBuild::Family(family)) = build.take() else {
            unreachable!("a family frame resumes its own build")
        };
        // A read named from kernel handles retired while the frame waited
        // answers nothing: it is dropped unread, its rails and receipt never
        // the build's, and the build reads its source again.
        let read = if family.awaits_retired_read(&self.continuation_stage()) {
            drop(delivery);
            super::family_continuation::retired_read()
        } else {
            self.read_frame_delivery(FrameDelivery {
                delivery,
                depth: depth + 1,
            })
        };
        let step = self.resume_family_build(*family, read);
        self.advance_family_frame(key, build, step, suspends_loads)
    }

    /// Carry a family build on from `step`: its output, or the query it
    /// reads next as the frame's need, the build retained by the frame. A
    /// read whose entry first normalizes its subject keeps the prelude that
    /// entry records, which a need does not carry: it is read where the
    /// synchronous build reads it, and the build goes on. A file it reads
    /// next is read at once when no load is needed; on a production poll's
    /// drive the frame otherwise waits on the file's load, the build
    /// retained, while the synchronous entry's drive loads it in place.
    fn advance_family_frame(
        &self,
        key: &SemanticQueryKey,
        build: &mut Option<FrameBuild>,
        mut step: FamilyStep,
        suspends_loads: bool,
    ) -> FramePoll<'a> {
        loop {
            match step {
                FamilyStep::Load(canonical, family) => {
                    if suspends_loads {
                        if let IndexedReadiness::Loading(load) =
                            self.ctx.begin_indexed_load(&canonical)
                        {
                            let ended = LoadEnded::default();
                            *build = Some(FrameBuild::FamilySource(
                                family,
                                canonical,
                                Arc::clone(&ended),
                            ));
                            return FramePoll::Await(LoadWait { load, ended });
                        }
                    }
                    let serve = self.ctx.ensure_indexed_ready_serve(&canonical);
                    step = self.resume_family_source(*family, serve);
                }
                FamilyStep::Global(read, family) => {
                    step = self.resume_family_global(*family, read);
                }
                FamilyStep::Declarations(family) => {
                    step = self.resume_family_declarations(*family);
                }
                FamilyStep::Done(output) => {
                    return FramePoll::Done(Box::new(self.family_value_output(key, *output)))
                }
                FamilyStep::Need(need, family) if self.reads_as_frame_need(&need) => {
                    *build = Some(FrameBuild::Family(family));
                    return FramePoll::Need(need);
                }
                FamilyStep::Need(need, family) => {
                    let read = self.read_family_need_in_place(need, &family);
                    step = self.resume_family_build(*family, read);
                }
            }
        }
    }

    /// The next load a frame's build would perform as it begins, when the
    /// frame runs on a production poll's drive: its own file — an
    /// instantiation's declaring file, a `typeof`'s value file — while it
    /// loads, then each file that file's prepared declaration bundle loads
    /// as it routes an import. `None` once none of them is loading, for a
    /// key naming no file, or on the synchronous entry's drive, whose build
    /// performs those loads in place as its external driver.
    ///
    /// Each file waited on joins `awaited` with the cell its outcome ends
    /// in, and is never waited on again: a load that failed is consumed by
    /// the build, whose first load of the file answers with that outcome as
    /// a synchronous build's own failed load answers. What the check reads
    /// is observed in a scope of its own and dropped; the build observes
    /// what it reads itself.
    fn frame_source_load(
        &self,
        key: &SemanticQueryKey,
        awaited: &mut Vec<(Arc<str>, Option<LoadEnded>)>,
        suspends_loads: bool,
    ) -> Option<LoadWait<'a>> {
        if !suspends_loads {
            return None;
        }
        let canonical: &str = match key {
            SemanticQueryKey::Instantiate(key) => key.base().defining_canonical.as_ref(),
            SemanticQueryKey::TypeOf { value_root, .. } => {
                value_root.root.scope.canonical_id.as_ref()
            }
            _ => return None,
        };
        if crate::semantic_query::is_non_file_base(canonical) {
            return None;
        }
        if !awaited.iter().any(|(file, _)| file.as_ref() == canonical) {
            if let IndexedReadiness::Loading(load) = self.ctx.begin_indexed_load(canonical) {
                let ended = LoadEnded::default();
                awaited.push((Arc::from(canonical), Some(Arc::clone(&ended))));
                return Some(LoadWait { load, ended });
            }
        }
        let files: Vec<Arc<str>> = awaited.iter().map(|(file, _)| Arc::clone(file)).collect();
        let source = crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx);
        let tracer = crate::resolver_core::resolver_context::OwnedFactTracer::new(
            source.live_basis(),
            source.fact_policy(),
        );
        let pending = {
            let _tracer = tracer.install();
            self.observe_read(|| self.ctx.begin_prepared_bundle_load(canonical, &files))
                .discard()
                .ok()
                .flatten()
        }?;
        let ended = LoadEnded::default();
        awaited.push((pending.canonical, Some(Arc::clone(&ended))));
        Some(LoadWait {
            load: pending.load,
            ended,
        })
    }

    /// Take the outcomes of a frame's awaited loads that ended since its last
    /// step: each that ended without integrating its file joins `failed`,
    /// for the build to consume.
    fn take_awaited_load_ends(
        awaited: &mut [(Arc<str>, Option<LoadEnded>)],
        failed: &mut crate::resolver_core::request_ports::ConsumedLoadEnds,
    ) {
        for (file, ended) in awaited {
            let Some(outcome) = ended.as_ref().and_then(|ended| ended.get().copied()) else {
                continue;
            };
            *ended = None;
            if let IndexedLoadOutcome::NotIntegrated(end) = outcome {
                failed.push((Arc::clone(file), end));
            }
        }
    }

    /// One step of an instantiation's frame: its taint frame and tracer
    /// installed, it begins or resumes its build and runs it until the
    /// build needs another instantiation or completes.
    fn step_instantiate_frame<'p>(
        &self,
        frame: &mut InstantiateFrame<'p, 'a, C>,
        delivery: Option<Outcome<QueryProgram<'p, 'a, C>>>,
        suspends_loads: bool,
    ) -> EvalStep<QueryProgram<'p, 'a, C>>
    where
        'a: 'p,
    {
        let _site = self.checking_site_scope(frame.site);
        // A build whose files are still loading waits on each load before
        // it begins: nothing of it has run, so the step the last load
        // restarts begins it once, over loaded files.
        Self::take_awaited_load_ends(&mut frame.awaited_loads, &mut frame.failed_loads);
        if frame.build.is_none() {
            if let Some(load) =
                self.frame_source_load(&frame.key, &mut frame.awaited_loads, suspends_loads)
            {
                return EvalStep::Await(PendingWait::Load(load));
            }
        }
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
        // The step's partiality is its frame's own, as a synchronous build's
        // is: it reaches the frame's output through its taint, and a
        // consumer through the delivery it reads, never the native caller
        // whose drive happens to be stepping the frame.
        let completeness = crate::request_context::ColdComputeCompletenessScope::enter();
        let delivery = delivery.map(|outcome| match outcome {
            Ok(delivery) => delivery,
            Err(()) => unreachable!("a stopped drive delivers nothing"),
        });
        // A load the frame waited on that ended without integrating its file
        // answers the build's first load of it: the build consumes the
        // wait's terminal outcome instead of submitting the load again.
        let mut failed_loads = std::mem::take(&mut frame.failed_loads);
        let poll: FramePoll<'a> =
            crate::resolver_core::request_ports::with_consumed_load_ends(&mut failed_loads, || {
                match frame.build.as_mut() {
                    Some(FrameBuild::Instantiate(build)) => self
                        .drain_instantiate(
                            build,
                            delivery
                                .map(|delivery| self.consume_delivery(delivery, frame.depth + 1)),
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
                    // A family continuation resumes from the stage it retained with
                    // the read it waited on; no completed stage runs again.
                    Some(FrameBuild::Family(_)) => {
                        let delivery =
                            delivery.expect("a family build resumes with the read it named");
                        self.resume_family_frame(
                            &frame.key,
                            &mut frame.build,
                            frame.depth,
                            delivery,
                            suspends_loads,
                        )
                    }
                    // A family build whose file's load ended reads the file from
                    // that terminal outcome, once: an integrated file is served as
                    // loaded, and a load that ended without integrating it leaves
                    // the build no input for it — the input a synchronous serve
                    // answers after the same failed load. The load is never begun
                    // again.
                    Some(FrameBuild::FamilySource(..)) => {
                        let Some(FrameBuild::FamilySource(family, canonical, ended)) =
                            frame.build.take()
                        else {
                            unreachable!("the build was just matched")
                        };
                        let serve = match ended.get() {
                            Some(IndexedLoadOutcome::Integrated) => {
                                self.ctx.ensure_indexed_ready_serve(&canonical)
                            }
                            Some(IndexedLoadOutcome::NotIntegrated(_)) => None,
                            None => unreachable!("a frame waiting on a load resumes once it ended"),
                        };
                        let step = self.resume_family_source(*family, serve);
                        self.advance_family_frame(
                            &frame.key,
                            &mut frame.build,
                            step,
                            suspends_loads,
                        )
                    }
                    None => self.begin_frame_build(
                        &frame.key,
                        frame.depth,
                        &mut frame.active,
                        &mut frame.build,
                        suspends_loads,
                    ),
                }
            });
        frame.failed_loads = failed_loads;
        let step = crate::request_context::current_cold_compute_completeness();
        if step.is_partial() {
            self.fold_into_top_build_local_taint_with(true, false, step.reasons());
        }
        completeness.discard();
        drop(installed);
        drop(taint);
        match poll {
            FramePoll::Await(load) => EvalStep::Await(PendingWait::Load(load)),
            // The program settles the need's site from the read's claim.
            FramePoll::Need(key) => EvalStep::Need {
                demand: QueryDemand {
                    key,
                    site: frame.site,
                },
                role: (),
            },
            FramePoll::Done(output) => {
                // Finishing the build left the declaration it entered.
                frame.active = None;
                EvalStep::Complete(self.complete_instantiate_frame(frame, *output))
            }
        }
    }

    /// The first step of a frame's build: it opens as a synchronous cold
    /// build opens and begins the build its key names.
    fn begin_frame_build(
        &self,
        frame_key: &SemanticQueryKey,
        depth: u32,
        active: &mut Option<(super::InstantiateIdentity, Arc<[SemanticNodeId]>)>,
        frame_build: &mut Option<FrameBuild>,
        suspends_loads: bool,
    ) -> FramePoll<'a> {
        self.open_cold_build(None);
        // A frame produces its key as a synchronous build does.
        #[cfg(any(test, feature = "test-support"))]
        if let Some(hook) = self.graph().produce_hook_for_tests() {
            hook(frame_key, &|nested| {
                let _ = self.execute_via_cold_build_helper(nested);
            });
        }
        match frame_key {
            SemanticQueryKey::Instantiate(key) => {
                let refusal = self
                    .cold_build_budget_refusal(frame_key)
                    .or_else(|| self.instantiation_budget_refusal(depth));
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
                        *active = Some((build.active_identity.clone(), Arc::clone(&build.args)));
                        let FrameBuild::Instantiate(build) =
                            frame_build.insert(FrameBuild::Instantiate(build))
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
                let begun = match self.cold_build_budget_refusal(frame_key) {
                    Some(refusal) => TypeofStart::Done(Box::new(refusal)),
                    None => self.begin_typeof(&value_root.root, path, context.projection_reduction),
                };
                match begun {
                    TypeofStart::Done(output) => InstantiatePoll::Done(output).into(),
                    TypeofStart::Build(build) => {
                        let FrameBuild::Typeof(build) =
                            frame_build.insert(FrameBuild::Typeof(build))
                        else {
                            unreachable!("the build was just inserted")
                        };
                        self.drain_typeof(build, None).into()
                    }
                }
            }
            SemanticQueryKey::Conditional { .. } => {
                match self.cold_build_budget_refusal(frame_key) {
                    Some(refusal) => InstantiatePoll::Done(Box::new(refusal)).into(),
                    None => {
                        let FrameBuild::Conditional(build) =
                            frame_build.insert(FrameBuild::Conditional(Box::default()))
                        else {
                            unreachable!("the build was just inserted")
                        };
                        self.drain_conditional(frame_key, build, None, depth + 1)
                            .into()
                    }
                }
            }
            SemanticQueryKey::ResolveCall(_) => match self.cold_build_budget_refusal(frame_key) {
                Some(refusal) => InstantiatePoll::Done(Box::new(refusal)).into(),
                None => {
                    let FrameBuild::Call(build) =
                        frame_build.insert(FrameBuild::Call(Box::default()))
                    else {
                        unreachable!("the build was just inserted")
                    };
                    self.drain_call(frame_key, build, None, depth + 1)
                }
            },
            SemanticQueryKey::FlowReturn(_) => {
                let FrameBuild::Flow(build) = frame_build.insert(FrameBuild::Flow(Box::default()))
                else {
                    unreachable!("the build was just inserted")
                };
                self.drain_flow(frame_key, build, None, depth + 1)
            }
            // A family built as a continuation needs each query its
            // stages read; any other dependency of a family without
            // frames of its own — a read a decision or flow
            // evaluation joined at its seam whose producer went away
            // before the join answered — builds in one step, as its
            // synchronous producer builds it.
            _ => self.start_family_frame(frame_key, frame_build, suspends_loads),
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
            // enters no family memo; the request keeps what it may serve
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
                    site: self.checking_site.get(),
                    read: self.attribute_query_read(
                        &frame.key,
                        true,
                        read,
                        &CarrierNormalizationPrelude::none(),
                    ),
                    carrier,
                };
                self.request_local_open
                    .borrow_mut()
                    .remove(&(frame.key.clone(), frame.site));
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
            site: self.checking_site.get(),
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
    ) -> FramePoll<'a> {
        let SemanticQueryKey::ResolveCall(call) = key else {
            unreachable!("a call frame produces a call resolution")
        };
        if let Some(delivery) = delivery {
            let answered = build
                .awaiting
                .take()
                .expect("a delivery answers the demand the walk waited on");
            build.answers.insert((answered, delivery.site), delivery);
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
            resumes: std::mem::take(&mut build.resumes),
            needs: SeamNeeds::default(),
            depth,
        }));
        verter_debug_assert!(installed.is_none(), "one call walk runs per step");
        let step = self.step_resolve_call_build(call, idx);
        let seam = self
            .call_seam
            .take()
            .expect("the walk's seam stays installed for its run");
        build.answers = seam.answers;
        build.resumes = seam.resumes;
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
        let (answers, resumes, depth) = {
            let mut seam = self.call_seam.borrow_mut();
            let seam = seam
                .as_mut()
                .expect("an operand demand runs inside its frame's walk");
            (
                std::mem::take(&mut seam.answers),
                std::mem::take(&mut seam.resumes),
                seam.depth,
            )
        };
        let outer = self.decision_seam.replace(Some(DecisionSeam {
            answers,
            resumes,
            needs: SeamNeeds::default(),
            depth,
            nested: 0,
            materialized_children: false,
        }));
        let demanded = self.observe_read(|| {
            let _ = self.demand_relation_operands(operands);
        });
        let _ = demanded.discard();
        let DecisionSeam {
            answers,
            resumes,
            needs,
            ..
        } = self
            .decision_seam
            .replace(outer)
            .expect("the operand demand's seam stays installed for its run");
        let waits = !needs.is_empty();
        let mut seam = self.call_seam.borrow_mut();
        let seam = seam
            .as_mut()
            .expect("an operand demand runs inside its frame's walk");
        seam.answers = answers;
        seam.resumes = resumes;
        seam.needs = needs;
        waits
    }

    /// Demand what a relation over `operands` unwraps at the running
    /// decision's seam, each operand's carrier walk resuming where the
    /// decision's last run stopped it. A walk over a chain of aliases reads
    /// one answer per hop and stops at the first hop without one, so a
    /// demand that restarted from the operand would read every answered hop
    /// again on each run: a chain of `n` aliases would cost `n * n / 2`
    /// reads. The hops before the resume point settled from answers the
    /// frame keeps, and what the demand observes is dropped, so skipping
    /// them changes nothing the decision decides: its relation still walks
    /// each operand whole, once, when every hop is answered.
    ///
    /// Each operand's entry is the checker recovery its walk ended at, every
    /// hop answered: the error type the operand is, such as the TS2589
    /// recovery of a walk that reached the instantiation budget.
    pub(super) fn demand_relation_operands(
        &self,
        operands: [SemanticNodeId; 2],
    ) -> [Option<SemanticNodeId>; 2] {
        let site = self.checking_site.get();
        let needs_now = || {
            self.decision_seam
                .borrow()
                .as_ref()
                .map_or(0, |seam| seam.needs.len())
        };
        operands.map(|operand| {
            let start = self
                .decision_seam
                .borrow()
                .as_ref()
                .and_then(|seam| seam.resumes.get(&(operand, site)).copied())
                .unwrap_or(super::relation::CarrierWalkPoint {
                    node: operand,
                    depth: 0,
                });
            let needs_before = needs_now();
            let mut resume = start;
            let walked = self.unwrap_identity_carrier_walk(start, &mut |reached| {
                if needs_now() == needs_before {
                    resume = reached;
                }
            });
            self.with_decision_seam(|seam| {
                seam.resumes.insert((operand, site), resume);
            });
            match walked {
                super::relation::IdentityCarrierUnwrap::Concrete(node)
                    if needs_now() == needs_before
                        && self.is_conditional_checker_recovery(node) =>
                {
                    Some(node)
                }
                _ => None,
            }
        })
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
    ) -> FramePoll<'a> {
        let SemanticQueryKey::FlowReturn(flow) = key else {
            unreachable!("a flow frame produces a flow return")
        };
        if let Some(delivery) = delivery {
            let answered = build
                .awaiting
                .take()
                .expect("a delivery answers the read its run waited on");
            build.answers.insert((answered, delivery.site), delivery);
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
            needs: SeamNeeds::default(),
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
            let read = match seam
                .answers
                .get(&(key.clone(), self.checking_site.get()))
                .cloned()
            {
                Some(delivery) => Seam::Answered(delivery),
                None if seam.needs.contains(key, self.checking_site.get()) => Seam::Waiting,
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
                        seam.answers
                            .insert((key.clone(), delivery.site), (*delivery).clone());
                    });
                    self.read_delivery(*delivery, nesting)
                }
                Ok(claim) => {
                    self.with_flow_seam(|seam| seam.needs.push(key.clone(), claim));
                    self.suspended_decision_read()
                }
            },
        })
    }

    /// Whether a read made here is an operand of the stepping flow frame:
    /// its own body is evaluating, so the read answers from the frame's
    /// deliveries or becomes its need.
    pub(super) fn flow_seam_reads_here(&self) -> bool {
        self.flow_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| self.flow_seam_is_open(seam))
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

    /// Whether a window [`Self::open_contextual_window`] opened is open: a
    /// read made inside it may stop the run instead of completing.
    pub(super) fn contextual_window_open(&self) -> bool {
        self.flow_seam
            .borrow()
            .as_ref()
            .is_some_and(|seam| seam.windows > 0)
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

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static SEAM_NEED_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static SEAM_NEED_QUESTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The membership questions seams asked on this thread since it started:
/// whether a read is already one of the needs its run reached.
#[cfg(any(test, feature = "test-support"))]
pub fn seam_need_questions_for_tests() -> u64 {
    SEAM_NEED_QUESTIONS.with(std::cell::Cell::get)
}

/// The stored needs a seam's membership queries compared a read with on
/// this thread since it started: a read already needed is compared with the
/// need that shares its demand's hash, never with every need the run
/// reached before it.
#[cfg(any(test, feature = "test-support"))]
pub fn seam_need_probes_for_tests() -> u64 {
    SEAM_NEED_PROBES.with(std::cell::Cell::get)
}

/// Take the suspensions call frames' walks made on this thread.
#[cfg(any(test, feature = "test-support"))]
pub fn take_call_suspensions_for_tests() -> Vec<CallSuspensionForTests> {
    CALL_SUSPENSIONS_FOR_TESTS.with(|observed| std::mem::take(&mut *observed.borrow_mut()))
}
