//! FlightCell — same-key production owner.
//!
//! One [`FlightCell`] owns production for each in-flight semantic key
//! ([`PreparedKeyHandle`] — full-key equality behind one `Arc`). Concurrent
//! waiters join that cell; only the winner computes. Failed, incomplete,
//! cancelled, or stale production never publishes into
//! [`crate::project_type_store::ProjectTypeStore`]. RAII guards keep the
//! producing task's open producers and the in-flight table consistent across
//! panics and early returns.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::semantic_query::{DepSignature, QueryError, QueryResult, SemanticQueryValue};

use super::empty_signature;
use super::prepared::PreparedKeyHandle;

/// Same-key production cell for one in-flight semantic query.
///
/// The inner mutex guards `state` exclusively. A joiner registers its waker
/// in `state` while the flight is unresolved, and the winner wakes every
/// registration once it publishes or aborts, so joiners do not busy-retry.
/// Ownership is released when the cell is retired from the store's flight
/// table after completion, abort, or panic.
pub struct FlightCell {
    pub(super) site: crate::semantic_query::checker_policy::TupleSplicePosition,
    pub(super) state: Mutex<InflightState>,
    /// The cell's block and its slot in the flight table, charged for as
    /// long as the cell lives — through every subscriber that still holds
    /// it after the flight retires. A flight is in-flight computation the
    /// dispatcher already admitted: its storage is charged ACTIVE, never
    /// refused, and the charge falls when the last holder drops the cell.
    _storage: Option<verter_session_query::retention::RetentionCharge>,
}

