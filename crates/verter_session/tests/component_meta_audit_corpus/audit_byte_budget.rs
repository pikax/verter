//! External-corpus byte-budget guard for ChatMessage's audit payload.
//!
//! Gated behind `external-corpus`: this test file only runs when the
//! caller passes `--features external-corpus`. The default
//! `cargo test --workspace --tests --verbose` run does NOT enable this
//! feature, so the body never executes in hermetic CI.
//!
//! The expectation: serialized `RequestAuditRecord` bytes for a
//! ChatMessage component must stay under 2 MiB. SA-1.B-impl's
//! graph-native synthesis publishes shallow shells; the legacy path's
//! materializer-driven expansion blows the budget.
//!
//! This file is excluded from the generator-managed
//! `crates/verter_session/tests/corpus_audit_tests.rs`. To wire it as a
//! standalone integration target, add it as a `[[test]]` in
//! `Cargo.toml` once SA-1.B-impl lands. Today the file is reachable
//! only as a `#[path = "..."]` mod from a Cargo `[[test]]` definition.

#![cfg(feature = "external-corpus")]

use std::path::Path;
use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::types::FileKind;
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const NUXT_UI_BENCH: &str = ".integration-tests/repos/nuxt-ui-codex-bench";

#[test]
fn chatmessage_audit_payload_byte_budget() {
    let chat_path = Path::new(NUXT_UI_BENCH).join("src/runtime/components/ChatMessage.vue");
    if !chat_path.exists() {
        eprintln!(
            "external corpus missing at {}; skipping",
            chat_path.display()
        );
        return;
    }
    let chat_message = std::fs::read_to_string(&chat_path).expect("read ChatMessage.vue");

    let result = AuditedRequest::builder()
        .files([("/ChatMessage.vue", chat_message.as_str())])
        .resolve_component_meta("/ChatMessage.vue");

    match result {
        Ok((_meta, _resolved, record)) => {
            let bytes = serde_json::to_vec(&record).expect("audit record serialization");
            let len = bytes.len();
            assert!(
                len < 2 * 1024 * 1024,
                "ChatMessage audit record must serialize under 2 MiB; observed {len} bytes",
            );
        }
        Err(err) => {
            panic!("chatmessage_audit_payload_byte_budget: AuditedRequest failed: {err:?}",)
        }
    }
}

// Belt-and-suspenders: a host-level smoke that verifies the helper
// path compiles without the `AuditedRequest` builder. Reachable only
// when the corpus is present.
#[test]
fn chatmessage_audit_byte_budget_via_host_only() {
    let chat_path = Path::new(NUXT_UI_BENCH).join("src/runtime/components/ChatMessage.vue");
    if !chat_path.exists() {
        eprintln!(
            "external corpus missing at {}; skipping",
            chat_path.display()
        );
        return;
    }
    let chat_message = std::fs::read_to_string(&chat_path).expect("read ChatMessage.vue");

    let mut config = HostConfig::default();
    config.audit_enabled = true;
    config.footprint_capture = true;
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = Arc::new(VerterHost::new(config, workspace));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/ChatMessage.vue".to_string(),
            source: Arc::from(chat_message.as_str()),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert");
    let _ = host.get_component_meta("/ChatMessage.vue");
}
