//! Integration tests for the Svelte client (`svelte/internal/client`) emission.
//!
//! These drive the full pipeline (parse → lower → plan → topology → emit) and pin
//! the emitted-JS shape against the official `svelte@5.56.10` output captured via
//! the oracle. Each test is discriminating with negative assertions; the
//! fail-closed family asserts the precise typed surface + diagnostic id (never a
//! silent empty module, never a panic).

use oxc_allocator::Allocator;

use super::{emit_client_module, ClientEmitOptions, ClientModule, ScopedCssArtifact};
use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::client::UnsupportedSvelteRuntimeSurface;
use crate::svelte::runtime::client_plan::ClientModulePlan;
use crate::svelte::runtime::html::StaticTemplatePlan;
use crate::svelte::runtime::topology::ClientTopologyPlan;
use crate::svelte::runtime::{
    compile_client, ClientCompileError, CoreOfficialValidationRule, SvelteRuntimeOptions,
};

/// Compile a Svelte source to its client JS, panicking on a lowering/unsupported
/// error (for the SUPPORTED fixtures).
fn emit(source: &str, filename: &str) -> String {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some(filename.to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false, false)
        .unwrap_or_else(|e| panic!("client emission failed for {filename}: {e:?}"))
        .code
}

/// Compile returning the `Result`, so a fail-closed test can assert the typed
/// surface.
fn emit_result(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false, false).map(|m| m.code)
}

/// [`emit_result`] with an explicit `runes` COMPILE-OPTION override (for the
/// compile-option vs in-source `<svelte:options runes={…}>` precedence tests).
fn emit_result_with_runes(source: &str, runes: Option<bool>) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        runes,
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false, false).map(|m| m.code)
}

/// The §1.2 conformance fixture.
const HELLO_INPUT: &str = "<script>\n\tlet name = $state('world');\n\tlet count = $state(0);\n</script>\n\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n";
/// Whether the emitted module parses as valid JavaScript (no TS syntax, valid
/// hoisting) — the F2/F3 validity gate. Parses with the runtime OXC parser at the
/// `module` grammar (the emitted module uses top-level `import`/`export`).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::default().with_module(true);
    let ret = verter_parser::oxc_parse::Parser::new(&alloc, code, source_type).parse();
    !ret.fatal_error && ret.diagnostics.is_empty()
}

/// Count the DECLARED occurrences of a binding `name` (any scope) in the emitted
/// module via an OXC AST walk over `BindingIdentifier`s. A `bind:group` accumulator
/// that collides with a user binding of the same name would declare the name TWICE
/// (an invalid redeclaration); the collision-aware allocator renames the accumulator
/// so each name is declared at most once. References (`IdentifierReference`) are NOT
/// `BindingIdentifier`s, so a `$.bind_group(name, …)` USE is not counted.
fn count_declared_binding(code: &str, name: &str) -> usize {
    use oxc_ast::ast::BindingIdentifier;
    use oxc_ast_visit::Visit;
    struct Counter<'n> {
        name: &'n str,
        count: usize,
    }
    impl<'a> Visit<'a> for Counter<'_> {
        fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
            if it.name.as_str() == self.name {
                self.count += 1;
            }
        }
    }
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::default().with_module(true);
    let ret = verter_parser::oxc_parse::Parser::new(&alloc, code, source_type).parse();
    let mut counter = Counter { name, count: 0 };
    counter.visit_program(&ret.program);
    counter.count
}

// ── lang="ts" ────────────────────────────────────────────────────────────────
// ── Fail-closed (per surface family, asserting the exact typed surface) ───────

/// Assert that `source` fails closed with an unsupported surface matching
/// `predicate` (the discriminating typed-surface check) and carrying the
/// machine-stable `svelte-runtime-unsupported-` diagnostic id.
fn assert_fail_closed(source: &str, predicate: impl Fn(&UnsupportedSvelteRuntimeSurface) -> bool) {
    assert_fail_closed_labeled("", source, predicate);
}

