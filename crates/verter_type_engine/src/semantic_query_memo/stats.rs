//! Telemetry — optional store counters and the diagnosis-instrumented
//! entries-mutex guard.
//!
//! `AtomicSemanticGraphStats` holds the store's measurement-only counter
//! set. No production decision reads it: every counter is a [`Tally`], an
//! atomic only under the observation gate
//! (`cfg(any(test, feature = "test-support", feature = "semantic-observe"))`) and a zero-size no-op
//! otherwise, and the [`SemanticGraphStore::stats_snapshot`] reader exists
//! only under the same gate, so a default build neither counts nor can
//! report a fabricated zero. `SampleCollector` is the bounded reservoir
//! backing the path-length histogram. `InFlightStatsGuard` is a
//! panic-safe RAII guard that decrements the in-flight presence counter.
//! `EntriesLockGuard` wraps the entries mutex with wait/hold timing for
//! the diagnosis benchmark.

use std::sync::atomic::Ordering;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use std::sync::atomic::{AtomicU32, AtomicU64};

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use parking_lot::Mutex;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use rustc_hash::FxHashMap;

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use super::derivation::sorted_percentile;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use super::SemanticGraphStore;
// Only the (gated) `EntriesLockGuard::hold_start` timing field uses
// `Instant`; gate the import to match so release does not see it unused.
#[cfg(any(test, feature = "test-support"))]
use crate::instant::Instant;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
use crate::semantic_query::{SemanticGraphStats, SemanticNodeId};

/// One measurement-only store counter.
///
/// Under the observation gate it is a relaxed atomic; without it it is a
/// zero-size value whose `fetch_add` compiles to nothing and which has no
/// reader at all, so an unobserved build carries no counter and cannot
/// substitute zero for one it never kept.
#[derive(Default)]
pub(crate) struct Tally {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    value: AtomicU64,
}

impl Tally {
    /// Add `amount` to the counter. A no-op without the observation gate.
    #[inline(always)]
    pub(crate) fn fetch_add(&self, amount: u64, order: Ordering) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        self.value.fetch_add(amount, order);
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = (amount, order);
    }

    /// Read the counter. Exists only where the counter is kept.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(crate) fn load(&self, order: Ordering) -> u64 {
        self.value.load(order)
    }
}

/// Bounded sample reservoir for histogram-style metrics (path length /
/// projection depth). Cap = 8192 samples per metric; once full, new
/// samples replace at a round-robin index so later samples have a chance
/// to land in the reservoir without unbounded memory growth.
///
/// Percentiles are computed at snapshot time by sorting a clone of the
/// reservoir — O(N log N) per snapshot where N <= cap, which is fine for
/// observational reads.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
pub(super) struct SampleCollector {
    samples: Vec<u32>,
    cap: usize,
    inserts: u64,
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
impl SampleCollector {
    pub(super) fn with_cap(cap: usize) -> Self {
        Self {
            samples: Vec::new(),
            cap,
            inserts: 0,
        }
    }

    pub(super) fn push(&mut self, value: u32) {
        self.inserts = self.inserts.saturating_add(1);
        if self.samples.len() < self.cap {
            self.samples.push(value);
        } else if self.cap > 0 {
            let idx = (self.inserts as usize) % self.cap;
            self.samples[idx] = value;
        }
    }

