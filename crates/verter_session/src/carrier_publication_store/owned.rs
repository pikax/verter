//! Owned publication and stable-parse coordination. Backend work is transferred
//! to the external caller; polling only advances these retained states.
use super::*;
use std::task::{Poll, Waker};
use verter_execution::cancellation::CancellationSubscription;
use verter_execution::tasks::{ExecutionScope, ExecutionTask, WaitEdge};
use verter_execution::wake::{WakeRegistration, WakerSet};

/// A stopped subscription contains no partial publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationError {
    Cancelled,
    Cycle,
    ProducerUnowned,
    ProducerDropped,
}

/// Stable parsing returns a complete product or an explicit terminal refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StableParseError {
    Cancelled,
    Cycle,
    ProducerUnowned,
    ProducerDropped,
    WinnerPanicked,
    Rejected(Arc<verter_language::SyntaxReject>),
}

struct Inputs {
    accepted: AcceptedRegisteredCarrierSource,
    request: PublicationRequestContext,
    id: FrameworkArtifactId,
}
impl Inputs {
    fn new(
        accepted: AcceptedRegisteredCarrierSource,
        mut request: PublicationRequestContext,
    ) -> Arc<Self> {
        // The initiating subscriber's stop never owns shared execution.
        request.cancellation = CancellationToken::new();
        let id = FrameworkArtifactId::derive(&accepted, parse_key_for_accepted(&accepted));
        Arc::new(Self {
            accepted,
            request,
            id,
        })
    }
}

pub(super) struct PublicationLane {
    state: Mutex<PublicationState>,
    task: Mutex<Option<ExecutionTask>>,
}
struct PublicationState {
    inputs: Option<Arc<Inputs>>,
    terminal: Option<Result<TerminalOutcome, PublicationError>>,
    delivery: Option<PublicationOutcome>,
    subscribers: usize,
    wakers: WakerSet,
}
impl PublicationState {
    fn can_retire(&self) -> bool {
        // An abandoned computation is retryable without a source edit. Keep
        // its terminal only while existing subscribers still need to read it.
        self.subscribers == 0 && (self.inputs.is_some() || matches!(self.terminal, Some(Err(_))))
    }
}
impl PublicationLane {
    fn settle(&self, answer: Result<PublicationOutcome, PublicationError>) {
        let (wakes, task) = {
            let mut state = self.state.lock().expect("publication state");
            if state.terminal.is_some() {
                return;
            }
            state.terminal = Some(
                answer
                    .as_ref()
                    .map(TerminalOutcome::from_outcome)
                    .map_err(|e| *e),
            );
            if state.subscribers != 0 {
                state.delivery = answer.ok();
            }
            (
                state.wakers.take(),
                self.task.lock().expect("publication task").take(),
            )
        };
        drop(task);
        wakes.wake();
    }
}

fn retire_publication(
    store: &CarrierPublicationStore,
    id: &FrameworkArtifactId,
    lane: &Arc<PublicationLane>,
) {
    let removed = {
        let mut lanes = store.lanes.lock().expect("publication table");
        if !lanes
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, lane))
        {
            return;
        }
        let state = lane.state.lock().expect("publication state");
        if state.can_retire() {
            lanes.remove(id)
        } else {
            None
        }
    };
    drop(removed);
}

/// Poll-only subscription. Its producer and work capabilities are independent
/// owned values; dropping this subscriber does not stop transferred work.
pub struct OwnedPublication {
    store: Option<CarrierPublicationStore>,
    inputs: Option<Arc<Inputs>>,
    lane: Option<Arc<PublicationLane>>,
    waiter: Option<ExecutionTask>,
    stop: Option<CancellationSubscription>,
    edge: Option<WaitEdge>,
    registration: WakeRegistration,
    early: Option<PublicationOutcome>,
    finished: bool,
}

/// Producer transfer is intermediate; poll the subscriber again for Outcome.
pub enum PublicationStep {
    Produce(PublicationProducer),
    Outcome(PublicationOutcome),
}

