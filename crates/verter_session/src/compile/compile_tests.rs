//! Assembly tests for the Vue runtime main module.
//!
//! These assert on the assembled CODE. The assembler now returns that code
//! coupled to its source map, so each reaches through `.code`; what each
//! checks is unchanged, because composing a map alongside the code does not
//! change which bytes are produced.

use super::*;

// ═══════════════════════════════════════════════════════════
// assemble_main_module tests
// ═══════════════════════════════════════════════════════════

use verter_compiler::framework_common::{
    RuntimeCompileOutput, RuntimeCustomBlock, RuntimeOutputDescriptor, RuntimeScriptBlock,
    RuntimeStyleBlock, RuntimeTemplateBlock, SourceMapFidelity,
};

fn test_output_descriptor(code: &str) -> RuntimeOutputDescriptor {
    RuntimeOutputDescriptor::generated(
        code,
        None,
        &[("test:space", "test:artifact")],
        SourceMapFidelity::Approximate,
    )
}

fn basic_compiled_result() -> RuntimeCompileOutput {
    let script_code = "const __sfc__ = _defineComponent({\n  setup(__props) {\n    const n = 1;\n\nreturn { n };\n\n}});\nexport default __sfc__;\n";
    let template_code = "function render(_ctx, _cache, $props, $setup) {\n  return $setup.n\n}";
    RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: script_code.to_string(),
            source_map: String::new(),
            setup: true,
            output_descriptor: test_output_descriptor(script_code),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
        }),
        template: Some(RuntimeTemplateBlock {
            code: template_code.to_string(),
            source_map: String::new(),
            imports: vec!["_openBlock".to_string(), "_createElementBlock".to_string()],
            ssr_imports: vec![],
            output_descriptor: test_output_descriptor(template_code),
        }),
        ..RuntimeCompileOutput::default()
    }
}

/// @ai-generated - SSR profile skips HMR block
#[test]
fn assemble_main_module_ssr_skips_hmr() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        ssr: true,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(!result.contains("import.meta.hot"));
    assert!(!result.contains("module.hot"));
}

/// SSR must register the module on `ssrContext.modules` so Vite can collect
/// CSS/JS assets for the render tree (drop-in parity with plugin-vue).
#[test]
fn assemble_main_module_ssr_registers_ssr_context_module() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: true,
        ssr: true,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("src/Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    // Positive: wrap setup with useSSRContext + modules.add
    assert!(
        result.contains("useSSRContext as __vite_useSSRContext"),
        "must import useSSRContext, got:\n{result}"
    );
    assert!(
        result.contains("ssrContext.modules"),
        "must register on ssrContext.modules, got:\n{result}"
    );
    assert!(
        result.contains("\"src/Comp.vue\"") || result.contains("'src/Comp.vue'"),
        "must add the component path to modules set, got:\n{result}"
    );
    assert!(
        result.contains("const _sfc_setup = _sfc_main.setup"),
        "must preserve original setup, got:\n{result}"
    );
    // Negative: client HMR must not appear in SSR assembly
    assert!(!result.contains("import.meta.hot"));
    assert!(!result.contains("module.hot"));
}

/// `emit_ssr_module_registration: false` suppresses the whole
/// `useSSRContext`/`ssrContext.modules` wrapper — the conformance
/// harness's own candidate-generation shape (a bare `@vue/compiler-sfc`-
/// equivalent assembly, matching goldens generated the same way, with no
/// `@vitejs/plugin-vue` bundler-plugin glue at all). Every OTHER field
/// stays at its default, proving this is a narrow, single-purpose knob —
/// not a side effect of some other axis.
#[test]
fn assemble_main_module_ssr_registration_suppressed_when_requested() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: true,
        ssr: true,
        emit_ssr_module_registration: false,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("src/Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        !result.contains("useSSRContext"),
        "emit_ssr_module_registration: false must suppress the useSSRContext \
         wrapper entirely, got:\n{result}"
    );
    assert!(
        !result.contains("ssrContext.modules"),
        "emit_ssr_module_registration: false must suppress ssrContext.modules \
         registration, got:\n{result}"
    );
    // Positive: the SSR render function itself is UNCHANGED — only the
    // wrapper is suppressed, not SSR codegen as a whole.
    assert!(
        result.contains("export default _sfc_main"),
        "the rest of assembly must proceed normally, got:\n{result}"
    );
}