    pub(super) fn percentile(&self, p: f64) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
        sorted[idx]
    }
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
pub(super) const SAMPLE_RESERVOIR_CAP: usize = 8192;

/// Measurement-only counter set updated on the hot path. Read into the immutable
/// [`SemanticGraphStats`](crate::semantic_query::SemanticGraphStats)
/// snapshot via [`super::SemanticGraphStore::stats_snapshot`].
///
/// Visibility: `pub(crate)` is the narrowest workable level. The
/// type is reachable at `pub(crate)` through the public
/// `SemanticGraphStore::stats` field's signature (`SemanticGraphStore`
/// itself is `pub`); narrowing the type to `pub(super)` triggers
/// the `private_interfaces` warning. Production callers outside
/// `semantic_query_memo` do not name the type — only the parent
/// module reaches it — so the broader visibility is documentation-
/// only in practice.
#[cfg_attr(
    not(any(test, feature = "test-support", feature = "semantic-observe")),
    derive(Default)
)]
pub(crate) struct AtomicSemanticGraphStats {
    pub(super) hits: Tally,
    pub(super) misses: Tally,
    pub(super) same_path_sentinel_returns: Tally,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) in_flight_current: AtomicU32,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) in_flight_peak: AtomicU32,
    pub(super) waits_ms: Tally,
    pub(super) joined_waits: Tally,
    pub(super) inflight_aborted_retries: Tally,
    /// Number of cooperative joiners that woke from the condvar and
    /// forked to a cold recompute rather than reusing the winner's
    /// result. A fork happens in two cases: the winner's carrier did
    /// NOT validate against the joiner's own `ctx` (the winner ran
    /// under a different overlay / view); or the winner was
    /// `cache_suppress` and its carrier carries no view-discriminating
    /// self-root (an unrootable build with an empty self-root set), so its carrier
    /// could only ever validate vacuously and is unsafe to reuse
    /// cross-view. A view-validated coalesce does not bump this
    /// counter; only a fork does.
    pub(super) joiner_view_mismatch_forks: Tally,
    pub(super) cold_aborts_swept: Tally,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) origin_edges_emitted: Tally,
    pub(super) instantiate_count: Tally,
    pub(super) conditional_decided_count: Tally,
    pub(super) conditional_deferred_count: Tally,
    pub(super) branch_selections_true: Tally,
    pub(super) branch_selections_false: Tally,
    /// Cold `AwaitedNormalize` builds observed by the store. Bumped by
    /// the builder itself, so a warm family hit does NOT increment it —
    /// the discriminating signal for awaited-relation memoization.
    pub(super) awaited_normalize_count: Tally,
    /// Cold `AsyncReturnPayload` builds observed by the store. Separate
    /// from `awaited_normalize_count`: the two relations are distinct
    /// families and must be countable apart.
    pub(super) async_return_payload_count: Tally,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) path_length_samples: Mutex<SampleCollector>,
    pub(super) decl_subexpression_lowering_count: Tally,
    pub(super) relation_check_count: Tally,
    /// Number of `intern_preserving_scope` calls observed by the
    /// store. The substitute helper's change-tracking optimisation
    /// short-circuits no-op arms and skips the rebuild +
    /// `intern_preserving_scope` call entirely, so a substitution
    /// that does not change anything must not increment this
    /// counter — it is the discriminating signal for the
    /// optimisation in tests.
    pub(super) intern_preserving_scope_calls: Tally,
    /// Per-K mapped-type materialiser invocations observed by the
    /// store. Increments once per call to either
    /// `materialize_mapped_member_value_for_key` (the
    /// `build_mapped_type` per-K path) or
    /// `materialize_selected_key_mapped_value` (the Shallow walker's
    /// `synthesise_mapped_surface` per-K path).
    ///
    /// Discriminating signal for the key-space-independent value
    /// hoist: a K-independent value expression collapses the per-K
    /// loop to ONE shared evaluation (not registered here because the
    /// hoist routes through evaluate_deferred + Instantiate directly,
    /// bypassing the per-K materialiser), so the counter should be
    /// `0` for hoist-eligible mapped types and `N = key_count` for
    /// K-dependent mapped types.
    pub(super) mapped_per_k_materializations: Tally,
    /// Hash-cons memo hits on
    /// `substitute_semantic_type_param`. Discriminating signal for
    /// the substitute hash-cons: a second call with the same
    /// `(value_expr, parameter_node, arg)` triple short-circuits
    /// the recursive walk and bumps this counter.
    pub(super) substitute_memo_hits: Tally,
    /// Hash-cons memo misses on
    /// `substitute_semantic_type_param`. Increments when the cache
    /// has no entry for the queried triple — the recursive walk
    /// runs and the result is published into the memo.
    pub(super) substitute_memo_misses: Tally,
    /// Hash-cons memo hits on
    /// `evaluate_deferred_semantic_node_with_context`. Bumped when
    /// a `(node, context)` pair is served from the memo, skipping
    /// the recursive fix-point walk.
    pub(super) evaluate_deferred_memo_hits: Tally,
    /// Hash-cons memo misses on
    /// `evaluate_deferred_semantic_node_with_context`. Bumped when
    /// the cache has no entry for the queried pair and a fresh
    /// fix-point walk runs.
    pub(super) evaluate_deferred_memo_misses: Tally,
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
impl Default for AtomicSemanticGraphStats {
    fn default() -> Self {
        Self {
            hits: Tally::default(),
            misses: Tally::default(),
            same_path_sentinel_returns: Tally::default(),
            in_flight_current: AtomicU32::new(0),
            in_flight_peak: AtomicU32::new(0),
            waits_ms: Tally::default(),
            joined_waits: Tally::default(),
            inflight_aborted_retries: Tally::default(),
            joiner_view_mismatch_forks: Tally::default(),
            cold_aborts_swept: Tally::default(),
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            origin_edges_emitted: Tally::default(),
            instantiate_count: Tally::default(),
            conditional_decided_count: Tally::default(),
            conditional_deferred_count: Tally::default(),
            branch_selections_true: Tally::default(),
            branch_selections_false: Tally::default(),
            awaited_normalize_count: Tally::default(),
            async_return_payload_count: Tally::default(),
            path_length_samples: Mutex::new(SampleCollector::with_cap(SAMPLE_RESERVOIR_CAP)),
            decl_subexpression_lowering_count: Tally::default(),
            relation_check_count: Tally::default(),
            intern_preserving_scope_calls: Tally::default(),
            mapped_per_k_materializations: Tally::default(),
            substitute_memo_hits: Tally::default(),
            substitute_memo_misses: Tally::default(),
            evaluate_deferred_memo_hits: Tally::default(),
            evaluate_deferred_memo_misses: Tally::default(),
        }
    }
}

