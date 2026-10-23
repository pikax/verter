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

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Poll, Wake, Waker};

use crate::wake::TakenWakers;
use crate::wake::{WakeRegistration, WakerSet};
use parking_lot::Mutex;

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
    wakers: Mutex<WakerSet>,
}

#[derive(Debug)]
struct AggregateOwners {
    ever_registered: AtomicBool,
    entries: Mutex<OwnerEntries>,
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
    aggregate: Weak<CancellationState>,
    subscription: Mutex<Option<CancellationSubscription>>,
}

/// One live requester attached to an aggregate job token.
///
/// Dropping the registration detaches only this requester. The aggregate
/// token becomes cancelled once it has had at least one owner and no attached,
/// uncancelled owner remains.
#[derive(Debug)]
pub struct CancellationOwner {
    state: Arc<CancellationOwnerState>,
}

impl CancellationToken {
    /// Creates a fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                owners: None,
                wakers: Mutex::new(WakerSet::default()),
            }),
        }
    }

    /// Create a job-liveness token whose cancellation is the aggregate of
    /// its registered request owners. An ownerless internal job remains live;
    /// after the first registration, loss/cancellation of every owner trips
    /// the one-shot token.
    pub fn aggregate() -> Self {
        Self {
            state: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                owners: Some(AggregateOwners {
                    ever_registered: AtomicBool::new(false),
                    entries: Mutex::new(OwnerEntries::default()),
                }),
                wakers: Mutex::new(WakerSet::default()),
            }),
        }
    }

    /// Attach one request token to this aggregate job token. `None` denotes
    /// an uncancellable owner that stays live until the registration drops.
    pub fn register_owner(&self, request: Option<CancellationToken>) -> Option<CancellationOwner> {
        let (owner, wakes) = self.register_owner_deferred(request);
        wakes.wake();
        owner
    }

    /// Register beneath an outer owner lock without invoking any stop
    /// callback. Deliver the returned notifications after releasing that lock.
    pub fn register_owner_deferred(
        &self,
        request: Option<CancellationToken>,
    ) -> (Option<CancellationOwner>, TakenWakers) {
        let owners = self
            .state
            .owners
            .as_ref()
            .expect("request owners can be registered only on aggregate tokens");
        if self.state.cancelled.load(Ordering::Acquire) {
            return (None, WakerSet::default().take());
        }
        let state = Arc::new(CancellationOwnerState {
            active: AtomicBool::new(true),
            request,
            aggregate: Arc::downgrade(&self.state),
            subscription: Mutex::new(None),
        });
        let mut entries = owners.entries.lock();
        // Linearize registration against `is_cancelled()`: once the aggregate
        // has ever had owners, a gap with no live owner is terminal. Detect
        // that gap here as well as in `is_cancelled()` so a late joiner cannot
        // revive a job merely because no worker happened to poll between the
        // final request cancellation and this registration attempt.
        if self.state.cancelled.load(Ordering::Acquire) {
            return (None, WakerSet::default().take());
        }
        if owners.ever_registered.load(Ordering::Acquire) && !entries.any_live() {
            drop(entries);
            return (None, self.cancel_deferred());
        }
        entries.push(Arc::downgrade(&state));
        owners.ever_registered.store(true, Ordering::Release);
        drop(entries);
        let mut wakes = WakerSet::default().take();
        if let Some(request) = &state.request {
            let mut subscription = request.subscribe();
            let waker = Waker::from(Arc::new(AggregateWake(Arc::downgrade(&self.state))));
            let (stopped, notifications) = subscription.poll_deferred(&waker);
            wakes.append(notifications);
            if stopped.is_ready() && self.is_cancelled() {
                wakes.append(self.cancel_deferred());
            }
            *state.subscription.lock() = Some(subscription);
        }
        (Some(CancellationOwner { state }), wakes)
    }

    /// Trips the flag. Idempotent — calling more than once leaves the
    /// token cancelled and never panics or toggles back.
    pub fn cancel(&self) {
        self.cancel_deferred().wake();
    }

    /// Publish stop under an enclosing owner lock, retaining wake delivery
    /// until that owner releases its lock. State publication is immediate.
    pub fn cancel_deferred(&self) -> TakenWakers {
        let mut wakers = self.state.wakers.lock();
        self.state.cancelled.store(true, Ordering::Release);
        wakers.take()
    }

    /// Observe cancellation without parking. Each subscriber owns exactly
    /// one registration; dropping it leaves sibling subscribers untouched.
    pub fn subscribe(&self) -> CancellationSubscription {
        CancellationSubscription {
            token: self.clone(),
            registration: WakeRegistration::default(),
        }
    }

    fn refresh_if_observed(&self) {
        let observed = !self.state.wakers.lock().is_empty();
        if observed && self.is_cancelled() {
            self.cancel();
        }
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
        // This probe may run under an enclosing owner lock. Notification
        // belongs to explicit cancellation/owner withdrawal, after unlock.
        self.state.cancelled.store(true, Ordering::Release);
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
    /// Detach this requester from the aggregate job. Idempotent.
    pub(crate) fn detach(&self) {
        self.detach_deferred().wake();
    }

    /// Withdraw under an enclosing owner lock. Deliver the returned wakes
    /// after unlocking, before dropping this registration.
    pub fn detach_deferred(&self) -> TakenWakers {
        if !self.state.active.swap(false, Ordering::AcqRel) {
            return WakerSet::default().take();
        }
        self.state.subscription.lock().take();
        if let Some(state) = self.state.aggregate.upgrade() {
            let aggregate = CancellationToken { state };
            let observed = !aggregate.state.wakers.lock().is_empty();
            if observed && aggregate.is_cancelled() {
                return aggregate.cancel_deferred();
            }
        }
        WakerSet::default().take()
    }
}

