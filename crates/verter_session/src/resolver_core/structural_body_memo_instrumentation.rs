//! Option-A instrumentation counters for the context-keyed structural-body memo.
//!
//! These are the MEASURED escalation evidence for the A-vs-B perf consult on the
//! context-keyed structural body memo
//! ([`StructuralBodyMemo`](super::structural_body_memo::StructuralBodyMemo)): the
//! warm hit-rate, distinct-live-contexts-per-body fan-out, and arena pressure.
//! They follow the [`loop5_instrumentation`](crate::loop5_instrumentation) model
//! EXACTLY: module-scope relaxed [`AtomicU64`] counters, bumped at the real memo
//! sites, read via [`dump_structural_body_memo_instrumentation`], reset via
//! [`reset_structural_body_memo_instrumentation`].
//!
//! ## Always-on low-overhead instrumentation
//!
//! The counters are a relaxed `fetch_add` per memo `get` / `insert`,
//! UNCONDITIONALLY compiled (the `loop5_instrumentation` model). A relaxed
//! `fetch_add` on an uncontended atomic is inexpensive enough to leave always-on;
//! they are NOT cfg-gated. This is NOT zero-cost — every `get`/`insert` performs
//! the relaxed RMW — but the cost is negligible and the counters are simply NOT
//! READ in production: no production code path calls `dump_*`; the dump exists
//! for the perf-consult harness and the tests.
//!
//! ## Every counter has a live bump site
//!
//! Every counter dumped here is bumped from the memo's real `get` / `insert`
//! methods (via [`record_get`] / [`record_insert_context`]), so each measures
//! real traffic the moment the cache is wired — no counter reports a constant
//! zero. The cold-build TIMING (summed cold-lower ns), the cold-lower FAILURE
//! count, and the direct-lower BYPASS detector land WITH the producer-wiring step
//! that introduces those paths (their bump sites only exist once the producer
//! times each cold lower, surfaces a lower failure, and detects a memo bypass);
//! they are intentionally NOT defined here, because a counter with no production
//! bump site would report a constant zero until then.
//!
//! ## Bump sites
//!
//! - [`STRUCTURAL_BODY_MEMO_LOOKUPS`] / [`STRUCTURAL_BODY_MEMO_HITS`] /
//!   [`STRUCTURAL_BODY_MEMO_MISSES`] — every memo `get` (via [`record_get`]).
//! - [`STRUCTURAL_BODY_MEMO_CELLS_CREATED`] /
//!   [`STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS`] — every memo `insert` that
//!   materializes a genuinely NEW distinct context cell (via
//!   [`record_insert_context`]); a re-insert over an existing key does NOT bump.

use std::sync::atomic::{AtomicU64, Ordering};

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

/// `get` calls that MISSED (returned `None`). A miss is the TRIGGER for a future
/// cold lower, NOT a cold build itself — the cold BUILD is what the producer-
/// wiring step does AFTER a miss, so this counts memo misses honestly. Bumped
/// from `get` when the lookup misses. `LOOKUPS == HITS + MISSES`.
pub static STRUCTURAL_BODY_MEMO_MISSES: AtomicU64 = AtomicU64::new(0);

/// `insert` calls that materialized a genuinely NEW distinct context cell (the
/// `insert` returned `None`) — the number of distinct context cells in the memo.
/// Bumped from `insert` ONLY when the key was not already present; a re-insert
/// over an existing key does NOT bump. Arena-pressure axis: extra cells from
/// distinct contexts show up as `CELLS_CREATED` outrunning the distinct bodies.
pub static STRUCTURAL_BODY_MEMO_CELLS_CREATED: AtomicU64 = AtomicU64::new(0);

