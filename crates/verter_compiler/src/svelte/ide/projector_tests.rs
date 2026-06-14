//! Svelte IDE TSX projection snapshots with NEGATIVE assertions.
//!
//! Each test pins a matrix row's projected TSX shape AND asserts the original
//! Svelte block/tag syntax left NO residue (`{#if`, `{@render`, `<script`,
//! `class:`, …). The clean-type-check gate (through TSGO) lives in the
//! session-side fixtures; these characterize the syntactic transform.

use super::projector::project_svelte_ide;
use crate::svelte::parser::parse_svelte;

/// Project a source and return the generated TSX code.
fn project(source: &str) -> String {
    let parsed = parse_svelte(source);
    project_svelte_ide(source, &parsed, Some("Comp.svelte"), false).code
}

#[test]
fn prelude_opens_the_projection_and_no_script_tag_survives() {
    let code = project("<script lang=\"ts\">let a = 1;</script>\n<div>{a}</div>");
    assert!(code.starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
    assert!(code.contains("let a = 1;"));
    assert!(!code.contains("<script"));
    assert!(!code.contains("</script>"));
    // The render scope function wraps the template.
    assert!(code.contains("function __verter_render()"));
}

#[test]
fn interpolation_is_kept_verbatim() {
    let code = project("<div>{count}</div>");
    assert!(code.contains("{count}"));
}

#[test]
fn if_block_projects_to_a_ternary_with_no_residue() {
    let code = project("<div>{#if cond}<span>a</span>{:else}<span>b</span>{/if}</div>");
    assert!(!code.contains("{#if"), "no {{#if residue: {code}");
    assert!(!code.contains("{:else}"), "no else residue");
    assert!(!code.contains("{/if}"), "no close residue");
    assert!(code.contains("cond ?"), "ternary present: {code}");
}

#[test]
fn empty_else_clause_leaves_no_raw_residue() {
    // P1-1: an EMPTY `{:else}` (no expr, no children) anchors at offset 0 under
    // the retired reverse-scan and never gets rewritten → raw `{:else}` leaks.
    // The `tag_span`-driven overwrite rewrites it regardless.
    let code = project("<div>{#if c}a{:else}{/if}</div>");
    assert!(!code.contains("{:else}"), "no empty-else residue: {code}");
    assert!(!code.contains("{/if}"), "no close residue: {code}");
    assert!(code.contains(") : (<>"), "else ternary arm present: {code}");
}

#[test]
fn empty_then_and_catch_clauses_leave_no_raw_residue() {
    // P1-1: empty `{:then}` / `{:catch}` (no binding, no children) must still be
    // rewritten — no raw `{:then}` / `{:catch}` leaks into the projected TSX.
    let code = project("<div>{#await p}load{:then}{:catch}{/await}</div>");
    assert!(!code.contains("{:then}"), "no empty-then residue: {code}");
    assert!(!code.contains("{:catch}"), "no empty-catch residue: {code}");
    assert!(!code.contains("{/await}"), "no /await residue: {code}");
    // Synthetic bindings appear for the empty clauses.
    assert!(
        code.contains("__verter_v: __VA"),
        "synthetic then binding: {code}"
    );
    assert!(
        code.contains("__verter_e: unknown"),
        "synthetic catch binding: {code}"
    );
}

#[test]
fn each_block_projects_to_map_with_no_residue() {
    let code = project("<ul>{#each items as item, i}<li>{item}</li>{/each}</ul>");
    assert!(!code.contains("{#each"), "no each residue: {code}");
    assert!(!code.contains("{/each}"));
    assert!(!code.contains(" as item"), "no ` as` residue: {code}");
    assert!(code.contains(".map((item, i) =>"), "map present: {code}");
}

#[test]
fn each_without_item_uses_a_synthetic_param() {
    let code = project("<ul>{#each list}<li>x</li>{/each}</ul>");
    assert!(!code.contains("{#each"));
    assert!(code.contains(".map(("), "map present: {code}");
}

#[test]
fn render_tag_projects_to_a_call_with_no_residue() {
    let code = project("<div>{@render row(item)}</div>");
    assert!(!code.contains("{@render"), "no @render residue: {code}");
    assert!(code.contains("{row(item)}"), "call present: {code}");
}

#[test]
fn snippet_block_projects_to_a_branded_declarator_hoisted_to_scope_top() {
    let code = project("<div>{@render row()}{#snippet row()}<span>hi</span>{/snippet}</div>");
    assert!(!code.contains("{#snippet"), "no #snippet residue: {code}");
    assert!(!code.contains("{/snippet}"));
    assert!(
        code.contains("const row = __verter_snippet("),
        "branded declarator present: {code}"
    );
    // The render call referencing `row` is still present.
    assert!(code.contains("{row()}"), "render call present: {code}");
    // D-ap ordering: the declarator precedes the `return (` of the render fn.
    let decl_idx = code.find("const row = __verter_snippet(").unwrap();
    let return_idx = code.find("return (<>").unwrap();
    assert!(
        decl_idx < return_idx,
        "snippet declarator must be hoisted ABOVE the render return (D-ap TDZ): {code}"
    );
}

#[test]
fn legacy_on_event_projects_verbatim_lowercase_never_oncamel() {
    let code = project("<button on:click={handle}>x</button>");
    assert!(!code.contains("on:click"), "no on: residue: {code}");
    assert!(
        code.contains("onclick={handle}"),
        "verbatim lowercase: {code}"
    );
    assert!(
        !code.contains("onClick"),
        "the onClick rename is RETIRED: {code}"
    );
}

#[test]
fn svelte5_event_attribute_stays_verbatim_lowercase() {
    let code = project("<button onclick={handle}>x</button>");
    assert!(code.contains("onclick={handle}"));
    assert!(!code.contains("onClick"));
}

#[test]
fn css_custom_property_attribute_is_stripped_from_jsx_position() {
    let code = project("<div --track-color={c}>x</div>");
    // A `--`-prefixed name is not a valid JSX attribute identifier — no
    // `--track-color` residue survives in the projection (D-ap).
    assert!(
        !code.contains("--track-color"),
        "no `--` custom-property attribute residue: {code}"
    );
    // The value expression stays present, void-checked via the spread (mapped,
    // checkable) — no `--` JSX attribute survives.
    assert!(
        code.contains("__verter_void(c)"),
        "value void-checked present: {code}"
    );
}

#[test]
fn bind_value_projects_to_a_checkable_attribute() {
    let code = project("<input bind:value={name} />");
    assert!(!code.contains("bind:value"), "no bind: residue: {code}");
    assert!(
        code.contains("value={name}"),
        "checkable value pair: {code}"
    );
}

#[test]
fn bind_this_is_out_of_scope_with_a_typed_diagnostic_and_void_check() {
    // P1-2: `bind:this` is out-of-scope v1. It must be stripped from the JSX,
    // the bound expression void-checked, and the typed-unsupported diagnostic
    // pushed (naming the binding).
    let source = "<input bind:this={inputEl} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "unsupported-binding diagnostic present: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("bind:this"),
        "no bind:this residue: {}",
        projection.code
    );
    assert!(
        projection.code.contains("__verter_void(inputEl)"),
        "bound expression void-checked: {}",
        projection.code
    );
}