/// [`assert_fail_closed`] with a CASE LABEL threaded into every panic message — so a
/// table-driven refusal loop names WHICH case leaked when one regresses (e.g. which import
/// form emitted instead of failing closed).
fn assert_fail_closed_labeled(
    label: &str,
    source: &str,
    predicate: impl Fn(&UnsupportedSvelteRuntimeSurface) -> bool,
) {
    let ctx = if label.is_empty() {
        String::new()
    } else {
        format!(" [{label}]")
    };
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            // The discriminating `predicate` pins the EXACT typed surface variant (the
            // machine-stable identity), so the assertion characterizes the refusal arm by
            // its enum shape + diagnostic code, never by a plan/phase label.
            assert!(
                predicate(&surface),
                "wrong fail-closed surface{ctx}: {surface:?} (code {})",
                surface.diagnostic_code()
            );
            // The diagnostic id has the `svelte-runtime-unsupported-` prefix —
            // except the css-analysis surface, which CARRIES the precise
            // official css code (`css_expected_identifier` /
            // `css_global_invalid_placement` / … / `css_render_failed`).
            if matches!(
                surface,
                UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { .. }
            ) {
                assert!(
                    surface.diagnostic_code().starts_with("css_"),
                    "css-analysis diagnostic id shape{ctx}: {}",
                    surface.diagnostic_code()
                );
            } else {
                assert!(
                    surface
                        .diagnostic_code()
                        .starts_with("svelte-runtime-unsupported-"),
                    "diagnostic id shape{ctx}: {}",
                    surface.diagnostic_code()
                );
            }
        }
        Ok(js) => panic!("expected fail-closed{ctx}, got a module:\n{js}"),
        Err(other) => panic!("expected an unsupported-surface error{ctx}, got: {other:?}"),
    }
}

/// Assert `{@debug …}` with a non-identifier argument fails closed at lowering with the
/// `debug_tag_invalid_arguments`-mirroring diagnostic, never an emitted module (which
/// would carry an invalid object key like `a.x: $.snapshot(...)`).
fn assert_debug_invalid_arguments(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Lowering(errs)) => assert!(
            errs.diagnostics
                .iter()
                .any(|d| d.code == "svelte-runtime-debug-invalid-arguments"),
            "expected the debug-invalid-arguments diagnostic, got {:?}",
            errs.diagnostics
        ),
        other => panic!("expected a `{{@debug}}` invalid-arguments refusal, got {other:?}"),
    }
}

/// The behavioral jsdom event-smoke fixture sources (kept in lockstep with the
/// committed `.client.mjs` by the
/// `event_smoke_*_module_matches_the_committed_jsdom_smoke_fixture` tests).
const EVENT_SMOKE_FIXTURES: &[(&str, &str)] = &[
    (
        "event_nondelegated",
        "<script>let focused = $state(false);</script>\n<input onfocus={() => focused = true} />\n<p>{focused}</p>\n",
    ),
    (
        "event_once",
        "<script>let count = $state(0);</script>\n<button on:click|once={() => count++}>btn</button>\n<p>{count}</p>\n",
    ),
    (
        "event_prevent_default",
        "<script>let hits = $state(0);</script>\n<button on:click|preventDefault={() => hits++}>btn</button>\n<p>{hits}</p>\n",
    ),
    (
        "event_stop_propagation",
        "<script>let inner = $state(0);\nlet outer = $state(0);</script>\n<div on:click={() => outer++}><button on:click|stopPropagation={() => inner++}>btn</button></div>\n<p>{inner}-{outer}</p>\n",
    ),
    (
        "event_self",
        "<script>let count = $state(0);</script>\n<div on:click|self={() => count++}><button>child</button></div>\n<p>{count}</p>\n",
    ),
    (
        "event_capture",
        // The BUBBLE handler is registered FIRST and the CAPTURE handler SECOND, so the
        // capture-phase ordering is observable: a correct capture fires `C` before the
        // bubble `B` (→ `CB`), while a DROPPED capture arg would make both bubble-phase
        // and fire in REGISTRATION order (→ `BC`). The smoke asserts `CB`, so it now
        // discriminates a missing 4th `true`.
        "<script>let log = $state('');</script>\n<div on:click={() => log += 'B'} on:click|capture={() => log += 'C'}><button>btn</button></div>\n<p>{log}</p>\n",
    ),
];