/// The number of distinct (provenance × merge_role) contexts each body fans out
/// into, attributed across the 2 × 3 = 6-element cross-product (indexed by
/// [`context_bucket_index`]). Bumped per NEW `insert` (per distinct context cell
/// created — a re-insert over an existing key does NOT bump). Distinct-live-
/// contexts-per-body axis: a body that fans into many contexts distributes its
/// inserts across multiple buckets.
pub static STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS: [AtomicU64; CONTEXT_BUCKET_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

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
/// (returned a cell), else bump `MISSES` (the miss / future cold-lower trigger).
/// Called from [`StructuralBodyMemo::get`](super::structural_body_memo::StructuralBodyMemo::get).
#[inline]
pub(super) fn record_get(hit: bool) {
    STRUCTURAL_BODY_MEMO_LOOKUPS.fetch_add(1, Ordering::Relaxed);
    if hit {
        STRUCTURAL_BODY_MEMO_HITS.fetch_add(1, Ordering::Relaxed);
    } else {
        STRUCTURAL_BODY_MEMO_MISSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record one `insert` that materialized a NEW distinct context cell: bump
/// `CELLS_CREATED` and the context bucket for the inserted cell's
/// `(provenance, merge_role)`. Called from
/// [`StructuralBodyMemo::insert`](super::structural_body_memo::StructuralBodyMemo::insert)
/// ONLY when `HashMap::insert` returned `None` (the key was not already present),
/// so a re-insert over an existing key does NOT re-bump and `CELLS_CREATED`
/// stays the count of distinct context cells. `insert` reads its own
/// (bundle-private) key fields and passes them here — the key fields stay
/// private; this helper takes the two axis enums by value.
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
// reset + dump (test isolation + perf-consult read)
// ---------------------------------------------------------------------------

/// Reset every structural-body-memo instrumentation counter to zero. Used between
/// test cases (and perf-consult passes) for per-pass attribution.
pub fn reset_structural_body_memo_instrumentation() {
    STRUCTURAL_BODY_MEMO_LOOKUPS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_HITS.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_MISSES.store(0, Ordering::Relaxed);
    STRUCTURAL_BODY_MEMO_CELLS_CREATED.store(0, Ordering::Relaxed);
    for slot in STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS.iter() {
        slot.store(0, Ordering::Relaxed);
    }
}

/// Snapshot every structural-body-memo instrumentation counter as a JSON-shaped
/// string, so the perf-consult harness can write it as a sidecar alongside the
/// rest of the audit JSON. Every counter's current value appears as a key, and
/// every printed counter has a live `get`/`insert` bump site (no constant-zero
/// counter).
pub fn dump_structural_body_memo_instrumentation() -> String {
    let lookups = STRUCTURAL_BODY_MEMO_LOOKUPS.load(Ordering::Relaxed);
    let hits = STRUCTURAL_BODY_MEMO_HITS.load(Ordering::Relaxed);
    let misses = STRUCTURAL_BODY_MEMO_MISSES.load(Ordering::Relaxed);
    let cells_created = STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(Ordering::Relaxed);

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
         \"STRUCTURAL_BODY_MEMO_MISSES\": {misses},\n  \
         \"STRUCTURAL_BODY_MEMO_CELLS_CREATED\": {cells_created},\n  \
         \"STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS\": {{{per_bucket}\n  }}\n}}"
    )
}

/// Process-wide serialization gate for tests that exercise a
/// [`StructuralBodyMemo`](super::structural_body_memo::StructuralBodyMemo) and
/// therefore mutate the global instrumentation counters. The counters are
/// process-global statics and `cargo test` runs tests in PARALLEL threads in one
/// process, so ANY test that drives [`StructuralBodyMemo::get`](super::structural_body_memo::StructuralBodyMemo::get)
/// / [`StructuralBodyMemo::insert`](super::structural_body_memo::StructuralBodyMemo::insert)
/// OR asserts counter values MUST acquire this gate FIRST (the counters are
/// process-global; un-gated concurrent memo traffic races the exact-count
/// assertions). This module's counter-value tests AND the sibling
/// `structural_body_memo` core test both hold the gate while they run — otherwise
/// one test's `get`/`insert` bumps race another's exact-count assertions. Acquire
/// via [`lock_counter_test_gate`] (poison-tolerant, so one genuine assertion
/// failure reports its OWN message instead of a misleading `PoisonError` in every
/// sibling test).
#[cfg(test)]
pub(crate) static COUNTER_TEST_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`COUNTER_TEST_GATE`], tolerating prior poisoning (a previous test
/// that panicked while holding it). Returns the guard for RAII release.
///
/// Any test that drives [`StructuralBodyMemo::get`](super::structural_body_memo::StructuralBodyMemo::get)
/// / [`StructuralBodyMemo::insert`](super::structural_body_memo::StructuralBodyMemo::insert)
/// OR asserts counter values MUST call this first — see [`COUNTER_TEST_GATE`].
#[cfg(test)]
pub(crate) fn lock_counter_test_gate() -> std::sync::MutexGuard<'static, ()> {
    COUNTER_TEST_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[path = "structural_body_memo_instrumentation_tests.rs"]
mod structural_body_memo_instrumentation_tests;
