//! `VerterHost::compile_with_audit` is the VUE-ONLY audited compile path.
//!
//! `compile_with_audit` drives the hardcoded Vue SFC runtime compiler
//! (`verter_compiler::compile::compile`) directly — NOT the framework-neutral
//! carrier registry. It therefore FAILS CLOSED on a non-Vue framework carrier
//! (a `.svelte` file) with a typed `VerterE001` diagnostic rather than silently
//! Vue-compiling it, so a `.svelte` request through the audited path can never
//! produce wrong (Vue-shaped) output.

use std::sync::Arc;

use verter_compiler::compile::CompileTarget;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const VUE_SFC: &str = "<script setup lang=\"ts\">\n\
                       const count = 0;\n\
                       </script>\n\
                       <template><button>{{ count }}</button></template>\n";

const SVELTE_SFC: &str =
    "<script lang=\"ts\">let count = 0;</script>\n<button onclick={() => count++}>{count}</button>\n";

fn build_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ))
}

/// A `.svelte` carrier through `compile_with_audit` must FAIL CLOSED with a
/// typed `VerterE001` diagnostic — it must NOT be silently Vue-compiled.
///
/// DISCRIMINATING: before the Vue-only guard, `compile_with_audit` drove
/// `verter_compiler::compile::compile` on the raw Svelte source, producing WRONG
/// (Vue-shaped) output (e.g. a `tsx` block from the Vue SFC compiler) and NO
/// `VerterE001` error. This test FAILS against that state (it would see no
/// `VerterE001` and a non-empty Vue output) and PASSES with the guard (the
/// typed error, no Vue-compiled bytes).
#[test]
fn compile_with_audit_rejects_a_non_vue_carrier_with_a_typed_error() {
    let host = build_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Counter.svelte".into()),
        input_id: "/Counter.svelte".into(),
        source: Arc::from(SVELTE_SFC),
        file_language: FileLanguage::svelte(),
        aliases: Vec::new(),
    });

    let (result, _record) = host
        .compile_with_audit("/Counter.svelte", CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    // The Vue-only guard fails closed with the typed diagnostic …
    assert!(
        result.errors.iter().any(|d| d.code == "VerterE001"),
        "a non-Vue carrier (.svelte) must be rejected from the Vue-only audited compile path with \
         a typed VerterE001 error; got errors = {:?}",
        result.errors
    );
    // … and produces NO silently-Vue-compiled output.
    assert!(
        result.tsx.is_none(),
        "the Vue-only guard must NOT silently Vue-compile a .svelte file into a TSX block; \
         got tsx = {:?}",
        result.tsx.is_some()
    );
    assert!(
        result.script.is_none() && result.template.is_none(),
        "the Vue-only guard must emit no Vue script/template blocks for a .svelte carrier"
    );
}

/// A `.vue` carrier still compiles successfully through the same path — the
/// Vue-only guard does NOT regress the Vue case (discrimination floor).
#[test]
fn compile_with_audit_still_compiles_a_vue_carrier() {
    let host = build_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/App.vue".into()),
        input_id: "/App.vue".into(),
        source: Arc::from(VUE_SFC),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });

    let (result, _record) = host
        .compile_with_audit("/App.vue", CompileTarget::IDE)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    assert!(
        !result.errors.iter().any(|d| d.code == "VerterE001"),
        "a .vue carrier must NOT be rejected by the Vue-only guard; got errors = {:?}",
        result.errors
    );
    assert!(
        result.tsx.is_some(),
        "the Vue carrier must still produce a TSX block through compile_with_audit"
    );
}