#[test]
fn bind_group_is_out_of_scope_with_a_typed_diagnostic() {
    let source = "<input bind:group={selected} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "unsupported-binding diagnostic: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:group"));
    assert!(projection.code.contains("__verter_void(selected)"));
}

#[test]
fn bind_checked_stays_supported_no_diagnostic() {
    // DISCRIMINATING: `bind:checked` is SUPPORTED — no diagnostic, projects to a
    // checkable `checked={…}` pair.
    let source = "<input bind:checked={on} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "no unsupported diagnostic for bind:checked: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("bind:checked"),
        "no bind: residue"
    );
    assert!(
        projection.code.contains("checked={on}"),
        "checkable pair: {}",
        projection.code
    );
}

#[test]
fn bind_this_on_a_component_is_out_of_scope_with_a_diagnostic() {
    // `bind:this` is out-of-scope v1 in EVERY context — on a COMPONENT it binds
    // the instance, NOT a $props-checkable surface. It must NOT take the
    // component bind:prop supported path.
    let source = "<MyComp bind:this={ref} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "bind:this on a component must be unsupported: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("bind:this"),
        "no bind:this residue"
    );
    assert!(
        !projection.code.contains("this={ref}"),
        "must NOT leak a bare this={{ref}} attribute: {}",
        projection.code
    );
    assert!(
        projection.code.contains("__verter_void(ref)"),
        "void-checked"
    );
}

