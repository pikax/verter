//! F4 regression coverage for `VerterHost::compile_with_audit_options`:
//!
//! 1. **Cheap FilteredNoop.** A consumer-filtered compile (the
//!    `AuditConsumerFilter` rejects `RequestKind::Compile`) must return
//!    the cheap default-filled record marked `FilteredNoop` — the
//!    zero-valued `CompilePayload` (only the `target` tag set), NOT a
//!    fully-assembled payload that was built and then discarded. The
//!    compile RESULT still computes (consumers asked for it).
//!
//! 2. **Parent correlation.** A compile issued inside an enclosing
//!    audited request's `RequestContext` window carries that request's
//!    id as its `parent_request_id`; a top-level compile carries `None`.
//!
//! Discrimination:
//! - The FilteredNoop test asserts `capture_state == FilteredNoop` AND
//!   that the payload is the zero default (only the `target` tag set, no
//!   producer counters). A regression that built and discarded the full
//!   payload on the filtered path would carry non-zero producer counters
//!   here and FAIL the zero-payload assertion — so the test discriminates
//!   the cheap record from an assemble-then-drop record, not just the
//!   capture-state tag.
//! - The parent test FAILS when `parent_request_id` is unconditionally
//!   `None` (it would observe `None` under an enclosing request) and
//!   PASSES once the field is sourced from the `RequestContext`.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_audit::payloads::tags::CompileTargetTag;
use verter_audit::{AuditCaptureState, AuditConfig, AuditConsumerFilter, CompilePayload};
use verter_compiler::compile::CompileTarget;
use verter_session::request_context::{RequestContext, RequestContextGuard};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   import { ref } from 'vue';\n\
                   const count = ref(0);\n\
                   </script>\n\
                   <template><button @click=\"count++\">{{ count }}</button></template>\n";

fn upsert(host: &VerterHost) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/f.vue".into()),
        input_id: "/f.vue".into(),
        source: Arc::from(SFC),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });
}

#[test]
fn filtered_compile_returns_cheap_filtered_noop_with_zero_payload() {
    // Audit enabled, but the consumer filter denies every kind so the
    // Compile registration becomes `Noop` at construction time.
    let mut host = VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: false,
        ..HostConfig::default()
    });
    host.replace_host_audit_runtime_for_test(AuditConfig {
        consumer_filter: AuditConsumerFilter::deny_all(),
        ..AuditConfig::default()
    });
    let host = Arc::new(host);
    upsert(&host);

    let (result, record) = host
        .compile_with_audit("/f.vue", CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    // The compile RESULT still computes on the filtered path.
    assert!(
        result.tsx.is_some(),
        "filtered compile must still produce the requested output; \
         errors = {:?}",
        result.errors
    );

    // The record is the cheap FilteredNoop, not an ActiveStored record.
    assert_eq!(
        record.capture_state,
        AuditCaptureState::FilteredNoop,
        "a consumer-filtered compile must return the cheap FilteredNoop record"
    );

    // The payload is the zero-valued default carrying only the target
    // tag — no producer counters were assembled on the filtered path.
    // (`CompilePayload` does not derive `PartialEq`, so assert the
    // discriminating zero-valued fields directly.)
    let payload = record
        .compile_payload()
        .expect("Compile kind ⇒ kind_payload carries CompilePayload");
    let default = CompilePayload::default();
    assert_eq!(payload.target, CompileTargetTag::Ide);
    assert_eq!(
        payload.output_bytes, default.output_bytes,
        "filtered path must NOT assemble output_bytes; got {payload:?}"
    );
    assert_eq!(
        payload.code_transform_ops, default.code_transform_ops,
        "filtered path must NOT assemble code_transform_ops; got {payload:?}"
    );
    assert_eq!(
        payload.num_components, default.num_components,
        "filtered path must NOT assemble num_components; got {payload:?}"
    );
    assert_eq!(
        payload.num_script_blocks, default.num_script_blocks,
        "filtered path must NOT assemble num_script_blocks; got {payload:?}"
    );
    assert!(
        payload.parse_ms.is_none()
            && payload.transform_ms.is_none()
            && payload.codegen_ms.is_none()
            && payload.sourcemap_ms.is_none(),
        "filtered path must NOT assemble phase timings; got {payload:?}"
    );
}

#[test]
fn compile_subrequest_carries_parent_request_id() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: false,
        ..HostConfig::default()
    }));
    upsert(&host);

    // Install an enclosing audited request context so the scheduler TLS
    // exposes a parent request id. The compile issued inside this guard
    // window is a sub-request of `PARENT_ID`.
    const PARENT_ID: u64 = 909_090;
    let parent_ctx = RequestContext::new(PARENT_ID, Arc::from("/parent.vue"), false, None);
    let _parent_guard = RequestContextGuard::install(parent_ctx);

    let (result, record) = host
        .compile_with_audit("/f.vue", CompileTarget::IDE)
        .into_parts();
    let _result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    assert_eq!(
        record.parent_request_id.as_deref(),
        Some(PARENT_ID.to_string().as_str()),
        "a compile sub-request must carry its enclosing request's id as \
         parent_request_id; the hard-coded `None` regression would fail here"
    );
}