impl AtomicSemanticGraphStats {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub(super) fn record_in_flight_enter(&self) {
        let now = self.in_flight_current.fetch_add(1, Ordering::Relaxed) + 1;
        // Compare-exchange peak forward; relaxed ordering is fine because
        // the peak is purely observational.
        let mut peak = self.in_flight_peak.load(Ordering::Relaxed);
        while now > peak {
            match self.in_flight_peak.compare_exchange_weak(
                peak,
                now,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[inline(always)]
    pub(super) fn record_in_flight_exit(&self) {
        self.in_flight_current.fetch_sub(1, Ordering::Relaxed);
    }
}

/// RAII guard that decrements the in-flight presence counter on drop —
/// fires whether the cold-build closure returns normally or panics.
/// Without this guard a panic in the build closure would leak the
/// in-flight counter, biasing `in_flight_peak` upward across the
/// remaining lifetime of the store.
pub(super) struct InFlightStatsGuard<'a> {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    stats: Option<&'a AtomicSemanticGraphStats>,
    #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
    lifetime: std::marker::PhantomData<&'a AtomicSemanticGraphStats>,
}

impl<'a> InFlightStatsGuard<'a> {
    #[inline(always)]
    pub(super) fn new<M: verter_audit::observe::ObserveExec>(
        stats: &'a AtomicSemanticGraphStats,
    ) -> Self {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            let stats = if super::records_telemetry::<M>() {
                stats.record_in_flight_enter();
                Some(stats)
            } else {
                None
            };
            Self { stats }
        }
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        {
            let _ = (stats, std::marker::PhantomData::<M>);
            Self {
                lifetime: std::marker::PhantomData,
            }
        }
    }
}

// The shipped guard carries no optional state, including its lifetime marker.
#[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
const _: () = assert!(std::mem::size_of::<InFlightStatsGuard<'static>>() == 0);

impl Drop for InFlightStatsGuard<'_> {
    fn drop(&mut self) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        if let Some(stats) = self.stats {
            stats.record_in_flight_exit();
        }
    }
}

/// RAII wrapper around a `parking_lot::MutexGuard` for the
/// `SemanticGraphStore::entries` mutex. Records the wait time
/// observed at acquisition and the hold time observed at drop on the
/// active [`crate::capture_token::CaptureToken`]. diagnosis
/// instrumentation only — the production hot path pays one extra
/// `Instant::now()` read per acquisition (constant-time) and the
/// Drop is a single `Instant::elapsed()` plus the no-op
/// `with_active_capture` hook when no token is bound.
pub(crate) struct EntriesLockGuard<'a, T> {
    pub(super) guard: Option<parking_lot::MutexGuard<'a, T>>,
    // Timing fields feed the capture-token entries-mutex hook only;
    // gated to match the instrumentation module (absent in release).
    #[cfg(any(test, feature = "test-support"))]
    pub(super) hold_start: Instant,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) wait_ns: u128,
}

impl<'a, T> std::ops::Deref for EntriesLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard
            .as_ref()
            .expect("guard taken before Drop")
            .deref()
    }
}

impl<'a, T> std::ops::DerefMut for EntriesLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard
            .as_mut()
            .expect("guard taken before Drop")
            .deref_mut()
    }
}

