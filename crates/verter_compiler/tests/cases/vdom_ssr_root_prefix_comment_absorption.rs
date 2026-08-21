//! Regression coverage for the VDOM/SSR root-prefix duplicate-ownership panic.
//!
//! Root cause: two independent producers wrote overlapping subranges of the
//! template header when a leading disabled comment precedes a single,
//! statically-classed root element under `comments: false` (production
//! build, non-inline render function — i.e. an Options API `<script>`, since
//! `<script setup>` production defaults to the inline render path and never
//! reaches the affected `leave_template` branch):
//!
//! 1. `visit_comment` → `comment::process_comment` unconditionally emitted a
//!    plain `overwrite(comment.start, comment.end, "")` into the `overwrites`
//!    channel when comments were disabled.
//! 2. `leave_template`'s single-root block-root path, carrying hoisted
//!    static-class anchors, emitted `overwrite_or_root_prefix_segmented`
//!    into the `segmented_overwrites` channel over a range that structurally
//!    CONTAINED producer 1's range (disabled comments are dropped from child
//!    bookkeeping without excluding their span from the claimed prefix).
//!
//! `CodeGenOutput::apply_to` flushes `overwrites` before
//! `segmented_overwrites` unconditionally, so by the time the segmented
//! overwrite ran, its target was no longer one untouched `Original` chunk,
//! and `try_overwrite_segmented`'s strict single-chunk precondition panicked:
//! `overwrite_segmented precondition violated at [0,N): ReplacedContentSplit`.
//!
//! The repair gives `leave_template`'s root-prefix/suffix owner sole
//! ownership of any disabled-comment removal wholly contained by the range
//! it claims (absorption); a comment left unclaimed (interior/trailing)
//! still gets an ordinary deletion. SSR shares the same duplicate-ownership
//! *shape* (comments excluded from `effective_count`, an independent
//! segmented deletion queued for a disabled comment, a zero-effective-root
//! branch claiming a whole-template segmented range) and is fixed
//! backend-locally with the same principle.
//!
//! DISCRIMINATING: every positive case below panicked (or, pre-fix,
//! would have panicked) against the unmodified tree; every negative control
//! (no static class / comments enabled / no leading comment / Vapor) passed
//! before AND after the fix — proving the repair is scoped to the exact
//! conflict, not a broadened refusal.
//!
//! Coverage completion: this file also crosses the axes the
//! initial matrix sampled only once each — root-level trailing comments,
//! comment-shape × root-class × build-mode, style × SSR, script-kind × SSR,
//! explicit no-comment negative controls across backend/build-mode, source
//! maps on/off with a decoded-token assertion, a runtime-link assertion
//! beyond bare parsing, and a direct `CarrierCompiler::compile_bundle`
//! invocation — the SEPARATE production entry point
//! (`compile_bundle_refuses_explicit_ssr_and_force_vapor`'s doc comment in
//! `framework_common/vue_bridge.rs` states this in-tree: "a SEPARATE
//! production entry into the shared codegen substrate from
//! `CompileRequest::new`") that the host's `compile_many`/`compileMany`
//! render lane reaches through
//! `VerterHost::compile_entry_runtime_render` → `compiler.compile_bundle`,
//! never through `StandaloneCompiler`. Exercising it here proves the repair
//! independently of the direct `StandaloneCompiler` route the rest of this
//! file uses.

use oxc_sourcemap::OwnedSourceMap;
use std::sync::Arc;
use verter_compiler::compile::types::VueExecutionInputs;
use verter_compiler::compile::VueMacroSemanticInput;
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
    VueBackendRequest, VueCompileRequest,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    CarrierCompileOutcome, CarrierCompiler, CarrierCompilerRegistry, FrameworkParseArtifact,
    RuntimeCompileOptions,
};
use verter_compiler::standalone::{StandaloneCompiler, StandaloneSourceBytes};

