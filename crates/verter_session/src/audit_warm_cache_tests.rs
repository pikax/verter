//! audit warm-cache
//! short-circuit tests.
//!
//! Validates that `VerterHost::get_component_meta_with_resolution` consults
//! the `ComponentMetaResultDb` on warm replays and synthesizes a
//! `RequestAuditRecord { from_cache: true, total_ms: 0 }` for audit consumers.
//!
//! These tests are the FAIL-FIRST contract for the cache-hit flow:
//!   - First call: cold; produces a record with `from_cache = false`.
//!   - Second call: warm; produces a record with `from_cache = true` and
//!     zeroed timing scalars.
//!   - After dependency change: cache miss, `from_cache = false` again.

use std::sync::Arc;

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

fn make_audit_enabled_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn upsert_basic_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props {
  message: string;
  level: number;
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
}

#[test]
fn audit_warm_path_first_call_is_cold_and_publishes_record() {
    let project = make_audit_enabled_project();
    upsert_basic_owner(&project);

    let host = project.host();
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("first call must succeed");

    let record = host
        .take_audit_record(resolution.request_id)
        .expect("audit record must be published for the first call");

    assert!(
        !record.from_cache,
        "first call is a cold cold-resolver run; from_cache must be false"
    );
}

#[test]
fn audit_warm_path_second_call_short_circuits_with_from_cache_true() {
    let project = make_audit_enabled_project();
    upsert_basic_owner(&project);

    let host = project.host();

    // First call: cold; warms the cache.
    let (_, first) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("first call must succeed");
    let _first_record = host
        .take_audit_record(first.request_id)
        .expect("first audit record must publish");

    // Second call: warm. The cache-hit short-circuit synthesizes a
    // from_cache audit record.
    let (_, second) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("second call must succeed");
    let second_record = host
        .take_audit_record(second.request_id)
        .expect("second audit record must publish (synthesized from_cache)");

    assert!(
        second_record.from_cache,
        "second call is a warm cache hit; from_cache must be true"
    );
    assert_eq!(
        second_record.timings.total_ms, 0.0,
        "from_cache record carries zero timing scalars"
    );
    assert_ne!(
        first.request_id, second.request_id,
        "request_ids must be unique per call"
    );
    assert_eq!(
        second_record.request_id, second.request_id,
        "synthesized record's request_id matches the resolution's"
    );
}

#[test]
fn audit_warm_path_dep_change_invalidates_cache() {
    let project = make_audit_enabled_project();
    upsert_basic_owner(&project);

    let host = project.host();

    // First call: cold.
    let (_, first) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("first call must succeed");
    let _first_record = host.take_audit_record(first.request_id);

    // Mutate the imported types file. The cache entry's dep_signature
    // becomes stale and is rejected at lookup.
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props {
  message: string;
  level: number;
  extra: boolean;
}"#,
        )
        .unwrap();

    // Second call: dep_signature mismatch → falls through to cold resolver.
    let (_, second) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("second call after dep change must succeed");
    let second_record = host
        .take_audit_record(second.request_id)
        .expect("second audit record must publish (cold path)");

    assert!(
        !second_record.from_cache,
        "after dep change the cache entry is stale; from_cache must be false"
    );
}

/// Like [`make_audit_enabled_project`] but with `footprint_capture`
/// on, so audited requests mine a `RequestFootprintAudit`.
fn make_footprint_capturing_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

#[test]
fn audit_warm_path_finalizes_footprint_some_with_per_request_isolation() {
    // Discriminating test for the warm-cache footprint gap. A warm
    // cache hit (`try_with_resolution_cache_hit`) must finalise a
    // footprint through the SAME path the cold resolver uses — the
    // synthesized record's `footprint` must be `Some(..)`, not `None`.
    //
    // Discrimination contract:
    // - Pre-fix tree: the warm-path synthesized record hardcoded
    //   `footprint: None`, so `warm_record.footprint.is_some()` FAILS
    //   (and the 16-thread isolation test panics on
    //   `record.footprint.expect(...)`).
    // - Post-fix tree: the warm path drains THIS request's accumulator
    //   and mines a footprint → `Some(..)` (typically empty, because a
    //   warm hit does little/no VFS work) → PASSES.
    let project = make_footprint_capturing_project();
    upsert_basic_owner(&project);

    let host = project.host();

    // First call: cold; warms the cache. Drain its record so the
    // store doesn't hand the cold record back by mistake later.
    let (_, first) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("first (cold) call must succeed");
    let first_record = host
        .take_audit_record(first.request_id)
        .expect("first audit record must publish");
    assert!(
        !first_record.from_cache,
        "first call must be cold (sanity: confirms the second is the warm hit)"
    );

    // Second call: warm cache hit.
    let (_, second) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("second (warm) call must succeed");
    let warm_record = host
        .take_audit_record(second.request_id)
        .expect("warm audit record must publish");

    assert!(
        warm_record.from_cache,
        "second call must be the warm cache hit (from_cache == true); got {}",
        warm_record.from_cache,
    );
    // THE discriminating assertion: the warm record carries a footprint.
    // Pre-fix the warm path synthesized `footprint: None`, leaving audited
    // warm-cache requests with no footprint coverage at all.
    assert!(
        warm_record.footprint.is_some(),
        "warm-cache audited request must finalise a footprint (Some), not None",
    );

    // A warm cache hit performs ZERO VFS READS, so the footprint's read lane
    // must be EMPTY. Asserting emptiness — not merely per-read attribution —
    // is what makes a regression that leaks the cold warm-up request's reads
    // into the warm footprint fail here: an attribution-only loop is vacuous
    // when the read lane is empty. (The record's `files` lane is the
    // STRUCTURAL direct-import list, legitimately non-empty for a warm hit,
    // so it is not a read-leak vector and is intentionally not asserted empty.)
    let footprint = warm_record
        .footprint
        .as_ref()
        .expect("footprint asserted Some above");
    assert!(
        footprint.vfs_reads.is_empty(),
        "warm-cache hit must perform zero VFS reads; got {}: {:?}",
        footprint.vfs_reads.len(),
        footprint.vfs_reads,
    );
    // Defense-in-depth: should a future warm path legitimately perform
    // reads, every read must still attribute to THIS request, never the
    // cold warm-up request (`first.request_id`), so a leak fails here too.
    for r in &footprint.vfs_reads {
        assert_eq!(
            r.request_id, second.request_id,
            "warm footprint must attribute only THIS request's reads ({}), \
             never the cold warm-up request's ({})",
            second.request_id, first.request_id,
        );
    }
}
