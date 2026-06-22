//! Emitted-JS normalized-topology gate for the native Svelte client backend.
//!
//! For each supported fixture, this compiles the component with Verter
//! (`compile_client`), normalizes the EMITTED JS into the SAME topology shape the
//! `scripts/svelte-golden-lib.mjs` extractors derive (the helper sequence/set/
//! counts, the import topology, the export-fn shape, the `from_html` template
//! skeletons + fragment flag, and the delegated event set), and compares it to the
//! COMMITTED official golden JSON (regenerated from the pinned `svelte@5.56.3` by
//! `scripts/gen-svelte-goldens.mjs`). It is BEHAVIOR/topology parity, NOT byte
//! identity — variable names, whitespace, and walk-strategy details are not pinned.
//!
//! Hermetic: the only inputs are the vendored fixtures + the committed goldens, so
//! the gate runs with no live `svelte` present. The golden is the oracle; a
//! Verter emitted-topology drift fails here.

use std::path::PathBuf;

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{
    compile_client, live_fallback_ledger, ClientCompileError, SvelteRuntimeOptions,
    UnsupportedSvelteRuntimeSurface,
};

/// The SUPPORTED fixtures (the §1.2 headline conformance target + the rune /
/// template surface) this gate covers. Every slug here resolves to a committed
/// `<slug>.client.json` golden, and Verter EMITS a module for it (it does not fail
/// closed). The latter group are the F1–F11 robustness fixtures whose FULL-MODULE
/// comparison catches argument/offset/identifier drift the helper-name sequence
/// misses.
const SUPPORTED_FIXTURES: &[&str] = &[
    // The §1.2 headline conformance target.
    "runes/hello_input",
    // A template-literal RHS in a `$state`-assign onclick body
    // (`onclick={() => { msg = `v${n}`; }}`).
    "runes/template_literal_handler",
    // An aliased no-default `$props()` destructure read in a bare interpolation
    // (`let { foo: bar } = $props(); {bar}`).
    "runes/props_alias",
    // A primitive `$state` reassigned to an object literal in an onclick body
    // (`onclick={() => o = { a: 1 }}`) — the `should_proxy(rhs)` gating.
    "runes/proxy_gating",
    // A pure single interpolation as an element child (`<p>{count}</p>`) — the
    // `is_text` clone flag (`$.child(p, true)`).
    "runes/is_text_flag",
    // `bind:value` + `bind:this` on sibling elements with a sibling reactive text.
    // The op-order oracle: `$.bind_this` is a RENDER-side binding emitted INLINE
    // during the walk (BEFORE the grouped sibling `$.template_effect`), while
    // `$.bind_value` is emitted post-walk (AFTER the text effect).
    "runes/bind_value_and_this",
    // A STATIC no-dynamic multi-root fragment (`<p>a</p><p>b</p>`) — the official
    // text-first `$.next()` cursor advance between the clone frame and `$.append`
    // (`var fragment = root(); $.next(); $.append(...)`). Catches a regression that
    // drops the trailing-static-run cursor advance (a hydration end-node divergence).
    "runes/static_fragment",
    // A STATIC single-element root (`<p>a</p>`) — the `is_single_element` clone-root
    // path: official clones the element directly (`var p = root(); $.append(...)`)
    // with NO `$.next()`. The negative golden for the fragment `$.next()` advance —
    // a single-element root must NOT emit it.
    "runes/static_single_root",
];

/// The SUPPORTED MATRIX — the exhaustive enumeration of supported client
/// sub-shapes, the positive half of the convergence gate. Each row is a minimal
/// component exercising ONE supported sub-shape; the gate (shared with
/// [`SUPPORTED_FIXTURES`] above) asserts each compiles, OXC-parses, and its
/// normalized FULL MODULE equals the official golden. A row fails if the topology
/// drifts — so the matrix discriminates every supported sub-shape, not just the
/// headline §1.2 example. (Adding a row is trivial: drop a `matrix/<name>.svelte`
/// fixture, regenerate the golden, append the slug.)
const SUPPORTED_MATRIX: &[&str] = &[
    // $state primitive-literal declarator + the §1.2-class onclick write forms
    // (assign / compound / postfix-update / prefix-update to the signal).
    "matrix/state_signal_assign",
    "matrix/state_signal_compound",
    "matrix/state_update_postfix",
    "matrix/state_update_prefix",
    // $props() no-default read-only / string-key alias (bare-interpolation reads).
    "matrix/props_readonly",
    "matrix/props_alias_string_key",
    // bind:value on an <input> to a reactive $state identifier.
    "matrix/bind_value_signal_ident",
    "matrix/bind_value_plain_ident",
    // a delegated onclick arrow with a $state-write body.
    "matrix/event_arrow",
    // reactive text (single / multi / mixed) — bare signal reads, simple-ASCII chunks.
    "matrix/text_single_read",
    "matrix/text_multi_read",
    "matrix/text_mixed",
    // static template (single root / fragment / serialized attrs).
    "matrix/static_single_root",
    "matrix/static_fragment",
    "matrix/static_attrs_serialized",
    // a ROOT-level leading static TEXT before the first named dynamic position
    // (`x<button onclick={…}>{c}</button>`) — the official `is_text_first` PRE-CLONE
    // `$.next();` emitted BEFORE `var fragment = root();` (codegen bug A). The negative
    // is the in-element leading text (the §1.2 `<button>clicks: {count}</button>`),
    // which must NOT emit a pre-clone `$.next()` — covered by `runes/hello_input`.
    "matrix/root_leading_text",
];