/// Compile a single `RuntimeClient` (VDOM/Vapor) product and return the
/// generated template code. Panics propagate to the caller — the RED state
/// for the bug this file locks down. `inline` is always pinned explicitly:
/// production defaults `inline` to `true` for a `<script setup>` VDOM compile
/// (never reaching the affected non-inline `leave_template` branch), and
/// Vapor does not yet support inline template compilation at all
/// (`VaporInlineNotYetImplemented`).
fn compile_client(
    source: &str,
    is_production: bool,
    backend: VueBackendRequest,
    inline: Option<bool>,
) -> String {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend,
            ..Default::default()
        }),
        None,
        Some("Root.vue".to_string()),
        None,
        is_production,
        true,
    )
    .expect("a lone RuntimeClient product must construct");
    let result = StandaloneCompiler
        .compile_source(
            &StandaloneSourceBytes::copied_from(source),
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        )
        .expect("a plain RuntimeClient compile must not be refused");
    assert!(
        result.errors.is_empty(),
        "compile diagnostics: {:?}",
        result.errors
    );
    result
        .template
        .as_ref()
        .unwrap_or_else(|| panic!("RuntimeClient compile must produce a template block"))
        .code
        .clone()
}

/// Compile a single `RuntimeServer` (SSR) product and return the generated
/// template code.
fn compile_server(source: &str, is_production: bool) -> String {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeServer(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            ssr: true,
            ..Default::default()
        }),
        None,
        Some("Root.vue".to_string()),
        None,
        is_production,
        true,
    )
    .expect("a lone RuntimeServer product must construct");
    let result = StandaloneCompiler
        .compile_source(
            &StandaloneSourceBytes::copied_from(source),
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        )
        .expect("a plain RuntimeServer compile must not be refused");
    assert!(
        result.errors.is_empty(),
        "compile diagnostics: {:?}",
        result.errors
    );
    result
        .template
        .as_ref()
        .unwrap_or_else(|| panic!("RuntimeServer compile must produce a template block"))
        .code
        .clone()
}

/// Assert `code` is parseable JS (the required-exit "generated JavaScript
/// parses" bar every acceptance-matrix cell must clear).
fn assert_parses(code: &str, label: &str) {
    let alloc = oxc_allocator::Allocator::new();
    let wrapped = format!("import {{}} from \"vue\";\n{code}");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, oxc_span::SourceType::mjs()).parse();
    assert!(
        parsed.errors.is_empty(),
        "{label}: generated JS failed to parse: {:?}\n--- code ---\n{code}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
}

/// Compile a `RuntimeClient` product with source maps requested and return
/// `(code, source_map_json)`. Separate from [`compile_client`] rather than
/// adding a parameter to it — every existing call site stays untouched.
fn compile_client_with_map(
    source: &str,
    is_production: bool,
    backend: VueBackendRequest,
) -> (String, String) {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: Some(false),
            runtime_source_map: true,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend,
            ..Default::default()
        }),
        None,
        Some("Root.vue".to_string()),
        None,
        is_production,
        true,
    )
    .expect("a lone RuntimeClient product must construct");
    let result = StandaloneCompiler
        .compile_source(
            &StandaloneSourceBytes::copied_from(source),
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        )
        .expect("a plain RuntimeClient compile must not be refused");
    assert!(result.errors.is_empty(), "diagnostics: {:?}", result.errors);
    let template = result
        .template
        .as_ref()
        .unwrap_or_else(|| panic!("RuntimeClient compile must produce a template block"));
    (template.code.clone(), template.source_map.clone())
}

/// SSR sibling of [`compile_client_with_map`].
fn compile_server_with_map(source: &str, is_production: bool) -> (String, String) {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeServer(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            ssr: true,
            ..Default::default()
        }),
        None,
        Some("Root.vue".to_string()),
        None,
        is_production,
        true,
    )
    .expect("a lone RuntimeServer product must construct");
    let result = StandaloneCompiler
        .compile_source(
            &StandaloneSourceBytes::copied_from(source),
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        )
        .expect("a plain RuntimeServer compile must not be refused");
    assert!(result.errors.is_empty(), "diagnostics: {:?}", result.errors);
    let template = result
        .template
        .as_ref()
        .unwrap_or_else(|| panic!("RuntimeServer compile must produce a template block"));
    (template.code.clone(), template.source_map.clone())
}