impl CarrierPublicationStore {
    /// Begin a snapshot-bound publication using the caller's shared task authority.
    pub fn begin_publication(
        &self,
        accepted: AcceptedRegisteredCarrierSource,
        request: PublicationRequestContext,
        waiter: ExecutionTask,
    ) -> OwnedPublication {
        OwnedPublication::new(self.clone(), accepted, request, Some(waiter))
    }

    /// Begin the snapshot-independent parse dependency. A parsed value retains
    /// its lane until external envelope finishing or its own drop.
    pub fn begin_stable_parse(
        &self,
        accepted: AcceptedRegisteredCarrierSource,
        request: PublicationRequestContext,
        waiter: ExecutionTask,
    ) -> OwnedStableParse {
        OwnedStableParse::new(
            self.clone(),
            Inputs::new(accepted, request.clone()),
            Some(waiter),
            request.cancellation,
        )
    }
}

impl OwnedPublication {
    fn new(
        store: CarrierPublicationStore,
        accepted: AcceptedRegisteredCarrierSource,
        request: PublicationRequestContext,
        waiter: Option<ExecutionTask>,
    ) -> Self {
        let stop = request.cancellation.subscribe();
        let early = if request.cancellation.is_cancelled() {
            Some(PublicationOutcome::Cancelled)
        } else if request.expected_source != *accepted.source().snapshot_id()
            || store
                .grammar_authority
                .validate_accepted_current(&store.source_authority, &accepted)
                .is_err()
        {
            Some(PublicationOutcome::Superseded(CurrentSourceEvidence {
                current_source: None,
            }))
        } else {
            None
        };
        let inputs = Inputs::new(accepted, request);
        let lane = if early.is_none() {
            store.audit.push(
                &inputs.request,
                &inputs.id,
                PublicationAuditKind::PublicationRequested,
            );
            let mut lanes = store.lanes.lock().expect("publication table");
            let expired = lanes.get(&inputs.id).is_some_and(|lane| {
                lane.state
                    .lock()
                    .expect("publication state")
                    .terminal
                    .as_ref()
                    .is_some_and(|t| matches!(t, Ok(t) if t.artifact_expired()))
            });
            if expired {
                lanes.remove(&inputs.id);
                store.audit.push(
                    &inputs.request,
                    &inputs.id,
                    PublicationAuditKind::LiveRecordRetired,
                );
            }
            if let Some(lane) = lanes.get(&inputs.id) {
                let mut state = lane.state.lock().expect("publication state");
                state.subscribers += 1;
                let kind = if state.terminal.is_some() {
                    PublicationAuditKind::LiveHit
                } else {
                    PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Waiter)
                };
                store.audit.push(&inputs.request, &inputs.id, kind);
                Some(lane.clone())
            } else {
                let canonical = inputs.id.source.canonical_digest();
                lanes.retain(|id, lane| {
                    if id.source.canonical_digest() != canonical { return true; }
                    let state = lane.state.lock().expect("publication state");
                    state.subscribers != 0 || state.terminal.as_ref().is_none_or(|t| matches!(t, Ok(t) if !t.artifact_expired() && !matches!(t, TerminalOutcome::Other(_))))
                });
                let lane = Arc::new(PublicationLane {
                    task: Mutex::new(Some(store.task_registry.register_task())),
                    state: Mutex::new(PublicationState {
                        inputs: Some(inputs.clone()),
                        terminal: None,
                        delivery: None,
                        subscribers: 1,
                        wakers: WakerSet::default(),
                    }),
                });
                lanes.insert(inputs.id.clone(), lane.clone());
                store.audit.push(
                    &inputs.request,
                    &inputs.id,
                    PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Leader),
                );
                Some(lane)
            }
        } else {
            None
        };
        Self {
            store: Some(store),
            inputs: Some(inputs),
            lane,
            waiter,
            stop: Some(stop),
            edge: None,
            registration: WakeRegistration::default(),
            early,
            finished: false,
        }
    }

    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<PublicationStep, PublicationError>> {
        assert!(!self.finished, "publication already completed");
        if self
            .stop
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
        {
            if let (Some(store), Some(inputs)) = (&self.store, &self.inputs) {
                store.audit.push(
                    &inputs.request,
                    &inputs.id,
                    PublicationAuditKind::WaiterDetachedCancelled,
                );
            }
            return self.finish(Err(PublicationError::Cancelled));
        }
        if let Some(outcome) = self.early.take() {
            return self.finish(Ok(PublicationStep::Outcome(outcome)));
        }
        let lane = self.lane.as_ref().expect("live lane").clone();
        let mut state = lane.state.lock().expect("publication state");
        if let Some(terminal) = &state.terminal {
            let answer = match terminal {
                Ok(terminal) => Ok(state
                    .delivery
                    .clone()
                    .or_else(|| terminal.outcome())
                    .unwrap_or(PublicationOutcome::RetryExhausted)),
                Err(error) => Err(*error),
            };
            drop(state);
            let answer = answer.map(|outcome| {
                let inputs = self.inputs.as_ref().expect("live inputs");
                let store = self.store.as_ref().expect("live store");
                if store
                    .grammar_authority
                    .validate_accepted_current(&store.source_authority, &inputs.accepted)
                    .is_err()
                {
                    PublicationOutcome::Superseded(CurrentSourceEvidence {
                        current_source: None,
                    })
                } else {
                    outcome
                }
            });
            return self.finish(answer.map(PublicationStep::Outcome));
        }
        if self.edge.is_none() {
            match edge(
                self.waiter.as_ref(),
                lane.task.lock().expect("publication task").as_ref(),
            ) {
                Ok(value) => self.edge = value,
                Err(error) => {
                    drop(state);
                    return self.finish(Err(error));
                }
            }
        }
        if let Some(inputs) = state.inputs.take() {
            drop(state);
            return Poll::Ready(Ok(PublicationStep::Produce(PublicationProducer {
                core: Some(Box::new(PublicationContinuation {
                    store: self.store.clone(),
                    lane: Some(lane),
                    inputs: Some(inputs),
                    phase: Phase::Initial,
                    registration: WakeRegistration::default(),
                    finished: false,
                })),
            })));
        }
        state.wakers.register(&mut self.registration, waker);
        Poll::Pending
    }

    fn finish(
        &mut self,
        answer: Result<PublicationStep, PublicationError>,
    ) -> Poll<Result<PublicationStep, PublicationError>> {
        self.finished = true;
        self.cleanup();
        Poll::Ready(answer)
    }
    fn cleanup(&mut self) {
        self.edge = None;
        self.stop = None;
        self.waiter = None;
        if let Some(lane) = self.lane.take() {
            let (delivery, unclaimed) = {
                let mut state = lane.state.lock().expect("publication state");
                state.wakers.withdraw(&mut self.registration);
                state.subscribers -= 1;
                (
                    if state.subscribers == 0 {
                        state.delivery.take()
                    } else {
                        None
                    },
                    state.can_retire(),
                )
            };
            drop(delivery);
            if unclaimed {
                let store = self.store.as_ref().expect("live store");
                let id = &self.inputs.as_ref().expect("live inputs").id;
                retire_publication(store, id, &lane);
            }
        }
        self.inputs = None;
        self.store = None;
    }
}
impl Drop for OwnedPublication {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn edge(
    waiter: Option<&ExecutionTask>,
    producer: Option<&ExecutionTask>,
) -> Result<Option<WaitEdge>, PublicationError> {
    match (waiter, producer) {
        (Some(waiter), Some(producer)) => waiter
            .wait_for(producer)
            .map(Some)
            .map_err(|_| PublicationError::Cycle),
        (Some(_), None) => Err(PublicationError::ProducerUnowned),
        _ => Ok(None),
    }
}

/// One external advancement of an independent publication producer.
pub enum PublicationProducerStep {
    Work(PublicationWork),
    Complete,
}
enum Initial {
    Done(PublicationOutcome),
    Parse,
}
enum WorkResult {
    Initial(Initial),
    Finished(PublicationOutcome),
    Stopped(PublicationError),
}
#[derive(Default)]
struct Reply {
    result: Option<WorkResult>,
    wakers: WakerSet,
}
enum Phase {
    Initial,
    Work(Arc<Mutex<Reply>>),
    Stable(OwnedStableParse),
    Finish(StableParseValue),
    Complete,
}
/// Resumable execution authority. Poll transfers backend phases and never runs
/// them. Dropping it settles its publication without cancelling a shared S.
pub struct PublicationProducer {
    core: Option<Box<PublicationContinuation>>,
}
impl PublicationProducer {
    pub fn poll(&mut self, waker: &Waker) -> Poll<PublicationProducerStep> {
        let result = self
            .core
            .as_mut()
            .expect("live publication producer")
            .poll(waker);
        if matches!(&result, Poll::Ready(PublicationProducerStep::Complete)) {
            self.core = None;
        }
        result
    }
}
struct PublicationContinuation {
    store: Option<CarrierPublicationStore>,
    lane: Option<Arc<PublicationLane>>,
    inputs: Option<Arc<Inputs>>,
    phase: Phase,
    registration: WakeRegistration,
    finished: bool,
}

impl PublicationContinuation {
    fn poll(&mut self, waker: &Waker) -> Poll<PublicationProducerStep> {
        assert!(!self.finished, "publication producer already completed");
        loop {
            match &mut self.phase {
                Phase::Initial => return self.transfer(WorkKind::Initial),
                Phase::Work(reply) => {
                    let mut state = reply.lock().expect("publication work reply");
                    let Some(result) = state.result.take() else {
                        state.wakers.register(&mut self.registration, waker);
                        return Poll::Pending;
                    };
                    drop(state);
                    match result {
                        WorkResult::Stopped(error) => return self.complete(Err(error)),
                        WorkResult::Initial(Initial::Done(outcome))
                        | WorkResult::Finished(outcome) => return self.complete(Ok(outcome)),
                        WorkResult::Initial(Initial::Parse) => {
                            let lane = self.lane.as_ref().expect("live lane");
                            self.phase = Phase::Stable(OwnedStableParse::new(
                                self.store.clone().expect("live store"),
                                self.inputs.clone().expect("live inputs"),
                                lane.task.lock().expect("publication task").clone(),
                                CancellationToken::new(),
                            ));
                        }
                    }
                }
                Phase::Stable(stable) => match stable.poll(waker) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(StableParseStep::Produce(producer))) => {
                        return Poll::Ready(PublicationProducerStep::Work(PublicationWork {
                            body: Some(WorkBody::Stable(Box::new(producer))),
                        }))
                    }
                    Poll::Ready(Ok(StableParseStep::Parsed(value))) => {
                        self.phase = Phase::Finish(value)
                    }
                    Poll::Ready(Err(error)) => {
                        let outcome = match error {
                            StableParseError::Rejected(reject) => PublicationOutcome::Failed(
                                CarrierParseFailure::ParserRejected(reject),
                            ),
                            StableParseError::WinnerPanicked => PublicationOutcome::WinnerPanicked,
                            StableParseError::Cycle => {
                                return self.complete(Err(PublicationError::Cycle))
                            }
                            StableParseError::ProducerUnowned => {
                                return self.complete(Err(PublicationError::ProducerUnowned))
                            }
                            StableParseError::ProducerDropped => {
                                return self.complete(Err(PublicationError::ProducerDropped))
                            }
                            StableParseError::Cancelled => PublicationOutcome::Cancelled,
                        };
                        return self.complete(Ok(outcome));
                    }
                },
                Phase::Finish(_) => {
                    let Phase::Finish(value) = std::mem::replace(&mut self.phase, Phase::Complete)
                    else {
                        unreachable!()
                    };
                    return self.transfer(WorkKind::Finish(Box::new(value)));
                }
                Phase::Complete => unreachable!("completed producer"),
            }
        }
    }
    fn transfer(&mut self, kind: WorkKind) -> Poll<PublicationProducerStep> {
        let reply = Arc::new(Mutex::new(Reply::default()));
        self.phase = Phase::Work(reply.clone());
        Poll::Ready(PublicationProducerStep::Work(PublicationWork {
            body: Some(WorkBody::Phase {
                store: self.store.clone().expect("live store"),
                inputs: self.inputs.clone().expect("live inputs"),
                lane: self.lane.clone().expect("live lane"),
                task: self
                    .lane
                    .as_ref()
                    .expect("live lane")
                    .task
                    .lock()
                    .expect("publication task")
                    .clone(),
                reply,
                kind,
            }),
        }))
    }
    fn complete(
        &mut self,
        answer: Result<PublicationOutcome, PublicationError>,
    ) -> Poll<PublicationProducerStep> {
        let store = self.store.as_ref().expect("live store");
        let inputs = self.inputs.as_ref().expect("live inputs");
        if !matches!(
            &answer,
            Ok(PublicationOutcome::Published(_) | PublicationOutcome::Adopted(_))
        ) {
            store.audit.push(
                &inputs.request,
                &inputs.id,
                PublicationAuditKind::TerminalFailure,
            );
        }
        self.finished = true;
        self.withdraw();
        self.phase = Phase::Complete;
        let lane = self.lane.take().expect("live lane");
        lane.settle(answer);
        retire_publication(
            self.store.as_ref().expect("live store"),
            &self.inputs.as_ref().expect("live inputs").id,
            &lane,
        );
        self.inputs = None;
        self.store = None;
        Poll::Ready(PublicationProducerStep::Complete)
    }
    fn withdraw(&mut self) {
        if let Phase::Work(reply) = &self.phase {
            reply
                .lock()
                .expect("work reply")
                .wakers
                .withdraw(&mut self.registration);
        }
    }
}
impl Drop for PublicationContinuation {
    fn drop(&mut self) {
        self.withdraw();
        // Drop the child subscription/edge before notifying this producer's waiters.
        self.phase = Phase::Complete;
        if !self.finished {
            if let Some(lane) = self.lane.take() {
                lane.settle(Err(PublicationError::ProducerDropped));
                retire_publication(
                    self.store.as_ref().expect("live store"),
                    &self.inputs.as_ref().expect("live inputs").id,
                    &lane,
                );
            }
        }
    }
}

