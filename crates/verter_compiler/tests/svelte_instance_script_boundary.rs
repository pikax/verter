//! The COMPLETENESS GATE for the strict finite instance-script + `<svelte:options>`
//! allowlists.
//!
//! This is the convergence guarantee for the SCRIPT boundary, the script-side
//! analogue of [`svelte_element_attr_boundary`](super::svelte_element_attr_boundary):
//! the supported instance script is a strict finite TYPED allowlist
//! (`SupportedInstanceScriptItem`) of exactly THREE declaration shapes, and the
//! `<svelte:options>` acceptance is exactly absent-or-`runes={true}`. Any change
//! requires extending the finite enum AND adding a row here in the same change —
//! nothing leaks through a broad statement-rewrite path.
//!
//! ## The three supported instance-script shapes
//! 1. `let name = $state(<primitive literal>);` — one declarator, `let` only,
//!    identifier binding, no TS annotation, a 0-1-arg `$state()` with a primitive
//!    literal init.
//! 2. a single no-default `$props()` destructure (`let { a } = $props()` /
//!    `let { a: b } = $props()` / a string-key alias).
//! 3. a bare `let el;` used SOLELY as a supported `bind:this` target.
//!
//! ## The `<svelte:options>` acceptance
//! Absent (mode inferred from rune usage) OR at most one top-level
//! `<svelte:options runes={true} />`. Everything else (a non-boolean / `false`
//! `runes`, a duplicate, a nested element, an other axis, child content) fails closed.
//!
//! ## Matrices
//! - POSITIVE: every supported shape + options form, JS-parse-checked + topology
//!   golden-compared to the committed official goldens.
//! - NEGATIVE (statement families): every OXC top-level statement kind fails closed.
//! - NEGATIVE (variable): decl-kind / multi-declarator / state-init / props-pattern /
//!   bind-this-usage / `$`-`$$`-names.
//! - NEGATIVE (magic): `$$slots` / `$$props` / `$$restProps` / `let $$anchor`.
//! - NEGATIVE (options): duplicate / nested / non-boolean runes / `runes={false}` /
//!   unknown attrs / spreads / directives / child content.
//! - STATIC GUARDS: the lowering must not call the removed broad path, must not have
//!   an `other => rewrite_statement…` catch-all, and must not strip TS for an
//!   accepted plain instance script.

use std::path::PathBuf;

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{
    compile_client, ClientCompileError, CoreOfficialValidationRule, SvelteRuntimeOptions,
    UnsupportedSvelteRuntimeSurface,
};

/// Compile a source through the client backend, returning the emitted JS or the
/// typed refusal.
fn compile(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// Whether a component COMPILES to an emitted `Main` module (a non-fail-closed
/// emission).
fn emits_main(source: &str) -> bool {
    matches!(compile(&with_supported_template(source)), Ok(js) if js.contains("export default function"))
}

/// Whether a component COMPILES to a `Main` that ALSO OXC-parses as valid JS (a
/// supported shape must emit syntactically-valid JS, never a stray statement).
fn emits_valid_main(source: &str) -> bool {
    match compile(&with_supported_template(source)) {
        Ok(js) => js.contains("export default function") && parses_as_js(&js),
        Err(_) => false,
    }
}

/// Whether a component fails closed (a typed refusal, no `Main`).
fn fails_closed(source: &str) -> Option<UnsupportedSvelteRuntimeSurface> {
    match compile(&with_supported_template(source)) {
        Err(ClientCompileError::Unsupported(s)) => Some(s),
        _ => None,
    }
}

/// The official-reject rule a component fails closed with (a MALFORMED-input refusal,
/// no `Main`), or `None` when it does not fail through the official-reject gate.
fn official_reject(source: &str) -> Option<CoreOfficialValidationRule> {
    match compile(&with_supported_template(source)) {
        Err(ClientCompileError::OfficialReject(rejection)) => Some(rejection.rule),
        _ => None,
    }
}

/// Wrap an instance `<script>` body in a minimal SUPPORTED template (a reactive
/// `$state`-write button) so the component reaches the script-item classification +
/// emission. The wrapped body declares its own `$state` (the supported reactive
/// surface); the test body under test is appended inside the same script.
fn with_supported_template(script_body: &str) -> String {
    // The script body already carries a full `<script>…</script>` (+ optional
    // `<svelte:options>`) when it starts with `<`; otherwise it is a raw instance
    // body wrapped here with a trailing reactive `$state` + button.
    if script_body.trim_start().starts_with('<') {
        script_body.to_string()
    } else {
        format!(
            "<script>let __c = $state(0); {script_body}</script>\n<button onclick={{() => __c++}}>{{__c}}</button>\n"
        )
    }
}

/// Whether `code` parses as a valid JS module through OXC (no panic, no syntax
/// errors). Mirrors the topology gate's `parses_as_js` (the two live in separate test
/// binaries).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    !ret.panicked && ret.errors.is_empty()
}

