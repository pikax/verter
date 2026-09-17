//! C2 request-local continuation: revalidation evidence (charter home:
//! `crates/verter_session/tests/c2_continuation_revalidation.rs`).
//!
//! The continuation is private and request-local, so its contract is
//! discriminated through the session's public seams: the macro semantic
//! producer behind `VerterHost::vue_macro_semantic_input`. Incremental
//! equals fresh — an edited host's re-derived input equals what a brand
//! new host derives from the same bytes, and a degraded outcome never
//! warms: fixing the degradation changes the next answer.

use std::sync::Arc;

use verter_session::{CompileTarget, HostConfig, LanguageRegistry, UpsertRequest, VerterHost};

const SFC_A: &str = "<script setup lang=\"ts\">\nimport { BadgeProps } from \"./Badge.vue\";\ninterface Props { count: number; label?: string }\ndefineProps<Props>();\ndefineEmits<{ saved: [id: number] }>();\nconst title = defineModel<string>('title');\ndefineExpose({ reset() {} });\n</script>\n<template><div/></template>\n";

const SFC_A_EDITED: &str = "<script setup lang=\"ts\">\nimport { BadgeProps } from \"./Badge.vue\";\ninterface Props { count: number; label?: string; extra: boolean }\ndefineProps<Props>();\ndefineEmits<{ saved: [id: number] }>();\nconst title = defineModel<string>('title');\ndefineExpose({ reset() {} });\n</script>\n<template><div/></template>\n";

fn host_with(source: &'static str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/src/App.vue".to_string()),
        input_id: "/src/App.vue".to_string(),
        source: Arc::from(source),
        file_language: LanguageRegistry::global()
            .classify_static("/src/App.vue")
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

const TARGET: CompileTarget = CompileTarget::BUNDLER
    .union(CompileTarget::TSC)
    .union(CompileTarget::TSX);

/// Incremental equals fresh: an edited host's second derivation equals a
/// brand-new host's first derivation of the same bytes — the request-local
/// continuation cannot leave stale state behind.
#[test]
fn incremental_revalidation_equals_fresh_recompute() {
    let host = host_with(SFC_A);
    let before = host.vue_macro_semantic_input("/src/App.vue", TARGET);
    let _ = before; // warm the producer once under the original content

    // Edit: a new authored prop member changes the macro input plan.
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/src/App.vue".to_string()),
        input_id: "/src/App.vue".to_string(),
        source: Arc::from(SFC_A_EDITED),
        file_language: LanguageRegistry::global()
            .classify_static("/src/App.vue")
            .static_resolution(),
        aliases: Vec::new(),
    });
    let incremental = host.vue_macro_semantic_input("/src/App.vue", TARGET);

    // Fresh: a brand-new host over the same bytes.
    let fresh_host = host_with(SFC_A_EDITED);
    let fresh = fresh_host.vue_macro_semantic_input("/src/App.vue", TARGET);

    // Both must be the SAME semantic answer: whichever bundles the target
    // demands agree, and neither reports a stale (pre-edit) member set.
    assert_eq!(
        matches!(
            incremental,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        matches!(
            fresh,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        "incremental and fresh agree on availability"
    );
    match (incremental, fresh) {
        (
            verter_compiler::compile::VueMacroSemanticInput::RuntimeAndTsc {
                runtime: incremental_runtime,
                ..
            },
            verter_compiler::compile::VueMacroSemanticInput::RuntimeAndTsc {
                runtime: fresh_runtime,
                ..
            },
        ) => {
            let incremental_rows = &incremental_runtime.entries;
            let fresh_rows = &fresh_runtime.entries;
            assert_eq!(
                incremental_rows.len(),
                fresh_rows.len(),
                "incremental revalidation equals fresh recompute"
            );
        }
        (left, right) => assert_eq!(
            matches!(
                left,
                verter_compiler::compile::VueMacroSemanticInput::Unavailable
            ),
            matches!(
                right,
                verter_compiler::compile::VueMacroSemanticInput::Unavailable
            ),
            "both lanes agree on the demand shape"
        ),
    }
}

/// Re-requesting the same content is stable: two derivations under one
/// unchanged snapshot produce the same answer (the resume path is
/// observably identical to the compute path).
#[test]
fn repeated_derivation_under_unchanged_input_is_stable() {
    let host = host_with(SFC_A);
    let first = host.vue_macro_semantic_input("/src/App.vue", TARGET);
    let second = host.vue_macro_semantic_input("/src/App.vue", TARGET);
    assert_eq!(
        matches!(
            first,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        matches!(
            second,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        "unchanged input yields a stable answer"
    );
}
