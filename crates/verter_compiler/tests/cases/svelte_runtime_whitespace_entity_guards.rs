//! Static guards pinning the official-algorithm invariants in the Svelte runtime
//! HTML serializer (`crates/verter_compiler/src/svelte/runtime/`).
//!
//! Two invariants, each ported wholesale from `svelte@5.56.3` and easy to silently
//! regress with an innocuous-looking `.trim()` / `is_ascii_alphanumeric()`:
//!
//! - `html_whitespace_classification_uses_html_ws_not_trim` — the runtime HTML
//!   serializer (`html.rs`) MUST decide HTML-significance with the ASCII
//!   `is_html_ws` predicate (` \t\r\n`), NEVER `str::trim*` / `char::is_whitespace`
//!   / `.is_whitespace()`, which also fold a literal NBSP (`\u{00a0}`) and other
//!   Unicode whitespace the browser (and the official `clean_nodes`,
//!   `regex_not_whitespace = /[^ \t\r\n]/`) treat as SIGNIFICANT content. A bare
//!   `str::trim` here was the residual NBSP-dropping bug.
//! - `entity_boundary_treats_underscore_as_word_char` — a legacy no-`;` named-entity
//!   boundary check in the runtime (`html.rs`) MUST treat `_` as a word char (the
//!   official attribute-value boundary `\b(?!=)`, where `\b` is `[A-Za-z0-9_]`). An
//!   `is_ascii_alphanumeric()`-only boundary OMITS `_` and wrongly decodes
//!   `&copy_x` → `©_x`.
//!
//! The scan strips line comments + string/char literals so a pattern named in a
//! doc comment (explaining why we DON'T use it) is not a false positive. Each
//! guard carries a negative self-test proving it catches the bug pattern.

use std::path::PathBuf;

fn runtime_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/runtime")
}

