//! TLS-observer propagation through `VerterHost::audit_mcp_tool_call`
//! (Wave 3 Slice 3.F follow-up).
//!
//! `audit_mcp_tool_call` wraps a synthetic tool-callback closure with
//! the standard registration / `RequestContextGuard` / finalize
//! lifecycle. This test drives the wrapper through the
//! [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
//! harness and asserts:
//!
//! - **Positive** (`install_audit=true`, `audit_enabled=true` on the
//!   host): the closure observes `current_observer() == Some(_)`
//!   mid-flight (the wrapper installs its `RequestContextGuard`
//!   BEFORE invoking the closure), the wrapper publishes a record
//!   with `RequestKind::Mcp`, and the harness's calling-thread
//!   observation is `Some` (the harness's outer guard remains
//!   visible after the wrapper's nested guard drops on return).
//! - **Negative** (`install_audit=false`, `audit_enabled=false`):
//!   the wrapper's registration takes the `Noop` arm and skips
//!   guard install, the closure observes `current_observer() ==
//!   None`, and no record enters the records store.
//!
//! Discrimination contract:
//! - Pre-change tree (no harness driver pinned to
//!   `audit_mcp_tool_call`): `mcp_audit_e2e.rs` verifies record
//!   contents (parent_request_id correlation) but does not probe
//!   the substrate's TLS slot inside the closure.
//! - Wired correctly: the closure's mid-flight
//!   `current_observer()` probe returns `Some(_)` in the
//!   audit-enabled case and `None` in the audit-disabled case.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use std::convert::Infallible;
use verter_audit::{RequestKind, RequestKindPayload};

use verter_session::host_mcp_audit::McpToolSuccess;
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{HostConfig, VerterHost};

#[test]
fn audit_mcp_tool_call_propagates_observer_into_tool_closure() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: false,
        ..HostConfig::default()
    }));

    let mut record_kind: Option<RequestKind> = None;
    let mut closure_saw_observer: bool = false;
    let report = assert_observer_reaches(true, || {
        // Drive the production audited entry-point. The wrapper
        // constructs the registration BEFORE installing
        // `RequestContextGuard` and invokes the closure under that
        // guard — so inside the closure body, the substrate observer
        // must be visible.
        let (outcome, record) = host
            .audit_mcp_tool_call::<bool, Infallible, _>("tls_probe_tool", "/probe.vue", 0, |_h| {
                let saw = verter_audit::current_observer().is_some();
                Ok(McpToolSuccess {
                    value: saw,
                    result_size_bytes: 0,
                })
            })
            .into_parts();
        closure_saw_observer = outcome.expect("infallible tool body");
        {
            let rec = &record;
            record_kind = Some(rec.kind.clone());
            // Sanity on the payload — the closure measured nothing
            // beyond the observer-visibility flag, but the wrapper
            // populates tool_name and request_id.
            match &rec.kind_payload {
                RequestKindPayload::Mcp(payload) => {
                    assert_eq!(
                        payload.tool_name, "tls_probe_tool",
                        "tool_name must round-trip from the wrapper's input",
                    );
                }
                other => panic!("expected Mcp payload, got {other:?}"),
            }
            assert_ne!(
                rec.request_id, 0,
                "audit_mcp_tool_call must stamp a non-zero request id",
            );
        }
    });

    assert!(
        closure_saw_observer,
        "synthetic tool closure must see `current_observer().is_some()` — \
         a regression that drops the TLS plumbing in `audit_mcp_tool_call`'s \
         `RequestContextGuard::install` would leave the observer absent inside \
         the closure and this assertion would fail. report = {report:?}",
    );
    assert!(
        matches!(record_kind, Some(RequestKind::Mcp { .. })),
        "wrapper must publish an Mcp record when audit is enabled; \
         a tautological regression that publishes through the Noop arm would \
         still produce a record but the discriminator above (closure_saw_observer) \
         would catch the deeper TLS regression. record_kind = {record_kind:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness's outer RequestContextGuard remains visible on the calling \
         thread after the wrapper's nested guard drops on return: {report:?}",
    );

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap.records_store_size, 1,
        "audit-enabled wrapper must publish exactly one record",
    );
    assert_eq!(
        snap.active_request_count, 0,
        "post-state: active-request registry must be drained by finalize",
    );
}

#[test]
fn audit_mcp_tool_call_observer_absent_when_audit_disabled() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    assert!(!host.config().audit_enabled);

    let mut closure_saw_observer: bool = true;
    let mut record_capture_state: Option<verter_audit::AuditCaptureState> = None;
    let report = assert_observer_reaches(false, || {
        // With audit disabled the registration takes the `Noop` arm,
        // no `RequestContextGuard` is installed, and the closure
        // must observe no substrate observer.
        let (outcome, record) = host
            .audit_mcp_tool_call::<bool, Infallible, _>("tls_probe_tool", "/probe.vue", 0, |_h| {
                let saw = verter_audit::current_observer().is_some();
                Ok(McpToolSuccess {
                    value: saw,
                    result_size_bytes: 0,
                })
            })
            .into_parts();
        closure_saw_observer = outcome.expect("infallible tool body");
        record_capture_state = Some(record.capture_state);
    });

    assert!(
        !closure_saw_observer,
        "with audit disabled, the wrapper's Noop registration must NOT install \
         a `RequestContextGuard`, and the closure must observe \
         `current_observer() == None`. A regression that always installed the \
         guard would surface here as a stray `Some`. report = {report:?}",
    );
    assert_eq!(
        record_capture_state,
        Some(verter_audit::AuditCaptureState::AuditDisabled),
        "audit_enabled=false ⇒ wrapper must NOT publish a stored record; the carrier \
         still returns a record but it is marked AuditDisabled — a regression that \
         published through an active arm would surface here",
    );
    assert!(
        !report.observer_seen_on_calling_thread,
        "harness installed no outer guard and the wrapper's Noop arm installs \
         no guard either; calling thread must see no observer: {report:?}",
    );

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(snap.records_store_size, 0);
    assert_eq!(snap.active_request_count, 0);
}
