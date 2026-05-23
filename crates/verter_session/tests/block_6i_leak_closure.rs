//! Block 6.i leak-closure — Q7 deletion architecture (leak-close-3).
//!
//! Architectural guard for the deletion of the
//! `deep_resolve_slot_function_refs` / `deep_resolve_type_refs` /
//! `deep_resolve_fn_refs` chain from
//! `crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs`.
//! The chain previously walked Object members / Function params /
//! Array elements / Union and Intersection arms and dispatched every
//! `TypeExpr::Ref` through
//! `project_expr_surface_expr_via_host_threaded(Expanded, Expanded,
//! Published)`. That per-Ref Expanded recursion was the ChatMessages
//! `outputSchema|execute` audit-footprint leak.
//!
//! ## Discrimination
//!
//! The `deep_resolve_chain_stays_deleted_in_production_source`
//! source-grep guard fires on `crates/*/src/**`:
//!   * Pre-deletion: each of the three function definitions is a
//!     `pub fn name(` / `fn name(` declaration + corresponding
//!     `engine.name(` call sites in `macro_shapes.rs` — guard FAILS.
//!   * Post-deletion (HEAD): the functions are gone; the only
//!     occurrences of the names are comment archaeology that does not
//!     match the `name(` pattern — guard PASSES.
//!
//! Verified empirically by stashing the Commit-3 deletion changes,
//! running the guard (FAIL: 4 violations across 2 files), unstashing,
//! and re-running (PASS).
//!
//! ## Why the leak-count audit invariant is NOT the test
//!
//! An audit-invariant assertion of the form "zero `ProjectMember`
//! edges for `outputSchema` / `execute` member names" cannot
//! discriminate Q7 on a hermetic SFC: the graph-native
//! `compute_bindings_via_graph` path's `Shallow` dispatch under
//! `Published` demand reduces Mapped types (per
//! `may_reduce_operator(ctx) == matches!(ctx.demand, Published)`),
//! which itself emits one `ProjectMember` edge per enumerated key.
//! Pre- and post-deletion footprints therefore both record the same
//! per-key edges for any Mapped-distributing fixture small enough to
//! reproduce hermetically. The audit gate that discriminates Q7
//! ChatMessages-wide (`grep -cE "outputSchema|execute"
//! cold-seq/ChatMessages.json` 62 → 0) lives in the integration
//! audit corpus, not the unit-test suite. The static guard above is
//! the load-bearing in-tree discriminator.

use std::fs;
use std::path::{Path, PathBuf};

/// Static guard for the Q7 deletion. The three deleted helpers
/// (`deep_resolve_slot_function_refs`, `deep_resolve_type_refs`,
/// `deep_resolve_fn_refs`) MUST NOT reappear in any production
/// source file under `crates/*/src/**`. Test files may reference
/// the names in comments / strings; this guard scopes the scan to
/// `src/` only.
///
/// Definition / call-site patterns are detected via `NAME(`
/// suffix matching. Bare comment mentions like "the deep_resolve_*
/// chain" do not produce that suffix and are correctly ignored,
/// preserving historical archaeology in comments.
///
/// Discriminating: a reintroduction of any of the three would
/// re-open the Rule-5 audit-footprint leak path the Q7 architecture
/// closed, and would FAIL this guard.
#[test]
fn deep_resolve_chain_stays_deleted_in_production_source() {
    fn workspace_root() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or(manifest_dir)
    }

    /// Recursively walk `dir` and collect every `*.rs` file under it.
    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    let crates_dir = workspace_root().join("crates");
    let mut all_src_files: Vec<PathBuf> = Vec::new();
    let Ok(crate_entries) = fs::read_dir(&crates_dir) else {
        panic!("could not read `crates/` directory at {crates_dir:?}");
    };
    for crate_entry in crate_entries.flatten() {
        let src_dir = crate_entry.path().join("src");
        if src_dir.is_dir() {
            collect_rs_files(&src_dir, &mut all_src_files);
        }
    }
    assert!(
        !all_src_files.is_empty(),
        "guard could not enumerate any `crates/*/src/**/*.rs` files \
         under {crates_dir:?} — workspace layout has changed",
    );

    // Search for definition / call-site patterns rather than bare
    // mentions so historical comments (e.g. "the deep_resolve_*
    // chain was deleted because…") do not flag as violations. A
    // function definition looks like `fn NAME(`; a method call /
    // free-function call looks like `NAME(`. Both forms are caught
    // by requiring a `(` immediately after the name; pure comment
    // references like "deep_resolve_*" or "deep_resolve_X chain"
    // never produce that suffix.
    const FORBIDDEN_PATTERNS: &[&str] = &[
        "deep_resolve_slot_function_refs(",
        "deep_resolve_type_refs(",
        "deep_resolve_fn_refs(",
    ];

    let mut violations: Vec<String> = Vec::new();
    for path in &all_src_files {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for forbidden in FORBIDDEN_PATTERNS {
            if content.contains(forbidden) {
                violations.push(format!(
                    "{}: references `{forbidden}` — must not reappear",
                    path.display(),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Block 6.i leak-close-3 (Q7): the deep_resolve_* chain was \
         deleted because its per-Ref Expanded recursion was the \
         ChatMessages `outputSchema|execute` audit-footprint leak. \
         Any reintroduction in production source re-opens the leak. \
         Violations:\n{}",
        violations.join("\n"),
    );
}
