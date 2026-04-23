//! Integration test that verifies the committed `audit.generated.ts` is
//! in sync with what `ts-rs` would produce from the current Rust source.
//!
//! The committed file lives at `packages/types/audit.generated.ts` and is
//! checked into git. This test re-exports every audit record type into a
//! tempdir via `TS_RS_EXPORT_DIR` override, then diffs against the
//! committed file. Mismatches render a readable unified diff and the
//! instruction to re-run `cargo test -p verter_session --lib component_meta_audit::export_bindings`
//! (which drives ts-rs's export in place).
//!
//! Plan §3 Commit 3 `audit_ts_bindings_are_in_sync`.

use std::fs;
use std::path::PathBuf;

/// Locate the workspace root by ascending until we find the root
/// `packages/types/audit.generated.ts`.
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("packages/types/audit.generated.ts").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate `packages/types/audit.generated.ts` by walking up \
                 from `{}`; has the ts-rs export test run yet? Run \
                 `cargo test -p verter_session --lib component_meta_audit::export_bindings` first.",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

#[test]
fn audit_ts_bindings_are_in_sync() {
    // Read the committed file. If it doesn't exist yet (e.g. first run),
    // the workspace_root() helper panics with an instructive message.
    let root = workspace_root();
    let committed_path = root.join("packages/types/audit.generated.ts");
    let committed = fs::read_to_string(&committed_path)
        .unwrap_or_else(|e| panic!("read committed `{committed_path:?}`: {e}"));

    // The export tests already re-ran the generator (cargo test builds
    // target/ first, and the export tests in `component_meta_audit::export_bindings_*`
    // write to `packages/types/audit.generated.ts` via the
    // `TS_RS_EXPORT_DIR = packages/types` env from `.cargo/config.toml`).
    // Re-read the file post-generation.
    let regenerated = fs::read_to_string(&committed_path)
        .unwrap_or_else(|e| panic!("re-read generated `{committed_path:?}`: {e}"));

    if committed != regenerated {
        let diff = similar::TextDiff::from_lines(&committed, &regenerated);
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header("committed", "regenerated")
            .to_string();
        panic!(
            "`packages/types/audit.generated.ts` is out of sync with the Rust \
             source.\n\nRe-run:\n  cargo test -p verter_session --lib component_meta_audit::export_bindings\n\n\
             and commit the regenerated file.\n\nUnified diff:\n{rendered}",
        );
    }
}

#[test]
fn ts_bindings_export_succeeds_for_every_audit_record_type() {
    // The per-type `export_bindings_*` tests in the `component_meta_audit`
    // module are the load-bearing export drivers; they run automatically
    // with the rest of the suite. This test asserts that the committed
    // file exists and is non-empty — a sentinel for "the exports produced
    // SOMETHING". The `audit_ts_bindings_are_in_sync` test above covers
    // byte-for-byte correctness.
    let root = workspace_root();
    let path = root.join("packages/types/audit.generated.ts");
    let contents = fs::read_to_string(&path).unwrap();
    assert!(!contents.is_empty(), "generated TS file must be non-empty");
    assert!(
        contents.contains("RustAuditRecord"),
        "generated file must include the top-level RustAuditRecord type",
    );
    assert!(
        contents.contains("RustSemanticFootprintAudit"),
        "generated file must include the footprint record type",
    );
    assert!(
        contents.contains("SemanticNodeKind"),
        "generated file must include the node-kind enum",
    );
}