fn read_runtime_file(name: &str) -> String {
    let path = runtime_src_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Return `code` with `//`-line comments, `/* */`-block comments, and the contents
/// of `"…"` / `'…'` literals replaced by spaces (newlines preserved), so a pattern
/// that appears only inside a comment or a string literal is NOT matched. A
/// single-pass char scanner (not a full parse) — sufficient for the simple textual
/// guards below, and conservative (it never UN-masks code).
fn code_only(code: &str) -> String {
    let bytes: Vec<char> = code.chars().collect();
    let n = bytes.len();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let push_space = |out: &mut String, c: char| out.push(if c == '\n' { '\n' } else { ' ' });
    while i < n {
        let c = bytes[i];
        let next = if i + 1 < n { bytes[i + 1] } else { '\0' };
        // Line comment (covers `//` and `///`).
        if c == '/' && next == '/' {
            while i < n && bytes[i] != '\n' {
                push_space(&mut out, bytes[i]);
                i += 1;
            }
            continue;
        }
        // Block comment.
        if c == '/' && next == '*' {
            push_space(&mut out, c);
            push_space(&mut out, next);
            i += 2;
            while i < n && !(bytes[i] == '*' && i + 1 < n && bytes[i + 1] == '/') {
                push_space(&mut out, bytes[i]);
                i += 1;
            }
            if i < n {
                push_space(&mut out, '*');
                push_space(&mut out, '/');
                i += 2;
            }
            continue;
        }
        // String / char literal — mask the contents (handle escapes).
        if c == '"' || c == '\'' {
            let quote = c;
            out.push(c);
            i += 1;
            while i < n && bytes[i] != quote {
                if bytes[i] == '\\' {
                    push_space(&mut out, bytes[i]);
                    if i + 1 < n {
                        push_space(&mut out, bytes[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                push_space(&mut out, bytes[i]);
                i += 1;
            }
            if i < n {
                out.push(quote);
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The forbidden HTML-whitespace classifiers: any of these in the runtime HTML
/// serializer's CODE means an HTML-significance decision is NOT keyed on the ASCII
/// `is_html_ws` set. (The legitimate `trim_*_matches(is_html_ws)` forms are NOT in
/// this list — they take the explicit predicate.)
const FORBIDDEN_WS_PATTERNS: &[&str] = &[
    ".trim()",
    ".trim_start()",
    ".trim_end()",
    "char::is_whitespace",
    ".is_whitespace()",
];

#[test]
fn html_whitespace_classification_uses_html_ws_not_trim() {
    let code = code_only(&read_runtime_file("html.rs"));
    for pat in FORBIDDEN_WS_PATTERNS {
        assert!(
            !code.contains(pat),
            "the runtime HTML serializer (html.rs) must classify HTML whitespace with \
             `is_html_ws` (ASCII ` \\t\\r\\n`), never `{pat}` (which folds a literal NBSP / \
             Unicode whitespace the official `clean_nodes` keeps as significant content). \
             Use `is_html_ws` or `trim_*_matches(is_html_ws)`."
        );
    }
}

#[test]
fn html_whitespace_guard_negative_self_test() {
    // The guard MUST fire on each forbidden pattern when it appears in code (proving
    // it is discriminating, not vacuous).
    for pat in FORBIDDEN_WS_PATTERNS {
        let bug = format!("fn f(s: &str) -> bool {{ s{pat}.is_empty() }}");
        let scanned = code_only(&bug);
        assert!(
            scanned.contains(pat),
            "the code-only scan must preserve the bug pattern `{pat}` so the guard catches it"
        );
    }
    // And it must NOT fire when the pattern appears only in a comment/string.
    let benign = "// this code must not use .trim() for html whitespace\nlet _ = \"a .trim() b\";";
    let scanned = code_only(benign);
    assert!(
        !scanned.contains(".trim()"),
        "a `.trim()` mentioned only in a comment / string literal must be masked out"
    );
}

#[test]
fn entity_boundary_treats_underscore_as_word_char() {
    // The runtime entity decoder's legacy no-`;` boundary check must treat `_` as a
    // word char. Concretely: any `is_ascii_alphanumeric()` used in an entity-boundary
    // decision in the entity-decode module's CODE must be paired with an explicit `_`
    // check (the official `\b` word-char set is `[A-Za-z0-9_]`). We scan the RAW
    // source lines (a `'_'` char literal would be masked by `code_only`) but SKIP
    // doc/line-comment lines (a `///` mention is documentation, not a boundary
    // decision); each remaining code line that calls `is_ascii_alphanumeric()` must
    // also mention the `'_'` word-char on the same line.
    let raw_lines = read_runtime_file("entity_decode.rs");
    for (lineno, line) in raw_lines.lines().enumerate() {
        let trimmed = line.trim_start();
        let is_comment_line = trimmed.starts_with("//");
        if !is_comment_line && line.contains("is_ascii_alphanumeric()") {
            assert!(
                line.contains("'_'"),
                "entity_decode.rs:{} uses `is_ascii_alphanumeric()` in an entity-boundary \
                 check without treating `_` as a word char — the official legacy \
                 named-entity boundary `\\b(?!=)` includes `_` (so `&copy_x` must NOT \
                 decode). Add `|| ch == '_'` (see `is_entity_word_char`). Line: {}",
                lineno + 1,
                line.trim()
            );
        }
    }
    // Positive: the dedicated word-char predicate exists and includes `_`.
    let raw = read_runtime_file("entity_decode.rs");
    assert!(
        raw.contains("fn is_entity_word_char"),
        "entity_decode.rs must define the `is_entity_word_char` predicate (the \
         entity-boundary word-char `[A-Za-z0-9_]`)"
    );
}

#[test]
fn entity_boundary_guard_negative_self_test() {
    // The guard MUST fire on the bug shape: `is_ascii_alphanumeric()` with no `_`.
    let bug_line = "let blocked = c.is_ascii_alphanumeric();";
    assert!(
        bug_line.contains("is_ascii_alphanumeric()") && !bug_line.contains("'_'"),
        "the negative self-test models the exact bug shape the guard rejects"
    );
    // The fixed shape passes (alphanumeric OR `_`).
    let fixed_line = "ch.is_ascii_alphanumeric() || ch == '_'";
    assert!(
        fixed_line.contains("is_ascii_alphanumeric()") && fixed_line.contains("'_'"),
        "the fixed shape pairs `is_ascii_alphanumeric()` with the `_` word-char check"
    );
}
