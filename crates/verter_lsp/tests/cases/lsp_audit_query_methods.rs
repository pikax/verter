//! LSP audit-query methods (read-only).
//!
//! Drives `verter/audit/getRecord` and `verter/audit/getRecent` end
//! to end. The methods consult the host's records store via
//! `host_audit_runtime().audit_records_store()` and return JSON.
//!
//! The methods are read-only: a `getRecord` call must not drain the
//! store, and two consecutive calls return the same payload. The
//! tests assert that explicitly so a future change that accidentally
//! draws records out of the store fails loudly.

use std::sync::Arc;

use serde_json::Value;
use tower_lsp_server::LspService;
use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{
    ComponentMetaPayload, LspRequestPayload, RequestAuditRecord, RequestKind, RequestKindPayload,
};
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::server::{GetAuditRecentParams, GetAuditRecordParams};
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

fn build_test_server(host: Arc<VerterHost>) -> LspService<VerterLanguageServer> {
    let host_for_server = Arc::clone(&host);
    let (service, _socket) = LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_reason: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    service
}

/// Synthesise and insert a ComponentMeta-kind record directly into
/// the host's records store. The records store's `insert` API is the
/// public surface used by producers; tests reuse it to land
/// non-Lsp-shaped records without having to drive the full
/// component-meta resolver. Returns a monotonic request id picked
/// outside the host's own counter range to avoid collisions.
fn publish_component_meta_record(host: &Arc<VerterHost>, canonical: &str) -> u64 {
    use verter_audit::{RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit};
    // Pick an id deliberately above the LSP session id range used by
    // the rest of the tests so the record's discriminator is the
    // kind, not the id ordering.
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000_000);
    let request_id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let record = RequestAuditRecord {
        request_id,
        canonical_id: canonical.to_string(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        timings: RequestTimingAudit::default(),
        store: RequestStoreAudit::default(),
        memory: RequestMemoryAudit::default(),
        footprint: None,
        scheduler: None,
        from_cache: false,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
        trace_id: String::new(),
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
    };
    host.host_audit_runtime()
        .audit_records_store()
        .insert(record);
    request_id
}

/// Drive the `LspAuditSession` lifecycle so a record lands in the
/// store with a known `request_id`. Returns the published request id.
fn publish_lsp_record(host: &Arc<VerterHost>, method: LspMethodTag, canonical: &str) -> u64 {
    let session = host.lsp_audit_begin(method.clone(), canonical);
    let request_id = session
        .request_id()
        .expect("Active session must expose its id");
    let payload = LspRequestPayload {
        method,
        position: Some(verter_audit::payloads::lsp::PositionInfo {
            canonical_id: canonical.to_string(),
            line: 1,
            character: 2,
        }),
        response_size_bytes: 8,
        ..LspRequestPayload::default()
    };
    let returned = session
        .finalize_ok(payload)
        .expect("finalize must publish a record");
    assert_eq!(returned.request_id, request_id);
    request_id
}

#[tokio::test]
async fn get_audit_record_returns_published_record_without_draining() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let request_id = publish_lsp_record(&host, LspMethodTag::Hover, "/probe.vue");

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    // First read — discriminating: this exercises the
    // `get_audit_record` method on `VerterLanguageServer`.
    let response = server
        .get_audit_record(GetAuditRecordParams {
            request_id: request_id.to_string(),
        })
        .await
        .expect("get_audit_record must succeed");
    let json = response.expect("record must be present in the records store");
    // The wire form uses `u64_as_decimal_string` so request_id is a string.
    assert_eq!(
        json.get("request_id").and_then(Value::as_str),
        Some(request_id.to_string().as_str()),
        "Returned record must carry the queried request id"
    );

    // Discriminating-2: kind tag must round-trip through JSON.
    let kind_tag = json
        .get("kind")
        .expect("record must serialize a `kind` field");
    assert!(
        kind_tag
            .get("Lsp")
            .and_then(|v| v.get("method"))
            .and_then(Value::as_str)
            .map(|s| s == "Hover")
            .unwrap_or(false),
        "Kind payload must reflect the LSP hover method, got: {kind_tag}"
    );

    // Read-only check: a second call must return the same record;
    // calling `get_audit_record` does NOT drain the store.
    let again = server
        .get_audit_record(GetAuditRecordParams {
            request_id: request_id.to_string(),
        })
        .await
        .expect("second get_audit_record must succeed");
    assert!(
        again.is_some(),
        "get_audit_record must be read-only — drain semantics would return None here"
    );

    // Sanity: the records store retained the record.
    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(snap.records_store_size, 1);
}

