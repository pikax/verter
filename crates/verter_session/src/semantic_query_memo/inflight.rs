//! In-flight admission — per-entry Mutex + Condvar pair.
//!
//! Cold builds register an `InflightEntry` keyed by the full
//! [`SemanticQueryKey`]; joiners block on the entry's `Condvar` until the
//! winner publishes. RAII guards keep the recursion stack and the
//! in-flight table consistent across panics and early returns.

use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

use crate::semantic_query::{
    DepSignature, QueryError, QueryResult, SemanticNodeId, SemanticQueryKey,
};

use super::empty_signature;

/// In-flight admission state for one cold build.
///
/// The inner mutex guards `state` exclusively; `ready` is signalled when
/// the winner publishes. Joiners wait on `ready` via `wait_while`, so they
/// do not busy-retry.
pub(super) struct InflightEntry {
    pub(super) state: Mutex<InflightState>,
    pub(super) ready: Condvar,
}

#[derive(Default)]
pub(super) struct InflightState {
    /// `None` while building; `Some` after the winner publishes.
    pub(super) completed: Option<QueryResult<SemanticNodeId>>,
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

impl InflightEntry {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(InflightState::default()),
            ready: Condvar::new(),
        }
    }
}

thread_local! {
    /// Per-thread set of query keys currently being executed. Used to
    /// detect same-path recursion so callers return a sentinel instead of
    /// self-awaiting.
    pub(super) static IN_FLIGHT_ON_THIS_THREAD: RefCell<Vec<SemanticQueryKey>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII guard that pops a key off [`IN_FLIGHT_ON_THIS_THREAD`] when dropped.
///
/// Ensures the recursion stack stays consistent even if the cold build
/// panics — otherwise a caught panic or unwind could leave a key on the
/// stack and future unrelated queries for that key from the same thread
/// would be misclassified as same-path recursion.
pub(super) struct RecursionStackGuard {
    key: Option<SemanticQueryKey>,
}

impl RecursionStackGuard {
    pub(super) fn push(key: SemanticQueryKey) -> Self {
        IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow_mut().push(key.clone()));
        Self { key: Some(key) }
    }
}

impl Drop for RecursionStackGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            IN_FLIGHT_ON_THIS_THREAD.with(|slot| {
                let mut v = slot.borrow_mut();
                if let Some(pos) = v.iter().rposition(|k| k == &key) {
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
    inflight: Arc<InflightEntry>,
    inflight_table: &'a Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
    key: SemanticQueryKey,
    finished: bool,
}

impl<'a> InflightPanicGuard<'a> {
    pub(super) fn new(
        inflight: Arc<InflightEntry>,
        inflight_table: &'a Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
        key: SemanticQueryKey,
    ) -> Self {
        Self {
            inflight,
            inflight_table,
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
        // installed a fresh `InflightEntry` for the same key; an
        // unconditional remove would evict that fresh entry. (On the
        // panic path the winner never published a `graph_carrier`, so
        // a joiner cannot have forked off THIS build — but the guard
        // stays `ptr_eq`-correct for defence in depth and parity with
        // the normal-return step-7 retire.)
        let mut table = self.inflight_table.lock();
        if table
            .get(&self.key)
            .is_some_and(|entry| Arc::ptr_eq(entry, &self.inflight))
        {
            table.remove(&self.key);
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