enum WorkKind {
    Initial,
    Finish(Box<StableParseValue>),
}
enum WorkBody {
    Stable(Box<StableParseProducer>),
    Phase {
        store: CarrierPublicationStore,
        inputs: Arc<Inputs>,
        lane: Arc<PublicationLane>,
        reply: Arc<Mutex<Reply>>,
        task: Option<ExecutionTask>,
        kind: WorkKind,
    },
}
/// Exclusive external CPU/backend phase. Its drop releases all retained work
/// and settles abandonment; it exposes no synchronous dependency driver.
pub struct PublicationWork {
    body: Option<WorkBody>,
}
impl PublicationWork {
    pub fn run(mut self) {
        let body = self.body.take().expect("work runs once");
        match body {
            WorkBody::Stable(producer) => producer.run(),
            WorkBody::Phase {
                store,
                inputs,
                lane: _,
                task,
                reply,
                kind,
            } => {
                let _scope = task
                    .as_ref()
                    .map(|task| ExecutionScope::install(task.registry(), task));
                let _stop = verter_execution::cancellation::JobCancellationGuard::install(
                    inputs.request.cancellation.clone(),
                );
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
                        WorkKind::Initial => WorkResult::Initial(initial_work(&store, &inputs)),
                        WorkKind::Finish(value) => {
                            let outcome = if let Some(adopted) = &value.product.adopted {
                                let (id, outcome) = adopted.as_ref();
                                if *id == inputs.id {
                                    store.fence_adopted(
                                        outcome.clone(),
                                        &inputs.accepted,
                                        &inputs.request,
                                        &inputs.id,
                                    )
                                } else {
                                    store.publish_shared(
                                        &value.product.artifact,
                                        &inputs.accepted,
                                        &inputs.request,
                                        &inputs.id,
                                    )
                                }
                            } else {
                                store.publish_shared(
                                    &value.product.artifact,
                                    &inputs.accepted,
                                    &inputs.request,
                                    &inputs.id,
                                )
                            };
                            WorkResult::Finished(outcome)
                        }
                    }));
                let result =
                    result.unwrap_or(WorkResult::Finished(PublicationOutcome::WinnerPanicked));
                drop(_stop);
                drop(_scope);
                drop(task);
                let wakes = {
                    let mut state = reply.lock().expect("work reply");
                    state.result = Some(result);
                    state.wakers.take()
                };
                wakes.wake();
            }
        }
    }
}
impl Drop for PublicationWork {
    fn drop(&mut self) {
        if let Some(WorkBody::Phase {
            store,
            inputs,
            lane,
            reply,
            ..
        }) = self.body.take()
        {
            let wakes = {
                let mut state = reply.lock().expect("work reply");
                state.result = Some(WorkResult::Stopped(PublicationError::ProducerDropped));
                state.wakers.take()
            };
            lane.settle(Err(PublicationError::ProducerDropped));
            retire_publication(&store, &inputs.id, &lane);
            wakes.wake();
        }
    }
}

