use super::*;

#[test]
fn client_emitter_signature_requires_the_narrow_plan() {
    // This coercion is the architecture guard: it stops compiling if the
    // emitter ever accepts the broad `SvelteRuntimeIr`, drops an input, or
    // changes the typed fail-closed result. Unlike a source-text scan, it is
    // insensitive to formatting and cannot pass because a token appeared in a
    // comment or unrelated declaration.
    type NarrowClientEmitter =
        for<'source, 'plan_ref, 'plan_data, 'html, 'topology, 'options, 'allocator> fn(
            &'source str,
            &'plan_ref ClientModulePlan<'plan_data>,
            &'html StaticTemplatePlan,
            &'topology ClientTopologyPlan,
            ClientEmitOptions<'options>,
            &'allocator Allocator,
        )
            -> Result<
            ClientModule,
            ClientCompileError,
        >;

    let _: NarrowClientEmitter = emit_client_module;

    fn assert_options_are_exact(options: ClientEmitOptions<'_>) {
        let ClientEmitOptions {
            injected_css,
            filename,
            want_source_map,
        } = options;
        let _: (Option<&ScopedCssArtifact>, Option<&str>, bool) =
            (injected_css, filename, want_source_map);
    }
    let _: fn(ClientEmitOptions<'_>) = assert_options_are_exact;
}

#[test]
fn debug_tag_member_argument_fails_closed() {
    // Official `debug_tag_invalid_arguments`: a `{@debug}` argument must be a bare
    // identifier, never a member expression. Accepting `{@debug a.x}` would emit an
    // invalid object key (`a.x: $.snapshot(...)`).
    assert_debug_invalid_arguments(
        "<script>let a = $state(0);</script>\n{@debug a.x}\n<button onclick={() => a++}>x</button>\n",
    );
}

#[test]
fn debug_tag_binary_argument_fails_closed() {
    // A binary-expression `{@debug}` argument is the same official refusal — arguments
    // are identifiers, not arbitrary expressions.
    assert_debug_invalid_arguments(
        "<script>let a = $state(0);</script>\n{@debug a + 1}\n<button onclick={() => a++}>x</button>\n",
    );
}

#[test]
fn invalid_passive_modifier_combinations_fail_closed() {
    // `passive` + `preventDefault` and `passive` + `nonpassive` are official
    // `event_handler_invalid_modifier_combination` compile errors — Verter keeps them
    // fail-closed / refused (matching official's rejection), in BOTH source orders.
    for src in [
        "<script>let n = $state(0);</script>\n<button on:click|passive|preventDefault={() => n++}>x</button>\n",
        "<script>let n = $state(0);</script>\n<button on:click|preventDefault|passive={() => n++}>x</button>\n",
        "<script>let n = $state(0);</script>\n<button on:click|passive|nonpassive={() => n++}>x</button>\n",
    ] {
        assert_fail_closed(src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::NonDelegatedEvent { .. })
        });
    }
}

#[test]
fn type_only_and_phase_imports_reject_with_official_parse_parity() {
    // A TypeScript TYPE-ONLY import in a PLAIN script (decl-level or
    // per-specifier) and an import PHASE (`import defer * as ns`) are official
    // acorn PARSE errors (`js_parse_error`, oracle-probed) — Verter's
    // parse-parity gate rejects each with the EXACT official code BEFORE the
    // import classifier runs (the classifier's type-only / phase arms stay a
    // defense-in-depth stop behind it). Never a Main, never an emitted
    // `import type` / phase statement.
    for (label, import_stmt) in [
        ("type-only-decl", "import type { T } from './t.js';"),
        ("type-only-specifier", "import { type T, x } from './t.js';"),
    ] {
        let src = format!(
            "<script>{import_stmt} let c = $state(0);</script>\n<button onclick={{() => c++}}>{{c}}</button>\n"
        );
        let err = emit_result(&src).expect_err("a type-only import must not compile to a Main");
        assert!(
            matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "js_parse_error"),
            "[{label}] expected the js_parse_error parse-parity reject, got {err:?}"
        );
    }
    // An import PHASE (`import defer * as ns`) parses under the TS-lenient shared
    // reparse (OXC supports the import-phase proposal), so the parse-parity gate
    // does not see it — the
    // import classifier's phase arm refuses it fail-closed (official also rejects,
    // `js_parse_error`; the exact-code parity for phase syntax is the parse-parity
    // surface's breadth, not the import prelude's).
    assert_fail_closed(
        "<script>import defer * as NS from './m.js'; let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ScriptImport {
                    construct: "import phase",
                    ..
                }
            )
        },
    );
}

