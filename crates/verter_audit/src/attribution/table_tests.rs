//! Substrate tests that need the counter table, so they only compile
//! with the `attribution` feature.
//!
//! These run in-process against process-global statics. Each test
//! therefore owns a DISJOINT set of sites — no test resets the table,
//! because a reset would race every other test in the binary.

use super::schema::{WorkSite, WorkUnit};
use super::table::{mix64, read, record_amount, record_call, record_digest, snapshot};

#[test]
fn call_recording_is_per_site_and_additive() {
    let before = read(WorkSite::TaskDedupJoin).calls;
    record_call(WorkSite::TaskDedupJoin);
    record_call(WorkSite::TaskDedupJoin);
    let after = read(WorkSite::TaskDedupJoin);
    assert_eq!(after.calls, before + 2);
    assert_eq!(after.amount, 0, "a call-unit site must not move `amount`");
}

#[test]
fn summing_site_accumulates_amount() {
    let before = read(WorkSite::CollapsePath);
    record_amount(WorkSite::CollapsePath, 7);
    record_amount(WorkSite::CollapsePath, 11);
    let after = read(WorkSite::CollapsePath);
    assert_eq!(after.calls, before.calls + 2);
    assert_eq!(after.amount, before.amount + 18);
}

#[test]
fn gauge_site_keeps_the_maximum_not_the_sum() {
    assert_eq!(WorkSite::QueueDepth.unit(), WorkUnit::Gauge);
    record_amount(WorkSite::QueueDepth, 40);
    record_amount(WorkSite::QueueDepth, 12);
    record_amount(WorkSite::QueueDepth, 31);
    let row = read(WorkSite::QueueDepth);
    assert_eq!(row.amount, 40, "gauge must hold the high-water mark");
    assert_eq!(row.calls, 3, "every gauge report still counts as a hit");
}

#[test]
fn digest_fold_is_order_independent_but_value_sensitive() {
    // Two disjoint gauge-free digest sites so the two folds cannot
    // interfere with each other.
    record_digest(WorkSite::ComponentMetaDigest, 3);
    record_digest(WorkSite::ComponentMetaDigest, 9);
    record_digest(WorkSite::ComponentMetaDigest, 27);
    let forward = read(WorkSite::ComponentMetaDigest).digest;

    record_digest(WorkSite::CompiledOutputDigest, 27);
    record_digest(WorkSite::CompiledOutputDigest, 3);
    record_digest(WorkSite::CompiledOutputDigest, 9);
    let reversed = read(WorkSite::CompiledOutputDigest).digest;

    assert_eq!(
        forward, reversed,
        "the same multiset must fold to the same digest in any order"
    );
    assert_ne!(forward, 0, "a non-trivial multiset must not fold to zero");
    assert_ne!(
        forward,
        mix64(3).wrapping_add(mix64(9)),
        "a DIFFERENT multiset must not collide with this one"
    );
}

#[test]
fn scope_guard_charges_time_and_a_call_to_its_site() {
    let before = read(WorkSite::FlowSliceCompute);
    {
        crate::attribute_scope!(FlowSliceCompute);
        // Enough work that a monotonic clock must move.
        let mut acc = 0u64;
        for i in 0..50_000u64 {
            acc = acc.wrapping_add(i * i);
        }
        std::hint::black_box(acc);
    }
    let after = read(WorkSite::FlowSliceCompute);
    assert_eq!(after.calls, before.calls + 1);
    assert!(
        after.nanos > before.nanos,
        "a timed region must advance the site's nanosecond column"
    );
}

#[test]
fn snapshot_omits_untouched_sites_and_reports_schema_columns() {
    record_call(WorkSite::ArtifactPinRelease);
    let rows = snapshot();
    let row = rows
        .iter()
        .find(|row| row.site == WorkSite::ArtifactPinRelease)
        .expect("a site with observations must appear in the snapshot");
    assert_eq!(row.id(), "session.artifact_pin_release");
    assert_eq!(row.domain().id(), "pinning");
    assert_eq!(row.unit().id(), "calls");
    assert!(
        rows.iter().all(|row| !row.is_empty()),
        "snapshot() must not emit rows with no observations"
    );
}

#[test]
fn macros_route_to_the_named_site() {
    let before_n = read(WorkSite::SourceTextCopy);
    crate::attribute_n!(SourceTextCopy, 64usize);
    let after_n = read(WorkSite::SourceTextCopy);
    assert_eq!(after_n.calls, before_n.calls + 1);
    assert_eq!(after_n.amount, before_n.amount + 64);

    let before_max = read(WorkSite::StoreRetainedBytes);
    crate::attribute_max!(StoreRetainedBytes, before_max.amount + 5);
    assert_eq!(
        read(WorkSite::StoreRetainedBytes).amount,
        before_max.amount + 5
    );

    let before_call = read(WorkSite::WasmBoundaryCall).calls;
    crate::attribute!(WasmBoundaryCall);
    assert_eq!(read(WorkSite::WasmBoundaryCall).calls, before_call + 1);
}
