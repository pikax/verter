//! Discriminating tests for the `Active` arm of
//! [`verter_session::host_audit_runtime::AuditRequestRegistration`].
//!
//! The first test probes the registration's lifecycle with a
//! synthetic [`verter_session::request_context::RequestContext`] so
//! the state machine itself stays unit-testable. The second test
//! drives a real component-meta request through
//! [`verter_session::VerterHost::get_component_meta_with_resolution`]
//! and discriminates against the pre-change tree where the public
//! audited entry-point did not wire `AuditRequestRegistration::new`.

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
        scheduler: None,
        files: Vec::new(),
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
        .resolve_component_meta("/x.vue")
        .expect("smoke: full audited request still works after the registration probe");
}

/// **Production-path discriminator.** Drives a real component-meta
/// request through
/// [`verter_session::VerterHost::get_component_meta_with_resolution`]
/// and asserts that the host's records store carries the produced
/// record after the request finalises (which means the public audited
/// entry-point wired `AuditRequestRegistration::new` and `finalize`).
///
/// Pre-change tree (no production wiring): the entry-point never
/// constructed an `AuditRequestRegistration`, so `finalize_active_request`
/// was never called. The records store would be empty after the call
/// and `take_record(request_id)` would return `None`. This test would
/// therefore fail against the pre-change tree.
#[test]
fn production_audited_entry_point_populates_active_registry_and_records_store() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..verter_session::HostConfig::default()
        },
    ));

    let canonical = "/Production.vue";
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(SFC),
        file_kind: verter_session::FileKind::from_path(canonical),
        aliases: Vec::new(),
    });

    // Pre-state: the host has no active requests and no records.
    let snap_before = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_before.active_request_count, 0,
        "pre-state: registry must be empty before any request runs"
    );
    assert_eq!(
        snap_before.records_store_size, 0,
        "pre-state: records store must be empty before any request runs"
    );

    // Drive a real request through the public audited entry-point.
    let (analysis, resolution) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component-meta resolution must succeed for the fixture");
    let request_id = resolution.request_id;
    assert_ne!(request_id, 0, "request_id must be stamped non-zero");
    let _ = analysis;

    // Post-state: the active-request registry has been drained
    // (registration finalised), and the records store carries one
    // record for our request_id.
    let snap_after = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap_after.active_request_count, 0,
        "post-state: registry must be empty after the request finalises (registration drained \
         the active-request slot via `finalize_active_request`)"
    );
    assert!(
        !snap_after.contains_active_request(request_id),
        "post-state: the request_id must not appear in the active-request registry"
    );
    assert_eq!(
        snap_after.records_store_size, 1,
        "post-state: the records store must hold the one record produced by the request. \
         Pre-change tree (no `AuditRequestRegistration` wiring in the entry-point) would \
         report 0 here because the registration was never created and \
         `finalize_active_request` was never called."
    );

    // Drain the record. It must be the one produced for our
    // request_id and reference the canonical we drove.
    let taken = host
        .host_audit_runtime()
        .take_record(request_id)
        .expect("records store must hold the record finalised by the registration");
    assert_eq!(taken.request_id, request_id);
    assert_eq!(taken.canonical_id, canonical);
    assert_eq!(taken.kind, RequestKind::ComponentMeta);
}
