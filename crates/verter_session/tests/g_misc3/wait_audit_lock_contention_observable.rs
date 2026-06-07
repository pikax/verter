//! `WaitAudit::lock_wait_ns` records observable contention under load.
//!
//! Drives 16 concurrent `resolve_component_meta` requests on the SAME
//! canonical against the SAME shared host. Every cold-resolver run on
//! that canonical hits the family-map / node-arena shard mutexes; with
//! `audit_timing_capture = true`, each acquisition's wait duration
//! flows into the per-request `WaitAudit::lock_wait_ns` aggregate.
//!
//! Discriminator: without the `RequestAuditRecord::waits` field, or
//! when it is always `None`, this fails. A naive stub that always wrote
//! `0` would also fail the strictly-positive assertion. The single
//! mutation point (`record_*_lock_acquisition` helpers in
//! `host_manage.rs`) is exercised across both arena.rs and
//! semantic_query_memo/mod.rs production sites under contention.
//!
//! Note: at least ONE record must observe a non-zero wait. We do NOT
//! require every record to do so because shard distribution and
//! lock-fairness mean uncontended fast-path acquisitions are common.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const REQUESTS: usize = 16;

const ENTRY_SFC: &str = r#"<script setup lang="ts">
import type { Pet, Owner, Litter } from './types';
defineProps<{ pet: Pet; owner: Owner; litter: Litter }>();
</script>
<template><div>{{ pet }} {{ owner }} {{ litter }}</div></template>
"#;

const TYPES_TS: &str = r#"
export interface Pet { name: string; age: number; tags: string[] }
export interface Owner { name: string; pets: Pet[]; address: string }
export interface Litter { mother: Pet; pups: Pet[]; born: string }
"#;

#[test]
fn at_least_one_concurrent_request_observes_non_zero_lock_wait_ns() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/widget.vue".into(), Arc::from(ENTRY_SFC));
    workspace.inject_file("/types.ts".into(), Arc::from(TYPES_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_timing_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ));

    let handles: Vec<_> = (0..REQUESTS)
        .map(|_| {
            let host = Arc::clone(&host);
            thread::spawn(move || {
                let (_, _, record) = AuditedRequest::builder()
                    .attach_to(host)
                    .resolve_component_meta("/widget.vue")
                    .expect("audited resolve should succeed");
                record
            })
        })
        .collect();

    let records: Vec<verter_audit::RequestAuditRecord> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Every record's waits MUST be Some(...) — flag is on.
    for r in &records {
        assert!(
            r.waits.is_some(),
            "request {}: waits must be Some when audit_timing_capture is on; \
             the field must exist and not be always None",
            r.request_id,
        );
    }

    // At LEAST ONE record must have observed lock acquisitions —
    // the cold-resolver winner hits at least one family-map /
    // node-arena shard. Warm-cache joiners may have zero acquisitions
    // because the warm path bypasses the shard mutexes entirely.
    let max_acquisitions = records
        .iter()
        .filter_map(|r| r.waits.as_ref())
        .map(|w| w.lock_acquisitions)
        .max()
        .unwrap_or(0);
    assert!(
        max_acquisitions > 0,
        "at least one of {} concurrent requests must observe a positive \
         lock_acquisitions count (cold-resolver winner hits the shard \
         mutexes), got max = {}",
        REQUESTS,
        max_acquisitions,
    );

    // At LEAST ONE record must observe a strictly-positive lock_wait_ns.
    // With 16 concurrent requests on the same canonical hammering the
    // same shard mutexes, contention is guaranteed to appear at least
    // once. A pre-change tree (no waits field, or always None) would
    // fail at the `is_some()` check above; a stub that wrote 0 would
    // fail here.
    let max_wait_ns = records
        .iter()
        .filter_map(|r| r.waits.as_ref())
        .map(|w| w.lock_wait_ns)
        .max()
        .unwrap_or(0);

    assert!(
        max_wait_ns > 0,
        "at least one of {} concurrent requests must observe a strictly \
         positive lock_wait_ns; max observed = {} ns. \
         A stub that always wrote 0 would also fail this assertion.",
        REQUESTS,
        max_wait_ns,
    );
}