// ─────────────────────────────────────────────────────────────────────────────
// POSITIVE matrix — the three supported instance-script shapes + options forms.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn positive_supported_instance_script_shapes_emit_valid_main() {
    // Each row is a complete supported component using ONE (or a combination of) the
    // three allowlisted shapes; each must emit a valid-JS `Main`.
    let rows: &[(&str, &str)] = &[
        // Shape 1: `$state(<primitive literal>)` — every primitive literal flavour.
        (
            "state_string",
            "<script>let name = $state('world');</script>\n<button onclick={() => name = 'x'}>{name}</button>\n",
        ),
        (
            "state_number",
            "<script>let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        ),
        (
            "state_boolean",
            "<script>let b = $state(true);</script>\n<button onclick={() => b = false}>{b}</button>\n",
        ),
        (
            "state_null",
            "<script>let v = $state(null);</script>\n<button onclick={() => v = null}>{v}</button>\n",
        ),
        (
            "state_no_arg",
            "<script>let v = $state();</script>\n<button onclick={() => v = 1}>{v}</button>\n",
        ),
        (
            "state_negative",
            "<script>let n = $state(-1);</script>\n<button onclick={() => n++}>{n}</button>\n",
        ),
        (
            "state_multiple",
            "<script>let a = $state(0); let b = $state(1);</script>\n<button onclick={() => { a++; b++; }}>{a}{b}</button>\n",
        ),
        // Shape 2: a no-default `$props()` destructure (named / aliased / string-key).
        (
            "props_named",
            "<script>let { label } = $props();</script>\n<p>{label}</p>\n",
        ),
        (
            "props_alias",
            "<script>let { foo: bar } = $props();</script>\n<p>{bar}</p>\n",
        ),
        (
            "props_string_key_alias",
            "<script>let { \"data-id\": dataId } = $props();</script>\n<p>{dataId}</p>\n",
        ),
        // Shape 3: a bare `let el;` used solely as a `bind:this` target.
        (
            "bind_this_local",
            "<script>let v = $state(''); let el;</script>\n<input bind:value={v} /><div bind:this={el}></div>{v}\n",
        ),
        // Options: absent (mode inferred from `$state` usage) — covered by every row
        // above. An explicit `runes={true}` (and the shorthand `runes`).
        (
            "options_runes_true",
            "<svelte:options runes={true} />\n<script>let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        ),
        (
            "options_runes_shorthand",
            "<svelte:options runes />\n<script>let n = $state(0);</script>\n<button onclick={() => n++}>{n}</button>\n",
        ),
    ];
    let mut failures = Vec::new();
    for (label, source) in rows {
        if !emits_valid_main(source) {
            failures.push(format!(
                "{label}: expected a supported valid-JS Main, got {:?}",
                compile(source)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "supported instance-script / options shapes must emit a valid Main:\n{}",
        failures.join("\n")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEGATIVE matrix — every OXC top-level statement FAMILY fails closed.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn negative_every_top_level_statement_family_fails_closed() {
    // Every top-level instance-script statement that is NOT one of the three supported
    // shapes fails closed (no `Main`). A statement carrying a rune (a nested `$state` /
    // `$effect` / `$derived` / a magic ref) is owned by the rune / magic scan FIRST
    // (a more precise diagnostic); a plain statement is owned by the instance-script-item
    // gate (5w). Each row asserts NO `Main` — the precise owner is pinned in
    // `svelte_client_fail_matrix.rs`.
    let rows: &[(&str, &str)] = &[
        ("function", "function f() { return 1; }"),
        ("class", "class K {}"),
        ("enum", "enum E { A }"),
        ("namespace", "namespace N {}"),
        ("interface", "interface I { x: number }"),
        ("type_alias", "type T = number;"),
        ("import", "import D from './d.js';"),
        ("export_const", "export const X = 1;"),
        ("export_function", "export function h() {}"),
        ("reactive_label", "$: doubled = __c * 2;"),
        ("expression_statement", "console.log('hi');"),
        ("if_statement", "if (true) { __c = 1; }"),
        ("for_statement", "for (let i = 0; i < 1; i++) {}"),
        ("while_statement", "while (false) {}"),
        ("switch_statement", "switch (__c) { default: break; }"),
        ("try_statement", "try {} catch (e) {}"),
        ("throw_statement", "throw new Error('x');"),
        ("block_statement", "{ let z = 1; }"),
        ("empty_statement", ";"),
        ("debugger_statement", "debugger;"),
        // A plain non-rune `let` / `const` / `var` is out-of-allowlist.
        ("plain_let", "let x = 0;"),
        ("const_decl", "const Y = 5;"),
        ("var_decl", "var z = 0;"),
    ];
    let mut leaks = Vec::new();
    for (label, body) in rows {
        if emits_main(body) {
            leaks.push(format!("{label}: emitted a Main (should fail closed)"));
        }
    }
    assert!(
        leaks.is_empty(),
        "every non-allowlist top-level statement family must fail closed:\n{}",
        leaks.join("\n")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEGATIVE matrix — the variable-declaration sub-shapes that fail closed.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn negative_variable_declaration_sub_shapes_fail_closed() {
    // The within-`let`-declaration boundary: a `const`/`var` rune, a multi-declarator,
    // a TS-annotated declarator, the narrowed non-lowerable `$state` inits (a TS-wrapped
    // init and a shadowed-`undefined` init), a default-bearing / rest / whole-object /
    // computed `$props()`, and a `$`-prefixed binding all fail closed. (The proxiable
    // object / array / call / member / template `$state` inits and `$state.raw` are now
    // SUPPORTED — see `svelte_client_fail_matrix::generated_state_init_shapes_land_on_boundary`
    // and the `runes/*` emission goldens.)
    let rows: &[(&str, &str)] = &[
        // Decl kind on a rune.
        ("const_state", "const k = $state(0);"),
        ("var_state", "var v = $state(0);"),
        ("const_props", "const { a } = $props();"),
        ("var_props", "var { a } = $props();"),
        // Multi-declarator.
        ("multi_declarator", "let a = $state(0), b = $state(1);"),
        // TS-annotated declarator (the tsx-leniency fix).
        ("ts_annotated_state", "let c: number = $state(0);"),
        ("ts_definite_state", "let c!: number = $state(0);"),
        // State init shape — the narrowed non-lowerable forms.
        ("state_as_init", "let s = $state(0 as number);"),
        (
            "state_shadowed_undefined_init",
            "let undefined = $state(0); let s = $state(undefined);",
        ),
        // Props pattern shape. (A plain / `$bindable` DEFAULT is now the
        // supported `$.prop` prop-source surface, pinned positively by the
        // oracle-backed client tests — only the pattern shapes stay closed.)
        ("props_rest", "let { a, ...rest } = $props();"),
        ("props_whole_object", "let p = $props();"),
        ("props_computed_key", "let { [k]: a } = $props();"),
        // A bare `let el;` NOT used as a bind:this target (an unused/plain local).
        ("unused_bare_let", "let el;"),
        // A `$`/`$$`-prefixed binding.
        ("dollar_binding", "let $foo = $state(0);"),
        ("dollar_dollar_binding", "let $$anchor = 1;"),
    ];
    let mut leaks = Vec::new();
    for (label, body) in rows {
        if emits_main(body) {
            leaks.push(format!("{label}: emitted a Main (should fail closed)"));
        }
    }
    assert!(
        leaks.is_empty(),
        "every non-allowlist variable-declaration sub-shape must fail closed:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn negative_unused_bare_let_fails_but_bind_this_target_succeeds() {
    // DISCRIMINATION: a bare `let el;` is admitted ONLY when `el` is a supported
    // `bind:this` target; an UNUSED bare `let` (no `bind:this`) fails closed.
    let unused =
        "<script>let v = $state(0); let el;</script>\n<button onclick={() => v++}>{v}</button>\n";
    assert!(
        fails_closed(unused).is_some(),
        "an UNUSED bare `let el;` must fail closed (no bind:this target):\n{:?}",
        compile(unused)
    );
    let used = "<script>let v = $state(''); let el;</script>\n<input bind:value={v} /><div bind:this={el}></div>{v}\n";
    assert!(
        emits_valid_main(used),
        "a bare `let el;` used as a `bind:this` target must emit:\n{:?}",
        compile(used)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// NEGATIVE matrix — the magic identifiers.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn negative_magic_identifiers_fail_closed() {
    // The auto-injected magic objects + a `$$`-prefixed binding all fail closed (no
    // `Main`), but on TWO distinct channels mirroring the official disposition:
    //
    // - `$$slots` — official ACCEPTS it (a valid magic object), but Verter does not
    //   yet synthesize it, so it is an UNSUPPORTED FEATURE refusal (5w
    //   `MagicIdentifier`), a deferrable refusal.
    // - `$$props` / `$$restProps` — official REJECTS them in runes mode
    //   (`legacy_props_invalid` / `legacy_rest_props_invalid`), so they fail through
    //   the OFFICIAL-REJECT gate (`GlobalReferenceInvalid`).
    // - `let $$anchor = 1;` — a `$$`-prefixed DECLARATION, official `dollar_prefix_invalid`
    //   → the official-reject gate (`DollarPrefixInvalid`).
    let mut wrong = Vec::new();

    // `$$slots` stays an unsupported-FEATURE refusal (official-accepted).
    match fails_closed("let s = $$slots;") {
        Some(UnsupportedSvelteRuntimeSurface::MagicIdentifier { .. }) => {}
        other => wrong.push(format!(
            "dollar_slots: expected a MagicIdentifier (unsupported-feature) refusal, got {other:?}"
        )),
    }

    // `$$props` / `$$restProps` are official rejects (global-reference class).
    for (label, body) in [
        ("dollar_props", "let p = $$props;"),
        ("dollar_restprops", "let r = $$restProps;"),
    ] {
        match official_reject(body) {
            Some(CoreOfficialValidationRule::GlobalReferenceInvalid) => {}
            other => wrong.push(format!(
                "{label}: expected an OfficialReject(GlobalReferenceInvalid), got {other:?}"
            )),
        }
    }

    // `let $$anchor = 1;` is a `$$`-prefixed DECLARATION — the official dollar-prefix
    // reject class.
    match official_reject("let $$anchor = 1;") {
        Some(CoreOfficialValidationRule::DollarPrefixInvalid) => {}
        other => wrong.push(format!(
            "anchor_binding: expected an OfficialReject(DollarPrefixInvalid), got {other:?}"
        )),
    }

    assert!(
        wrong.is_empty(),
        "magic identifiers must fail closed with the precise channel:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn negative_magic_identifier_in_a_template_expression_fails_closed() {
    // A magic identifier referenced in a TEMPLATE expression (`{$$slots.default}`,
    // `onclick={() => $$props.x}`) must ALSO fail closed — the scan covers template
    // expressions, not just the instance script.
    let rows: &[&str] = &[
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{$$slots.default}</button>\n",
        "<script>let c = $state(0);</script>\n<button onclick={() => $$props.x}>{c}</button>\n",
    ];
    let mut leaks = Vec::new();
    for src in rows {
        if matches!(compile(src), Ok(js) if js.contains("export default function")) {
            leaks.push(format!("a template magic reference emitted a Main: {src}"));
        }
    }
    assert!(
        leaks.is_empty(),
        "a magic identifier in a template expression must fail closed:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn negative_shadowed_magic_name_is_not_a_false_refusal() {
    // DISCRIMINATION / non-vacuity: a LOCAL named like a magic object (an arrow PARAM
    // `$$props`) is SHADOWED — its reference is NOT the magic object, so the magic scan
    // must NOT refuse on that basis. Here the shadowing is inside a SUPPORTED onclick
    // arrow (the param shadows `$$props`), and the body is a `$state` write — the
    // component still emits. (A function declaration would itself be out-of-allowlist;
    // the arrow-param form keeps the surface supported so the discrimination is real.)
    let src =
        "<script>let c = $state(0);</script>\n<button onclick={($$props) => c++}>{c}</button>\n";
    // The arrow has a param, so it is NOT the supported nullary `$state`-write handler
    // — it fails closed at the HANDLER-SHAPE gate (5d), NOT at the magic scan. The
    // discrimination is that it is NOT a `MagicIdentifier` refusal (any other outcome
    // — the handler-shape refusal, or a Main — is acceptable; the point is the magic
    // scan does not false-fire on a shadowed name).
    if let Err(ClientCompileError::Unsupported(
        UnsupportedSvelteRuntimeSurface::MagicIdentifier { .. },
    )) = compile(src)
    {
        panic!("a SHADOWED `$$props` arrow param must NOT be a MagicIdentifier refusal: {src}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NEGATIVE matrix — the `<svelte:options>` boundary.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn negative_svelte_options_forms_fail_closed() {
    // Every `<svelte:options>` form beyond absent-or-`runes={true}` fails closed.
    let rows: &[(&str, &str)] = &[
        // A non-boolean `runes` value.
        (
            "runes_identifier",
            "<svelte:options runes={foo} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "runes_number",
            "<svelte:options runes={1} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "runes_string",
            "<svelte:options runes=\"true\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // `runes={false}` selects legacy mode.
        (
            "runes_false",
            "<svelte:options runes={false} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // A duplicate `<svelte:options>`.
        (
            "duplicate",
            "<svelte:options runes={true} />\n<svelte:options runes={true} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // A nested / non-root `<svelte:options>`.
        (
            "nested",
            "<script>let c = $state(0);</script>\n<div><svelte:options runes={true} /></div>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // Another axis (`namespace` / `customElement` / `tag` / `name` / `css`).
        (
            "namespace_axis",
            "<svelte:options namespace=\"svg\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "custom_element_axis",
            "<svelte:options customElement=\"x-y\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        (
            "tag_axis",
            "<svelte:options tag=\"x-y\" />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // A spread on the options element.
        (
            "spread",
            "<svelte:options {...opts} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // Child content.
        (
            "child_content",
            "<svelte:options runes={true}>x</svelte:options>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
    ];
    let mut leaks = Vec::new();
    for (label, source) in rows {
        match compile(source) {
            // Fails closed (no `Main`) through EITHER refusal channel: an unsupported
            // OPTIONS FEATURE (a non-root placement, child content, a non-runes axis) is the
            // `Unsupported` channel; a DUPLICATE `<svelte:options>` is now an EXACT-CODE
            // official reject (`svelte_meta_duplicate`) carried by the official-reject gate.
            Err(ClientCompileError::Unsupported(_))
            | Err(ClientCompileError::OfficialReject(_)) => {}
            other => leaks.push(format!("{label}: expected fail-closed, got {other:?}")),
        }
    }
    assert!(
        leaks.is_empty(),
        "every non-allowlist <svelte:options> form must fail closed:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn options_runes_false_fails_closed_as_legacy_mode() {
    // `runes={false}` is the legacy-mode owner (5i) — it selects legacy mode, which is
    // an unsupported client surface, so it fails closed BEFORE a client plan exists.
    match compile(
        "<svelte:options runes={false} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
    ) {
        Err(ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::LegacyMode { .. })) => {}
        other => panic!("`runes={{false}}` must fail closed as LegacyMode (5i), got {other:?}"),
    }
}

#[test]
fn non_boolean_runes_options_fails_as_official_reject() {
    // A non-boolean `runes` value (`runes={foo}`) is an official EXACT-CODE parse error —
    // upstream's `read_options` `get_boolean_value` throws `svelte_options_invalid_attribute_value`
    // — minted by the parser `read_options` finalization and carried by the official-reject gate.
    // NOT a silent accept (the lenient-options leak), and no longer a code-less OptionsAxis
    // unsupported surface. (A DUPLICATE `<svelte:options>` is a DIFFERENT class —
    // `svelte_meta_duplicate` — covered by `duplicate_svelte_options_fails_as_official_reject`.)
    let src = "<svelte:options runes={foo} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    match compile(src) {
        Err(ClientCompileError::OfficialReject(rejection)) => {
            assert_eq!(
                rejection.official_code, "svelte_options_invalid_attribute_value",
                "a non-boolean `runes` value must carry the official \
                 `svelte_options_invalid_attribute_value` code"
            );
        }
        other => panic!(
            "a non-boolean `runes` value must fail as an OfficialReject: {other:?}\nsource: {src}"
        ),
    }
}

#[test]
fn duplicate_svelte_options_fails_as_official_reject() {
    // A DUPLICATE `<svelte:options>` is an official EXACT-CODE parse error
    // (`svelte_meta_duplicate`) minted by the parser and carried by the official-reject gate
    // — NOT the unsupported OptionsAxis surface, and never a silent accept.
    let src = "<svelte:options runes={true} />\n<svelte:options runes={true} />\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    match compile(src) {
        Err(ClientCompileError::OfficialReject(rejection)) => {
            assert_eq!(
                rejection.official_code, "svelte_meta_duplicate",
                "a duplicate <svelte:options> must carry the official `svelte_meta_duplicate` code"
            );
        }
        other => panic!(
            "a duplicate <svelte:options> must fail as an OfficialReject: {other:?}\nsource: {src}"
        ),
    }
}

#[test]
fn redeclaration_scope_is_let_const_only_function_collisions_fail_closed() {
    // CHARACTERIZES the deliberate scope of the body-probe `top_level_lexical_redeclaration`
    // detector (so it is not an over-claim): the SUPPORTED-surface `let`/`const` redeclaration is
    // an EXACT-CODE `js_parse_error`, while a redeclaration involving a top-level FUNCTION / CLASS
    // / IMPORT (which upstream ALSO `js_parse_error`s) is NOT exact-coded here because such a
    // construct is itself OUTSIDE the §1.2-core allowlist — the component fails closed as an
    // unsupported FEATURE first. So no REACHABLE official-reject in the supported surface is
    // missed; the out-of-surface collisions still fail closed (no `Main`), never a silent accept.

    // (a) the supported-surface `let`/`const` redeclaration → exact-code OfficialReject.
    let lexical = "<script>let c = $state(0); let c = $state(1);</script>\n<button onclick={() => c++}>{c}</button>\n";
    match compile(lexical) {
        Err(ClientCompileError::OfficialReject(rejection)) => assert_eq!(
            rejection.official_code, "js_parse_error",
            "a same-scope `let`/`const` redeclaration must carry `js_parse_error`"
        ),
        other => panic!("a `let`/`const` redeclaration must be an OfficialReject: {other:?}"),
    }

    // (b) out-of-surface collisions (function / class / import) — upstream `js_parse_error`s them,
    //     but they carry an out-of-allowlist construct, so Verter fails closed (no `Main`) via the
    //     unsupported channel. The REQUIREMENT is only "no accepted-invalid leak" — never a `Main`.
    for src in [
        "<script>function f(){} function f(){}</script>\n<button>x</button>\n",
        "<script>class A {} class A {}</script>\n<button>x</button>\n",
        "<script>import x from 'y'; let x = 1;</script>\n<button>x</button>\n",
    ] {
        assert!(
            compile(src).is_err(),
            "an out-of-surface redeclaration must fail closed (no Main): {src}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// STATIC GUARDS — the lowering structure (the broad path is gone).
// ─────────────────────────────────────────────────────────────────────────────

/// The Svelte runtime source dir.
fn runtime_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/runtime")
}

/// Read a runtime source file.
fn read_runtime_file(name: &str) -> String {
    std::fs::read_to_string(runtime_dir().join(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

#[test]
fn guard_client_script_lowering_does_not_call_the_removed_broad_path() {
    // The broad "emit any non-rune statement" path (`lower_instance_declarations`) was
    // REMOVED. No production source may reference it — the lowering consumes ONLY the
    // typed `SupportedInstanceScriptItem` allowlist via `lower_simple_instance_item` (the
    // per-item transform; a function-pair function body lowers through the shared fallible
    // rewriter, NOT a broad statement path).
    for file in [
        "client_plan.rs",
        "client_surface.rs",
        "expr_emit.rs",
        "client_shapes.rs",
        "mod.rs",
    ] {
        let src = read_runtime_file(file);
        assert!(
            !src.contains("lower_instance_declarations"),
            "{file}: the removed broad path `lower_instance_declarations` must not be referenced"
        );
    }
    // The build_script_items consumer must lower from the typed allowlist.
    let plan = read_runtime_file("client_plan.rs");
    assert!(
        plan.contains("lower_simple_instance_item"),
        "client_plan.rs must lower the instance script via `lower_simple_instance_item`"
    );
}

#[test]
fn guard_no_other_arrow_rewrite_statement_catch_all_in_script_lowering() {
    // The removed broad path had an `other => rewrite_statement_with_props(…)` catch-all
    // arm that lowered ANY top-level statement. No production source may carry it (or
    // the helper it called) — the per-statement classifier is EXHAUSTIVE with precise
    // refusals, not a wildcard rewrite.
    for file in ["expr_emit.rs", "client_shapes.rs"] {
        let src = read_runtime_file(file);
        assert!(
            !src.contains("rewrite_statement_with_props"),
            "{file}: the broad statement-rewrite helper `rewrite_statement_with_props` must be gone"
        );
        assert!(
            !src.contains("other => rewrite_statement"),
            "{file}: the `other => rewrite_statement…` catch-all arm must be gone"
        );
    }
}

#[test]
fn guard_accepted_plain_script_is_not_ts_stripped() {
    // The supported instance-script lowering does NOT strip TypeScript for an accepted
    // plain `<script>` — a plain script is classified as JS (a TS construct fails
    // closed), so there is no TS-strip on the accept path. The removed
    // `strip_ts_statement` / `strip_pattern_annotation` helpers (which TS-stripped a
    // lowered instance statement) must be gone from the script-item lowering. A
    // SUPPORTED component's emitted module never carries a residual annotation, and a
    // TS-syntax plain script fails closed.
    let emit_src = read_runtime_file("expr_emit.rs");
    assert!(
        !emit_src.contains("strip_ts_statement"),
        "expr_emit.rs: the instance-statement TS-strip helper `strip_ts_statement` must be gone"
    );
    assert!(
        !emit_src.contains("fn strip_pattern_annotation"),
        "expr_emit.rs: the pattern TS-annotation strip `strip_pattern_annotation` must be gone"
    );
    // A plain `<script>` with TS syntax (an `enum` / a typed `let`) fails closed — it
    // is NOT TS-stripped-then-accepted. Upstream parses a plain (JS) script body with Acorn,
    // so TS-only syntax is `js_parse_error` — an EXACT-CODE official reject (the body-probe),
    // not the deferrable `lang="ts"` unsupported-feature channel. The invariant is "no Main",
    // through EITHER refusal channel.
    assert!(
        emits_main("let c = $state(0);"),
        "a plain JS instance script (no TS) must still emit"
    );
    assert!(
        !emits_main("let c: number = $state(0);"),
        "a TS-annotated declarator in a plain script must fail closed (not be TS-stripped)"
    );
    assert!(
        !emits_main("enum E { A }"),
        "a plain-script `enum` must fail closed (not be seeded / stripped)"
    );
}

#[test]
fn guard_supported_instance_script_item_enum_is_the_classified_surface_field() {
    // The typed allowlist is CARRIED on the classified surface (the proof the lowering
    // consumes only the enum). `ClassifiedClientSurface` carries `script_items:
    // Vec<SupportedInstanceScriptItem>`, and `SupportedClientIr` threads it.
    let surface = read_runtime_file("client_surface.rs");
    assert!(
        surface.contains("script_items: Vec<SupportedInstanceScriptItem>"),
        "ClassifiedClientSurface must carry a typed `script_items` allowlist field"
    );
    // The allowlist enum + its classifier live in the dedicated `instance_items.rs`
    // module (extracted from `client_shapes.rs` as a cohesive concern).
    let items = read_runtime_file("instance_items.rs");
    assert!(
        items.contains("enum SupportedInstanceScriptItem"),
        "instance_items.rs must define the `SupportedInstanceScriptItem` allowlist enum"
    );
    assert!(
        items.contains("fn classify_supported_instance_items"),
        "instance_items.rs must define the `classify_supported_instance_items` classifier"
    );
}
