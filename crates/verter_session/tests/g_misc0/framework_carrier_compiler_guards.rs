//! Static architecture guards for the compiler-side carrier framework
//! scaffold.
//!
//! - `framework_codegen_uses_code_transform` — the carrier compilers'
//!   IDE codegen path produces generated code through `CodeTransform`
//!   (delegating to the compiler's `compile`/`compile_from_parsed`
//!   pipeline, whose unified `CodeTransform` is the single source of
//!   truth for generated-code edits) and NEVER post-hoc string-munges the
//!   built output. Seeded with a negative self-test.
//! - `carrier_descriptors_have_compilers` — every carrier-bearing session
//!   descriptor has a registered `CarrierCompiler` in the compiler-side
//!   registry (Vue-through-the-bridge satisfies it). RED if the registry
//!   lands without the Vue bridge registration.
//!
//! Each guard is a discriminating check: it FAILS against a tree that
//! violates the rule and PASSES against the landed final-state tree.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crate")
        .to_path_buf()
}

fn read_src(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read `{}`: {err}", path.display()))
}

/// The post-hoc string-munging patterns forbidden on built codegen output
/// inside the carrier compiler IDE path. A carrier compiler must emit
/// generated code through `CodeTransform`, not by mutating the
/// already-built string.
///
/// Each pattern is a NON-COMMENT detector: it matches code that takes a
/// built compile result's string and rewrites it (`.replace(`,
/// `.replacen(`), or synthesises-then-reparses (`format!(...).parse`).
fn post_hoc_munging_detectors() -> &'static [&'static str] {
    &[".replace(", ".replacen(", ".parse_type_annotation("]
}

/// Strip `//` line comments and `///` doc comments so the detectors only
/// scan live code (the doc prose legitimately mentions "string munging").
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            // Preserve string literals well enough for this guard: the
            // forbidden tokens never appear inside a `//`-prefixed comment
            // we care about, so a naive split at the first `//` outside an
            // obvious string is sufficient for a source-scan guard.
            match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `code` (already comment-stripped) post-hoc munges built output.
fn contains_post_hoc_munging(code: &str) -> bool {
    post_hoc_munging_detectors()
        .iter()
        .any(|pat| code.contains(pat))
}

#[test]
fn framework_codegen_uses_code_transform() {
    // The carrier-compiler IDE path lives in the compiler-side bridge(s).
    // Today the one carrier compiler is the Vue bridge, which delegates
    // its IDE codegen to `compile_from_parsed` (the unified-CodeTransform
    // pipeline) and lifts the result verbatim — no post-hoc munging.
    let bridge = strip_line_comments(&read_src(
        "crates/verter_compiler/src/framework_common/vue_bridge.rs",
    ));
    assert!(
        !contains_post_hoc_munging(&bridge),
        "the Vue carrier bridge must not post-hoc string-munge built codegen output \
         (`.replace`, `.replacen`, synthesise-then-reparse) — generated code edits go \
         through the pipeline's CodeTransform"
    );

    // It MUST reach codegen through the CodeTransform-backed pipeline.
    assert!(
        bridge.contains("compile_from_parsed"),
        "the Vue carrier bridge's compile_ide must delegate to the compiler's \
         `compile_from_parsed` (CodeTransform-backed) pipeline"
    );

    // The trait + neutral I/O vocabulary must NOT expose a post-build
    // string-rewrite surface either.
    let carrier = strip_line_comments(&read_src(
        "crates/verter_compiler/src/framework_common/carrier_compiler.rs",
    ));
    assert!(
        !contains_post_hoc_munging(&carrier),
        "the CarrierCompiler trait/IO module must not post-hoc string-munge codegen output"
    );
}

#[test]
fn framework_codegen_uses_code_transform_negative_self_test() {
    // A synthetic carrier-compiler body that post-hoc rewrites the built
    // TSX string MUST be caught by the detector — proving the guard
    // discriminates rather than passing vacuously.
    let offending = r#"
        fn compile_ide(&self, source: &str) -> String {
            let result = compile_from_parsed(source);
            // Post-hoc munging on the built string — forbidden.
            result.tsx.unwrap().code.replace(".vue'", ".vue.ts'")
        }
    "#;
    let stripped = strip_line_comments(offending);
    assert!(
        contains_post_hoc_munging(&stripped),
        "the detector must catch a `.replace(` rewrite of built codegen output"
    );

    // And a clean delegation body must NOT trip it.
    let clean = r#"
        fn compile_ide(&self, source: &str) -> IdeOutput {
            let result = compile_from_parsed(source);
            IdeOutput { code: result.tsx.unwrap().code }
        }
    "#;
    assert!(
        !contains_post_hoc_munging(&strip_line_comments(clean)),
        "a clean delegation that lifts the pipeline's CodeTransform output must pass"
    );
}

#[test]
fn carrier_descriptors_have_compilers() {
    // The compiler-completeness leg split out of B5's
    // `framework_registry_complete`: every carrier-bearing session
    // descriptor (a descriptor whose `carrier_language` is `Some`) MUST
    // have a registered `CarrierCompiler` in the compiler-side registry.
    use verter_compiler::framework_common::CarrierCompilerRegistry;
    use verter_session::framework::descriptor::vue_descriptor;

    let registry = CarrierCompilerRegistry::built_in();

    // Vue is the keystone carrier descriptor: it carries a carrier
    // language, so it MUST have a registered compiler (the bridge).
    let vue = vue_descriptor();
    assert!(
        vue.carrier_language.is_some(),
        "the Vue descriptor is carrier-bearing"
    );
    assert!(
        registry.contains(&vue.id),
        "a carrier-bearing descriptor (Vue) MUST have a registered CarrierCompiler — \
         the registry landed without the Vue bridge registration"
    );

    // The registered compiler answers to the descriptor's adapter id.
    let compiler = registry
        .get(&vue.id)
        .expect("Vue carrier compiler registered");
    assert_eq!(
        compiler.adapter_id(),
        vue.id,
        "the registered compiler's adapter id must match the descriptor's"
    );
}
