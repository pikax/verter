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

/// Discriminating regression for F91 (warm-cache footprint synthesis)
/// + F93 (warm-cache file_audit_vec symmetry).
///
/// Before F91 the warm-cache short-circuit emitted
/// `RequestAuditRecord { footprint: None, ... }` even when the host
/// was configured with `footprint_capture = true`. Before F93 the
/// same short-circuit emitted `files: Vec::new()` regardless of
/// footprint capture. Both regressions were invisible in single-shot
/// runs because the only other warm-cache test
/// (`audit_warm_path_second_call_short_circuits_with_from_cache_true`)
/// constructed a host with `footprint_capture = false` and asserted
/// nothing about `footprint` / `files`.
///
/// This test pins both invariants on the warm path with
/// `footprint_capture = true` and `audit_timing_capture = true`,
/// and asserts the cold-record establishes the baseline (both fields
/// populated). Reverting either F91 or F93 fails this assertion.
#[test]
fn audit_warm_path_second_call_synthesizes_footprint_and_files_when_footprint_capture_enabled() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        audit_timing_capture: true,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    upsert_basic_owner(&project);

    let host = project.host();

    // First call: cold. The cold path runs `build_file_audit_vec`
    // before `mine_footprint`, so both fields must be populated.
    let (_, first) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("first (cold) call must succeed");
    let first_record = host
        .take_audit_record(first.request_id)
        .expect("first (cold) audit record must publish");

    assert!(
        !first_record.from_cache,
        "first call is cold; from_cache must be false"
    );
    assert!(
        first_record.footprint.is_some(),
        "cold record must carry Some(_) footprint when footprint_capture=true"
    );
    assert!(
        !first_record.files.is_empty(),
        "cold record must carry non-empty files when the entry has direct imports \
         (here /Owner.vue imports /types.ts)"
    );

    // Second call: warm. The synthesised record must mirror the cold
    // path's footprint+files symmetry — both fields populated, not
    // just one. A regression that re-introduces `footprint: None` OR
    // `files: Vec::new()` on the warm path fails here.
    let (_, second) = host
        .get_component_meta_with_resolution("/Owner.vue")
        .expect("second (warm) call must succeed");
    let second_record = host
        .take_audit_record(second.request_id)
        .expect("second (warm) audit record must publish (synthesised from_cache)");

    assert!(
        second_record.from_cache,
        "second call is a warm cache hit; from_cache must be true"
    );
    assert!(
        second_record.footprint.is_some(),
        "warm record must carry Some(_) footprint when footprint_capture=true \
         (F91: drain the per-request accumulator and mine on the synthesised path)"
    );
    assert!(
        !second_record.files.is_empty(),
        "warm record must carry non-empty files when the entry has direct imports \
         (F93: mirror cold-path build_file_audit_vec before mine_footprint consumes state)"
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