/// `emit_ssr_module_registration` defaults to `true` — every EXISTING
/// caller (constructed via `..CompileProfile::default()`, which is every
/// production call site in the codebase) keeps today's byte-identical
/// wrapper behavior with zero changes required at the call site. This is
/// the SAME assertion `assemble_main_module_ssr_registers_ssr_context_module`
/// already makes; this test exists to name the discriminating contract
/// explicitly (default-true vs explicit-false) in one place.
#[test]
fn assemble_main_module_ssr_registration_default_is_true() {
    assert!(
        CompileProfile::default().emit_ssr_module_registration,
        "emit_ssr_module_registration must default to true — every \
         existing production caller must keep the useSSRContext wrapper \
         unless it explicitly opts out"
    );
}

/// The registered id must match the ssr-manifest KEY FORM. When the
/// bundler supplies a root-relative `ssr_module_id`, an ABSOLUTE
/// canonical id (the real transform-time shape) must NOT be the
/// registered id — Vite's manifest keys are root-relative, so
/// registering the absolute path makes every `renderPreloadLinks`
/// lookup miss.
#[test]
fn assemble_main_module_ssr_registers_bundler_supplied_module_id() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: true,
        ssr: true,
        ssr_module_id: Some("src/Comp.vue".to_string()),
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    // Absolute canonical id — the shape real transforms pass.
    let result =
        assemble_vue_main_module("/home/user/app/src/Comp.vue", &compiled, &meta, &profile)
            .expect("assembly with maps disabled cannot fail")
            .code;
    assert!(
        result.contains(".add(\"src/Comp.vue\")"),
        "must register the bundler-supplied root-relative id, got:\n{result}"
    );
    assert!(
        !result.contains(".add(\"/home/user/app/src/Comp.vue\")"),
        "must NOT register the absolute canonical id when a module id is supplied, got:\n{result}"
    );
}

/// Without a bundler-supplied module id, the canonical id is the
/// fallback registration.
#[test]
fn assemble_main_module_ssr_falls_back_to_canonical_id() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: true,
        ssr: true,
        ssr_module_id: None,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("/abs/src/Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains(".add(\"/abs/src/Comp.vue\")"),
        "absent ssr_module_id must fall back to the canonical id, got:\n{result}"
    );
}

/// Non-SSR assembly must NOT inject useSSRContext wrapping.
#[test]
fn assemble_main_module_client_no_ssr_context_wrap() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        ssr: false,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("src/Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        !result.contains("useSSRContext"),
        "client assembly must not wrap setup with useSSRContext, got:\n{result}"
    );
    assert!(!result.contains("ssrContext.modules"));
}

/// @ai-generated - Webpack HMR strategy uses module.hot
#[test]
fn assemble_main_module_webpack_hmr() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        ssr: false,
        hmr_strategy: HmrStrategy::Webpack,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(result.contains("module.hot"));
    assert!(!result.contains("import.meta.hot"));
}

