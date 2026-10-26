//! Cheap, clonable, thread-safe cancellation flag.
//!
//! A [`CancellationToken`] is a one-shot latch shared between a request
//! handle and the work it admitted. A token rides on a work item; its
//! dispatch path checks the flag, and the owning handle trips it on
//! `Drop`, so dropping a handle cancels its still-pending work.
//!
//! A request token is a one-shot atomic latch. A scheduler job may instead
//! use an aggregate token whose live-owner registrations keep shared work
//! alive until every attached request has cancelled or detached.
//!
//! A pollable computation that waits on something else also waits on its
//! cancellation: [`CancellationToken::subscribe`] gives it a
//! [`CancellationSubscription`] whose poll registers the computation's waker
//! with the token, so the cancellation itself wakes the pending poll instead
//! of a timer re-polling it. An aggregate token wakes its subscribers when it
//! becomes cancelled: when its last live owner's request cancels, or when its
//! last live owner detaches.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::task::{Poll, Wake, Waker};

use parking_lot::Mutex;

use crate::wake::{TakenWakers, WakeRegistration, WakerSet};

/// Delivery rail for an aggregate owned by a scheduler with outer lifecycle
/// locks. Cancellation still latches immediately; callbacks can wait for unlock.
pub trait CancellationWakeDelivery: Send + Sync + std::fmt::Debug {
    /// Deliver these registrations once the owner's lifecycle holds release.
    fn deliver(&self, wakers: TakenWakers);
}

/// A cheap, clonable, thread-safe one-shot cancellation flag.
///
/// All clones share the same underlying flag — cancelling any clone is
/// observed by every other clone. `cancel()` is idempotent.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

#[derive(Debug)]
struct CancellationState {
    cancelled: AtomicBool,
    owners: Option<AggregateOwners>,
    /// The polls waiting for this token to cancel. Taken, and woken after
    /// the lock is released, by whoever sets `cancelled` first. Delivery
    /// never locks this while an enclosing aggregate's `entries` are held.
    wakers: Mutex<WakerSet>,
}

impl CancellationState {
    fn new(owners: Option<AggregateOwners>) -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            owners,
            wakers: Mutex::new(WakerSet::default()),
        })
    }

    /// Set the flag. The setter that flips it delivers every waiting poll,
    /// outside the waker lock.
    fn trip(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            let taken = self.wakers.lock().take();
            self.deliver(taken);
        }
    }

    fn deliver(&self, taken: TakenWakers) {
        match self
            .owners
            .as_ref()
            .and_then(|owners| owners.delivery.get())
        {
            Some(delivery) => delivery.deliver(taken),
            None => taken.wake(),
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Probes [`CancellationToken::is_cancelled`] opened on this thread.
    static PROBES_OPENED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Probes may traverse nested aggregates while owner or subscription locks
/// are held. Latch immediately, but deliver every discovered cancellation
/// only after the outermost probe releases all of those locks.
#[derive(Default)]
struct ProbedCancellations(Vec<Arc<CancellationState>>);

impl ProbedCancellations {
    fn take_wakers(&mut self) -> TakenWakers {
        let mut taken = WakerSet::default().take();
        for state in self.0.drain(..) {
            taken.append(state.wakers.lock().take());
        }
        taken
    }
}

impl Drop for ProbedCancellations {
    fn drop(&mut self) {
        // Nearly every probe latches nothing: skip the drain machinery.
        if self.0.is_empty() {
            return;
        }
        for state in self.0.drain(..) {
            let taken = state.wakers.lock().take();
            state.deliver(taken);
        }
    }
}

#[derive(Debug)]
struct AggregateOwners {
    delivery: OnceLock<Arc<dyn CancellationWakeDelivery>>,
    ever_registered: AtomicBool,
    entries: Mutex<OwnerEntries>,
    /// The waker each owner's request token wakes when it cancels: it
    /// re-evaluates this aggregate for its own subscribers. Built once.
    request_wake: OnceLock<Waker>,
}

/// Woken by an owner's request token as it cancels: delivers the
/// aggregate's own subscribers if no live owner remains.
struct AggregateWake(Weak<CancellationState>);

impl Wake for AggregateWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(state) = self.0.upgrade() {
            CancellationToken { state }.deliver_if_cancelled();
        }
    }
}

