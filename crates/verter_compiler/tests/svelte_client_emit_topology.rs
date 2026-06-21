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
use verter_compiler::svelte::runtime::{compile_client, SvelteRuntimeOptions};

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
/// ([`SUPPORTED_FIXTURES`]) PLUS the exhaustive supported-sub-shape matrix
/// ([`SUPPORTED_MATRIX`]). Both groups run through the identical compile +
/// OXC-parse + full-module-comparison gate.
fn all_supported_slugs() -> Vec<&'static str> {
    SUPPORTED_FIXTURES
        .iter()
        .chain(SUPPORTED_MATRIX.iter())
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
