//! Audit records do NOT leak across `VerterHost` instances.
//!
//! Construct two hosts, drive one audit request on each, and assert
//! each host's audit-runtime snapshot only sees its own record.
//!
//! Discrimination contract: a process-singleton store would expose
//! BOTH hosts' records on EITHER host's snapshot — `host_a` would
//! observe `host_b`'s record (and vice versa). The per-host runtime
//! architecture publishes records into the host-owned
//! `AuditRecordsStore`, so each host's snapshot reports only its own
//! record. The cross-host `take_record(other_id)` probe further pins
//! down isolation: each host must NOT yield the other host's record
//! id through its records-store accessor.

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC_A: &str = r#"<script setup lang="ts">
defineProps<{ alpha: string }>()
</script>
<template><div>{{ alpha }}</div></template>
"#;

const SFC_B: &str = r#"<script setup lang="ts">
defineProps<{ beta: number }>()
</script>
<template><div>{{ beta }}</div></template>
"#;

/// Stand up a fresh `VerterHost` with audit + footprint capture and a
/// single canonical fixture, then drive a real component-meta request
/// through the public audited entry-point. Returns the host (so
/// callers can probe its `host_audit_runtime`) and the `request_id`
/// the entry-point stamped.
fn run_one_request(canonical: &str, source: &str) -> (Arc<VerterHost>, u64) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component-meta resolution must succeed for the fixture");
    (host, resolution.request_id)
}

#[test]
fn audit_records_per_host_isolated_two_hosts_dedicated_record_stores() {
    let (host_a, request_id_a) = run_one_request("/HostA.vue", SFC_A);
    let (host_b, request_id_b) = run_one_request("/HostB.vue", SFC_B);

    // Each host owns its own records store. host_a must hold
    // exactly the one record it produced; host_b must hold exactly
    // the one record IT produced. A process-singleton store would
    // expose 2 records on EITHER side (both `host_a`'s and
    // `host_b`'s record visible from each host's accessor).
    let snap_a = host_a.host_audit_runtime().snapshot();
    let snap_b = host_b.host_audit_runtime().snapshot();
    assert_eq!(
        snap_a.records_store_size, 1,
        "host_a must hold exactly its own record; pre-change (process-singleton) would show 2 \
         because host_b's record would leak into the shared store"
    );
    assert_eq!(
        snap_b.records_store_size, 1,
        "host_b must hold exactly its own record; pre-change (process-singleton) would show 2 \
         because host_a's record would leak into the shared store"
    );
    assert_eq!(
        snap_a.active_request_count, 0,
        "no in-flight requests remain on host_a"
    );
    assert_eq!(
        snap_b.active_request_count, 0,
        "no in-flight requests remain on host_b"
    );

    // Each host's per-host counter starts independently, so the
    // two request_ids may legitimately coincide; the discrimination
    // is on the *canonical_id* the record references, not the id.
    let record_a = host_a
        .host_audit_runtime()
        .take_record(request_id_a)
        .expect("host_a must yield its own record by id");
    let record_b = host_b
        .host_audit_runtime()
        .take_record(request_id_b)
        .expect("host_b must yield its own record by id");
    assert_eq!(record_a.request_id, request_id_a);
    assert_eq!(record_b.request_id, request_id_b);
    // Canonical-id discrimination: each host's record must
    // reference the canonical it ran for. A singleton would have
    // collapsed both records into one canonical (or held both
    // canonicals on both hosts).
    assert_eq!(
        record_a.canonical_id, "/HostA.vue",
        "host_a's record must reference /HostA.vue, not host_b's canonical"
    );
    assert_eq!(
        record_b.canonical_id, "/HostB.vue",
        "host_b's record must reference /HostB.vue, not host_a's canonical"
    );
    assert_ne!(
        record_a.canonical_id, record_b.canonical_id,
        "discrimination: each host's record must reference its own canonical"
    );
}
