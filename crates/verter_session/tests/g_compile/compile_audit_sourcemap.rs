//! Slice 3.B test: sourcemap phase timing + sourcemap_bytes surface
//! through `CompilePayload::{sourcemap_ms, sourcemap_bytes}` when the
//! producer generates a sourcemap.
//!
//! `compile_with_audit` defaults `verter_options.source_map = true`,
//! so the script + template paths both run sourcemap generation. The
//! producer emits `record_phase_timing("compile.sourcemap", ...)` only
//! when `verter_options.source_map = true`, so the audit-disabled
//! variant of `compile_with_audit_options` exercises the negative case.

use std::sync::Arc;

use verter_compiler::compile::{CompileTarget, VerterCompileOptions};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   const greeting = 'hi';\n\
                   </script>\n\
                   <template><div>{{ greeting }}</div></template>\n";

fn host_for() -> Arc<VerterHost> {
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
        canonical_id: Some("/sm.vue".into()),
        input_id: "/sm.vue".into(),
        source: Arc::from(SFC),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });
    host
}

#[test]
fn compile_with_audit_populates_sourcemap_ms_and_bytes_when_source_map_enabled() {
    let host = host_for();
    // Default `compile_with_audit` enables source_map.
    let (result, record) = host
        .compile_with_audit("/sm.vue", CompileTarget::BUNDLER)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    let script_block = result
        .script
        .as_ref()
        .expect("setup SFC must produce a script block");
    assert!(
        !script_block.source_map.is_empty(),
        "source_map=true must produce a non-empty script source_map; \
         got len={}",
        script_block.source_map.len()
    );

    let payload = record.compile_payload().cloned().expect("CompilePayload");

    assert!(
        payload.sourcemap_ms.is_some(),
        "source_map=true ⇒ producer must emit `compile.sourcemap` phase timing. \
         A regression that drops the emit would leave this None. payload = {payload:?}",
    );
    let sm_ms = payload.sourcemap_ms.unwrap();
    assert!(
        sm_ms >= 0.0,
        "sourcemap_ms must be non-negative; got {sm_ms}"
    );
    assert!(
        payload.sourcemap_bytes > 0,
        "sourcemap_bytes must be > 0 when source_map produces content; \
         got {}",
        payload.sourcemap_bytes,
    );
}

#[test]
fn compile_with_audit_options_leaves_sourcemap_ms_none_when_source_map_disabled() {
    let host = host_for();
    // Explicit `source_map: false` short-circuits the producer emit.
    let opts = VerterCompileOptions {
        source_map: false,
        ..VerterCompileOptions::default()
    };
    let (result, record) = host
        .compile_with_audit_options("/sm.vue", CompileTarget::BUNDLER, opts)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    let script_block = result
        .script
        .as_ref()
        .expect("setup SFC must produce a script block");
    assert!(
        script_block.source_map.is_empty(),
        "source_map=false must produce an empty source_map string",
    );

    let payload = record.compile_payload().cloned().expect("CompilePayload");

    // Negative discriminator: source_map=false ⇒ no emit ⇒ accumulator
    // stays 0 ⇒ payload reports None.
    assert!(
        payload.sourcemap_ms.is_none(),
        "source_map=false must leave sourcemap_ms = None; \
         a producer that emits unconditionally would surface Some(0.0) here. \
         payload = {payload:?}",
    );
    assert_eq!(
        payload.sourcemap_bytes, 0,
        "no sourcemap content written ⇒ 0 bytes"
    );
}