/// The Block-5a ATTRIBUTE corpus — the `attributes/*` fixtures exercising the
/// dynamic-attribute / boolean-DOM-property / `class:`-`style:` directive / autofocus
/// surface. Each row runs through the IDENTICAL compile + OXC-parse + BYTE-PRECISE
/// full-module comparison gate as the matrix above: the committed `attributes/<slug>.
/// client.json` is the official oracle (regenerated from the pinned `svelte@5.56.3`),
/// and Verter's normalized emitted module must equal `clientModule` exactly. This is
/// the argument/offset/identifier-precise oracle for the attribute surface — the
/// substring/helper-name checks the corpus shipped with could not see a
/// `set_attribute` arg drift, a dropped literal chunk in a mixed property, the
/// single-vs-fragment clone-root choice, or a JS-escaping divergence.
const SUPPORTED_ATTRIBUTES: &[&str] = &[
    // A static-only attribute set baked into the `from_html` template (`id="x"
    // disabled class="base" type="submit"`) — no per-attribute dynamic op.
    "attributes/static_baked",
    // A single dynamic string attribute (`id={id}`) — `$.set_attribute` inside a
    // `template_effect`.
    "attributes/dynamic_attr",
    // Boolean DOM properties (`disabled={d}` / `readonly={r}`) — the `el.<prop> =`
    // property write, NOT `set_attribute`.
    "attributes/boolean_property",
    // The `contenteditable` / `hidden` set-attribute-vs-property split.
    "attributes/set_attribute_props",
    // `muted` on a `<video>` — the media-only DOM property, AND the lone-`<video>`
    // single clone-root (flag `2`, NOT a fragment) — the [P0] crash fixture.
    "attributes/muted_video",
    // `class={expr}` dynamic class — `$.set_class` / class accumulator.
    "attributes/class_expression",
    // `class:foo={on}` directives, with and without a base `class=`.
    "attributes/class_directives",
    // `style={expr}` dynamic style.
    "attributes/style_expression",
    // `style:color={c}` / `style:--x={x}` / `style:color|important={c}` directives.
    "attributes/style_directives",
    // `autofocus` (static + dynamic) — the `$.autofocus` init.
    "attributes/autofocus",
    // A single element carrying `id` + `class` + `style` dynamic attrs in one
    // `template_effect` (the combined-effect coalescing order).
    "attributes/combined_effect",
    // A `$props()`-backed dynamic attr (`id={who}`) — props reads are REACTIVE, so
    // the `$.set_attribute` joins the `$.template_effect` (not a one-shot init).
    "attributes/props_dynamic_attr",
    // A MIXED property value (`<video muted="pre-{v}-post">`) — the full literal+expr
    // template literal `video.muted = \`pre-${$.get(v) ?? ''}-post\``, never dropping
    // the literal chunks.
    "attributes/mixed_property",
    // `muted` on a NON-media element (`<div muted={v}>`) — `muted` is a DOM property
    // on ANY element (`is_dom_property` is element-agnostic), so `div.muted = $.get(v)`.
    "attributes/muted_on_div",
    // A call-expression attr (`id={String(v)}`) — the official memoized deps-array
    // `$.template_effect(($0) => $.set_attribute(el, 'id', $0), [() => String($.get(v))])`.
    "attributes/call_expr_attr",
    // JS-escaping edges — a static `class`/`style` base with an HTML ENTITY (decoded
    // for the JS-string / template chunk) and a `$` adjacent to an interpolation.
    "attributes/escaping_edges",
    // A call-expression CLASS base (`class={String(c)}`) — the base `$.clsx(...)` arg
    // is MEMOIZED into the deps-array form `[() => $.clsx(String($.get(c)))]`.
    "attributes/class_call_expr",
    // A call-expression STYLE directive (`style:width={String(c)}`) — the directives
    // object arg is memoized, parenthesized as `() => ({ width: String($.get(c)) })`.
    "attributes/style_directive_call",
    // ── Escaping edges: the JS-string + template-literal serializer surface ──
    // A static `class` BASE carrying a literal NEWLINE (`class="a\nb"`) consumed as a
    // single-quoted runtime string — the official esrap `quote` escapes `\n` → `\n`
    // (a raw newline inside `'…'` is invalid JS). Discriminates the `js_single_quoted`
    // newline escape.
    "attributes/static_class_newline",
    // A static `class` BASE carrying CR / TAB / BACKSLASH / QUOTE — the esrap-exact
    // escape set: `\r`→`\r`, `\`→`\\`, `'`→`\'`, and a TAB passes through VERBATIM
    // (a raw tab is valid inside `'…'`, and the official serializer leaves it).
    "attributes/static_string_escapes",
    // A REACTIVE mixed attribute whose decoded literal text contains `${` (`id="a${b{v}"`,
    // the `${` an entity-decoded literal) — emitted as a template literal, so the
    // official `sanitize_template_string` escapes `${` → `\${`. Discriminates the
    // `escape_template_text` `${` escape (an unescaped `${` is invalid/misparsed JS).
    "attributes/mixed_template_dollar",
    // ── Memoization granularity: per-expression-part, not whole-template ──
    // A REACTIVE mixed `class` base with a CALL expression (`class="a{String(c)}b"`) —
    // official memoizes the EXPRESSION PART (`` `a${$0 ?? ''}b` ``, dep `() => String(c)`),
    // NOT the whole rendered template. Discriminates the structured class base.
    "attributes/mixed_class_call",
    // The same per-part memoize for a mixed `style` base with a call.
    "attributes/mixed_style_call",
    // TWO reactive directives (a `class:` + a `style:`) on one node — the combined
    // BLOCK-body `$.template_effect(() => { classes = …; styles = …; })` with both
    // accumulators. Discriminates the multi-write memoized form + the normalizer's
    // bracket-hugging-whitespace symmetry.
    "attributes/reactive_multi_directive",
    // ── `bind:this` + init-domain ordering ──
    // `bind:this` sharing a node with `autofocus` + a `class:` directive — official
    // emits the init-domain writes (`$.autofocus`, `$.set_class`) BEFORE `$.bind_this`.
    "attributes/bind_this_init_order",
    // `bind:this` sharing a node with a one-shot dynamic attr (`id={who}`, `who` a
    // demoted `$state`) — `$.set_attribute` (init) BEFORE `$.bind_this`.
    "attributes/bind_this_dynamic_attr",
    // ── `has_call` reactive trigger (independent of `has_state`) ──
    // A DEMOTED `$state` call-expr property (`readonly={Boolean(v)}`, `v` never
    // written → `let v`) — official STILL memoizes into `$.template_effect(($0) =>
    // input.readOnly = $0, [() => Boolean(v)])` because the value `has_call`. The
    // official rule is `has_state || has_call`, NOT a rune-binding check.
    "attributes/call_expr_property_demoted",
    // Its REACTIVE counterpart (`v` written → `$.state`) — the same memoized shape
    // with a `$.get(v)` read; proves the demoted/reactive pair both byte-match.
    "attributes/call_expr_property_reactive",
    // An OPTIONAL-CHAIN method call in an attr (`readonly={v?.startsWith?.('x')}`) +
    // in a `class:` directive (`class:active={w?.endsWith?.('y')}`). Both `v`/`w` are
    // DEMOTED `$state` (read-only → plain `let`), so there is NO `has_state` reactive
    // read — the SOLE reason official memoizes both into the deps array (`[() =>
    // v?.startsWith?.('x'), () => ({ active: w?.endsWith?.('y') })]`) is `has_call`
    // (the optional method calls' callees root at a declared binding → `is_pure ===
    // false`). In OXC an optional call is an `Expression::ChainExpression` wrapping
    // `ChainElement::CallExpression`; discriminates the `has_call` scan's chain-
    // wrapped-call detection (a plain optional MEMBER like `c?.x` is NOT a call and
    // must not memoize).
    "attributes/optional_chain_call",
    // ── `has_call` is PER-CALL in SOURCE order (the `deps > 0` half) ──
    // A PURE optional call BEFORE its first dependency in source order
    // (`readonly={(globalThis?.check?.() ?? false) || flag}`, `flag` a demoted
    // `$state` → plain `let`, so there is NO `has_state` and `has_call` is the SOLE
    // memoize lever). Official's `dependencies` set accumulates AS the expression is
    // walked and the call's `has_call` check runs against the deps-SO-FAR: at the pure
    // call, zero deps have accumulated (the `flag` dependency is observed LATER), and
    // the callee roots at a global → NOT `has_call`. So official emits an INLINE
    // one-shot `input.readOnly = (globalThis?.check?.() ?? false) || flag`, NOT the
    // memoized deps-array form. Discriminates the per-call source-order rule: a
    // whole-expression "references any binding" precompute over-memoizes this into a
    // `$.template_effect`. (A boolean-property attr, not `class=`, so the clsx-wrap
    // path is not involved — the source-order memoize decision is isolated.)
    "attributes/pure_call_before_dep",
    // Its mirror — the SAME parts with the dependency FIRST
    // (`readonly={flag || (globalThis?.check?.() ?? false)}`). At the pure call, `flag`
    // has already accumulated (deps > 0) → `has_call` → official MEMOIZES into
    // `$.template_effect(($0) => input.readOnly = $0, [() => flag || (globalThis?.check?.() ?? false)])`.
    // The positive half of the source-order discrimination pair.
    "attributes/dep_before_pure_call",
];