/// Registered owners of one aggregate token. An owner that has detached,
/// dropped or seen its request cancelled is dead for good, so the liveness
/// probe pops dead owners off the end until it meets a live one: each
/// registration is examined as dead at most once, and the probe never
/// rescans the owners that remain.
///
/// Dead owners buried under a live tail are retired by registration: once
/// the list reaches twice the live owners the previous compaction kept, the
/// next registration keeps only live owners. Each compaction examines at
/// most twice as many entries as registrations since the last one, so the
/// work stays amortized constant per registration, and the list never
/// exceeds twice the owners live at the last compaction (at least
/// [`MIN_OWNER_COMPACTION`]) however many owners have come and gone.
#[derive(Debug)]
struct OwnerEntries {
    owners: Vec<Weak<CancellationOwnerState>>,
    /// List length at which the next registration compacts.
    compact_at: usize,
    /// Owner entries the liveness probe and compaction have examined.
    /// Tests only.
    #[cfg(test)]
    examined: u64,
}

/// Smallest owner-list length that triggers a compaction.
const MIN_OWNER_COMPACTION: usize = 8;

impl Default for OwnerEntries {
    fn default() -> Self {
        Self {
            owners: Vec::new(),
            compact_at: MIN_OWNER_COMPACTION,
            #[cfg(test)]
            examined: 0,
        }
    }
}

impl OwnerEntries {
    /// Whether any registered owner is still live, discarding the dead
    /// owners met on the way.
    fn any_live(&mut self, probed: &mut ProbedCancellations) -> bool {
        while let Some(last) = self.owners.last() {
            #[cfg(test)]
            {
                self.examined += 1;
            }
            if owner_is_live(last, probed) {
                return true;
            }
            self.owners.pop();
        }
        false
    }

    /// Register `owner`, first retiring every dead owner when the list has
    /// reached its compaction length.
    fn push(&mut self, owner: Weak<CancellationOwnerState>, probed: &mut ProbedCancellations) {
        if self.owners.len() >= self.compact_at {
            #[cfg(test)]
            {
                self.examined += self.owners.len() as u64;
            }
            self.owners.retain(|owner| owner_is_live(owner, probed));
            self.compact_at = (2 * self.owners.len()).max(MIN_OWNER_COMPACTION);
            self.owners.shrink_to(self.compact_at);
        }
        self.owners.push(owner);
    }
}

fn owner_is_live(owner: &Weak<CancellationOwnerState>, probed: &mut ProbedCancellations) -> bool {
    owner.upgrade().is_some_and(|owner| owner.is_live(probed))
}

#[derive(Debug)]
struct CancellationOwnerState {
    active: AtomicBool,
    request: Option<CancellationToken>,
    /// The aggregate's wake registered on `request`, withdrawn when the
    /// owner detaches.
    request_wake: Mutex<WakeRegistration>,
}

/// One live requester attached to an aggregate job token.
///
/// Dropping the registration detaches only this requester. The aggregate
/// token becomes cancelled once it has had at least one owner and no attached,
/// uncancelled owner remains.
#[derive(Debug)]
pub struct CancellationOwner {
    state: Arc<CancellationOwnerState>,
    /// The aggregate token this owner keeps live.
    aggregate: Weak<CancellationState>,
}

/// A poll's wait for one token's cancellation.
///
/// [`Self::poll`] answers `Ready` once the token is cancelled, and
/// otherwise registers the waker with the token in the same critical
/// section the cancellation takes to deliver, so a cancellation racing the
/// poll still wakes it. A wake only schedules a later poll. Re-polling with
/// a waker that wakes the same task keeps the registration; dropping the
/// subscription withdraws exactly it and cancels nothing.
#[derive(Debug)]
pub struct CancellationSubscription {
    token: CancellationToken,
    registration: WakeRegistration,
}

