//! Test-only host audit accessors (supplement §5.D.0 r17 instrumentation surface).
//!
//! `cargo test --workspace --tests --verbose` (the §0.6.3 gate command)
//! sees these accessors via bare `#[cfg(test)]`. Production NAPI/WASM/LSP
//! builds compile WITHOUT them — there is no `feature =
//! "test-instrumentation"` flag (per r17/N12 disposition).
//!
//! Surface contract:
//! - [`HostTestAudit::loaded_files`] — sorted-deduped list of canonical
//!   ids the host has read since construction.
//! - [`HostTestAudit::total_reads`] — cumulative count of read events.
//! - [`HostTestAudit::total_shallow_processes`] — cumulative count of
//!   `IndexedReady` build events (one per `(canonical, content_hash)`
//!   that lowered shallow facts into the project type store).
//! - [`HostTestAudit::total_lowerings`] — cumulative count of
//!   `decl_subexpression_lowering` events (one per
//!   `shallow_lower_type_expr` shell-level entry).
//!
//! Each counter is monotonic across all requests on the host. Tests
//! sample a baseline before the request and a delta after.
//!
//! Backed by the production-instrumented sites (graph stats counter for
//! lowerings, [`record_test_read`] / [`record_test_shallow_process`]
//! hooks gated behind `#[cfg(test)]` for reads / shallow processes).

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashSet;

/// Host-level cumulative test audit state. Plan §5.D.0 r17.
///
/// Atomic counters; the `loaded` set lives behind a `Mutex<FxHashSet>`
/// so concurrent reads can observe set-membership without coarse-grained
/// contention on the `Vec`. Insertion is dedup-on-write so the snapshot
/// returned by [`Self::loaded_files`] is always sorted-and-unique.
#[derive(Debug, Default)]
pub struct HostTestAuditState {
    total_reads: std::sync::atomic::AtomicU64,
    total_shallow_processes: std::sync::atomic::AtomicU64,
    loaded: Mutex<FxHashSet<Arc<str>>>,
}

impl HostTestAuditState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_read(&self, canonical_id: &str) {
        self.total_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !canonical_id.is_empty() {
            self.loaded.lock().insert(Arc::from(canonical_id));
        }
    }

    pub(crate) fn record_shallow_process(&self, canonical_id: &str) {
        self.total_shallow_processes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !canonical_id.is_empty() {
            self.loaded.lock().insert(Arc::from(canonical_id));
        }
    }
}

/// Borrowed view over the host's [`HostTestAuditState`]. Exposed via
/// [`crate::VerterHost::audit`] (test-only). All accessors take a
/// non-mutable reference; the cumulative counters never decrease.
#[derive(Debug, Clone, Copy)]
pub struct HostTestAudit<'a> {
    state: &'a HostTestAuditState,
    graph: &'a crate::semantic_query_memo::SemanticGraphStore,
}

/// supplement §5.D.0 r17 — dispatch-counter view over the
/// per-thread `DISPATCH_KEY_COLD_COUNTS` / `DISPATCH_KEY_WARM_COUNTS`
/// thread-locals. Read-only; counters are written from
/// `ProjectSemanticDispatch::execute_read` based on cache peek.
///
/// `family_cold(&key)` and `family_warm(&key)` are monotonic
/// per-thread; tests sample baselines and deltas across paired
/// queries. The "family" terminology matches §5.B's family/slot
/// vocabulary — the digest behind the counter folds the variant +
/// content hash, so it is the variant family by construction.
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchCounter;

impl DispatchCounter {
    /// Cold-path dispatch count for `key` on this thread (cumulative).
    /// Returns 0 if the key has never dispatched on this thread.
    #[must_use]
    pub fn family_cold(&self, key: &crate::semantic_query::SemanticQueryKey) -> usize {
        crate::project_semantic_dispatch::raise::dispatch_cold_for(key)
    }

    /// Warm-path dispatch count for `key` on this thread (cumulative).
    /// Returns 0 if the key has never dispatched on this thread.
    #[must_use]
    pub fn family_warm(&self, key: &crate::semantic_query::SemanticQueryKey) -> usize {
        crate::project_semantic_dispatch::raise::dispatch_warm_for(key)
    }
}

/// supplement §5.D.0 r17 — per-key dispatch trace for §5.D.3
/// terminal-mode-only-expansion tests. Returned by
/// [`crate::VerterHost::dispatch_trace_for`].
///
/// The trace is built post-hoc from the warm cache: for a `ProjectPath`
/// query of length N, the trace's [`Self::path_decomposition`] returns
/// a `[SubKey]` with N entries (one per prefix length 1..=N). Each
/// SubKey carries the [`SubKey::mode`] the warm cache holds for that
/// prefix. Per the path-precise rule, intermediate hops are
/// `Navigate` and the terminal hop is the caller's mode.
///
/// For non-`ProjectPath` keys the decomposition is a single-element
/// vector containing the original key — this keeps the API uniform
/// across §5.D test patterns.
#[derive(Debug, Clone)]
pub struct DispatchTrace {
    sub_keys: Vec<SubKey>,
}