impl Drop for CancellationOwner {
    fn drop(&mut self) {
        self.detach();
    }
}

/// A poll-only cancellation subscription. Registration and cancellation
/// linearize under the same lock; wake callbacks run after that lock releases.
#[derive(Debug)]
pub struct CancellationSubscription {
    token: CancellationToken,
    registration: WakeRegistration,
}

impl CancellationSubscription {
    /// Ready when stopped; otherwise register this poll's waker.
    pub fn poll(&mut self, waker: &Waker) -> Poll<()> {
        let (answer, wakes) = self.poll_deferred(waker);
        wakes.wake();
        answer
    }

    fn poll_deferred(&mut self, waker: &Waker) -> (Poll<()>, TakenWakers) {
        let mut wakers = self.token.state.wakers.lock();
        let cancelled = self.token.state.cancelled.load(Ordering::Acquire)
            || self.token.state.owners.as_ref().is_some_and(|owners| {
                owners.ever_registered.load(Ordering::Acquire) && !owners.entries.lock().any_live()
            });
        if cancelled {
            self.token.state.cancelled.store(true, Ordering::Release);
            wakers.withdraw(&mut self.registration);
            let wakes = wakers.take();
            (Poll::Ready(()), wakes)
        } else {
            wakers.register(&mut self.registration, waker);
            (Poll::Pending, WakerSet::default().take())
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

struct AggregateWake(Weak<CancellationState>);

impl Wake for AggregateWake {
    fn wake(self: Arc<Self>) {
        if let Some(state) = self.0.upgrade() {
            CancellationToken { state }.refresh_if_observed();
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
    fn cancellation_wakes_one_subscription_and_withdraws_its_sibling() {
        use std::sync::atomic::AtomicUsize;
        use std::task::{Poll, Wake, Waker};

        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&count));
        let token = CancellationToken::new();
        let mut kept = token.subscribe();
        let mut dropped = token.subscribe();
        assert_eq!(kept.poll(&waker), Poll::Pending);
        assert_eq!(kept.poll(&waker), Poll::Pending);
        assert_eq!(dropped.poll(&waker), Poll::Pending);
        drop(dropped);
        token.cancel();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(kept.poll(&waker), Poll::Ready(()));
        assert_eq!(token.subscribe().poll(&waker), Poll::Ready(()));
        token.cancel();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn aggregate_stop_wakes_on_final_request_cancel_and_final_owner_drop() {
        use std::sync::atomic::AtomicUsize;
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        for drop_owner in [false, true] {
            let aggregate = CancellationToken::aggregate();
            let first_request = CancellationToken::new();
            let second_request = CancellationToken::new();
            let first = aggregate
                .register_owner(Some(first_request.clone()))
                .unwrap();
            let second = aggregate
                .register_owner(Some(second_request.clone()))
                .unwrap();
            let count = Arc::new(Count(AtomicUsize::new(0)));
            let waker = Waker::from(Arc::clone(&count));
            let mut subscription = aggregate.subscribe();
            assert!(subscription.poll(&waker).is_pending());
            first_request.cancel();
            assert_eq!(count.0.load(Ordering::SeqCst), 0);
            if drop_owner {
                drop(second);
            } else {
                second_request.cancel();
            }
            assert_eq!(count.0.load(Ordering::SeqCst), 1);
            assert!(subscription.poll(&waker).is_ready());
            drop(first);
        }
    }

    #[test]
    fn owner_registration_preserves_deferred_stop_delivery() {
        use std::sync::atomic::AtomicUsize;
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let aggregate = CancellationToken::aggregate();
        let request = CancellationToken::new();
        let _owner = aggregate.register_owner(Some(request.clone())).unwrap();
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&count));
        let mut stopped = aggregate.subscribe();
        assert!(stopped.poll(&waker).is_pending());
        // An enclosing owner has published stop but still holds its lock.
        let request_wakes = request.cancel_deferred();
        let (late, wakes) = aggregate.register_owner_deferred(None);
        assert!(late.is_none());
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        wakes.wake();
        request_wakes.wake();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert!(stopped.poll(&waker).is_ready());
    }

    #[test]
    fn repeated_owner_withdrawal_cannot_deliver_a_siblings_stop() {
        let aggregate = CancellationToken::aggregate();
        let first = aggregate.register_owner(None).unwrap();
        let request = CancellationToken::new();
        let _second = aggregate.register_owner(Some(request.clone())).unwrap();
        let mut stop = aggregate.subscribe();
        assert!(stop.poll(Waker::noop()).is_pending());
        assert!(first.detach_deferred().is_empty());
        let notification = request.cancel_deferred();
        // A later Drop of the already detached owner must not take the
        // sibling's notification from its enclosing delivery hold.
        assert!(first.detach_deferred().is_empty());
        notification.wake();
        assert!(stop.poll(Waker::noop()).is_ready());
    }

    #[test]
    fn cancellation_registration_races_never_miss_the_stop() {
        use std::sync::atomic::AtomicUsize;
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        for _ in 0..64 {
            let token = CancellationToken::new();
            let cancelled = token.clone();
            let count = Arc::new(Count(AtomicUsize::new(0)));
            let waker = Waker::from(Arc::clone(&count));
            let mut subscription = token.subscribe();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let ready = Arc::clone(&barrier);
            let producer = std::thread::spawn(move || {
                ready.wait();
                cancelled.cancel();
            });
            barrier.wait();
            let pending = subscription.poll(&waker).is_pending();
            producer.join().unwrap();
            assert!(!pending || count.0.load(Ordering::SeqCst) == 1);
            assert!(subscription.poll(&waker).is_ready());
        }
    }

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
