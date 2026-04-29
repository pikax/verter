//! Test-only host audit accessors (Phase 5g-supplement §5.D.0 r17 instrumentation surface).
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
        self.graph.stats_snapshot().decl_subexpression_lowering_count as usize
    }
}