impl CancellationToken {
    /// Creates a fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: CancellationState::new(None),
        }
    }

    /// Create a job-liveness token whose cancellation is the aggregate of
    /// its registered request owners. An ownerless internal job remains live;
    /// after the first registration, loss/cancellation of every owner trips
    /// the one-shot token.
    pub fn aggregate() -> Self {
        Self {
            state: CancellationState::new(Some(AggregateOwners {
                delivery: OnceLock::new(),
                ever_registered: AtomicBool::new(false),
                entries: Mutex::new(OwnerEntries::default()),
                request_wake: OnceLock::new(),
            })),
        }
    }

    /// Bind an aggregate's cancellation callbacks to its owner's delivery rail.
    /// Install before exposing subscriptions. The first binding wins; returns
    /// `false` if already bound. Request tokens cannot be bound.
    pub fn set_wake_delivery(&self, delivery: Arc<dyn CancellationWakeDelivery>) -> bool {
        self.state
            .owners
            .as_ref()
            .expect("only aggregate tokens have an owner delivery rail")
            .delivery
            .set(delivery)
            .is_ok()
    }

    /// Attach one request token to this aggregate job token. `None` denotes
    /// an uncancellable owner that stays live until the registration drops.
    pub fn register_owner(&self, request: Option<CancellationToken>) -> Option<CancellationOwner> {
        self.register_owner_probed(request, &mut ProbedCancellations::default())
    }

    /// Register beneath an outer owner lock. Deliver the returned notifications
    /// after releasing that lock, using the owner's wake delivery rail.
    pub fn register_owner_deferred(
        &self,
        request: Option<CancellationToken>,
    ) -> (Option<CancellationOwner>, TakenWakers) {
        let mut probed = ProbedCancellations::default();
        let owner = self.register_owner_probed(request, &mut probed);
        (owner, probed.take_wakers())
    }

    fn register_owner_probed(
        &self,
        request: Option<CancellationToken>,
        probed: &mut ProbedCancellations,
    ) -> Option<CancellationOwner> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return None;
        }
        let state = Arc::new(CancellationOwnerState::new(request));
        self.register_owner_states(std::slice::from_ref(&state), probed)
            .then(|| CancellationOwner {
                state,
                aggregate: Arc::downgrade(&self.state),
            })
    }

    /// Attach existing request owners in input order as one registration.
    /// Cancellation probes cannot observe a partially registered batch. A
    /// previously ownerless job stays live if any request in the batch is live;
    /// a job that already lost all its owners cannot be revived. An empty batch
    /// does not turn an ownerless internal job into an owned job.
    pub fn register_owners(
        &self,
        requests: impl IntoIterator<Item = Option<CancellationToken>>,
    ) -> Option<Vec<CancellationOwner>> {
        self.register_owners_probed(requests, &mut ProbedCancellations::default())
    }

    /// Atomically attach an owner batch beneath an outer owner lock. Deliver
    /// the returned notifications only after releasing that lock.
    pub fn register_owners_deferred(
        &self,
        requests: impl IntoIterator<Item = Option<CancellationToken>>,
    ) -> (Option<Vec<CancellationOwner>>, TakenWakers) {
        let mut probed = ProbedCancellations::default();
        let owners = self.register_owners_probed(requests, &mut probed);
        (owners, probed.take_wakers())
    }

    fn register_owners_probed(
        &self,
        requests: impl IntoIterator<Item = Option<CancellationToken>>,
        probed: &mut ProbedCancellations,
    ) -> Option<Vec<CancellationOwner>> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return None;
        }
        let states: Vec<_> = requests
            .into_iter()
            .map(|request| Arc::new(CancellationOwnerState::new(request)))
            .collect();
        self.register_owner_states(&states, probed).then(|| {
            states
                .into_iter()
                .map(|state| CancellationOwner {
                    state,
                    aggregate: Arc::downgrade(&self.state),
                })
                .collect()
        })
    }

    fn register_owner_states(
        &self,
        states: &[Arc<CancellationOwnerState>],
        probed: &mut ProbedCancellations,
    ) -> bool {
        let owners = self
            .state
            .owners
            .as_ref()
            .expect("request owners can be registered only on aggregate tokens");
        if self.state.cancelled.load(Ordering::Acquire) {
            return false;
        }
        let mut entries = owners.entries.lock();
        // Linearize registration against is_cancelled: a previously owned
        // aggregate cannot be revived after its last live owner leaves.
        if self.state.cancelled.load(Ordering::Acquire) {
            return false;
        }
        if owners.ever_registered.load(Ordering::Acquire) && !entries.any_live(probed) {
            if !self.state.cancelled.swap(true, Ordering::AcqRel) {
                probed.0.push(Arc::clone(&self.state));
            }
            drop(entries);
            return false;
        }
        for state in states {
            entries.push(Arc::downgrade(state), probed);
        }
        if !states.is_empty() {
            owners.ever_registered.store(true, Ordering::Release);
        }
        drop(entries);
        // The owner's request cancelling may leave this aggregate with no
        // live owner: its token wakes the aggregate's re-evaluation.
        for state in states {
            let Some(request) = state.request.as_ref() else {
                continue;
            };
            let wake = owners
                .request_wake
                .get_or_init(|| Waker::from(Arc::new(AggregateWake(Arc::downgrade(&self.state)))));
            request
                .state
                .wakers
                .lock()
                .register(&mut state.request_wake.lock(), wake);
            if request.probe(probed) {
                self.probe(probed);
            }
        }
        true
    }

    /// Trips the flag. Idempotent — calling more than once leaves the
    /// token cancelled and never panics or toggles back. The first call
    /// wakes every pending [`CancellationSubscription`], after releasing the
    /// token's waker lock.
    pub fn cancel(&self) {
        self.state.trip();
    }

    /// Latch stop immediately while retaining callbacks for delivery after an
    /// enclosing owner lock releases.
    pub fn cancel_deferred(&self) -> TakenWakers {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.wakers.lock().take()
    }

    /// A wait for this token's cancellation, for a poll that must wake when
    /// it cancels.
    #[must_use]
    pub fn subscribe(&self) -> CancellationSubscription {
        CancellationSubscription {
            token: self.clone(),
            registration: WakeRegistration::default(),
        }
    }

    /// Deliver this token's subscribers if it is cancelled now. A token no
    /// poll waits on is left to its next probe.
    fn deliver_if_cancelled(&self) {
        if self.state.wakers.lock().is_empty() {
            return;
        }
        let _ = self.is_cancelled();
    }

    /// Whether the token is cancelled, setting the flag (without delivering
    /// its subscribers) when an aggregate has just lost its last live owner.
    /// Adds each newly latched aggregate to `probed` for delivery once the
    /// caller releases its locks. Nested owner probes share this collection.
    fn probe(&self, probed: &mut ProbedCancellations) -> bool {
        if self.state.cancelled.load(Ordering::Acquire) {
            return true;
        }
        let Some(owners) = self.state.owners.as_ref() else {
            return false;
        };
        let mut entries = owners.entries.lock();
        if !owners.ever_registered.load(Ordering::Acquire) || entries.any_live(probed) {
            return false;
        }
        // Latch under the registration lock so a late owner cannot revive
        // the aggregate in the gap between probing and publishing the flag.
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            probed.0.push(Arc::clone(&self.state));
        }
        true
    }

    /// Whether two handles name the same cancellation source: clones of one
    /// token agree, independent tokens never do. Identity is content-free —
    /// it never observes the cancelled flag.
    #[must_use]
    pub fn same_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Returns whether the token has been cancelled.
    ///
    /// Every semantic checkpoint asks this. A latched token and a request
    /// token (which has no owners to probe) answer from the flag alone;
    /// only an unlatched aggregate opens a probe, whose discoveries are
    /// delivered once it releases its locks.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        if self.state.cancelled.load(Ordering::Acquire) {
            return true;
        }
        if self.state.owners.is_none() {
            return false;
        }
        #[cfg(test)]
        PROBES_OPENED.with(|opened| opened.set(opened.get() + 1));
        self.probe(&mut ProbedCancellations::default())
    }

    /// Whether this is an aggregate job token that has accepted at least one
    /// request owner. Ownerless DAG tokens use the installed request token for
    /// semantic cancellation; scoped shared jobs use this aggregate instead.
    #[must_use]
    pub fn has_registered_owners(&self) -> bool {
        match &self.state.owners {
            Some(owners) => owners.ever_registered.load(Ordering::Acquire),
            None => false,
        }
    }
}