#[test]
fn bind_value_shorthand_projects_to_a_self_bound_pair() {
    // The valueless shorthand `bind:value` binds the same-named local — it must
    // become `value={value}`, NOT a bare `value` (which would be boolean `true`
    // and not check the bound variable).
    let code = project("<input bind:value />");
    assert!(!code.contains("bind:value"), "no bind: residue: {code}");
    assert!(
        code.contains("value={value}"),
        "self-bound pair present: {code}"
    );
}

#[test]
fn component_bind_prop_stays_supported_no_diagnostic() {
    // DISCRIMINATING: component `bind:prop` ($bindable) is SUPPORTED — a
    // capitalised tag name takes the supported path regardless of local name.
    let source = "<MyInput bind:custom={v} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "no unsupported diagnostic for component bind:prop: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:custom"), "no bind: residue");
    assert!(projection.code.contains("custom={v}"), "prop pair present");
}

#[test]
fn style_directive_is_out_of_scope_with_a_typed_diagnostic_and_void_check() {
    // P2-1: `style:` is out-of-scope v1. Stripped, value void-checked, typed
    // diagnostic on the directive span.
    let source = "<div style:color={c}>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-style-directive"),
        "style-directive diagnostic present: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("style:color"),
        "no style: residue"
    );
    assert!(
        projection.code.contains("__verter_void(c)"),
        "value void-checked: {}",
        projection.code
    );
}

#[test]
fn transition_in_out_animate_directives_emit_typed_diagnostics_and_void_check() {
    // P2-1: transition/in/out/animate — stripped, params void-checked, typed
    // diagnostic on the directive span.
    for (src, name) in [
        ("<div transition:fade={{ duration: d }}>x</div>", "d"),
        ("<div in:fly={{ y: a }}>x</div>", "a"),
        ("<div out:slide={{ x: b }}>x</div>", "b"),
        ("<div animate:flip={{ delay: e }}>x</div>", "e"),
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-unsupported-transition-directive"),
            "transition-directive diagnostic for {src}: {:?}",
            projection.diagnostics
        );
        assert!(
            !projection.code.contains("transition:")
                && !projection.code.contains("in:")
                && !projection.code.contains("out:")
                && !projection.code.contains("animate:"),
            "no directive residue for {src}: {}",
            projection.code
        );
        // The directive params object is void-checked (the inner var checks
        // through it): `{...(__verter_void({ … name }), {})}`.
        assert!(
            projection.code.contains("__verter_void(") && projection.code.contains(name),
            "params void-checked for {src}: {}",
            projection.code
        );
    }
}

#[test]
fn class_directive_projects_to_a_data_attribute() {
    let code = project("<div class:active={isActive}>x</div>");
    assert!(!code.contains("class:active"), "no class: residue: {code}");
    assert!(code.contains("{isActive}"), "value present: {code}");
}

