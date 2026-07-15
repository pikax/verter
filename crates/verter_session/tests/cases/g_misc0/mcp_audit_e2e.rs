//! End-to-end discriminating test for `VerterHost::audit_mcp_tool_call`.
//!
//! Wires an MCP tool invocation through the audit harness, drives a
//! sub-request (`compile_with_audit`) inside the MCP closure, and
//! asserts:
//! 1. The MCP wrapper produces an audit record tagged
//!    `RequestKind::Mcp { tool: <name> }` carrying a populated
//!    `McpToolPayload` (tool_name, args/result sizes, error).
//! 2. The downstream `compile_with_audit` record records the MCP
//!    request id as its `parent_request_id` — the scheduler-side TLS
//!    propagation channel correlates the parent/child relationship
//!    automatically.
//! 3. Disabled-audit short-circuit: when `audit_enabled = false` the
//!    closure still runs but no record is produced.
//!
//! Discrimination: pre-change tree has no `audit_mcp_tool_call`
//! method on `VerterHost`, so this file does not compile against
//! it. Post-change tree must compile AND satisfy the parent_request_id
//! correlation.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use std::convert::Infallible;

use verter_audit::{AuditCaptureState, RequestKind, RequestKindPayload};
use verter_compiler::compile::CompileTarget;
use verter_session::host_mcp_audit::McpToolSuccess;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   import { ref } from 'vue';\n\
                   const count = ref(0);\n\
                   </script>\n\
                   <template><button @click=\"count++\">{{ count }}</button></template>\n";

fn build_host(audit_enabled: bool) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/m.vue".into()),
        input_id: "/m.vue".into(),
        source: Arc::from(SFC),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    host
}

#[test]
fn audit_mcp_tool_call_publishes_record_with_mcp_kind_and_payload() {
    let host = build_host(true);

    let args_json = "{\"path\":\"/m.vue\"}";
    let args_size = args_json.len() as u32;

    let (outcome, record) = host
        .audit_mcp_tool_call::<u32, Infallible, _>("compile_file", "/m.vue", args_size, |h| {
            // Tool body: drive a real audited sub-request. The
            // RequestContextGuard installed by audit_mcp_tool_call is on
            // TLS during this closure, so the sub-request inherits the
            // MCP request as its parent.
            let result = match h
                .compile_with_audit("/m.vue", CompileTarget::IDE)
                .into_result()
            {
                Ok(r) => r,
                Err(e) => match e {},
            };
            let bytes = result
                .tsx
                .as_ref()
                .map(|t| t.code.len() as u32)
                .unwrap_or(0);
            Ok(McpToolSuccess {
                value: bytes,
                result_size_bytes: bytes,
            })
        })
        .into_parts();
    let compiled_bytes = outcome.expect("infallible tool body");

    // 1. Closure value plumbed through unchanged.
    assert!(
        compiled_bytes > 0,
        "IDE compile must produce a non-empty TSX block — discriminates a \
         regression where audit_mcp_tool_call would swallow the closure value",
    );

    // 2. Record envelope is the MCP kind.
    match &record.kind {
        RequestKind::Mcp { tool } => {
            assert_eq!(
                tool, "compile_file",
                "tool name on the kind discriminant must round-trip from the caller"
            );
        }
        other => panic!("expected RequestKind::Mcp, got {other:?}"),
    }
    assert_eq!(record.canonical_id, "/m.vue");
    assert!(
        record.parent_request_id.is_none(),
        "top-level MCP call has no enclosing parent — parent_request_id must be None"
    );

    // 3. McpToolPayload variant + populated fields.
    let payload = match &record.kind_payload {
        RequestKindPayload::Mcp(p) => p,
        other => panic!("expected RequestKindPayload::Mcp, got {other:?}"),
    };
    assert_eq!(payload.tool_name, "compile_file");
    assert_eq!(
        payload.args_size_bytes, args_size,
        "args_size_bytes must round-trip from the caller — discriminates a \
         regression that hardcodes 0 / drops the input"
    );
    assert!(
        payload.result_size_bytes > 0,
        "compile_file produced a non-empty TSX block; payload.result_size_bytes \
         must reflect the closure-supplied measurement"
    );
    assert!(
        payload.error.is_none(),
        "successful tool run must leave error == None"
    );

    // 4. Total wall-clock recorded.
    assert!(
        record.timings.total_ms >= 0.0,
        "total_ms must be non-negative; got {}",
        record.timings.total_ms
    );
}

