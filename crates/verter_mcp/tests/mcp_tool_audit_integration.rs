//! Production-path integration test for `RequestKind::Mcp` audit
//! emission.
//!
//! Drives a REAL MCP tool handler (`analyze_file` on
//! [`verter_mcp::VerterMcpServer`]) against a freshly-built host
//! with audit enabled. Asserts that the audited tool invocation:
//!
//! 1. Produces an audit record on the host store with
//!    `kind == RequestKind::Mcp { tool: "analyze_file" }`.
//! 2. The record's [`verter_audit::McpToolPayload`] carries the
//!    populated `tool_name`, non-zero `result_size_bytes` (the
//!    handler returns serialized JSON), and `error == None`.
//! 3. The canonical id from the tool argument round-trips into
//!    the record envelope.
//!
//! Discrimination: pre-Slice-3.F there was no `audit_mcp_tool_call`
//! method on `VerterHost` and the MCP handler did not emit an audit
//! record; the host store therefore had no record for this call to
//! retrieve. Post-Slice-3.F the handler routes through the wrapper
//! and the host store gains a record per call.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use verter_audit::{RequestKind, RequestKindPayload};
use verter_diagnostics::Linter;
use verter_mcp::config::McpServerConfig;
use verter_mcp::tools::diagnostics::make_lint_config;
use verter_mcp::VerterMcpServer;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   import { ref } from 'vue';\n\
                   const count = ref(0);\n\
                   defineProps<{ label: string }>();\n\
                   </script>\n\
                   <template><button @click=\"count++\">{{ label }}: {{ count }}</button></template>\n";

fn build_server() -> VerterMcpServer {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/c.vue".into()),
        input_id: "/c.vue".into(),
        source: Arc::from(SFC),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });
    let linter = Arc::new(Linter::new(make_lint_config("recommended")));
    VerterMcpServer::new(host, linter, McpServerConfig::default())
}

#[tokio::test]
async fn analyze_file_tool_emits_mcp_audit_record_with_populated_payload() {
    let server = build_server();

    // Drive the REAL `analyze_file` tool handler. This is the same
    // entry-point the rmcp router invokes when an MCP client issues
    // `tools/call` for `analyze_file`. The handler wraps its body in
    // `host.audit_mcp_tool_call("analyze_file", ...)`.
    let params = verter_mcp::server::AnalyzeFileParams {
        path: "/c.vue".to_string(),
        sections: None,
    };
    let result = server
        .analyze_file(Parameters(params))
        .await
        .expect("analyze_file tool succeeded");

    // The handler returned a `CallToolResult` with non-empty content.
    let text_present = result
        .content
        .iter()
        .any(|c| c.as_text().map(|t| !t.text.is_empty()).unwrap_or(false));
    assert!(
        text_present,
        "analyze_file must return non-empty text content"
    );

    // Drain the host's audit-records store and confirm at least one
    // `RequestKind::Mcp { tool: \"analyze_file\" }` record landed.
    // The post-change tree records exactly one MCP record per tool
    // invocation; the pre-change tree records zero (no
    // audit_mcp_tool_call wrapper to feed the records store).
    let host = &server.host;
    let mut found_analyze_file_record = None;
    // Scan a small window of request ids — the integration-level
    // test issues a single tool invocation; sub-requests sit in
    // `[1, 64)` even after a handful of compile / analysis fan-out
    // calls fire.
    for rid in 1u64..64 {
        if let Some(record) = host.take_audit_record(rid) {
            if let RequestKind::Mcp { tool } = &record.kind {
                if tool == "analyze_file" {
                    found_analyze_file_record = Some(record);
                    break;
                }
            }
        }
    }

    let mcp_record = found_analyze_file_record
        .expect("analyze_file tool handler must publish an MCP audit record");
    let payload = match &mcp_record.kind_payload {
        RequestKindPayload::Mcp(p) => p,
        other => panic!("expected RequestKindPayload::Mcp, got {other:?}"),
    };
    assert_eq!(payload.tool_name, "analyze_file");
    assert!(
        payload.result_size_bytes > 0,
        "analyze_file returned non-empty JSON; result_size_bytes must be > 0"
    );
    assert!(payload.error.is_none(), "successful tool ⇒ error == None");

    // The MCP record itself is top-level — no parent.
    assert!(
        mcp_record.parent_request_id.is_none(),
        "top-level MCP record must have no parent"
    );

    // Canonical id propagates from the caller into the record envelope.
    assert_eq!(
        mcp_record.canonical_id, "/c.vue",
        "canonical_id from the tool argument must round-trip into the record envelope"
    );
}
