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
