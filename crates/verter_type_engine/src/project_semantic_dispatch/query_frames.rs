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

/// The program instantiations run on: the dispatch they evaluate through
/// and the task that owns every producer the drive claims.
pub(super) struct QueryProgram<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'p ProjectSemanticDispatch<'a, C>,
    task: ExecutionTask,
    /// Claims that continue after an outside producer went away.
    retries: FxHashMap<SemanticQueryKey, ClaimAttempt>,
    /// The depth a frame started now nests at: one below the frame whose
    /// step needed it (the drive runs one chain, so a demand starts right
    /// after the step that needed it, or after its wait restarts it).
    child_depth: u32,
}

/// A suspended semantic computation.
pub(super) enum QueryFrame<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    Instantiate(Box<InstantiateFrame<'p, 'a, C>>),
}

/// The build a frame carries between its steps: an instantiation's, a
/// `typeof`'s whose value's annotation is lowering, or a conditional
/// type's decision.
pub(super) enum FrameBuild {
    Instantiate(Box<InstantiateBuild>),
    Typeof(Box<TypeofBuild>),
    Conditional(Box<ConditionalBuild>),
}

/// A conditional type's decision between its steps: the answers to the
/// instantiations and `typeof`s its decision read, and the one it waits
/// on.
///
/// The decision procedure is a function of its operands and of those
/// answers. A step runs it with the answers the frame holds; a read none of
/// them answers becomes the frame's need, and the step's run is discarded
/// — its observations with it — to run again once the need is delivered.
/// No read is ever evaluated in place one native level deeper, so a chain
/// of conditionals each deciding over the one before is a chain of frames.
#[derive(Default)]
pub(super) struct ConditionalBuild {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    awaiting: Option<SemanticQueryKey>,
    /// The claim the awaited read entered with, until the runtime takes it.
    claim: Option<ClaimAttempt>,
}

/// What a running conditional decision's reads are answered from: its
/// frame's answers, and the first read they do not answer.
pub(super) struct DecisionSeam {
    answers: FxHashMap<SemanticQueryKey, QueryDelivery>,
    need: Option<SemanticQueryKey>,
    /// The frame depth a needed demand's own frame runs at.
    depth: u32,
    /// The claim the need entered with, for the runtime to produce it.
    claim: Option<ClaimAttempt>,
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
}

impl Drop for NestedDecisionQuery<'_> {
    fn drop(&mut self) {
        if let Some(cell) = self.seam {
            if let Some(seam) = cell.borrow_mut().as_mut() {
                seam.nested -= 1;
            }
        }
    }
}

/// An instantiation's — or a `typeof`'s — build between its steps.
pub(super) struct InstantiateFrame<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'p ProjectSemanticDispatch<'a, C>,
    key: SemanticQueryKey,
    lease: Option<ProducerLease<'p>>,
    tracer: Option<crate::fact_signature_helpers::StepwiseFactTracer<'p, C::Clocks>>,
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

