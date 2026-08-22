//! Assembly tests for the Vue runtime main module.
//!
//! These assert on the assembled CODE. The assembler now returns that code
//! coupled to its source map, so each reaches through `.code`; what each
//! checks is unchanged, because composing a map alongside the code does not
//! change which bytes are produced.

use super::*;

// assemble_main_module tests

use verter_compiler::framework_common::{
    RuntimeCompileOutput, RuntimeCustomBlock, RuntimeOutputDescriptor, RuntimeScriptBlock,
    RuntimeStyleBlock, RuntimeTemplateBlock, SourceMapFidelity, TemplateRenderExport,
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
            sfc_export_placement: super::map_compose::literal_scan_placement_for_fixture(
                script_code,
            ),
        }),
        template: Some(RuntimeTemplateBlock {
            code: template_code.to_string(),
            source_map: String::new(),
            imports: vec!["_openBlock".to_string(), "_createElementBlock".to_string()],
            ssr_imports: vec![],
            render_export: TemplateRenderExport::Render,
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

/// `emit_ssr_module_registration: false` suppresses the
/// `useSSRContext`/`ssrContext.modules` wrapper (harness candidate shape).
/// Other fields stay at default — a single-purpose knob.
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

/// `emit_ssr_module_registration` defaults to `true` so existing
/// `CompileProfile::default()` callers keep today's wrapper.
#[test]
fn assemble_main_module_ssr_registration_default_is_true() {
    assert!(
        CompileProfile::default().emit_ssr_module_registration,
        "emit_ssr_module_registration must default to true — every \
         existing production caller must keep the useSSRContext wrapper \
         unless it explicitly opts out"
    );
}

/// Registered id must match the ssr-manifest key form: a bundler-supplied
/// root-relative `ssr_module_id` wins over an absolute canonical id.
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

/// Official plugin-vue gates `__file` on `devToolsEnabled || (devServer &&
/// !isProduction)`. `hmr_strategy: None` means no dev-server tooling, so
/// skip `__file` too — not just the HMR block.
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
    let compiler = VueCarrierCompiler;
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
        .expect("vue carrier produces a runtime bundle")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

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
    let compiler = VueCarrierCompiler;
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
        .expect("vue carrier produces a runtime bundle")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

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

/// Runtime Main passes `__returned__` through unchanged. Setup-binding
/// elision is the compiler's `build_returned_object`, not an assembly
/// text post-pass.
#[test]
fn assemble_passes_compiler_returned_bindings_verbatim() {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    // UnusedSetupImport must be elided by the COMPILER (not by any
    // assembly-level text filtering); `msg` is template-used and stays.
    let source = "<script setup>\nimport { ref } from 'vue'\nimport UnusedComp from './UnusedComp.vue'\nconst msg = ref('hello')\n</script>\n<template><div>{{ msg }}</div></template>";
    let alloc = Allocator::new();
    let compiler = VueCarrierCompiler;
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
        .expect("vue carrier produces a runtime bundle")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

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

/// Assembler returns code and map together. An absent map is a hard
/// failure, never a skip.
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
            sfc_export_placement: super::map_compose::literal_scan_placement_for_fixture(
                script_code,
            ),
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

/// Export binding is decided by the template block's DECLARED
/// [`TemplateRenderExport`], never by scanning the generated code for a
/// literal `function render(`/`function ssrRender(` occurrence. A decoy
/// occurrence of the OTHER function's name embedded in the body must not
/// change which property gets bound.
#[test]
fn render_export_binding_follows_declared_fact_not_generated_text() {
    let ssr_decoy_code =
        "function ssrRender(_ctx, _push, _parent, _attrs) {\n  // function render( appears only in this comment\n  _push(`<div></div>`)\n}";
    let compiled = RuntimeCompileOutput {
        template: Some(RuntimeTemplateBlock {
            code: ssr_decoy_code.to_string(),
            source_map: String::new(),
            imports: vec![],
            ssr_imports: vec!["_ssrRenderAttrs".to_string()],
            render_export: TemplateRenderExport::SsrRender,
            output_descriptor: test_output_descriptor(ssr_decoy_code),
        }),
        ..RuntimeCompileOutput::default()
    };
    let profile = CompileProfile::default();
    let meta = FileMeta {
        has_template: true,
        ..FileMeta::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("_sfc_main.ssrRender = ssrRender"),
        "declared SsrRender must bind ssrRender even though the body text \
         also contains the string \"function render(\" in a comment, got:\n{result}"
    );
    assert!(
        !result.contains("_sfc_main.render = render"),
        "must not additionally bind render from the decoy text, got:\n{result}"
    );

    // Positive control, inverse declaration: a body containing the decoy
    // "function ssrRender(" text but declared Render must bind render.
    let render_decoy_code =
        "function render(_ctx, _cache) {\n  // function ssrRender( appears only in this comment\n  return null\n}";
    let compiled = RuntimeCompileOutput {
        template: Some(RuntimeTemplateBlock {
            code: render_decoy_code.to_string(),
            source_map: String::new(),
            imports: vec![],
            ssr_imports: vec![],
            render_export: TemplateRenderExport::Render,
            output_descriptor: test_output_descriptor(render_decoy_code),
        }),
        ..RuntimeCompileOutput::default()
    };
    let result = assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile)
        .expect("assembly with maps disabled cannot fail")
        .code;
    assert!(
        result.contains("_sfc_main.render = render"),
        "declared Render must bind render even though the body text also \
         contains the string \"function ssrRender(\" in a comment, got:\n{result}"
    );
    assert!(
        !result.contains("_sfc_main.ssrRender = ssrRender"),
        "must not additionally bind ssrRender from the decoy text, got:\n{result}"
    );
}

/// @ai-generated - Multi-root template must use Fragment wrapping
#[test]
fn compile_multi_root_template_uses_fragment() {
    use verter_compiler::compile::types::VueExecutionInputs;
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, ProductKind,
        RuntimeProductRequest, VueCompileRequest,
    };
    use verter_compiler::standalone::{DirectExecutionInputs, StandaloneCompiler};

    let source = "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div>aaaaa</template>";
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: Some(false),
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        true,
    )
    .expect("a lone RuntimeClient product must construct");
    let execution_inputs = VueExecutionInputs::default();
    let output = StandaloneCompiler
        .compile(
            source,
            &request,
            DirectExecutionInputs::Vue {
                execution: &execution_inputs,
                macros: &verter_compiler::compile::VueMacroSemanticInput::Unavailable,
            },
        )
        .expect("a plain RuntimeClient compile must not be refused");

    // The template's own generated code is written VERBATIM into the
    // composed module (`compose_fragments` never rewrites it), so every
    // template-codegen fact this test pins survives as a substring of the
    // published artifact.
    let code = output
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("RuntimeClient must produce an artifact")
        .code();

    // Multi-root template must use Fragment
    assert!(
        code.contains("_Fragment"),
        "multi-root template should use _Fragment, got:\n{code}"
    );
    // Must include _createTextVNode for the text node
    assert!(
        code.contains("_createTextVNode"),
        "multi-root template should use _createTextVNode for text, got:\n{code}"
    );
    // The composed module's own import line must include Fragment
    assert!(
        code.contains("Fragment as _Fragment"),
        "multi-root template's composed import line must include _Fragment, got:\n{code}"
    );
}
