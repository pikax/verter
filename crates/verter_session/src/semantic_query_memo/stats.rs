//! Telemetry — atomic counters and the diagnosis-instrumented
//! entries-mutex guard.
//!
//! `AtomicSemanticGraphStats` holds the lock-free counter set updated on
//! the hot path. `SampleCollector` is the bounded reservoir backing
//! histogram-style metrics (path length, projection depth).
//! `InFlightStatsGuard` is a panic-safe RAII guard that decrements the
//! in-flight presence counter. `EntriesLockGuard` wraps the entries
//! mutex with wait/hold timing for the diagnosis benchmark.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use parking_lot::Mutex;

/// Bounded sample reservoir for histogram-style metrics (path length /
/// projection depth). Cap = 8192 samples per metric; once full, new
/// samples replace at a round-robin index so later samples have a chance
/// to land in the reservoir without unbounded memory growth.
///
/// Percentiles are computed at snapshot time by sorting a clone of the
/// reservoir — O(N log N) per snapshot where N <= cap, which is fine for
/// observational reads.
pub(super) struct SampleCollector {
    samples: Vec<u32>,
    cap: usize,
    inserts: u64,
}

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

pub(super) const SAMPLE_RESERVOIR_CAP: usize = 8192;

/// Lock-free counter set updated on the hot path. Read into the immutable
/// [`SemanticGraphStats`](crate::semantic_query::SemanticGraphStats)
/// snapshot via [`super::SemanticGraphStore::stats_snapshot`].
pub(super) struct AtomicSemanticGraphStats {
    pub(super) hits: AtomicU64,
    pub(super) misses: AtomicU64,
    pub(super) same_path_sentinel_returns: AtomicU64,
    pub(super) in_flight_current: AtomicU32,
    pub(super) in_flight_peak: AtomicU32,
    pub(super) waits_ms: AtomicU64,
    pub(super) joined_waits: AtomicU64,
    pub(super) inflight_aborted_retries: AtomicU64,
    pub(super) cold_aborts_swept: AtomicU64,
    pub(super) origin_edges_emitted: AtomicU64,
    pub(super) instantiate_count: AtomicU64,
    pub(super) conditional_decided_count: AtomicU64,
    pub(super) conditional_deferred_count: AtomicU64,
    pub(super) branch_selections_true: AtomicU64,
    pub(super) branch_selections_false: AtomicU64,
    pub(super) budget_fallback_count: AtomicU64,
    pub(super) path_length_samples: Mutex<SampleCollector>,
    pub(super) projection_depth_samples: Mutex<SampleCollector>,
    pub(super) decl_subexpression_lowering_count: AtomicU64,
    pub(super) relation_check_count: AtomicU64,
    /// Of `intern_preserving_scope` calls
    /// observed by the store. Pre-Fix-D substitute helpers rebuilt
    /// every match arm unconditionally; post-Fix-D the no-op
    /// branches short-circuit and skip the call entirely.
    /// Discriminating signal for the change-tracking optimization.
    pub(super) intern_preserving_scope_calls: AtomicU64,
}

impl Default for AtomicSemanticGraphStats {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            same_path_sentinel_returns: AtomicU64::new(0),
            in_flight_current: AtomicU32::new(0),
            in_flight_peak: AtomicU32::new(0),
            waits_ms: AtomicU64::new(0),
            joined_waits: AtomicU64::new(0),
            inflight_aborted_retries: AtomicU64::new(0),
            cold_aborts_swept: AtomicU64::new(0),
            origin_edges_emitted: AtomicU64::new(0),
            instantiate_count: AtomicU64::new(0),
            conditional_decided_count: AtomicU64::new(0),
            conditional_deferred_count: AtomicU64::new(0),
            branch_selections_true: AtomicU64::new(0),
            branch_selections_false: AtomicU64::new(0),
            budget_fallback_count: AtomicU64::new(0),
            path_length_samples: Mutex::new(SampleCollector::with_cap(SAMPLE_RESERVOIR_CAP)),
            projection_depth_samples: Mutex::new(SampleCollector::with_cap(SAMPLE_RESERVOIR_CAP)),
            decl_subexpression_lowering_count: AtomicU64::new(0),
            relation_check_count: AtomicU64::new(0),
            intern_preserving_scope_calls: AtomicU64::new(0),
        }
    }
}

impl AtomicSemanticGraphStats {
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
    pub(super) stats: &'a AtomicSemanticGraphStats,
}

impl Drop for InFlightStatsGuard<'_> {
    fn drop(&mut self) {
        self.stats.record_in_flight_exit();
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
pub(super) struct EntriesLockGuard<'a, T> {
    pub(super) guard: Option<parking_lot::MutexGuard<'a, T>>,
    pub(super) hold_start: Instant,
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
        let hold_ns = self.hold_start.elapsed().as_nanos();
        let wait_ns = self.wait_ns;
        crate::capture_token::with_active_capture(|t| {
            t.record_entries_mutex_timing(wait_ns, hold_ns);
        });
    }
}
