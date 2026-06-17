//! Static architecture guard: the `verter_session` resolution / routing
//! surface must classify framework-component files carrier-generically.
//!
//! Verter is one shared, carrier-generic substrate. A `.vue` SFC and a
//! `.svelte` component are both framework CARRIERS (`FileLanguage::Framework`,
//! `is_framework_carrier()`). The resolution / routing code that decides
//! whether a resolved child is a component carrier — fallthrough / root-attr
//! inheritance, export-graph reachability, child-resolution branching — must
//! gate on the carrier-generic predicate, never on a Vue-only `.is_vue()`
//! call or a `.vue`-suffix path classifier. A carrier-neutral `.vue` gate
//! forces every OTHER carrier (`.svelte`) into the unresolved branch and
//! strands it below parity (the `fallthrough.rs` child-resolution gate was
//! exactly this bug).
//!
//! ## Scan scope (deliberately TIGHT)
//!
//! The scan is scoped to `verter_session/src/resolver_core/` ONLY — the
//! resolution / routing surface where a carrier-neutral `.vue` / `is_vue()`
//! gate is a genuine cross-carrier PARITY bug. The rest of `verter_session/src`
//! is intentionally OUT of scope: there `.vue` / `is_vue()` is legitimately
//! dense (Vue api-projector / synth / adapter / descriptor identity,
//! `host_compile_audit.rs`'s Svelte fail-closed gate, the `vue_exec` relocated
//! Vue delegates, the legacy parse metric, the per-carrier compiler bridge),
//! and a whole-`src` scan would drown the parity signal in Vue-intrinsic
//! noise. The minimal scope that durably catches the bug class is the
//! resolution/routing tree where `fallthrough.rs` lives.
//!
//! ## Allowlist (needle-NARROW, Vue-intrinsic only)
//!
//! Within the scan scope, the Vue-MACRO resolution helper
//! `component_meta/direct_macro.rs` is allowlisted as a FILE: its gates are
//! genuinely Vue-runtime / Vue-macro intrinsic — `dep.import_source == "vue"`
//! detects the Vue runtime npm package's own `Slot` type, and
//! `keep_direct_imported_vue_macro`'s `.ends_with(".vue")` carves out the
//! `defineProps<ImportedVueProps>()` Vue-component-surface case. Neither is a
//! carrier classification of an arbitrary component file. Test code
//! (`#[cfg(test)]` blocks + `*_tests.rs` files, stripped), comments (stripped),
//! and explicit `is_svelte()` checks (a DIFFERENT carrier) are excluded. The
//! allowlist NEVER excuses a carrier-neutral gate; any new carrier-neutral
//! `.vue` / `is_vue()` gate in scope is a violation, not an allowlist entry.
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "Framework Adapter Substrate"
//! as `session_resolution_routing_has_no_hardcoded_vue_gate`. Documented in
//! the `/framework-adapters` skill.

use std::fs;
use std::path::{Path, PathBuf};

/// The single scan root (relative to the crate `src/`): the resolution /
/// routing tree where a carrier-neutral `.vue` / `is_vue()` gate is a genuine
/// parity bug.
const SCAN_DIR: &str = "resolver_core";

/// Files excluded from the scan because their gates are genuinely Vue-runtime
/// / Vue-macro intrinsic (NOT carrier classification of an arbitrary component
/// file). `direct_macro.rs` resolves Vue `defineProps`/`defineSlots` macro
/// types: it detects the Vue runtime package's own `Slot` type
/// (`import_source == "vue"`) and carves out the imported-Vue-component-surface
/// case — both Vue-specific by definition, neither a cross-carrier gate.
const FILE_EXCLUSIONS: &[&str] = &["direct_macro.rs"];

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively collect `.rs` files under `dir`, skipping `*_tests.rs` and
/// `tests.rs` (test fixtures, not routing) and the Vue-intrinsic exclusions.
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
        if FILE_EXCLUSIONS.contains(&name) {
            continue;
        }
        out.push(path);
    }
}

fn scan_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&crate_src().join(SCAN_DIR), &mut files);
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