impl CancellationOwnerState {
    fn new(request: Option<CancellationToken>) -> Self {
        Self {
            active: AtomicBool::new(true),
            request,
            request_wake: Mutex::new(WakeRegistration::default()),
        }
    }

    /// Attached and its request (if any) not cancelled. Once false, never
    /// true again.
    fn is_live(&self, probed: &mut ProbedCancellations) -> bool {
        self.active.load(Ordering::Acquire)
            && self
                .request
                .as_ref()
                .is_none_or(|request| !request.probe(probed))
    }
}

impl CancellationOwner {
    /// Detach this requester from the aggregate job. Idempotent. When it
    /// was the aggregate's last live owner, the aggregate's subscribers are
    /// woken.
    pub(crate) fn detach(&self) {
        self.detach_probed(&mut ProbedCancellations::default());
    }

    /// Withdraw this owner under an outer lock without calling its subscribers.
    pub fn detach_deferred(&self) -> TakenWakers {
        let mut probed = ProbedCancellations::default();
        self.detach_probed(&mut probed);
        probed.take_wakers()
    }

    fn detach_probed(&self, probed: &mut ProbedCancellations) {
        if !self.state.active.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(request) = self.state.request.as_ref() {
            request
                .state
                .wakers
                .lock()
                .withdraw(&mut self.state.request_wake.lock());
        }
        if let Some(state) = self.aggregate.upgrade() {
            let observed = !state.wakers.lock().is_empty();
            if observed {
                CancellationToken { state }.probe(probed);
            }
        }
    }
}

