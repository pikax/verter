//! Task-owned store-view capture and the shared synchronous driver.
use super::*;
use std::task::{Poll, Waker};
use verter_execution::cancellation::{CancellationSubscription, CancellationToken};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, WaitEdge};
use verter_execution::wake::{TakenWakers, WakeRegistration, WakerSet};

/// A terminal stop carries no current or partial store-view capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreViewError {
    Cancelled,
    Cycle,
    ProducerUnowned,
    ProducerDropped,
    ProducerPanicked,
    Reset,
}

/// Opaque capture result; currentness remains bound to its private read.
pub struct StoreViewValue {
    read: StoreViewRead,
}
impl StoreViewValue {
    pub fn is_current(&self) -> bool {
        matches!(self.read, StoreViewRead::Current(_))
    }
    pub(crate) fn into_read(self) -> StoreViewRead {
        self.read
    }
}
/// A poll either transfers cold work or returns a captured base view.
pub enum StoreViewStep {
    Produce(StoreViewProducer),
    Value(StoreViewValue),
}
/// Owned, exclusive subscription. It exposes no host or blocking driver.
pub struct StoreViewHandoff {
    core: Core<Arc<VerterHost>>,
}
/// Independent execution ownership, movable without borrowing its subscriber.
pub struct StoreViewProducer {
    core: Producer<Arc<VerterHost>>,
}
impl StoreViewProducer {
    pub fn run(self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.core.run()));
    }
}
impl StoreViewHandoff {
    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<StoreViewStep, StoreViewError>> {
        // Core and its transient cached views can release the final service
        // owner during a terminal poll. Retire it only after all poll locals
        // and mapped payloads have gone, using the lowering handoff's release.
        let service = self
            .core
            .host
            .as_ref()
            .expect("live host")
            .decl_lowering
            .clone();
        let result = self.core.poll(waker).map(|answer| {
            answer.and_then(|step| match step {
                Step::Produce(core) => Ok(StoreViewStep::Produce(StoreViewProducer { core })),
                Step::Value(value) => Ok(StoreViewStep::Value(value)),
                Step::Reentrant(_) => Err(StoreViewError::Cycle),
            })
        });
        service.release_without_join();
        result
    }
}
impl VerterHost {
    /// Capture a base store view with owned suspension and no work in poll.
    /// Session overlay application and membership ingestion remain caller work.
    pub fn begin_store_view(
        self: &Arc<Self>,
        waiter: ExecutionTask,
        stop: CancellationToken,
    ) -> StoreViewHandoff {
        StoreViewHandoff {
            core: Core::new(self.clone(), waiter, stop, false),
        }
    }
}

trait HostRef: Clone {
    fn host(&self) -> &VerterHost;
    fn release(self) {
        drop(self);
    }
}
impl HostRef for Arc<VerterHost> {
    fn host(&self) -> &VerterHost {
        self
    }
    fn release(self) {
        if let Some(host) = Arc::into_inner(self) {
            // Host teardown can release other service pins (artifacts and the
            // scheduler executor). Keep one until all those owners have gone,
            // then use the same nonjoining release as a lowering handoff.
            let service = host.decl_lowering.clone();
            drop(host);
            service.release_without_join();
        }
    }
}
impl HostRef for &VerterHost {
    fn host(&self) -> &VerterHost {
        self
    }
}

enum Attempt {
    Published(HostStoreView),
    Fenced(HostStoreView, StoreViewReturnOnlyReason),
    Read(StoreViewRead),
}
#[derive(Default)]
struct Reply {
    result: Option<Result<Attempt, StoreViewError>>,
    failure: Option<StoreViewError>,
    wakers: WakerSet,
}
/// Immutable identity and owner; its reply and execution bit are private to
/// the claim protocol. Physical execution remains serialized across reset.
pub(super) struct Claim {
    task: Option<ExecutionTask>,
    reset_generation: u64,
    started: std::sync::atomic::AtomicBool,
    revoked: std::sync::atomic::AtomicBool,
    compatibility: bool,
    reply: Arc<parking_lot::Mutex<Reply>>,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Claim")
            .field("task", &self.task)
            .field("reset_generation", &self.reset_generation)
            .finish_non_exhaustive()
    }
}
/// Only the claim owner module can change the active physical producer.
#[derive(Debug, Default)]
pub(super) struct ClaimFlag(Option<Arc<Claim>>);
impl ClaimFlag {
    pub(super) fn is_claimed(&self) -> bool {
        self.0.is_some()
    }
    pub(super) fn current(&self) -> Option<&Arc<Claim>> {
        self.0.as_ref()
    }
}

