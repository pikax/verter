//! Discriminating tests for audit transparency and explicit cancellation.
//!
//! Drives a hover-style audited request whose body runs past the observational
//! audit SLO. Asserts that:
//!
//! * the successful result is still delivered,
//! * the record published to the records store carries no error,
//! * the active-request registry no longer contains the request id
//!   (the registration was finalised, not leaked),
//! * explicit cancellation can still finalize a registration with the marker.
//!
//! Every audited LSP handler enters
//! [`verter_lsp::audit_harness::run_with_audit`]. Audit collection must never
//! introduce a timeout that normal request execution does not have.

use std::sync::Arc;
use std::time::Duration;

use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{RequestKind, RequestKindPayload};
use verter_lsp::audit_harness;
use verter_session::{HostConfig, LspMethodTimeoutsConfig, VerterHost};

#[tokio::test]
async fn audit_slo_does_not_cancel_a_slow_successful_request() {
    // Tight hover SLO so the request exceeds it deterministically.
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        lsp_method_timeouts: LspMethodTimeoutsConfig {
            audit_supersede: verter_session::LspMethodBudgets {
                hover: Duration::from_millis(20),
                ..verter_session::LspMethodBudgets::audit_supersede_defaults()
            },
            ..LspMethodTimeoutsConfig::default()
        },
        ..HostConfig::default()
    }));

    let canonical = "/probe.vue".to_string();
    let position = tower_lsp_server::ls_types::Position {
        line: 0,
        character: 0,
    };

    // Pre-state: empty registry + records store.
    let pre = host.host_audit_runtime().snapshot();
    assert_eq!(pre.active_request_count, 0);
    assert_eq!(pre.records_store_size, 0);

    // Drive a body whose latency exceeds the hover audit SLO. The harness must
    // remain transparent and finalize the successful response normally.
    let result = audit_harness::run_with_audit::<u8, _, _>(
        &host,
        LspMethodTag::Hover,
        canonical.clone(),
        Some(position),
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(7u8)
        },
        |payload, value| {
            payload.response_size_bytes = u32::from(*value);
        },
    )
    .await;

    assert_eq!(result.expect("audit must not cancel the request"), 7);

    // The cancellation marker must be observable in the records
    // store. The active-request registry MUST be drained (the
    // registration finalised; no leak).
    let post = host.host_audit_runtime().snapshot();
    assert_eq!(
        post.active_request_count, 0,
        "the registration must not linger after the successful request"
    );
    assert_eq!(
        post.records_store_size, 1,
        "the successful audit record must be published"
    );

    // Drain the (single) record and verify the marker.
    let map = host.host_audit_runtime().audit_records_store();
    // We don't know the request_id without exposing an iter; the
    // store has exactly one entry, so we drain it via `take` after
    // peeking at the snapshot's recorded count.
    assert_eq!(map.len(), 1);
    // Locate the request_id by sampling the active-request slot
    // BEFORE finalise, OR by trying low ids until one hits — for a
    // brand-new host the first audited request gets id 1.
    let mut record = None;
    for candidate_id in 1..=64u64 {
        if let Some(r) = host.host_audit_runtime().take_record(candidate_id) {
            record = Some(r);
            break;
        }
    }
    let record = record.expect("the successful audit record must exist in the store");
    assert!(matches!(
        record.kind,
        RequestKind::Lsp {
            method: LspMethodTag::Hover
        }
    ));
    let payload = match record.kind_payload {
        RequestKindPayload::Lsp(p) => p,
        other => panic!("expected RequestKindPayload::Lsp, got {other:?}"),
    };
    assert_eq!(payload.error, None);
    assert_eq!(payload.response_size_bytes, 7);
    assert_eq!(payload.method, LspMethodTag::Hover);
}

/// Discriminator: `contains_active_request(id)` flips from `true`
/// (mid-flight) to `false` (post-finalize) for a cancellation. A host
/// without audit wiring never inserts into the active-request registry
/// to begin with, so the `mid.contains_active_request(id) == true`
/// assertion is what proves the wiring is live.
#[tokio::test]
async fn cancellation_drains_request_id_from_active_registry() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        lsp_method_timeouts: LspMethodTimeoutsConfig {
            audit_supersede: verter_session::LspMethodBudgets {
                hover: Duration::from_millis(20),
                ..verter_session::LspMethodBudgets::audit_supersede_defaults()
            },
            ..LspMethodTimeoutsConfig::default()
        },
        ..HostConfig::default()
    }));

    // Open the session manually so we have a first-class id; then
    // finalize with the cancellation marker the harness would
    // produce.
    let session = host.lsp_audit_begin(LspMethodTag::Hover, "/probe.vue");
    let request_id = session.request_id().expect("Active session has an id");
    let mid = host.host_audit_runtime().snapshot();
    assert!(
        mid.contains_active_request(request_id),
        "mid-flight: the registration MUST appear in the active-request registry"
    );
    let returned = session
        .finalize_cancelled()
        .expect("first-cancel publishes");
    assert!(matches!(
        returned.kind,
        RequestKind::Lsp {
            method: LspMethodTag::Hover
        }
    ));
    let payload = match returned.kind_payload {
        RequestKindPayload::Lsp(p) => p,
        other => panic!("expected RequestKindPayload::Lsp, got {other:?}"),
    };
    assert_eq!(payload.error.as_deref(), Some("cancelled"));

    // Post-finalize: id is drained.
    let post = host.host_audit_runtime().snapshot();
    assert!(
        !post.contains_active_request(request_id),
        "post-cancel: the registration MUST be drained from the registry"
    );
    assert_eq!(post.records_store_size, 1);
}
