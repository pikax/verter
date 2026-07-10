//! Architecture guard: the Svelte CSS selector-to-template matcher tree
//! (`crates/verter_compiler/src/svelte/runtime/css/match.rs`,
//! `match_index.rs`, `match_values.rs`) drives every semantic decision from
//! the OWNED typed projections lowered during the single template-expression
//! parse (`AnalyzedExpr::matcher_expr` / `AnalyzedExpr::render_callee`) — it
//! NEVER re-parses expression source.
//!
//! The Typed-IR-Only / no-format-then-reparse rule facet this pins: the CSS
//! matcher used to synthesize `({text});` wrappers and re-run `oxc_parser`
//! over `analyzed.source` at query time (`parse_wrapped_expression`,
//! `parse_render_call(analyzed.source)`) — a second query-time parse of
//! source the expression analysis had ALREADY parsed once. The guard fails on
//! any reintroduction: an `oxc_parser` / `oxc_allocator` import, a
//! `Parser::new` / `Allocator::` construction, the `format!("(…")`
//! synth-wrap formatting, a semantic `analyzed.source` read, or the retired
//! `parse_render_call` reparse entry.
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "Typed-IR-Only Resolver Rule"
//! (`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`).

mod svelte_guard_support;

use std::fs;
use std::path::PathBuf;

use svelte_guard_support::strip_rust_comments;

/// The CSS matcher tree's production files (relative to this crate's root) —
/// the files that must read the typed projection, never parse source.
const MATCHER_TREE: &[&str] = &[
    "src/svelte/runtime/css/match.rs",
    "src/svelte/runtime/css/match_index.rs",
    "src/svelte/runtime/css/match_values.rs",
];

/// The banned tokens (checked on comment-stripped code): parser / allocator
/// imports and constructions, the synth-wrap parse formatting, the semantic
/// raw-source read, and the retired reparse entry-point.
const BANNED: &[&str] = &[
    "oxc_parser",
    "oxc_allocator",
    "Parser::new",
    "Allocator::",
    "format!(\"(",
    "analyzed.source",
    "parse_render_call",
];

/// The POSITIVE signals: the matcher tree must actually read the typed
/// projections (scan non-vacuity — an empty or drifted scan set cannot pass).
const REQUIRED_TYPED_READS: &[&str] = &["matcher_expr", "render_callee"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The verdict predicate (shared with the discrimination self-tests): the
/// banned tokens found in this code, after stripping comments through the
/// SHARED string-aware scanner (`svelte_guard_support::strip_rust_comments`
/// — string/char-literal content preserved, so a `//` or `/*` inside a
/// literal can never hide a same-line banned token).
fn banned_tokens_in(code: &str) -> Vec<&'static str> {
    let stripped = strip_rust_comments(code);
    BANNED
        .iter()
        .copied()
        .filter(|token| stripped.contains(token))
        .collect()
}