#[tokio::test]
async fn get_audit_record_returns_none_for_unknown_id() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let _ = publish_lsp_record(&host, LspMethodTag::Hover, "/probe.vue");

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let response = server
        .get_audit_record(GetAuditRecordParams {
            request_id: "999999".to_string(),
        })
        .await
        .expect("get_audit_record must succeed");
    assert!(
        response.is_none(),
        "Unknown request id must yield None — never a default-shaped record"
    );

    // Negative: a malformed `request_id` (non-numeric) must also be
    // surfaced as `None`, never as a parser error or a panic.
    let response = server
        .get_audit_record(GetAuditRecordParams {
            request_id: "not-a-number".to_string(),
        })
        .await
        .expect("get_audit_record must succeed for malformed id");
    assert!(response.is_none());
}

#[tokio::test]
async fn get_audit_recent_returns_all_records_when_no_filter() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let id_a = publish_lsp_record(&host, LspMethodTag::Hover, "/a.vue");
    let id_b = publish_lsp_record(&host, LspMethodTag::Completion, "/b.vue");
    let id_c = publish_lsp_record(&host, LspMethodTag::Diagnostics, "/c.vue");

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let records = server
        .get_audit_recent(None)
        .await
        .expect("get_audit_recent must succeed");
    assert_eq!(records.len(), 3, "All published records must be returned");

    let returned_ids: Vec<u64> = records
        .iter()
        .map(|r| {
            r.get("request_id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .collect();
    assert!(returned_ids.contains(&id_a));
    assert!(returned_ids.contains(&id_b));
    assert!(returned_ids.contains(&id_c));

    // Discriminating: records must be sorted by request id descending.
    let mut sorted = returned_ids.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        returned_ids, sorted,
        "Records must be sorted descending by request id"
    );

    // Read-only check: a second call returns the same records.
    let again = server
        .get_audit_recent(None)
        .await
        .expect("second get_audit_recent must succeed");
    assert_eq!(again.len(), 3, "get_audit_recent must not drain the store");
}

#[tokio::test]
async fn get_audit_recent_filters_by_kind() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    // Three Lsp records, one ComponentMeta record.
    let _ = publish_lsp_record(&host, LspMethodTag::Hover, "/a.vue");
    let _ = publish_lsp_record(&host, LspMethodTag::Completion, "/b.vue");
    let _ = publish_lsp_record(&host, LspMethodTag::Diagnostics, "/c.vue");

    // Publish a ComponentMeta-kind record directly via the audit
    // registration so the kind filter has a non-Lsp variant to
    // exclude.
    let cm_request_id = publish_component_meta_record(&host, "/cm.vue");

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    // Filter to only Lsp.
    let lsp_only = server
        .get_audit_recent(Some(GetAuditRecentParams {
            kind: Some("Lsp".to_string()),
            limit: None,
        }))
        .await
        .expect("get_audit_recent must succeed");
    assert_eq!(lsp_only.len(), 3, "Three Lsp records must match the filter");
    for record in &lsp_only {
        // Discriminating: every returned record must be an Lsp kind.
        let kind = record.get("kind").expect("record must have kind");
        assert!(
            kind.get("Lsp").is_some(),
            "kind filter must exclude non-Lsp records, got: {kind}"
        );
    }
    let cm_id_str = cm_request_id.to_string();
    assert!(
        !lsp_only
            .iter()
            .any(|r| r.get("request_id").and_then(Value::as_str) == Some(cm_id_str.as_str())),
        "ComponentMeta record must not appear under Lsp filter"
    );

    // Filter to only ComponentMeta.
    let cm_only = server
        .get_audit_recent(Some(GetAuditRecentParams {
            kind: Some("ComponentMeta".to_string()),
            limit: None,
        }))
        .await
        .expect("get_audit_recent must succeed");
    assert_eq!(cm_only.len(), 1, "One ComponentMeta record must match");
    assert_eq!(
        cm_only[0].get("request_id").and_then(Value::as_str),
        Some(cm_request_id.to_string().as_str())
    );
}