#[test]
fn audit_mcp_tool_call_disabled_audit_returns_none_record_but_still_runs_closure() {
    let host = build_host(false);

    let mut closure_ran = false;
    let (outcome, record) = host
        .audit_mcp_tool_call::<u32, Infallible, _>("get_component_api", "/m.vue", 0, |_h| {
            closure_ran = true;
            Ok(McpToolSuccess {
                value: 42_u32,
                result_size_bytes: 0,
            })
        })
        .into_parts();
    let value = outcome.expect("infallible tool body");

    assert!(
        closure_ran,
        "audit_enabled=false must still run the closure"
    );
    assert_eq!(
        value, 42,
        "closure value must round-trip even with audit off"
    );
    assert_eq!(
        record.capture_state,
        AuditCaptureState::AuditDisabled,
        "audit_enabled=false ⇒ the carrier still returns a record, marked AuditDisabled — \
         discriminates against a regression that mislabels the disabled path"
    );
}

#[test]
fn audit_mcp_tool_call_subrequest_records_mcp_request_as_parent_request_id() {
    // This is the parent_request_id correlation discriminator. Drive
    // an MCP tool call whose closure initiates a component-meta
    // sub-request through the same host. The component-meta path
    // routes through `RequestContext::with_kind_and_timing`, which
    // sniffs the scheduler-side TLS slot for an enclosing parent.
    // The MCP wrapper installs that TLS slot for the duration of the
    // closure, so the sub-request must inherit the MCP request id.
    let host = build_host(true);

    let mut sub_request_id: Option<u64> = None;
    let (_, mcp_record) = host
        .audit_mcp_tool_call::<u64, Infallible, _>("get_component_meta", "/m.vue", 0, |h| {
            let (_analysis, resolution) = h
                .get_component_meta_with_resolution("/m.vue")
                .expect("component-meta resolution must succeed for the fixture SFC");
            sub_request_id = Some(resolution.request_id);
            Ok(McpToolSuccess {
                value: resolution.request_id,
                result_size_bytes: 0,
            })
        })
        .into_parts();
    let sub_request_id = sub_request_id.expect("component-meta resolution must stamp request_id");
    let sub_record = host
        .take_audit_record(sub_request_id)
        .expect("component-meta sub-request must publish an audit record");

    // The MCP record's request_id must be the parent of the sub-request.
    assert_ne!(
        mcp_record.request_id, sub_record.request_id,
        "MCP and sub-request must have distinct request ids"
    );
    assert_eq!(
        sub_record.parent_request_id,
        Some(mcp_record.request_id.to_string()),
        "downstream component-meta record must inherit MCP request id as parent_request_id; \
         audit_mcp_tool_call installs RequestContextGuard so the \
         sub-request RequestContext sniffs the parent at construction"
    );

    // Negative: the MCP record itself must NOT carry a parent (top-level call).
    assert!(
        mcp_record.parent_request_id.is_none(),
        "top-level MCP record must have no parent; got {:?}",
        mcp_record.parent_request_id
    );
}

#[test]
fn audit_mcp_tool_call_propagates_error_into_payload() {
    let host = build_host(true);

    let (outcome, record) = host
        .audit_mcp_tool_call::<u32, String, _>("compile_file", "/m.vue", 0, |_h| {
            Err("simulated tool failure".to_string())
        })
        .into_parts();

    // The closure's `Err(E)` rides the carrier's `Err` arm — no
    // nested Result inside `Ok`.
    assert_eq!(
        outcome.unwrap_err(),
        "simulated tool failure",
        "the typed error must round-trip through the carrier's Err arm"
    );
    let payload = match &record.kind_payload {
        RequestKindPayload::Mcp(p) => p,
        other => panic!("expected Mcp payload, got {other:?}"),
    };
    // The wrapper folds the error into the payload via its `Debug`
    // rendering; a `String` Debug-renders quoted.
    assert_eq!(
        payload.error.as_deref(),
        Some("\"simulated tool failure\""),
        "error message must round-trip into the McpToolPayload — \
         discriminates a regression that drops or rewrites the message"
    );
}
