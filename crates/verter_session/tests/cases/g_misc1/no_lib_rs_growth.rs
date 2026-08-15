//! Architecture guard: `crates/verter_session/src/lib.rs` MUST stay
//! under the line-count target.
//!
//! `lib.rs` is kept as a thin
//! crate root that re-exports submodules and owns only the
//! root-`VerterHost` struct definition. Method bodies, large doc blocks,
//! and ancillary impl blocks belong in cohesive submodules.
//!
//! Adding a new method directly on lib.rs that pushes the file past the
//! target should fail this gate; the right pattern is to extend
//! `VerterHost` from a submodule (`host_manage.rs`, `host_resolve.rs`,
//! etc.) — every other host submodule already follows this shape.
//!
//! The target line-count is the ceiling. The current value
//! is calibrated to give regular maintenance some breathing room without
//! permitting another 1k-line growth episode.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

// The ceiling accounts for the hash-cons cache layers'
// two `for_tests::dispatch_*_for_tests` shims
// (`substitute_semantic_type_param`,
// `evaluate_deferred_semantic_node_with_context`). The shims live
// inside the existing `for_tests` module gating pattern; further
// extraction would split the test surface for marginal gain.
// +1 for the `pub mod binder_identity_facts;` module declaration (the
// family-A `BinderIdentityFacts` substrate home — one irreducible
// module-declaration line; all payload types live in the submodule).
// +1 for the second `pub use compile::{...}` re-export line. The Vue
// assembler's typed code-plus-map result, its fail-closed outcome, and the
// uncomposable-input-map taxonomy appear in its own public signature, so a
// caller outside the crate cannot name its return type without them. Six
// names do not fit one 100-column line; the payload types all live in the
// `compile` submodule and only the re-export is irreducibly here.
const LIB_RS_LINE_CEILING: usize = 857;

#[test]
fn lib_rs_stays_under_line_ceiling() {
    let path = workspace_root().join("crates/verter_session/src/lib.rs");
    let content =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let line_count = content.lines().count();
    assert!(
        line_count <= LIB_RS_LINE_CEILING,
        "crates/verter_session/src/lib.rs has {line_count} lines (ceiling: \
         {LIB_RS_LINE_CEILING}). Move impl methods or struct definitions \
         to a submodule. Existing pattern: `impl VerterHost {{ ... }}` in \
         host_manage.rs, host_resolve.rs, host_compile.rs, host_upsert.rs, \
         resolver_store.rs, cross_file.rs."
    );
}