/// @ai-generated - No script and no template → bare `const _sfc_main = {}`
#[test]
fn assemble_main_module_no_script_no_template() {
    let compiled = RuntimeCompileOutput::default();
    let profile = CompileProfile::default();
    let result = assemble_vue_main_module("Comp.vue", &compiled, &FileMeta::default(), &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(result.contains("const _sfc_main = {}"));
}

/// @ai-generated - Custom blocks produce import + invocation lines
#[test]
fn assemble_main_module_custom_blocks() {
    let compiled = RuntimeCompileOutput {
        custom_blocks: vec![RuntimeCustomBlock {
            block_type: "i18n".to_string(),
            content: "{\"en\":{}}".to_string(),
        }],
        ..RuntimeCompileOutput::default()
    };
    let profile = CompileProfile::default();
    let meta = FileMeta {
        custom_types: vec!["i18n".to_string()],
        custom_langs: vec![None],
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(result.contains("import block0 from"));
    assert!(result.contains("if (typeof block0 === 'function') block0(_sfc_main)"));
}

/// @ai-generated - Production mode skips __file
#[test]
fn assemble_main_module_production_skips_file() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: true,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(!result.contains("__file"));
}

/// Official `@vitejs/plugin-vue@6.0.7`'s real `transformMain` gates `__file`
/// on `devToolsEnabled || (devServer && !isProduction)` — it is a live
/// dev-server / devtools marker, not a bare dev-vs-prod split. Verter has no
/// separate `devToolsEnabled` concept, but `hmr_strategy: None` already
/// means "no dev-server tooling requested" (its own doc comment: "No HMR
/// code is emitted") — a dev-mode assembly that explicitly opts out of HMR
/// must ALSO skip `__file`, not just the HMR block itself. Confirmed
/// against the pinned rc.3 golden for `basic-interpolation.vue`'s dev
/// cell, which has neither `__file` nor HMR (the harness's golden-
/// generation never runs inside a live dev server).
#[test]
fn assemble_main_module_no_hmr_strategy_skips_file_even_in_dev() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        hmr_strategy: HmrStrategy::None,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        !result.contains("__file"),
        "a dev-mode assembly with hmr_strategy: None must skip __file too, got:\n{result}"
    );
}

/// Regression guard: a dev-mode assembly that DOES request an HMR strategy
/// keeps `__file` — the fix above must not silently drop it whenever
/// `is_production` is false.
#[test]
fn assemble_main_module_dev_with_hmr_strategy_keeps_file() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("__file"),
        "a dev-mode assembly requesting an HMR strategy must still emit __file, got:\n{result}"
    );
}

/// @ai-generated - assemble_main_module with styles produces import lines
#[test]
fn assemble_main_module_with_styles_produces_import_lines() {
    let compiled = RuntimeCompileOutput {
        styles: vec![
            RuntimeStyleBlock {
                code: ".a{}".to_string(),
                source_map: None,
                lang: None,
                scope_hash: None,
                has_global: false,
                output_descriptor: test_output_descriptor(".a{}"),
            },
            RuntimeStyleBlock {
                code: ".b{}".to_string(),
                source_map: None,
                lang: Some("scss".to_string()),
                scope_hash: None,
                has_global: false,
                output_descriptor: test_output_descriptor(".b{}"),
            },
        ],
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        style_langs: vec![None, Some("scss".to_string())],
        ..FileMeta::default()
    };
    let profile = CompileProfile::default();
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("import \"Comp.vue?vue&type=style&index=0"),
        "should import style 0: {}",
        result
    );
    assert!(
        result.contains("import \"Comp.vue?vue&type=style&index=1"),
        "should import style 1: {}",
        result
    );
}

/// @ai-generated - Vite HMR code generation in dev mode
#[test]
fn assemble_main_module_vite_hmr() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile {
        is_production: false,
        ssr: false,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("import.meta.hot"),
        "should contain Vite HMR code"
    );
    assert!(
        result.contains("HMR(vite)"),
        "should contain HMR(vite) comment"
    );
}

/// @ai-generated - Render function binding: _sfc_main.render = render
#[test]
fn assemble_main_module_render_function_binding() {
    let compiled = basic_compiled_result();
    let profile = CompileProfile::default();
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("_sfc_main.render = render"),
        "should bind render function to component"
    );
}