#[test]
fn css_matcher_walks_the_typed_projection_and_never_reparses_source() {
    let root = crate_root();
    let mut violations: Vec<String> = Vec::new();
    let mut typed_reads_seen: Vec<&'static str> = Vec::new();
    for rel in MATCHER_TREE {
        let path = root.join(rel);
        let code = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the matcher tree file {rel} must exist and read: {e}"));
        assert!(
            !code.trim().is_empty(),
            "the matcher tree file {rel} is empty — the scan set drifted"
        );
        for token in banned_tokens_in(&code) {
            violations.push(format!("{rel}: `{token}`"));
        }
        let stripped = strip_rust_comments(&code);
        for read in REQUIRED_TYPED_READS {
            if stripped.contains(read) && !typed_reads_seen.contains(read) {
                typed_reads_seen.push(read);
            }
        }
    }
    // Non-vacuity: the matcher tree must actually READ the typed projections
    // (`analyzed.matcher_expr` / `analyzed.render_callee`) — otherwise the
    // matcher's structural authority drifted somewhere this guard no longer
    // watches.
    assert_eq!(
        typed_reads_seen.len(),
        REQUIRED_TYPED_READS.len(),
        "the matcher tree must read every typed projection {REQUIRED_TYPED_READS:?}; \
         saw only {typed_reads_seen:?} — the scan set (or the matcher wiring) drifted"
    );
    assert!(
        violations.is_empty(),
        "the CSS selector-to-template matcher must walk the owned typed projection \
         (`AnalyzedExpr::matcher_expr` / `AnalyzedExpr::render_callee`) lowered during \
         the single template-expression parse — never re-parse expression source \
         (no `oxc_parser` / `Allocator`, no `format!(\"(…\")` synth-wrap, no semantic \
         `analyzed.source` read). Violations:\n  {}",
        violations.join("\n  ")
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────────

#[test]
fn detector_flags_reparse_and_raw_source_reads() {
    // A parser import.
    assert_eq!(
        banned_tokens_in("use oxc_parser::Parser;\nfn ok() {}"),
        ["oxc_parser"]
    );
    // An allocator import + construction.
    assert_eq!(
        banned_tokens_in(
            "use oxc_allocator::Allocator;\nfn f() { let alloc = Allocator::default(); }"
        ),
        ["oxc_allocator", "Allocator::"]
    );
    // A parse construction.
    assert_eq!(
        banned_tokens_in("fn f(a: &A, s: &str) { let p = Parser::new(a, s, t()).parse(); }"),
        ["Parser::new"]
    );
    // The synth-wrap formatting (string CONTENT is kept by the strip).
    assert_eq!(
        banned_tokens_in("fn f(text: &str) -> String { format!(\"({text});\") }"),
        ["format!(\"("]
    );
    // A semantic raw-source read.
    assert_eq!(
        banned_tokens_in("fn f() { let shape = expression_attr_shape(analyzed.source); }"),
        ["analyzed.source"]
    );
    // The retired reparse entry-point.
    assert_eq!(
        banned_tokens_in("fn f() { let shape = parse_render_call(text); }"),
        ["parse_render_call"]
    );
}

#[test]
fn detector_ignores_comment_mentions() {
    assert!(banned_tokens_in("// the old walk used oxc_parser here\nfn ok() {}").is_empty());
    assert!(banned_tokens_in(
        "/// Never `Parser::new` / `Allocator::default()` again.\nfn ok() {}"
    )
    .is_empty());
    assert!(
        banned_tokens_in("/* format!(\"({text});\") was the synth-wrap; analyzed.source fed parse_render_call */ fn ok() {}")
            .is_empty()
    );
}

#[test]
fn detector_sees_through_comment_lookalikes_inside_strings() {
    // A `//` INSIDE a string is NOT a comment start: a naive stripper would
    // swallow the rest of the line and HIDE the same-line banned token.
    assert_eq!(
        banned_tokens_in(r#"fn f() { let url = "http://x"; let p = Parser::new(a, url, t()); }"#),
        ["Parser::new"]
    );
    // A `/*` inside a RAW string is not a block-comment start either.
    assert_eq!(
        banned_tokens_in(r##"fn f() { let s = r"a/*b"; let x = analyzed.source; }"##),
        ["analyzed.source"]
    );
    // A `'/'` char literal must not open anything that hides a later token.
    assert_eq!(
        banned_tokens_in("fn f() { let c = '/'; let x = parse_render_call(s); }"),
        ["parse_render_call"]
    );
    // A genuine comment AFTER a string still strips — prose stays clean.
    assert!(banned_tokens_in(
        r#"fn f() { let url = "http://x"; } // Parser::new mentioned in prose"#
    )
    .is_empty());
}

#[test]
fn scan_set_is_exactly_the_matcher_tree() {
    // The three matcher production files exist under this crate — a rename /
    // move must update the guard's scan set in the same change.
    let root = crate_root();
    for rel in MATCHER_TREE {
        assert!(
            root.join(rel).is_file(),
            "matcher tree file {rel} not found — update MATCHER_TREE alongside the move"
        );
    }
}