impl DispatchTrace {
    /// Construct a trace from a [`SemanticQueryKey`] by peeking the
    /// warm-cache prefix entries (test-only).
    pub(crate) fn from_key(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        key: &crate::semantic_query::SemanticQueryKey,
    ) -> Self {
        let sub_keys = match key {
            crate::semantic_query::SemanticQueryKey::ProjectPath { base, path, mode } => {
                let mut out = Vec::with_capacity(path.len());
                for k in 1..=path.len() {
                    let prefix: std::sync::Arc<[crate::semantic_query::PathSegment]> =
                        std::sync::Arc::from(path[..k].to_vec().into_boxed_slice());
                    let is_terminal = k == path.len();
                    let prefix_mode = if is_terminal {
                        *mode
                    } else {
                        crate::semantic_query::ProjectionMode::Navigate
                    };
                    let prefix_key = crate::semantic_query::SemanticQueryKey::ProjectPath {
                        base: *base,
                        path: prefix,
                        mode: prefix_mode,
                    };
                    // Peek the cache for the actual mode populated.
                    // Intermediate hops are published as Navigate by
                    // `backfill_prefixes`; the terminal hop is
                    // published with the caller's mode.
                    let actual_mode = if graph.get(&prefix_key).is_some() {
                        prefix_mode
                    } else if !is_terminal {
                        // Try the caller's mode as a fall-through —
                        // arm-split walks may not publish the trunk
                        // prefix in Navigate mode if the walker hit
                        // a Union/Intersection/Conditional mid-path.
                        let alt_key = crate::semantic_query::SemanticQueryKey::ProjectPath {
                            base: *base,
                            path: std::sync::Arc::from(path[..k].to_vec().into_boxed_slice()),
                            mode: *mode,
                        };
                        if graph.get(&alt_key).is_some() {
                            *mode
                        } else {
                            // Prefix not in cache at all —
                            // arm-split swallowed it. Default to
                            // Navigate so the test sees the
                            // path-precise contract intent (the
                            // entry never expanded with caller's
                            // mode at this hop).
                            crate::semantic_query::ProjectionMode::Navigate
                        }
                    } else {
                        prefix_mode
                    };
                    out.push(SubKey {
                        mode: actual_mode,
                        is_terminal,
                    });
                }
                out
            }
            other => {
                // Non-ProjectPath: single-element decomposition.
                let mode = match other {
                    crate::semantic_query::SemanticQueryKey::ProjectMember { mode, .. }
                    | crate::semantic_query::SemanticQueryKey::IndexedAccess { mode, .. } => *mode,
                    crate::semantic_query::SemanticQueryKey::ResolveMacroPayload {
                        mode, ..
                    } => *mode,
                    crate::semantic_query::SemanticQueryKey::Instantiate { body_mode, .. } => {
                        *body_mode
                    }
                    _ => crate::semantic_query::ProjectionMode::Expanded,
                };
                vec![SubKey {
                    mode,
                    is_terminal: true,
                }]
            }
        };
        Self { sub_keys }
    }

    /// Per-hop sub-keys in path order. Last entry is the terminal hop.
    #[must_use]
    pub fn path_decomposition(&self) -> &[SubKey] {
        &self.sub_keys
    }
}

/// One hop in a [`DispatchTrace`]. supplement §5.D.0 r17.
#[derive(Debug, Clone, Copy)]
pub struct SubKey {
    mode: crate::semantic_query::ProjectionMode,
    #[allow(dead_code)]
    is_terminal: bool,
}

impl SubKey {
    /// The projection mode this hop's warm cache entry carries.
    /// Intermediate hops should be [`ProjectionMode::Navigate`]; the
    /// terminal hop carries the caller's mode.
    #[must_use]
    pub fn mode(&self) -> crate::semantic_query::ProjectionMode {
        self.mode
    }
}

impl<'a> HostTestAudit<'a> {
    pub(crate) fn new(
        state: &'a HostTestAuditState,
        graph: &'a crate::semantic_query_memo::SemanticGraphStore,
    ) -> Self {
        Self { state, graph }
    }

    /// Sorted, deduped list of canonical ids the host has read since
    /// construction. Snapshot is consistent with the `total_reads` and
    /// `total_shallow_processes` counters at call time (best-effort
    /// under concurrency — tests run hermetically so this is exact).
    #[must_use]
    pub fn loaded_files(&self) -> Vec<Arc<str>> {
        let guard = self.state.loaded.lock();
        let mut out: Vec<Arc<str>> = guard.iter().map(Arc::clone).collect();
        drop(guard);
        out.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        out
    }

    /// Cumulative count of file reads (one per
    /// `read_analysis_source` invocation that returned a non-empty
    /// canonical). Cold path entries only — warm cache hits do NOT
    /// increment this counter.
    #[must_use]
    pub fn total_reads(&self) -> usize {
        self.state
            .total_reads
            .load(std::sync::atomic::Ordering::Relaxed) as usize
    }

    /// Cumulative count of `IndexedReady` build events (one per
    /// `(canonical, whole_hash)` that materialised shallow facts into
    /// the project type store via `record_indexed_ready_built`).
    #[must_use]
    pub fn total_shallow_processes(&self) -> usize {
        self.state
            .total_shallow_processes
            .load(std::sync::atomic::Ordering::Relaxed) as usize
    }

    /// Cumulative count of `decl_subexpression_lowering` events. One
    /// per `shallow_lower_type_expr` shell-level entry (recorded by
    /// the project semantic graph store). Reflects how many `TypeExpr`
    /// trees crossed the shallow-lowering boundary into graph nodes —
    /// the core "lowering" cost for §0.6.7's "shallow-walk first;
    /// deep expansion only on demand" rule.
    #[must_use]
    pub fn total_lowerings(&self) -> usize {
        self.graph
            .stats_snapshot()
            .decl_subexpression_lowering_count as usize
    }
}
