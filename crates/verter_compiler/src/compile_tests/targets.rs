use super::*;

#[test]
fn vmrs_ide_rejects_noncomplete_runtime_semantics_without_registering_bindings() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroFailure, MacroPartialReason, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome,
    };

    let source = r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script>
<template>{{ title }}</template>"#;
    let cases = [
        (
            "unavailable",
            VueMacroSemanticInput::Unavailable,
            "XMissingMacroSemanticBundle",
        ),
        (
            "partial",
            VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
                entries: vec![MacroRuntimeEntry {
                    syntax_index: 0,
                    macro_index: 0,
                    outcome: MacroRuntimeOutcome::Partial(MacroFailure::new(
                        MacroPartialReason::IncompleteTraversal,
                        None,
                    )),
                }],
            })),
            "XUnavailableMacroSemanticResult",
        ),
        (
            "unknown syntax entry",
            VueMacroSemanticInput::Runtime(crate::test_helpers::runtime_bundle([
                crate::test_helpers::runtime_props_entry(
                    1,
                    0,
                    verter_macro_dto::PropsDefaultsAssociation::None,
                    [crate::test_helpers::runtime_prop(
                        "title",
                        false,
                        [verter_macro_dto::RuntimeConstructor::String],
                    )],
                ),
            ])),
            "XMissingMacroSemanticBundle",
        ),
        (
            "TSC-only",
            VueMacroSemanticInput::Tsc(Arc::new(verter_macro_dto::MacroTscBundle::default())),
            "XMissingMacroSemanticBundle",
        ),
    ];

    for (case, semantics, expected_code) in cases {
        let alloc = Allocator::new();
        let result = compile(
            source,
            &CodegenOptions {
                filename: Some("App.vue".to_string()),
                target: CompileTarget::IDE,
                ..Default::default()
            },
            &VerterCompileOptions::default(),
            &semantics,
            &alloc,
        );

        assert!(
            result
                .errors
                .iter()
                .any(|diagnostic| diagnostic.code == expected_code),
            "{case} must fail closed with {expected_code}: {:?}",
            result.errors
        );
        let code = result.tsx.expect("IDE output remains available").code;
        assert!(
            !code.contains("__props.title"),
            "{case} must not publish an unvalidated prop binding: {code}"
        );
    }
}

#[test]
fn vdom_mode_no_vapor_flag() {
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        !script.code.contains("__vapor"),
        "VDOM mode should NOT contain __vapor flag, got:\n{}",
        script.code
    );
}

#[test]
fn vue_js_ide_carrier_declares_the_same_official_jsx_authority() {
    let source = concat!(
        "<script setup>\n",
        "/** @type {string} */\n",
        "const label = 'ok'\n",
        "</script>\n",
        "<template><div class=\"card\">{{ label }}</div></template>\n",
    );
    let result = compile_tsx_with_force_js(source, true);
    let tsx = result.tsx.expect("Vue JS IDE carrier");
    assert!(tsx.is_jsx, "a JavaScript Vue carrier must remain JSX");
    assert!(
        tsx.code.starts_with("/** @jsxImportSource vue */\n"),
        "JS+JSDoc and TS carriers must select the same official Vue JSX surface:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_override_existing_typed_params() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(e: MouseEvent) {
  return e.clientX
}
</script>
<template>
  <div @click="handler"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handler(e: MouseEvent)"),
        "Typed function param should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Typed function param should not be replaced by inferred tuple rest, got:\n{}",
        tsx.code
    );
}