#[test]
fn import_reassignment_rejects_constant_assignment() {
    // A handler ASSIGNMENT to an import binding is the official
    // `constant_assignment` compile error ("Cannot assign to import") — carried
    // through the rewriter's official-reject channel, never a raw plain write.
    for (label, handler) in [("assign", "() => x = 1"), ("update", "() => x++")] {
        // A DIRECT (non-delegated) event routes the arrow body through the shared
        // rewriter — the write refusal is the rewriter's, not the delegated-shape
        // gate's (which admits only `$state`-write arrows and would mask it).
        let err = emit_result(&format!(
            "<script>import {{ x }} from './m.js'; let c = $state(0);</script>\n<button onmouseenter={{{handler}}} onclick={{() => c++}}>{{c}}</button>\n"
        ))
        .expect_err("a write to an import binding must fail closed");
        let ClientCompileError::OfficialReject(rejection) = err else {
            panic!("[{label}] expected the constant_assignment official reject, got {err:?}");
        };
        assert_eq!(
            rejection.official_code, "constant_assignment",
            "[{label}] the exact official code"
        );
    }
    // A MODULE-slot import write from an instance handler rejects identically (the
    // import resolves up the lexical chain).
    let err = emit_result(
        "<script module>import { m } from './base.js';</script>\n<script>let c = $state(0);</script>\n<button onmouseenter={() => m = 1} onclick={() => c++}>{c}</button>\n",
    )
    .expect_err("a write to a module-slot import must fail closed");
    assert!(
        matches!(&err, ClientCompileError::OfficialReject(r) if r.official_code == "constant_assignment"),
        "a module-import write is the same constant_assignment reject, got {err:?}"
    );
}

#[test]
fn svelte_self_root_placement_fails_closed() {
    // Official `svelte@5.56.10` HARD-ERRORS on a `<svelte:self>` with NO allowed enclosing
    // context (`svelte_self_invalid_placement`: it may only exist inside {#if}/{#each}/
    // {#snippet} blocks or slots passed to components). A ROOT `<svelte:self>` — bare OR
    // `bind:this` — must FAIL CLOSED with the typed component/snippet refusal, never emit
    // the recursive `App(node, {})` / `$.bind_this(App(node, {}), …)` self-call.
    for (label, src) in [
        (
            "root",
            "<script>let { depth } = $props();</script>\n<svelte:self />\n",
        ),
        (
            "root_bind_this",
            "<script>let { depth } = $props(); let x;</script>\n<svelte:self bind:this={x} />\n",
        ),
    ] {
        assert_fail_closed_labeled(label, src, |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. }
                    if *construct == "svelte:self at invalid placement"
            )
        });
    }
}

#[test]
fn svelte_boundary_async_runtime_fails_closed() {
    // The justified ExperimentalAsync deferral: an ASYNC construct inside a `<svelte:boundary>`
    // (here an async `onerror` handler — the experimental-async runtime surface) fails closed
    // via `ExperimentalAsync`, never emitting. Plain (non-async) boundaries are supported.
    assert_fail_closed(
        "<script>let n = $state(0);</script>\n<svelte:boundary onerror={async () => { n++; }}><p>x</p></svelte:boundary>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ExperimentalAsync { .. }),
    );
}

