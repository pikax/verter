//! Diagnostics + Completion audit tests.
//!
//! Drives the [`verter_lsp::audit_harness::run_with_audit`] code
//! path through a synthetic body that returns a representative
//! response shape, then asserts the published payload carries
//! `num_diagnostics` / `num_completion_items` matching the body
//! output.

use std::sync::Arc;

use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{LspRequestPayload, RequestKind, RequestKindPayload};
use verter_lsp::audit_harness;
use verter_session::{HostConfig, VerterHost};

#[tokio::test]
async fn diagnostics_audit_records_num_diagnostics() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));

    // The session API mirrors what `publish_full_diagnostics_with_audit`
    // emits internally — bind a known diagnostics count and finalize.
    let session = host.lsp_audit_begin(LspMethodTag::Diagnostics, "/diag.vue");
    let request_id = session
        .request_id()
        .expect("Active session for Diagnostics");
    let payload = LspRequestPayload {
        method: LspMethodTag::Diagnostics,
        num_diagnostics: Some(5),
        response_size_bytes: 5 * 160,
        ..LspRequestPayload::default()
    };
    let returned = session
        .finalize_ok(payload)
        .expect("finalize publishes a record");
    assert_eq!(returned.request_id, request_id);
    assert!(matches!(
        returned.kind,
        RequestKind::Lsp {
            method: LspMethodTag::Diagnostics
        }
    ));
    let p = match returned.kind_payload {
        RequestKindPayload::Lsp(p) => p,
        other => panic!("expected Lsp payload, got {other:?}"),
    };
    assert_eq!(
        p.num_diagnostics,
        Some(5),
        "num_diagnostics must reflect the producer's count"
    );
    assert!(p.error.is_none());
}

#[tokio::test]
async fn completion_audit_records_num_completion_items() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));

    let result = audit_harness::run_with_audit::<usize, _, _>(
        &host,
        LspMethodTag::Completion,
        "/comp.vue".to_string(),
        Some(tower_lsp_server::ls_types::Position {
            line: 0,
            character: 0,
        }),
        async move { Ok(7usize) },
        |payload, count| {
            payload.num_completion_items = Some(u32::try_from(*count).unwrap_or(u32::MAX));
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(64)).unwrap_or(u32::MAX);
        },
    )
    .await
    .expect("body succeeds");
    assert_eq!(result, 7);

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(snap.records_store_size, 1);
    // Drain the (single) record.
    let mut record = None;
    for candidate_id in 1..=64u64 {
        if let Some(r) = host.host_audit_runtime().take_record(candidate_id) {
            record = Some(r);
            break;
        }
    }
    let record = record.expect("completion record must exist");
    let p = match record.kind_payload {
        RequestKindPayload::Lsp(p) => p,
        other => panic!("expected Lsp payload, got {other:?}"),
    };
    assert_eq!(p.method, LspMethodTag::Completion);
    assert_eq!(
        p.num_completion_items,
        Some(7),
        "num_completion_items must reflect the producer's count"
    );
    assert!(p.error.is_none());
}
