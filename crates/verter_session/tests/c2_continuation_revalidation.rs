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

/// The authored public prop names of the props macro in one runtime
/// bundle — the member set a stale pre-edit answer would get wrong.
fn props_names(bundle: &verter_macro_dto::MacroRuntimeBundle) -> Vec<String> {
    bundle
        .entries
        .iter()
        .filter_map(|entry| match &entry.outcome {
            verter_macro_dto::MacroRuntimeOutcome::Complete(
                verter_macro_dto::MacroRuntimeShape::Props(shape),
            ) => Some(
                shape
                    .props
                    .iter()
                    .map(|prop| prop.name.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .concat()
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

    // Both must be the SAME semantic answer: the complete runtime and tsc
    // payloads are equal, and neither reports a stale (pre-edit) member
    // set — the new `extra` prop row is present on BOTH sides.
    match (incremental, fresh) {
        (
            verter_compiler::compile::VueMacroSemanticInput::RuntimeAndTsc {
                runtime: incremental_runtime,
                tsc: incremental_tsc,
            },
            verter_compiler::compile::VueMacroSemanticInput::RuntimeAndTsc {
                runtime: fresh_runtime,
                tsc: fresh_tsc,
            },
        ) => {
            assert_eq!(
                incremental_runtime, fresh_runtime,
                "incremental revalidation equals fresh recompute on the complete runtime bundle"
            );
            assert_eq!(
                incremental_tsc, fresh_tsc,
                "incremental revalidation equals fresh recompute on the complete tsc bundle"
            );
            assert_eq!(
                props_names(&incremental_runtime),
                vec![
                    "count".to_string(),
                    "label".to_string(),
                    "extra".to_string()
                ],
                "the incremental answer carries the post-edit member set"
            );
            assert_eq!(
                props_names(&fresh_runtime),
                vec![
                    "count".to_string(),
                    "label".to_string(),
                    "extra".to_string()
                ],
                "the fresh answer carries the post-edit member set"
            );
        }
        (left, right) => {
            panic!("expected RuntimeAndTsc for both derivations, got {left:?} and {right:?}")
        }
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
    // Stability is over the WHOLE payload, not just availability: the
    // resumed answer equals the computed one member-for-member.
    assert_eq!(
        first.runtime(),
        second.runtime(),
        "the resumed runtime bundle equals the computed one"
    );
    assert_eq!(
        first.tsc(),
        second.tsc(),
        "the resumed tsc bundle equals the computed one"
    );
    assert_eq!(
        matches!(
            &first,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        matches!(
            &second,
            verter_compiler::compile::VueMacroSemanticInput::Unavailable
        ),
        "unchanged input yields a stable answer"
    );
}