#[derive(Default)]
pub(super) struct InflightState {
    pub(super) lease: super::delivery_lease::SemanticDeliveryLease,
    /// `None` while building; `Some` after the winner publishes.
    pub(super) completed: Option<QueryResult<SemanticQueryValue>>,
    /// Dispatch-fence dep signature the winner's cold build produced
    /// — the `QueryBuildOutput.dep_signature` value. Used by joiners
    /// purely as the transitive-dependency payload they return on
    /// `CacheRead.dep_signature`; cache validity is decided exclusively
    /// by the published carrier (`graph_carrier`), never by this rail.
    pub(super) dep_signature: Option<DepSignature>,
    /// The self-version-rooted carrier the winner's cold build produced.
    /// Set by the winner alongside `completed`; joiners that observe
    /// `aborted == false` bubble its path-precise fact rail into their
    /// active TLS tracer before returning the warm result — ensuring
    /// nested outer tracers capture the semantic node's dependencies.
    /// `None` when the winner's build was non-cacheable
    /// (`cache_suppress`) — joiners then have no carrier to bubble.
    /// `Box`ed to match `QueryBuildOutput::graph_carrier` and keep the
    /// in-flight state compact.
    pub(super) graph_carrier:
        Option<Box<verter_session_query::facts::fact_cache::ReadSetSignature>>,
    /// The winner build's **self-root canonicals** — the keyed (or
    /// file-derived input) canonical(s) the winner's value depends on
    /// for its own identity. Set by the winner alongside `completed`
    /// and `graph_carrier`.
    ///
    /// A follower joining this in-flight build is NOT guaranteed to be
    /// running under the same view as the winner: two requests can
    /// carry the same [`SemanticQueryKey`] while executing under
    /// different overlays (a base context and a session/overlay
    /// context, or two different overlays). Their results are NOT
    /// interchangeable — each must validate against its own content
    /// identity. Before a follower bubbles + returns the winner's
    /// carrier it validates `graph_carrier` against the FOLLOWER's
    /// `ctx` via [`verter_session_query::facts::fact_cache::ReadSetSignature::validate_with_self_roots`],
    /// passing this set as the strict self-root canonicals — the same
    /// validation a warm hit (`MemoEntry::validate`) performs. If the
    /// winner's carrier validates under the follower's view the
    /// coalesce is legitimate; if it does not, the follower forks and
    /// cold-recomputes for its own view. Empty for a winner build with
    /// no observable cold-compute pass (synthetic / test fixtures) —
    /// validation then degrades to the plain carrier rails.
    pub(super) self_root_canonicals: std::sync::Arc<[std::sync::Arc<str>]>,
    /// Walker diagnostics observed during the winner's cold build.
    /// Joiners read this alongside `completed` so warm-replay parity is
    /// preserved across cooperative-admission joins. Empty for non-
    /// walker queries.
    pub(super) walker_diagnostics:
        Option<crate::project_semantic_dispatch::walk::WalkerDiagnostics>,
    /// The receipt of the winner's build, when it sealed one: a
    /// subscriber's demand replays it, exactly as a warm read does, so
    /// waiting on another task's computation costs what computing it would.
    pub(super) cost_receipt:
        Option<Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>>,
    /// The winner build's `cache_suppress` flag. Set by the winner
    /// alongside `completed`; a joiner that observes `aborted == false`
    /// returns this verbatim in its `CacheRead.cache_suppress`. A
    /// `cache_suppress` winner is non-cacheable (a non-cacheable read,
    /// pathological input, or an unrootable / `None` signature); the
    /// joiner MUST inherit the same non-cacheability so a joiner inside
    /// an outer cold query cannot publish an outer entry that — through
    /// a composition helper threading the joiner's read — would
    /// otherwise be admitted despite a non-cacheable transitive child.
    /// `false` for the abort/retry path (the sentinel result there is
    /// not a real winner build).
    pub(super) cache_suppress: bool,
    /// The producer's adopted availability, broadcast unchanged to followers.
    pub(super) fact_status: crate::semantic_query::FactStatus,
    /// The generation-qualified task that claimed this flight. Subscribers
    /// register a temporary wait-for edge to this task before parking, so a
    /// cycle of waiting tasks escapes through ReturnOnly.
    pub(super) owner: Option<super::tasks::TaskId>,
    /// `true` once some thread owns the build. Subsequent threads wait on
    /// `ready` rather than trying to own it themselves.
    pub(super) claimed: bool,
    /// Set by [`super::SemanticGraphStore::invalidate_canonical`] when this
    /// in-flight entry's `(family, slot)` matched the sweep. Joiners that
    /// wake from the condvar observe this flag and re-enter dispatch from
    /// step 1 rather than returning the (now stale) winner result. The
    /// cold winner skips warm publish when the flag is set so the stale
    /// result never re-populates the cache.
    pub(super) aborted: bool,
    /// The wakers of subscribers that polled the flight while it was
    /// unresolved, each registered under this lock by its own
    /// subscription; the flight wakes every one once it resolves
    /// ([`FlightCell::wake_subscribers`]). Two subscriptions polled with the
    /// same executor waker hold two registrations, so withdrawing one never
    /// withdraws the other.
    pub(super) wakers: verter_execution::wake::WakerSet,
}

impl FlightCell {
    /// Wake every subscriber after the flight resolved (completed or
    /// aborted) under its lock, through their registered wakers. A
    /// subscriber, polling or blocking, registers its waker only after
    /// finding the flight unresolved under the same lock, so every waker
    /// registered before the resolution is taken here and none is
    /// registered after it.
    pub(super) fn wake_subscribers(&self) {
        let wakers = self.state.lock().wakers.take();
        wakers.wake();
    }

    /// An unclaimed cell whose storage is charged as in-flight work
    /// against `account`.
    pub(super) fn charged(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            site: Default::default(),
            state: Mutex::new(InflightState::default()),
            _storage: Some(account.reserve_in_flight(flight_storage_bytes())),
        }
    }

    /// A fixture cell owning no storage charge, for a suite that drives a
    /// flight by hand rather than through a store.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            site: Default::default(),
            state: Mutex::new(InflightState::default()),
            _storage: None,
        }
    }
}

/// Bytes one flight keeps while it lives: the cell's `Arc` block and its
/// slot in a flight table (key handle, cell handle and control byte). What
/// the winner settles into the cell — its value, rails and lease — is
/// shared with the delivery that produced it.
pub(super) fn flight_storage_bytes() -> usize {
    verter_session_query::retention::allocation::arc_block_bytes(std::mem::size_of::<FlightCell>())
        + std::mem::size_of::<(PreparedKeyHandle, Arc<FlightCell>)>()
        + 1
}