/// The repository root (two levels up from this crate's `tests/` dir).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The vendored fixture source for a slug.
fn fixture_source(slug: &str) -> String {
    let path = repo_root()
        .join("crates/verter_compiler/tests/svelte_oracle_corpus/fixtures")
        .join(format!("{slug}.svelte"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {slug}: {e}"))
}

/// The committed official client golden JSON for a slug.
fn client_golden(slug: &str) -> serde_json::Value {
    let path = repo_root()
        .join("crates/verter_compiler/tests/svelte_oracle_corpus/goldens")
        .join(format!("{slug}.client.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read golden {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse golden {slug}: {e}"))
}

/// The `componentNameFor` slug → component-name rule the golden generator uses
/// (so Verter compiles under the same `name`).
fn component_name_for(slug: &str) -> String {
    let stem = slug.rsplit('/').next().unwrap_or(slug);
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

/// Compile a fixture to its emitted client JS.
fn emit(slug: &str) -> String {
    let source = fixture_source(slug);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = SvelteRuntimeOptions {
        filename: Some(format!("{slug}.svelte")),
        name: Some(component_name_for(slug)),
        ..Default::default()
    };
    compile_client(&source, &parsed, &opts, &alloc, false)
        .unwrap_or_else(|e| panic!("client emission failed for {slug}: {e:?}"))
        .code
}

// ── Topology extraction (a faithful Rust port of the svelte-golden-lib concepts) ──

/// Mask the non-code regions of a JS module — string literals, template-literal
/// TEXT spans, and line/block comments — to spaces, so a `$.<helper>` scan keys on
/// real code only (a helper-shaped token inside a string/template cannot
/// false-match). Mirrors `maskNonCodeRegions` (template `${...}` interpolations
/// are kept as code).
fn mask_non_code(code: &str) -> String {
    let bytes: Vec<char> = code.chars().collect();
    let n = bytes.len();
    let mut out = bytes.clone();
    // Template-literal nesting: each frame tracks the `${}` interpolation depth.
    let mut tmpl: Vec<i32> = Vec::new();
    let mut i = 0;
    let mask = |out: &mut Vec<char>, idx: usize| {
        if out[idx] != '\n' && out[idx] != '\r' {
            out[idx] = ' ';
        }
    };
    while i < n {
        let in_tmpl_text = tmpl.last().copied() == Some(0);
        if in_tmpl_text {
            let ch = bytes[i];
            if ch == '\\' {
                mask(&mut out, i);
                if i + 1 < n {
                    mask(&mut out, i + 1);
                }
                i += 2;
                continue;
            }
            if ch == '`' {
                tmpl.pop();
                i += 1;
                continue;
            }
            if ch == '$' && i + 1 < n && bytes[i + 1] == '{' {
                *tmpl.last_mut().unwrap() = 1;
                i += 2;
                continue;
            }
            mask(&mut out, i);
            i += 1;
            continue;
        }
        let ch = bytes[i];
        let next = if i + 1 < n { bytes[i + 1] } else { '\0' };
        if ch == '/' && next == '/' {
            while i < n && bytes[i] != '\n' {
                mask(&mut out, i);
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == '*' {
            mask(&mut out, i);
            mask(&mut out, i + 1);
            i += 2;
            while i < n && !(bytes[i] == '*' && i + 1 < n && bytes[i + 1] == '/') {
                mask(&mut out, i);
                i += 1;
            }
            if i < n {
                mask(&mut out, i);
                mask(&mut out, i + 1);
                i += 2;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            i += 1;
            while i < n && bytes[i] != quote {
                if bytes[i] == '\\' {
                    mask(&mut out, i);
                    if i + 1 < n {
                        mask(&mut out, i + 1);
                    }
                    i += 2;
                    continue;
                }
                mask(&mut out, i);
                i += 1;
            }
            if i < n {
                i += 1;
            }
            continue;
        }
        if ch == '`' {
            tmpl.push(0);
            i += 1;
            continue;
        }
        if let Some(depth) = tmpl.last_mut() {
            if *depth > 0 {
                if ch == '{' {
                    *depth += 1;
                    i += 1;
                    continue;
                }
                if ch == '}' {
                    *depth -= 1;
                    i += 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// The ORDERED `$.<helper>` reference sequence over the code-only view.
fn helper_sequence(code: &str) -> Vec<String> {
    let masked = mask_non_code(code);
    let mut seq = Vec::new();
    let chars: Vec<char> = masked.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '$' && chars[i + 1] == '.' {
            let mut j = i + 2;
            let mut name = String::new();
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                name.push(chars[j]);
                j += 1;
            }
            if !name.is_empty() {
                seq.push(name);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    seq
}

/// The committed golden's `helperSequence` as a `Vec<String>`.
fn golden_sequence(golden: &serde_json::Value) -> Vec<String> {
    golden["helperSequence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// The golden's `delegatedEvents`.
fn golden_delegated(golden: &serde_json::Value) -> Vec<String> {
    golden["delegatedEvents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// The emitted module's delegated event set (the `$.delegate([...])` literals).
fn emitted_delegated(code: &str) -> Vec<String> {
    let Some(start) = code.find("$.delegate([") else {
        return Vec::new();
    };
    let body = &code[start + "$.delegate([".len()..];
    let Some(end) = body.find(']') else {
        return Vec::new();
    };
    body[..end]
        .split(',')
        .filter_map(|s| {
            let t = s.trim().trim_matches(|c| c == '\'' || c == '"');
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect()
}

/// The emitted `from_html` template literals + fragment flags, as `(html, flag)`.
fn emitted_templates(code: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut search = code;
    while let Some(idx) = search.find("$.from_html(`") {
        let after = &search[idx + "$.from_html(`".len()..];
        let Some(close) = after.find('`') else { break };
        let html = after[..close].to_string();
        // The trailing flag (if any): between the closing backtick and the `)`.
        let rest = &after[close + 1..];
        let flag = rest
            .strip_prefix(", ")
            .and_then(|r| r.split(')').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with(')'));
        out.push((html, flag));
        search = rest;
    }
    out
}

/// The golden's `templates` as `(html, flag)`.
fn golden_templates(golden: &serde_json::Value) -> Vec<(String, Option<String>)> {
    golden["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["html"].as_str().unwrap().to_string(),
                t["flag"].as_str().map(|s| s.to_string()),
            )
        })
        .collect()
}

/// The emitted module's default-export fn name + param list.
fn emitted_export(code: &str) -> (String, Vec<String>) {
    let marker = "export default function ";
    let idx = code.find(marker).expect("an export default function");
    let after = &code[idx + marker.len()..];
    let name = after.split('(').next().unwrap().trim().to_string();
    let params_str = after.split('(').nth(1).unwrap().split(')').next().unwrap();
    let params = params_str
        .split(',')
        .map(|p| p.split('=').next().unwrap().trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    (name, params)
}

/// The golden's `exportDefault` name + params.
fn golden_export(golden: &serde_json::Value) -> (String, Vec<String>) {
    let name = golden["exportDefault"]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let params = golden["exportDefault"]["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    (name, params)
}

/// Normalize a FULL JS module for the emitted-JS equivalence comparison — the
/// Rust port of `scripts/svelte-golden-lib.mjs::normalizeModuleForComparison`.
///
/// Collapses cosmetic whitespace OUTSIDE string/template/HTML literals (so a
/// tabs-vs-spaces / line-wrap / blank-line reflow does not false-fail), while
/// preserving whitespace INSIDE string / template-literal literals BYTE-EXACT (so
/// `$$props.bar` vs `.foo`, raw `count` vs `$.get(count)`, a dropped `$.child(_,
/// true)` arg, a sibling-offset drift, or significant template TEXT whitespace
/// still fails). Comments are dropped. This is the FIDELITY the helper-name
/// sequence misses; it MUST stay byte-equivalent to the JS lib so the committed
/// `clientModule` (produced by the lib) and Verter's normalized output compare.
fn normalize_module_for_comparison(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    // Template-literal frames: each tracks the `${}` interpolation depth (0 = in
    // template TEXT).
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
            // Template TEXT — copied verbatim (significant DOM whitespace).
            out.push(ch);
            i += 1;
            continue;
        }
        let ch = chars[i];
        let next = if i + 1 < n { chars[i + 1] } else { '\0' };
        if ch == '/' && next == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            out.push(ch);
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
                out.push(chars[i]);
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
        if ch.is_whitespace() {
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            // Suppress a collapsed space that HUGS a bracket: a space right after an
            // opener (`(` `[` `{`) or right before a closer (`)` `]` `}`). This makes
            // the official multi-line call wrap (`$.template_effect(\n\t($0) => …\n)`
            // → `$.template_effect( ($0) => … )`) byte-comparable with the single-line
            // form (`$.template_effect(($0) => …)`). Symmetric (both sides) and
            // cosmetic-only: a token difference INSIDE the brackets still fails, and
            // string/template literals are copied verbatim above.
            let prev_is_opener = matches!(out.chars().last(), Some('(') | Some('[') | Some('{'));
            let next_is_closer = matches!(chars.get(i), Some(')') | Some(']') | Some('}'));
            if prev_is_opener || next_is_closer {
                continue;
            }
            out.push(' ');
            continue;
        }
        out.push(ch);
        i += 1;
    }
    // Trim the leading/trailing space the outside-literal collapse may have left.
    // (A `split_whitespace`-style collapse here would DESTROY the whitespace inside
    // string / template literals the per-char loop deliberately preserved.)
    out.trim().to_string()
}

/// The golden's `clientModule` (the normalized official full-module oracle).
fn golden_client_module(golden: &serde_json::Value) -> String {
    golden["clientModule"]
        .as_str()
        .expect("a client golden carries `clientModule`")
        .to_string()
}

/// Whether the emitted module carries the `import * as $ from
/// 'svelte/internal/client'` namespace + the disclose-version side effect (the
/// golden's import topology for a runes component).
fn emitted_imports_ok(code: &str, golden: &serde_json::Value) -> bool {
    let imports = golden["imports"].as_array().unwrap();
    imports.iter().all(|imp| {
        let source = imp["source"].as_str().unwrap();
        match imp["kind"].as_str().unwrap() {
            "sideEffect" => code.contains(&format!("import '{source}';")),
            "namespace" => code.contains(&format!("import * as $ from '{source}';")),
            _ => true,
        }
    })
}

/// Whether `code` parses as a valid JS module through OXC (no panic, no syntax
/// errors). A guard against an emitted module that is structurally valid topology
/// but syntactically broken JS (a stray `export` inside a fn, an unbalanced wrap).
fn parses_as_js(code: &str) -> bool {
    let alloc = Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let ret = oxc_parser::Parser::new(&alloc, code, source_type).parse();
    !ret.panicked && ret.errors.is_empty()
}

/// Every supported emission slug — the headline / robustness fixtures
/// ([`SUPPORTED_FIXTURES`]), the exhaustive supported-sub-shape matrix
/// ([`SUPPORTED_MATRIX`]), and the Block-5a attribute corpus
/// ([`SUPPORTED_ATTRIBUTES`]). All three groups run through the identical compile +
/// OXC-parse + full-module-comparison gate.
fn all_supported_slugs() -> Vec<&'static str> {
    SUPPORTED_FIXTURES
        .iter()
        .chain(SUPPORTED_MATRIX.iter())
        .chain(SUPPORTED_ATTRIBUTES.iter())
        .copied()
        .collect()
}

#[test]
fn every_supported_fixture_emits_valid_js() {
    // GATE: every emitted SUPPORTED fixture module must be VALID JS (OXC-parses
    // clean). Catches a syntactically-broken emission (a stray `export` inside the
    // component fn, an unbalanced expression wrap) that the topology comparison
    // alone would not flag.
    for &slug in &all_supported_slugs() {
        let code = emit(slug);
        assert!(
            parses_as_js(&code),
            "emitted client module for {slug} must be valid JS:\n{code}"
        );
    }
}

#[test]
fn supported_matrix_enumerates_every_documented_sub_shape() {
    // The supported matrix is the positive half of the convergence gate; a
    // shrinking matrix is a coverage regression. This count gate fails LOUDLY if a
    // row is dropped.
    assert_eq!(
        SUPPORTED_MATRIX.len(),
        16,
        "the supported matrix must enumerate all 16 documented supported sub-shapes"
    );
    // No duplicate slugs across the matrix.
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_MATRIX {
        assert!(seen.insert(slug), "duplicate supported-matrix slug {slug}");
    }
}

#[test]
fn supported_attributes_cover_the_full_block5a_corpus() {
    // The Block-5a attribute corpus is the byte-precise oracle for the
    // dynamic-attribute / boolean-property / class-style-directive / autofocus
    // surface; a dropped row is a coverage regression. This count gate fails LOUDLY
    // if a row is dropped, and the no-duplicate check guards against a typo.
    assert_eq!(
        SUPPORTED_ATTRIBUTES.len(),
        31,
        "the attribute corpus must enumerate all 31 `attributes/*` fixtures"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &slug in SUPPORTED_ATTRIBUTES {
        assert!(
            seen.insert(slug),
            "duplicate supported-attribute slug {slug}"
        );
    }
}

#[test]
fn emitted_client_topology_matches_official_goldens() {
    for &slug in &all_supported_slugs() {
        let code = emit(slug);
        let golden = client_golden(slug);

        // (1) The helper SEQUENCE (the load-bearing oracle: the helper families
        // and the order they are emitted).
        assert_eq!(
            helper_sequence(&code),
            golden_sequence(&golden),
            "helper sequence drift for {slug}:\n--- emitted ---\n{code}"
        );

        // (2) The import topology (disclose-version + the client namespace).
        assert!(
            emitted_imports_ok(&code, &golden),
            "import topology drift for {slug}:\n{code}"
        );

        // (3) The export-fn shape (name + params).
        assert_eq!(
            emitted_export(&code),
            golden_export(&golden),
            "export shape drift for {slug}:\n{code}"
        );

        // (4) The `from_html` template skeletons + fragment flags.
        assert_eq!(
            emitted_templates(&code),
            golden_templates(&golden),
            "template skeleton drift for {slug}:\n{code}"
        );

        // (5) The delegated event set (first-seen order).
        assert_eq!(
            emitted_delegated(&code),
            golden_delegated(&golden),
            "delegated-event drift for {slug}:\n{code}"
        );

        // (6) THE FULL-MODULE comparison — Verter's normalized emitted module vs the
        // official normalized golden. This is the argument/offset/identifier-precise
        // oracle: it catches a `$$props.bar` vs `.foo`, a raw `count` vs
        // `$.get(count)`, a dropped `$.child(_, true)` arg, a sibling-offset drift,
        // or significant template TEXT whitespace that the helper-name SEQUENCE
        // (assertion 1) cannot see. Cosmetic whitespace OUTSIDE literals is
        // normalized on both sides; literal/template TEXT is byte-exact.
        assert_eq!(
            normalize_module_for_comparison(&code),
            golden_client_module(&golden),
            "FULL-MODULE drift for {slug} (argument/identifier/offset-precise):\n\
             --- emitted (normalized) ---\n{}\n--- golden (official, normalized) ---\n{}\n\
             --- emitted (raw) ---\n{code}",
            normalize_module_for_comparison(&code),
            golden_client_module(&golden),
        );
    }
}

#[test]
fn full_module_gate_discriminates_the_pre_fix_defects() {
    // DISCRIMINATION proof for the full-module gate: the normalized full-module
    // comparison REJECTS representative pre-fix output shapes for KEPT supported
    // fixtures, proving the gate is non-vacuous (it would have FAILED a defective
    // tree). For each, the defective normalized output differs from the committed
    // official golden; the current emitter matches (asserted by the main gate test).
    //
    // props alias: a defect reading `$$props.bar` (the alias) instead of the source
    // key `$$props.foo`.
    let alias_golden = golden_client_module(&client_golden("runes/props_alias"));
    let pre_fix_alias = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<p> </p>`);\n\
         export default function props_alias($$anchor, $$props) {\n\
           var p = root();\n\
           var text = $.child(p, true);\n\
           $.reset(p);\n\
           $.template_effect(() => $.set_text(text, $$props.bar));\n\
           $.append($$anchor, p);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_alias, alias_golden,
        "the gate MUST reject the alias-keyed `$$props.bar` output"
    );

    // is_text: a defect dropping the `true` arg on a pure-interp text child
    // (`$.child(p)` instead of `$.child(p, true)`).
    let is_text_golden = golden_client_module(&client_golden("runes/is_text_flag"));
    let pre_fix_is_text = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<p> </p> <button>x</button>`, 1);\n\
         export default function is_text_flag($$anchor) {\n\
           let count = $.state(0);\n\
           var fragment = root();\n\
           var p = $.first_child(fragment);\n\
           var text = $.child(p);\n\
           $.reset(p);\n\
           var button = $.sibling(p, 2);\n\
           $.template_effect(() => $.set_text(text, $.get(count)));\n\
           $.delegated('click', button, () => $.update(count));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_is_text, is_text_golden,
        "the gate MUST reject the `$.child(p)` (missing is_text) output"
    );

    // [P0] lone `<video>` (template flag `2` = USE_IMPORT_NODE, NOT a fragment): the
    // pre-fix defect treated ANY trailing `from_html` flag as a multi-root fragment,
    // so it cloned via `$.first_child(root())` — which is `null` for a single
    // element → a runtime `TypeError`. Official takes the single clone-root path
    // (`var video = root();`). The gate MUST reject the fragment-walk shape.
    let muted_golden = golden_client_module(&client_golden("attributes/muted_video"));
    let pre_fix_muted = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<video></video>`, 2);\n\
         export default function muted_video($$anchor) {\n\
           let muted = $.state(false);\n\
           var fragment = root();\n\
           var video = $.first_child(fragment);\n\
           $.template_effect(() => video.muted = $.get(muted));\n\
           $.delegated('click', video, () => $.set(muted, !$.get(muted)));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_muted, muted_golden,
        "the gate MUST reject the lone-`<video>` fragment-walk (`$.first_child`) output"
    );

    // Emission ORDER: the pre-fix defect emitted ALL element walk vars first, THEN
    // all the `let classes;` accumulator decls in one post-walk block. Official
    // INTERLEAVES each `let classes;` immediately after its element's walk var. The
    // gate MUST reject the non-interleaved order.
    let class_dirs_golden = golden_client_module(&client_golden("attributes/class_directives"));
    let pre_fix_class_dirs = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<button></button> <button></button>`, 1);\n\
         export default function class_directives($$anchor) {\n\
           let on = $.state(false);\n\
           let off = $.state(false);\n\
           let c = 'a';\n\
           var fragment = root();\n\
           var button = $.first_child(fragment);\n\
           var button_1 = $.sibling(button, 2);\n\
           let classes;\n\
           let classes_1;\n\
           $.template_effect(() => {\n\
             classes = $.set_class(button, 1, 'base', null, classes, { foo: $.get(on) });\n\
             classes_1 = $.set_class(button_1, 1, $.clsx(c), null, classes_1, { foo: $.get(on), bar: !$.get(off) });\n\
           });\n\
           $.delegated('click', button, () => $.set(on, !$.get(on)));\n\
           $.delegated('click', button_1, () => $.set(off, !$.get(off)));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_class_dirs, class_dirs_golden,
        "the gate MUST reject the non-interleaved (post-walk-block) `let classes;` order"
    );

    // Emission ORDER: the pre-fix defect emitted all walk vars (+ the `$.reset`) and
    // THEN the `$.autofocus(...)` inits in a post-walk block. Official emits each
    // `$.autofocus(node, …)` inline immediately after the node's walk var. The gate
    // MUST reject the post-walk autofocus block.
    let autofocus_golden = golden_client_module(&client_golden("attributes/autofocus"));
    let pre_fix_autofocus = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/> <input/> <button> </button>`, 1);\n\
         export default function autofocus($$anchor) {\n\
           let c = $.state(0);\n\
           let on = $.state(true);\n\
           var fragment = root();\n\
           var input = $.first_child(fragment);\n\
           var input_1 = $.sibling(input, 2);\n\
           var button = $.sibling(input_1, 2);\n\
           var text = $.child(button, true);\n\
           $.reset(button);\n\
           $.autofocus(input, true);\n\
           $.autofocus(input_1, $.get(on));\n\
           $.template_effect(() => $.set_text(text, $.get(c)));\n\
           $.delegated('click', input_1, () => $.set(on, !$.get(on)));\n\
           $.delegated('click', button, () => $.update(c));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_autofocus, autofocus_golden,
        "the gate MUST reject the post-walk `$.autofocus(...)` block order"
    );

    // ESCAPING — literal `${` in a mixed template: the pre-fix `escape_template_text`
    // left `${` UNescaped, producing `` `a${b${…}` `` (the literal `${b` opens a
    // bogus interpolation = invalid JS). Official escapes it to `\${`.
    let dollar_golden = golden_client_module(&client_golden("attributes/mixed_template_dollar"));
    let pre_fix_dollar = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/> <button>go</button>`, 1);\n\
         export default function mixed_template_dollar($$anchor) {\n\
           let v = $.state(0);\n\
           var fragment = root();\n\
           var input = $.first_child(fragment);\n\
           var button = $.sibling(input, 2);\n\
           $.template_effect(() => $.set_attribute(input, 'id', `a${b${$.get(v) ?? ''}`));\n\
           $.delegated('click', button, () => $.update(v));\n\
           $.append($$anchor, fragment);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_dollar, dollar_golden,
        "the gate MUST reject the unescaped dollar-brace template (escape_template_text)"
    );

    // ESCAPING — newline in a single-quoted static class base: the pre-fix
    // `js_single_quoted` escaped only `\\` and `'`, emitting a RAW newline inside
    // `'…'` (invalid JS). Official escapes it to `\\n`. The pre-fix shape carries a
    // real newline in the literal (preserved byte-exact by the normalizer), so it
    // differs from the `'a\\nb'` golden.
    let newline_golden = golden_client_module(&client_golden("attributes/static_class_newline"));
    let pre_fix_newline = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<div></div>`);\n\
         export default function static_class_newline($$anchor) {\n\
           let c = false;\n\
           var div = root();\n\
           $.set_class(div, 1, 'a\nb', null, {}, { x: c });\n\
           $.append($$anchor, div);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_newline, newline_golden,
        "the gate MUST reject a raw newline inside a single-quoted base (`js_single_quoted`)"
    );

    // MEMOIZATION GRANULARITY — a mixed class base with a call: the pre-fix path
    // memoized the WHOLE rendered template (`$0` as the value, the template in the
    // dep), instead of the EXPRESSION PART (the template in the body, the call in
    // the dep). Official: `($0) => $.set_class(div, 1, `a${$0 ?? ''}b`), [() =>
    // String($.get(c))]`.
    let mixed_call_golden = golden_client_module(&client_golden("attributes/mixed_class_call"));
    let pre_fix_mixed_call = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<div></div>`);\n\
         export default function mixed_class_call($$anchor) {\n\
           let c = $.state('x');\n\
           var div = root();\n\
           $.template_effect(($0) => $.set_class(div, 1, $0), [() => `a${String($.get(c)) ?? ''}b`]);\n\
           $.delegated('click', div, () => $.set(c, $.get(c) + '!'));\n\
           $.append($$anchor, div);\n\
         }\n\
         $.delegate(['click']);\n",
    );
    assert_ne!(
        pre_fix_mixed_call, mixed_call_golden,
        "the gate MUST reject whole-template memoization of a mixed class base"
    );

    // ORDERING — `bind:this` + an init-domain dynamic attr: the pre-fix order emitted
    // `$.bind_this(...)` BEFORE the `$.set_attribute(...)` init. Official emits the
    // init-domain write first, then `$.bind_this`.
    let bind_order_golden =
        golden_client_module(&client_golden("attributes/bind_this_dynamic_attr"));
    let pre_fix_bind_order = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function bind_this_dynamic_attr($$anchor) {\n\
           let el;\n\
           let who = 'a';\n\
           var input = root();\n\
           $.bind_this(input, ($$value) => el = $$value, () => el);\n\
           $.set_attribute(input, 'id', who);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_bind_order, bind_order_golden,
        "the gate MUST reject `$.bind_this` emitted before the init-domain attr write"
    );

    // `has_call` REACTIVE TRIGGER — a demoted `$state` call-expr property: the pre-fix
    // gate (reactive iff `has_state`) emitted a one-shot `input.readOnly = Boolean(v)`
    // init. Official memoizes any `has_call` value into the effect:
    // `$.template_effect(($0) => input.readOnly = $0, [() => Boolean(v)])`.
    let demoted_golden =
        golden_client_module(&client_golden("attributes/call_expr_property_demoted"));
    let pre_fix_demoted = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function call_expr_property_demoted($$anchor) {\n\
           let v = false;\n\
           var input = root();\n\
           input.readOnly = Boolean(v);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_demoted, demoted_golden,
        "the gate MUST reject the one-shot (non-memoized) demoted-$state call-expr init"
    );

    // `has_call` SOURCE ORDER — a PURE call BEFORE its first dependency: the pre-fix
    // `has_call` used a WHOLE-EXPRESSION "references any binding" precompute, so it saw
    // the (later) `flag` dependency and OVER-MEMOIZED the pure-call-before-dep value
    // into the deps-array effect form. Official accumulates `dependencies` in source
    // order and checks PER CALL against the deps-so-far: at the pure call, zero deps
    // have accumulated → NOT `has_call` → an INLINE `input.readOnly = … || flag` init.
    // The gate MUST reject the over-memoized `$.template_effect(($0) => …, [() => …])`
    // shape for the before-dep value.
    let before_golden = golden_client_module(&client_golden("attributes/pure_call_before_dep"));
    let pre_fix_before = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function pure_call_before_dep($$anchor) {\n\
           let flag = false;\n\
           var input = root();\n\
           $.template_effect(($0) => input.readOnly = $0, [() => (globalThis?.check?.() ?? false) || flag]);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_before, before_golden,
        "the gate MUST reject over-memoizing a pure call that precedes its first dependency"
    );
    // And the AFTER-dep mirror is genuinely the memoized form (a confidence check that
    // the discrimination pair is not symmetric): the pre-fix INLINE shape for the
    // after-dep value differs from the memoized golden, so a regression that
    // UNDER-memoizes the after-dep case is also caught.
    let after_golden = golden_client_module(&client_golden("attributes/dep_before_pure_call"));
    let pre_fix_after_inline = normalize_module_for_comparison(
        "import 'svelte/internal/disclose-version';\n\
         import * as $ from 'svelte/internal/client';\n\
         var root = $.from_html(`<input/>`);\n\
         export default function dep_before_pure_call($$anchor) {\n\
           let flag = false;\n\
           var input = root();\n\
           input.readOnly = flag || (globalThis?.check?.() ?? false);\n\
           $.append($$anchor, input);\n\
         }\n",
    );
    assert_ne!(
        pre_fix_after_inline, after_golden,
        "the gate MUST reject under-memoizing a pure call that follows a dependency"
    );
}

#[test]
fn normalizer_preserves_whitespace_inside_literals() {
    // F12: the normalizer collapses cosmetic whitespace OUTSIDE literals but
    // PRESERVES it inside string / template-literal TEXT — so meaningful text
    // whitespace drift (a `Hello  world` vs `Hello world` in a template, or a
    // changed string literal) still fails. RED against the old normalizer that
    // stripped ALL whitespace including inside literals.
    let a = normalize_module_for_comparison("var x =  `Hello  ${y}  world`;");
    let b = normalize_module_for_comparison("var x = `Hello ${y} world`;");
    assert_ne!(
        a, b,
        "whitespace INSIDE a template literal must be preserved (not collapsed)"
    );
    // Outer whitespace RUNS are collapsed to a single space, so two modules
    // differing ONLY in outer whitespace-run length (tabs / newlines / multiple
    // spaces vs one) normalize equal.
    let c = normalize_module_for_comparison("var x  =\n\t`t`;");
    let d = normalize_module_for_comparison("var x = `t`;");
    assert_eq!(c, d, "cosmetic whitespace OUTSIDE literals is collapsed");
    // A string literal's interior whitespace is preserved.
    assert_ne!(
        normalize_module_for_comparison("var s = 'a  b';"),
        normalize_module_for_comparison("var s = 'a b';"),
        "whitespace inside a string literal must be preserved"
    );
}

#[test]
fn helper_sequence_masking_ignores_helper_shaped_strings() {
    // DISCRIMINATING self-test for the masker: a `$.foo` token inside a STRING or
    // a template-literal TEXT span is NOT a helper reference. (Guards the gate
    // against a naive regex that would mis-count.)
    let code = "var x = '$.fake'; var y = `text $.alsofake ${$.real(1)}`; $.outer();";
    let seq = helper_sequence(code);
    assert_eq!(seq, vec!["real".to_string(), "outer".to_string()]);
    assert!(!seq.iter().any(|h| h == "fake" || h == "alsofake"));
}

// ===========================================================================
// The SYSTEMATIC CODEGEN CORPUS — Block 5a's slice of the cumulative 5a–5m
// codegen corpus (`scripts/gen-svelte-codegen-corpus.mjs`).
//
// The generator mechanically enumerates Block 5a's codegen surface over three
// orthogonal axes — value-expression SHAPE × TARGET × REACTIVITY — and pins the
// OFFICIAL pinned-`svelte@5.56.3` module of every cell as the golden (under the
// `codegen/` subtree: `<slug>.svelte` + `<slug>.client.json` together). This gate
// recompiles every cell with Verter, normalizes its emitted module the SAME way, and
// asserts byte-equality (the argument/offset/identifier-precise oracle), plus the
// helper-topology fields. A COVERAGE gate (reading the manifest) asserts every
// required value-shape / target / reactivity axis contributes ≥1 committed row, so a
// dropped enumerator fails HARD. This is the convergence tool: the per-edge byte tail
// is closed by a systematic enumeration, not whack-a-mole.
// ===========================================================================

/// The `codegen/` corpus directory (fixtures + goldens together).
fn codegen_dir() -> PathBuf {
    repo_root().join("crates/verter_compiler/tests/svelte_oracle_corpus/codegen")
}

/// Every BYTE-MATCH codegen cell slug (a `<slug>.client.json` golden under `codegen/`
/// whose Verter output must equal official's byte-for-byte). A `refuse` cell has NO
/// `.client.json` (official compile-fails it) and a `live-fallback` cell has NO
/// `.client.json` (Verter emits the LIVE form, not official's folded literal — and that
/// literal can be a lone surrogate a strict JSON reader rejects), so neither appears here.
/// Sorted lexicographically; `manifest.json` is excluded.
fn codegen_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".client.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// Every `refuse`-bucket cell slug (a `<slug>.refuse.json` marker — an official-rejected
/// const-fold throw Verter must ALSO refuse). Sorted.
fn codegen_refuse_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".refuse.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// Every `live-fallback`-bucket cell slug (a `<slug>.live.json` marker — official folds but
/// Verter emits the LIVE form). Sorted.
fn codegen_live_fallback_slugs() -> Vec<String> {
    let dir = codegen_dir();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read codegen dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".live.json").map(|s| s.to_string())
        })
        .collect();
    slugs.sort();
    slugs
}

/// The `.refuse.json` / `.live.json` record for a bucket cell slug.
fn codegen_bucket_record(slug: &str, ext: &str) -> serde_json::Value {
    let path = codegen_dir().join(format!("{slug}.{ext}"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read codegen {ext} record {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen {ext} record {slug}: {e}"))
}

/// The fixture source for a codegen cell slug.
fn codegen_fixture_source(slug: &str) -> String {
    let path = codegen_dir().join(format!("{slug}.svelte"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read codegen fixture {slug}: {e}"))
}

/// The committed official client golden for a codegen cell slug.
fn codegen_golden(slug: &str) -> serde_json::Value {
    let path = codegen_dir().join(format!("{slug}.client.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read codegen golden {slug}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen golden {slug}: {e}"))
}

/// Compile a codegen cell to its emitted client JS (the `codegen/` fixtures live in
/// their own subtree, so this reuses the shared `component_name_for` rule but reads
/// from `codegen_dir`).
fn codegen_emit(slug: &str) -> String {
    codegen_try_emit(slug)
        .unwrap_or_else(|e| panic!("codegen client emission failed for {slug}: {e:?}"))
}

/// The FALLIBLE Verter compile of a codegen cell — `Ok(code)` when Verter emits a module,
/// `Err(ClientCompileError)` when it refuses (the refuse-bucket gate asserts the `Err`
/// carries the `const-fold-throw` diagnostic).
fn codegen_try_emit(slug: &str) -> Result<String, ClientCompileError> {
    let source = codegen_fixture_source(slug);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = SvelteRuntimeOptions {
        filename: Some(format!("{slug}.svelte")),
        name: Some(component_name_for(slug)),
        ..Default::default()
    };
    compile_client(&source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// The codegen corpus manifest (the coverage authority).
fn codegen_manifest() -> serde_json::Value {
    let path = codegen_dir().join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read codegen manifest: {e}\n(run scripts/gen-svelte-codegen-corpus.mjs)")
    });
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse codegen manifest: {e}"))
}

#[test]
fn codegen_corpus_is_nonempty_and_every_cell_emits_valid_js() {
    let slugs = codegen_slugs();
    assert!(
        slugs.len() >= 200,
        "the codegen corpus must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    // Every emitted codegen cell must be VALID JS (OXC-parses clean) — catches a
    // syntactically-broken emission the topology comparison alone would not flag.
    for slug in &slugs {
        let code = codegen_emit(slug);
        assert!(
            parses_as_js(&code),
            "emitted codegen module for {slug} must be valid JS:\n{code}"
        );
    }
}

#[test]
fn codegen_corpus_covers_every_required_axis() {
    // COVERAGE GATE: the manifest declares the required value-shape / target /
    // reactivity axes; every one must contribute ≥1 committed cell. A dropped
    // enumerator (the corpus silently losing a finite axis) fails HARD here, mirroring
    // the JS generator's own coverage check. This is what makes the corpus a REAL gate
    // rather than a hand-curated sample.
    let manifest = codegen_manifest();
    let required = |key: &str| -> Vec<String> {
        manifest[key]
            .as_array()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let counts = |key: &str| -> serde_json::Map<String, serde_json::Value> {
        manifest[key]
            .as_object()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .clone()
    };

    // (The const-fold tri-state buckets — `fold-exact` / `refuse` / `live-fallback` — are
    // covered by `const_fold_buckets_cover_every_required_family_and_eagerness`.)
    for (req_key, count_key, label) in [
        ("required_shape_axes", "shape_counts", "value-shape"),
        ("required_target_axes", "target_counts", "target"),
        (
            "required_reactivity_axes",
            "reactivity_counts",
            "reactivity",
        ),
        ("required_content_axes", "content_counts", "content"),
        ("required_container_axes", "container_counts", "container"),
    ] {
        let req = required(req_key);
        let cnts = counts(count_key);
        for axis in &req {
            let n = cnts.get(axis).and_then(|v| v.as_u64()).unwrap_or(0);
            assert!(
                n >= 1,
                "codegen corpus is missing the {label} axis `{axis}` (0 committed cells); \
                 regenerate with scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }

    // The committed manifest total matches the on-disk cell count across ALL THREE buckets
    // (byte-match `fold-exact`/PASS1/PASS2 cells + `refuse` markers + `live-fallback`
    // markers) — no orphan / missing cell the discovery walk would silently include or
    // exclude. (`codegen_slugs()` already excludes the live-fallback cells, which are
    // counted separately.)
    let total = manifest["total"].as_u64().unwrap() as usize;
    let on_disk =
        codegen_slugs().len() + codegen_refuse_slugs().len() + codegen_live_fallback_slugs().len();
    assert_eq!(
        total,
        on_disk,
        "manifest `total` must equal the committed cell count across all three buckets \
         (byte-match {} + refuse {} + live-fallback {})",
        codegen_slugs().len(),
        codegen_refuse_slugs().len(),
        codegen_live_fallback_slugs().len(),
    );

    // HARDCODED axis anchor: the manifest's `required_*` lists are DERIVED from the
    // generator's `SHAPES`/`TARGETS`/`REACTIVITIES`, so dropping an enumerator there also
    // shrinks the `required_*` list — which the ≥1-row loop above would then vacuously
    // satisfy. Pinning the full Block-5a axis vocabulary HERE (independent of the
    // manifest) makes a generator-side axis drop fail the Rust gate, not only the JS
    // generator's own check. These are the architect-specified Block-5a codegen axes.
    let shapes: std::collections::BTreeSet<String> =
        required("required_shape_axes").into_iter().collect();
    for shape in [
        "literal",
        "binary",
        "template",
        "member",
        "call_pure",
        "call_impure",
        "optional_call",
        "optional_member",
        "conditional",
        "logical_and",
        "logical_or",
        "logical_nullish",
        "sequence",
        "object",
        "array",
        "call_arg_spread",
        "array_spread",
        "object_spread",
        "new",
        "tagged_template",
    ] {
        assert!(
            shapes.contains(shape),
            "the codegen corpus must declare the `{shape}` value-shape axis \
             (a dropped generator enumerator)"
        );
    }
    let targets: std::collections::BTreeSet<String> =
        required("required_target_axes").into_iter().collect();
    for target in [
        "attr",
        "boolean",
        "class",
        "class_directive",
        "style",
        "style_directive",
    ] {
        assert!(
            targets.contains(target),
            "the codegen corpus must declare the `{target}` target axis"
        );
    }
    let reactivities: std::collections::BTreeSet<String> =
        required("required_reactivity_axes").into_iter().collect();
    for react in ["state", "props", "demoted", "pure"] {
        assert!(
            reactivities.contains(react),
            "the codegen corpus must declare the `{react}` reactivity axis"
        );
    }

    // The CONTENT sub-axis (the inner value-shape a container's hole holds) and the
    // CONTAINER axis (the value-form with a content hole) — the fix-#6 nesting class.
    // Pinned HERE (independent of the manifest) so a generator-side enumerator drop fails
    // the Rust gate, not only the JS generator's own check.
    let content_axes: std::collections::BTreeSet<String> =
        required("required_content_axes").into_iter().collect();
    for content in [
        "identifier",
        "binary",
        "logical_and",
        "logical_or",
        "logical_nullish",
        "conditional",
        "unary",
        "sequence",
        "member",
        "call",
    ] {
        assert!(
            content_axes.contains(content),
            "the codegen corpus must declare the `{content}` content sub-axis"
        );
    }
    let container_axes: std::collections::BTreeSet<String> =
        required("required_container_axes").into_iter().collect();
    for container in ["tmpl", "cond", "log", "call_arg"] {
        assert!(
            container_axes.contains(container),
            "the codegen corpus must declare the `{container}` container axis"
        );
    }

    // The `fold-exact` bucket families (the EXACT-value rows pinning official's
    // `scope.evaluate` coercion / globals / operator semantics that Verter folds
    // byte-identically). Pinned HERE so a generator-side enumerator drop fails the Rust
    // gate, not only the JS check. (The `refuse` / `live-fallback` bucket families are
    // pinned in `const_fold_buckets_cover_every_required_family_and_eagerness`.)
    let fold_exact_families: std::collections::BTreeSet<String> =
        required("required_fold_exact_families")
            .into_iter()
            .collect();
    for family in [
        "bigint",
        "number_coerce",
        "string_coerce",
        "global_call",
        "global_const",
        "tricky_number",
    ] {
        assert!(
            fold_exact_families.contains(family),
            "the fold-exact bucket must declare the `{family}` const-fold edge family \
             (a dropped generator enumerator)"
        );
    }

    // THE DOCTRINE: every container × content combination MUST have a committed cell (a
    // missing container×content cell is a generator bug). Each such cell's slug is
    // `<container>_<content>[_multi]__<target>__<reactivity>`, so a `<container>_<content>__`
    // prefix proves the combination exists.
    let slugs = codegen_slugs();
    for container in &container_axes {
        for content in &content_axes {
            let prefix = format!("{container}_{content}__");
            let multi_prefix = format!("{container}_{content}_multi__");
            assert!(
                slugs
                    .iter()
                    .any(|s| s.starts_with(&prefix) || s.starts_with(&multi_prefix)),
                "the codegen corpus is missing the container×content cell `{container}×{content}` \
                 (no `{prefix}…` slug); regenerate with scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }
}

#[test]
fn emitted_codegen_corpus_matches_official_goldens() {
    // THE convergence gate: for every systematic codegen cell, Verter's normalized
    // emitted module must equal the OFFICIAL normalized golden byte-for-byte (modulo
    // the cosmetic-whitespace collapse), plus the helper-topology fields. A divergence
    // in helper choice, memoization shape, effect/thunk structure, dependency tracking,
    // prop/property routing, or class/style normalization fails here.
    for slug in &codegen_slugs() {
        let code = codegen_emit(slug);
        let golden = codegen_golden(slug);

        // (1) The helper SEQUENCE (families + emission order).
        assert_eq!(
            helper_sequence(&code),
            golden_sequence(&golden),
            "helper sequence drift for codegen cell {slug}:\n--- emitted ---\n{code}"
        );
        // (2) The import topology.
        assert!(
            emitted_imports_ok(&code, &golden),
            "import topology drift for codegen cell {slug}:\n{code}"
        );
        // (3) The export-fn shape.
        assert_eq!(
            emitted_export(&code),
            golden_export(&golden),
            "export shape drift for codegen cell {slug}:\n{code}"
        );
        // (4) The `from_html` template skeletons + fragment flags.
        assert_eq!(
            emitted_templates(&code),
            golden_templates(&golden),
            "template skeleton drift for codegen cell {slug}:\n{code}"
        );
        // (5) The delegated event set.
        assert_eq!(
            emitted_delegated(&code),
            golden_delegated(&golden),
            "delegated-event drift for codegen cell {slug}:\n{code}"
        );
        // (6) THE FULL-MODULE byte comparison (argument/offset/identifier-precise).
        assert_eq!(
            normalize_module_for_comparison(&code),
            golden_client_module(&golden),
            "FULL-MODULE drift for codegen cell {slug} (argument/identifier/offset-precise):\n\
             --- emitted (normalized) ---\n{}\n--- golden (official, normalized) ---\n{}\n\
             --- emitted (raw) ---\n{code}",
            normalize_module_for_comparison(&code),
            golden_client_module(&golden),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The const-fold TRI-STATE contract — the three corpus buckets.
//
// `fold-exact` cells are byte-compared in `emitted_codegen_corpus_matches_official_goldens`
// (they carry a `.client.json` golden Verter must match). The `refuse` and `live-fallback`
// buckets need DISTINCT gates: a refuse cell has NO official output (official compile-fails)
// and Verter must REFUSE; a live-fallback cell has an official FOLDED golden but Verter
// emits the LIVE form (deliberately NOT byte-equal).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn refuse_bucket_cells_are_refused_with_const_fold_throw() {
    // Each `refuse`-bucket cell is an official-rejected const-fold throw (the generator
    // confirmed pinned svelte COMPILE-FAILS it). Verter must ALSO refuse — deterministically,
    // with the `const-fold-throw` diagnostic — never emit live code (which would turn the
    // compile-failure into a runtime crash).
    let slugs = codegen_refuse_slugs();
    assert!(
        slugs.len() >= 10,
        "the refuse bucket must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    for slug in &slugs {
        // The generator's `.refuse.json` records official's compile-failure.
        let record = codegen_bucket_record(slug, "refuse.json");
        assert_eq!(
            record["officialRejected"].as_bool(),
            Some(true),
            "refuse cell {slug} must record official rejection"
        );
        // Verter must REFUSE.
        match codegen_try_emit(slug) {
            Err(ClientCompileError::Unsupported(surface)) => {
                assert!(
                    matches!(surface, UnsupportedSvelteRuntimeSurface::ConstFoldThrow { .. }),
                    "refuse cell {slug} must refuse with ConstFoldThrow, got {surface:?}"
                );
                assert_eq!(
                    surface.diagnostic_code(),
                    "svelte-runtime-unsupported-const-fold-throw",
                    "refuse cell {slug} must carry the const-fold-throw diagnostic code"
                );
                assert_eq!(
                    surface.owning_block(),
                    "5a",
                    "a const-fold throw is a 5a mixed-attribute surface"
                );
            }
            Ok(code) => panic!(
                "refuse cell {slug} was EMITTED by Verter (official compile-FAILS it — emitting \
                 live code turns a compile error into a runtime crash):\n{code}"
            ),
            Err(other) => panic!(
                "refuse cell {slug} must refuse with ConstFoldThrow, got a different error: {other:?}"
            ),
        }
    }
}

#[test]
fn live_fallback_bucket_cells_emit_live_not_the_folded_literal() {
    // Each `live-fallback`-bucket cell folds in official, but Verter cannot prove byte-exact
    // emission so it emits the LIVE expression. The gate asserts: (a) the ledger reason is a
    // checked-in `live_fallback_ledger()` label, (b) official FOLDED the chunk (recorded —
    // no `${` interpolation in official's module), (c) Verter EMITS (a non-throwing value —
    // not a refusal), (d) Verter's output is valid JS, and (e) Verter's output is the LIVE
    // form (a `${…}` interpolation a folded literal never has) — the structural proof that
    // Verter did NOT fold the not-byte-exact value. (No byte comparison against official's
    // folded literal: Verter deliberately differs, and the literal can be a lone surrogate a
    // strict JSON reader rejects.)
    let slugs = codegen_live_fallback_slugs();
    assert!(
        slugs.len() >= 10,
        "the live-fallback bucket must be substantial (got {} cells); regenerate with \
         scripts/gen-svelte-codegen-corpus.mjs",
        slugs.len()
    );
    let ledger_labels: std::collections::BTreeSet<String> = live_fallback_ledger()
        .into_iter()
        .map(|row| row.label.to_string())
        .collect();
    for slug in &slugs {
        let record = codegen_bucket_record(slug, "live.json");
        // (a) The reason is a checked-in ledger label.
        let reason = record["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("live cell {slug} must record a ledger reason"));
        assert!(
            ledger_labels.contains(reason),
            "live cell {slug} reason `{reason}` must be a checked-in live_fallback_ledger() \
             label (got ledger {ledger_labels:?})"
        );
        // (b) Official FOLDED the chunk (its module inlined the value, no `${` for it).
        assert_eq!(
            record["officialModuleHasInterpolation"].as_bool(),
            Some(false),
            "live cell {slug}: official must FOLD the chunk (no live `${{` in its module) — \
             the contrast the bucket documents"
        );
        // (c) Verter EMITS (a live-fallback is a non-throwing value — never a refusal).
        let code = match codegen_try_emit(slug) {
            Ok(code) => code,
            Err(e) => panic!(
                "live-fallback cell {slug} must EMIT the live form (official folds a \
                 non-throwing value), but Verter refused: {e:?}"
            ),
        };
        // (d) Valid JS.
        assert!(
            parses_as_js(&code),
            "live-fallback cell {slug} must emit valid JS:\n{code}"
        );
        // (e) The emitted module must contain a LIVE template interpolation (a `${…}` over
        // the chunk) — the structural proof Verter emitted live, NOT a folded
        // `$.set_attribute(.., 'a <lit> b')`. A folded literal never has `${`.
        assert!(
            code.contains("${"),
            "live-fallback cell {slug} must emit a live template interpolation (`${{…}}`) — a \
             folded output (no `${{`) would mean Verter wrongly folded a not-byte-exact \
             value:\n{code}"
        );
    }
}

#[test]
fn const_fold_buckets_cover_every_required_family_and_eagerness() {
    // The strengthened coverage gate: each of the THREE buckets contributes its required
    // families, the eagerness refuse rows are present, and the buckets are crossed with the
    // class/style/boolean targets (target-independence of the tri-state decision).
    let manifest = codegen_manifest();
    let required = |key: &str| -> Vec<String> {
        manifest[key]
            .as_array()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let counts = |key: &str| -> serde_json::Map<String, serde_json::Value> {
        manifest[key]
            .as_object()
            .unwrap_or_else(|| panic!("manifest missing {key}"))
            .clone()
    };

    // Every required family in each bucket has ≥1 row.
    for (req_key, count_key, label) in [
        (
            "required_fold_exact_families",
            "fold_exact_counts",
            "fold-exact",
        ),
        ("required_refuse_families", "refuse_counts", "refuse"),
        (
            "required_live_fallback_families",
            "live_fallback_counts",
            "live-fallback",
        ),
    ] {
        let req = required(req_key);
        assert!(
            !req.is_empty(),
            "the {label} bucket must declare at least one required family"
        );
        let cnts = counts(count_key);
        for fam in &req {
            let n = cnts.get(fam).and_then(|v| v.as_u64()).unwrap_or(0);
            assert!(
                n >= 1,
                "the {label} bucket is missing family `{fam}` (0 rows); regenerate with \
                 scripts/gen-svelte-codegen-corpus.mjs"
            );
        }
    }

    // The EAGERNESS refuse family MUST be present (a throw in a non-selected logical operand
    // / conditional branch — the `false && (1n/0n)` / `true ? 1 : (1n/0n)` rows).
    let refuse_fams: std::collections::BTreeSet<String> =
        required("required_refuse_families").into_iter().collect();
    assert!(
        refuse_fams.contains("refuse_eager"),
        "the refuse bucket must include the `refuse_eager` family (eagerness throws)"
    );

    // The three buckets exist on disk and are non-trivial.
    assert!(
        codegen_refuse_slugs().len() >= 10,
        "the refuse bucket must have ≥10 cells"
    );
    assert!(
        codegen_live_fallback_slugs().len() >= 10,
        "the live-fallback bucket must have ≥10 cells"
    );

    // The buckets are CROSSED with class / style / boolean targets (a `__class` / `__style`
    // / `__boolean` slug suffix proves target-independence of the const-fold decision).
    for (slugs, bucket) in [
        (codegen_refuse_slugs(), "refuse"),
        (codegen_live_fallback_slugs(), "live-fallback"),
    ] {
        for axis in ["class", "style", "boolean"] {
            let suffix = format!("__{axis}");
            assert!(
                slugs.iter().any(|s| s.ends_with(&suffix)),
                "the {bucket} bucket must cross a representative row over the `{axis}` target \
                 (no `…{suffix}` slug)"
            );
        }
    }

    // An eagerness refuse cell MUST actually be present on disk and refuse (cross-check the
    // family declaration against a concrete cell).
    let refuse_slugs = codegen_refuse_slugs();
    assert!(
        refuse_slugs.iter().any(|s| s.contains("eager")),
        "the refuse bucket must contain a concrete eagerness cell"
    );
}