#[test]
fn html_tag_projects_to_a_string_checkable_position() {
    let code = project("<div>{@html markup}</div>");
    assert!(!code.contains("{@html"), "no @html residue: {code}");
    assert!(code.contains("markup"), "value present: {code}");
}

#[test]
fn attach_tag_projects_to_the_checker_argument() {
    let code = project("<canvas {@attach draw}></canvas>");
    // `{@attach draw}` as a tag is rendered through __verter_attach.
    assert!(!code.contains("{@attach"), "no @attach residue: {code}");
}

#[test]
fn debug_tag_projects_to_a_void_reference() {
    let code = project("<div>{@debug a, b}</div>");
    assert!(!code.contains("{@debug"), "no @debug residue: {code}");
    assert!(code.contains("__verter_void"), "void ref present: {code}");
}

#[test]
fn const_declaration_tag_hoists_to_a_sibling_visible_statement() {
    let code = project("<div>{const total = a + b}{total}</div>");
    // No raw declaration-tag residue (`{const total` directly in the markup).
    assert!(
        !code.contains("<div>{const total"),
        "no declaration-tag residue: {code}"
    );
    // The const is HOISTED to a real statement (D-ap sibling-run scope) — it
    // precedes the render `return` so the sibling `{total}` reference resolves.
    assert!(
        code.contains("const") && code.contains("total = a + b"),
        "const present: {code}"
    );
    let decl_idx = code.find("total = a + b").unwrap();
    let return_idx = code.find("return (<>").unwrap();
    assert!(
        decl_idx < return_idx,
        "the declaration must hoist ABOVE the render return (sibling-visible): {code}"
    );
    // The sibling `{total}` reference is still present.
    assert!(
        code.contains("{total}"),
        "sibling reference present: {code}"
    );
}

#[test]
fn component_style_block_is_stripped() {
    let code = project("<div>x</div>\n<style>.a { color: red; }</style>");
    assert!(
        !code.contains("color: red"),
        "style content stripped: {code}"
    );
    assert!(!code.contains("<style"), "no style tag: {code}");
}

#[test]
fn await_block_out_of_scope_expression_emits_diagnostic_but_projects() {
    let source = "<div>{#await p}loading{:then v}{v}{:catch e}{e}{/await}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(!projection.code.contains("{#await"), "no #await residue");
    assert!(!projection.code.contains("{:then"), "no :then residue");
    assert!(!projection.code.contains("{/await}"), "no /await residue");
}

#[test]
fn function_binding_is_out_of_scope_with_a_typed_diagnostic() {
    let source = "<input bind:value={get, set} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-function-binding"),
        "function-binding diagnostic present: {:?}",
        projection.diagnostics
    );
    // The binding is stripped from the JSX position (out of scope).
    assert!(!projection.code.contains("bind:value"));
}

