//! Discriminating test for the defensive `Drop` semantics on
//! [`verter_session::host_audit_runtime::ActiveRegistration`].
//!
//! Contract: every `AuditRequestRegistration` either reaches
//! `finalize` exactly once OR `drop` exactly once, never neither.
//! Drop without finalize must clean up the active-request registry
//! BUT MUST NOT publish a record — the record's absence is itself
//! observable.
//!
//! Pre-change (drop publishes a partial record): the records store
//! shows a record after the registration drops without finalize ⇒
//! this test fails.
//! Post-change (drop is defensive cleanup only): the records store
//! is empty ⇒ this test passes.

use std::sync::Arc;

use verter_session::host_audit_runtime::AuditRequestRegistration;
use verter_session::request_context::RequestContext;

#[test]
fn dropping_active_registration_without_finalize_clears_active_map_and_emits_no_record() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..verter_session::HostConfig::default()
        },
    ));
    let ctx = RequestContext::new(8888, Arc::from("/dropped.vue"), false, None);

    {
        // Construct the registration in a scope so it drops at the
        // closing brace WITHOUT calling finalize.
        let registration = AuditRequestRegistration::new(host.as_ref(), Arc::clone(&ctx));
        assert_eq!(
            registration.request_id(),
            Some(8888),
            "filter allows ComponentMeta: registration must be Active before drop"
        );
        // Active map contains the entry mid-scope.
        let snap_during = host.host_audit_runtime().snapshot();
        assert!(
            snap_during.contains_active_request(8888),
            "active map must hold the registration's request id mid-scope"
        );
        // No finalize call — drop runs at the end of this block.
    }

    // Post-drop:
    // 1. The active map no longer contains 8888 (defensive Drop ran).
    // 2. The records store is empty (no record was published).
    let snap_after = host.host_audit_runtime().snapshot();
    assert!(
        !snap_after.contains_active_request(8888),
        "active map must be empty after drop — defensive cleanup ran"
    );
    assert_eq!(
        snap_after.active_request_count, 0,
        "no other active requests should be present"
    );
    assert_eq!(
        snap_after.records_store_size, 0,
        "drop without finalize must NOT publish a record — record absence is the \
         observable contract per the cancellation table. Pre-change behaviour \
         (drop emitting a partial record) would show 1 here and fail this test."
    );
    assert!(
        host.host_audit_runtime().take_record(8888).is_none(),
        "explicit take_record must return None for a dropped-without-finalize registration"
    );
}

#[test]
fn finalize_then_drop_does_not_publish_a_second_record() {
    // Lifecycle invariant: the Drop impl checks the `finalized`
    // flag and only cleans up if it's still false. Otherwise it
    // takes no action — finalize already removed the entry and
    // published the record.
    use verter_audit::{
        ComponentMetaPayload, RequestAuditRecord, RequestKind, RequestKindPayload,
        RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
    };

    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..verter_session::HostConfig::default()
        },
    ));
    let ctx = RequestContext::new(9999, Arc::from("/finalised_then_dropped.vue"), false, None);

    {
        let registration = AuditRequestRegistration::new(host.as_ref(), Arc::clone(&ctx));
        let record = RequestAuditRecord {
            request_id: 9999,
            canonical_id: "/finalised_then_dropped.vue".to_string(),
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
            registration.finalize(record),
            "finalize succeeds first time"
        );
        // Registration goes out of scope here; Drop runs.
    }

    // Exactly ONE record exists for 9999.
    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap.records_store_size, 1,
        "exactly one record published — Drop after finalize must NOT add a second"
    );
    assert!(
        host.host_audit_runtime().take_record(9999).is_some(),
        "the published record must be retrievable"
    );
    let snap_after_take = host.host_audit_runtime().snapshot();
    assert_eq!(snap_after_take.records_store_size, 0);
}