fn initial_work(store: &CarrierPublicationStore, inputs: &Inputs) -> Initial {
    let language = inputs.accepted.source().resolved_file_language();
    if language.adapter_id().zip(language.carrier_language_id()).is_none_or(|(adapter, language)| verter_compiler::framework_common::registered_carrier_projection::registered_frontend_for(adapter, language).is_none()) {
        return Initial::Done(PublicationOutcome::Unsupported(RegisteredCarrierUnsupported::NoRegisteredProducer));
    }
    if let Some(candidate) = store.units.retained(&inputs.id, &inputs.accepted) {
        if let StableAdoption::Adopted(outcome) =
            store.adopt_retained(candidate, &inputs.accepted, &inputs.request, &inputs.id)
        {
            return Initial::Done(store.fence_adopted(
                outcome,
                &inputs.accepted,
                &inputs.request,
                &inputs.id,
            ));
        }
    }
    Initial::Parse
}

#[derive(Clone)]
struct StableProduct {
    artifact: Arc<FrameworkParseArtifact>,
    adopted: Option<Arc<(FrameworkArtifactId, PublicationOutcome)>>,
}
pub(super) struct StableParseLane {
    state: Mutex<StableState>,
    task: Mutex<Option<ExecutionTask>>,
}
struct StableState {
    inputs: Option<Arc<Inputs>>,
    result: Option<Result<StableProduct, StableParseError>>,
    producer_live: bool,
    consumers: usize,
    wakers: WakerSet,
}
impl StableParseLane {
    fn settle(&self, result: Result<StableProduct, StableParseError>) {
        let (wakes, task) = {
            let mut state = self.state.lock().expect("stable state");
            if state.result.is_some() {
                return;
            }
            state.result = Some(result);
            state.producer_live = false;
            (
                state.wakers.take(),
                self.task.lock().expect("stable task").take(),
            )
        };
        drop(task);
        wakes.wake();
    }
}
struct StableInterest {
    store: CarrierPublicationStore,
    key: StableUnitKey,
    lane: Arc<StableParseLane>,
}
impl Drop for StableInterest {
    fn drop(&mut self) {
        self.lane.state.lock().expect("stable state").consumers -= 1;
        retire_stable(&self.store, &self.key, &self.lane);
    }
}
fn retire_stable(
    store: &CarrierPublicationStore,
    key: &StableUnitKey,
    lane: &Arc<StableParseLane>,
) {
    let removed = {
        let mut table = store.stable_parses.lock().expect("stable table");
        if !table
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, lane))
        {
            return;
        }
        let state = lane.state.lock().expect("stable state");
        if state.consumers == 0 && (!state.producer_live || state.inputs.is_some()) {
            table.remove(key)
        } else {
            None
        }
    };
    drop(removed);
}

