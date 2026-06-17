//! Static architecture guard: the `verter_mcp` tool surface must classify and
//! route framework-component files carrier-generically.
//!
//! Verter is one shared, carrier-generic substrate. A `.vue` SFC and a
//! `.svelte` component are both framework CARRIERS (`FileLanguage::Framework`,
//! `is_framework_carrier()`); the MCP tools that operate on carrier-NEUTRAL
//! analysis (CSS bleed, linting, the component graph, props, prop-drilling,
//! events, test-impact, route snapshots, project stats) must gate on the
//! carrier-generic predicate, never on a Vue-only `.is_vue()` call or a
//! `.vue`-suffix path classifier. A Vue-only gate over carrier-neutral data
//! silently strands every OTHER carrier (`.svelte`) below parity.
//!
//! This guard scans the NON-TEST production source under `verter_mcp/src`
//! (`server.rs` is the bulk) and FAILS on any executable `.vue`-literal /
//! `"vue"`-language-id GATE outside a NARROW, documented allowlist of
//! Vue-INTRINSIC tools — tools whose semantics are Vue-specific by definition
//! (Provide/Inject API classification, Options→Composition migration targets,
//! Pinia/Vuex store analysis, Vue-lifecycle SSR readiness), where `is_vue()`
//! is the correct, intended classification.
//!
//! A "gate" is a CLASSIFICATION / ROUTING decision keyed on a `.vue` (or
//! `"vue"`-language-id) literal — exactly the shapes that strand `.svelte`.
//! The detector flags, in executable code:
//!
//!   - `.is_vue(` (the legacy carrier predicate),
//!   - a `.vue`-suffix CLASSIFIER: `ends_with` / `strip_suffix` /
//!     `trim_end_matches` / `starts_with` / `contains` against `".vue"`,
//!   - a `.vue` / `"vue"` EQUALITY or `matches!` gate (`== ".vue"`,
//!     `lang == "vue"`, `matches!(x, "vue")`),
//!   - a bare `"vue"` LANGUAGE-ID classifier (`contains` / `starts_with` /
//!     `ends_with` / `strip_prefix` / `strip_suffix` / `trim_*_matches`
//!     against the bare `"vue"` language-id literal),
//!   - a hardcoded carrier PROVIDER literal (`.vue.ts` / `.vue.tsx` /
//!     `.vue.jsx`), including a `format!("{src}.vue.ts")` reconstruction.
//!
//! The allowlist is FUNCTION-scoped and needle-NARROW: an `is_vue()` /
//! `.vue` gate is excused ONLY inside one of the enumerated Vue-intrinsic tool
//! functions ([`VUE_INTRINSIC_FNS`]). A carrier-neutral tool may NOT carry a
//! Vue gate, and the allowlist never whitelists a carrier-neutral function.
//! Test code (`#[cfg(test)]` + `*_tests.rs`, stripped), comments (stripped),
//! and explicit `is_svelte()` checks (a DIFFERENT carrier) are excluded.
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "Framework Adapter Substrate"
//! as `mcp_routing_has_no_hardcoded_vue_gate`. Documented in the
//! `/framework-adapters` skill.

use std::fs;
use std::path::{Path, PathBuf};

/// The Vue-INTRINSIC MCP tool functions whose semantics are Vue-specific by
/// definition, where an `is_vue()` classification is correct and intended. An
/// `is_vue()` / `.vue` gate is allowlisted ONLY when it appears textually
/// inside one of these `fn` bodies. Every OTHER tool operates on carrier-
/// neutral analysis and must be carrier-generic.
///
/// - `validate_provide_inject` — Vue `provide`/`inject` API classification.
/// - `detect_migration_targets` — Options→Composition / `defineComponent`.
/// - `get_store_usage` / `get_store_graph` / `trace_store_flow` — Pinia/Vuex
///   `store_definitions`.
/// - `ssr_project_report` — Vue-lifecycle SSR readiness.
const VUE_INTRINSIC_FNS: &[&str] = &[
    "validate_provide_inject",
    "detect_migration_targets",
    "get_store_usage",
    "get_store_graph",
    "trace_store_flow",
    "ssr_project_report",
];

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively collect `.rs` files under `dir`, skipping `*_tests.rs` and
/// `tests.rs` (test fixtures, not routing).
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".rs") {
            continue;
        }
        if name.ends_with("_tests.rs") || name == "tests.rs" {
            continue;
        }
        out.push(path);
    }
}