/// RAII guard that fails the in-flight entry if the cold build panics.
///
/// Without this guard, a panic inside the winner's build closure would
/// leave `state.claimed == true` with `state.completed == None`. Any
/// subsequent caller for the same key would block on the condvar forever
/// because no publish ever wakes it. The guard detects the abnormal drop
/// via a `completed` flag, marks the entry aborted (a partial,
/// non-cacheable fault sentinel), wakes joiners so they retry cold, and
/// removes the entry from the in-flight table so fresh callers start a new
/// build.
pub(super) struct InflightPanicGuard<'a> {
    inflight: Arc<FlightCell>,
    registration: InflightRegistration<'a>,
    key: PreparedKeyHandle,
    finished: bool,
}

enum InflightRegistration<'a> {
    Joining(&'a Mutex<FxHashMap<PreparedKeyHandle, Arc<FlightCell>>>),
    Independent(&'a Mutex<FxHashMap<PreparedKeyHandle, Vec<Arc<FlightCell>>>>),
}

impl<'a> InflightPanicGuard<'a> {
    pub(super) fn new(
        inflight: Arc<FlightCell>,
        inflight_table: &'a Mutex<FxHashMap<PreparedKeyHandle, Arc<FlightCell>>>,
        key: PreparedKeyHandle,
    ) -> Self {
        Self {
            inflight,
            registration: InflightRegistration::Joining(inflight_table),
            key,
            finished: false,
        }
    }

    pub(super) fn new_independent(
        inflight: Arc<FlightCell>,
        inflight_table: &'a Mutex<FxHashMap<PreparedKeyHandle, Vec<Arc<FlightCell>>>>,
        key: PreparedKeyHandle,
    ) -> Self {
        Self {
            inflight,
            registration: InflightRegistration::Independent(inflight_table),
            key,
            finished: false,
        }
    }

    pub(super) fn mark_finished(&mut self) {
        self.finished = true;
    }
}

impl<'a> Drop for InflightPanicGuard<'a> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Panic / early-return path — mark the entry aborted, with an
        // error sentinel, so joiners wake rather than wait forever on a
        // condvar that will never be signalled. The build answered
        // nothing: a parked joiner retries the query cold, and one past
        // its retry budget reads a partial, non-cacheable fault, never a
        // finished error it could admit.
        {
            let mut state = self.inflight.state.lock();
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(
                    verter_type_expr::OwnedStr::from("cold build aborted (panic or early return)"),
                )));
                state.dep_signature = Some(empty_signature());
            }
            state.aborted = true;
            state.graph_carrier = None;
            state.cost_receipt = None;
            state.cache_suppress = true;
            state.fact_status = crate::semantic_query::FactStatus::unavailable(
                crate::semantic_query::PartialReasonSet::SEMANTIC_QUERY_FAULT,
            );
        }
        self.inflight.wake_subscribers();
        // `ptr_eq`-guarded remove: only retire THIS guard's own
        // in-flight entry. A cross-view joiner that forked may have
        // installed a fresh `FlightCell` for the same key; an
        // unconditional remove would evict that fresh entry. (On the
        // panic path the winner never published a `graph_carrier`, so
        // a joiner cannot have forked off THIS build — but the guard
        // stays `ptr_eq`-correct for defence in depth and parity with
        // the normal-return step-7 retire.)
        match &self.registration {
            InflightRegistration::Joining(table) => {
                let mut table = table.lock();
                if table
                    .get(&self.key)
                    .is_some_and(|entry| Arc::ptr_eq(entry, &self.inflight))
                {
                    table.remove(&self.key);
                }
            }
            InflightRegistration::Independent(table) => {
                let mut table = table.lock();
                if let Some(entries) = table.get_mut(&self.key) {
                    entries.retain(|entry| !Arc::ptr_eq(entry, &self.inflight));
                    if entries.is_empty() {
                        table.remove(&self.key);
                    }
                }
            }
        }
    }
}