#[tokio::test]
async fn get_audit_recent_respects_limit() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let _ = publish_lsp_record(&host, LspMethodTag::Hover, "/a.vue");
    let _ = publish_lsp_record(&host, LspMethodTag::Completion, "/b.vue");
    let id_c = publish_lsp_record(&host, LspMethodTag::Diagnostics, "/c.vue");

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let records = server
        .get_audit_recent(Some(GetAuditRecentParams {
            kind: None,
            limit: Some(2),
        }))
        .await
        .expect("get_audit_recent must succeed");
    assert_eq!(records.len(), 2, "limit must cap the result count");

    // The two newest records (highest ids) must be returned in
    // descending order.
    let returned_ids: Vec<u64> = records
        .iter()
        .map(|r| {
            r.get("request_id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .collect();
    assert_eq!(
        returned_ids[0], id_c,
        "Newest record (highest id) must come first under desc sort"
    );
    assert!(
        returned_ids[0] > returned_ids[1],
        "Records must be desc-sorted by request id; got {returned_ids:?}"
    );
}

#[tokio::test]
async fn get_audit_recent_uses_default_limit_when_none() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    // Publish more than the default limit (50) to verify it is
    // applied. `LspAuditSession` is cheap and synchronous.
    for i in 0..60 {
        let path = format!("/file_{i}.vue");
        let _ = publish_lsp_record(&host, LspMethodTag::Hover, &path);
    }

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let records = server
        .get_audit_recent(None)
        .await
        .expect("get_audit_recent must succeed");
    assert_eq!(
        records.len(),
        50,
        "Default limit (50) must apply when `limit` is None; got {} records",
        records.len()
    );
}

#[tokio::test]
async fn get_audit_recent_empty_store_returns_empty_array() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let records = server
        .get_audit_recent(None)
        .await
        .expect("get_audit_recent must succeed on empty store");
    assert!(records.is_empty(), "Empty store must yield empty array");
}

#[tokio::test]
async fn get_audit_record_preserves_kind_and_payload_pair() {
    // Discriminating coverage that the round-trip through
    // `serde_json::to_value` actually preserves the kind/payload
    // discriminator pair, not just the request id.
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let request_id = publish_lsp_record(&host, LspMethodTag::Diagnostics, "/d.vue");

    // Sanity check: the in-memory record before any JSON round-trip
    // must be Lsp-shaped on both `kind` and `kind_payload`.
    let mut probe: Option<RequestAuditRecord> = None;
    {
        use verter_audit::batch::AuditRecordSource;
        host.host_audit_runtime()
            .audit_records_store()
            .for_each_record(&mut |_inserted_at, r| {
                if r.request_id == request_id {
                    probe = Some(r.clone());
                }
            });
    }
    let probe = probe.expect("Lsp record must be present");
    assert!(matches!(probe.kind, RequestKind::Lsp { .. }));
    assert!(matches!(probe.kind_payload, RequestKindPayload::Lsp(_)));

    let service = build_test_server(Arc::clone(&host));
    let server = service.inner();

    let json = server
        .get_audit_record(GetAuditRecordParams {
            request_id: request_id.to_string(),
        })
        .await
        .expect("get_audit_record succeeds")
        .expect("record must be present");

    // Discriminating: the JSON round-trip must serialise both the
    // `kind` discriminant AND the `kind_payload` discriminator.
    let kind_tag = json
        .get("kind")
        .expect("record JSON must carry the kind tag");
    assert!(
        kind_tag.get("Lsp").is_some(),
        "kind must be Lsp-shaped in JSON, got: {kind_tag}"
    );

    let payload = json
        .get("kind_payload")
        .expect("record JSON must carry kind_payload");
    let payload_tag = payload
        .get("kind")
        .and_then(Value::as_str)
        .expect("kind_payload must carry a discriminator tag");
    assert_eq!(
        payload_tag, "Lsp",
        "kind_payload tag must match the LSP variant"
    );
}
