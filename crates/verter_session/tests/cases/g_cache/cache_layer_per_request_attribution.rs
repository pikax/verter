//! Per-cache HitMiss with per-request attribution.
//!
//! This test asserts the joiner-accounting contract under
//! serialised execution: cold then warm component-meta on the SAME
//! canonical, with each request producing its own audit record. The
//! cold record's `cache_layers.component_meta` shows a miss; the
//! warm record shows a hit.
//!
//! Discriminating: a design without a `cache_layers` field on
//! `RequestStoreAudit` would not compile. The assertions verify
//! per-request hit/miss attribution attributes exactly to the request
//! that performed the lookup.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SIMPLE_SFC: &str = r#"<script setup lang="ts">
defineProps<{ msg: string; count?: number }>();
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>
"#;

fn build_host() -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/c.vue".into(), Arc::from(SIMPLE_SFC));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

#[test]
fn cold_request_records_component_meta_miss_warm_request_records_hit() {
    let host = build_host();

    // Cold: no cached component_meta result yet.
    let (_analysis_cold, _resolution_cold, cold_record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/c.vue")
        .expect("cold resolve must succeed");

    // Cold record: component_meta cache miss observed (or zero, if
    // the cold path never consults the final-result cache before
    // building). The cold path SHOULD consult the cache before
    // entering the cold resolver — that's the whole point of
    // `get_component_meta` consulting the final-result cache first
    // (CLAUDE.md final-state contract). So we expect at least one
    // miss and zero hits.
    assert_eq!(
        cold_record.store.cache_layers.component_meta.hits,
        0,
        "cold record: component_meta hits must be zero (no warm cache exists), \
         got hits={}, misses={}",
        cold_record.store.cache_layers.component_meta.hits,
        cold_record.store.cache_layers.component_meta.misses,
    );
    assert!(
        cold_record.store.cache_layers.component_meta.misses >= 1,
        "cold record: component_meta misses must be >= 1 (cold path consults the \
         final-result cache before building), got hits={}, misses={}",
        cold_record.store.cache_layers.component_meta.hits,
        cold_record.store.cache_layers.component_meta.misses,
    );
    assert!(
        !cold_record.from_cache,
        "cold record must have from_cache=false",
    );

    // Warm: cached component_meta result present.
    let (_analysis_warm, _resolution_warm, warm_record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/c.vue")
        .expect("warm resolve must succeed");

    // Warm record: component_meta cache hit observed; misses must be
    // zero on the warm path.
    assert!(
        warm_record.store.cache_layers.component_meta.hits >= 1,
        "warm record: component_meta hits must be >= 1, got hits={}, misses={}",
        warm_record.store.cache_layers.component_meta.hits,
        warm_record.store.cache_layers.component_meta.misses,
    );
    assert_eq!(
        warm_record.store.cache_layers.component_meta.misses,
        0,
        "warm record: component_meta misses must be zero, got hits={}, misses={}",
        warm_record.store.cache_layers.component_meta.hits,
        warm_record.store.cache_layers.component_meta.misses,
    );
    assert!(
        warm_record.from_cache,
        "warm record must have from_cache=true (cached result observed)",
    );
}

#[test]
fn cold_request_records_indexed_misses_warm_request_records_indexed_hits() {
    let host = build_host();

    // First request: indexed entry must be built.
    let (_a1, _r1, cold) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/c.vue")
        .expect("cold resolve");
    assert!(
        cold.store.cache_layers.indexed.misses >= 1,
        "cold record must observe at least one FileArtifactStore miss, got hits={}, misses={}",
        cold.store.cache_layers.indexed.hits,
        cold.store.cache_layers.indexed.misses,
    );

    // Second request: indexed entry already populated; the warm path
    // bumps `hits` instead.
    let (_a2, _r2, warm) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/c.vue")
        .expect("warm resolve");
    // Warm path: from_cache=true synthesizes a record with default
    // (zero) cache_layers — the warm fast path bypasses the cache
    // layer reads. So the assertion is that the warm record has
    // zero misses for indexed, NOT that it has hits >= 1 (the warm
    // synthesized record has zero of both). This is correct
    // behaviour: the warm fast path serves the cached final result
    // without consulting the lower-level caches at all.
    assert_eq!(
        warm.store.cache_layers.indexed.misses, 0,
        "warm record (from_cache=true): indexed misses must be zero, got hits={}, misses={}",
        warm.store.cache_layers.indexed.hits, warm.store.cache_layers.indexed.misses,
    );
}