/// @ai-generated - Regression: template-only SFC must produce valid assembled output
/// with _sfc_main defined (no script block → fallback to empty object).
#[test]
fn assemble_main_module_template_only_sfc() {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    // Drive the Vue CARRIER `compile_bundle` (the registry-routed producer)
    // so this end-to-end assembly test exercises the neutral bundle path.
    let source = "<template><div>hello</div></template>";
    let alloc = Allocator::new();
    let compiler = VueCarrierCompiler::default();
    // Route the carrier parse through the counted chokepoint (the dedup
    // rail authority), not a raw `compiler.parse`.
    let provenance = crate::types::MetaProvenance::default();
    let artifact = crate::parse::build_vue_parse_artifact_from_source(source, &provenance);
    let result = compiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                force_js: true,
                ..RuntimeCompileOptions::default()
            },
            &alloc,
        )
        .expect("vue carrier produces a runtime bundle");

    // script should be None for template-only SFC
    assert!(
        result.script.is_none(),
        "template-only SFC should have no script block"
    );
    assert!(
        result.template.is_some(),
        "template-only SFC should have template block"
    );

    let profile = CompileProfile::default();
    let meta = FileMeta {
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("NoScript.vue", &result, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;

    // Must contain _sfc_main definition (fallback empty object)
    assert!(
        assembled.contains("const _sfc_main = {}"),
        "template-only SFC must define _sfc_main, got:\n{}",
        assembled
    );
    // Must bind render function
    assert!(
        assembled.contains("_sfc_main.render = render"),
        "template-only SFC must bind render, got:\n{}",
        assembled
    );
    // Must export
    assert!(
        assembled.contains("export default _sfc_main"),
        "template-only SFC must export, got:\n{}",
        assembled
    );
}

/// @ai-generated - Inline topology: assembly emits no standalone render
/// function — the render closure already lives inside `setup()`.
#[test]
fn assemble_main_module_inline_topology() {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    let source = "<script setup>\nimport { ref } from 'vue'\nconst msg = ref('hello')\n</script>\n<template><div>{{ msg }}</div></template>";
    let alloc = Allocator::new();
    let compiler = VueCarrierCompiler::default();
    let provenance = crate::types::MetaProvenance::default();
    let artifact = crate::parse::build_vue_parse_artifact_from_source(source, &provenance);
    let result = compiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                force_js: true,
                inline: Some(true),
                ..RuntimeCompileOptions::default()
            },
            &alloc,
        )
        .expect("vue carrier produces a runtime bundle");

    // Inline compile: no separate template block, topology flag set.
    assert!(result.inline, "bundle must carry the inline topology flag");
    assert!(
        result.template.is_none(),
        "inline compile must not emit a template block"
    );

    let profile = CompileProfile::default();
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("App.vue", &result, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;

    // The render closure is inside setup — no standalone render attach.
    assert!(
        assembled.contains("return (_ctx,_cache) => {"),
        "render must be inlined into setup, got:\n{}",
        assembled
    );
    assert!(
        !assembled.contains("function render("),
        "inline assembly must not emit a standalone render fn, got:\n{}",
        assembled
    );
    assert!(
        !assembled.contains("_sfc_main.render = render"),
        "inline assembly must not attach render, got:\n{}",
        assembled
    );
    // No __returned__ bindings object in inline mode.
    assert!(
        !assembled.contains("__returned__"),
        "inline assembly must not contain __returned__, got:\n{}",
        assembled
    );
    // Component object + final export still present.
    assert!(
        assembled.contains("const _sfc_main = {"),
        "assembled module must define _sfc_main, got:\n{}",
        assembled
    );
    assert!(
        assembled.trim_end().ends_with("export default _sfc_main"),
        "assembled module must end with the default export, got:\n{}",
        assembled
    );
}

/// The runtime Main passes the compiler-emitted `__returned__` bindings
/// object through UNCHANGED: setup-binding elision (type-only imports,
/// unused setup imports) is owned by the compiler's `build_returned_object`
/// (template_used_vars-driven), not by a text-level post-pass on the
/// assembled module. (The old `filter_setup_return` was removed — it
/// keyed on a `return { ... };` shape the compiler has not emitted since
/// `__returned__` was introduced, so it was dead code on the real
/// production shape, proven by canary.)
#[test]
fn assemble_passes_compiler_returned_bindings_verbatim() {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    // UnusedSetupImport must be elided by the COMPILER (not by any
    // assembly-level text filtering); `msg` is template-used and stays.
    let source = "<script setup>\nimport { ref } from 'vue'\nimport UnusedComp from './UnusedComp.vue'\nconst msg = ref('hello')\n</script>\n<template><div>{{ msg }}</div></template>";
    let alloc = Allocator::new();
    let compiler = VueCarrierCompiler::default();
    let provenance = crate::types::MetaProvenance::default();
    let artifact = crate::parse::build_vue_parse_artifact_from_source(source, &provenance);
    let result = compiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                force_js: true,
                ..RuntimeCompileOptions::default()
            },
            &alloc,
        )
        .expect("vue carrier produces a runtime bundle");

    let profile = CompileProfile::default();
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("App.vue", &result, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;

    // `ref` is called in the script's own body (`ref('hello')`), so it is
    // template-body-used and included after the local `msg` declaration
    // (declared-bindings-first, imports-last — see `build_returned_object`).
    assert!(
        assembled.contains("const __returned__ = { msg, ref };"),
        "the compiler-emitted __returned__ survives assembly verbatim, got:\n{}",
        assembled
    );
    assert!(
        !assembled.contains("return { msg, UnusedComp }")
            && !assembled.contains("__returned__ = { msg, UnusedComp }"),
        "unused setup import must already be elided by the compiler, got:\n{}",
        assembled
    );
}