/// Byte offset → 0-based (line, column) for an ASCII fixture (every source
/// string in this file is plain ASCII, so a byte-count column is also the
/// UTF-16 column the source-map format uses).
fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in text.as_bytes().iter().enumerate() {
        if i == byte_offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    ((line), (byte_offset - line_start) as u32)
}

/// Decode `map_json`, look up the FIRST occurrence of `target` in
/// `generated_code`, and assert it maps back to matching text in `source` —
/// the sourcemap-accuracy bar (a token that maps to the WRONG source text is
/// caught, not merely "a position exists"). Mirrors the pattern this crate's
/// own `framework_common::sourcemap_e2e_helpers` module uses for the IDE
/// projection (that module is `#[cfg(test)]`-internal, so this is a small
/// standalone re-implementation for the runtime template map).
fn assert_source_map_token_resolves(
    generated_code: &str,
    map_json: &str,
    source: &str,
    target: &str,
) {
    assert!(
        !map_json.is_empty(),
        "source maps were requested but the emitted map string is empty"
    );
    let sm = OwnedSourceMap::from_json_string(map_json)
        .expect("a requested source map must be valid map JSON");
    assert!(
        sm.get_source_content(0).is_some() || sm.get_source(0).is_some(),
        "the emitted map carries no source entry at all"
    );
    let target_offset = generated_code
        .find(target)
        .unwrap_or_else(|| panic!("{target:?} not found in generated code:\n{generated_code}"));
    let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, target_offset);
    let lookup = sm.generate_lookup_table();
    let token = sm
        .lookup_token(&lookup, gen_line, gen_col)
        .unwrap_or_else(|| {
            panic!("no source-map token at generated {gen_line}:{gen_col} for {target:?}")
        });
    assert!(
        token.get_source_id().is_some(),
        "token at generated {gen_line}:{gen_col} for {target:?} is unmapped"
    );
    let src_line = token.get_src_line() as usize;
    let source_lines: Vec<&str> = source.lines().collect();
    assert!(
        src_line < source_lines.len(),
        "mapped source line {src_line} out of bounds ({} lines) for {target:?}",
        source_lines.len()
    );
    assert!(
        source_lines[src_line].contains(target),
        "token maps {target:?} → source line {src_line} ({:?}), which does not contain {target:?}",
        source_lines[src_line]
    );
}

/// Build a [`FrameworkParseArtifact`] the same way the crate's own
/// `#[cfg(test)]`-internal `vue_bridge::tests::artifact_for` does, but as a
/// standalone re-implementation reachable from this integration test (the
/// internal one is not `pub`/visible outside the crate's own unit-test
/// build).
fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
        FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };
    let source_authority = RegisteredSourceAuthority::new().unwrap();
    let grammar_authority = CarrierGrammarAuthority::new().unwrap();
    let config = CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap();
    grammar_authority
        .register_carrier_grammar(
            verter_language::FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            config.clone(),
        )
        .unwrap();
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new("file:///native-route-fixture.vue"),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            verter_language::FileLanguage::vue(),
            Arc::from(source),
        )
        .unwrap();
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .unwrap();
    Arc::new(
        CarrierCompilerRegistry::built_in()
            .project_registered(&accepted)
            .expect("fixture source parses")
            .into_framework_parse_artifact(),
    )
}

