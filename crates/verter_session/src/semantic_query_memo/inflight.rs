//! FlightCell — same-key production owner.
//!
//! One [`FlightCell`] owns production for each in-flight semantic key
//! ([`PreparedKeyHandle`] — full-key equality behind one `Arc`). Concurrent
//! waiters join that cell; only the winner computes. Failed, incomplete,
//! cancelled, or stale production never publishes into
//! [`crate::project_type_store::ProjectTypeStore`]. RAII guards keep the
//! recursion stack and the in-flight table consistent across panics and
//! early returns.

use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

use crate::semantic_query::{DepSignature, QueryError, QueryResult, SemanticQueryValue};

use super::empty_signature;
use super::prepared::PreparedKeyHandle;

/// Same-key production cell for one in-flight semantic query.
///
/// The inner mutex guards `state` exclusively; `ready` is signalled when
/// the winner publishes. Joiners wait on `ready` via `wait_while`, so they
/// do not busy-retry. Ownership is released when the cell is retired from
/// the store's flight table after completion, abort, or panic.
pub(super) struct FlightCell {
    pub(super) state: Mutex<InflightState>,
    pub(super) ready: Condvar,
}

#[derive(Default)]
pub(super) struct InflightState {
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
    pub(super) graph_carrier: Option<Box<crate::fact_signature_helpers::ReadSetSignature>>,
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
    /// `ctx` via [`crate::fact_signature_helpers::ReadSetSignature::validate_with_self_roots`],
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
        Option<std::sync::Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>>,
    /// The winner build's `cache_suppress` flag. Set by the winner
    /// alongside `completed`; a joiner that observes `aborted == false`
    /// returns this verbatim in its `CacheRead.cache_suppress`. A
    /// `cache_suppress` winner is non-cacheable (tracer overflow,
    /// pathological input, or an unrootable / `None` signature); the
    /// joiner MUST inherit the same non-cacheability so a joiner inside
    /// an outer cold query cannot publish an outer entry that — through
    /// a composition helper threading the joiner's read — would
    /// otherwise be admitted despite a non-cacheable transitive child.
    /// `false` for the abort/retry path (the sentinel result there is
    /// not a real winner build).
    pub(super) cache_suppress: bool,
    /// The winner build's `result_is_partial` flag — the partial-result
    /// signal (budget / cancellation / recursion / walker fatal). Set by
    /// the winner alongside `completed`; a joiner that observes
    /// `aborted == false` returns this verbatim so a joiner inside an
    /// outer component-meta synthesis inherits the partial taint and the
    /// warm gate suppresses the outer result. `false` for the abort/retry
    /// path. Distinct from [`Self::cache_suppress`] (memo admission only).
    pub(super) result_is_partial: bool,
    /// The winner build's partial CLASSES — see
    /// [`crate::semantic_query::CacheRead::partial_reasons`]. Published
    /// alongside [`Self::result_is_partial`] and returned verbatim to a
    /// joiner, so a rendezvous does not launder a named class into the
    /// anonymous bridge on the follower's side.
    pub(super) partial_reasons: crate::semantic_query::PartialReasonSet,
    /// The generation-qualified execution owner that claimed this flight.
    /// Joiners register a temporary wait-for edge to this owner before
    /// parking, allowing cross-thread cycles to escape through ReturnOnly.
    pub(super) owner: Option<super::wait_cycle::ExecutionOwner>,
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
}

impl FlightCell {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(InflightState::default()),
            ready: Condvar::new(),
        }
    }
}