fn scan_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&crate_src(), &mut files);
    files.sort();
    files
}

/// Strip the comment portion of a source line (line `//` comments and the
/// trailing part of a `/* … */` that opens-and-closes on one line), being
/// careful not to treat `//` inside a string literal as a comment. Returns
/// only the executable code prefix. Block comments spanning multiple lines
/// are handled by the line iterator's `in_block_comment` state.
fn strip_comment(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            out.push(b as char);
            if b == b'\\' && i + 1 < bytes.len() {
                // Escaped char inside a string — copy verbatim.
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_str = Some(b);
            out.push(b as char);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // line comment — rest is non-executable
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            // Single-line block comment: skip to the closing `*/` if present.
            if let Some(end) = code[i + 2..].find("*/") {
                i = i + 2 + end + 2;
                continue;
            }
            break; // opens here, closes on a later line — handled by caller state
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// One flagged violation: file, 1-based line, the executable text.
type Violation = (String, usize, String);

/// Detect the function name on a `fn NAME(` / `async fn NAME(` definition line
/// (already comment-stripped), so the allowlist can be FUNCTION-scoped.
fn fn_name_on_line(code: &str) -> Option<String> {
    let trimmed = code.trim_start();
    let after = trimmed.strip_prefix("pub ").unwrap_or(trimmed).trim_start();
    let after = after.strip_prefix("async ").unwrap_or(after).trim_start();
    let rest = after.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Walk one file, tracking `#[cfg(test)]` brace depth, multi-line block
/// comments, AND the enclosing function name (for the Vue-intrinsic
/// allowlist), and flag every executable line that contains a forbidden
/// `.vue`/`"vue"` gate outside an allowlisted Vue-intrinsic function.
fn file_violations(path: &Path) -> Vec<Violation> {
    let Ok(src) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let rel = path
        .strip_prefix(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let mut violations = Vec::new();
    let mut in_block_comment = false;
    let mut pending_cfg_test = false;
    let mut cfg_test_depth: Option<i32> = None;
    let mut depth: i32 = 0;

    // Enclosing-function tracking: when a `fn NAME(` opens its body, remember
    // NAME and the brace depth its body lives at; clear it when we close back
    // out. A line is "inside" the function while `depth > fn_depth`.
    let mut current_fn: Option<(String, i32)> = None;
    let mut pending_fn: Option<String> = None;

    for (idx, raw) in src.lines().enumerate() {
        let lineno = idx + 1;
        let mut line = raw.to_string();

        // Resolve any open multi-line block comment first.
        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].to_string();
                in_block_comment = false;
            } else {
                continue; // entire line is inside a block comment
            }
        }

        let code = strip_comment(&line);
        // Detect an unterminated block comment opened on this line.
        if let Some(open) = code_opens_block_comment(&line) {
            in_block_comment = open;
        }

        let trimmed = code.trim();

        // `#[cfg(test)]` attribute → arm the skip for the next braced item.
        if trimmed.contains("#[cfg(test)]") {
            pending_cfg_test = true;
        }

        // A `fn NAME(` definition arms the enclosing-fn record for the brace
        // its body opens on (this line or a following one).
        if let Some(name) = fn_name_on_line(&code) {
            pending_fn = Some(name);
        }

        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;

        if pending_cfg_test && opens > 0 {
            cfg_test_depth = Some(depth);
            pending_cfg_test = false;
        }
        if let Some(name) = pending_fn.clone() {
            if opens > 0 {
                current_fn = Some((name, depth));
                pending_fn = None;
            }
        }

        let inside_cfg_test = cfg_test_depth.is_some();
        let in_vue_intrinsic_fn = current_fn
            .as_ref()
            .map(|(name, _)| VUE_INTRINSIC_FNS.contains(&name.as_str()))
            .unwrap_or(false);

        if !inside_cfg_test
            && !in_vue_intrinsic_fn
            && !trimmed.is_empty()
            && line_gate_kind(&code).is_some()
        {
            violations.push((rel.clone(), lineno, trimmed.to_string()));
        }

        depth += opens - closes;
        if let Some(base) = cfg_test_depth {
            if depth <= base {
                cfg_test_depth = None;
            }
        }
        if let Some((_, fn_depth)) = &current_fn {
            if depth <= *fn_depth {
                current_fn = None;
            }
        }
    }
    violations
}

/// Whether the line leaves a block comment open (a trailing `/*` with no
/// matching `*/` after it on the same line). String- and line-comment-aware:
/// a `/*` inside a string literal or after a `//` line comment does NOT open a
/// block comment.
fn code_opens_block_comment(line: &str) -> Option<bool> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    let mut open = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_str = Some(b);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // line comment — nothing after it opens a block comment
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(close_rel) = line[i + 2..].find("*/") {
                i = i + 2 + close_rel + 2;
                open = false;
                continue;
            }
            open = true;
            break;
        }
        i += 1;
    }
    Some(open)
}