/// The genuine production assembler must return the assembled module code
/// AND the source map it was generated from, together. Before the
/// assembled-map composition landed it terminated at a bare `String`, so a
/// map-enabled compile observed code with no map at all — an absent map is
/// a hard failure, never a skip.
#[test]
fn assemble_returns_code_and_map_together_when_maps_are_requested() {
    let script_code = "const __sfc__ = {}\n";
    let compiled = RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: script_code.to_string(),
            source_map:
                "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}"
                    .to_string(),
            setup: true,
            output_descriptor: test_output_descriptor(script_code),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
        }),
        ..RuntimeCompileOutput::default()
    };
    let profile = CompileProfile {
        source_map: true,
        ..CompileProfile::default()
    };
    let meta = FileMeta {
        has_script: true,
        ..FileMeta::default()
    };

    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("a composable script map assembles");

    assert!(
        assembled.code.contains("const _sfc_main = {}"),
        "the assembled code is unchanged, got:\n{}",
        assembled.code
    );
    let map = assembled
        .source_map
        .as_deref()
        .expect("a map-enabled assembly returns the map paired with its code");
    assert!(
        map.contains("\"sources\":[\"Comp.vue\"]"),
        "the fragment's declared source identity is carried, got:\n{map}"
    );
    assert!(
        map.contains("\"version\":3"),
        "the emitted map is a flat v3 artifact, got:\n{map}"
    );
    assert!(
        !map.contains("\"mappings\":\"\""),
        "a fragment declaring a segment must produce a non-empty mapping, got:\n{map}"
    );
    // The input declares one segment at (0,6) -> authored (1,6), exactly where
    // the rename replaces `__sfc__`. So the segment is dropped and replaced by
    // the replacement's own at (0,6), surviving text resumes at (0,6+9)=(0,15)
    // carrying the same authored position, and the fragment's trailing empty
    // line takes the sourceless boundary. Full geometry is pinned in
    // `map_tests`; this regression only proves the map arrives at all.
    assert!(
        map.contains("\"mappings\":\"MACM,SAAA;A\""),
        "expected the replacement, resume and boundary segments, got:\n{map}"
    );
}

/// @ai-generated - Multi-root template must use Fragment wrapping
#[test]
fn compile_multi_root_template_uses_fragment() {
    use verter_compiler::compile::CodegenOptions;
    use verter_compiler::compile::VerterCompileOptions;
    use verter_compiler::standalone::{StandaloneCompiler, StandaloneSourceBytes};

    let source = "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div>aaaaa</template>";
    let opts = CodegenOptions {
        inline: Some(false),
        ..CodegenOptions::default()
    };
    let vopts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = StandaloneCompiler.compile_source(
        &StandaloneSourceBytes::copied_from(source),
        &opts,
        &vopts,
        &verter_compiler::compile::VueMacroSemanticInput::Unavailable,
    );

    let tpl = result.template.expect("should have template block");

    // Multi-root template must use Fragment
    assert!(
        tpl.code.contains("_Fragment"),
        "multi-root template should use _Fragment, got:\n{}",
        tpl.code
    );
    // Must include _createTextVNode for the text node
    assert!(
        tpl.code.contains("_createTextVNode"),
        "multi-root template should use _createTextVNode for text, got:\n{}",
        tpl.code
    );
    // Imports must include Fragment
    assert!(
        tpl.imports.contains(&"_Fragment"),
        "multi-root template imports must include _Fragment, got: {:?}",
        tpl.imports
    );
}