/// Maximum number of times a joiner re-enters dispatch after its
/// in-flight entry was aborted by a canonical invalidation sweep. Bounds
/// pathological retry loops (e.g. an invalidation that keeps firing on
/// the same canonical) to a small constant; in practice 0-1 retries is
/// typical because the next call either hits a freshly-warm slot or
/// claims the fresh in-flight as winner.
pub const MAX_INFLIGHT_RETRIES: usize = 3;

/// Store-owned admission token for an SCC member computed inline on
/// another obligation's transaction.
///
/// ONE type across every deferred domain (relation, flow-return,
/// call-resolution): a member's claim/abort/completion coordination is
/// the SAME work in all three, and a per-domain copy of it could drift
/// on exactly the properties [`super::scc_publish`] fences whole-batch.
/// The member's typed family identity is NOT erased — it stays in
/// `prepared`, the full [`crate::semantic_query::SemanticQueryKey`] the
/// flight claimed, which the batch asserts against the member's own key
/// before staging its candidate.
///
/// Registering the token in the ORDINARY family flight table lets a
/// concurrent top-level request join the inline compute instead of
/// starting duplicate cold work.
#[derive(Clone)]
pub struct InlineMemberFlight {
    pub(super) prepared: PreparedKeyHandle,
    pub(super) inflight: Arc<FlightCell>,
    /// The task computing the member inline. Holding it keeps the task
    /// active while the flight is open, whether it is the installed task of
    /// the enclosing execution or one registered for this flight alone.
    _task: super::tasks::ExecutionTask,
    /// The member's decided value, owned from the moment its component
    /// closes until the batched publish stages it: the construction scope
    /// that computed it may close first.
    /// The root set rides inline here; its operand list, once it spills,
    /// is charged as in-flight work against the store's retention account
    /// for as long as it is held — ACTIVE until the batched publish or the
    /// abort ends the hold.
    held: Option<(
        super::node_roots::NodeRootSet,
        Option<verter_session_query::retention::shared::CallerHold>,
    )>,
    /// The decided value named a node that was no longer live when the
    /// component closed: the batched publish refuses the whole component.
    hold_refused: bool,
}

impl std::fmt::Debug for InlineMemberFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineMemberFlight")
            .field("key", self.prepared.key())
            .finish_non_exhaustive()
    }
}

impl InlineMemberFlight {
    /// Own the graph nodes of `value` — the member's decided value — until
    /// this flight is published or aborted. A value whose operands are not
    /// live cannot be held; the flight records it, and the batched publish
    /// then refuses the whole component ([`Self::hold_refused`]).
    pub(crate) fn hold_value(
        &mut self,
        graph: &super::SemanticGraphStore,
        value: &crate::semantic_query::QueryResult<crate::semantic_query::SemanticQueryValue>,
    ) {
        let mut nodes = Vec::new();
        value.for_each_retained_node(|node| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR {
                nodes.push(node);
            }
        });
        if nodes.is_empty() {
            return;
        }
        match graph.root_nodes(nodes) {
            Ok(roots) => {
                let spill = roots.heap_bytes();
                let charge = (spill > 0).then(|| {
                    verter_session_query::retention::shared::CallerHold::in_flight(
                        graph.retention_account(),
                        spill,
                    )
                });
                self.held = Some((roots, charge));
            }
            Err(_) => self.hold_refused = true,
        }
    }

    /// Whether the member's decided value could not be held.
    pub(super) fn hold_refused(&self) -> bool {
        self.hold_refused
    }

    /// Test seam over [`Self::hold_value`] for a hand-staged member.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_value_for_tests(
        &mut self,
        graph: &super::SemanticGraphStore,
        value: &crate::semantic_query::QueryResult<crate::semantic_query::SemanticQueryValue>,
    ) {
        self.hold_value(graph, value);
    }

    pub(super) fn state(&self) -> &Mutex<InflightState> {
        &self.inflight.state
    }

    pub(super) fn mark_aborted(&self) {
        self.state().lock().aborted = true;
    }

    pub(super) fn is_aborted(&self) -> bool {
        self.state().lock().aborted
    }

    /// Wake every joiner and retire the flight from the ORDINARY table —
    /// the table [`super::SemanticGraphStore::begin_inline_member_flight`]
    /// claimed in.
    pub(super) fn notify_and_retire(self, store: &super::SemanticGraphStore) {
        self.inflight.wake_subscribers();
        store.retire_inflight(&self.prepared, &self.inflight, false);
    }
}

