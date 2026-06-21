//! Option-A instrumentation counters for the context-keyed structural-body memo.
//!
//! These are the MEASURED escalation evidence for the A-vs-B perf consult on the
//! context-keyed structural body memo
//! ([`StructuralBodyMemo`](super::structural_body_memo::StructuralBodyMemo)): the
//! warm hit-rate, distinct-live-contexts-per-body fan-out, arena pressure, and
//! relower-once-per-context cost. They follow the
//! [`loop5_instrumentation`](crate::loop5_instrumentation) model EXACTLY:
//! module-scope relaxed [`AtomicU64`] counters, bumped at the real memo sites,
//! read via [`dump_structural_body_memo_instrumentation`], reset via
//! [`reset_structural_body_memo_instrumentation`].
//!
//! ## Zero-cost-off
//!
//! The counters are unconditionally compiled relaxed atomics — they are INERT in
//! production: a relaxed `fetch_add` on an uncontended atomic on the (currently
//! dead-code-until-wired) memo path is negligible, and NOTHING in production
//! READS them. They are NOT cfg-gated off (the loop5 model does not gate). The
//! "zero-cost-off" property is read-side: no production code path calls
//! `dump_*`; the dump exists for the perf-consult harness and the tests.
//!
//! ## Bump-site split
//!
//! The hit-rate + fan-out counters ([`STRUCTURAL_BODY_MEMO_LOOKUPS`],
//! [`STRUCTURAL_BODY_MEMO_HITS`], [`STRUCTURAL_BODY_MEMO_COLD_BUILDS`],
//! [`STRUCTURAL_BODY_MEMO_CELLS_CREATED`], [`STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS`])
//! are bumped NOW from the memo's real `get`/`insert` methods (via
//! [`record_get`] / [`record_insert_context`]), so they measure real traffic the
//! moment the cache is wired. The cold-build timing / failure / wiring-gap
//! counters ([`STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL`],
//! [`STRUCTURAL_BODY_MEMO_ERRORS`], [`STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS`])
//! have NO production cold-build/error/bypass path yet — that path lands with the
//! producer-wiring step — so they are DEFINED + test-exercised now via the public
//! [`record_cold_build_ns`] / [`record_error`] / [`record_direct_lower_bypass`]
//! increment helpers, and the producer-wiring step adds their production bump
//! sites. This is honest: the counter exists + is test-exercised; the production
//! bump site lands with the wiring.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::instant::Instant;
use crate::semantic_query::{MemberMergeRole, SurfaceProvenanceContext};

// ---------------------------------------------------------------------------
// The decided counter set (Option-A axes)
// ---------------------------------------------------------------------------

/// Total [`StructuralBodyMemo::get`](super::structural_body_memo::StructuralBodyMemo::get)
/// calls (warm + cold). Bumped on every `get` entry.
pub static STRUCTURAL_BODY_MEMO_LOOKUPS: AtomicU64 = AtomicU64::new(0);

/// `get` calls that returned `Some` (a warm hit — the context cell was already
/// memoized). Bumped from `get` when the lookup hits.
pub static STRUCTURAL_BODY_MEMO_HITS: AtomicU64 = AtomicU64::new(0);

/// `get` calls that MISSED (returned `None`) — the future cold-lower path. Bumped
/// from `get` when the lookup misses. `LOOKUPS == HITS + COLD_BUILDS`.
pub static STRUCTURAL_BODY_MEMO_COLD_BUILDS: AtomicU64 = AtomicU64::new(0);

/// `insert` calls — the number of distinct context cells materialized into the
/// memo. Bumped from `insert`. Arena-pressure axis: extra cells from duplicate
/// contexts show up as `CELLS_CREATED` outrunning the distinct bodies.
pub static STRUCTURAL_BODY_MEMO_CELLS_CREATED: AtomicU64 = AtomicU64::new(0);

/// Summed nanoseconds of cold builds (time ONLY the cold-lower path, never warm
/// hits). Bumped by the producer-wiring step's cold-lower path; DEFINED +
/// test-exercised now via [`record_cold_build_ns`] / [`ColdBuildTimerGuard`].
/// Divide by [`STRUCTURAL_BODY_MEMO_COLD_BUILDS`] for the mean cold-build ns.
pub static STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Cold builds that failed to produce a cell. Bumped by the producer-wiring
/// step's cold-lower error path; DEFINED + test-exercised now via
/// [`record_error`].
pub static STRUCTURAL_BODY_MEMO_ERRORS: AtomicU64 = AtomicU64::new(0);

