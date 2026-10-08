//! Instantiations on the continuation runtime.
//!
//! An instantiation evaluated outside any drive is driven: its build runs
//! as a frame whose declaration-body projection, where it would evaluate an
//! instantiation in place, needs it instead, so a chain of instantiations
//! (an alias chain, a reference whose arguments are applications) is a
//! chain of heap-owned frames, never of native calls. Each frame owns what
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

use super::build::{InstantiateBuild, InstantiatePoll, InstantiateStart};
use super::{BuildLocalTaint, CarrierNormalizationPrelude, ProjectSemanticDispatch};
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::semantic_execution::{EvalStep, External, Outcome, Program, SemanticExecution, Start};
use crate::semantic_query::{
    CacheRead, QueryError, QueryResult, ReadReceipt, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue,
};
use crate::semantic_query_memo::{
    Claim, ClaimAttempt, ExecutionTask, Joined, ProducerLease, ReadCapture, Recursion,
    SemanticGraphStore, Subscription,
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

/// An instantiation's build between its steps.
pub(super) struct InstantiateFrame<'p, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'p ProjectSemanticDispatch<'a, C>,
    key: SemanticQueryKey,
    lease: Option<ProducerLease<'p>>,
    tracer: Option<crate::fact_signature_helpers::StepwiseFactTracer<'p, C::Clocks>>,
    taint: BuildLocalTaint,
    build: Option<Box<InstantiateBuild>>,
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
    attempt: ClaimAttempt,
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
        let mut attempt = match self.retries.remove(key) {
            Some(attempt) => attempt,
            None => match dispatch.enter_query_frame(key) {
                Ok(attempt) => attempt,
                Err(delivery) => return Start::Answer(Ok(*delivery)),
            },
        };
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
                        dispatch.admit_served(receipt, 0),
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
            Claim::Read(read) => Start::Answer(Ok(dispatch.answered(key, read, carrier))),
            Claim::Recursive(recursion) => {
                Start::Answer(Ok(dispatch.recursion_delivery(key, recursion)))
            }
            Claim::Subscribed(subscription) => Start::Subscribe(PendingClaim {
                key: key.clone(),
                subscription,
                attempt,
            }),
            Claim::Produce(lease) => {
                if let Some((read, carrier)) = dispatch.own_published_read(key, 0) {
                    // Dropping the lease releases the flight unpublished.
                    drop(lease);
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
                })))
            }
        }
    }

    fn step(&mut self, frame: &mut Self::Frame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        match frame {
            QueryFrame::Instantiate(frame) => {
                self.child_depth = frame.depth + 1;
                self.dispatch.step_instantiate_frame(frame, delivery)
            }
        }
    }

    fn close_cycle(&mut self, key: &SemanticQueryKey, _role: ()) -> Outcome<Self> {
        // The open demand's producer is this task's own: the memo answers
        // the claim exactly as it answers a synchronous re-entry — a warm
        // result where one validates, else the same-path carrier.
        let dispatch = self.dispatch;
        let mut carrier = None;
        let mut capture = ReadCapture::deferring_carrier(&mut carrier);
        let graph = dispatch.graph();
        let claim = match graph.begin_query_claim(
            dispatch.ctx,
            dispatch.snapshot.flags(),
            key.clone(),
            &mut capture,
        ) {
            Err(read) => return Ok(dispatch.answered(key, read, carrier)),
            Ok(mut attempt) => graph.claim_query(
                dispatch.ctx,
                dispatch.snapshot.flags(),
                &mut attempt,
                &self.task,
                &mut capture,
            ),
        };
        Ok(match claim {
            Claim::Read(read) => dispatch.answered(key, read, carrier),
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
        !delivery.read.cache_suppress && !delivery.read.result_is_partial
    }

    fn wait(&mut self, pending: PendingClaim<'p>) -> External<Self> {
        let dispatch = self.dispatch;
        let PendingClaim {
            key,
            subscription,
            mut attempt,
        } = pending;
        let mut carrier = None;
        let joined = {
            let mut capture = ReadCapture::deferring_carrier(&mut carrier);
            subscription.wait(
                dispatch.ctx,
                dispatch.snapshot.flags(),
                &mut attempt,
                &mut capture,
            )
        };
        // A joined result this demand cannot pay for is computed instead.
        if let Joined::Read(CacheRead {
            receipt: ReadReceipt::Priced(receipt),
            ..
        }) = &joined
        {
            if !attempt.is_recomputing()
                && matches!(
                    dispatch.admit_served(receipt, 0),
                    super::ServedAdmission::Recompute(_)
                )
            {
                attempt.recompute();
                self.retries.insert(key, attempt);
                return External::Restart;
            }
        }
        match joined {
            Joined::Read(read) => External::Complete(Ok(dispatch.answered(&key, read, carrier))),
            Joined::Recursive(recursion) => {
                External::Complete(Ok(dispatch.recursion_delivery(&key, recursion)))
            }
            Joined::Retry => {
                self.retries.insert(key, attempt);
                External::Restart
            }
        }
    }

    fn stop(&self) -> Option<()> {
        self.dispatch.cancellation.is_cancelled().then_some(())
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether `key` runs on the continuation runtime when it is evaluated
    /// outside any drive: an instantiation read for its value alone.
    pub(super) fn drives_on_the_runtime(&self, key: &SemanticQueryKey) -> bool {
        matches!(key, SemanticQueryKey::Instantiate(_))
            && !crate::semantic_execution::drive_running()
            && self.active_operand_evidence.borrow().is_empty()
    }

    /// Evaluate `key` on the continuation runtime: the synchronous entry
    /// of an instantiation. A warm result answers without the runtime.
    pub(super) fn drive_query(&self, key: SemanticQueryKey) -> ValueRead {
        // The connected demand spans the whole drive; the entry itself is
        // charged exactly as a synchronous entry is.
        let (connected_guard, _) = self.enter_connected_demand(false);
        // An isolated root whose refusal is sealed answers from it.
        let isolated_root = self.isolated_root_identity(&connected_guard, &key);
        if let Some(summary) = isolated_root
            .as_ref()
            .and_then(|root| self.sealed_refusal_for(root))
        {
            return self.deliver_sealed_refusal(&summary);
        }
        // The drive runs on this native stack: its entry is one nested
        // query level, as a synchronous entry is.
        let (attempt, _query_depth_guard) = match self.enter_query(&key, None, true) {
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
                    self.connected_demand.record_prerequisite(receipt, 1);
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
            read.result_is_partial,
            read.cache_suppress,
            read.partial_reason_classes(),
        );
        read
    }

    /// A need's entry: the dispatch it replaces is counted, charged and
    /// looked up exactly as a synchronous entry would be. `Err` answers it.
    fn enter_query_frame(
        &self,
        key: &SemanticQueryKey,
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
        let entered = self.enter_query(key, Some(&mut carrier), false);
        entered
            .map(|(attempt, _)| attempt)
            .map_err(|read| Box::new(QueryDelivery { read, carrier }))
    }

    /// Count, charge and look up one query entry, exactly as the
    /// synchronous entry does. `carrier`, when given, receives a warm
    /// result's carrier instead of the tracers active now; `charge_depth`
    /// enters one nested native query level, held by the returned guard.
    /// `Err` is the read that answers the entry.
    fn enter_query(
        &self,
        key: &SemanticQueryKey,
        carrier: Option<&mut Option<ReadSetSignature>>,
        charge_depth: bool,
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
        let query_depth_guard = self.charge_query_entry(key, preexisting_trip, charge_depth)?;
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
                match self.served_read(key, read, u16::from(charge_depth)) {
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
    ) -> QueryDelivery {
        let read = match self.served_read(key, read, 0) {
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
        let read = SemanticGraphStore::recursion_read(recursion, self.query_sentinel(key));
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
                    name: Arc::clone(&k.base().merged_symbol_name),
                    args: Arc::clone(k.args()),
                }));
        }
        self.graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::Miss))
    }

    /// Consume a delivery in the consuming frame's step: replay its
    /// carrier into the frame's tracer, fold its rails into the frame's
    /// taint, and read the instantiated node as a synchronous
    /// instantiation's reader does.
    fn consume_delivery(&self, delivery: QueryDelivery) -> SemanticNodeId {
        let QueryDelivery { read, carrier } = delivery;
        if let Some(carrier) = carrier {
            carrier.bubble_via_tls();
        }
        if let Some(receipt) = read.receipt.priced() {
            self.connected_demand.record_prerequisite(receipt, 0);
        }
        self.fold_cache_read_rails(
            read.result_is_partial,
            read.cache_suppress,
            read.partial_reason_classes(),
        );
        match read.value {
            QueryResult::Value(SemanticQueryValue::TypeNode(node)) => node,
            _ => self.opaque(QueryError::Miss),
        }
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
        let delivered = delivery.map(|outcome| match outcome {
            Ok(delivery) => self.consume_delivery(delivery),
            Err(()) => unreachable!("a stopped drive delivers nothing"),
        });
        let poll = match frame.build.as_mut() {
            Some(build) => self.drain_instantiate(build, delivered),
            None => {
                self.open_cold_build(None);
                let SemanticQueryKey::Instantiate(key) = &frame.key else {
                    unreachable!("an instantiation frame produces an instantiation")
                };
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
                        let build = frame.build.insert(build);
                        self.drain_instantiate(build, None)
                    }
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
        let traced_soundly = matches!(
            finalise,
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(_)
        );
        let output = self.close_cold_build(
            output.into(),
            std::mem::take(&mut frame.taint),
            finalise,
            &CarrierNormalizationPrelude::none(),
            None,
            &frame.key,
        );
        let lease = frame
            .lease
            .take()
            .expect("a producing frame owns its lease");
        if frame.depth == 1 {
            // The drive's root: its entry seals a refusal on these facts.
            *self.driven_root_traced.borrow_mut() =
                super::TracedFacts::of_output(&output, traced_soundly);
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
                                self.keep_published(&frame.key, &read, carrier);
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