impl super::SemanticGraphStore {
    /// Claim the ordinary family flight for a member that will be
    /// computed inline on the current transaction. `None` means another
    /// cold owner already owns this exact full key.
    pub(super) fn begin_inline_member_flight(
        &self,
        key: crate::semantic_query::SemanticQueryKey,
    ) -> Option<InlineMemberFlight> {
        let prepared = self.prepare_rooted_key(key);
        let inflight = Arc::new(FlightCell::charged(self.retention_account()));
        let task = super::tasks::ExecutionScope::current(&self.task_registry)
            .unwrap_or_else(|| self.task_registry.register_task());
        {
            let mut state = inflight.state.lock();
            state.claimed = true;
            state.owner = Some(task.id());
        }
        let mut table = self.inflight.lock();
        if table.contains_key(&prepared) {
            return None;
        }
        table.insert(prepared.clone(), Arc::clone(&inflight));
        Some(InlineMemberFlight {
            prepared,
            inflight,
            _task: task,
            held: None,
            hold_refused: false,
        })
    }

    /// Release an inline flight that cannot publish a decided member.
    /// Waiting top-level callers wake on the abort sentinel and retry
    /// admission. The sentinel is the payload-free superseded abort a joiner
    /// of an aborted flight already reads: a relation that stops part-way can
    /// abandon thousands of members, and a per-member message would hold an
    /// allocation for each until the relation returns.
    pub fn abort_inline_member_flight(&self, flight: &InlineMemberFlight) {
        {
            let mut state = flight.inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::ExecutionAborted(
                    crate::semantic_query::ExecutionAbort::Superseded,
                )));
                state.dep_signature = Some(empty_signature());
            }
            state.graph_carrier = None;
            state.cost_receipt = None;
            state.walker_diagnostics = None;
            state.cache_suppress = true;
            state.fact_status = crate::semantic_query::FactStatus::unavailable(
                crate::semantic_query::PartialReasonSet::SEMANTIC_QUERY_FAULT,
            );
        }
        flight.inflight.wake_subscribers();
        self.retire_inflight(&flight.prepared, &flight.inflight, false);
    }
}

/// Why a [`FlightRendezvous`] wait returned without its event.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightRendezvousMiss {
    /// The event did not arrive within the caller's hang bound. The bound
    /// only stops a broken schedule from hanging its driver; it never
    /// decides whether the work overlapped.
    HangBound,
}

/// What a [`FlightRendezvous`] has observed of its key's cold producers and
/// of the subscribers registered to the held producer's flight.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlightRendezvousObservation {
    /// Joinable producers of the key that reached the cold-build boundary
    /// while the rendezvous was armed. Only the first is held.
    pub producer_claims: usize,
    /// Subscribers that registered a wait on the held producer's flight. A
    /// subscriber of any other flight, key or store is never counted.
    pub subscribers: usize,
    /// The held producer is parked at its cold-build boundary now.
    pub parked: bool,
    /// The held producer may run (released, or the rendezvous dropped).
    pub released: bool,
}