thread_local! { static HELD_CLAIMS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn manager_key(manager: &StoreViewManager) -> usize {
    Arc::as_ptr(&manager.state).addr()
}
struct RunScope(usize);
impl RunScope {
    fn enter(manager: &StoreViewManager) -> Self {
        let key = manager_key(manager);
        HELD_CLAIMS.with(|held| held.borrow_mut().push(key));
        Self(key)
    }
}
impl Drop for RunScope {
    fn drop(&mut self) {
        HELD_CLAIMS.with(|held| {
            let mut held = held.borrow_mut();
            let pos = held
                .iter()
                .rposition(|key| *key == self.0)
                .expect("installed claim scope");
            held.remove(pos);
        });
    }
}
fn claim_held_by_current_thread(manager: &StoreViewManager) -> bool {
    HELD_CLAIMS.with(|held| held.borrow().contains(&manager_key(manager)))
}

/// Exact owned claim; dropping an abandoned producer settles its reply and
/// releases only this incarnation, with every wake delivered after unlocking.
pub(super) struct BuildClaimGuard {
    manager: StoreViewManager,
    claim: Arc<Claim>,
    settled: bool,
}
impl BuildClaimGuard {
    #[cfg(test)]
    pub(super) fn claim(
        manager: &StoreViewManager,
        state: &mut StoreViewManagerState,
    ) -> (Self, u64) {
        Self::claim_for(manager, state, None, true)
    }
    fn claim_for(
        manager: &StoreViewManager,
        state: &mut StoreViewManagerState,
        task: Option<ExecutionTask>,
        compatibility: bool,
    ) -> (Self, u64) {
        assert!(
            !state.building.is_claimed(),
            "one physical store-view producer"
        );
        let claim = Arc::new(Claim {
            task,
            reset_generation: state.reset_generation,
            started: std::sync::atomic::AtomicBool::new(false),
            revoked: std::sync::atomic::AtomicBool::new(false),
            compatibility,
            reply: Arc::new(parking_lot::Mutex::new(Reply::default())),
        });
        state.building.0 = Some(claim.clone());
        (
            Self {
                manager: manager.clone(),
                claim,
                settled: false,
            },
            state.reset_generation,
        )
    }
    fn start(&self) -> bool {
        let state = self.manager.state.lock();
        if self.claim.revoked.load(Ordering::Relaxed)
            || !state
                .building
                .current()
                .is_some_and(|claim| Arc::ptr_eq(claim, &self.claim))
        {
            return false;
        }
        self.claim.started.store(true, Ordering::Relaxed);
        true
    }
    fn release(&mut self, failure: Option<StoreViewError>) -> TakenWakers {
        self.settled = true;
        let mut state = self.manager.state.lock();
        let mut reply = self.claim.reply.lock();
        if let Some(failure) = failure {
            reply.failure.get_or_insert(failure);
            if reply.result.is_none() {
                reply.result = Some(Err(failure));
            }
        }
        let mut wakes = reply.wakers.take();
        if state
            .building
            .current()
            .is_some_and(|claim| Arc::ptr_eq(claim, &self.claim))
        {
            state.building.0 = None;
            state.build_releases = state.build_releases.wrapping_add(1);
            wakes.append(state.build_waiters.take());
        }
        wakes
    }
}
impl Drop for BuildClaimGuard {
    fn drop(&mut self) {
        if !self.settled {
            let failure = if self.claim.started.load(Ordering::Relaxed) && std::thread::panicking()
            {
                StoreViewError::ProducerPanicked
            } else {
                StoreViewError::ProducerDropped
            };
            self.release(Some(failure)).wake();
        }
    }
}

/// Called with the manager lock held; the caller delivers notifications.
pub(super) fn reset(state: &mut StoreViewManagerState) -> TakenWakers {
    let mut wakes = state.build_waiters.take();
    if let Some(claim) = state.building.current().cloned() {
        let mut reply = claim.reply.lock();
        if !claim.compatibility && reply.result.is_none() {
            reply.result = Some(Err(StoreViewError::Reset));
            reply.failure = Some(StoreViewError::Reset);
        }
        wakes.append(reply.wakers.take());
        if !claim.started.load(Ordering::Relaxed) {
            claim.revoked.store(true, Ordering::Relaxed);
            reply.failure = Some(StoreViewError::Reset);
            if reply.result.is_none() {
                reply.result = Some(Err(StoreViewError::Reset));
            }
            state.building.0 = None;
            state.build_releases = state.build_releases.wrapping_add(1);
        }
    }
    wakes
}

#[derive(Clone, Copy)]
enum Phase {
    Main,
    Final,
    Last,
}
enum Step<H: HostRef> {
    Produce(Producer<H>),
    Value(StoreViewValue),
    Reentrant(StoreViewRead),
}
struct Core<H: HostRef> {
    host: Option<H>,
    manager: Option<StoreViewManager>,
    waiter: Option<ExecutionTask>,
    stop: Option<CancellationSubscription>,
    desired_reset: u64,
    compatibility: bool,
    phase: Phase,
    rounds: usize,
    after_join: bool,
    fallback: Option<(HostStoreView, StoreViewReturnOnlyReason)>,
    join: Option<BuildRelease>,
    reply: Option<Arc<parking_lot::Mutex<Reply>>>,
    registration: WakeRegistration,
    edge: Option<WaitEdge>,
    finished: bool,
}
impl<H: HostRef> Core<H> {
    fn new(host: H, waiter: ExecutionTask, stop: CancellationToken, compatibility: bool) -> Self {
        let manager = host.host().store_view_manager().clone();
        let desired_reset = manager.state.lock().reset_generation;
        Self {
            host: Some(host),
            manager: Some(manager),
            waiter: Some(waiter),
            stop: Some(stop.subscribe()),
            desired_reset,
            compatibility,
            phase: Phase::Main,
            rounds: 0,
            after_join: false,
            fallback: None,
            join: None,
            reply: None,
            registration: WakeRegistration::default(),
            edge: None,
            finished: false,
        }
    }
    fn cleanup(&mut self) {
        self.edge = None;
        self.stop = None;
        self.waiter = None;
        self.join = None;
        if let Some(reply) = self.reply.take() {
            reply.lock().wakers.withdraw(&mut self.registration);
        }
        self.fallback = None;
        self.manager = None;
        if let Some(host) = self.host.take() {
            host.release();
        }
    }
    fn finish(
        &mut self,
        answer: Result<StoreViewRead, StoreViewError>,
    ) -> Poll<Result<Step<H>, StoreViewError>> {
        self.finished = true;
        self.cleanup();
        Poll::Ready(answer.map(|read| Step::Value(StoreViewValue { read })))
    }
    fn poll(&mut self, waker: &Waker) -> Poll<Result<Step<H>, StoreViewError>> {
        assert!(!self.finished, "store-view handoff already completed");
        if self
            .stop
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
        {
            return self.finish(Err(StoreViewError::Cancelled));
        }
        loop {
            let manager = self.manager.as_ref().expect("live manager").clone();
            if !self.compatibility && manager.state.lock().reset_generation != self.desired_reset {
                return self.finish(Err(StoreViewError::Reset));
            }
            if let Some(reply) = &self.reply {
                let answer = {
                    let mut reply = reply.lock();
                    if reply.result.is_none() {
                        reply.wakers.register(&mut self.registration, waker);
                    }
                    let answer = reply.result.take();
                    if answer.is_some() {
                        reply.wakers.withdraw(&mut self.registration);
                    }
                    answer
                };
                let Some(answer) = answer else {
                    return Poll::Pending;
                };
                self.edge = None;
                self.reply = None;
                match answer {
                    Ok(Attempt::Read(read @ StoreViewRead::ReturnOnly { .. })) => {
                        return self.finish(Ok(read))
                    }
                    Ok(Attempt::Published(view))
                    | Ok(Attempt::Read(StoreViewRead::Current(CurrentHostStoreView(view)))) => {
                        let state = manager.state.lock();
                        if !self.compatibility && state.reset_generation != self.desired_reset {
                            drop(state);
                            return self.finish(Err(StoreViewError::Reset));
                        }
                        let live = capture_store_view_validation_token(
                            self.host.as_ref().expect("live host").host(),
                        );
                        let current = state.cached.as_ref().is_some_and(|(token, cached)| {
                            *token == live && Arc::ptr_eq(&cached.snapshot, &view.snapshot)
                        });
                        drop(state);
                        if current {
                            return self
                                .finish(Ok(StoreViewRead::Current(CurrentHostStoreView(view))));
                        }
                        if matches!(self.phase, Phase::Last) {
                            return self.finish(Ok(StoreViewRead::ReturnOnly {
                                view,
                                reason: StoreViewReturnOnlyReason::PublishDeclined,
                            }));
                        }
                        self.fallback = Some((view, StoreViewReturnOnlyReason::PublishDeclined));
                        self.rounds += 1;
                    }
                    Ok(Attempt::Fenced(view, reason)) => {
                        if matches!(self.phase, Phase::Last) {
                            return self.finish(Ok(StoreViewRead::ReturnOnly { view, reason }));
                        }
                        self.fallback = Some((view, reason));
                        self.rounds += 1;
                    }
                    Err(error) if !self.compatibility => return self.finish(Err(error)),
                    Err(_) => self.rounds += 1,
                }
            }
            if let Some(join) = &mut self.join {
                if join.poll(waker).is_pending() {
                    return Poll::Pending;
                }
                let failure = join
                    .claim
                    .as_ref()
                    .and_then(|claim| claim.reply.lock().failure);
                self.edge = None;
                self.join = None;
                if !self.compatibility {
                    if let Some(error) = failure.filter(|error| *error != StoreViewError::Reset) {
                        return self.finish(Err(error));
                    }
                }
                match self.phase {
                    Phase::Main => self.rounds += 1,
                    Phase::Final => self.after_join = true,
                    Phase::Last => {}
                }
            }
            if self.rounds >= STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
                if let Some((view, reason)) = self.fallback.take() {
                    return self.finish(Ok(StoreViewRead::ReturnOnly { view, reason }));
                }
                self.phase = match self.phase {
                    Phase::Main => Phase::Final,
                    _ => Phase::Last,
                };
                self.rounds = 0;
            }
            let host = self.host.as_ref().expect("live host").clone();
            let mut state = manager.state.lock();
            if !self.compatibility && state.reset_generation != self.desired_reset {
                drop(state);
                return self.finish(Err(StoreViewError::Reset));
            }
            #[cfg(test)]
            if matches!(self.phase, Phase::Main)
                && FORCE_WARM_PROBE_TOKEN_BUMP.with(|c| {
                    let value = c.get();
                    c.set(false);
                    value
                })
            {
                host.host().bump_store_view_epoch();
            }
            let cached = if matches!(self.phase, Phase::Main) {
                let live = capture_store_view_validation_token(host.host());
                state
                    .cached
                    .as_ref()
                    .filter(|(token, _)| *token == live)
                    .map(|(_, view)| StoreViewRead::Current(CurrentHostStoreView(view.clone())))
            } else {
                StoreViewManager::current_or_stale_cached_read(host.host(), &state)
            };
            let maintenance = match cached {
                Some(read)
                    if !host
                        .host()
                        .project_type_store()
                        .indexed()
                        .root_release_reclamation_pending() =>
                {
                    drop(state);
                    return self.finish(Ok(read));
                }
                read => read,
            };
            if state.building.is_claimed() {
                if self.compatibility && claim_held_by_current_thread(&manager) {
                    let view = state
                        .cached
                        .as_ref()
                        .map_or_else(HostStoreView::default, |(_, view)| view.clone());
                    drop(state);
                    self.finished = true;
                    self.cleanup();
                    return Poll::Ready(Ok(Step::Reentrant(StoreViewRead::ReturnOnly {
                        view,
                        reason: StoreViewReturnOnlyReason::ReentrantBuildClaim,
                    })));
                }
                if matches!(self.phase, Phase::Final) && self.after_join {
                    self.after_join = false;
                    self.rounds += 1;
                    drop(state);
                    continue;
                }
                let producer = state
                    .building
                    .current()
                    .and_then(|claim| claim.task.clone());
                let mut join = BuildRelease::new(&manager, &state);
                #[cfg(test)]
                {
                    state.parked_waiters += 1;
                    join.counted = true;
                }
                drop(state);
                let _ = join.poll(waker);
                if let Some(producer) = producer {
                    match self
                        .waiter
                        .as_ref()
                        .expect("live waiter")
                        .wait_for(&producer)
                    {
                        Ok(edge) => self.edge = Some(edge),
                        Err(_) => {
                            drop(join);
                            return self.finish(Err(StoreViewError::Cycle));
                        }
                    }
                } else if !self.compatibility {
                    drop(join);
                    return self.finish(Err(StoreViewError::ProducerUnowned));
                }
                self.join = Some(join);
                continue;
            }
            let task = manager
                .registry
                .as_ref()
                .unwrap_or_else(|| self.waiter.as_ref().expect("live waiter").registry())
                .register_task();
            // Register before publishing a claim. Refusal cannot abandon a
            // visible producer and invoke sibling wakers inside this poll.
            match self.waiter.as_ref().expect("live waiter").wait_for(&task) {
                Ok(edge) => self.edge = Some(edge),
                Err(_) if !self.compatibility => {
                    drop(state);
                    return self.finish(Err(StoreViewError::Cycle));
                }
                Err(_) => {}
            }
            let (claim, _) = BuildClaimGuard::claim_for(
                &manager,
                &mut state,
                Some(task.clone()),
                self.compatibility,
            );
            drop(state);
            let producer = Producer {
                host: Some(host),
                claim: Some(claim),
                cached_read: maintenance,
            };
            self.reply = Some(
                producer
                    .claim
                    .as_ref()
                    .expect("live claim")
                    .claim
                    .reply
                    .clone(),
            );
            return Poll::Ready(Ok(Step::Produce(producer)));
        }
    }
}
impl<H: HostRef> Drop for Core<H> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