thread_local! {
    /// Per-thread stack of prepared query tokens currently being
    /// executed. Used to detect same-path recursion so callers return a
    /// sentinel instead of self-awaiting. Frames are `Arc` handles —
    /// pushing costs one refcount bump, and membership probes
    /// fast-reject on the token's cached key hash before full-key
    /// equality.
    pub(super) static IN_FLIGHT_ON_THIS_THREAD: RefCell<Vec<PreparedKeyHandle>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII guard that pops a frame off [`IN_FLIGHT_ON_THIS_THREAD`] when dropped.
///
/// Ensures the recursion stack stays consistent even if the cold build
/// panics — otherwise a caught panic or unwind could leave a frame on the
/// stack and future unrelated queries for that key from the same thread
/// would be misclassified as same-path recursion.
pub(super) struct RecursionStackGuard {
    handle: Option<PreparedKeyHandle>,
}

impl RecursionStackGuard {
    pub(super) fn push(handle: PreparedKeyHandle) -> Self {
        IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow_mut().push(handle.clone()));
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for RecursionStackGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            IN_FLIGHT_ON_THIS_THREAD.with(|slot| {
                let mut v = slot.borrow_mut();
                // Pop the exact frame this guard pushed — pointer
                // identity, no key comparison.
                if let Some(pos) = v.iter().rposition(|k| k.same_instance(&handle)) {
                    v.remove(pos);
                }
            });
        }
    }
}

/// RAII guard that fails the in-flight entry if the cold build panics.
///
/// Without this guard, a panic inside the winner's build closure would
/// leave `state.claimed == true` with `state.completed == None`. Any
/// subsequent caller for the same key would block on the condvar forever
/// because no publish ever wakes it. The guard detects the abnormal drop
/// via a `completed` flag, marks the entry with an error sentinel, wakes
/// joiners, and removes the entry from the in-flight table so fresh
/// callers start a new build.
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
        // Panic / early-return path — mark the entry completed with an
        // error sentinel so joiners can wake and fail fresh rather than
        // wait forever on a condvar that will never be signalled.
        {
            let mut state = self.inflight.state.lock();
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "cold build aborted (panic or early return)",
                ))));
                state.dep_signature = Some(empty_signature());
            }
        }
        self.inflight.ready.notify_all();
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
pub(super) const MAX_INFLIGHT_RETRIES: usize = 3;

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
pub(crate) struct InlineMemberFlight {
    pub(super) prepared: PreparedKeyHandle,
    pub(super) inflight: Arc<FlightCell>,
    /// Present only when an inline flight starts outside an existing
    /// semantic execution stack. Production nested members reuse the
    /// active owner; direct callers hold this detached RAII lease.
    _owner_registration: Option<super::wait_cycle::ExecutionOwnerRegistration>,
}

impl std::fmt::Debug for InlineMemberFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineMemberFlight")
            .field("key", self.prepared.key())
            .finish_non_exhaustive()
    }
}

impl InlineMemberFlight {
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
        self.inflight.ready.notify_all();
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
        let prepared = PreparedKeyHandle::prepare(key);
        let inflight = Arc::new(FlightCell::new());
        let (owner, owner_registration) = if let Some(owner) =
            super::wait_cycle::ExecutionOwnerScope::current(&self.wait_for_graph)
        {
            (owner, None)
        } else {
            let registration = self.wait_for_graph.register_owner();
            (registration.owner(), Some(registration))
        };
        {
            let mut state = inflight.state.lock();
            state.claimed = true;
            state.owner = Some(owner);
        }
        let mut table = self.inflight.lock();
        if table.contains_key(&prepared) {
            return None;
        }
        table.insert(prepared.clone(), Arc::clone(&inflight));
        Some(InlineMemberFlight {
            prepared,
            inflight,
            _owner_registration: owner_registration,
        })
    }

    /// Release an inline flight that cannot publish a decided member.
    /// Waiting top-level callers wake on the abort sentinel and retry
    /// admission.
    pub(crate) fn abort_inline_member_flight(&self, flight: &InlineMemberFlight) {
        {
            let mut state = flight.inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "inline member flight abandoned",
                ))));
                state.dep_signature = Some(empty_signature());
            }
            state.graph_carrier = None;
            state.walker_diagnostics = None;
            state.cache_suppress = true;
            state.result_is_partial = true;
        }
        flight.inflight.ready.notify_all();
        self.retire_inflight(&flight.prepared, &flight.inflight, false);
    }
}
