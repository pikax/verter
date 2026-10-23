//! Cancellation and teardown bookkeeping for in-flight scheduler work.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;
use verter_execution::wake::WakerSet;

#[derive(Clone)]
pub(super) enum ScopedCacheTerminal {
    Value(Arc<dyn Any + Send + Sync>),
    Cancelled,
    Shutdown,
    Panicked,
}

pub(super) struct ScopedFlightOwner {
    pub(super) request: Option<CancellationToken>,
    pub(super) aggregate_registration: Option<CancellationOwner>,
}

#[derive(Default)]
pub(super) struct ScopedCacheFlightState {
    pub(super) owners: HashMap<u64, ScopedFlightOwner>,
    pub(super) aggregate: Option<CancellationToken>,
    pub(super) dispatched: bool,
    /// The transferred producer's execution task and the handoff that claimed
    /// it. Followers retain their own tasks and register edges to this one.
    pub(super) producer: Option<(ExecutionTask, u64)>,
    pub(super) terminal: Option<ScopedCacheTerminal>,
    pub(super) wakers: WakerSet,
}

impl ScopedCacheFlight {
    pub(super) fn new(delivery: Arc<DagMutex>) -> Self {
        Self {
            state: Mutex::new(ScopedCacheFlightState::default()),
            delivery,
        }
    }

    /// Attach an owner before its inbox submission. Returns `false` only when
    /// this flight is already terminal (including an aggregate token that has
    /// latched cancellation); the caller must retry against a fresh flight.
    pub(super) fn try_add_owner(&self, owner_id: u64, request: Option<CancellationToken>) -> bool {
        let _wakes = self.delivery.defer_wakes();
        let mut state = self.state.lock();
        if state.terminal.is_some() {
            return false;
        }
        let aggregate_registration = match state.aggregate.as_ref() {
            Some(aggregate) => match aggregate.register_owner(request.clone()) {
                Some(registration) => Some(registration),
                None => {
                    state.terminal = Some(ScopedCacheTerminal::Cancelled);
                    state.producer = None;
                    state.owners.clear();
                    self.delivery.deliver(vec![state.wakers.take()]);
                    return false;
                }
            },
            None => None,
        };
        let prior = state.owners.insert(
            owner_id,
            ScopedFlightOwner {
                request,
                aggregate_registration,
            },
        );
        verter_debug_assert!(prior.is_none(), "scoped owner ids are process-unique");
        true
    }

    /// Bind the DAG node's aggregate token to every owner that arrived before
    /// admission. Later owners register directly in [`Self::try_add_owner`].
    pub(super) fn attach_aggregate(&self, aggregate: CancellationToken) -> bool {
        let _wakes = self.delivery.defer_wakes();
        let mut state = self.state.lock();
        if state.terminal.is_some() {
            return false;
        }
        if state.aggregate.is_some() {
            return true;
        }
        for owner in state.owners.values_mut() {
            let Some(registration) = aggregate.register_owner(owner.request.clone()) else {
                state.terminal = Some(ScopedCacheTerminal::Cancelled);
                state.producer = None;
                state.owners.clear();
                self.delivery.deliver(vec![state.wakers.take()]);
                return false;
            };
            owner.aggregate_registration = Some(registration);
        }
        state.aggregate = Some(aggregate);
        true
    }

    /// Remove one request owner. Returns `true` when this transition made the
    /// flight terminal-cancelled and its DAG node must be cancelled.
    pub(super) fn detach_owner(&self, owner_id: u64) -> bool {
        let _wakes = self.delivery.defer_wakes();
        let mut state = self.state.lock();
        state.owners.remove(&owner_id);
        if state.terminal.is_some() {
            return false;
        }
        let no_live_owner = state.owners.is_empty()
            || state
                .aggregate
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled);
        if !no_live_owner {
            return false;
        }
        state.terminal = Some(ScopedCacheTerminal::Cancelled);
        state.producer = None;
        state.owners.clear();
        self.delivery.deliver(vec![state.wakers.take()]);
        true
    }

    /// Signal that normal DAG dispatch selected this flight. No closure runs on
    /// the driver; one waiting caller claims it and executes on the CPU pool.
    pub(super) fn mark_dispatched(&self, aggregate: CancellationToken) -> bool {
        let _wakes = self.delivery.defer_wakes();
        let mut state = self.state.lock();
        if state.terminal.is_some() || aggregate.is_cancelled() {
            if state.terminal.is_none() {
                state.terminal = Some(ScopedCacheTerminal::Cancelled);
                state.producer = None;
                state.owners.clear();
                self.delivery.deliver(vec![state.wakers.take()]);
            }
            return false;
        }
        state.aggregate.get_or_insert(aggregate);
        state.dispatched = true;
        self.delivery.deliver(vec![state.wakers.take()]);
        true
    }

    pub(super) fn terminal(&self) -> Option<ScopedCacheTerminal> {
        self.state.lock().terminal.clone()
    }

    pub(super) fn set_terminal(&self, terminal: ScopedCacheTerminal) -> bool {
        let _wakes = self.delivery.defer_wakes();
        let mut state = self.state.lock();
        if state.terminal.is_some() {
            return false;
        }
        state.terminal = Some(terminal);
        state.producer = None;
        state.owners.clear();
        self.delivery.deliver(vec![state.wakers.take()]);
        true
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn owner_count(&self) -> usize {
        self.state.lock().owners.len()
    }
}

