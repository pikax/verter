//! Basic LSP audit smoke test.
//!
//! Drives the [`verter_session::host_lsp_audit::LspAuditSession`]
//! lifecycle end-to-end: open a session, finalize with an LSP
//! payload, observe the published record on the host's records
//! store. Without audit wiring the LSP handlers would leave the
//! records store empty.
//!
//! Canonical nextest execution uses the single aggregate `#[tokio::test]`
//! below. Its four sequential cases share only execution worker pools; every
//! case constructs a fresh workspace, host, scheduler/driver, and audit state.

use std::sync::Arc;

use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{LspRequestPayload, RequestKind, RequestKindPayload};
use verter_lsp::audit_harness;
use verter_session::host_lsp_audit::LspAuditSession;
use verter_session::{HostConfig, LspMethodTimeoutsConfig, TestHostWorkerPools, VerterHost};

fn fresh_host(config: HostConfig, worker_pools: &Arc<TestHostWorkerPools>) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(Default::default()));
    Arc::new(VerterHost::new_with_test_worker_pools(
        config,
        workspace,
        Arc::clone(worker_pools),
    ))
}

async fn lsp_audit_hover_session_publishes_lsp_record(worker_pools: &Arc<TestHostWorkerPools>) {
    let host = fresh_host(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            lsp_method_timeouts: LspMethodTimeoutsConfig::default(),
            ..HostConfig::default()
        },
        worker_pools,
    );

    let canonical = "/probe.vue".to_string();
    let session = host.lsp_audit_begin(
        LspMethodTag::Hover,
        verter_audit::RequestTargetIdentity::RegisteredCanonical(canonical.clone()),
    );
    let request_id = session
        .request_id()
        .expect("Active session must expose its id");

    // Mid-flight: the request must appear in the active-request
    // registry (the registration's slot).
    let mid = host.host_audit_runtime().snapshot();
    assert_eq!(mid.active_request_count, 1);
    assert!(mid.contains_active_request(request_id));
    assert_eq!(mid.records_store_size, 0);

    // Finalise with a populated payload.
    let payload = LspRequestPayload {
        method: LspMethodTag::Hover,
        position: Some(verter_audit::payloads::lsp::PositionInfo {
            canonical_id: canonical.clone(),
            target_identity: Some(verter_audit::RequestTargetIdentity::RegisteredCanonical(
                canonical.clone(),
            )),
            line: 4,
            character: 7,
        }),
        response_size_bytes: 42,
        ..LspRequestPayload::default()
    };
    let returned = session
        .finalize_ok(payload)
        .expect("finalize_ok must produce the published record");
    assert_eq!(returned.request_id, request_id);
    assert!(matches!(
        returned.kind,
        RequestKind::Lsp {
            method: LspMethodTag::Hover
        }
    ));

    // Post-flight: the active-request slot is drained AND the
    // record is observable in the records store.
    let post = host.host_audit_runtime().snapshot();
    assert_eq!(post.active_request_count, 0);
    assert!(!post.contains_active_request(request_id));
    assert_eq!(post.records_store_size, 1);

    let record = host
        .host_audit_runtime()
        .take_record(request_id)
        .expect("Lsp record must be retrievable from the records store");
    let payload = match record.kind_payload {
        RequestKindPayload::Lsp(p) => p,
        other => panic!("expected RequestKindPayload::Lsp, got {other:?}"),
    };
    assert_eq!(payload.method, LspMethodTag::Hover);
    let pos = payload.position.expect("position must be populated");
    assert_eq!(pos.line, 4);
    assert_eq!(pos.character, 7);
    assert_eq!(pos.canonical_id, canonical);
    assert_eq!(payload.response_size_bytes, 42);
    assert!(
        payload.error.is_none(),
        "happy-path payload must have no cancellation marker"
    );
}