/// Compile the headline shape through `CarrierCompiler::compile_bundle` —
/// the production entry point the host's `compile_many`/native `compileMany`
/// render lane reaches via `compile_entry_runtime_render` (see this file's
/// module doc). Distinct from every other test in this file, which drives
/// the `StandaloneCompiler` route instead.
fn native_route_compile_bundle(
    source: &str,
    is_production: bool,
    ssr: bool,
    force_vapor: bool,
) -> String {
    let compiler = VueCarrierCompiler;
    let artifact = artifact_for(source);
    let alloc = oxc_allocator::Allocator::new();
    let outcome = compiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Root.vue".to_string()),
                is_production,
                ssr,
                force_vapor,
                inline: Some(false),
                want_runtime: true,
                ..Default::default()
            },
            &alloc,
        )
        .expect("compile_bundle must not refuse a plain runtime request");
    let produced = match outcome {
        CarrierCompileOutcome::Produced(bundle) => bundle,
        CarrierCompileOutcome::RuntimeSurfaceRefused(refusal) => {
            panic!("runtime surface refused: {refusal:?}")
        }
    };
    produced
        .template
        .unwrap_or_else(|| panic!("compile_bundle must produce a template block"))
        .code
}

// Non-inline production forces the Options API (or template-only) path —
// `<script setup>` production defaults to the inline render function, which
// never reaches the affected `leave_template` branch. `inline: Some(false)`
// pins the non-inline path explicitly regardless of script shape so every
// matrix cell below hits the same codegen branch the bug report did.

// ==================== VDOM: headline reproduction ====================

/// HEADLINE: leading disabled comment + single static-class root, production
/// VDOM, Options API. This is the exact regression-intake shape.
#[test]
fn vdom_leading_comment_static_class_root_production_options_api() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(
        !code.contains("<!--"),
        "disabled comment must not leak into generated JS:\n{code}"
    );
    assert_parses(&code, "vdom leading comment + static class");
}

/// Same shape via `<script setup>` with `inline: Some(false)` pinned
/// explicitly (script-axis coverage: `<script setup>` can still take the
/// non-inline path when a caller asks for it).
#[test]
fn vdom_leading_comment_static_class_root_production_script_setup_non_inline() {
    let source = r#"<script setup>
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom script-setup non-inline");
}

/// Development build: comments stay ENABLED by default, so no disabled-comment
/// removal is queued at all — this is a negative control proving the fix does
/// not touch the comments-enabled path.
#[test]
fn vdom_leading_comment_static_class_root_development() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, false, VueBackendRequest::Vdom, Some(false));
    assert!(
        code.contains("_createCommentVNode"),
        "dev build must keep the comment as a vnode:\n{code}"
    );
    assert_parses(&code, "vdom dev leading comment");
}

// ==================== VDOM: full cross of the defect-relevant axes ====================

