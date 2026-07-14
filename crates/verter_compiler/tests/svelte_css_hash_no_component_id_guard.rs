//! Architecture guard: the Svelte CSS scope-hash derivation must NEVER read
//! `component_id`.
//!
//! The Svelte scope class is derived from exactly two inputs: the RESOLVED
//! `cssHash` override (the user callback's byte-exact result, computed OUTSIDE
//! the compiler) OR the official default djb2 rule over the `filename` /
//! raw-css text. `component_id` is the Vue EXPLICIT component/scope id — a
//! DIFFERENT semantic surface. Overloading it for the Svelte cssHash override
//! would silently conflate two unrelated scope-id concepts, so the Svelte CSS
//! scoping substrate (`src/svelte/runtime/css/`) must never reference it.
//!
//! The guard scans every production `.rs` under `src/svelte/runtime/css/`
//! (comments stripped through the shared string-aware scanner) and FAILS on any
//! `component_id` reference. A discrimination self-test proves the predicate
//! fails on the banned token via inline fixtures — the production tree is never
//! edited to prove discrimination.

mod svelte_guard_support;

use std::fs;
use std::path::{Path, PathBuf};

use svelte_guard_support::strip_rust_comments;

/// The banned Vue-scope-id token in the Svelte CSS scoping substrate.
const BANNED_TOKEN: &str = "component_id";

/// The Svelte CSS scoping substrate.
fn svelte_css_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/runtime/css")
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

/// The verdict predicate (shared by the guard + its self-test): whether `code`
/// references `component_id` (comments stripped through the shared string-aware
/// scanner, so a `//`/`/*` inside a string literal can never hide a same-line
/// banned reference).
fn reads_component_id(code: &str) -> bool {
    strip_rust_comments(code).contains(BANNED_TOKEN)
}

#[test]
fn svelte_css_scoping_never_reads_component_id() {
    let mut files = Vec::new();
    collect_rs(&svelte_css_dir(), &mut files);
    assert!(
        !files.is_empty(),
        "the svelte css scan found no source files — the guard scanned nothing"
    );

    let mut violations = Vec::new();
    for path in &files {
        let code =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        if reads_component_id(&code) {
            violations.push(path.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "the Svelte CSS scope-hash derivation must not read `component_id` (the \
         Vue explicit scope id — the Svelte scope class derives from the resolved \
         cssHash override or the default filename/css djb2 rule):\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_discriminates_the_banned_token() {
    // The predicate FAILS on a real `component_id` read (inline fixture — the
    // production tree is never edited to prove discrimination).
    assert!(
        reads_component_id("let hash = opts.component_id.clone().unwrap_or_default();"),
        "the guard must fail on a `component_id` read"
    );
    // A COMMENT mention does not trip the guard (the doc comments naming the ban
    // must stay clean).
    assert!(
        !reads_component_id(
            "// NEVER overload component_id for the Svelte cssHash override\nfn ok() {}"
        ),
        "a comment mention must not trip the guard"
    );
    // Clean Svelte css code passes (the resolved-override / default path).
    assert!(
        !reads_component_id(
            "let hash = match resolved_css_hash { Some(o) => o.to_string(), None => css_scope_hash(filename, css) };"
        ),
        "the resolved-override / default derivation must pass"
    );
    // A `component_id` after a string-embedded `//` must stay VISIBLE (the
    // string-aware stripper closes the guard-bypass class).
    assert!(
        reads_component_id(r#"fn f() { let url = "http://x"; let s = opts.component_id; }"#),
        "a banned reference after a string-embedded `//` must stay visible"
    );
}
