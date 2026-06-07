//! `RequestAuditRecord::waits` is `None` when `audit_timing_capture`
//! is off, regardless of contention.
//!
//! Drives the same 16-concurrent-request load as
//! `wait_audit_lock_contention_observable` but with the flag OFF.
//! Every record's `waits` field must be `None`. This proves the
//! flag-gate is enforced at the envelope level and the producer
//! short-circuit is wired correctly: producers may NOT silently
//! populate `waits` when the host's flag is off, even if the load
//! produces high contention.
//!
//! Discriminator: an implementation that ignored the flag would
//! populate `waits = Some(WaitAudit { ... })` because the locks ARE
//! still acquired (the lock-acquisition counter aggregate is
//! unconditional) — the only thing the flag controls is whether the
//! envelope surfaces them. A tree without the field also fails
//! compilation against this test.
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
fn waits_is_none_when_audit_timing_capture_is_off() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/widget.vue".into(), Arc::from(ENTRY_SFC));
    workspace.inject_file("/types.ts".into(), Arc::from(TYPES_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            // FLAG OFF — the discriminator.
            audit_timing_capture: false,
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

    // Every record MUST have `waits == None`. Same load that produced
    // observable contention in `wait_audit_lock_contention_observable`
    // must surface as `None` here because the flag is off. An
    // implementation that surfaced `Some(WaitAudit { lock_wait_ns: 0 })`
    // on the off-path would also fail this strict-`None` assertion.
    for r in &records {
        assert!(
            r.waits.is_none(),
            "request {}: waits must be None when audit_timing_capture is off; \
             got {:?}",
            r.request_id,
            r.waits,
        );
    }
}