/// The behavioral jsdom CONTROL-FLOW-BLOCK smoke fixture sources (kept in lockstep with the
/// committed `.client.mjs` by `block_smoke_modules_match_the_committed_jsdom_fixtures`). Each
/// source ALSO lives in the golden corpus (`svelte_oracle_corpus/fixtures/blocks/`), so the
/// emitted module is independently proven STRUCTURALLY conformant to the pinned official
/// compiler — the smoke adds the BEHAVIORAL (mount-and-react) proof on top.
const BLOCK_SMOKE_FIXTURES: &[(&str, &str)] = &[
    // `{#if}` — the true branch renders its body.
    (
        "block_if_single",
        "<script>\n\tlet show = $state(true);\n</script>\n\n{#if show}\n\t<p>shown</p>\n{/if}\n",
    ),
    // `{#each}` (unkeyed, `$props()`-sourced) — the body is rendered once per item and the
    // item is a SIGNAL (`$.get(row)`), proven by the per-item text reflecting the prop array.
    (
        "block_each_unkeyed",
        "<script>\n\tlet { rows } = $props();\n</script>\n\n{#each rows as row}\n\t<p>{row}</p>\n{/each}\n",
    ),
    // `{#key}` — the keyed block renders its body, and the reactive `count` read INSIDE the
    // block updates on a delegated click (no re-key needed), proving block-interior reactivity.
    (
        "block_key_reactive",
        "<script>\n\tlet selected = $state(0);\n\tlet count = $state(5);\n</script>\n\n<button onclick={() => count++}>inc</button>\n{#key selected}\n\t<p>{count}</p>\n{/key}\n",
    ),
];

/// The behavioral jsdom LIFECYCLE-directive smoke fixture sources (kept in lockstep
/// with the committed `.client.mjs` by
/// `lifecycle_smoke_modules_match_the_committed_jsdom_fixtures`). The lifecycle
/// callables (action / transition / animation fns, the attachment) are PROP-driven so
/// the happy-dom spec passes real recording functions through `mount(App, { props })`
/// — the instance script stays inside the supported allowlist (no top-level
/// `function` declarations).
const LIFECYCLE_SMOKE_FIXTURES: &[(&str, &str)] = &[
    // `use:act` — a prop-backed action: `$.action(div, ($$node) => $$props.act?.($$node))`
    // runs the action once with the mounted element.
    (
        "lifecycle_use",
        "<script>\n\tlet { act } = $props();\n</script>\n\n<div use:act>x</div>\n",
    ),
    // `transition:fx` inside an `{#if}` — toggling the branch on runs the intro through
    // the prop-backed transition fn (FLAG 3, the bidirectional kind).
    (
        "lifecycle_transition",
        "<script>\n\tlet { fx } = $props();\n\tlet show = $state(false);\n</script>\n\n<button onclick={() => show = !show}>t</button>\n{#if show}\n\t<div transition:fx>x</div>\n{/if}\n",
    ),
    // Element-position `{@attach hook}` — the attachment runs on mount with the element.
    (
        "lifecycle_attach",
        "<script>\n\tlet { hook } = $props();\n</script>\n\n<div {@attach hook}>x</div>\n",
    ),
    // `animate:fx` in a KEYED each (the ternary source reorders on a `$state` flip) —
    // the each mounts with the ANIMATED flag and reorders on click.
    (
        "lifecycle_animate",
        "<script>\n\tlet { fx } = $props();\n\tlet flipped = $state(false);\n</script>\n\n<button onclick={() => flipped = !flipped}>swap</button>\n{#each (flipped ? ['b', 'a'] : ['a', 'b']) as item (item)}\n\t<p animate:fx>{item}</p>\n{/each}\n",
    ),
    // `use:act` + legacy `on:click` — the non-delegated event is EFFECT-WRAPPED
    // (`$.effect(() => $.event('click', div, …))`, after `$.action`) and the
    // listener still fires: a click increments the rendered count.
    (
        "lifecycle_use_legacy_event",
        "<script>\n\tlet { act } = $props();\n\tlet c = $state(0);\n</script>\n\n<div use:act on:click={() => c++}>{c}</div>\n",
    ),
];

