//! Architecture guard: the Svelte-owned duplicate CSS grammar file is
//! physically absent from the source tree.
//!
//! J1 row 5 (`docs/arch/refactor/rev11/charters/J1.md`, acceptance IDs A4 /
//! A11a) requires `svelte/runtime/css/parse.rs` — Svelte's own hand-rolled
//! CSS grammar, superseded by the shared `verter_css_syntax` authority — to
//! be deleted. This is the EXECUTABLE half of that gate for the Svelte
//! side: a durable regression test, not a one-off shell check.
//!
//! A4/A11a's path-absence requirement is a TWO-SLICE gate: this file's
//! `crate_root()`-relative path covers ONLY the Svelte side (row 5, this
//! block). The Vue side (row 3 — `crates/verter_compiler/src/css/` — and
//! the `lightningcss` dependency it pulls in) is a SEPARATE landing unit
//! whose source has not been removed yet, and is deliberately NOT asserted
//! here: asserting its absence against a tree that still contains it
//! would make this test permanently red on a correct, in-progress tree. The
//! retained-vs-deleted grammar-type-name list in Svelte's own `types.rs`
//! stays review-enforced (per A4/A11a's own text) rather than a second
//! landed name-scanner here.

use std::path::PathBuf;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The deleted Svelte-owned grammar file A4/A11a require absent.
const DELETED_SVELTE_GRAMMAR_FILE: &str = "src/svelte/runtime/css/parse.rs";

#[test]
fn svelte_css_grammar_parse_rs_is_absent() {
    let path = crate_root().join(DELETED_SVELTE_GRAMMAR_FILE);
    assert!(
        !path.exists(),
        "{} must stay deleted — Svelte CSS parses exclusively through the shared \
         verter_css_syntax grammar (StyleSyntaxIr); a resurrected parse.rs reintroduces \
         the duplicate grammar authority A4/A11a retire",
        path.display()
    );
}

/// Discrimination: prove the assertion actually distinguishes "absent" from
/// "present" — a path this test knows is currently present (this very test
/// file) must NOT be reported as absent by the same existence check the
/// production assertion above uses.
#[test]
fn discrimination_a_present_file_is_not_reported_absent() {
    let path = crate_root().join("tests/cases/svelte_css_grammar_path_absence.rs");
    assert!(
        path.exists(),
        "the discrimination fixture itself must exist: {}",
        path.display()
    );
}

/// The deleted J1 row-6 reject-gate reader file A16 requires absent
/// (`crates/verter_compiler/src/svelte/runtime/css_reject.rs`). The reject
/// codes now project from `StyleSyntaxIr` (`style_body_reject_code`).
/// Path-absence is still only a filename check; the load-bearing proof is
/// `compile_client_parses_a_style_body_once` plus the svelte_compat_profile
/// suite. This file is retained because A16's charter names
/// "path-absence for css_reject.rs" as one required check.
const DELETED_SVELTE_CSS_REJECT_FILE: &str = "src/svelte/runtime/css_reject.rs";

#[test]
fn svelte_css_reject_rs_is_absent() {
    let path = crate_root().join(DELETED_SVELTE_CSS_REJECT_FILE);
    assert!(
        !path.exists(),
        "{} must stay deleted — reject codes project from StyleSyntaxIr; \
         a resurrected css_reject.rs reintroduces a second CSS grammar",
        path.display()
    );
}