struct Producer<H: HostRef> {
    host: Option<H>,
    claim: Option<BuildClaimGuard>,
    cached_read: Option<StoreViewRead>,
}
impl<H: HostRef> Producer<H> {
    fn run(mut self) {
        let mut claim = self.claim.take().expect("one producer");
        if !claim.start() {
            claim.settled = true;
            return;
        }
        let host = self.host.take().expect("producer host");
        let _scope = claim
            .claim
            .task
            .as_ref()
            .map(|task| ExecutionScope::install(task.registry(), task));
        let _held = RunScope::enter(&claim.manager);
        let answer = if let Some(read) = self.cached_read.take() {
            host.host()
                .project_type_store()
                .indexed()
                .reclaim_after_root_release();
            Attempt::Read(read)
        } else {
            match HostStoreView::build_coherent(host.host(), None) {
                SnapshotBuildOutcome::Coherent { view, token } => {
                    let mut state = claim.manager.state.lock();
                    match claim.manager.publish_coherent(
                        &mut state,
                        host.host(),
                        claim.claim.reset_generation,
                        token,
                        view,
                    ) {
                        PublishOutcome::Published { view } => Attempt::Published(view),
                        PublishOutcome::Declined { view } => {
                            Attempt::Fenced(view, StoreViewReturnOnlyReason::PublishDeclined)
                        }
                    }
                }
                SnapshotBuildOutcome::Superseded { view } => {
                    Attempt::Fenced(view, StoreViewReturnOnlyReason::Superseded)
                }
            }
        };
        let mut wakes = {
            let mut reply = claim.claim.reply.lock();
            if reply.result.is_none() {
                reply.result = Some(Ok(answer));
            }
            reply.wakers.take()
        };
        wakes.append(claim.release(None));
        host.host()
            .project_type_store()
            .indexed()
            .reclaim_after_root_release();
        // The running-scope guard must leave before a callback can re-enter.
        drop(_held);
        drop(_scope);
        drop(claim);
        wakes.wake();
    }
}