/// Walk one file, tracking `#[cfg(test)]` brace depth and multi-line block
/// comments, and flag every executable line that contains a forbidden
/// `.vue`/`"vue"` gate.
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

        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;

        if pending_cfg_test && opens > 0 {
            cfg_test_depth = Some(depth);
            pending_cfg_test = false;
        }

        let inside_cfg_test = cfg_test_depth.is_some();

        if !inside_cfg_test && !trimmed.is_empty() && line_gate_kind(&code).is_some() {
            violations.push((rel.clone(), lineno, trimmed.to_string()));
        }

        depth += opens - closes;
        if let Some(base) = cfg_test_depth {
            if depth <= base {
                cfg_test_depth = None;
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

/// Classify a single (comment-stripped) executable code line as a Vue-only
/// resolution/classification GATE, returning a human-readable gate kind for
/// the failure diagnostic, or `None` when the line is carrier-generic / not a
/// gate.
///
/// An explicit `is_svelte()` check is a DIFFERENT carrier needle and is never
/// a gate here.
fn line_gate_kind(code: &str) -> Option<&'static str> {
    // (0) Hardcoded carrier PROVIDER literals.
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
fn session_resolution_routing_has_no_hardcoded_vue_gate() {
    let files = scan_files();
    assert!(
        !files.is_empty(),
        "guard found no resolver_core files to scan — the scan root drifted"
    );
    let mut violations = Vec::new();
    for f in &files {
        violations.extend(file_violations(f));
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "Carrier session-resolution-routing guard violations: resolution/routing code\n\
         under `resolver_core/` re-introduced a Vue-only gate over a carrier file.\n\
         A `.vue` SFC and a `.svelte` component are both framework CARRIERS — gate on\n\
         the carrier-generic predicate instead:\n\
           - `file_language.is_framework_carrier()` when you hold a `FileLanguage`,\n\
           - `verter_language::LanguageRegistry::global().classify_static(path)\n\
                .static_resolution().is_framework_carrier()` for a path string,\n\
           - `verter_workspace::path_is_carrier(path)` for a bare path helper.\n\
         Allowlisted ONLY: the Vue-MACRO resolution helper `direct_macro.rs`\n\
         (Vue-runtime/Vue-macro intrinsic), test code, comments, and explicit\n\
         `is_svelte()` checks.\n\n\
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
// shape and PASSES against the carrier-generic post-change shape, and that the
// comment/test/string stripping + the `direct_macro.rs` exclusion behave
// precisely.

/// The exact pre-change executable shapes from the carrier-NEUTRAL resolution
/// sites. The detector MUST flag each.
const PRE_CHANGE_VIOLATING_LINES: &[(&str, &str)] = &[
    // fallthrough.rs:289 — the child-resolution carrier gate (the A2 bug).
    ("if !child_id.ends_with(\".vue\") {", "ends_with(\".vue\")"),
    ("if !kind.is_vue() {", ".is_vue("),
    (
        "if canonical_id.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
];

/// The carrier-generic post-change shapes. The detector MUST NOT flag any.
const POST_CHANGE_CLEAN_LINES: &[&str] = &[
    "if !verter_language::LanguageRegistry::global().classify_static(&child_id).static_resolution().is_framework_carrier() {",
    "if !kind.is_framework_carrier() {",
    "if verter_workspace::path_is_carrier(&canonical_id) {",
    "if file_language.is_svelte() {",
];

/// Per-line detector mirroring the file scanner's executable-line decision
/// for a single, already-comment-stripped code line.
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
    assert!(!line_flags(&strip_comment(
        "// only .vue children resolve here"
    )));
    assert!(!line_flags(&strip_comment(
        "/// Mirrors the is_vue() short-circuit for the carrier"
    )));
    assert!(!line_flags(&strip_comment(
        "let c = lang.is_framework_carrier(); // was: lang.is_vue()"
    )));
}

#[test]
fn cfg_test_blocks_are_skipped() {
    // A `.vue` gate INSIDE a `#[cfg(test)]` mod must NOT flag (test fixtures
    // legitimately name `.vue` paths); the SAME gate in production code DOES.
    let src = "\
fn production() {
    if id.ends_with(\".vue\") {
        do_thing();
    }
}
#[cfg(test)]
mod tests {
    fn t() {
        if id.ends_with(\".vue\") {
            do_thing();
        }
    }
}
";
    let tmp = std::env::temp_dir().join("session_guard_cfgtest_selftest.rs");
    fs::write(&tmp, src).unwrap();
    let v = file_violations(&tmp);
    let _ = fs::remove_file(&tmp);
    // Exactly ONE violation: the production gate (line 2), NOT the test gate.
    assert_eq!(
        v.len(),
        1,
        "expected only the production gate to flag; got {v:?}"
    );
    assert_eq!(v[0].1, 2, "the flagged gate must be the production one");
}

#[test]
fn direct_macro_file_is_excluded_from_scan() {
    // The `direct_macro.rs` Vue-macro helper is allowlisted as a file: its
    // genuinely Vue-runtime/Vue-macro gates must never appear in the scan set.
    let scanned = scan_files();
    assert!(
        !scanned.is_empty(),
        "scan must find resolver_core production files"
    );
    assert!(
        scanned
            .iter()
            .all(|p| p.file_name().and_then(|n| n.to_str()) != Some("direct_macro.rs")),
        "direct_macro.rs (Vue-macro intrinsic) must be excluded from the scan"
    );
}

#[test]
fn broadened_gate_forms_flag() {
    assert!(line_flags(&strip_comment("if lang == \"vue\" {")));
    assert!(line_flags(&strip_comment("if x.language_id != \"vue\" {")));
    assert!(line_flags(&strip_comment(
        "if matches!(language_id.as_str(), \"vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "if canonical_id.contains(\".vue\") {"
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
    // a language-id gate (it is the import source / package name).
    assert!(!line_flags(&strip_comment("source: \"vue\".into(),")));
    assert!(!line_flags(&strip_comment(
        "import_source: Some(\"vue\".to_string()),"
    )));
    // A `.vue` extension in a non-gate position (a `vec!` of extensions) is NOT
    // a gate either — only classifier shapes flag.
    assert!(!line_flags(&strip_comment(
        "extensions: vec![\".ts\".into(), \".vue\".into()],"
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