async fn lsp_audit_disabled_returns_noop_session(worker_pools: &Arc<TestHostWorkerPools>) {
    let host = fresh_host(HostConfig::default(), worker_pools);
    assert!(!host.config().audit_enabled);

    let session = host.lsp_audit_begin(
        LspMethodTag::Hover,
        verter_audit::RequestTargetIdentity::RegisteredCanonical("/probe.vue".to_string()),
    );
    assert!(matches!(session, LspAuditSession::Noop));
    assert!(session.request_id().is_none());

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap.records_store_size, 0,
        "Noop sessions must NOT publish records"
    );
    assert_eq!(
        snap.active_request_count, 0,
        "Noop sessions must NOT enter the active-request registry"
    );

    // Finalising a Noop session is a tested no-op.
    let nothing = session.finalize_ok(LspRequestPayload {
        method: LspMethodTag::Hover,
        ..LspRequestPayload::default()
    });
    assert!(nothing.is_none());
}

/// `run_with_audit` end-to-end: drives the harness through the same
/// path the LSP handlers take, asserting that audit-enabled runs
/// publish a record AND audit-disabled runs short-circuit.
async fn run_with_audit_publishes_record_when_audit_enabled(
    worker_pools: &Arc<TestHostWorkerPools>,
) {
    let host = fresh_host(
        HostConfig {
            audit_enabled: true,
            ..HostConfig::default()
        },
        worker_pools,
    );
    let snap_before = host.host_audit_runtime().snapshot();
    assert_eq!(snap_before.records_store_size, 0);

    let canonical = "/probe.vue".to_string();
    let position = tower_lsp_server::ls_types::Position {
        line: 1,
        character: 2,
    };
    let result = audit_harness::run_with_audit::<u8, _, _>(
        &host,
        LspMethodTag::Hover,
        verter_audit::RequestTargetIdentity::RegisteredCanonical(canonical),
        Some(position),
        async move { Ok(7u8) },
        |payload, value| {
            payload.response_size_bytes = u32::from(*value);
        },
    )
    .await
    .expect("body must succeed");
    assert_eq!(result, 7);

    // Exactly one record published; active-request registry drained.
    let snap_after = host.host_audit_runtime().snapshot();
    assert_eq!(snap_after.records_store_size, 1);
    assert_eq!(snap_after.active_request_count, 0);
}

async fn run_with_audit_short_circuits_when_audit_disabled(
    worker_pools: &Arc<TestHostWorkerPools>,
) {
    let host = fresh_host(HostConfig::default(), worker_pools);
    let result = audit_harness::run_with_audit::<u8, _, _>(
        &host,
        LspMethodTag::Hover,
        verter_audit::RequestTargetIdentity::RegisteredCanonical("/probe.vue".to_string()),
        Some(tower_lsp_server::ls_types::Position {
            line: 0,
            character: 0,
        }),
        async move { Ok(1u8) },
        |payload, _value| {
            payload.response_size_bytes = 1;
        },
    )
    .await
    .expect("audit-disabled body should pass through");
    assert_eq!(result, 1);

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(snap.records_store_size, 0);
    assert_eq!(snap.active_request_count, 0);
}

#[tokio::test]
async fn lsp_audit_hover_cases_share_only_execution_pools() {
    let config = HostConfig::default();
    let worker_pools = TestHostWorkerPools::new(
        &config,
        verter_scheduler::scheduler::SchedulerConfig::default(),
    );
    let pool_ids = worker_pools.pool_ids();

    lsp_audit_hover_session_publishes_lsp_record(&worker_pools).await;
    lsp_audit_disabled_returns_noop_session(&worker_pools).await;
    run_with_audit_publishes_record_when_audit_enabled(&worker_pools).await;
    run_with_audit_short_circuits_when_audit_disabled(&worker_pools).await;

    let receipt = worker_pools.receipt();
    assert_eq!(receipt.pool_ids, pool_ids);
    assert_eq!(receipt.host_shells_created, 4);
    assert_eq!(receipt.scheduler_shells_created, 4);
    assert_eq!(receipt.active_scheduler_shells, 0);
}
