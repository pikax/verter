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
    // compiler module, not a single hardcoded bridge. GENERALIZED: scan
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
    // GENERALIZED: iterate every built-in descriptor and filter the
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
    // F13 + F9: the `$events` / `$slots` shim members are rendered as STATIC TYPE
    // TEXT (a derived mapped type + an exact key map) — TSGO resolves them at
    // check time. The render adds NO dispatch / OXC (covered by the detector scan
    // above); these positive assertions pin that the new surfaces are present and
    // dispatch-free string renders.
    assert!(
        projector.contains("__VerterEventsSurface") && projector.contains("__VerterSlotsSurface"),
        "the Svelte api-projector must render the $events / $slots shim surfaces"
    );
    assert!(
        projector.contains("__VerterCallbackEvents"),
        "the $events surface is the DERIVED callback-prop mapped type (TSGO resolves it)"
    );
    // NEGATIVE: the shim must NOT emit a loose `CustomEvent<any>` / `Record<…>`
    // placeholder for the event/slot surfaces.
    assert!(
        !projector.contains("CustomEvent<any>"),
        "the Svelte api-projector must not emit a loose CustomEvent<any> event surface"
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

#[test]
fn svelte_component_on_directive_not_loose_rewrite() {
    // F13: the IDE projector's `On` directive arm MUST disambiguate component vs
    // intrinsic — a COMPONENT-kind element routes the checked `__verter_event`
    // helper (payload-checked), while an INTRINSIC element keeps the verbatim DOM
    // `onevent` rewrite. The loose `on:`→`onclick` verbatim rewrite must NOT be
    // applied unconditionally to every element.
    let projector = strip_line_comments(&read_src(
        "crates/verter_compiler/src/svelte/ide/projector/mod.rs",
    ));
    // The `On` arm branches on `SvelteElementKind::Component` and routes the
    // checked component-event helper for components.
    assert!(
        projector.contains("rewrite_component_on_event"),
        "the projector must route a COMPONENT `on:` through the checked event helper"
    );
    assert!(
        projector.contains("__verter_event("),
        "the component `on:` projection must emit the `__verter_event` checked call"
    );
    // The intrinsic DOM rewrite is preserved (the `on:`→`onevent` overwrite).
    assert!(
        projector.contains("rewrite_legacy_on"),
        "the projector must keep the intrinsic DOM `on:`→`onevent` rewrite"
    );
    // DISCRIMINATING: the `On` arm must consult the element kind — an
    // unconditional `rewrite_legacy_on` for ALL elements (the retired loose path)
    // is forbidden. The arm guards the component route on the element kind.
    let on_arm = projector
        .split("SvelteDirectiveKind::On =>")
        .nth(1)
        .and_then(|rest| rest.split("SvelteDirectiveKind::Class").next())
        .unwrap_or("");
    assert!(
        on_arm.contains("SvelteElementKind::Component"),
        "the `On` directive arm must disambiguate the COMPONENT element kind \
         (not unconditionally apply the loose DOM rewrite):\n{on_arm}"
    );
}

#[test]
fn no_custom_event_any_or_record_string_any_in_svelte_surfaces() {
    // F13 + F9: NO loose `CustomEvent<any>` / `Record<string, *>` placeholder may
    // appear in the Svelte `$events` / `$slots` projection or resolution surfaces.
    // The event/slot surfaces are precise (an exact event/slot map with typed
    // payload/binding nodes), never an untyped bag. The legacy `$$props`/
    // `$$restProps` magic objects are the ONLY documented `Record<string, any>`
    // carve-out (F12) and live behind the `LEGACY_MAGIC_PRELUDE` — scanning the
    // event/slot-bearing files here excludes them.
    let svelte_surface_files = [
        "crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs",
        "crates/verter_session/src/framework/api_projectors/svelte.rs",
        "crates/verter_session/src/resolver_core/svelte_default_synth.rs",
    ];
    for rel in svelte_surface_files {
        let src = strip_line_comments(&read_src(rel));
        assert!(
            !src.contains("CustomEvent<any>"),
            "{rel} must not emit a loose `CustomEvent<any>` event surface"
        );
        assert!(
            !src.contains("Record<string, any>") && !src.contains("Record<string,any>"),
            "{rel} must not emit a loose `Record<string, any>` event/slot surface"
        );
        assert!(
            !src.contains("Record<string, boolean>"),
            "{rel} must not emit a loose `Record<string, boolean>` slot-presence bag"
        );
    }
}

#[test]
fn no_custom_event_any_detector_discriminates() {
    // The loose-surface detector must catch a synthetic source emitting
    // `CustomEvent<any>` / `Record<string, any>` — proving the guard
    // discriminates rather than passing vacuously.
    let offending = "fn render() -> String { \"$events: Record<string, any>\".to_string() }";
    let stripped = strip_line_comments(offending);
    assert!(
        stripped.contains("Record<string, any>"),
        "the detector must catch a loose Record<string, any> surface"
    );
    let offending2 = "fn render() -> String { \"on:select -> CustomEvent<any>\".to_string() }";
    assert!(
        strip_line_comments(offending2).contains("CustomEvent<any>"),
        "the detector must catch a loose CustomEvent<any> surface"
    );
}

#[test]
fn template_data_ingestion_is_registry_dispatched() {
    // Shared-substrate rule: template-data ingestion is REGISTRY-DISPATCHED
    // by the file's carrier row, NOT gated on a hardcoded `.vue` / `is_vue()`
    // check. The `compute_template_analysis_if_missing` body and the
    // `build_template_analysis` body must route the extraction through the
    // carrier registry (`file_language_has_template_data_compiler` +
    // `compile_template_data`), and must NOT contain a `.vue` literal gate or an
    // `is_vue()` gate on the template-data path.
    let analysis_io = strip_line_comments(&read_src(
        "crates/verter_session/src/host_manage/analysis_io.rs",
    ));

    // The registry-dispatched gate + extraction must be present.
    assert!(
        analysis_io.contains("file_language_has_template_data_compiler"),
        "the template-data ingestion gate must be the registry-dispatched \
         `file_language_has_template_data_compiler`, not a hardcoded carrier check"
    );
    assert!(
        analysis_io.contains("compile_template_data("),
        "template-data extraction must route through the carrier-neutral \
         `compile_template_data` (registry-dispatched), not a Vue-only path"
    );

    // The retired Vue-only gates must NOT reappear on the template-data path.
    // Scope the scan to the two ingestion bodies so an unrelated `.vue` literal
    // elsewhere in the (large) file does not false-positive.
    for marker in [
        "fn compute_template_analysis_if_missing",
        "fn build_template_analysis",
    ] {
        let body = analysis_io
            .split(marker)
            .nth(1)
            .and_then(|rest| rest.split("\n    pub").next().or(Some(rest)))
            .unwrap_or("");
        assert!(
            !body.contains("ends_with(\".vue\")"),
            "`{marker}` must not gate the template-data path on a `.vue` literal \
             (registry-dispatched ingestion):\n{body}"
        );
        assert!(
            !body.contains("file_language.is_vue()") && !body.contains("hd.file_language.is_vue()"),
            "`{marker}` must not gate the template-data path on `is_vue()` \
             (registry-dispatched ingestion):\n{body}"
        );
    }

    // The retired Vue-only extraction helper must be GONE (no dual path/shim).
    let parse = strip_line_comments(&read_src("crates/verter_session/src/parse.rs"));
    assert!(
        !parse.contains("fn compile_vue_template_data"),
        "the Vue-only `compile_vue_template_data` must be retired in favour of the \
         carrier-neutral `compile_template_data` — no dual path"
    );
}

#[test]
fn template_data_ingestion_registry_dispatch_detector_discriminates() {
    // The detector must catch a synthetic ingestion body that re-introduces the
    // retired Vue-only gate — proving the guard discriminates.
    let offending = r#"
        fn compute_template_analysis_if_missing(&self) {
            if !canonical.ends_with(".vue") { return; }
        }
    "#;
    let stripped = strip_line_comments(offending);
    let body = stripped
        .split("fn compute_template_analysis_if_missing")
        .nth(1)
        .unwrap_or("");
    assert!(
        body.contains("ends_with(\".vue\")"),
        "the detector must catch a `.vue` literal gate on the template-data path"
    );
}

#[test]
fn svelte_template_data_producer_is_typed_ir_only() {
    // Producer rule: the Svelte `template_data` producer walks the typed
    // `ParsedSvelte` template tree (typed-IR / typed template AST) and may
    // span-slice the carrier source for expression TEXT only. It must NOT
    // STRUCTURALLY scan the source — no regex, no `find`/`contains`-driven tag
    // discovery, no synthesise-then-reparse, no type lowering.
    let producer = strip_line_comments(&read_src(
        "crates/verter_compiler/src/svelte/template_facts.rs",
    ));

    // Positive: it walks the typed AST node families (structural classification
    // by KIND, not by source scanning).
    assert!(
        producer.contains("SvelteNode::Element")
            && producer.contains("SvelteNode::Block")
            && producer.contains("SvelteElementKind::Component"),
        "the producer must classify components structurally off the typed AST"
    );
    assert!(
        producer.contains("collect_component_usages"),
        "the producer must be the structural recursive walk `collect_component_usages`"
    );

    // NEGATIVE: no structural source scanning / reparse / type lowering inside
    // the producer.
    for forbidden in [
        "parse_type_annotation",
        "lower_ts_type",
        "Regex",
        "regex::",
        ".find(\"<\")",
        "split_top_level",
        "ProjectSemanticDispatch",
        "Parser::new(",
    ] {
        assert!(
            !producer.contains(forbidden),
            "the Svelte template-data producer must not `{forbidden}` — it walks the \
             typed `ParsedSvelte` tree and span-slices for expression TEXT only"
        );
    }
}

#[test]
fn svelte_template_data_producer_typed_ir_detector_discriminates() {
    // The detector must catch a synthetic producer that re-introduces a
    // source-scan / reparse / type-lowering path — proving the guard
    // discriminates rather than always passing.
    let offending = r#"
        fn collect_component_usages(source: &str) {
            let ty = parse_type_annotation(source);
            let _ = lower_ts_type(&ty);
        }
    "#;
    let stripped = strip_line_comments(offending);
    let mut tripped = false;
    for forbidden in ["parse_type_annotation", "lower_ts_type"] {
        if stripped.contains(forbidden) {
            tripped = true;
        }
    }
    assert!(
        tripped,
        "the typed-IR producer guard must catch a reparse / type-lowering path \
         inside the producer"
    );
}

#[test]
fn svelte_surface_source_exhaustive() {
    // The closed `SvelteSurfaceSource` enum (including the new `CallbackPropEvents`
    // family) is matched EXHAUSTIVELY across the resolution leg — adding a family
    // forces an acknowledgement at every match site. The `store_kind_for_source`
    // mapping and the `compute_svelte_surface` dispatch both match without a
    // wildcard, and every family maps to exactly one wire kind.
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    use verter_session::typeinfo::framework_surface::SvelteSurfaceSource;

    let all = [
        SvelteSurfaceSource::RunesProps,
        SvelteSurfaceSource::LegacyExportLet,
        SvelteSurfaceSource::Bindable,
        SvelteSurfaceSource::SnippetProps,
        SvelteSurfaceSource::LegacySlotInventory,
        SvelteSurfaceSource::LegacyDispatcher,
        SvelteSurfaceSource::CallbackPropEvents,
        SvelteSurfaceSource::InstanceExports,
    ];
    // Exhaustive match — adding a family breaks this (no wildcard).
    for source in all {
        let kind = match source {
            SvelteSurfaceSource::RunesProps | SvelteSurfaceSource::LegacyExportLet => {
                FrameworkSurfaceKind::Props
            }
            SvelteSurfaceSource::Bindable => FrameworkSurfaceKind::Model,
            SvelteSurfaceSource::SnippetProps | SvelteSurfaceSource::LegacySlotInventory => {
                FrameworkSurfaceKind::Slots
            }
            SvelteSurfaceSource::LegacyDispatcher | SvelteSurfaceSource::CallbackPropEvents => {
                FrameworkSurfaceKind::Emits
            }
            SvelteSurfaceSource::InstanceExports => FrameworkSurfaceKind::Expose,
        };
        // The new callback-prop event source maps to EMITS (the derived index).
        if matches!(source, SvelteSurfaceSource::CallbackPropEvents) {
            assert_eq!(kind, FrameworkSurfaceKind::Emits);
        }
    }
    // The source-leg dispatch matches the new family WITHOUT a wildcard.
    let leg = strip_line_comments(&read_src(
        "crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs",
    ));
    assert!(
        leg.contains("SvelteSurfaceSource::CallbackPropEvents =>"),
        "the resolution leg must dispatch the CallbackPropEvents family explicitly"
    );
}