impl<C: crate::resolver_core::ResolverCapabilities> Drop for InstantiateFrame<'_, '_, C> {
    fn drop(&mut self) {
        // A build stopped between its steps leaves the declaration it
        // entered; its lease aborts its flight on drop.
        if let Some((identity, args)) = self.active.take() {
            self.dispatch.leave_instantiate_active(&identity, &args);
        }
        // A frame stopped before it completed leaves no receipt; what it
        // charged is its consumer's.
        if self.recording {
            self.dispatch.connected_demand.abandon_cost_scope();
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

impl<'p, 'a, C: crate::resolver_core::ResolverCapabilities> Program for QueryProgram<'p, 'a, C> {
    type Demand = SemanticQueryKey;
    type Frame = QueryFrame<'p, 'a, C>;
    type Value = QueryDelivery;
    type Failure = ();
    type Role = ();
    type Subscription = PendingClaim<'p>;

    fn start(&mut self, key: &SemanticQueryKey) -> Start<Self> {
        let dispatch = self.dispatch;
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
        let (claim, carrier) = loop {
            let mut carrier = None;
            let claim = {
                let mut capture = ReadCapture::deferring_carrier(&mut carrier);
                dispatch.graph().claim_query(
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
                if recursion == Recursion::SamePath {
                    dispatch.connected_demand.note_divergence_certificate();
                }
                Start::Answer(Ok(dispatch.recursion_delivery(key, recursion)))
            }
            Claim::Subscribed(subscription) => Start::Subscribe(PendingClaim {
                key: key.clone(),
                subscription,
                attempt: Some(attempt),
                nesting,
            }),
            Claim::Produce(lease) => {
                Start::Produce(QueryFrame::Instantiate(Box::new(InstantiateFrame {
                    dispatch,
                    key: key.clone(),
                    lease: Some(lease),
                    tracer: None,
                    taint: BuildLocalTaint::default(),
                    build: None,
                    active: None,
                    depth: self.child_depth,
                    recording: false,
                    computed_at: dispatch.workspace_edit(),
                })))
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
                if let (EvalStep::Need { demand, .. }, Some(FrameBuild::Conditional(build))) =
                    (&step, frame.build.as_mut())
                {
                    if let Some(claim) = build.claim.take() {
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
        let dispatch = self.dispatch;
        let nesting = Nesting::Frame {
            depth: self.child_depth,
        };
        let mut carrier = None;
        let mut capture = ReadCapture::deferring_carrier(&mut carrier);
        let graph = dispatch.graph();
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
            Claim::Recursive(recursion) => {
                if recursion == Recursion::SamePath {
                    dispatch.connected_demand.note_divergence_certificate();
                }
                dispatch.recursion_delivery(key, recursion)
            }
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
        let dispatch = self.dispatch;
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
                if recursion == Recursion::SamePath {
                    dispatch.connected_demand.note_divergence_certificate();
                }
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
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether `key` runs on the continuation runtime when it is evaluated
    /// outside any drive: an instantiation or a `typeof` read for its value
    /// alone. A `typeof` whose value's type lowers from its authored value
    /// position (its annotation, its inferred initializer, its const-object
    /// shape) builds as a frame, so a chain of values each typed by the one
    /// before is a chain of frames; any other `typeof` completes in the
    /// frame's first step.
    pub(super) fn drives_on_the_runtime(&self, key: &SemanticQueryKey) -> bool {
        matches!(
            key,
            SemanticQueryKey::Instantiate(_) | SemanticQueryKey::TypeOf { .. }
        ) && !crate::semantic_execution::drive_running()
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
        // A claim needs the entry's task; a warm read took none.
        let execution = self.graph().enter_execution();
        let mut program = QueryProgram {
            dispatch: self,
            task: execution.task().clone(),
            retries: FxHashMap::default(),
            child_depth: 1,
        };
        program.retries.insert(key.clone(), attempt);
        let outcome = SemanticExecution::new().drive(&mut program, key.clone());
        drop(program);
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
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.record_dispatched_query_tag(key.tag());
        }
        let (_connected_guard, preexisting_trip) = self.enter_connected_demand(false);
        let query_depth_guard =
            self.charge_query_entry(key, preexisting_trip, nesting.query_levels() > 0)?;
        // A result this dispatcher's build already published answers before
        // the memo is looked up.
        match self.replay_kept_read(key, nesting) {
            super::KeptReplay::Served(read, held) => {
                match carrier {
                    Some(slot) => *slot = Some(held),
                    None => held.bubble_via_tls(),
                }
                return Err(self.attribute_query_read(
                    key,
                    false,
                    read,
                    &CarrierNormalizationPrelude::none(),
                ));
            }
            super::KeptReplay::Refused(_) => {
                let attempt = self
                    .graph()
                    .begin_query_recompute(self.snapshot.flags(), key.clone())?;
                return Ok((attempt, query_depth_guard));
            }
            super::KeptReplay::Absent => {}
        }
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
    pub(super) fn consume_frame_delivery(&self, delivery: FrameDelivery) -> SemanticNodeId {
        self.consume_delivery(delivery.delivery, delivery.depth)
    }

    fn consume_delivery(&self, delivery: QueryDelivery, depth: u32) -> SemanticNodeId {
        match self
            .read_delivery(delivery, Some(Nesting::Frame { depth }))
            .value
        {
            QueryResult::Value(SemanticQueryValue::TypeNode(node)) => node,
            _ => self.opaque(QueryError::Miss),
        }
    }

    /// The read a delivery answers its consumer with: its carrier replayed
    /// into the tracers active now, its receipt recorded as the consumer's
    /// prerequisite read the way `nesting` names (when it is the
    /// consumer's to pay), its rails folded into the consumer's taint —
    /// what the read's own entry leaves on its reader.
    fn read_delivery(&self, delivery: QueryDelivery, nesting: Option<Nesting>) -> ValueRead {
        let QueryDelivery { read, carrier } = delivery;
        if let Some(carrier) = carrier {
            carrier.bubble_via_tls();
        }
        if let (Some(receipt), Some(nesting)) = (read.receipt.priced(), nesting) {
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
            need: None,
            claim: None,
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
        let output = match seam.need {
            Some(need) => match run.discard() {
                // An aborted run is the decision's answer: its abort is
                // observed, never discarded.
                Err(abort) => self.aborted_build_output(abort),
                Ok(_) => {
                    build.awaiting = Some(need.clone());
                    build.claim = seam.claim;
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
    /// frame's answers; one that must be computed becomes the frame's need
    /// (when the run does not wait on one already), its claim kept for the
    /// runtime to produce it, and reads as a typed gap nothing built from it
    /// keeps. A query the decision evaluates in place reads only the frame's
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
        // computed in a frame of its own, the needed demand's, which the
        // decision's frame paid as its need when its own run read it: a
        // query the decision evaluates in place reads it as a value the
        // decision already holds, its carrier and rails and no cost of its
        // own.
        enum Seam {
            Answered(QueryDelivery, bool),
            Waiting,
            Enter,
        }
        let (seam, depth) = {
            let installed = self.decision_seam.borrow();
            let seam = installed.as_ref()?;
            let answered = seam.answers.get(key).cloned();
            let read = match answered {
                Some(delivery) => Seam::Answered(delivery, seam.nested > 0),
                None if seam.nested > 0 => return None,
                None if seam.need.is_some() => Seam::Waiting,
                None => Seam::Enter,
            };
            (read, seam.depth)
        };
        let nesting = Nesting::Frame { depth };
        Some(match seam {
            Seam::Answered(delivery, nested) => {
                self.read_delivery(delivery, (!nested).then_some(nesting))
            }
            Seam::Waiting => self.suspended_decision_read(),
            Seam::Enter => match self.enter_query_frame(key, depth) {
                Err(delivery) => {
                    self.with_decision_seam(|seam| {
                        seam.answers.insert(key.clone(), (*delivery).clone());
                    });
                    self.read_delivery(*delivery, Some(nesting))
                }
                Ok(claim) => {
                    self.with_decision_seam(|seam| {
                        seam.need = Some(key.clone());
                        seam.claim = Some(claim);
                    });
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
            .is_some_and(|seam| seam.need.is_some())
    }

    /// Mark a query the running decision reached as evaluating in place:
    /// until it returns, reads the decision's answers do not hold evaluate
    /// as they would anywhere.
    pub(super) fn nested_decision_query(&self) -> NestedDecisionQuery<'_> {
        let mut seam = self.decision_seam.borrow_mut();
        NestedDecisionQuery {
            seam: seam.as_mut().map(|seam| {
                seam.nested += 1;
                &self.decision_seam
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
                .lease
                .as_ref()
                .expect("a stepping frame owns its lease")
                .cost_identity();
            self.connected_demand.open_cost_scope(identity);
            frame.recording = true;
        }
        let tracer = frame.tracer.get_or_insert_with(|| {
            crate::fact_signature_helpers::StepwiseFactTracer::new(
                crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(frame.dispatch.ctx),
            )
        });
        let taint = super::StepTaint::install(&self.build_local_taint, &mut frame.taint);
        let installed = tracer.install();
        let delivery = delivery.map(|outcome| match outcome {
            Ok(delivery) => delivery,
            Err(()) => unreachable!("a stopped drive delivers nothing"),
        });
        let poll = match frame.build.as_mut() {
            Some(FrameBuild::Instantiate(build)) => self.drain_instantiate(
                build,
                delivery.map(|delivery| self.consume_delivery(delivery, frame.depth + 1)),
            ),
            // A typeof build consumes its delivery where its evaluation
            // reads it.
            Some(FrameBuild::Typeof(build)) => self.drain_typeof(
                build,
                delivery.map(|delivery| FrameDelivery {
                    delivery,
                    depth: frame.depth + 1,
                }),
            ),
            // A decision keeps its deliveries as answers to its reads.
            Some(FrameBuild::Conditional(build)) => {
                self.drain_conditional(&frame.key, build, delivery, frame.depth + 1)
            }
            None => {
                self.open_cold_build(None);
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
                            InstantiateStart::Done(output) => InstantiatePoll::Done(output),
                            InstantiateStart::Build(build) => {
                                frame.active =
                                    Some((build.active_identity.clone(), Arc::clone(&build.args)));
                                let FrameBuild::Instantiate(build) =
                                    frame.build.insert(FrameBuild::Instantiate(build))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_instantiate(build, None)
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
                            TypeofStart::Done(output) => InstantiatePoll::Done(output),
                            TypeofStart::Build(build) => {
                                let FrameBuild::Typeof(build) =
                                    frame.build.insert(FrameBuild::Typeof(build))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_typeof(build, None)
                            }
                        }
                    }
                    SemanticQueryKey::Conditional { .. } => {
                        match self.cold_build_budget_refusal(&frame.key) {
                            Some(refusal) => InstantiatePoll::Done(Box::new(refusal)),
                            None => {
                                let FrameBuild::Conditional(build) =
                                    frame.build.insert(FrameBuild::Conditional(Box::default()))
                                else {
                                    unreachable!("the build was just inserted")
                                };
                                self.drain_conditional(&frame.key, build, None, frame.depth + 1)
                            }
                        }
                    }
                    _ => unreachable!(
                        "a frame produces an instantiation, a typeof or a conditional decision"
                    ),
                }
            }
        };
        drop(installed);
        drop(taint);
        match poll {
            InstantiatePoll::Need(key) => EvalStep::Need {
                demand: key,
                role: (),
            },
            InstantiatePoll::Done(output) => {
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
        output: crate::project_semantic_dispatch::walk::QueryBuildOutput,
    ) -> QueryDelivery {
        let finalise = frame
            .tracer
            .take()
            .expect("a stepped frame owns its tracer")
            .finish();
        let (output, rooting) = self.close_cold_build(
            output.into(),
            std::mem::take(&mut frame.taint),
            finalise,
            &CarrierNormalizationPrelude::none(),
        );
        let lease = frame
            .lease
            .take()
            .expect("a producing frame owns its lease");
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
        let read = {
            let mut capture = ReadCapture::deferring_carrier(&mut carrier);
            match lease.settle(self.ctx, self.snapshot.flags(), output, receipt.clone()) {
                Err(read) => read,
                Ok(mut settled) => {
                    match settled.admit(self.ctx, self.snapshot.flags(), &mut capture) {
                        Err(read) => read,
                        Ok(()) => {
                            let published = settled.published_carrier();
                            let read = settled.complete(self.ctx, &mut capture);
                            if let Some(carrier) = published {
                                self.keep_published(
                                    &frame.key,
                                    &read,
                                    carrier,
                                    frame.computed_at,
                                    true,
                                );
                            }
                            read
                        }
                    }
                }
            }
        };
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
}
