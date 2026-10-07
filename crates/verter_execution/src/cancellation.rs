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
#[derive(Debug, Default)]
struct OwnerEntries {
    owners: Vec<Weak<CancellationOwnerState>>,
    /// Owner entries the liveness probe has examined. Tests only.
    #[cfg(test)]
    examined: u64,
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
            if last.upgrade().is_some_and(|owner| owner.is_live()) {
                return true;
            }
            self.owners.pop();
        }
        false
    }
}

#[derive(Debug)]
struct CancellationOwnerState {
    active: AtomicBool,
    request: Option<CancellationToken>,
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
            }),
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
            self.state.cancelled.store(true, Ordering::Release);
            return None;
        }
        entries.owners.push(Arc::downgrade(&state));
        owners.ever_registered.store(true, Ordering::Release);
        Some(CancellationOwner { state })
    }

    /// Trips the flag. Idempotent — calling more than once leaves the
    /// token cancelled and never panics or toggles back.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
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
        self.state.cancelled.store(true, Ordering::Release);
        true
    }

    /// Whether this is an aggregate job token that has accepted at least one
    /// request owner. Ownerless DAG tokens use the installed request token for
    /// semantic cancellation; scoped shared jobs use this aggregate instead.
    #[must_use]
    pub fn has_registered_owners(&self) -> bool {
        self.state
            .owners
            .as_ref()
            .is_some_and(|owners| owners.ever_registered.load(Ordering::Acquire))
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
        self.state.active.store(false, Ordering::Release);
    }
}

impl Drop for CancellationOwner {
    fn drop(&mut self) {
        self.detach();
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

/// Stack-safe TLS guard for one scheduler-owned job cancellation token.
pub struct JobCancellationGuard {
    previous: Option<CancellationToken>,
}

impl JobCancellationGuard {
    /// Install `token` for the current job and restore the prior token on drop.
    #[must_use]
    pub fn install(token: CancellationToken) -> Self {
        let previous = CURRENT_JOB_CANCELLATION.with(|slot| slot.replace(Some(token)));
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
