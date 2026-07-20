//! End-to-end test for the IDE (`CompileTarget::IDE`) codegen
//! path through `VerterHost::compile_with_audit`.
//!
//! IDE target is the LSP/tsgo path — emits valid TSX. The audit kind
//! tag must be `Ide`, the producer must emit phase timings and
//! CodeTransform op events through the same TLS observer surface as
//! the VDOM path.

use std::sync::Arc;

use verter_audit::payloads::tags::CompileTargetTag;
use verter_compiler::compile::CompileTarget;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   import { ref } from 'vue';\n\
                   const count = ref(0);\n\
                   </script>\n\
                   <template><button @click=\"count++\">{{ count }}</button></template>\n";

const TYPED_PROPS_SFC: &str = "<script setup lang=\"ts\">\n\
                               defineProps<{ title: string }>();\n\
                               </script>\n\
                               <template><h1>{{ title }}</h1></template>\n";

fn build_host() -> Arc<VerterHost> {
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
        canonical_id: Some("/i.vue".into()),
        input_id: "/i.vue".into(),
        source: Arc::from(SFC),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    host
}

fn build_typed_props_host() -> Arc<VerterHost> {
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
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/typed.vue".into()),
            input_id: "/typed.vue".into(),
            source: Arc::from(TYPED_PROPS_SFC),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("typed-props fixture must index");
    host
}

#[test]
fn compile_with_audit_ide_publishes_record_with_ide_tag_and_tsx_block() {
    let host = build_host();
    let (result, record) = host
        .compile_with_audit("/i.vue", CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    // IDE target produces a TSX block, not a VDOM template block.
    assert!(
        result.tsx.is_some(),
        "IDE target must produce a TSX block; result.errors = {:?}",
        result.errors
    );

    // record is always present now (carrier `audit` field is mandatory).
    match &record.kind {
        verter_audit::RequestKind::Compile { target } => {
            assert_eq!(
                *target,
                CompileTargetTag::Ide,
                "IDE target ⇒ Ide tag — discriminates against a regression that \
                 always tags as Vdom regardless of the bitflags input",
            );
        }
        other => panic!("expected RequestKind::Compile, got {other:?}"),
    }

    let payload = record
        .compile_payload()
        .expect("Compile kind ⇒ kind_payload must carry CompilePayload");
    assert_eq!(payload.target, CompileTargetTag::Ide);
    // IDE codegen heavily uses CodeTransform — non-zero op count
    // confirms TLS observer reached the producer crate.
    assert!(
        payload.code_transform_ops > 0,
        "IDE codegen must observe CodeTransform ops; payload = {payload:?}",
    );
    // IDE codegen emits TSX bytes — output_bytes must include them.
    assert!(
        payload.output_bytes > 0,
        "IDE compile must report non-zero output_bytes; payload = {payload:?}",
    );
}

#[test]
fn compile_with_audit_ide_routes_authoritative_runtime_props_to_tsx_bindings() {
    let host = build_typed_props_host();
    let (result, _record) = host
        .compile_with_audit("/typed.vue", CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(result) => result,
        Err(never) => match never {},
    };

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = result.tsx.expect("IDE target must emit TSX").code;
    assert!(
        code.contains("__props.title"),
        "the session's authoritative runtime DTO must reach IDE binding ownership: {code}"
    );
    assert!(
        !code.contains("_ctx.title"),
        "typed props must not degrade to context bindings: {code}"
    );
}