impl Scheduler {
    pub(super) fn detach_scoped_cache_owner(
        &self,
        identity: &WorkNodeIdentity,
        flight: &Arc<ScopedCacheFlight>,
        owner_id: u64,
    ) {
        let _wakes = self.dag.defer_wakes();
        let _gate = self.scoped_cache_gate.lock();
        let lost_all_owners = flight.detach_owner(owner_id);
        if lost_all_owners || flight.terminal().is_some() {
            // Incarnation-scoped teardown: the DAG node is keyed by identity
            // alone, so cancelling it from a SUPERSEDED flight would tear down
            // the node a later incarnation already owns. See
            // `scoped_flight_is_current`.
            if self.scoped_flight_is_current(identity, flight) {
                let _ = self.dag.lock().cancel(identity);
            }
            self.remove_scoped_flight_locked(identity, flight);
        }
    }

    /// Terminalize a flight and its DAG node under one incarnation gate.
    /// `success` is permitted only for a live aggregate job; cancellation
    /// discovered at this final publication rail always wins.
    pub(super) fn terminalize_scoped_cache_flight(
        &self,
        identity: &WorkNodeIdentity,
        flight: &Arc<ScopedCacheFlight>,
        terminal: ScopedCacheTerminal,
        success: bool,
    ) {
        let _wakes = self.dag.defer_wakes();
        let _gate = self.scoped_cache_gate.lock();
        if flight.terminal().is_some() {
            // Already terminal: this flight owns no live DAG node of its own.
            // It may also already have been SUPERSEDED, in which case the
            // identity's node belongs to a newer incarnation and must survive.
            if self.scoped_flight_is_current(identity, flight) {
                let _ = self.dag.lock().cancel(identity);
            }
            self.remove_scoped_flight_locked(identity, flight);
            return;
        }
        let aggregate_cancelled = self
            .dag
            .lock()
            .cancellation_for(identity)
            .is_some_and(|token| token.is_cancelled());
        if success && !aggregate_cancelled {
            let _ = self.dag.lock().complete(identity);
            let _ = flight.set_terminal(terminal);
        } else {
            let _ = self.dag.lock().cancel(identity);
            let effective = if aggregate_cancelled {
                ScopedCacheTerminal::Cancelled
            } else {
                terminal
            };
            let _ = flight.set_terminal(effective);
        }
        self.remove_scoped_flight_locked(identity, flight);
    }

    /// Signal `Shutdown` to every completion sender carried by a
    /// drained-but-unprocessed submission so the corresponding handles
    /// resolve instead of hanging. `Wake` / `StageComplete` carry no
    /// waiter sender and are dropped silently.
    pub(super) fn shutdown_drained_submission(&self, submission: Submission) {
        match submission {
            Submission::NewRequest { sender, .. } => {
                self.dag.send_completion(&sender, CompletionState::Shutdown);
            }
            Submission::NewRequestBatch { requests } => {
                for req in requests {
                    self.dag
                        .send_completion(&req.sender, CompletionState::Shutdown);
                }
            }
            Submission::ScopedCacheNode { flight, .. } => {
                let _ = flight.set_terminal(ScopedCacheTerminal::Shutdown);
            }
            Submission::Wake | Submission::StageComplete { .. } => {}
        }
    }

    pub(super) fn shutdown_all_scoped_cache_flights(&self) {
        let _wakes = self.dag.defer_wakes();
        let _gate = self.scoped_cache_gate.lock();
        for flight in self.scoped_cache_flights.iter() {
            let _ = flight.set_terminal(ScopedCacheTerminal::Shutdown);
        }
        self.scoped_cache_flights.clear();
    }

    // ── Edge Management ──
}
