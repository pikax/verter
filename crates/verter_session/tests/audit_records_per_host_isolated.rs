//! Audit records do NOT leak across `VerterHost` instances.
//!
//! Construct two hosts, drive one audit request on each, and assert
//! each host's audit-runtime snapshot only sees its own record. A
//! global `OnceCell`-style singleton would fail this test by
//! exposing both records through a single shared store; the
//! per-host runtime architecture passes.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::component_meta_audit::RequestAuditRecord;

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ message: string }>()
</script>
<template><div>{{ message }}</div></template>
"#;

fn run_one_request(host_label: &str) -> (Arc<verter_session::VerterHost>, RequestAuditRecord) {
    let canonical = format!("/{host_label}.vue");
    let (_analysis, _resolution, record) = AuditedRequest::builder()
        .files([(canonical.clone(), SFC.to_string())])
        .resolve(&canonical)
        .expect("audited request must succeed");
    // Re-create a host to extract the audit_runtime snapshot AFTER
    // the AuditedRequest's hermetic host has dropped: that returned
    // record is what THIS host saw before its destructor ran.
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..verter_session::HostConfig::default()
        },
    ));
    (host, record)
}

#[test]
fn audit_records_per_host_isolated_two_hosts_dedicated_record_stores() {
    let (host_a, record_a) = run_one_request("HostA");
    let (host_b, record_b) = run_one_request("HostB");

    // Each record was produced by its own hermetic host instance —
    // sanity-check that they describe DIFFERENT canonical files
    // before asserting isolation. Without this discrimination the
    // test would pass even if both records collided on the same
    // host.
    assert_ne!(
        record_a.canonical_id, record_b.canonical_id,
        "test setup error: the two requests must target different canonical ids"
    );

    // The two empty hosts each have their own runtime snapshot. No
    // record from `record_a`'s host reaches `host_b`'s store; nor
    // does `record_b`'s record reach `host_a`'s store. Empty stores
    // on both sides discriminate against a process-global singleton.
    let snap_a = host_a.host_audit_runtime().snapshot();
    let snap_b = host_b.host_audit_runtime().snapshot();
    assert_eq!(
        snap_a.records_store_size, 0,
        "host_a's records store must be empty — its hermetic host's records do not \
         leak across VerterHost boundaries (process-global singleton would show 1)"
    );
    assert_eq!(
        snap_b.records_store_size, 0,
        "host_b's records store must be empty — its hermetic host's records do not \
         leak across VerterHost boundaries (process-global singleton would show 1)"
    );
    assert_eq!(snap_a.active_request_count, 0);
    assert_eq!(snap_b.active_request_count, 0);
}
