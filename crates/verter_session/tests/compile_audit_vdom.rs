//! Slice 3.B end-to-end test for the VDOM (`CompileTarget::BUNDLER`)
//! codegen path through `VerterHost::compile_with_audit`.
//!
//! Discrimination contract:
//! - **Pre-change tree**: no `compile_with_audit` entry-point exists,
//!   no producer-side instrumentation, no audit record published.
//!   The test would fail to compile (missing method) AND the
//!   payload assertions could not run.
//! - **Post-change tree**: `compile_with_audit` exists, producer
//!   crate emits `record_phase_timing` and `record_event(...)` at
//!   phase boundaries, and the assembled `CompilePayload` carries
//!   non-trivial `code_transform_ops`, `output_bytes`,
//!   `num_script_blocks`, and the `target == Vdom` tag.

use std::sync::Arc;

use verter_audit::payloads::tags::CompileTargetTag;
use verter_compiler::compile::CompileTarget;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   const greeting = 'hello';\n\
                   </script>\n\
                   <template><div>{{ greeting }}</div></template>\n";

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
        canonical_id: Some("/v.vue".into()),
        input_id: "/v.vue".into(),
        source: Arc::from(SFC),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });
    host
}

#[test]
fn compile_with_audit_vdom_publishes_record_with_compile_kind_and_vdom_tag() {
    let host = build_host(true);
    let (result, record) = host.compile_with_audit("/v.vue", CompileTarget::BUNDLER);

    // Compile result itself must be non-empty.
    assert!(
        result.script.is_some(),
        "VDOM compile must produce a script block for setup SFC; result.errors = {:?}",
        result.errors
    );
    assert!(
        result.template.is_some(),
        "VDOM compile must produce a template block for the <template> region",
    );

    // Audit record must be published with Compile kind + Vdom tag.
    let record = record.expect(
        "compile_with_audit must publish a record when audit_enabled=true; \
         a regression that drops the registration finalize would surface as None",
    );
    assert_eq!(record.canonical_id, "/v.vue");
    match &record.kind {
        verter_audit::RequestKind::Compile { target } => {
            assert_eq!(*target, CompileTargetTag::Vdom, "BUNDLER target ⇒ Vdom tag");
        }
        other => panic!("expected RequestKind::Compile, got {other:?}"),
    }

    let payload = record
        .compile_payload()
        .expect("Compile kind ⇒ kind_payload must carry CompilePayload");

    // Discriminate against a producer that never emitted: the BUNDLER
    // path runs script + template codegen which both touch
    // CodeTransform many times. A regression that loses TLS would
    // leave this counter at 0.
    assert!(
        payload.code_transform_ops > 0,
        "VDOM compile must observe at least one CodeTransform op; \
         a regression that drops producer-side `record_event(CompileCodeTransformOp)` \
         would leave this at 0. payload = {payload:?}",
    );
    assert_eq!(payload.target, CompileTargetTag::Vdom);
    // Output_bytes is the sum of each block's code length. Non-zero
    // for our simple fixture.
    assert!(
        payload.output_bytes > 0,
        "VDOM compile must report non-zero output_bytes; payload = {payload:?}",
    );
    assert_eq!(
        payload.num_script_blocks, 1,
        "fixture has exactly one <script setup> block",
    );
}

#[test]
fn compile_with_audit_vdom_returns_no_record_when_audit_disabled() {
    let host = build_host(false);
    let (result, record) = host.compile_with_audit("/v.vue", CompileTarget::BUNDLER);
    assert!(
        record.is_none(),
        "audit_enabled=false ⇒ no record. A regression that always publishes \
         (e.g. forgetting the `audit_enabled` gate) would fail this assertion.",
    );
    assert!(
        result.script.is_some(),
        "compile must still run on the audit-disabled fast path",
    );
}
