//! Architecture guard: `crates/verter_session/src/lib.rs` MUST stay
//! under the line-count target.
//!
//! `lib.rs` declared one of the largest single source files in the
//! repository before the Tier C cleanup; the long-term shape is a thin
//! crate root that re-exports submodules and owns only the
//! root-`VerterHost` struct definition. Method bodies, large doc blocks,
//! and ancillary impl blocks belong in cohesive submodules.
//!
//! Adding a new method directly on lib.rs that pushes the file past the
//! target should fail this gate; the right pattern is to extend
//! `VerterHost` from a submodule (`host_manage.rs`, `host_resolve.rs`,
//! etc.) — every other Tier C cleanup file already follows this shape.
//!
//! The target line-count is the post-shrink ceiling. The current value
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

const LIB_RS_LINE_CEILING: usize = 830;

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