/// A complete stable parse and its in-flight retention interest. Keep this value
/// through envelope finishing; dropping it retires only its own lane instance.
pub struct StableParseValue {
    product: StableProduct,
    _interest: StableInterest,
}
impl StableParseValue {
    pub fn artifact(&self) -> &Arc<FrameworkParseArtifact> {
        &self.product.artifact
    }
}
pub enum StableParseStep {
    Produce(StableParseProducer),
    Parsed(StableParseValue),
}
/// Independent owned subscription to immutable stable work.
pub struct OwnedStableParse {
    interest: Option<StableInterest>,
    waiter: Option<ExecutionTask>,
    stop: Option<CancellationSubscription>,
    edge: Option<WaitEdge>,
    registration: WakeRegistration,
    finished: bool,
}
impl OwnedStableParse {
    fn new(
        store: CarrierPublicationStore,
        inputs: Arc<Inputs>,
        waiter: Option<ExecutionTask>,
        stop: CancellationToken,
    ) -> Self {
        let key = StableUnitKey::new(&inputs.id, &inputs.accepted);
        let lane = {
            let mut table = store.stable_parses.lock().expect("stable table");
            let lane = table
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(StableParseLane {
                        task: Mutex::new(Some(store.task_registry.register_task())),
                        state: Mutex::new(StableState {
                            inputs: Some(inputs),
                            result: None,
                            producer_live: true,
                            consumers: 0,
                            wakers: WakerSet::default(),
                        }),
                    })
                })
                .clone();
            lane.state.lock().expect("stable state").consumers += 1;
            lane
        };
        Self {
            interest: Some(StableInterest { store, key, lane }),
            waiter,
            stop: Some(stop.subscribe()),
            edge: None,
            registration: WakeRegistration::default(),
            finished: false,
        }
    }
    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<StableParseStep, StableParseError>> {
        assert!(!self.finished, "stable subscription already completed");
        if self
            .stop
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
        {
            return self.finish(Err(StableParseError::Cancelled));
        }
        let lane = self.interest.as_ref().expect("live interest").lane.clone();
        let mut state = lane.state.lock().expect("stable state");
        if let Some(result) = state.result.clone() {
            drop(state);
            self.edge = None;
            self.stop = None;
            self.waiter = None;
            let result = result.map(|product| {
                StableParseStep::Parsed(StableParseValue {
                    product,
                    _interest: self.interest.take().expect("live interest"),
                })
            });
            return self.finish(result);
        }
        if self.edge.is_none() {
            match edge(
                self.waiter.as_ref(),
                lane.task.lock().expect("stable task").as_ref(),
            ) {
                Ok(edge) => self.edge = edge,
                Err(error) => {
                    drop(state);
                    return self.finish(Err(match error {
                        PublicationError::ProducerUnowned => StableParseError::ProducerUnowned,
                        _ => StableParseError::Cycle,
                    }));
                }
            }
        }
        if let Some(inputs) = state.inputs.take() {
            drop(state);
            let interest = self.interest.as_ref().expect("live interest");
            return Poll::Ready(Ok(StableParseStep::Produce(StableParseProducer {
                store: interest.store.clone(),
                key: interest.key.clone(),
                task: lane.task.lock().expect("stable task").clone(),
                lane: lane.clone(),
                inputs: Some(inputs),
            })));
        }
        state.wakers.register(&mut self.registration, waker);
        Poll::Pending
    }
    fn finish(
        &mut self,
        answer: Result<StableParseStep, StableParseError>,
    ) -> Poll<Result<StableParseStep, StableParseError>> {
        self.finished = true;
        self.cleanup();
        Poll::Ready(answer)
    }
    fn cleanup(&mut self) {
        self.edge = None;
        self.stop = None;
        self.waiter = None;
        if let Some(interest) = self.interest.take() {
            interest
                .lane
                .state
                .lock()
                .expect("stable state")
                .wakers
                .withdraw(&mut self.registration);
        }
    }
}
impl Drop for OwnedStableParse {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Exclusive stable producer. Parsing, retained adoption and their backend
/// callbacks run only here under the actual S task. Drop settles abandonment.
pub struct StableParseProducer {
    store: CarrierPublicationStore,
    key: StableUnitKey,
    lane: Arc<StableParseLane>,
    task: Option<ExecutionTask>,
    inputs: Option<Arc<Inputs>>,
}
impl StableParseProducer {
    pub fn run(mut self) {
        let inputs = self.inputs.take().expect("stable producer runs once");
        let _scope = self
            .task
            .as_ref()
            .map(|task| ExecutionScope::install(task.registry(), task));
        let _stop = verter_execution::cancellation::JobCancellationGuard::install(
            inputs.request.cancellation.clone(),
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(candidate) = self.store.units.retained(&inputs.id, &inputs.accepted) {
                if let StableAdoption::Adopted(PublicationOutcome::Adopted(envelope)) = self
                    .store
                    .adopt_retained(candidate, &inputs.accepted, &inputs.request, &inputs.id)
                {
                    return Ok(StableProduct {
                        artifact: envelope.artifact().clone(),
                        adopted: Some(Arc::new((
                            inputs.id.clone(),
                            PublicationOutcome::Adopted(envelope),
                        ))),
                    });
                }
            }
            match self
                .store
                .parse_stable_unit(&inputs.accepted, &inputs.request, &inputs.id)
            {
                StableParseOutcome::Parsed(artifact) => Ok(StableProduct {
                    artifact,
                    adopted: None,
                }),
                StableParseOutcome::Rejected(reject) => Err(StableParseError::Rejected(reject)),
            }
        }))
        .unwrap_or(Err(StableParseError::WinnerPanicked));
        drop(_stop);
        drop(_scope);
        self.task = None;
        self.lane.settle(result);
        retire_stable(&self.store, &self.key, &self.lane);
    }
}
impl Drop for StableParseProducer {
    fn drop(&mut self) {
        if self.inputs.take().is_some() {
            self.lane.settle(Err(StableParseError::ProducerDropped));
            retire_stable(&self.store, &self.key, &self.lane);
        }
    }
}