/// Whether `head` (the text BEFORE a `=>`, already right-trimmed) is a
/// `match`-arm PATTERN that includes a bare `"vue"` literal, rather than an
/// arbitrary expression. A match-arm pattern over string literals is a
/// `|`-joined list of quoted literals, optionally preceded by an arm/block
/// boundary token (`{`, `,`, `;`, `}`). We take the fragment after the last such
/// boundary and require it to be composed ONLY of string literals, `|`
/// alternation separators, and whitespace, with `"vue"` as one alternative —
/// so `"vue" => {}` and `"vue" | "vuex" => …` both flag. This deliberately
/// REJECTS value-side lines like `kind => "vue".to_string()` (the head is
/// `kind`, not a `"vue"` literal pattern), `label => format!("{}.vue", …)`
/// (head `label`), and a comparison head such as `ext == "vue"` (not pure
/// pattern syntax).
fn pattern_head_is_match_arm(head: &str) -> bool {
    // Cut at the last arm/block boundary so a multi-arm line or a `match x {`
    // prefix does not pollute the pattern check.
    let frag = head
        .rsplit(['{', ';', ',', '}'])
        .next()
        .unwrap_or(head)
        .trim();
    if frag.is_empty() {
        return false;
    }
    // The fragment must be `"<lit>" ( | "<lit>" )*` — an alternation of bare
    // string literals only — AND one of those literals must be exactly `"vue"`.
    let alts: Vec<&str> = frag.split('|').map(str::trim).collect();
    let all_literals = alts.iter().all(|a| {
        a.len() >= 2 && a.starts_with('"') && a.ends_with('"') && !a[1..a.len() - 1].contains('"')
    });
    all_literals && alts.iter().any(|a| *a == "\"vue\"")
}