/// Client-runtime breadth fixtures that exercise the supported families not
/// covered by the focused bind/event/lifecycle suites. Each emitted module is
/// executed by `svelte-client-breadth-smoke.spec.ts` against the pinned runtime.
const CLIENT_BREADTH_SMOKE_FIXTURES: &[(&str, &str)] = &[
    (
        "breadth_spread_html",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_spread_html.svelte"),
    ),
    (
        "breadth_await",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_await.svelte"),
    ),
    (
        "breadth_snippet",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_snippet.svelte"),
    ),
    (
        "breadth_dynamic_child",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_dynamic_child.svelte"),
    ),
    (
        "breadth_dynamic_parent",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_dynamic_parent.svelte"),
    ),
    (
        "breadth_special_head_window",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_special_head_window.svelte"),
    ),
    (
        "breadth_legacy_store",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_legacy_store.svelte"),
    ),
    (
        "breadth_typescript_script",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_typescript_script.svelte"),
    ),
    (
        "breadth_runes_effect",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_runes_effect.svelte"),
    ),
    (
        "breadth_scoped_css",
        include_str!("../../../tests/svelte_oracle_corpus/fixtures/runtime/breadth_scoped_css.svelte"),
    ),
];

// ====================================================================================
// The plain-Svelte-JS function-pair bind lane (mjs + strict official-delta scan,
// no-strip rewrite). The function-pair element acceptance routes through the
// default-CLOSED `parse_plain_svelte_function_pair` helper: each element is parsed as
// plain Svelte JS (`SourceType::mjs()`), the exact two-element sequence shape is
// validated, and a strict official-delta scan refuses the OXC-mjs-over-Acorn residual
// (TS-only class/member fields + decorators + implements/type-params + accessor) that
// official svelte@5.56.10 REJECTS but OXC's plain-JS parse tolerates. Every outcome below
// is oracle-verified against pinned svelte@5.56.10.

/// Assert a function-pair bind source FAILS CLOSED (no module emitted), via the typed
/// `Binding` unsupported-surface — the form reached the bind classifier (it parses
/// cleanly under the upstream tsx expr gate) and the plain-Svelte-JS lane refused it
/// (an `mjs` parse error OR a strict-delta violation). The pre-fix tree silently
/// TS-stripped these and emitted a module, so the `Ok` arm is the RED-before state.
fn assert_function_pair_binding_refused(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Unsupported(surface)) => {
            assert!(
                matches!(&surface, UnsupportedSvelteRuntimeSurface::Binding { target, .. } if target == "value"),
                "expected a `Binding {{ target: \"value\" }}` refusal, got: {surface:?}"
            );
        }
        Ok(js) => panic!("expected fail-closed (official rejects this), got a module:\n{js}"),
        Err(other) => panic!("expected a `Binding` unsupported surface, got: {other:?}"),
    }
}

/// Assert a function-pair bind source FAILS CLOSED via the upstream expr-parse channel —
/// the element is a plain-`.svelte` PARSE ERROR even under OXC's tsx leniency (e.g.
/// `abstract` in an expression-position class), so the template expression fails at the
/// `svelte-runtime-expr-parse` gate BEFORE the bind classifier. Official svelte@5.56.10
/// likewise REJECTS it (`Expected token }` / `Unexpected token`).
fn assert_function_pair_expr_parse_refused(source: &str) {
    match emit_result(source) {
        Err(ClientCompileError::Lowering(errs)) => {
            assert!(
                errs.diagnostics
                    .iter()
                    .any(|d| d.code == "svelte-runtime-expr-parse"),
                "expected a `svelte-runtime-expr-parse` diagnostic, got: {errs:?}"
            );
        }
        Ok(js) => panic!("expected fail-closed (official rejects this), got a module:\n{js}"),
        Err(other) => panic!("expected an expr-parse lowering error, got: {other:?}"),
    }
}

/// Compile to the FULL [`ClientModule`] (code + the external css artifact),
/// under the fixed `App.svelte` filename — the scoped-style tests read both.
fn module_result(source: &str) -> Result<super::ClientModule, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false, false)
}

