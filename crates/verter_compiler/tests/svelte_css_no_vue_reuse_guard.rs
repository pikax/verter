//! Architecture guard: the Svelte compiler tree (`src/svelte/`) must NOT
//! reuse the Vue style pipeline.
//!
//! Svelte CSS scoping is a Svelte-OWNED, span-bearing substrate
//! (`src/svelte/runtime/css/`) — a faithful port of the official
//! `svelte@5.56.3` scoping semantics operating on byte spans of the original
//! component source. The Vue pipeline (`crate::css` — `process_style` /
//! `apply_scoped_normalized` / `normalize_css` over lightningcss) implements
//! DIFFERENT scoping semantics and exposes NO byte spans (lightningcss
//! reports only per-rule line/UTF-16-column start locations), so routing any
//! Svelte style handling through it would both diverge semantically and
//! destroy the source-position mapping the Svelte render path edits through.
//!
//! The guard scans every production `.rs` file under `src/svelte/`
//! (comments stripped through the SHARED string-aware scanner —
//! [`svelte_guard_support::strip_rust_comments`]) and FAILS on any reference
//! to the Vue style pipeline: `process_style`, `apply_scoped_normalized`,
//! `css::scoped`, `css::modules`, `normalize_css`, or `lightningcss`.
//!
//! A DISCRIMINATION self-test proves the verdict predicate fails on each
//! banned pattern via inline-string fixtures — never by editing production
//! source.

mod svelte_guard_support;

use std::fs;
use std::path::{Path, PathBuf};

use svelte_guard_support::strip_rust_comments;

/// The Vue-style-pipeline tokens forbidden anywhere in the Svelte tree.
const FORBIDDEN_TOKENS: &[&str] = &[
    "process_style",
    "apply_scoped_normalized",
    "css::scoped",
    "css::modules",
    "normalize_css",
    "lightningcss",
];

/// The Svelte compiler source tree.
fn svelte_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte")
}

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The verdict predicate (shared by the guard + its discrimination
/// self-test): the forbidden Vue-pipeline tokens found in `code` (comments
/// stripped through the shared string-aware scanner, so a `//` or `/*`
/// inside a string literal can never hide a same-line banned call).
fn vue_style_pipeline_tokens(code: &str) -> Vec<&'static str> {
    let stripped = strip_rust_comments(code);
    FORBIDDEN_TOKENS
        .iter()
        .copied()
        .filter(|token| stripped.contains(token))
        .collect()
}

#[test]
fn svelte_tree_never_references_the_vue_style_pipeline() {
    let mut files = Vec::new();
    collect_rs(&svelte_dir(), &mut files);
    assert!(
        !files.is_empty(),
        "the svelte tree scan found no source files — the guard scanned nothing"
    );

    let mut violations = Vec::new();
    for path in &files {
        let code =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for token in vue_style_pipeline_tokens(&code) {
            violations.push(format!("{} references `{token}`", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "the Svelte tree must not route through the Vue style pipeline \
         (Svelte css scoping is the Svelte-owned span-bearing substrate in \
         src/svelte/runtime/css/):\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_discriminates_each_banned_pattern() {
    // The predicate FAILS on every banned token (inline fixtures — the
    // production tree is never edited to prove discrimination).
    let banned_fixtures = [
        (
            "process_style",
            "fn compile(css: &str) { crate::css::process_style(css, true); }",
        ),
        (
            "apply_scoped_normalized",
            "let out = apply_scoped_normalized(body, &scope);",
        ),
        ("css::scoped", "use crate::css::scoped::ScopedRewriter;"),
        ("css::modules", "use crate::css::modules::CssModuleMap;"),
        (
            "normalize_css",
            "let normalized = normalize_css(&style.content);",
        ),
        (
            "lightningcss",
            "use lightningcss::stylesheet::StyleSheet as LcStyleSheet;",
        ),
    ];
    for (token, fixture) in banned_fixtures {
        assert_eq!(
            vue_style_pipeline_tokens(fixture),
            vec![token],
            "the guard must fail on `{token}`"
        );
    }

    // A banned token in a COMMENT does not trip the guard…
    assert!(
        vue_style_pipeline_tokens("// the Vue pipeline calls process_style\nfn ok() {}").is_empty(),
        "a comment mention must not trip the guard"
    );
    // …and clean Svelte css code passes (the Svelte-owned module path is not
    // the Vue pipeline).
    assert!(
        vue_style_pipeline_tokens(
            "use super::css::parse::parse_style_body;\nlet plan = css::build_style_scope_plan(source, span, None, mode);"
        )
        .is_empty(),
        "the Svelte-owned css module path must pass"
    );
}

#[test]
fn guard_sees_through_comment_lookalikes_inside_string_literals() {
    // The bypass class a NON-string-aware stripper is blind to: a `//`
    // INSIDE a string literal is NOT a comment start — a crude stripper
    // swallows the rest of the line and HIDES the same-line banned Vue
    // pipeline call (a real, avoidable guard false negative).
    assert_eq!(
        vue_style_pipeline_tokens(
            r#"fn f() { let url = "http://x"; crate::css::process_style(url, true); }"#
        ),
        vec!["process_style"],
        "a banned call after a string-embedded `//` must stay VISIBLE"
    );
    // A `/*` inside a RAW string must not eat to a distant `*/` (or EOF).
    assert_eq!(
        vue_style_pipeline_tokens(r##"fn f() { let s = r"a/*b"; let out = normalize_css(s); }"##),
        vec!["normalize_css"],
        "a banned call after a raw-string-embedded `/*` must stay VISIBLE"
    );
    // The genuine-comment direction still strips: prose mentions stay clean.
    assert!(
        vue_style_pipeline_tokens("// crate::css::process_style\nfn ok() {}").is_empty(),
        "a genuine `// crate::css::process_style` comment must stay ignored"
    );
    assert!(
        vue_style_pipeline_tokens(
            r#"fn f() { let url = "http://x"; } // process_style mentioned in prose"#
        )
        .is_empty(),
        "a genuine comment after a string on the same line must still strip"
    );
}