/// Root class: static / none / dynamic / static+dynamic — each combined with
/// a leading disabled comment in production.
#[test]
fn vdom_leading_comment_root_class_matrix() {
    let cases: &[(&str, &str)] = &[
        ("static", r#"<div class="root">hi</div>"#),
        ("none", r#"<div>hi</div>"#),
        ("dynamic", r#"<div :class="cls">hi</div>"#),
        (
            "static+dynamic",
            r#"<div class="root" :class="cls">hi</div>"#,
        ),
    ];
    for (label, el) in cases {
        let source = format!(
            "<script>\nexport default {{ data() {{ return {{ cls: 'x' }} }} }}\n</script>\n<template><!-- lead -->{el}</template>\n"
        );
        let code = compile_client(&source, true, VueBackendRequest::Vdom, Some(false));
        assert!(!code.contains("<!--"), "[{label}] comment leaked:\n{code}");
        assert_parses(&code, &format!("vdom root-class[{label}]"));
    }
}

/// Comment position: first (already covered above) vs. later (interior,
/// between two elements under a wrapping root — trailing at the very end of
/// a multi-child single root is exercised as "interior" here since a
/// single-root template body only has one logical child; the general
/// interior/trailing case is covered structurally by the leftover-removal
/// path, which is identical to today's plain-overwrite behavior).
#[test]
fn vdom_interior_and_trailing_disabled_comment() {
    let source = r#"<script>
export default {}
</script>
<template><div class="root"><span>a</span><!-- mid --><span>b</span><!-- tail --></div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom interior+trailing comment");
}

/// Comment shape: short / long / whitespace-only.
#[test]
fn vdom_leading_comment_shape_matrix() {
    let cases: &[(&str, &str)] = &[
        ("short", "<!--x-->"),
        (
            "long",
            "<!-- this is a much longer comment body spanning many characters to exercise a wider claimed byte range -->",
        ),
        ("whitespace-only", "<!--   -->"),
    ];
    for (label, comment) in cases {
        let source = format!(
            "<script>\nexport default {{}}\n</script>\n<template>{comment}<div class=\"root\">hi</div></template>\n"
        );
        let code = compile_client(&source, true, VueBackendRequest::Vdom, Some(false));
        assert!(!code.contains("<!--"), "[{label}] comment leaked:\n{code}");
        assert_parses(&code, &format!("vdom comment-shape[{label}]"));
    }
}

/// Template body: interpolation / static text / directive-free minimum, each
/// with a leading disabled comment and a static-class root.
#[test]
fn vdom_leading_comment_template_body_matrix() {
    let cases: &[(&str, &str)] = &[
        ("interpolation", r#"<div class="root">{{ msg }}</div>"#),
        ("static-text", r#"<div class="root">hi</div>"#),
        ("directive-free-minimum", r#"<div class="root"></div>"#),
    ];
    for (label, el) in cases {
        let source = format!(
            "<script>\nexport default {{ data() {{ return {{ msg: 'x' }} }} }}\n</script>\n<template><!-- lead -->{el}</template>\n"
        );
        let code = compile_client(&source, true, VueBackendRequest::Vdom, Some(false));
        assert!(!code.contains("<!--"), "[{label}] comment leaked:\n{code}");
        assert_parses(&code, &format!("vdom template-body[{label}]"));
    }
}

/// Style axis: a `<style>` block alongside the failing template shape must
/// not change codegen for the template block itself.
#[test]
fn vdom_leading_comment_with_style_block() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
<style>.root { color: red; }</style>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom with style block");
}

// ==================== Vapor: negative control ====================

/// NEGATIVE CONTROL: Vapor omits disabled comments from its own private
/// assembly and never emits an independent comment overwrite (per the
/// ratified ruling) — the identical source shape must simply keep working,
/// with no new machinery required.
#[test]
fn vapor_leading_comment_static_class_root_production_unaffected() {
    let source = r#"<script setup>
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vapor, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vapor leading comment (negative control)");
}

// ==================== SSR ====================

/// SSR sibling: leading disabled comment + single static-class root,
/// production build. SSR does not reproduce the exact VDOM collision for a
/// nonempty root (see evidence doc) but shares the comment-exclusion /
/// segmented-deletion shape; this proves the backend-local SSR fix holds.
/// A comment beside a single root still forces SSR's own `<!--[-->`/`<!--]-->`
/// fragment markers (mirroring VDOM's DEV_ROOT_FRAGMENT logic) even though the
/// comment ITSELF is disabled — so this checks the authored comment TEXT is
/// absent, not a blanket `<!--` ban.
#[test]
fn ssr_leading_comment_static_class_root_production() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert_parses(&code, "ssr leading comment + static class");
}

/// SSR: a template whose ONLY content is a disabled leading comment (zero
/// effective roots) — the branch that claims a whole-template segmented
/// replacement.
#[test]
fn ssr_only_disabled_comment_zero_effective_roots() {
    let source = r#"<script>
export default {}
</script>
<template><!-- only comment --></template>
"#;
    let code = compile_server(source, true);
    assert!(
        !code.contains("only comment"),
        "comment content leaked:\n{code}"
    );
    assert_parses(&code, "ssr zero-effective-root comment-only template");
}

/// SSR: interior/trailing disabled comments around a static-class root.
#[test]
fn ssr_interior_and_trailing_disabled_comment() {
    let source = r#"<script>
export default {}
</script>
<template><div class="root"><span>a</span><!-- mid --><span>b</span><!-- tail --></div></template>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "ssr interior+trailing comment");
}

/// SSR development build negative control: comments enabled by default.
#[test]
fn ssr_leading_comment_static_class_root_development() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_server(source, false);
    assert!(
        code.contains("<!--"),
        "dev SSR build must keep the comment marker:\n{code}"
    );
    assert_parses(&code, "ssr dev leading comment");
}

// ==================== Coverage completion ====================
//
// Everything below closes specific conformance-review gaps: native
// `compile_bundle` invocation, source maps on/off, root-level trailing
// comments, comment-shape × root-class × build-mode crossing, style × SSR,
// script-kind × SSR, explicit no-comment negative controls, and a
// runtime-link assertion beyond bare parsing.

// -------------------- Native route: `CarrierCompiler::compile_bundle` --------------------

/// The headline VDOM shape through the SAME production entry point the
/// host's `compile_many`/native `compileMany` render lane reaches
/// (`compile_entry_runtime_render` → `compiler.compile_bundle`) — see the
/// module doc for why this is a genuinely SEPARATE call chain from
/// `StandaloneCompiler`.
#[test]
fn native_route_vdom_leading_comment_static_class_root_production() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = native_route_compile_bundle(source, true, false, false);
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "native-route vdom leading comment + static class");
}

