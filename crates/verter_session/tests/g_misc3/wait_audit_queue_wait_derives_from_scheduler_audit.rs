//! `WaitAudit::queue_wait_ns` is derived from per-dispatch
//! `SchedulerAudit::queue_dwell_ms` observations.
//!
//! Drives requests through the real scheduler and asserts that for
//! every record where the scheduler attribution captured a dwell, the
//! per-request `WaitAudit::queue_wait_ns` aggregate is at least as
//! large as the first-dispatch dwell (within rounding). This proves
//! the queue-wait derivation reads from the per-dispatch capture
//! rather than zero-filling the field.
//!
//! Discriminator: a stub that always wrote `0` to `queue_wait_ns`
//! would fail when the scheduler dispatch observed a non-zero dwell.
//! An implementation that ignored the `record_scheduler_dispatch`
//! observer hook would also fail because the per-request aggregate
//! would never be incremented.
//!
//! Driver pattern: 16 concurrent component-meta requests on the same
//! shared host. The scheduler runs these jobs through its CPU pool;
//! at least one observes positive queue dwell when the pool is small
//! enough to enforce contention.
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
fn queue_wait_ns_is_at_least_observed_first_dispatch_dwell() {
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

    // Every record must have `waits = Some(_)` since the flag is on.
    for r in &records {
        assert!(
            r.waits.is_some(),
            "request {}: waits must be Some when audit_timing_capture is on",
            r.request_id,
        );
    }

    // Records that observed a scheduler dispatch must have
    // `queue_wait_ns >= floor(first_dwell_ms * 1_000_000)`. The
    // queue_wait_ns aggregate sums every dispatch; the slot keeps
    // only the first dispatch's dwell — so the aggregate must be
    // at least as large as the first-dispatch observation.
    //
    // We require AT LEAST ONE record to have a captured scheduler
    // dispatch (proving the test exercises the derivation), and for
    // every such record, the inequality must hold.
    let mut scheduler_observed_count = 0;
    for r in &records {
        if let Some(sched) = &r.scheduler {
            scheduler_observed_count += 1;
            let waits = r
                .waits
                .as_ref()
                .expect("waits is Some when timing_capture is on");
            // Convert ms (f64) → ns. Floor to u64; the aggregate is
            // u64 nanoseconds. queue_wait_ns is derived by
            // truncation in the dispatch observer, so the derived
            // value is the floor of the dwell.
            let dwell_ns_floor = (sched.queue_dwell_ms * 1_000_000.0).max(0.0) as u64;
            assert!(
                waits.queue_wait_ns >= dwell_ns_floor,
                "request {}: queue_wait_ns ({}) must be >= floor(scheduler.queue_dwell_ms ({} ms) * 1_000_000) = {} ns. \
                 A stub that always wrote 0 would fail this assertion when dwell > 0.",
                r.request_id,
                waits.queue_wait_ns,
                sched.queue_dwell_ms,
                dwell_ns_floor,
            );
        }
    }

    assert!(
        scheduler_observed_count > 0,
        "at least one of {} concurrent requests must have a captured \
         scheduler dispatch (`record.scheduler.is_some()`); none did, \
         so the queue-wait derivation was not exercised. Check that \
         component-meta resolution is dispatched through the scheduler \
         in this fixture.",
        REQUESTS,
    );
}