#[test]
fn await_expression_in_interpolation_records_the_experimental_diagnostic() {
    let source = "<div>{await thing}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "await-experimental diagnostic present: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_in_instance_script_top_level_records_the_diagnostic() {
    // P2-2: await at instance-script top level (D-bg position 1).
    let source = "<script lang=\"ts\">const x = await fetchThing();</script>\n<div>x</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "script-top-level await diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_inside_derived_arg_records_the_diagnostic() {
    // P2-2: await inside `$derived(...)` / `$derived.by(...)` args (position 2),
    // in BOTH script and markup.
    let script_src = "<script lang=\"ts\">const v = $derived(await load());</script>\n<div>x</div>";
    let parsed = parse_svelte(script_src);
    let projection = project_svelte_ide(script_src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "$derived(await …) in script: {:?}",
        projection.diagnostics
    );

    // A DIRECT await in the derived arg (the experimental reactive form) IS
    // flagged in markup; an await nested inside an async arrow is NOT (ordinary
    // TS) — the discriminating async-fn case is covered separately.
    let markup_src = "<div>{$derived(await load())}</div>";
    let parsed = parse_svelte(markup_src);
    let projection = project_svelte_ide(markup_src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "$derived(await …) in markup: {:?}",
        projection.diagnostics
    );
}

#[test]
fn nested_await_in_markup_records_the_diagnostic() {
    // P2-2: a NESTED markup await `{foo(await bar())}` (not a leading prefix) is
    // caught by the word-boundary scan — the retired leading-prefix check missed
    // it.
    let source = "<div>{foo(await bar())}</div>";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "nested markup await diagnostic: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_inside_an_async_function_body_is_not_flagged() {
    // An `await` inside an `async function` / `async () =>` body in the script
    // is ORDINARY TypeScript — NOT the experimental Svelte await-expression. It
    // must NOT be flagged. DISCRIMINATING against a top-level await (which IS).
    for src in [
        "<script lang=\"ts\">async function f() { await load(); }</script>\n<div>x</div>",
        "<script lang=\"ts\">const g = async () => { await load(); };</script>\n<div>x</div>",
        "<script lang=\"ts\">const o = { async m() { await load(); } };</script>\n<div>x</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "await inside an async fn must NOT be flagged for {src}: {:?}",
            projection.diagnostics
        );
    }

    // DISCRIMINATING: a TOP-LEVEL script await IS flagged.
    let top = "<script lang=\"ts\">const x = await load();</script>\n<div>x</div>";
    let parsed = parse_svelte(top);
    let projection = project_svelte_ide(top, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "a top-level script await MUST be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn await_in_async_arrow_expression_body_is_not_flagged() {
    // An async arrow with an EXPRESSION body (no braces) — `async () => await x`
    // — is ordinary TS; its await must NOT be flagged.
    for src in [
        "<script lang=\"ts\">const f = async () => await load();</script>\n<div>x</div>",
        "<div>{$derived.by(async () => await load())}</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "await in an async arrow expr body must NOT be flagged for {src}: {:?}",
            projection.diagnostics
        );
    }
}

#[test]
fn await_inside_a_template_literal_interpolation_is_scanned() {
    // A top-level template-literal `${await …}` IS the experimental form and
    // must be flagged (the scan recurses into `${}`).
    let top = "<script lang=\"ts\">const x = `${await load()}`;</script>\n<div>x</div>";
    let parsed = parse_svelte(top);
    let projection = project_svelte_ide(top, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "a top-level `${{await}}` must be flagged: {:?}",
        projection.diagnostics
    );

    // DISCRIMINATING: the same inside an async arrow's template literal is NOT
    // flagged (ordinary TS).
    let shadowed =
        "<script lang=\"ts\">const f = async () => `${await load()}`;</script>\n<div>x</div>";
    let parsed = parse_svelte(shadowed);
    let projection = project_svelte_ide(shadowed, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "an `${{await}}` inside an async arrow must NOT be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn nested_async_arrows_do_not_unshadow_an_outer_async_body() {
    // `async () => wrap(async () => await a()) + await b()` — BOTH awaits are
    // inside the OUTER async arrow body and must NOT be flagged (the inner arrow
    // must not lose the outer's async shadow). Grammar-correct via OXC scoping.
    let src =
        "<script lang=\"ts\">const f = async () => wrap(async () => await a()) + await b();</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "neither await in nested async arrows may be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn asi_terminated_async_arrow_does_not_swallow_a_following_top_level_await() {
    // Semicolon-less ASI: `const f = async () => await a()\nconst x = await b()`
    // — `await b()` is a real top-level experimental await and MUST be flagged
    // (the async arrow body ended at the statement boundary). Grammar-correct.
    let src =
        "<script lang=\"ts\">const f = async () => await a()\nconst x = await b()</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    let count = projection
        .diagnostics
        .iter()
        .filter(|d| d.code == "svelte-await-experimental")
        .count();
    assert_eq!(
        count, 1,
        "exactly the top-level `await b()` must flag (ASI ends the arrow body): {:?}",
        projection.diagnostics
    );
}

#[test]
fn template_literal_close_does_not_swallow_a_following_top_level_await() {
    // A completed template literal must not leave the scanner in template mode —
    // a following top-level await is still flagged.
    let src =
        "<script lang=\"ts\">const s = `${foo}`; const y = await load();</script>\n<div>x</div>";
    let parsed = parse_svelte(src);
    let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the top-level await after a template literal must be flagged: {:?}",
        projection.diagnostics
    );
}

#[test]
fn async_arrow_expr_shadows_the_whole_body_not_just_the_first_subexpr() {
    // `async () => (await a) + await b` — BOTH awaits are inside the async arrow
    // expression body and must NOT be flagged (a `(await a)` close must not end
    // the arrow-expr shadow).
    let shadowed =
        "<script lang=\"ts\">const f = async () => (await a) + await b;</script>\n<div>x</div>";
    let parsed = parse_svelte(shadowed);
    let projection = project_svelte_ide(shadowed, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "neither await in the async arrow body may be flagged: {:?}",
        projection.diagnostics
    );

    // DISCRIMINATING: `const a = async () => await x, b = await y;` — the arrow
    // body ends at the comma, so `await y` (a top-level initializer) IS flagged
    // while `await x` (inside the arrow) is NOT — exactly one diagnostic.
    let mixed =
        "<script lang=\"ts\">const a = async () => await x, b = await y;</script>\n<div>x</div>";
    let parsed = parse_svelte(mixed);
    let projection = project_svelte_ide(mixed, &parsed, Some("C.svelte"), false);
    let count = projection
        .diagnostics
        .iter()
        .filter(|d| d.code == "svelte-await-experimental")
        .count();
    assert_eq!(
        count, 1,
        "exactly the top-level `await y` must flag (arrow body ends at the comma): {:?}",
        projection.diagnostics
    );
}

#[test]
fn dotted_component_bind_prop_stays_supported() {
    // A dotted/namespaced component `<ns.Widget bind:custom={v} />` is a
    // COMPONENT (parser classifies dotted tags as components) — its bind:prop is
    // supported, no diagnostic.
    let source = "<ns.Widget bind:custom={v} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        !projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-unsupported-binding"),
        "dotted component bind:prop must be supported: {:?}",
        projection.diagnostics
    );
    assert!(!projection.code.contains("bind:custom"), "no bind: residue");
}