/// The SSR sibling of [`native_route_vdom_leading_comment_static_class_root_production`].
#[test]
fn native_route_ssr_leading_comment_static_class_root_production() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = native_route_compile_bundle(source, true, true, false);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert_parses(&code, "native-route ssr leading comment + static class");
}

/// The zero-effective-root SSR shape (the branch that claims a
/// whole-template segmented range) through the native route too.
#[test]
fn native_route_ssr_only_disabled_comment_zero_effective_roots() {
    let source = r#"<script>
export default {}
</script>
<template><!-- only comment --></template>
"#;
    let code = native_route_compile_bundle(source, true, true, false);
    assert!(
        !code.contains("only comment"),
        "comment content leaked:\n{code}"
    );
    assert_parses(&code, "native-route ssr zero-effective-root comment-only");
}

// -------------------- Source maps: on/off, with a decoded-token assertion --------------------

/// VDOM, source maps ON: the map decodes, and the root element's tag maps
/// back to matching text in the original carrier source — the sourcemap
/// ACCURACY bar, not merely "a map string is present".
#[test]
fn vdom_leading_comment_source_map_on_resolves_root_tag() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let (code, map) = compile_client_with_map(source, true, VueBackendRequest::Vdom);
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom map-on leading comment");
    assert_source_map_token_resolves(&code, &map, source, "div");
}

/// VDOM, source maps OFF: no map is emitted, compilation still succeeds.
#[test]
fn vdom_leading_comment_source_map_off_emits_no_map() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom map-off leading comment");
    // `compile_client` never requests `runtime_source_map`, matching every
    // other direct-route test in this file (its own negative control for
    // this axis).
}

/// SSR, source maps ON: same accuracy bar as the VDOM case above. SSR's
/// `ssrRenderAttrs`/`mergeProps` codegen maps the `class` ATTRIBUTE NAME
/// token (not every literal inside the merged object), so that is the
/// mapped anchor this asserts against — confirmed by dumping the map's raw
/// token table before picking it (`class` is the first token with a
/// non-`None` source id).
#[test]
fn ssr_leading_comment_source_map_on_resolves_class_attr() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let (code, map) = compile_server_with_map(source, true);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert_parses(&code, "ssr map-on leading comment");
    assert_source_map_token_resolves(&code, &map, source, "class");
}

/// SSR, source maps OFF: `compile_server` (used throughout this file) never
/// requests a map; confirm the produced template's map field is empty.
#[test]
fn ssr_leading_comment_source_map_off_emits_no_map() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeServer(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            ssr: true,
            ..Default::default()
        }),
        None,
        Some("Root.vue".to_string()),
        None,
        true,
        true,
    )
    .expect("a lone RuntimeServer product must construct");
    let result = StandaloneCompiler
        .compile_source(
            &StandaloneSourceBytes::copied_from(source),
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        )
        .expect("a plain RuntimeServer compile must not be refused");
    assert!(result.errors.is_empty(), "diagnostics: {:?}", result.errors);
    let template = result
        .template
        .as_ref()
        .expect("RuntimeServer compile must produce a template block");
    assert!(
        template.source_map.is_empty(),
        "no source map was requested, but one was emitted:\n{}",
        template.source_map
    );
}

