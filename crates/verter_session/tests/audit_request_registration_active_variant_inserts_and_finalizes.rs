//! Positive-path discriminating test for the `Active` arm of
//! [`verter_session::AuditRequestRegistration`].
//!
//! Pre-change (no `AuditRequestRegistration` substrate): no
//! active-request registry exists; the test cannot compile because
//! `host.host_audit_runtime().snapshot()` is absent.
//!
//! Post-change: install an audit-config filter that allows
//! `RequestKind::ComponentMeta`, construct a registration, observe
//! the request id present in the active map between `new` and
//! `finalize`, then observe the published record in the records
//! store after `finalize` and the registry empty.

use std::sync::Arc;

use verter_audit::{
    ComponentMetaPayload, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit,
};
use verter_session::audited_request::AuditedRequest;
use verter_session::host_audit_runtime::AuditRequestRegistration;
use verter_session::request_context::RequestContext;

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ message: string }>()
</script>
<template><div>{{ message }}</div></template>
"#;

#[test]
fn active_registration_appears_in_active_requests_until_finalize_then_publishes_record() {
    // Use the harness to spin up a host with audit_enabled; we
    // attach to it for the registration test rather than driving a
    // full request through the resolver.
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..verter_session::HostConfig::default()
        },
    ));

    // Construct a synthetic per-request context. Real
    // entry-points construct one via the cold resolver; for this
    // unit-level test the synthetic context is sufficient because
    // the registration only consumes request_id + kind.
    let ctx = RequestContext::new(7777, Arc::from("/probe.vue"), false, None);

    // Pre-state: empty.
    let snap_before = host.host_audit_runtime().snapshot();
    assert_eq!(snap_before.active_request_count, 0);
    assert!(!snap_before.contains_active_request(7777));

    // Register.
    let registration = AuditRequestRegistration::new(host.as_ref(), Arc::clone(&ctx));
    assert_eq!(
        registration.request_id(),
        Some(7777),
        "filter allows ComponentMeta kind: registration must be Active"
    );

    // Mid-state: active map contains 7777, records store empty.
    let snap_during = host.host_audit_runtime().snapshot();
    assert_eq!(snap_during.active_request_count, 1);
    assert!(snap_during.contains_active_request(7777));
    assert_eq!(snap_during.records_store_size, 0);

    // Finalize.
    let record = RequestAuditRecord {
        request_id: 7777,
        canonical_id: "/probe.vue".to_string(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
    };
    assert!(
        registration.finalize(record.clone()),
        "finalize on Active must return true on the first call"
    );

    // Post-state: record is in the store, active map is empty.
    let snap_after = host.host_audit_runtime().snapshot();
    assert_eq!(snap_after.active_request_count, 0);
    assert!(!snap_after.contains_active_request(7777));
    assert_eq!(snap_after.records_store_size, 1);
    let taken = host
        .host_audit_runtime()
        .take_record(7777)
        .expect("the finalised record must be retrievable from the records store");
    assert_eq!(taken.request_id, 7777);
    assert_eq!(taken.canonical_id, "/probe.vue");

    // Calling finalize again is idempotent.
    assert!(
        !registration.finalize(record),
        "finalize must be idempotent — second call returns false"
    );

    // Sanity: the audited-request harness still works after our
    // synthetic registration churn (regression smoke against
    // accidentally messing up TLS state or the records store).
    let _smoke = AuditedRequest::builder()
        .files([("/x.vue".to_string(), SFC.to_string())])
        .resolve("/x.vue")
        .expect("smoke: full audited request still works after the registration probe");
}