/// The state a rendezvous shares between its driver and the store's hook
/// points. It names its flight weakly: the allocation stays addressable so
/// a later flight can never reuse its identity, but the flight's value and
/// charge retire with the flight.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
pub(super) struct FlightRendezvousShared {
    key: crate::semantic_query::SemanticQueryKey,
    state: Mutex<FlightRendezvousState>,
    changed: parking_lot::Condvar,
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
#[derive(Default)]
struct FlightRendezvousState {
    flight: Option<std::sync::Weak<FlightCell>>,
    producer_claims: usize,
    subscribers: usize,
    parked: bool,
    released: bool,
    /// The parked producer's request token while it is parked, and the
    /// tokens of the subscribers counted above — the driver's cancellation
    /// control over exactly the participants it observes.
    producer_cancellation: Option<verter_execution::cancellation::CancellationToken>,
    subscriber_cancellations: Vec<verter_execution::cancellation::CancellationToken>,
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl FlightRendezvousState {
    fn observation(&self) -> FlightRendezvousObservation {
        FlightRendezvousObservation {
            producer_claims: self.producer_claims,
            subscribers: self.subscribers,
            parked: self.parked,
            released: self.released,
        }
    }
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl FlightRendezvousShared {
    /// A joinable producer of `key` reached its cold-build boundary on
    /// `flight`. The first one parks here, holding no memo or flight lock,
    /// until the rendezvous releases it or its own request is cancelled;
    /// later ones are counted and run.
    pub(super) fn hold_producer(
        &self,
        key: &crate::semantic_query::SemanticQueryKey,
        flight: &Arc<FlightCell>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
    ) {
        if key != &self.key {
            return;
        }
        let mut state = self.state.lock();
        state.producer_claims += 1;
        if state.released || state.flight.is_some() {
            self.changed.notify_all();
            return;
        }
        state.flight = Some(Arc::downgrade(flight));
        state.parked = true;
        state.producer_cancellation = crate::request_context::current_request_cancellation_token();
        self.changed.notify_all();
        // Timed parking is the cancellation observation rail, exactly as a
        // subscriber's wait: a producer whose request is cancelled returns
        // so its build observes the cancellation, never mutating the
        // rendezvous or the flight it holds.
        while !state.released && !flags.is_cancelled() {
            self.changed
                .wait_for(&mut state, std::time::Duration::from_millis(2));
        }
        state.parked = false;
        state.producer_cancellation = None;
        self.changed.notify_all();
    }

    /// A subscriber of `key` registered its wait on `flight`. Counted only
    /// when `flight` is the held producer's flight; the returned
    /// registration retires the cancellation token when the subscriber's
    /// wait returns, so a completed subscriber is never a cancellable
    /// participant again. The registration itself stays recorded: the
    /// subscriber count is the event record, not a liveness probe.
    pub(super) fn note_subscriber(
        self: &Arc<Self>,
        key: &crate::semantic_query::SemanticQueryKey,
        flight: &Arc<FlightCell>,
    ) -> SubscriberRegistration {
        if key != &self.key {
            return SubscriberRegistration::inactive();
        }
        let mut state = self.state.lock();
        let held = state
            .flight
            .as_ref()
            .is_some_and(|held| std::ptr::eq(held.as_ptr(), Arc::as_ptr(flight)));
        if !held {
            return SubscriberRegistration::inactive();
        }
        state.subscribers += 1;
        let token = crate::request_context::current_request_cancellation_token();
        if let Some(token) = &token {
            state.subscriber_cancellations.push(token.clone());
        }
        self.changed.notify_all();
        SubscriberRegistration {
            registration: Some(ActiveSubscriberRegistration {
                shared: Arc::clone(self),
                token,
            }),
        }
    }

    /// Retire one counted subscriber's cancellation registration: its wait
    /// returned, so its token no longer names a live participant. The
    /// registration count stays: it is the event record the driver's
    /// readiness oracle reads. No wait condition reads the token list, so
    /// this changes nothing a driver waits on.
    fn retire_subscriber(&self, token: Option<&verter_execution::cancellation::CancellationToken>) {
        let mut state = self.state.lock();
        if let Some(position) = token.and_then(|token| {
            state
                .subscriber_cancellations
                .iter()
                .position(|saved| saved.same_source(token))
        }) {
            state.subscriber_cancellations.swap_remove(position);
        }
    }

    fn release(&self) {
        self.state.lock().released = true;
        self.changed.notify_all();
    }

    fn wait_until(
        &self,
        hang_bound: std::time::Duration,
        reached: impl Fn(&FlightRendezvousState) -> bool,
    ) -> Result<FlightRendezvousObservation, FlightRendezvousMiss> {
        let deadline = std::time::Instant::now() + hang_bound;
        let mut state = self.state.lock();
        while !reached(&state) {
            if self.changed.wait_until(&mut state, deadline).timed_out() && !reached(&state) {
                return Err(FlightRendezvousMiss::HangBound);
            }
        }
        Ok(state.observation())
    }
}

/// A counted subscriber's registration with an armed rendezvous. Dropping
/// it — the subscriber's wait returned, was cancelled or unwound — retires
/// its cancellation token, so
/// [`FlightRendezvous::cancel_participants`] only ever names participants
/// whose wait is still live. The registration count itself stays recorded.
/// A note that matched no held flight carries no registration and drops as
/// a no-op.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
#[must_use = "the registration retires the subscriber when it returns; hold it until the wait ends"]
pub(super) struct SubscriberRegistration {
    registration: Option<ActiveSubscriberRegistration>,
}

/// The counted half of a [`SubscriberRegistration`]: which rendezvous
/// counts it, and its cancellation token when the subscribing request
/// carries one.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
struct ActiveSubscriberRegistration {
    shared: Arc<FlightRendezvousShared>,
    token: Option<verter_execution::cancellation::CancellationToken>,
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl SubscriberRegistration {
    fn new(registration: Option<ActiveSubscriberRegistration>) -> Self {
        Self { registration }
    }

    /// A registration that matched no held flight: dropping it retires
    /// nothing.
    pub(super) fn inactive() -> Self {
        Self::new(None)
    }

    /// Retire this registration now. Idempotent; the drop path calls it too.
    pub(super) fn retire(&mut self) {
        if let Some(ActiveSubscriberRegistration { shared, token }) = self.registration.take() {
            shared.retire_subscriber(token.as_ref());
        }
    }
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl Drop for SubscriberRegistration {
    fn drop(&mut self) {
        self.retire();
    }
}

/// A scoped rendezvous with the next cold producer of one key on one store.
///
/// Armed by [`super::SemanticGraphStore::arm_flight_rendezvous`]. The first
/// joinable producer of the key that reaches its cold-build boundary
/// through the dispatcher's synchronous path parks there, after its flight
/// is claimed and before its raw computation, holding no memo or flight
/// lock. A subscriber registering its wait on that exact flight is
/// reported; a subscriber of another key, flight or store never is, and a
/// warm read reports nothing. The driver then
/// [`releases`](Self::release) the producer.
///
/// Dropping the rendezvous — normal completion, a failed assertion's unwind
/// or an abandoned schedule — releases a parked producer, wakes every wait
/// and removes exactly this guard's registration from the store, in any
/// drop order against other armed guards; nothing it observed is retained
/// afterwards. It controls only when real work runs, never what it
/// answers.
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
#[must_use = "the rendezvous disarms when dropped"]
pub struct FlightRendezvous<'s> {
    store: &'s super::SemanticGraphStore,
    shared: Arc<FlightRendezvousShared>,
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl FlightRendezvous<'_> {
    /// Wait until the held producer is parked at its cold-build boundary.
    pub fn wait_producer_parked(
        &self,
        hang_bound: std::time::Duration,
    ) -> Result<FlightRendezvousObservation, FlightRendezvousMiss> {
        self.shared.wait_until(hang_bound, |state| state.parked)
    }

    /// Wait until a subscriber registered its wait on the held producer's
    /// flight.
    pub fn wait_subscriber_registered(
        &self,
        hang_bound: std::time::Duration,
    ) -> Result<FlightRendezvousObservation, FlightRendezvousMiss> {
        self.shared
            .wait_until(hang_bound, |state| state.subscribers > 0)
    }

    /// Let the held producer run. A producer reaching the boundary later is
    /// not held.
    pub fn release(&self) {
        self.shared.release();
    }

    /// What the rendezvous has observed so far.
    #[must_use]
    pub fn observe(&self) -> FlightRendezvousObservation {
        self.shared.state.lock().observation()
    }

    /// Whether the held producer's flight state is unlocked right now: a
    /// parked producer holds no lock on the flight it owns. `None` when no
    /// producer is held or its flight has retired.
    #[must_use]
    pub fn held_flight_is_unlocked(&self) -> Option<bool> {
        let flight = self.shared.state.lock().flight.as_ref()?.upgrade()?;
        let unlocked = flight.state.try_lock().is_some();
        Some(unlocked)
    }

    /// The held producer's flight, named weakly exactly as the rendezvous
    /// names it. Upgrading the result fails once the flight cell is
    /// destroyed — the observation that the flight itself, not just its
    /// table entry, is gone — so a driver can hold it past the guard and
    /// prove nothing retains the flight. `None` while no producer is held.
    #[must_use]
    pub fn held_flight_weak(&self) -> Option<std::sync::Weak<FlightCell>> {
        self.shared.state.lock().flight.clone()
    }

    /// Cancel the held producer's request and every registered
    /// subscriber's, through their own cancellation tokens. Each
    /// participant then returns on its own cancellation rail — the parked
    /// producer unparks and its build observes the cancellation, a
    /// subscriber's wait detaches — so a parked schedule can be abandoned
    /// without hanging any participant. Returns how many live
    /// participants were cancelled.
    pub fn cancel_participants(&self) -> usize {
        let mut tokens = Vec::new();
        {
            let mut state = self.shared.state.lock();
            tokens.extend(state.producer_cancellation.take());
            tokens.append(&mut state.subscriber_cancellations);
        }
        let cancelled = tokens.len();
        for token in tokens {
            token.cancel();
        }
        cancelled
    }
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl Drop for FlightRendezvous<'_> {
    fn drop(&mut self) {
        // Remove only this guard's registration, whatever order overlapping
        // guards end in: a still-armed scope — armed before or after this
        // one — stays armed exactly as its own guard left it, and the store
        // disarms once the last guard ends.
        self.store
            .flight_rendezvous
            .lock()
            .retain(|armed| !Arc::ptr_eq(armed, &self.shared));
        self.store
            .flight_rendezvous_armed
            .fetch_sub(1, std::sync::atomic::Ordering::Release);
        self.shared.release();
    }
}

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
impl super::SemanticGraphStore {
    /// Arm a [`FlightRendezvous`] for `key` on this store until the returned
    /// guard drops. Another store, and every other key, is unaffected.
    ///
    /// `key` must name a query the dispatcher's synchronous path produces:
    /// an [`crate::semantic_query::SemanticQueryKey::Instantiate`] read is
    /// evaluated on the continuation runtime, whose frame producers never
    /// reach the synchronous cold-build hold, so arming one is refused.
    pub fn arm_flight_rendezvous(
        &self,
        key: crate::semantic_query::SemanticQueryKey,
    ) -> FlightRendezvous<'_> {
        assert!(
            !matches!(key, crate::semantic_query::SemanticQueryKey::Instantiate(_)),
            "an Instantiate key is driven on the continuation runtime; its producers \
             never reach the synchronous cold-build hold, so a rendezvous armed \
             for one could never observe them"
        );
        let shared = Arc::new(FlightRendezvousShared {
            key,
            state: Mutex::new(FlightRendezvousState::default()),
            changed: parking_lot::Condvar::new(),
        });
        self.flight_rendezvous.lock().push(Arc::clone(&shared));
        self.flight_rendezvous_armed
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        FlightRendezvous {
            store: self,
            shared,
        }
    }

    /// Whether no rendezvous is armed on this store.
    #[must_use]
    pub fn flight_rendezvous_is_disarmed(&self) -> bool {
        self.flight_rendezvous.lock().is_empty()
    }

    /// The armed rendezvous a cold build of `key` reports to: the first
    /// one armed for that key, so a rendezvous armed later for another key
    /// never shadows an older live one. Cloned out so no store lock is held
    /// while a hook point signals or parks. The armed count is the lock-free
    /// miss path: with no rendezvous armed anywhere on the store, a cold
    /// build or joined wait never takes the table's mutex at all.
    pub(super) fn armed_flight_rendezvous(
        &self,
        key: &crate::semantic_query::SemanticQueryKey,
    ) -> Option<Arc<FlightRendezvousShared>> {
        if self
            .flight_rendezvous_armed
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            return None;
        }
        self.flight_rendezvous
            .lock()
            .iter()
            .find(|armed| &armed.key == key)
            .cloned()
    }
}
