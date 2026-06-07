//! `VERTER_LSP_AUDIT_TRACE_OUT` integration test.
//!
//! Mirrors the existing `VERTER_COMPONENT_META_AUDIT_JSON_OUT`
//! drainer: when the env var is set, every finalised LSP audit
//! record is appended to the named path as a single JSON line. The
//! test populates a temp directory, sets the env var, drives one
//! audited LSP run, and asserts the file holds the produced record.

use std::sync::Arc;
use std::time::Duration;

use verter_audit::payloads::tags::LspMethodTag;
use verter_lsp::audit_harness;
use verter_session::{HostConfig, VerterHost};

#[tokio::test]
async fn audited_lsp_run_appends_jsonline_to_trace_out_when_env_var_set() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let dir = tempfile::tempdir().expect("tempdir for trace-out");
    let path = dir.path().join("lsp-audit-trace.jsonl");

    // SAFETY: the test runtime is single-process; the var is
    // namespaced and removed at the end of the test.
    unsafe {
        std::env::set_var("VERTER_LSP_AUDIT_TRACE_OUT", &path);
    }

    let canonical = "/trace.vue".to_string();
    let _ = audit_harness::run_with_audit::<u8, _, _>(
        &host,
        LspMethodTag::Hover,
        canonical,
        Some(tower_lsp_server::ls_types::Position {
            line: 1,
            character: 1,
        }),
        Duration::from_secs(2),
        async move { Ok(1u8) },
        |payload, _value| {
            payload.response_size_bytes = 1;
        },
    )
    .await;

    unsafe {
        std::env::remove_var("VERTER_LSP_AUDIT_TRACE_OUT");
    }

    let contents = std::fs::read_to_string(&path).expect("trace-out file must exist");
    assert!(
        !contents.trim().is_empty(),
        "trace-out file must contain at least one line"
    );
    let line = contents
        .lines()
        .next()
        .expect("trace-out must contain a JSON line");
    let parsed: serde_json::Value =
        serde_json::from_str(line).expect("trace-out line must be valid JSON");
    let kind = parsed
        .get("kind")
        .expect("record must carry a kind discriminant");
    let method = kind
        .get("Lsp")
        .and_then(|v| v.get("method"))
        .expect("kind.Lsp.method must be set on an Lsp record");
    assert_eq!(
        method.as_str(),
        Some("Hover"),
        "trace-out record must reflect the producer method"
    );
}
