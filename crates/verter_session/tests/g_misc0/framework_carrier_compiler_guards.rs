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
//! - `non_vue_api_projector_has_no_dispatch_or_oxc` — a NON-Vue api-projector
//!   leg (the Svelte declaration-shim renderer) renders PURELY over cached
//!   shallow state: it must NOT call `ProjectSemanticDispatch` / `Instantiate`,
//!   run OXC at render time, or reach query-time resolution. The Vue leg is the
//!   sole exemption (it delegates to the deep legacy extraction body).
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

/// Recursively collect `.rs` source under a directory (skipping `*_tests.rs`
/// sibling files, which are test prose, not codegen code).
fn collect_rs_recursive(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with("_tests.rs") {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[test]
fn framework_codegen_uses_code_transform() {
    // The carrier-compiler IDE codegen lives across EVERY carrier-bearing
    // compiler module, not a single hardcoded bridge. GENERALIZED (B8a): scan
    // ALL of `framework_common/` PLUS each framework's `src/<framework>/**`
    // (Vue's bridge under framework_common, Svelte's carrier under
    // `src/svelte/**`). None may post-hoc string-munge built codegen output.
    let root = workspace_root();
    let mut scanned = Vec::new();
    scanned.extend(collect_rs_recursive(
        &root.join("crates/verter_compiler/src/framework_common"),
    ));
    scanned.extend(collect_rs_recursive(
        &root.join("crates/verter_compiler/src/svelte"),
    ));
    assert!(
        scanned.len() >= 3,
        "the scan must cover framework_common + the svelte carrier modules, got {}",
        scanned.len()
    );

    for path in &scanned {
        let code = strip_line_comments(
            &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}")),
        );
        assert!(
            !contains_post_hoc_munging(&code),
            "{path:?} must not post-hoc string-munge built codegen output \
             (`.replace`, `.replacen`, synthesise-then-reparse) — generated code edits go \
             through the pipeline's CodeTransform"
        );
    }

    // The Vue bridge MUST reach codegen through the CodeTransform-backed
    // pipeline (the positive half — a carrier with an IDE projection delegates,
    // never hand-rolls a string).
    let bridge = strip_line_comments(&read_src(
        "crates/verter_compiler/src/framework_common/vue_bridge.rs",
    ));
    assert!(
        bridge.contains("compile_from_parsed"),
        "the Vue carrier bridge's compile_ide must delegate to the compiler's \
         `compile_from_parsed` (CodeTransform-backed) pipeline"
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
    // `framework_registry_complete`: EVERY carrier-bearing session
    // descriptor (a descriptor whose `carrier_language` is `Some`) MUST
    // have a registered `CarrierCompiler` in the compiler-side registry.
    //
    // GENERALIZED (B8a): iterate every built-in descriptor and filter the
    // carrier-bearing rows, rather than hardcoding Vue. A new carrier vertical's
    // descriptor is covered automatically; if Svelte's carrier registers without
    // a compiler, this is RED.
    use verter_compiler::framework_common::CarrierCompilerRegistry;
    use verter_session::framework::descriptor::built_in_descriptors;

    let registry = CarrierCompilerRegistry::built_in();

    let carrier_bearing: Vec<_> = built_in_descriptors()
        .into_iter()
        .filter(|d| d.carrier_language.is_some())
        .collect();
    assert!(
        carrier_bearing.len() >= 2,
        "at least Vue and Svelte are carrier-bearing descriptors, got {}",
        carrier_bearing.len()
    );

    for descriptor in &carrier_bearing {
        assert!(
            registry.contains(&descriptor.id),
            "the carrier-bearing descriptor `{}` MUST have a registered CarrierCompiler — \
             a carrier registered without a compiler",
            descriptor.id
        );
        // The registered compiler answers to the descriptor's adapter id, AND it
        // serves the descriptor's carrier language (the full carrier-row gate).
        let carrier_language = descriptor
            .carrier_language
            .as_ref()
            .expect("filtered to carrier-bearing");
        let compiler = registry
            .compiler_for_carrier_language(&descriptor.id, carrier_language)
            .unwrap_or_else(|| {
                panic!(
                    "carrier-bearing descriptor `{}` ({}) has no compiler for its carrier language",
                    descriptor.id, carrier_language
                )
            });
        assert_eq!(
            compiler.adapter_id(),
            descriptor.id,
            "the registered compiler's adapter id must match the descriptor's"
        );
    }
}

/// The render-time dispatch / OXC / query-resolution patterns forbidden inside a
/// NON-Vue api-projector leg. A non-Vue projector renders PURELY over already-
/// cached shallow state — it never re-resolves, re-parses, or dispatches.
fn projector_dispatch_detectors() -> &'static [&'static str] {
    &[
        ".dispatch(",
        "ProjectSemanticDispatch",
        "execute_type_node",
        "SemanticQueryKey::",
        "Instantiate",
        "oxc_parser::",
        "Parser::new(",
        "lower_ts_type",
        "resolve_vue_public_type",
        "project_shallow_surface_from_base",
    ]
}

#[test]
fn non_vue_api_projector_has_no_dispatch_or_oxc() {
    // The Svelte api-projector leg (the declaration-shim renderer) is a PURE
    // render over cached shallow state. It must NOT call dispatch / Instantiate,
    // run OXC, or reach query-time resolution at render time. The Vue leg is the
    // SOLE exemption (it delegates to the deep legacy extraction).
    let projector = strip_line_comments(&read_src(
        "crates/verter_session/src/framework/api_projectors/svelte.rs",
    ));
    for pattern in projector_dispatch_detectors() {
        assert!(
            !projector.contains(pattern),
            "the Svelte api-projector must not `{pattern}` at render time — it renders \
             purely over cached shallow state (no dispatch, no OXC, no query resolution)"
        );
    }
    // Positive: it MUST read the cached shallow state (the pure-render input).
    assert!(
        projector.contains("ensure_indexed_ready") || projector.contains("shallow_state"),
        "the Svelte api-projector must render over the cached shallow state"
    );
}

#[test]
fn non_vue_api_projector_dispatch_detector_discriminates() {
    // The detector must catch a synthetic projector body that re-dispatches —
    // proving the guard discriminates rather than passing vacuously.
    let offending = r#"
        fn render_api(&self, cx: Ctx) -> Option<Resp> {
            let node = self.dispatch().execute_type_node(key);
            None
        }
    "#;
    let stripped = strip_line_comments(offending);
    assert!(
        projector_dispatch_detectors()
            .iter()
            .any(|p| stripped.contains(p)),
        "the detector must catch a render-time dispatch call"
    );
}