pub(super) fn drive(
    store: CarrierPublicationStore,
    accepted: AcceptedRegisteredCarrierSource,
    request: PublicationRequestContext,
) -> PublicationOutcome {
    let waiter = Some(
        ExecutionScope::current(&store.task_registry)
            .unwrap_or_else(|| store.task_registry.register_task()),
    );
    let mut handoff = OwnedPublication::new(store, accepted, request, waiter);
    let mut producer = None;
    let mut stopped = false;
    verter_execution::blocking::block_on(None, |waker| {
        if let Some(active) = &mut producer {
            match PublicationProducer::poll(active, waker) {
                Poll::Ready(PublicationProducerStep::Work(work)) => {
                    work.run();
                    waker.wake_by_ref();
                }
                Poll::Ready(PublicationProducerStep::Complete) => producer = None,
                Poll::Pending => {}
            }
        }
        if stopped {
            return if producer.is_none() {
                Poll::Ready(PublicationOutcome::Cancelled)
            } else {
                Poll::Pending
            };
        }
        match handoff.poll(waker) {
            Poll::Ready(Ok(PublicationStep::Produce(active))) => {
                producer = Some(active);
                waker.wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Ok(PublicationStep::Outcome(outcome))) => Poll::Ready(outcome),
            Poll::Ready(Err(PublicationError::Cancelled)) if producer.is_some() => {
                stopped = true;
                Poll::Pending
            }
            Poll::Ready(Err(PublicationError::Cancelled | PublicationError::ProducerDropped)) => {
                Poll::Ready(PublicationOutcome::Cancelled)
            }
            Poll::Ready(Err(PublicationError::Cycle | PublicationError::ProducerUnowned)) => {
                Poll::Ready(PublicationOutcome::RetryExhausted)
            }
            Poll::Pending => Poll::Pending,
        }
    })
}

#[cfg(test)]
#[path = "owned_tests.rs"]
mod tests;