pub(super) async fn pollable(host: &VerterHost) -> StoreViewRead {
    let scope = host.scheduler.enter_execution_scope();
    let mut core = Core::new(host, scope.task().clone(), CancellationToken::new(), true);
    drop(scope);
    loop {
        match std::future::poll_fn(|cx| core.poll(cx.waker())).await {
            Ok(Step::Produce(producer)) => producer.run(),
            Ok(Step::Value(value)) => return value.into_read(),
            Ok(Step::Reentrant(read)) => return report_reentrant(read),
            Err(_) => {
                let view = host
                    .store_view_manager()
                    .state
                    .lock()
                    .cached
                    .as_ref()
                    .map_or_else(HostStoreView::default, |(_, view)| view.clone());
                return StoreViewRead::ReturnOnly {
                    view,
                    reason: StoreViewReturnOnlyReason::ReentrantBuildClaim,
                };
            }
        }
    }
}

pub(super) fn drive(manager: &StoreViewManager, host: &VerterHost) -> StoreViewRead {
    let scope = host.scheduler.enter_execution_scope();
    let mut core = Core::new(host, scope.task().clone(), CancellationToken::new(), true);
    // The shared manager must be the host's exact cache owner.
    verter_debug_assert::verter_debug_assert!(Arc::ptr_eq(
        &manager.state,
        &core.manager.as_ref().expect("manager").state
    ));
    loop {
        match verter_execution::blocking::block_on_when_pending(
            verter_execution::blocking::WaitFamily::StoreViewBuild,
            |waker| core.poll(waker),
        ) {
            Ok(Step::Produce(producer)) => producer.run(),
            Ok(Step::Value(value)) => {
                let read = value.into_read();
                host.project_type_store()
                    .indexed()
                    .reclaim_after_root_release();
                return read;
            }
            Ok(Step::Reentrant(read)) => return report_reentrant(read),
            Err(_) => {
                let view = manager
                    .state
                    .lock()
                    .cached
                    .as_ref()
                    .map_or_else(HostStoreView::default, |(_, view)| view.clone());
                return StoreViewRead::ReturnOnly {
                    view,
                    reason: StoreViewReturnOnlyReason::ReentrantBuildClaim,
                };
            }
        }
    }
}
fn report_reentrant(read: StoreViewRead) -> StoreViewRead {
    #[cfg(test)]
    {
        SELF_AWAIT_REFUSALS_THIS_THREAD.with(|count| count.set(count.get().saturating_add(1)));
        LAST_SELF_AWAIT_REFUSAL_THIS_THREAD
            .with(|slot| slot.set(read.return_only_reason_for_tests()));
    }
    tracing::error!(target: "verter::store_view", "store-view build re-entered its own claim; refusing self-await");
    verter_debug_assert!(
        false,
        "STORE-VIEW SELF-AWAIT: a build re-entered its own store-view claim"
    );
    read
}

#[cfg(test)]
#[path = "store_view_handoff_tests.rs"]
mod store_view_handoff_tests;
