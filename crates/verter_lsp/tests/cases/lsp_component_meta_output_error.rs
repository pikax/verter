//! `$/verter/getComponentMeta` wire failure semantics (fail-closed).
//!
//! Drives the ACTUAL LSP custom method end to end and pins the three
//! DISCRIMINATED outcomes:
//!
//! - a resolving component returns the FFI JSON payload;
//! - a typed output-materialization failure surfaces as a JSON-RPC ERROR
//!   carrying the failed lane/index in `data` — NEVER `null` (a real
//!   fail-closed materialization failure must not be reported as "the
//!   component does not exist");
//! - a genuinely missing component stays `null` (absence is reserved for
//!   absence).
//!
//! The failure is injected through `verter_session`'s canonical-keyed
//! `test_only::component_meta_output::force_output_failure_for` seam (the
//! `test-support` feature this test target activates), so the assertion
//! exercises the real audited entry
//! (`get_component_meta_output_with_resolution`) and the real handler
//! mapping — not a mock.

use std::sync::Arc;

use tower_lsp_server::LspService;
use verter_lsp::server::{GetComponentMetaParams, VerterLanguageServer};
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
                type_provider_topology: verter_lsp::TypeProviderTopology::None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: TypeProviderKind::Tsserver,
                mcp_port: None,
                type_provider_reason: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    service
}

#[tokio::test]
async fn component_meta_output_failure_surfaces_as_jsonrpc_error_not_null() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let _ = host
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some("/LspOutputFail.vue".to_string()),
            input_id: "/LspOutputFail.vue".to_string(),
            source: r#"<script setup lang="ts">
defineProps<{ label: string }>()
</script>
<template><div /></template>"#
                .into(),
            file_language: verter_session::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let service = build_test_server(Arc::clone(&host));

    // Control 1: the un-forced request returns the real FFI JSON payload.
    let ok = service
        .inner()
        .get_component_meta(GetComponentMetaParams {
            uri: "file:///LspOutputFail.vue".to_string(),
        })
        .await
        .expect("the un-forced request must not error")
        .expect("the component resolves to a payload");
    assert!(ok.is_object(), "the payload is the FFI JSON object");

    // A typed output-materialization failure is a JSON-RPC ERROR — never
    // `null`. `null` would falsely report a real fail-closed failure as
    // "the component does not exist".
    verter_session::test_only::component_meta_output::force_output_failure_for(
        "/LspOutputFail.vue",
    );
    let err = service
        .inner()
        .get_component_meta(GetComponentMetaParams {
            uri: "file:///LspOutputFail.vue".to_string(),
        })
        .await
        .expect_err(
            "a typed output-materialization failure must surface as a \
             JSON-RPC error, never the null absence result",
        );
    assert!(
        err.message.contains("getComponentMeta"),
        "the error names the method; got {err:?}"
    );
    let data = err.data.as_ref().expect("the error carries a data payload");
    assert_eq!(
        data.get("lane").and_then(|v| v.as_str()),
        Some("props[].type"),
        "the data payload carries the failed lane path; got {data:?}"
    );
    assert!(
        data.get("index").is_some() && data.get("failure").is_some(),
        "the data payload carries the positional index and failure class; got {data:?}"
    );

    // Control 2: a genuinely missing component stays `null`.
    let missing = service
        .inner()
        .get_component_meta(GetComponentMetaParams {
            uri: "file:///DoesNotExist.vue".to_string(),
        })
        .await
        .expect("a missing component is not an error");
    assert!(
        missing.is_none(),
        "absence stays the null result — reserved exclusively for absence"
    );
}

/// A MALFORMED request URI is a REQUEST fault — JSON-RPC `InvalidParams` —
/// never the `null` absence result (`null` is reserved EXCLUSIVELY for a
/// canonical that does not resolve to a component).
///
/// Discriminating: with the handler mapping a URI parse failure to
/// `Ok(None)`, the `expect_err` below fails RED.
#[tokio::test]
async fn component_meta_malformed_uri_is_invalid_params_not_null() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    let service = build_test_server(Arc::clone(&host));

    let err = service
        .inner()
        .get_component_meta(GetComponentMetaParams {
            uri: "not a uri \u{0}".to_string(),
        })
        .await
        .expect_err("a malformed URI must be InvalidParams, never the null absence result");
    assert_eq!(
        err.code,
        tower_lsp_server::jsonrpc::ErrorCode::InvalidParams,
        "the request fault is InvalidParams; got {err:?}"
    );
}
