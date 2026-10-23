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

use crate::wake::{WakeRegistration, WakerSet};

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
    /// the lock is released, by whoever sets `cancelled` first. Never locked
    /// while an aggregate's `entries` are held.
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
            taken.wake();
        }
    }
}

#[derive(Debug)]
struct AggregateOwners {
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
    fn any_live(&mut self) -> bool {
        while let Some(last) = self.owners.last() {
            #[cfg(test)]
            {
                self.examined += 1;
            }
            if owner_is_live(last) {
                return true;
            }
            self.owners.pop();
        }
        false
    }

    /// Register `owner`, first retiring every dead owner when the list has
    /// reached its compaction length.
    fn push(&mut self, owner: Weak<CancellationOwnerState>) {
        if self.owners.len() >= self.compact_at {
            #[cfg(test)]
            {
                self.examined += self.owners.len() as u64;
            }
            self.owners.retain(owner_is_live);
            self.compact_at = (2 * self.owners.len()).max(MIN_OWNER_COMPACTION);
            self.owners.shrink_to(self.compact_at);
        }
        self.owners.push(owner);
    }
}

fn owner_is_live(owner: &Weak<CancellationOwnerState>) -> bool {
    owner.upgrade().is_some_and(|owner| owner.is_live())
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
                ever_registered: AtomicBool::new(false),
                entries: Mutex::new(OwnerEntries::default()),
                request_wake: OnceLock::new(),
            })),
        }
    }

    /// Attach one request token to this aggregate job token. `None` denotes
    /// an uncancellable owner that stays live until the registration drops.
    pub fn register_owner(&self, request: Option<CancellationToken>) -> Option<CancellationOwner> {
        let owners = self
            .state
            .owners
            .as_ref()
            .expect("request owners can be registered only on aggregate tokens");
        if self.state.cancelled.load(Ordering::Acquire) {
            return None;
        }
        let state = Arc::new(CancellationOwnerState {
            active: AtomicBool::new(true),
            request,
            request_wake: Mutex::new(WakeRegistration::default()),
        });
        let mut entries = owners.entries.lock();
        // Linearize registration against `is_cancelled()`: once the aggregate
        // has ever had owners, a gap with no live owner is terminal. Detect
        // that gap here as well as in `is_cancelled()` so a late joiner cannot
        // revive a job merely because no worker happened to poll between the
        // final request cancellation and this registration attempt.
        if self.state.cancelled.load(Ordering::Acquire) {
            return None;
        }
        if owners.ever_registered.load(Ordering::Acquire) && !entries.any_live() {
            drop(entries);
            self.state.trip();
            return None;
        }
        entries.push(Arc::downgrade(&state));
        owners.ever_registered.store(true, Ordering::Release);
        drop(entries);
        // The owner's request cancelling may leave this aggregate with no
        // live owner: its token wakes the aggregate's re-evaluation.
        if let Some(request) = state.request.as_ref() {
            let wake = owners
                .request_wake
                .get_or_init(|| Waker::from(Arc::new(AggregateWake(Arc::downgrade(&self.state)))));
            request
                .state
                .wakers
                .lock()
                .register(&mut state.request_wake.lock(), wake);
            if request.is_cancelled() {
                self.deliver_if_cancelled();
            }
        }
        Some(CancellationOwner {
            state,
            aggregate: Arc::downgrade(&self.state),
        })
    }

    /// Trips the flag. Idempotent — calling more than once leaves the
    /// token cancelled and never panics or toggles back. The first call
    /// wakes every pending [`CancellationSubscription`], after releasing the
    /// token's waker lock.
    pub fn cancel(&self) {
        self.state.trip();
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
    /// Returns `(cancelled, newly)`, where `newly` means this probe set the
    /// flag and the caller must deliver. Takes the aggregate's `entries`
    /// only, so it runs under the token's waker lock.
    fn probe(&self) -> (bool, bool) {
        if self.state.cancelled.load(Ordering::Acquire) {
            return (true, false);
        }
        let Some(owners) = self.state.owners.as_ref() else {
            return (false, false);
        };
        if !owners.ever_registered.load(Ordering::Acquire) || owners.entries.lock().any_live() {
            return (false, false);
        }
        (true, !self.state.cancelled.swap(true, Ordering::AcqRel))
    }

    /// Whether two handles name the same cancellation source: clones of one
    /// token agree, independent tokens never do. Identity is content-free —
    /// it never observes the cancelled flag.
    #[must_use]
    pub fn same_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Returns whether the token has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        if self.state.cancelled.load(Ordering::Acquire) {
            return true;
        }
        let Some(owners) = self.state.owners.as_ref() else {
            return false;
        };
        if !owners.ever_registered.load(Ordering::Acquire) {
            return false;
        }

        if owners.entries.lock().any_live() {
            return false;
        }
        self.state.trip();
        true
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
    /// Attached and its request (if any) not cancelled. Once false, never
    /// true again.
    fn is_live(&self) -> bool {
        self.active.load(Ordering::Acquire)
            && self
                .request
                .as_ref()
                .is_none_or(|request| !request.is_cancelled())
    }
}