fn assert_generated_offset_maps_to_exact_source_offset(
    map: &oxc_sourcemap::OwnedSourceMap,
    generated: &str,
    generated_offset: usize,
    expected_source_offset: usize,
) {
    use crate::framework_common::sourcemap_e2e_helpers as helpers;

    let lookup = helpers::build_lookup_table(map);
    let (generated_line, generated_column) =
        helpers::byte_offset_to_line_col(generated, generated_offset);
    let token = map
        .lookup_token(&lookup, generated_line, generated_column)
        .expect("the authored runtime token has a source-map segment");
    assert_eq!(token.get_source_id(), Some(0), "the token is mapped");
    let mapped_offset = helpers::line_col_to_byte_offset(
        map.get_source_content(0)
            .expect("embedded component source"),
        token.get_src_line(),
        token.get_src_col(),
    )
    .expect("the mapped source position is in bounds");
    assert_eq!(mapped_offset, expected_source_offset);
}

/// Compile `source` with a demanded map and return the module plus the decoded
/// map, for the authored-provenance assertions below.
fn compile_with_map(source: &str, filename: &str) -> (String, oxc_sourcemap::OwnedSourceMap) {
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some(filename.to_string()),
        ..Default::default()
    };
    let alloc = Allocator::default();
    let module = compile_client(source, &parsed, &opts, &alloc, false, true)
        .expect("the fixture compiles with a map");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(
        module.source_map.as_deref().expect("demanded JS map"),
    )
    .expect("valid JS map");
    (module.code, map)
}

/// Assert the generated byte offset claims NO authored provenance — the
/// negative half every provenance addition needs, so a producer cannot satisfy
/// an anchor by mapping its synthesized scaffolding too.
fn assert_generated_offset_is_unmapped(
    map: &oxc_sourcemap::OwnedSourceMap,
    code: &str,
    generated_offset: usize,
    what: &str,
) {
    use crate::framework_common::sourcemap_e2e_helpers as helpers;
    let (line, column) = helpers::byte_offset_to_line_col(code, generated_offset);
    let token = map.lookup_token(&helpers::build_lookup_table(map), line, column);
    assert!(
        token.is_none_or(|token| token.get_source_id().is_none()),
        "{what}: synthesized scaffolding must not claim authored provenance"
    );
}

/// Assert that `source` rejects with the EXACT `inspect_trace_invalid_placement`
/// official-reject disposition (rule + official code + diagnostic id) — the
/// reject-parity channel, not a generic unsupported surface.
fn assert_inspect_trace_invalid_placement(source: &str) {
    let err = emit_result(source).expect_err("a misplaced `$inspect.trace()` must reject");
    let ClientCompileError::OfficialReject(rejection) = err else {
        panic!("expected an OfficialReject(InspectTraceInvalidPlacement), got {err:?}");
    };
    assert_eq!(
        rejection.rule,
        CoreOfficialValidationRule::InspectTraceInvalidPlacement,
        "wrong official-reject rule: {rejection:?}"
    );
    assert_eq!(
        rejection.official_code, "inspect_trace_invalid_placement",
        "wrong official code: {rejection:?}"
    );
    assert_eq!(
        rejection.rule.diagnostic_code(),
        "svelte-official-reject-inspect-trace-invalid-placement"
    );
}

/// Assert a committed jsdom-smoke `.mjs` fixture stays equivalent (modulo cosmetics)
/// to Verter's emitted module for `source`, so the happy-dom behavioral smoke can
/// never drift from `compile_client`.
fn assert_jsdom_fixture_in_sync(source: &str, fixture_name: &str) {
    let js = emit(source, "App.svelte");
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/svelte-runtime-tests/test/fixtures/svelte")
        .join(fixture_name);
    let committed = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("read smoke fixture {}: {e}", fixture_path.display()));
    assert_eq!(
        normalize_js_cosmetics(&js),
        normalize_js_cosmetics(&committed),
        "the emitted module diverged STRUCTURALLY from the committed jsdom-smoke fixture \
         {fixture_name}; regenerate it from `compile_client` and re-run oxfmt"
    );
}

