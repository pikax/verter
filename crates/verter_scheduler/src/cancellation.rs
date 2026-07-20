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
    entries: Mutex<Vec<Weak<CancellationOwnerState>>>,
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
pub(crate) struct CancellationOwner {
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
    pub(crate) fn aggregate() -> Self {
        Self {
            state: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                owners: Some(AggregateOwners {
                    ever_registered: AtomicBool::new(false),
                    entries: Mutex::new(Vec::new()),
                }),
            }),
        }
    }

    /// Attach one request token to this aggregate job token. `None` denotes
    /// an uncancellable owner that stays live until the registration drops.
    pub(crate) fn register_owner(
        &self,
        request: Option<CancellationToken>,
    ) -> Option<CancellationOwner> {
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
        if owners.ever_registered.load(Ordering::Acquire) {
            let mut any_live = false;
            entries.retain(|weak| {
                let Some(owner) = weak.upgrade() else {
                    return false;
                };
                if owner.active.load(Ordering::Acquire)
                    && owner
                        .request
                        .as_ref()
                        .is_none_or(|request| !request.is_cancelled())
                {
                    any_live = true;
                }
                true
            });
            if !any_live {
                self.state.cancelled.store(true, Ordering::Release);
                return None;
            }
        }
        entries.push(Arc::downgrade(&state));
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

        let mut entries = owners.entries.lock();
        let mut any_live = false;
        entries.retain(|weak| {
            let Some(owner) = weak.upgrade() else {
                return false;
            };
            if owner.active.load(Ordering::Acquire)
                && owner
                    .request
                    .as_ref()
                    .is_none_or(|request| !request.is_cancelled())
            {
                any_live = true;
            }
            true
        });
        if any_live {
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