#[test]
fn svelte_host_special_child_content_fails_closed() {
    // F3: `<svelte:window|document|body>` render NO DOM, so child content is the official
    // `svelte_meta_invalid_content` error (`cannot have children`) — fail closed rather than
    // SILENTLY dropping it. RED against the pre-fix no-DOM path that dropped text children.
    for (src, host) in [
        (
            "<script>let c = $state(0);</script>\n<svelte:window onresize={() => c++}>text</svelte:window>\n",
            "svelte:window",
        ),
        (
            "<script>let c = $state(0);</script>\n<svelte:window onresize={() => c++}><p></p></svelte:window>\n",
            "svelte:window",
        ),
        (
            "<script>let c = $state(0);</script>\n<svelte:body onclick={() => c++}>hi</svelte:body>\n",
            "svelte:body",
        ),
        (
            "<script>let c = $state(0);</script>\n<svelte:document onclick={() => c++}>x</svelte:document>\n",
            "svelte:document",
        ),
    ] {
        assert_fail_closed(src, |s| {
            matches!(s, UnsupportedSvelteRuntimeSurface::ComponentOrSnippet { construct, .. } if *construct == host)
        });
    }
    // A self-closing host (no children) still emits (regression).
    assert!(emit_result(
        "<script>let c = $state(0);</script>\n<svelte:window onresize={() => c++} />\n"
    )
    .is_ok());
}

#[test]
fn svelte_options_unsupported_feature_axis_fails_closed() {
    // A `<svelte:options>` axis carrying an unsupported FEATURE option (here inline
    // `immutable`) fails closed with the typed compile-option surface — NOT an
    // official reject (the official compiler accepts `immutable`).
    assert_fail_closed(
        "<svelte:options immutable />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        |s| matches!(
            s,
            UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported {
                option: crate::svelte::runtime::UnsupportedSvelteCompileOption::Immutable,
                origin: crate::svelte::runtime::CompileOptionOrigin::Inline,
                ..
            }
        ),
    );
}

#[test]
fn inspect_trace_nested_block_rejects() {
    // A trace nested inside an `if` consequent (first in the IF's block, but that
    // block is not a FUNCTION body) — official still hard-errors. Same for a bare
    // nested `{ }` block. RED before the fix (silent drop → `if (ok) ;`-class
    // wrong/invalid JS).
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0);</script>\n<button onclick={() => { if (c > 0) { $inspect.trace(); } c++; }}>{c}</button>\n",
    );
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0); $effect(() => { { $inspect.trace(); } c++; });</script>\n<p>{c}</p>\n",
    );
}

#[test]
fn inspect_trace_top_level_rejects_with_exact_code() {
    // A TOP-LEVEL `$inspect.trace();` is an official ERROR
    // (`inspect_trace_invalid_placement`: it must be the first statement of a
    // FUNCTION body) — the production elision does NOT extend to it. It now rejects
    // through the official-reject gate with the EXACT code (previously the generic
    // instance-script-item refusal — the exact-code disposition is the parity
    // improvement).
    assert_inspect_trace_invalid_placement(
        "<script>let c = $state(0); $inspect.trace();</script>\n<button onclick={() => c++}>{c}</button>\n",
    );
}

#[test]
fn empty_template_root_fails_closed() {
    // An EMPTY template (only a `<script>`, no rendered DOM) compiles in official to
    // a component fn with NO `root()` call / NO `$.append` (the body is just the
    // script lowering). Verter's clone-frame path would synthesise a `$.comment()`
    // root and then call `root()` on that NODE → `TypeError`. It fails closed
    // rather than emit an undeclared/broken clone frame. RED against the pre-fix
    // tree (which emitted `var root = $.comment();` + `var fragment = root();`).
    assert_fail_closed("<script>let c=$state(0);</script>\n", |s| {
        matches!(s, UnsupportedSvelteRuntimeSurface::RootTextRegion { .. })
    });
}

#[test]
fn dev_codegen_request_fails_closed() {
    // F4: a DEV-MODE codegen request (`dev_codegen: true`) fails closed — the
    // dev-mode output axis is not emitted; only production output is. The dev signal
    // is distinct from `is_production` (the §1.2 default does NOT request dev). RED
    // against the pre-fix path (which ignored the dev flag and emitted prod output).
    let alloc = Allocator::default();
    let parsed = parse_svelte(HELLO_INPUT);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        dev_codegen: true,
        ..Default::default()
    };
    match compile_client(HELLO_INPUT, &parsed, &opts, &alloc, false, false) {
        Err(ClientCompileError::Unsupported(
            surface @ UnsupportedSvelteRuntimeSurface::DevMode { .. },
        )) => {
            assert_eq!(
                surface.diagnostic_code(),
                "svelte-runtime-unsupported-dev-mode"
            );
        }
        other => panic!("a dev-codegen request must fail closed to DevMode, got: {other:?}"),
    }
    // NEGATIVE: the SAME component WITHOUT dev_codegen emits (the default is not
    // dev — §1.2 must still compile).
    let prod_opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    assert!(
        compile_client(
            HELLO_INPUT,
            &parse_svelte(HELLO_INPUT),
            &prod_opts,
            &alloc,
            false,
            false
        )
        .is_ok(),
        "the production default must still emit (no dev fail-closed)"
    );
}

