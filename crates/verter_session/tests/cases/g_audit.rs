//! Consolidated integration-test group `audit`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_audit/audit_byte_budget.rs"]
mod audit_byte_budget;
#[path = "g_audit/audit_caps_wiring.rs"]
mod audit_caps_wiring;
#[path = "g_audit/audit_docs.rs"]
mod audit_docs;
#[path = "g_audit/audit_dropped_registration_emits_no_record.rs"]
mod audit_dropped_registration_emits_no_record;
#[path = "g_audit/audit_event_shape.rs"]
mod audit_event_shape;
#[path = "g_audit/audit_helper_envelope_e2e.rs"]
mod audit_helper_envelope_e2e;
#[path = "g_audit/audit_observer_tls_propagation.rs"]
mod audit_observer_tls_propagation;
#[path = "g_audit/audit_records_per_host_isolated.rs"]
mod audit_records_per_host_isolated;
#[path = "g_audit/audit_request_registration_active_variant_inserts_and_finalizes.rs"]
mod audit_request_registration_active_variant_inserts_and_finalizes;
#[path = "g_audit/audit_request_registration_survives_worker_tls_churn.rs"]
mod audit_request_registration_survives_worker_tls_churn;
#[path = "g_audit/audit_synthetic_fixtures.rs"]
mod audit_synthetic_fixtures;