/// Normalize a JS module to its cosmetic-insensitive form for the smoke-fixture
/// equivalence check. LITERAL-AWARE: collapses cosmetic whitespace OUTSIDE
/// string/template literals (so a tabs-vs-spaces / line-wrap reflow does not
/// false-fail) but PRESERVES whitespace INSIDE string + template-literal TEXT (so
/// the significant `Hello ${...}!` template space, or any meaningful text
/// whitespace, still discriminates). The string-literal DELIMITER is unified to
/// `"` (so `'world'` ≡ `"world"`, the oxfmt single→double cosmetic) WITHOUT
/// touching literal content. A trailing comma before a closing `)` / `]` / `}` is
/// dropped (the oxfmt line-wrap cosmetic). Any token / structure / literal-content
/// change still fails.
fn normalize_js_cosmetics(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut tmpl: Vec<i32> = Vec::new();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    while i < n {
        let in_tmpl_text = tmpl.last().copied() == Some(0);
        if in_tmpl_text {
            let ch = chars[i];
            if ch == '\\' {
                out.push(ch);
                if i + 1 < n {
                    out.push(chars[i + 1]);
                }
                i += 2;
                continue;
            }
            if ch == '`' {
                tmpl.pop();
                out.push('`');
                i += 1;
                continue;
            }
            if ch == '$' && i + 1 < n && chars[i + 1] == '{' {
                *tmpl.last_mut().unwrap() = 1;
                out.push_str("${");
                i += 2;
                continue;
            }
            out.push(ch); // template TEXT — preserved verbatim.
            i += 1;
            continue;
        }
        let ch = chars[i];
        // String literal: unify the DELIMITER to `"`, preserve interior verbatim.
        if ch == '\'' || ch == '"' {
            let quote = ch;
            out.push('"');
            i += 1;
            while i < n && chars[i] != quote {
                if chars[i] == '\\' {
                    out.push(chars[i]);
                    if i + 1 < n {
                        out.push(chars[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                out.push(chars[i]);
                i += 1;
            }
            if i < n {
                out.push('"');
                i += 1;
            }
            continue;
        }
        if ch == '`' {
            tmpl.push(0);
            out.push('`');
            i += 1;
            continue;
        }
        if let Some(depth) = tmpl.last_mut() {
            if *depth > 0 {
                if ch == '{' {
                    *depth += 1;
                    out.push('{');
                    i += 1;
                    continue;
                }
                if ch == '}' {
                    *depth -= 1;
                    out.push('}');
                    i += 1;
                    continue;
                }
            }
        }
        // Whitespace OUTSIDE a literal is dropped entirely (the smoke check is
        // token-adjacency-insensitive — official and Verter differ in line breaks /
        // indentation but the token stream + literal content must match).
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    let collapsed = out.replace(",)", ")").replace(",]", "]").replace(",}", "}");
    // oxfmt wraps a single-EXPRESSION arrow body that is an assignment in parentheses
    // (`() => (x = y)`); the emitter (and official svelte) emit the bare `() => x = y`.
    // The parens are cosmetic, so strip a paren group that WRAPS an arrow body
    // (`=>(EXPR)` → `=>EXPR`) before comparing — this keeps the fixture oxfmt-clean
    // AND lockstep-matching without forcing the emitter to parenthesize.
    strip_redundant_arrow_parens(&collapsed)
}

/// Strip a single redundant paren group that WRAPS an arrow-function body: every
/// `=>(…)` whose `(` is the immediate arrow body and whose matching `)` ends the body
/// (the next char is a statement/expression terminator `;` `}` `)` `]` `,` or EOF) is
/// reduced to `=>…`. Operates on the already-normalized (whitespace-stripped) token
/// stream, so the `(` after `=>` is the body opener. A NON-wrapping paren (a call
/// `f()`, a grouped sub-expression that is not the whole body) is left intact.
fn strip_redundant_arrow_parens(s: &str) -> String {
    let bytes: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // Detect `=>(` at this position.
        if i + 2 < bytes.len() && bytes[i] == '=' && bytes[i + 1] == '>' && bytes[i + 2] == '(' {
            // Find the matching `)` for the `(` at i+2 (paren-balanced).
            let mut depth = 0;
            let mut j = i + 2;
            let mut close = None;
            while j < bytes.len() {
                match bytes[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if let Some(c) = close {
                // The paren wraps the whole arrow body iff the char after `)` is a body
                // terminator (or EOF). A `(` that is part of a larger expression
                // (`=>(a)+b`) is NOT a wrapping body paren.
                let after = bytes.get(c + 1).copied();
                let is_body_wrap = matches!(after, None | Some(';' | '}' | ')' | ']' | ','));
                if is_body_wrap {
                    out.push_str("=>");
                    out.extend(&bytes[(i + 3)..c]); // the inner body, sans parens
                    i = c + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

// ── dynamic attributes + boolean DOM props + class/style ─────────────
//
// Every form is pinned BYTE-FAITHFULLY (modulo cosmetics) to svelte@5.56.10 client
// output via the live-compiler probe. Each test is discriminating with a negative
// assertion (the misform that must be ABSENT). The cross-cut: a reactive dynamic
// attr/class/style joins the SAME combined `$.template_effect` as reactive text, in
// source/DOM-walk order; a NON-reactive one is a plain init statement.
//
// The event handlers are the supported inline-arrow `$state`-write shape
// (`onclick={() => v = !v}`) — a named-function handler is the event-wrapper surface.

/// Normalize an EXPECTED emitted-JS substring the same way [`normalize_js_cosmetics`]
/// normalizes the emitter output (whitespace stripped, JS string-literal quotes
/// unified to `"`), so an attribute assertion is written in NATURAL spaced single-quote form
/// and compared against the equally-normalized emitter output.
fn nc(expected: &str) -> String {
    normalize_js_cosmetics(expected)
}

// ─── Value-position emission is source-preserving (author parens kept) ───
// The value/property printer keeps the author's parens verbatim. The one BEHAVIORAL
// value-position transform is the sequence wrap: a top-level `SequenceExpression` is wrapped
// in one paren pair so it stays a single value (`{@html a, b}` -> `() => (a, b)`) rather than
// splitting `b` into a positional argument. Redundant author parens around a non-sequence
// value are a behavior-preserving cosmetic difference the minifier collapses, so the
// behavioral tests below are paren-COUNT-insensitive on the value bytes.

/// Collapse runs of ASCII whitespace to a single space WITHOUT touching parens — for the
/// `{@html}` arrow-body tests, where the sequence-wrap `=>(…)` distinction is exactly what
/// `normalize_js_cosmetics` deliberately erases (`strip_redundant_arrow_parens`). A
/// paren-preserving collapse is the discriminating comparator for the sequence-wrap.
fn collapse_ws_keep_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

/// Run the REAL emit path (`ClientEmitter::emit`) for `source` with the emitter's
/// after-update rank map emptied AFTER construction — the synthetic "unranked op"
/// drive of the actual `client_emit.rs` after-update batch. The two call-site
/// fail-loud tests below use this: they discriminate the CALL SITES
/// (`after_update_pre_rank` / `after_update_post_rank`), not just the helper —
/// reverting either call-site body to a silent `.unwrap_or(u32::MAX)` fallback
/// (while `require_after_update_rank` stays intact) makes the emit SUCCEED with a
/// silent tail sort, and the `#[should_panic]` tests FAIL.
fn emit_with_after_update_ranks_cleared(source: &str) -> String {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let ir = crate::svelte::runtime::lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .expect("lowering succeeds for the ranked-op fixture");
    let classified = super::super::client_surface::ClientSyntaxSurface::classify(&ir)
        .expect("classification succeeds for the ranked-op fixture");
    let plan = super::super::client_plan::SupportedClientIr::build(&classified, &ir, None)
        .expect("plan build succeeds for the ranked-op fixture");
    let html_plan = crate::svelte::runtime::plan_static_templates(&ir, None);
    let topology = crate::svelte::runtime::plan_client_topology(&ir, &html_plan, None);
    let mut emitter = super::ClientEmitter::new(&plan);
    emitter.after_update_rank.clear();
    emitter
        .emit(
            source,
            &html_plan,
            &topology,
            opts.filename.as_deref(),
            false,
            &alloc,
        )
        .expect("a valid emitter state produces a module")
        .code
}

// ── The UNIFIED authored-value preparation (`prepare_template_value`) — the ──
// remaining `build_expression` consumers: class/style bases, `style:` inner
// values, attribute-effect folds (+ `<svelte:element>`), block heads, `{@html}`,
// `{@const}`, `{@attach}`, `<title>` chunks. Every oracle line below is a
// direct pinned `svelte@5.56.10` compile of the same fixture.

const LEGACY_OBJ: &str = "<script>export let obj;</script>\n";

/// The legacy deep-read/untrack wrap over a member-of-`obj` payload.
fn obj_wrap(payload: &str) -> String {
    format!("$.deep_read_state(obj()), $.untrack(() => {payload})")
}

/// The `$.each(anchor, <flags>, …)` flags argument of the module's single
/// `each` call.
fn each_flags(js: &str) -> u32 {
    let call = js
        .split_once("$.each(")
        .unwrap_or_else(|| panic!("the module makes no `$.each` call:\n{js}"))
        .1;
    assert!(
        !call.contains("$.each("),
        "the module makes more than one `$.each` call, so this reading is ambiguous:\n{js}"
    );
    let (_anchor, rest) = call
        .split_once(", ")
        .unwrap_or_else(|| panic!("the `$.each` call has no second argument:\n{js}"));
    let flags = rest
        .split_once(',')
        .unwrap_or_else(|| panic!("the `$.each` flags argument is unterminated:\n{js}"))
        .0;
    flags.trim().parse::<u32>().unwrap_or_else(|_| {
        panic!("the `$.each` flags argument `{flags}` is not an integer:\n{js}")
    })
}

/// A single-name each-item destructure whose LOCAL NAME is not a valid
/// property-key read (a rename, an array element, a rest) must fail closed
/// with a real source span — never silently emit a read by the wrong key.
/// Only the plain object-shorthand shape (`{ id }`, pinned above) is
/// supported.
fn assert_each_item_destructure_shape_refuses(source: &str) {
    let err = emit_result(source).expect_err("an unsupported each-item destructure must refuse");
    let ClientCompileError::Unsupported(surface) = &err else {
        panic!("expected a typed unsupported refusal, got {err:?}");
    };
    let UnsupportedSvelteRuntimeSurface::Block { construct, span } = surface else {
        panic!("expected a Block refusal, got {surface:?}");
    };
    assert_eq!(*construct, "destructuring-binding");
    assert_ne!(
        (span.start, span.end),
        (0, 0),
        "the refusal must carry the REAL source span, not a placeholder:\n{source}"
    );
}

// ---------------------------------------------------------------------------
// cssHash scope-class override (the resolved `cssHash` callback result threaded
// into the SINGLE style-plan construction point). The override REPLACES the
// default `svelte-<hash>` scope class byte-exact in BOTH the serialized HTML
// skeleton (baked static `class`) and the external CSS artifact; absent, the
// default djb2 derivation is unchanged.
// ---------------------------------------------------------------------------

/// Compile a styled component with an explicit resolved `css_hash_override`,
/// returning the full client module (JS body + external css artifact).
fn emit_module_with_css_override(
    source: &str,
    filename: &str,
    css_hash_override: Option<&str>,
) -> crate::svelte::runtime::client::ClientModule {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some(filename.to_string()),
        css_hash_override: css_hash_override.map(str::to_string),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false, false)
        .unwrap_or_else(|e| panic!("client emission failed for {filename}: {e:?}"))
}

const CSS_OVERRIDE_INPUT: &str = "<div class=\"card\">x</div>\n<style>.card{color:blue}</style>\n";

#[path = "client_tests/bindings.rs"]
mod bindings;
#[path = "client_tests/components.rs"]
mod components;
#[path = "client_tests/control_flow.rs"]
mod control_flow;
#[path = "client_tests/elements.rs"]
mod elements;
#[path = "client_tests/events.rs"]
mod events;
#[path = "client_tests/general.rs"]
mod general;
#[path = "client_tests/reactivity.rs"]
mod reactivity;
#[path = "client_tests/sourcemaps.rs"]
mod sourcemaps;
#[path = "client_tests/styles.rs"]
mod styles;
#[path = "client_tests/validation.rs"]
mod validation;