impl CancellationOwner {
    /// Detach this requester from the aggregate job. Idempotent. When it
    /// was the aggregate's last live owner, the aggregate's subscribers are
    /// woken.
    pub(crate) fn detach(&self) {
        self.state.active.store(false, Ordering::Release);
        if let Some(request) = self.state.request.as_ref() {
            request
                .state
                .wakers
                .lock()
                .withdraw(&mut self.state.request_wake.lock());
        }
        if let Some(state) = self.aggregate.upgrade() {
            CancellationToken { state }.deliver_if_cancelled();
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
        let mut wakers = state.wakers.lock();
        match self.token.probe() {
            (true, newly) => {
                wakers.withdraw(&mut self.registration);
                // This probe set an aggregate's flag under the lock its
                // delivery takes: deliver the siblings here, after unlock.
                let taken = newly.then(|| wakers.take());
                drop(wakers);
                if let Some(taken) = taken {
                    taken.wake();
                }
                Poll::Ready(())
            }
            (false, _) => {
                wakers.register(&mut self.registration, waker);
                Poll::Pending
            }
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
mod tests {
    use super::*;

    #[test]
    fn new_token_is_not_cancelled() {
        assert!(!CancellationToken::new().is_cancelled());
    }

    #[test]
    fn cancel_then_observe() {
        let t = CancellationToken::new();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent() {
        let t = CancellationToken::new();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let t = CancellationToken::new();
        let c = t.clone();
        t.cancel();
        assert!(c.is_cancelled(), "clone observes a cancel on the sibling");
    }

    #[test]
    fn default_matches_new() {
        assert!(!CancellationToken::default().is_cancelled());
    }

    #[test]
    fn aggregate_stays_live_while_any_request_owner_is_live() {
        let aggregate = CancellationToken::aggregate();
        let first_request = CancellationToken::new();
        let second_request = CancellationToken::new();
        let _first = aggregate
            .register_owner(Some(first_request.clone()))
            .expect("first owner registers");
        let _second = aggregate
            .register_owner(Some(second_request.clone()))
            .expect("second owner registers");

        first_request.cancel();
        assert!(!aggregate.is_cancelled());
        second_request.cancel();
        assert!(aggregate.is_cancelled());
    }

    #[test]
    fn late_owner_cannot_revive_an_unpolled_cancelled_aggregate() {
        let aggregate = CancellationToken::aggregate();
        let first_request = CancellationToken::new();
        let _first = aggregate
            .register_owner(Some(first_request.clone()))
            .expect("first owner registers");

        first_request.cancel();
        let late_request = CancellationToken::new();
        assert!(aggregate.register_owner(Some(late_request)).is_none());
        assert!(aggregate.is_cancelled());
    }

    fn examined(aggregate: &CancellationToken) -> u64 {
        aggregate
            .state
            .owners
            .as_ref()
            .expect("aggregate token")
            .entries
            .lock()
            .examined
    }

    /// Cancelling sibling owners one by one, polling after each, examines
    /// every registration a bounded number of times: the work grows
    /// linearly with the sibling count, never with the owners still
    /// registered.
    #[test]
    fn sibling_owner_cancellations_examine_each_owner_a_bounded_number_of_times() {
        let mut per_sibling = Vec::new();
        for siblings in [128_u64, 256, 512, 1024] {
            let aggregate = CancellationToken::aggregate();
            let requests: Vec<CancellationToken> =
                (0..siblings).map(|_| CancellationToken::new()).collect();
            let registrations: Vec<CancellationOwner> = requests
                .iter()
                .map(|request| {
                    aggregate
                        .register_owner(Some(request.clone()))
                        .expect("live aggregate accepts owners")
                })
                .collect();
            let before = examined(&aggregate);
            // Cancel from the oldest registration forward, then from the
            // newest back: both ends of the owner list are exercised.
            let half = requests.len() / 2;
            for request in requests[..half].iter().chain(requests[half..].iter().rev()) {
                assert!(!aggregate.is_cancelled() || request.is_cancelled());
                request.cancel();
            }
            assert!(aggregate.is_cancelled(), "no live owner remains");
            let visited = examined(&aggregate) - before;
            assert!(
                visited <= 3 * siblings,
                "{siblings} sibling cancellations examined {visited} owner entries",
            );
            per_sibling.push(visited / siblings);
            drop(registrations);
        }
        assert!(
            per_sibling.windows(2).all(|pair| pair[0] == pair[1]),
            "per-sibling probe work is independent of the sibling count: {per_sibling:?}",
        );
    }

    /// Dropping every registration lets the probe discard each owner, and
    /// later live owners are still observed.
    #[test]
    fn dropped_owners_are_discarded_and_a_live_owner_keeps_the_job() {
        let aggregate = CancellationToken::aggregate();
        let survivor_request = CancellationToken::new();
        let survivor = aggregate
            .register_owner(Some(survivor_request.clone()))
            .expect("first owner registers");
        let transient: Vec<CancellationOwner> = (0..64)
            .map(|_| aggregate.register_owner(None).expect("owner registers"))
            .collect();
        drop(transient);
        assert!(!aggregate.is_cancelled(), "the first owner is still live");
        let owners = aggregate.state.owners.as_ref().expect("aggregate token");
        assert_eq!(
            owners.entries.lock().owners.len(),
            1,
            "only the live owner stays registered",
        );
        survivor_request.cancel();
        assert!(aggregate.is_cancelled());
        assert!(owners.entries.lock().owners.is_empty());
        drop(survivor);
    }

    /// A job that is never left ownerless keeps a bounded owner list: each
    /// successor registers before its predecessor departs, so every
    /// departed owner sits below a live one, with or without a long-lived
    /// builder at the head. The retired-registration storage stays bounded
    /// by the live owners, and the work per handoff stays constant.
    #[test]
    fn rolling_owner_handoffs_retain_only_a_bounded_owner_list() {
        for with_builder in [false, true] {
            let mut per_handoff = Vec::new();
            for handoffs in [128_u64, 256, 512, 1024] {
                let aggregate = CancellationToken::aggregate();
                let builder = with_builder.then(|| {
                    aggregate
                        .register_owner(None)
                        .expect("the builder registers")
                });
                let mut request = CancellationToken::new();
                let mut current = aggregate
                    .register_owner(Some(request.clone()))
                    .expect("the first joiner registers");
                let before = examined(&aggregate);
                for _ in 0..handoffs {
                    let next_request = CancellationToken::new();
                    let next = aggregate
                        .register_owner(Some(next_request.clone()))
                        .expect("a successor joins a live job");
                    request.cancel();
                    drop(current);
                    assert!(!aggregate.is_cancelled(), "the successor keeps the job");
                    (request, current) = (next_request, next);
                }
                let owners = aggregate.state.owners.as_ref().expect("aggregate token");
                let entries = owners.entries.lock();
                assert!(
                    entries.owners.len() <= MIN_OWNER_COMPACTION
                        && entries.owners.capacity() <= 2 * MIN_OWNER_COMPACTION,
                    "{handoffs} handoffs with builder={with_builder} retain {} owner entries \
                     (capacity {})",
                    entries.owners.len(),
                    entries.owners.capacity(),
                );
                let visited = entries.examined - before;
                drop(entries);
                assert!(
                    visited <= 4 * handoffs,
                    "{handoffs} handoffs examined {visited} owner entries",
                );
                per_handoff.push(visited / handoffs);
                drop(current);
                drop(builder);
                assert!(aggregate.is_cancelled(), "no owner remains");
            }
            assert!(
                per_handoff.windows(2).all(|pair| pair[0] == pair[1]),
                "per-handoff work is independent of the handoff count: {per_handoff:?}",
            );
        }
    }

    /// Counts its wakes.
    #[derive(Default)]
    struct Counting(std::sync::atomic::AtomicUsize);

    impl Wake for Counting {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counting() -> (Arc<Counting>, Waker) {
        let counter = Arc::new(Counting::default());
        (Arc::clone(&counter), Waker::from(counter))
    }

    fn wakes(counter: &Counting) -> usize {
        counter.0.load(Ordering::SeqCst)
    }

    fn registered(token: &CancellationToken) -> usize {
        token.state.wakers.lock().len()
    }

    /// A cancellation that lands before the first poll answers it without
    /// a registration; one that lands while it is pending wakes it once,
    /// and the next poll answers.
    #[test]
    fn a_subscription_answers_or_is_woken_by_the_cancellation() {
        let early = CancellationToken::new();
        early.cancel();
        let (count, waker) = counting();
        assert!(early.subscribe().poll(&waker).is_ready());
        assert_eq!((registered(&early), wakes(&count)), (0, 0));

        let token = CancellationToken::new();
        let mut subscription = token.subscribe();
        assert!(subscription.poll(&waker).is_pending());
        assert!(subscription.poll(&waker).is_pending());
        assert_eq!(registered(&token), 1, "a re-poll keeps one registration");
        token.cancel();
        token.cancel();
        assert_eq!(wakes(&count), 1, "the cancellation wakes the poll once");
        assert!(subscription.poll(&waker).is_ready());
        assert!(subscription.poll(&waker).is_ready());
    }

    /// Re-polling with another waker replaces the registration; dropping a
    /// subscription withdraws only its own, so a sibling registered with the
    /// same waker is still woken, and nothing is cancelled.
    #[test]
    fn a_dropped_subscription_withdraws_only_itself() {
        let token = CancellationToken::new();
        let (first_count, first) = counting();
        let (second_count, second) = counting();
        let mut replaced = token.subscribe();
        assert!(replaced.poll(&first).is_pending());
        assert!(replaced.poll(&second).is_pending());
        let mut sibling = token.subscribe();
        assert!(sibling.poll(&second).is_pending());
        let mut dropped = token.subscribe();
        assert!(dropped.poll(&second).is_pending());
        drop(dropped);
        assert_eq!(registered(&token), 2);
        assert!(!token.is_cancelled(), "a dropped wait cancels nothing");
        token.cancel();
        assert_eq!(wakes(&first_count), 0, "the replaced waker is not woken");
        assert_eq!(wakes(&second_count), 2, "each live subscription is woken");
        assert!(sibling.poll(&second).is_ready());
        drop(replaced);
        assert_eq!(registered(&token), 0);
    }

    /// An aggregate wakes its subscribers when its last live owner's request
    /// cancels, not before, and when its last live owner detaches.
    #[test]
    fn an_aggregate_wakes_when_its_last_live_owner_goes() {
        for detach_last in [false, true] {
            let aggregate = CancellationToken::aggregate();
            let first = CancellationToken::new();
            let second = CancellationToken::new();
            let first_owner = aggregate.register_owner(Some(first.clone())).unwrap();
            let mut second_owner = Some(aggregate.register_owner(Some(second.clone())).unwrap());
            let (count, waker) = counting();
            let mut subscription = aggregate.subscribe();
            assert!(subscription.poll(&waker).is_pending());
            first.cancel();
            assert_eq!(wakes(&count), 0, "a live sibling owner keeps the job");
            assert!(subscription.poll(&waker).is_pending());
            if detach_last {
                drop(second_owner.take());
            } else {
                second.cancel();
            }
            assert_eq!(wakes(&count), 1, "detach_last={detach_last}");
            assert!(subscription.poll(&waker).is_ready());
            drop(first_owner);
            drop(second_owner);
            assert_eq!(registered(&first) + registered(&second), 0);
        }
    }

    /// An owner whose request was already cancelled when it registered still
    /// wakes the aggregate's subscribers once no live owner remains.
    #[test]
    fn an_owner_registered_cancelled_delivers_its_aggregate() {
        let aggregate = CancellationToken::aggregate();
        let builder = aggregate.register_owner(None).unwrap();
        let (count, waker) = counting();
        let mut subscription = aggregate.subscribe();
        assert!(subscription.poll(&waker).is_pending());
        let request = CancellationToken::new();
        request.cancel();
        let late = aggregate.register_owner(Some(request)).unwrap();
        assert_eq!(wakes(&count), 0, "the builder keeps the job live");
        drop(builder);
        assert_eq!(wakes(&count), 1);
        drop(late);
        assert!(subscription.poll(&waker).is_ready());
    }

    /// A cancellation racing the poll's registration is never lost: every
    /// poll that returned `Pending` is woken.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_cancellation_racing_registration_always_wakes() {
        for _ in 0..200 {
            for aggregate in [false, true] {
                let (token, request, owner) = if aggregate {
                    let token = CancellationToken::aggregate();
                    let request = CancellationToken::new();
                    let owner = token.register_owner(Some(request.clone())).unwrap();
                    (token, request, Some(owner))
                } else {
                    let token = CancellationToken::new();
                    (token.clone(), token, None)
                };
                let (count, waker) = counting();
                let mut subscription = token.subscribe();
                let canceller = std::thread::spawn(move || request.cancel());
                let pending = subscription.poll(&waker).is_pending();
                canceller.join().unwrap();
                if pending {
                    assert_eq!(wakes(&count), 1, "aggregate={aggregate}");
                }
                assert!(subscription.poll(&waker).is_ready());
                drop(owner);
            }
        }
    }

    #[test]
    fn detached_final_owner_cancels_before_a_late_registration() {
        let aggregate = CancellationToken::aggregate();
        let owner = aggregate
            .register_owner(None)
            .expect("uncancellable owner registers");
        owner.detach();

        assert!(aggregate.register_owner(None).is_none());
        assert!(aggregate.is_cancelled());
    }
}