/// The number of distinct (provenance × merge_role) contexts each body fans out
/// into, attributed across the 2 × 3 = 6-element cross-product (indexed by
/// [`context_bucket_index`]). Bumped per `insert` (per context cell created).
/// Distinct-live-contexts-per-body axis: a body that fans into many contexts
/// distributes its inserts across multiple buckets.
pub static STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS: [AtomicU64; CONTEXT_BUCKET_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Live-wiring-gap detector: a future path that lowers a context-body WITHOUT
/// consulting the memo bumps this. The producer-wiring step asserts it stays 0
/// (the memo is actually consulted). DEFINED + test-exercised now via
/// [`record_direct_lower_bypass`].
pub static STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS: AtomicU64 = AtomicU64::new(0);

/// The number of context buckets: 2 `SurfaceProvenanceContext` variants ×
/// 3 `MemberMergeRole` variants. The exhaustive `context_bucket_index` match
/// below is the structural pin keeping this in lockstep with the two axis enums.
pub const CONTEXT_BUCKET_COUNT: usize = 6;

/// Map a `(provenance, merge_role)` context to its dense bucket index in
/// `0..CONTEXT_BUCKET_COUNT`. Real arithmetic over the two axes' ordinals
/// (`provenance_ord * 3 + merge_role_ord`), so the 2 × 3 = 6 distinct contexts
/// map to 6 DISTINCT buckets (the discriminating property the test asserts: a
/// colliding fn such as `provenance_ord + merge_role_ord` would map e.g.
/// `(MacroTypeArgOwnBody, Authored)` and `(Structural, OwnBody)` to the same
/// index and FAIL the distinctness assertion).
///
/// The two inner matches are EXHAUSTIVE over the axis enums, so a new variant on
/// either axis fails to compile here — the structural pin that keeps the bucket
/// arithmetic and [`CONTEXT_BUCKET_COUNT`] in lockstep with the enums. Modelled
/// on `kind_index_for_key`.
pub fn context_bucket_index(
    provenance: SurfaceProvenanceContext,
    merge_role: MemberMergeRole,
) -> usize {
    let provenance_ord = match provenance {
        SurfaceProvenanceContext::Structural => 0,
        SurfaceProvenanceContext::MacroTypeArgOwnBody => 1,
    };
    let merge_role_ord = match merge_role {
        MemberMergeRole::Authored => 0,
        MemberMergeRole::OwnBody => 1,
        MemberMergeRole::Heritage => 2,
    };
    provenance_ord * 3 + merge_role_ord
}

// ---------------------------------------------------------------------------
// Bump helpers — called from the memo's real `get`/`insert` methods
// ---------------------------------------------------------------------------

/// Record one `get` lookup: always bump `LOOKUPS`; bump `HITS` if the lookup hit
/// (returned a cell), else bump `COLD_BUILDS` (the miss / future cold-lower path).
/// Called from [`StructuralBodyMemo::get`](super::structural_body_memo::StructuralBodyMemo::get).
#[inline]
pub(super) fn record_get(hit: bool) {
    STRUCTURAL_BODY_MEMO_LOOKUPS.fetch_add(1, Ordering::Relaxed);
    if hit {
        STRUCTURAL_BODY_MEMO_HITS.fetch_add(1, Ordering::Relaxed);
    } else {
        STRUCTURAL_BODY_MEMO_COLD_BUILDS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record one `insert`: bump `CELLS_CREATED` and the context bucket for the
/// inserted cell's `(provenance, merge_role)`. Called from
/// [`StructuralBodyMemo::insert`](super::structural_body_memo::StructuralBodyMemo::insert),
/// which reads its own (bundle-private) key fields and passes them here — the key
/// fields stay private; this helper takes the two axis enums by value.
#[inline]
pub(super) fn record_insert_context(
    provenance: SurfaceProvenanceContext,
    merge_role: MemberMergeRole,
) {
    STRUCTURAL_BODY_MEMO_CELLS_CREATED.fetch_add(1, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS[context_bucket_index(provenance, merge_role)]
        .fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Test-exercised-only helpers — the producer-wiring step adds their production
// bump sites (no cold-build / error / bypass path exists yet).
// ---------------------------------------------------------------------------

/// Add `ns` to the cold-build ns total. The production cold-lower path (which
/// times itself, e.g. via [`ColdBuildTimerGuard`]) lands with the producer-wiring
/// step; defined + test-exercised now.
#[inline]
pub fn record_cold_build_ns(ns: u64) {
    STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL.fetch_add(ns, Ordering::Relaxed);
}

/// Record one cold build that failed to produce a cell. The production error path
/// lands with the producer-wiring step; defined + test-exercised now.
#[inline]
pub fn record_error() {
    STRUCTURAL_BODY_MEMO_ERRORS.fetch_add(1, Ordering::Relaxed);
}

/// Record one direct-lower bypass (a path that lowered a context-body WITHOUT
/// consulting the memo). The production detector lands with the producer-wiring
/// step; defined + test-exercised now.
#[inline]
pub fn record_direct_lower_bypass() {
    STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// RAII timer that adds elapsed nanoseconds to
/// [`STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL`] on drop — the cold-build timing
/// guard the producer-wiring step wraps each cold-lower body in. Modelled on the
/// loop5 `TimerGuard` (ns-only variant: the `COLD_BUILDS` count is recorded by
/// the `get` miss, so this guard times without re-counting). Defined +
/// test-exercised now.
pub struct ColdBuildTimerGuard {
    started: Instant,
}

impl ColdBuildTimerGuard {
    /// Capture the start time; on drop, add the elapsed ns to the cold-build ns
    /// total.
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Default for ColdBuildTimerGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ColdBuildTimerGuard {
    fn drop(&mut self) {
        let elapsed_ns = self.started.elapsed().as_nanos() as u64;
        STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL.fetch_add(elapsed_ns, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// reset + dump (test isolation + perf-consult read)
// ---------------------------------------------------------------------------

/// Reset every structural-body-memo instrumentation counter to zero. Used between
/// test cases (and perf-consult passes) for per-pass attribution.
pub fn reset_structural_body_memo_instrumentation() {
    STRUCTURAL_BODY_MEMO_LOOKUPS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_HITS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_COLD_BUILDS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_CELLS_CREATED.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_ERRORS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS.store(0, Ordering::Relaxed);
    for slot in STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS.iter() {
        slot.store(0, Ordering::Relaxed);
    }
}

/// Snapshot every structural-body-memo instrumentation counter as a JSON-shaped
/// string, so the perf-consult harness can write it as a sidecar alongside the
/// rest of the audit JSON. Every counter's current value appears as a key.
pub fn dump_structural_body_memo_instrumentation() -> String {
    let lookups = STRUCTURAL_BODY_MEMO_LOOKUPS.load(Ordering::Relaxed);
    let hits = STRUCTURAL_BODY_MEMO_HITS.load(Ordering::Relaxed);
    let cold_builds = STRUCTURAL_BODY_MEMO_COLD_BUILDS.load(Ordering::Relaxed);
    let cells_created = STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(Ordering::Relaxed);
    let cold_build_ns_total = STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL.load(Ordering::Relaxed);
    let errors = STRUCTURAL_BODY_MEMO_ERRORS.load(Ordering::Relaxed);
    let direct_lower_bypass_calls =
        STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS.load(Ordering::Relaxed);

    let mut per_bucket = String::new();
    for (idx, bucket) in STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS.iter().enumerate() {
        let count = bucket.load(Ordering::Relaxed);
        if idx > 0 {
            per_bucket.push_str(",\n    ");
        } else {
            per_bucket.push_str("\n    ");
        }
        per_bucket.push_str(&format!("\"{idx}\": {count}"));
    }

    format!(
        "{{\n  \
         \"STRUCTURAL_BODY_MEMO_LOOKUPS\": {lookups},\n  \
         \"STRUCTURAL_BODY_MEMO_HITS\": {hits},\n  \
         \"STRUCTURAL_BODY_MEMO_COLD_BUILDS\": {cold_builds},\n  \
         \"STRUCTURAL_BODY_MEMO_CELLS_CREATED\": {cells_created},\n  \
         \"STRUCTURAL_BODY_MEMO_COLD_BUILD_NS_TOTAL\": {cold_build_ns_total},\n  \
         \"STRUCTURAL_BODY_MEMO_ERRORS\": {errors},\n  \
         \"STRUCTURAL_BODY_MEMO_DIRECT_LOWER_BYPASS_CALLS\": {direct_lower_bypass_calls},\n  \
         \"STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS\": {{{per_bucket}\n  }}\n}}"
    )
}

/// Process-wide serialization gate for tests that exercise a
/// [`StructuralBodyMemo`](super::structural_body_memo::StructuralBodyMemo) and
/// therefore mutate the global instrumentation counters. The counters are
/// process-global statics and `cargo test` runs tests in PARALLEL threads in one
/// process, so EVERY test that drives a memo (this module's counter-value tests
/// AND the sibling `structural_body_memo` core test) must hold this gate while it
/// runs — otherwise one test's `get`/`insert` bumps race another's exact-count
/// assertions. Acquire via [`lock_counter_test_gate`] (poison-tolerant, so one
/// genuine assertion failure reports its OWN message instead of a misleading
/// `PoisonError` in every sibling test).
#[cfg(test)]
pub(crate) static COUNTER_TEST_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`COUNTER_TEST_GATE`], tolerating prior poisoning (a previous test
/// that panicked while holding it). Returns the guard for RAII release.
#[cfg(test)]
pub(crate) fn lock_counter_test_gate() -> std::sync::MutexGuard<'static, ()> {
    COUNTER_TEST_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[path = "structural_body_memo_instrumentation_tests.rs"]
mod structural_body_memo_instrumentation_tests;