impl<'a, T> Drop for EntriesLockGuard<'a, T> {
    fn drop(&mut self) {
        // Drop the inner guard FIRST so the mutex is released before
        // we record the hold time. Releasing the lock before the
        // capture-token hook keeps the hold-time measurement honest:
        // the hook itself runs outside the critical section. We use
        // `Option::take` + explicit `drop` (the `let _ = ...` form
        // is a clippy `let_underscore_lock` violation because it
        // could otherwise be read as a no-op binding).
        if let Some(guard) = self.guard.take() {
            std::mem::drop(guard);
        }
        // Entries-mutex timing recording — test/debug instrumentation
        // only; gated to match the capture-token module (absent in
        // release).
        #[cfg(any(test, feature = "test-support"))]
        let hold_ns = self.hold_start.elapsed().as_nanos();
        #[cfg(any(test, feature = "test-support"))]
        let wait_ns = self.wait_ns;
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| {
            t.record_entries_mutex_timing(wait_ns, hold_ns);
            t.record_counter(crate::capture_token::ENTRIES_MUTEX_ACQUISITIONS, 1);
        });
    }
}

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
impl SemanticGraphStore {
    /// Read an immutable snapshot of every counter the store maintains.
    /// Safe to call mid-request; counters are atomic and percentile
    /// computation locks-and-clones the sample reservoir so no torn
    /// reads.
    #[must_use]
    pub fn stats_snapshot(&self) -> SemanticGraphStats {
        let derivation = self.derivation.lock();
        let origin_edge_count = derivation.edge_count() as u64;
        let mut by_node: FxHashMap<SemanticNodeId, u32> = FxHashMap::default();
        for (node, _kind, edges) in derivation.iter_edges() {
            let cell = by_node.entry(*node).or_insert(0);
            *cell = cell.saturating_add(edges.len() as u32);
        }
        drop(derivation);
        let mut per_node_counts: Vec<u32> = by_node.into_values().collect();
        per_node_counts.sort_unstable();
        let origin_edges_per_node_p50 = sorted_percentile(&per_node_counts, 0.5);
        let origin_edges_per_node_p95 = sorted_percentile(&per_node_counts, 0.95);

        let stats = &self.stats;
        let path_samples = stats.path_length_samples.lock();
        let path_length_p50 = path_samples.percentile(0.5);
        let path_length_p95 = path_samples.percentile(0.95);
        drop(path_samples);

        SemanticGraphStats {
            hits: stats.hits.load(Ordering::Relaxed),
            misses: stats.misses.load(Ordering::Relaxed),
            same_path_sentinel_returns: stats.same_path_sentinel_returns.load(Ordering::Relaxed),
            in_flight_peak: stats.in_flight_peak.load(Ordering::Relaxed),
            waits_ms: stats.waits_ms.load(Ordering::Relaxed),
            memo_entry_count: self.memo_entry_count() as u64,
            joined_waits: stats.joined_waits.load(Ordering::Relaxed),
            inflight_aborted_retries: stats.inflight_aborted_retries.load(Ordering::Relaxed),
            cold_aborts_swept: stats.cold_aborts_swept.load(Ordering::Relaxed),
            origin_edge_count,
            origin_edges_emitted: stats.origin_edges_emitted.load(Ordering::Relaxed),
            origin_edges_per_node_p50,
            origin_edges_per_node_p95,
            instantiate_count: stats.instantiate_count.load(Ordering::Relaxed),
            conditional_decided_count: stats.conditional_decided_count.load(Ordering::Relaxed),
            conditional_deferred_count: stats.conditional_deferred_count.load(Ordering::Relaxed),
            branch_selections_true: stats.branch_selections_true.load(Ordering::Relaxed),
            branch_selections_false: stats.branch_selections_false.load(Ordering::Relaxed),
            awaited_normalize_count: stats.awaited_normalize_count.load(Ordering::Relaxed),
            async_return_payload_count: stats.async_return_payload_count.load(Ordering::Relaxed),
            path_length_p50,
            path_length_p95,
            decl_subexpression_lowering_count: stats
                .decl_subexpression_lowering_count
                .load(Ordering::Relaxed),
            relation_check_count: stats.relation_check_count.load(Ordering::Relaxed),
            mapped_per_k_materializations: stats
                .mapped_per_k_materializations
                .load(Ordering::Relaxed),
            substitute_memo_hits: stats.substitute_memo_hits.load(Ordering::Relaxed),
            substitute_memo_misses: stats.substitute_memo_misses.load(Ordering::Relaxed),
            evaluate_deferred_memo_hits: stats.evaluate_deferred_memo_hits.load(Ordering::Relaxed),
            evaluate_deferred_memo_misses: stats
                .evaluate_deferred_memo_misses
                .load(Ordering::Relaxed),
        }
    }
}