impl Drop for CancellationOwner {
    fn drop(&mut self) {
        self.detach();
    }
}

impl CancellationSubscription {
    /// The token this subscription waits on.
    #[must_use]
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// `Ready` once the token is cancelled; otherwise `Pending` with
    /// `waker` registered to be woken when it is.
    pub fn poll(&mut self, waker: &Waker) -> Poll<()> {
        let state = &self.token.state;
        let mut probed = ProbedCancellations::default();
        let mut wakers = state.wakers.lock();
        if self.token.probe(&mut probed) {
            wakers.withdraw(&mut self.registration);
            Poll::Ready(())
        } else {
            wakers.register(&mut self.registration, waker);
            Poll::Pending
        }
    }
}

impl Drop for CancellationSubscription {
    fn drop(&mut self) {
        if self.registration.is_registered() {
            self.token
                .state
                .wakers
                .lock()
                .withdraw(&mut self.registration);
        }
    }
}

thread_local! {
    static CURRENT_JOB_CANCELLATION: RefCell<Option<CancellationToken>> =
        const { RefCell::new(None) };
}

/// Return the aggregate cancellation token for the scheduler job executing on
/// this thread, if the current work is scheduler-owned.
#[must_use]
pub fn current_job_cancellation_token() -> Option<CancellationToken> {
    CURRENT_JOB_CANCELLATION.with(|slot| slot.borrow().clone())
}

/// Read the aggregate cancellation token of the scheduler job executing on
/// this thread, if any, without taking a reference to it. A checkpoint that
/// only asks whether the token is cancelled reads it in place.
pub fn with_current_job_cancellation_token<R>(
    read: impl FnOnce(Option<&CancellationToken>) -> R,
) -> R {
    CURRENT_JOB_CANCELLATION.with(|slot| read(slot.borrow().as_ref()))
}

/// Stack-safe TLS guard for one scheduler-owned job cancellation token.
pub struct JobCancellationGuard {
    previous: Option<CancellationToken>,
}

impl JobCancellationGuard {
    /// Install `token` for the current job and restore the prior token on drop.
    #[must_use]
    pub fn install(token: CancellationToken) -> Self {
        Self::replace(Some(token))
    }

    /// Install `token` — or no job token at all — for work this thread runs
    /// on behalf of a job that began elsewhere, and restore the prior token
    /// on drop. Work suspended under one job and resumed on another thread
    /// observes its own job's cancellation, never the thread's.
    #[must_use]
    pub fn replace(token: Option<CancellationToken>) -> Self {
        let previous = CURRENT_JOB_CANCELLATION.with(|slot| slot.replace(token));
        Self { previous }
    }
}

impl Drop for JobCancellationGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_JOB_CANCELLATION.with(|slot| {
            slot.replace(previous);
        });
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "cancellation_tests.rs"]
mod tests;
