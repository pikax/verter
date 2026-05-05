//! Filtered-kind requests MUST be invisible to the host-owned
//! peak-RSS sampler.
//!
//! Plan §1.8.1: when `AuditRequestRegistration::new` returns the
//! `Noop` variant (the `consumer_filter` rejected the request kind),
//! the registration does NOT enter `active_requests` AND does NOT
//! publish a record. The sampler, which ticks over `active_requests`,
//! never observes the filtered request — its peak slot is never
//! touched, and `take_record(request_id)` returns `None`.
//!
//! Discrimination contract:
//! - The synthetic registration here has kind `ComponentMeta`. We
//!   override the audit-config consumer filter with a deny-all
//!   bitset, so the registration becomes `Noop` at construction
//!   time. The runtime snapshot must NEVER contain the request id;
//!   the records store must produce `None` for that id.
//! - Pre-change tree (registration unconditionally inserts): the
//!   snapshot would contain the id and a record would land — this
//!   test FAILS.
//! - Post-change tree (Noop arm short-circuits): both contracts
//!   hold — this test PASSES.
//!
//! Skipped on WASM (sampler isn't relevant; the flag-off contract
//! covers that).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_audit::{
    AuditConfig, AuditConsumerFilter, ComponentMetaPayload, RequestAuditRecord, RequestKind,
    RequestKindPayload, RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
};
use verter_session::host_audit_runtime::AuditRequestRegistration;
use verter_session::request_context::RequestContext;
use verter_session::{HostConfig, VerterHost};

#[test]
fn filtered_kind_registration_is_noop_and_invisible_to_sampler() {
    // Construct a host whose audit-config denies ALL kinds. The
    // `ComponentMeta` registration we build below must therefore
    // return the `Noop` variant.
    let mut host = VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true,
        footprint_capture: true,
        ..HostConfig::default()
    });
    // Replace the runtime's default audit-config with a deny-all
    // filter so the synthetic registration below is rejected.
    host.replace_host_audit_runtime_for_test(AuditConfig {
        consumer_filter: AuditConsumerFilter::deny_all(),
        ..AuditConfig::default()
    });
    let host = Arc::new(host);
    let request_id: u64 = 7373;
    let ctx = RequestContext::new(request_id, Arc::from("/filtered.vue"), false, None);

    // Pre-state: registry empty.
    let snap0 = host.host_audit_runtime().snapshot();
    assert!(!snap0.contains_active_request(request_id));

    // Construct the registration. With the deny-all filter, the
    // result MUST be the Noop variant.
    let registration = AuditRequestRegistration::new(host.as_ref(), Arc::clone(&ctx));
    assert_eq!(
        registration.request_id(),
        None,
        "deny-all filter must produce a Noop registration, not Active"
    );

    // Mid-state: the request id is NEVER in the registry.
    let snap_mid = host.host_audit_runtime().snapshot();
    assert!(
        !snap_mid.contains_active_request(request_id),
        "Noop registration must NOT enter the active-request registry. \
         Pre-change tree (unconditional insert) would show the id present \
         and fail this assertion."
    );

    // Calling `finalize` on Noop returns false unconditionally and
    // publishes nothing.
    let bogus_record = RequestAuditRecord {
        request_id,
        canonical_id: "/filtered.vue".to_string(),
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
        !registration.finalize(bogus_record),
        "finalize on Noop returns false; no record is stored"
    );

    // Post-state: still no entry, still no record.
    let snap_after = host.host_audit_runtime().snapshot();
    assert!(!snap_after.contains_active_request(request_id));
    assert_eq!(
        snap_after.records_store_size, 0,
        "Noop registration must NOT publish any record"
    );
    assert!(
        host.host_audit_runtime().take_record(request_id).is_none(),
        "take_record must return None for a filtered (Noop) request — \
         per §1.8.1's `Option<RequestAuditRecord>` rule, callers see \
         absence rather than a placeholder shell"
    );
}