#[test]
fn ssr_fails_closed_to_block_8() {
    let alloc = Allocator::default();
    let parsed = parse_svelte(HELLO_INPUT);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    match compile_client(
        HELLO_INPUT,
        &parsed,
        &opts,
        &alloc,
        /*ssr*/ true,
        false,
    ) {
        Err(ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::ServerGenerate {
            ..
        })) => {}
        other => panic!("ssr must fail closed to ServerGenerate, got: {other:?}"),
    }
}
#[test]
fn instance_export_const_fails_closed() {
    // An instance-script `export const` is the `$$exports` component-export
    // surface — it fails closed under its OWN identity
    // (`ComponentExportBinding` construct `const`) rather than emitting an
    // `export` inside the component function (invalid JS).
    assert_fail_closed(
        "<script>let n = $state(0); export const helper = 1;</script>\n<button onclick={() => n++}>{n}</button>\n",
        |s| matches!(s, UnsupportedSvelteRuntimeSurface::ComponentExportBinding { construct, .. } if *construct == "const"),
    );
}

#[test]
#[should_panic(expected = "after-update op target not ranked")]
fn unranked_after_update_op_is_a_hard_error_not_a_silent_tail_sort() {
    // An UNRANKED after-update op target is a HARD ERROR — never the retired
    // silent `u32::MAX` tail position (which would misorder the stream without a
    // trace). The panic message names the invariant.
    use super::super::super::client_lifecycle::require_after_update_rank;
    use super::super::super::ir::NodeId;
    let map = rustc_hash::FxHashMap::default();
    let _ = require_after_update_rank(&map, NodeId(7));
}

#[test]
fn paren_receiver_advanced_root_member_fails_closed_via_generic_fallback() {
    // A member access on a parenthesized ADVANCED-only rune root (`($host).foo`,
    // `($bindable).foo`) refuses through the generic `$rune.<member>` fallback —
    // exactly like the plain spellings (official `rune_invalid_name`,
    // oracle-verified against svelte@5.56.10). These roots have no bare-position
    // refusal of their own, so without the member-form classification the
    // parenthesized spelling would slip the scan entirely.
    let cases: &[(&str, &str)] = &[
        (
            "paren_host_member",
            "<script>let c = $state(0); const h = ($host).foo;</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "paren_bindable_member",
            "<script>let c = $state(0); const b = ($bindable).foo;</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ];
    for (label, source) in cases {
        assert_fail_closed_labeled(
            label,
            source,
            |s| matches!(s, UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, .. } if *rune == "$rune.<member>"),
        );
    }
}

#[test]
fn export_family_malformed_siblings_fail_closed_precisely() {
    // Destructured `export let` (object / array pattern) — official ACCEPTS it
    // as a props surface (flag 24 lazy defaults), but it is OUT of the supported
    // export-let shape: fail closed with the PRECISE destructured-export label.
    assert_fail_closed(
        "<script>export let { a, b } = { a: 1, b: 2 };</script>\n<p>hi</p>\n",
        |s| {
            matches!(
                s,
                UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                    construct: "destructured export let",
                    ..
                }
            )
        },
    );
    assert_fail_closed("<script>export let [a] = [1];</script>\n<p>hi</p>\n", |s| {
        matches!(
            s,
            UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                construct: "destructured export let",
                ..
            }
        )
    });
    // `export var` — official lowers it as a prop with the `var` keyword (a
    // DISTINCT surface out of the export-let scope): its own precise label.
    // (Static template: a `{v}` read would hit the earlier template-walk
    // refusal — the item gate owns this statement's identity.)
    assert_fail_closed("<script>export var v = 1;</script>\n<p>hi</p>\n", |s| {
        matches!(
            s,
            UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
                construct: "export var declaration",
                ..
            }
        )
    });
}