#[test]
fn ts_combined_target_parses_once_per_ide_completion_value() {
    reset_parse_template_expressions_calls();
    let result = compile_with_target(
        TS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let calls = parse_template_expressions_call_count();
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    assert!(result.template.is_some(), "runtime template missing");
    assert!(result.tsx.is_some(), "tsx block missing");
    // Both lanes use `tsx()`, but completion mode differs: the runtime lane
    // parses with `ide_completion = false` and the IDE/TSX lane with `true`.
    // Those store different binding facts, so the overlay keeps one entry per
    // value — two parses, each reused within its lane (the early script-elision
    // and runtime template-codegen consumers share the single `false` entry).
    assert_eq!(
        calls, 2,
        "combined TS BUNDLER|TSX must parse once per ide_completion value (runtime false + TSX true), got {calls}"
    );
    // Both parses are `tsx()` — `(is_typescript, is_jsx)`; they differ only in
    // the (unrecorded) ide_completion flag (runtime false, then TSX true).
    assert_eq!(
        parse_template_expressions_source_types(),
        vec![(true, true), (true, true)],
        "both TS overlay parses must use tsx() (runtime false, then TSX true)"
    );
}

#[test]
fn js_combined_target_does_not_share_overlay_across_source_types() {
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(
        JS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let calls = parse_template_expressions_call_count();
    // Three distinct overlays, never shared across source types or completion
    // modes: the runtime lane parses with `tsx()/completion=false`, the liveness
    // lane with `jsx()/completion=false` (a JS SFC's liveness uses the JS source
    // type, so it does NOT collide with the runtime `tsx()` entry), and the TSX
    // lane with `jsx()/completion=true`.
    assert_eq!(
        calls, 3,
        "JS BUNDLER|TSX parses tsx runtime + jsx liveness + jsx TSX, got {calls}"
    );
    // Prove WHICH source types parsed — not just that there were three parses.
    // `tsx()` records `(is_typescript = true, is_jsx = true)`; `jsx()` records
    // `(is_typescript = false, is_jsx = true)`. Order: runtime `tsx()`, then the
    // liveness `jsx()` overlay, then the TSX-codegen `jsx()` overlay.
    let source_types = parse_template_expressions_source_types();
    assert_eq!(
        source_types,
        vec![(true, true), (false, true), (false, true)],
        "JS combined must parse runtime=tsx(), liveness=jsx(), TSX=jsx(); got {source_types:?}"
    );
}

// =========================================================================
// D7 — result.inline reflects the runtime inline ACTUALLY happening
// =========================================================================

#[test]
fn result_inline_false_for_ide_target() {
    // IDE emits only TSX (no runtime inline body) — result.inline must not
    // be set merely because the option is on.
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        inline: Some(true),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(result.tsx.is_some(), "IDE target emits TSX");
    assert!(
        !result.inline,
        "result.inline must be false for the IDE target (no runtime inline)"
    );
}

/// A `;` inside a STRING literal is not a statement boundary. The parse says
/// one statement, so the handler takes the expression container.
#[test]
fn semicolon_inside_a_string_literal_is_not_a_statement_boundary() {
    let source = r#"<script setup>
const foo = (s) => s
</script>
<template><button @click="foo('a;b')">x</button></template>"#;
    let vdom = compile_and_validate_template(source);
    assert!(
        vdom.contains("$event => ($setup.foo('a;b'))"),
        "a `;` inside a string must not force a statement body, got:\n{vdom}"
    );
    let vapor = compile_and_validate_vapor_template(source);
    assert!(
        vapor.contains("() => (_ctx.foo('a;b'))"),
        "Vapor must reach the same expression-container decision as VDOM \
         -- and omit the unused $event param, got:\n{vapor}"
    );
}

/// `for (x of xs)` assigns to `x` on each iteration. Emitting the iterated
/// expression before the target produced `for (x of $setup.xs$setup.x of xs)`:
/// the target's prefix landed after the source chunk that already contained it,
/// and the chunk between them was dropped. That does not parse, where the
/// pre-change output (`for (x of xs)`, unresolved) at least did.
#[test]
fn a_for_of_assignment_target_resolves_in_every_position() {
    let source = loop_target_sfc("for (x of xs) log(x)");

    let vdom = compile_and_validate_template(&source);
    assert!(
        vdom.contains("$event => {for ($setup.x of $setup.xs) $setup.log($setup.x)}"),
        "VDOM must resolve the target, the iterated expression and the body, got:\n{vdom}"
    );

    let inline = compile_and_validate_inline_script(&source);
    assert!(
        inline.contains("$event => {for (x.value of xs.value) log(x.value)}"),
        "inline must unwrap every ref in the head, got:\n{inline}"
    );

    let vapor = compile_and_validate_vapor_template(&source);
    assert!(
        vapor.contains("() => { for (_ctx.x of _ctx.xs) _ctx.log(_ctx.x) }"),
        "Vapor must resolve the target, got:\n{vapor}"
    );

    // Negative: the duplicated-iterable shape the out-of-order walk emitted.
    for corrupt in [
        "$setup.xs$setup.x",
        "xs.valuex.value",
        "_ctx.xs_ctx.x",
        " of xs)",
    ] {
        for (backend, code) in [("VDOM", &vdom), ("inline", &inline), ("Vapor", &vapor)] {
            assert!(
                !code.contains(corrupt),
                "[{backend}] must not re-emit the iterated expression ({corrupt:?}):\n{code}"
            );
        }
    }
}

/// The `for…in` form is the dangerous one: `for (x in $setup.xs$setup.x in xs)`
/// PARSES, because `xs$setup` is a legal identifier, so the corruption is a
/// silent wrong-property read rather than a build failure. Pin the emitted text,
/// not merely that it parses.
#[test]
fn a_for_in_assignment_target_resolves_in_every_position() {
    let source = loop_target_sfc("for (x in xs) log(x)");

    let vdom = compile_and_validate_template(&source);
    assert!(
        vdom.contains("$event => {for ($setup.x in $setup.xs) $setup.log($setup.x)}"),
        "VDOM must resolve the target, got:\n{vdom}"
    );
    // The corrupt form parses, so only the text discriminates.
    assert!(
        !vdom.contains("$setup.xs$setup"),
        "the silently-parseable `xs$setup` member read must be gone:\n{vdom}"
    );

    let inline = compile_and_validate_inline_script(&source);
    assert!(
        inline.contains("$event => {for (x.value in xs.value) log(x.value)}"),
        "inline must unwrap every ref in the head, got:\n{inline}"
    );

    let vapor = compile_and_validate_vapor_template(&source);
    assert!(
        vapor.contains("() => { for (_ctx.x in _ctx.xs) _ctx.log(_ctx.x) }"),
        "Vapor must resolve the target, got:\n{vapor}"
    );
}

/// A member target (`for (obj.k of xs)`) writes through the object, so the
/// object root resolves exactly like any other reference.
#[test]
fn a_for_of_member_target_resolves_in_every_position() {
    let source = loop_target_sfc("for (obj.k of xs) log(obj.k)");

    let vdom = compile_and_validate_template(&source);
    assert!(
        vdom.contains("$event => {for ($setup.obj.k of $setup.xs) $setup.log($setup.obj.k)}"),
        "VDOM must resolve the member target's root, got:\n{vdom}"
    );

    let inline = compile_and_validate_inline_script(&source);
    assert!(
        inline.contains("$event => {for (obj.value.k of xs.value) log(obj.value.k)}"),
        "inline must unwrap the member target's root ref, got:\n{inline}"
    );

    let vapor = compile_and_validate_vapor_template(&source);
    assert!(
        vapor.contains("() => { for (_ctx.obj.k of _ctx.xs) _ctx.log(_ctx.obj.k) }"),
        "Vapor must resolve the member target's root, got:\n{vapor}"
    );
}

/// A destructuring target (`for ([a, b] of xs)`) is an assignment PATTERN, not a
/// declaration: each element writes to an existing binding and must resolve.
///
/// This is the one deliberate divergence in the family. `@vue/compiler-sfc`
/// 3.6.0-rc.5 emits `for ([a, b] of $setup.xs)` — it leaves the pattern elements
/// bare, so the loop writes to two undeclared globals and the setup refs are
/// never updated. Verter resolves them.
#[test]
fn a_for_of_destructured_target_resolves_in_every_position() {
    let source = loop_target_sfc("for ([a, b] of xs) log(a)");

    let vdom = compile_and_validate_template(&source);
    assert!(
        vdom.contains("$event => {for ([$setup.a, $setup.b] of $setup.xs) $setup.log($setup.a)}"),
        "VDOM must resolve both pattern elements, got:\n{vdom}"
    );

    let inline = compile_and_validate_inline_script(&source);
    assert!(
        inline.contains("$event => {for ([a.value, b.value] of xs.value) log(a.value)}"),
        "inline must unwrap both pattern element refs, got:\n{inline}"
    );

    let vapor = compile_and_validate_vapor_template(&source);
    assert!(
        vapor.contains("() => { for ([_ctx.a, _ctx.b] of _ctx.xs) _ctx.log(_ctx.a) }"),
        "Vapor must resolve both pattern elements, got:\n{vapor}"
    );

    // Negative: the truncated pattern the out-of-order walk emitted, where the
    // opening `[` was swallowed with the dropped chunk.
    for (backend, code) in [("VDOM", &vdom), ("inline", &inline), ("Vapor", &vapor)] {
        assert!(
            !code.contains("] of xs)"),
            "[{backend}] must not re-emit the iterated expression:\n{code}"
        );
    }
}