/// Classify a single (comment-stripped) executable code line as a Vue-only
/// routing/classification GATE, returning a human-readable gate kind for the
/// failure diagnostic, or `None` when the line is carrier-generic / not a gate.
///
/// An explicit `is_svelte()` check is a DIFFERENT carrier needle and is never
/// a gate here.
fn line_gate_kind(code: &str) -> Option<&'static str> {
    // (0) Hardcoded carrier PROVIDER literals — unambiguous provider-artifact
    //     paths regardless of surrounding context.
    for lit in [".vue.ts", ".vue.tsx", ".vue.jsx"] {
        if code.contains(lit) {
            return Some("hardcoded carrier provider literal (.vue.ts/.tsx/.jsx)");
        }
    }

    // (1) The legacy Vue carrier predicate.
    if code.contains(".is_vue(") {
        return Some(".is_vue() carrier predicate");
    }

    // (2) `.vue`-suffix CLASSIFIERS.
    for m in [
        "ends_with(\".vue\")",
        "strip_suffix(\".vue\")",
        "trim_end_matches(\".vue\")",
        "starts_with(\".vue\")",
        "contains(\".vue\")",
    ] {
        if code.contains(m) {
            return Some(".vue-suffix classification gate (ends_with/strip_suffix/contains/...)");
        }
    }

    // (3) `.vue` EQUALITY gate.
    if code.contains("== \".vue\"") || code.contains("!= \".vue\"") {
        return Some(".vue equality gate (== / !=)");
    }

    // (3b) `"vue"` MATCH-ARM carrier gate: a bare `"vue" =>` (or
    //      `"vue" | "vuex" => `) pattern arm over an extension / language-id
    //      `match` — the exact shape that stranded `.svelte` in the MCP scanner
    //      (`match ext { "vue" => {} ... _ => continue }`). None of the
    //      equality / method-call needles fire on a bare match-arm pattern, so
    //      this clause closes that blind spot. We flag only when the `"vue"`
    //      literal sits in PATTERN position: it is the last pattern token before
    //      the arm's `=>`, with only match-pattern syntax (`|`-joined string
    //      literals / whitespace) between it and the fat arrow — never an
    //      arbitrary `... => ...vue...` display line.
    if let Some(arrow) = code.find("=>") {
        let head = code[..arrow].trim_end();
        if pattern_head_is_match_arm(head) {
            return Some("\"vue\" match-arm carrier gate (\"vue\" => ...)");
        }
    }

    // (4) `"vue"`-as-language-id EQUALITY / `matches!` / classification gate.
    if code.contains("== \"vue\"") || code.contains("!= \"vue\"") {
        return Some("\"vue\" language-id equality gate (== / !=)");
    }
    if code.contains("matches!") && code.contains("\"vue\"") {
        return Some("\"vue\" language-id matches! gate");
    }
    for m in [
        "contains(\"vue\")",
        "starts_with(\"vue\")",
        "ends_with(\"vue\")",
        "strip_prefix(\"vue\")",
        "strip_suffix(\"vue\")",
        "trim_start_matches(\"vue\")",
        "trim_end_matches(\"vue\")",
        "trim_matches(\"vue\")",
    ] {
        if code.contains(m) {
            return Some("\"vue\" language-id classification gate (contains/starts_with/ends_with/strip/trim)");
        }
    }

    None
}