#[test]
fn await_word_boundary_does_not_false_positive_on_identifiers_or_strings() {
    // DISCRIMINATING: `awaited`/`myawait` identifiers and an `await` inside a
    // string literal must NOT trigger the diagnostic.
    for src in [
        "<div>{awaited}</div>",
        "<div>{myawait + awaiter}</div>",
        "<div>{\"await this\"}</div>",
    ] {
        let parsed = parse_svelte(src);
        let projection = project_svelte_ide(src, &parsed, Some("C.svelte"), false);
        assert!(
            !projection
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-await-experimental"),
            "no false-positive await diagnostic for {src}: {:?}",
            projection.diagnostics
        );
    }
}

#[test]
fn deprecated_special_element_records_a_diagnostic_but_projects() {
    let source = "<svelte:self count={n} />";
    let parsed = parse_svelte(source);
    let projection = project_svelte_ide(source, &parsed, Some("C.svelte"), false);
    assert!(
        projection
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-deprecated-special-element"),
        "deprecated-special-element diagnostic: {:?}",
        projection.diagnostics
    );
    assert!(
        !projection.code.contains("svelte:self"),
        "name rewritten: {}",
        projection.code
    );
}

#[test]
fn empty_template_still_produces_a_valid_module() {
    let code = project("<script lang=\"ts\">export let x: number;</script>");
    assert!(code.contains("export let x: number;"));
    assert!(code.contains("function __verter_render()"));
}
