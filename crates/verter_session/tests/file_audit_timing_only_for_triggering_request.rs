//! Read-once invariant: the request that TRIGGERED the I/O / parse
//! gets `Some(*_ms)` with `triggered_by_this_request = true`.
//! Subsequent warm-cache requests for the same canonical get
//! `*_ms = None`, `triggered_by_this_request = false`,
//! `cache_hit = true`.
//!
//! Discriminating: a "always populate parse_ms" implementation (the
//! pre-fix v3 design) would set `Some(0.0)` on the warm request — which
//! would FAIL the cold-vs-warm assertions below.
//!
//! Fixture pattern (matches `audited_request_e2e.rs`): inject
//! directly into the workspace, NOT via `host.upsert`, so the parse
//! happens INSIDE the audited request window.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const ENTRY_SFC: &str = r#"<script setup lang="ts">
import type { Pet } from './types';
defineProps<{ pet: Pet }>();
</script>
<template><div>{{ pet }}</div></template>
"#;

const TYPES_TS: &str = r#"
export interface Pet { name: string; age: number }
"#;

fn build_host(timing_on: bool) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/entry.vue".into(), Arc::from(ENTRY_SFC));
    workspace.inject_file("/types.ts".into(), Arc::from(TYPES_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_timing_capture: timing_on,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

#[test]
fn cold_request_records_triggered_warm_request_does_not() {
    let host = build_host(true);

    let (_, _, record1) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/entry.vue")
        .expect("first audited resolve succeeds (cold)");

    // Cold request must have at least one triggered FileAudit entry —
    // the executor's source stage fires for the entry and any
    // resolved deps, and the read-once invariant marks them as
    // `triggered_by_this_request = true`.
    let any_triggered = record1.files.iter().any(|f| f.triggered_by_this_request);
    assert!(
        any_triggered,
        "cold request must have at least one triggered_by_this_request entry; \
         got files: {:?}",
        record1.files,
    );

    let (_, _, record2) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/entry.vue")
        .expect("second audited resolve succeeds (warm)");

    // Warm request: every FileAudit entry served from cache must NOT
    // be marked triggered, and its `*_ms` fields must all be `None`
    // (NOT `Some(0.0)` — that would be the pre-fix "always populate"
    // bug). Cache-hit entries also have `cache_hit = true`.
    for f in &record2.files {
        if !f.triggered_by_this_request {
            assert!(
                f.read_ms.is_none(),
                "warm request, untriggered entry: read_ms must be None; got {:?}",
                f,
            );
            assert!(
                f.parse_ms.is_none(),
                "warm request, untriggered entry: parse_ms must be None (NOT Some(0.0)); got {:?}",
                f,
            );
            assert!(
                f.lower_ms.is_none(),
                "warm request, untriggered entry: lower_ms must be None; got {:?}",
                f,
            );
        }
    }

    // Specifically for the entry SFC on the warm path: the cache
    // serves it without re-parse → triggered = false, all *_ms = None,
    // cache_hit = true.
    if let Some(entry2) = record2
        .files
        .iter()
        .find(|f| f.canonical_id == "/entry.vue")
    {
        assert!(
            !entry2.triggered_by_this_request,
            "warm request: /entry.vue must not be marked triggered; got {:?}",
            entry2,
        );
        assert!(
            entry2.cache_hit,
            "warm request: /entry.vue must report cache_hit = true; got {:?}",
            entry2,
        );
        assert!(
            entry2.read_ms.is_none() && entry2.parse_ms.is_none() && entry2.lower_ms.is_none(),
            "warm cache hit: all *_ms must be None per read-once invariant; got {:?}",
            entry2,
        );
    }
}