#[test]
fn mcp_routing_has_no_hardcoded_vue_gate() {
    let files = scan_files();
    assert!(
        !files.is_empty(),
        "guard found no production source files to scan — the scan root drifted"
    );
    let mut violations = Vec::new();
    for f in &files {
        violations.extend(file_violations(f));
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "Carrier MCP-routing guard violations: production MCP tool code re-introduced\n\
         a Vue-only gate over carrier-NEUTRAL analysis. A `.vue` SFC and a `.svelte`\n\
         component are both framework CARRIERS — route through the carrier-generic\n\
         substrate instead:\n\
           - `file_language.is_framework_carrier()` when you hold a `FileLanguage`\n\
             (e.g. `list_files()` yields `(String, FileLanguage)` tuples),\n\
           - `verter_workspace::path_is_carrier(path)` for a bare path string.\n\
         Allowlisted ONLY: the enumerated Vue-INTRINSIC tools (Provide/Inject,\n\
         migration targets, Pinia/Vuex store analysis, Vue-lifecycle SSR), test\n\
         code, comments, and explicit `is_svelte()` checks.\n\n\
         Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, ln, line)| format!("{rel}:{ln}: {line}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────
//
// These prove the guard's detector FAILS against the pre-change (`.vue`-gated)
// shapes and PASSES against the carrier-generic post-change shapes, and that
// the comment/test/string stripping + the function-scoped allowlist behave
// precisely.

/// The exact pre-change executable shapes from the carrier-NEUTRAL MCP tool
/// sites. The detector MUST flag each.
const PRE_CHANGE_VIOLATING_LINES: &[(&str, &str)] = &[
    (".filter(|(_, k)| k.is_vue())", ".is_vue("),
    (
        "\"files_checked\": files.iter().filter(|(_, k)| k.is_vue()).count(),",
        ".is_vue(",
    ),
    (
        "let vue_files: Vec<_> = files.iter().filter(|(_, k)| k.is_vue()).collect();",
        ".is_vue(",
    ),
    (
        "if !found_test && file.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
    // The MCP-scanner blind spot: a bare `"vue" =>` match-arm carrier gate.
    ("            \"vue\" => {}", "\"vue\" => match-arm"),
    (
        "        \"vue\" | \"vuex\" => keep(),",
        "\"vue\" => alternation match-arm",
    ),
];

/// The carrier-generic post-change shapes. The detector MUST NOT flag any.
const POST_CHANGE_CLEAN_LINES: &[&str] = &[
    ".filter(|(_, k)| k.is_framework_carrier())",
    "\"files_checked\": files.iter().filter(|(_, k)| k.is_framework_carrier()).count(),",
    "let component_files: Vec<_> = files.iter().filter(|(_, k)| k.is_framework_carrier()).collect();",
    "if !found_test && verter_workspace::path_is_carrier(file) {",
    // The additive carrier-neutral wire key + its deprecated alias (both carry
    // the carrier-generic count) are NOT gates.
    "\"componentFiles\": component_files.len(),",
    "\"vue_files\": component_files.len(),",
    "if file_language.is_svelte() {",
    // The carrier-generic scanner gate that REPLACES the `"vue" => {}` arm —
    // a path-based carrier check, not a match-arm literal.
    "let is_carrier = verter_workspace::path_is_carrier(&canonical);",
    "if !is_carrier && !is_script_dep {",
    // A `=>` line whose `"vue"` is on the VALUE side (display / mapping), not in
    // pattern position, must NOT be flagged by the match-arm clause.
    "label => format!(\"{}.vue\", stem),",
    "kind => \"vue\".to_string(),",
];

/// Per-line detector mirroring the file scanner's executable-line decision
/// for a single, already-comment-stripped code line (NO function-scope: these
/// lines stand alone, outside any Vue-intrinsic fn).
fn line_flags(code: &str) -> bool {
    line_gate_kind(code).is_some()
}

#[test]
fn detector_flags_every_pre_change_violation() {
    for (line, expected_needle) in PRE_CHANGE_VIOLATING_LINES {
        let code = strip_comment(line);
        assert!(
            line_flags(&code),
            "detector failed to flag pre-change violation ({expected_needle}): {line}"
        );
    }
}

#[test]
fn detector_passes_every_post_change_clean_line() {
    for line in POST_CHANGE_CLEAN_LINES {
        let code = strip_comment(line);
        assert!(
            !line_flags(&code),
            "detector wrongly flagged carrier-generic line: {line}"
        );
    }
}

#[test]
fn comments_are_not_flagged() {
    assert!(!line_flags(&strip_comment("// only .vue files count here")));
    assert!(!line_flags(&strip_comment(
        "/// Mirrors is_vue() for the carrier filter"
    )));
    assert!(!line_flags(&strip_comment(
        "let x = k.is_framework_carrier(); // was: k.is_vue()"
    )));
}

#[test]
fn match_arm_vue_gate_is_flagged_but_value_side_vue_is_not() {
    // The bare match-arm carrier gate (the MCP-scanner blind spot) IS flagged…
    assert!(line_flags(&strip_comment("            \"vue\" => {}")));
    assert!(line_flags(&strip_comment(
        "    \"vue\" => host.upsert(req),"
    )));
    assert!(line_flags(&strip_comment(
        "        \"vue\" | \"svelte\" => keep(),"
    )));
    // …but a `"vue"` literal on the VALUE side of an arm (display / mapping) is
    // NOT a gate, and neither is a plain comparison.
    assert!(!line_flags(&strip_comment("label => format!(\"{}.vue\", s),")));
    assert!(!line_flags(&strip_comment("kind => \"vue\".to_string(),")));
    assert!(!line_flags(&strip_comment(
        "let label = \"vue\"; // a value, not a gate"
    )));
    // `pattern_head_is_match_arm` rejects a comparison head outright.
    assert!(!pattern_head_is_match_arm("ext == \"vue\""));
    assert!(pattern_head_is_match_arm("\"vue\""));
    assert!(pattern_head_is_match_arm("\"vue\" | \"vuex\""));
}

#[test]
fn vue_intrinsic_fn_allowlist_is_function_scoped() {
    // An `is_vue()` gate INSIDE an allowlisted Vue-intrinsic fn is NOT flagged;
    // the SAME gate inside a carrier-neutral fn IS flagged. This proves the
    // allowlist is function-scoped (not whole-file) and discriminating.
    let src = "\
async fn get_store_usage(&self) -> Result<X, Y> {
    for (_, kind) in files {
        if !kind.is_vue() {
            continue;
        }
    }
}
async fn detect_css_bleed(&self) -> Result<X, Y> {
    for (_, kind) in files {
        if !kind.is_vue() {
            continue;
        }
    }
}
";
    let tmp = std::env::temp_dir().join("mcp_guard_fn_scope_selftest.rs");
    fs::write(&tmp, src).unwrap();
    let v = file_violations(&tmp);
    let _ = fs::remove_file(&tmp);
    // Exactly ONE violation: the `is_vue()` inside the carrier-neutral
    // `detect_css_bleed`, NOT the one inside the allowlisted `get_store_usage`.
    assert_eq!(
        v.len(),
        1,
        "expected exactly the carrier-neutral fn's gate to flag; got {v:?}"
    );
    assert!(
        v[0].2.contains("is_vue"),
        "the flagged line must be the is_vue gate; got {:?}",
        v[0].2
    );
    // The flagged line is INSIDE detect_css_bleed (line 10 in the fixture),
    // never inside get_store_usage (line 3).
    assert_eq!(v[0].1, 10, "the flagged gate must be in detect_css_bleed");
}

#[test]
fn fn_name_detector_parses_definition_forms() {
    assert_eq!(
        fn_name_on_line("    async fn get_store_usage(&self) -> Result<X, Y> {"),
        Some("get_store_usage".to_string())
    );
    assert_eq!(
        fn_name_on_line("    fn build_route_snapshot("),
        Some("build_route_snapshot".to_string())
    );
    assert_eq!(
        fn_name_on_line("pub async fn detect_css_bleed("),
        Some("detect_css_bleed".to_string())
    );
    assert_eq!(fn_name_on_line("    let x = foo();"), None);
    // `fn` as a substring of an identifier must NOT parse as a definition.
    assert_eq!(fn_name_on_line("    transform_data();"), None);
}

#[test]
fn broadened_gate_forms_flag() {
    assert!(line_flags(&strip_comment("if lang == \"vue\" {")));
    assert!(line_flags(&strip_comment(
        "if file.language_id != \"vue\" {"
    )));
    assert!(line_flags(&strip_comment(
        "if matches!(language_id.as_str(), \"vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "if canonical.contains(\".vue\") {"
    )));
    assert!(line_flags(&strip_comment("if ext == \".vue\" {")));
    assert!(line_flags(&strip_comment(
        "if name.starts_with(\".vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "let s = id.trim_end_matches(\".vue\");"
    )));
    assert!(line_flags(&strip_comment(
        "let p = format!(\"{src}.vue.ts\");"
    )));
}

#[test]
fn vue_npm_specifier_literal_is_not_a_gate() {
    // The bare `"vue"` runtime npm specifier in a NON-equality position is NOT
    // a language-id gate.
    assert!(!line_flags(&strip_comment("source: \"vue\".into(),")));
    assert!(!line_flags(&strip_comment(
        "callee_import_source: Some(\"vue\".to_string()),"
    )));
    // A field/accessor named `vue_api_calls` (no string literal) must NOT flag.
    assert!(!line_flags(&strip_comment(
        "for call in analysis.vue_api_calls.iter() {"
    )));
}

#[test]
fn is_svelte_check_is_not_flagged() {
    assert!(!line_flags(&strip_comment(
        "if file_language.is_svelte() {"
    )));
}

#[test]
fn block_comment_open_detector_is_string_aware() {
    assert_eq!(
        code_opens_block_comment("let s = \"a /* not a comment\";"),
        Some(false)
    );
    assert_eq!(code_opens_block_comment("let s = \"/*\";"), Some(false));
    assert_eq!(
        code_opens_block_comment("let x = 1; // trailing /* not open"),
        Some(false)
    );
    assert_eq!(
        code_opens_block_comment("let x = 1; /* opens here"),
        Some(true)
    );
    assert_eq!(
        code_opens_block_comment("let x = 1; /* closed */ let y = 2;"),
        Some(false)
    );
}