// -------------------- Root-level trailing comment --------------------

/// A disabled comment AFTER the single root element closes — distinct from
/// the already-covered "comment inside the root's own children" case: this
/// one sits in the root's SUFFIX claim range at the template level.
#[test]
fn vdom_root_level_trailing_disabled_comment() {
    let source = r#"<script>
export default {}
</script>
<template><div class="root">hi</div><!-- trail --></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert_parses(&code, "vdom root-level trailing comment");
}

/// SSR sibling of [`vdom_root_level_trailing_disabled_comment`].
#[test]
fn ssr_root_level_trailing_disabled_comment() {
    let source = r#"<script>
export default {}
</script>
<template><div class="root">hi</div><!-- trail --></template>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("trail"), "comment content leaked:\n{code}");
    assert_parses(&code, "ssr root-level trailing comment");
}

// -------------------- Comment-shape × root-class × build-mode --------------------

/// The comment-shape matrix (short/long/whitespace-only), each crossed
/// against static class, dynamic class, AND a development-build negative
/// control (dev keeps comments enabled, so no absorption is even queued —
/// proving the shape axis doesn't interact with the dev/prod axis either).
#[test]
fn vdom_comment_shape_x_root_class_x_build_mode_matrix() {
    let shapes: &[(&str, &str)] = &[
        ("short", "<!--x-->"),
        (
            "long",
            "<!-- this is a much longer comment body spanning many characters to exercise a wider claimed byte range -->",
        ),
        ("whitespace-only", "<!--   -->"),
    ];
    let root_classes: &[(&str, &str)] = &[
        ("static", r#"<div class="root">hi</div>"#),
        ("dynamic", r#"<div :class="cls">hi</div>"#),
    ];
    for (shape_label, comment) in shapes {
        for (class_label, el) in root_classes {
            // Production: the comment must be absorbed, never leaked.
            let prod_source = format!(
                "<script>\nexport default {{ data() {{ return {{ cls: 'x' }} }} }}\n</script>\n<template>{comment}{el}</template>\n"
            );
            let prod_code =
                compile_client(&prod_source, true, VueBackendRequest::Vdom, Some(false));
            assert!(
                !prod_code.contains("<!--"),
                "[{shape_label}/{class_label}/prod] comment leaked:\n{prod_code}"
            );
            assert_parses(
                &prod_code,
                &format!("vdom shape[{shape_label}]/class[{class_label}]/prod"),
            );

            // Development: comments stay enabled by default — negative
            // control proving this axis cross doesn't touch the dev path.
            let dev_source = format!(
                "<script>\nexport default {{ data() {{ return {{ cls: 'x' }} }} }}\n</script>\n<template>{comment}{el}</template>\n"
            );
            let dev_code = compile_client(&dev_source, false, VueBackendRequest::Vdom, Some(false));
            assert!(
                dev_code.contains("_createCommentVNode"),
                "[{shape_label}/{class_label}/dev] dev build must keep the comment as a vnode:\n{dev_code}"
            );
            assert_parses(
                &dev_code,
                &format!("vdom shape[{shape_label}]/class[{class_label}]/dev"),
            );
        }
    }
}

// -------------------- Style × SSR --------------------

/// The style-block axis, crossed with SSR (previously only exercised
/// against VDOM).
#[test]
fn ssr_leading_comment_with_style_block() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
<style>.root { color: red; }</style>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert_parses(&code, "ssr with style block");
}

// -------------------- Script kind × SSR --------------------

/// `<script setup>` non-inline, crossed with SSR (previously the
/// script-setup cell only existed for VDOM).
#[test]
fn ssr_leading_comment_static_class_root_production_script_setup() {
    let source = r#"<script setup>
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert_parses(&code, "ssr script-setup leading comment");
}

// -------------------- Explicit no-comment negative controls × build mode × backend --------------------

/// No comment anywhere in the template — every backend/build-mode
/// combination must simply compile as if this fix never existed (the
/// absorption machinery is a pure no-op with an empty pending vec).
#[test]
fn no_comment_negative_controls_across_backend_and_build_mode() {
    let source_options_api = r#"<script>
export default {}
</script>
<template><div class="root">hi</div></template>
"#;
    for &is_production in &[true, false] {
        let vdom_code = compile_client(
            source_options_api,
            is_production,
            VueBackendRequest::Vdom,
            Some(false),
        );
        assert!(
            !vdom_code.contains("<!--"),
            "[vdom/prod={is_production}] no comment was authored, none should appear:\n{vdom_code}"
        );
        assert_parses(
            &vdom_code,
            &format!("vdom no-comment control prod={is_production}"),
        );

        let ssr_code = compile_server(source_options_api, is_production);
        assert_parses(
            &ssr_code,
            &format!("ssr no-comment control prod={is_production}"),
        );

        let vapor_code = compile_client(
            source_options_api,
            is_production,
            VueBackendRequest::Vapor,
            Some(false),
        );
        assert!(
            !vapor_code.contains("<!--"),
            "[vapor/prod={is_production}] no comment was authored, none should appear:\n{vapor_code}"
        );
        assert_parses(
            &vapor_code,
            &format!("vapor no-comment control prod={is_production}"),
        );
    }
}

// -------------------- Runtime-link: helpers are present AND correctly invoked --------------------

/// Beyond "the JS parses": the headline VDOM shape's generated code links
/// against the intended Vue runtime contract — the exact static-hoist +
/// `_openBlock`/`_createElementBlock` call shape production static-root
/// codegen must emit, matching the established assertion style this crate's
/// own `template/code_gen/vdom/tests.rs` suite uses for every VDOM codegen
/// test (`code.contains(...)` against an exact call-site string, not merely
/// a parse check).
#[test]
fn vdom_headline_shape_links_against_intended_runtime_helpers() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_client(source, true, VueBackendRequest::Vdom, Some(false));
    assert!(!code.contains("<!--"), "comment leaked:\n{code}");
    assert!(
        code.contains(r#"const _hoisted_1 = { class: "root" }"#),
        "the static class object must be hoisted verbatim:\n{code}"
    );
    assert!(
        code.contains("function render(_ctx, _cache, $props, $setup, $data, $options)"),
        "the non-inline render function signature must be intact:\n{code}"
    );
    assert!(
        code.contains(r#"_openBlock(), _createElementBlock("div", _hoisted_1, "hi")"#),
        "the root element must link against the real `_openBlock`/`_createElementBlock` \
         runtime helpers with the hoisted anchor and static text preserved:\n{code}"
    );
}

/// SSR sibling: the generated module links against the intended
/// `@vue/server-renderer` contract (`ssrRender` signature, `_push`,
/// `_ssrRenderAttrs`, `_mergeProps` for the class merge), not just parseable
/// JS.
#[test]
fn ssr_headline_shape_links_against_intended_runtime_helpers() {
    let source = r#"<script>
export default {}
</script>
<template><!-- lead --><div class="root">hi</div></template>
"#;
    let code = compile_server(source, true);
    assert!(!code.contains("lead"), "comment content leaked:\n{code}");
    assert!(
        code.contains(
            "function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options)"
        ),
        "the SSR render function signature must be intact:\n{code}"
    );
    assert!(
        code.contains("_push(`"),
        "SSR codegen must link against the real `_push` runtime helper:\n{code}"
    );
    assert!(
        code.contains("_ssrRenderAttrs(_mergeProps({ class: \"root\" }, _attrs))"),
        "the root element's attrs must link against the real `_ssrRenderAttrs`/`_mergeProps` \
         runtime helpers with the static class preserved:\n{code}"
    );
}
