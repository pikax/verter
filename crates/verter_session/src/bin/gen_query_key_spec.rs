//! Generator for the `SemanticQueryKeySpec` table artifact.
//!
//! This binary is the SOLE writer of
//! `crates/verter_session/src/semantic_query/query_key_spec_table.txt`. It
//! renders the hand-authored
//! [`semantic_query_key_specs`](verter_session::semantic_query::query_key_spec::semantic_query_key_specs)
//! through
//! [`render_spec_table`](verter_session::semantic_query::query_key_spec::render_spec_table)
//! and writes the result to the checked-in artifact.
//!
//! Run via the pnpm script `gen:query-key-spec`
//! (`cargo run -p verter_session --bin gen-query-key-spec`). The diff-test
//! `semantic_query_key_spec_table_equals_enum` re-renders in memory and
//! byte-compares; it never writes (repo `generators_not_tests` rule).

// Build-time generator binary — NOT a semantic session path. It writes a
// checked-in source artifact via the plain `std::fs` import (the same
// convention the `verter-audit-inspect` generator bin uses); it never reads or
// writes workspace files at session runtime, so it does not route through
// `WorkspaceAccess` / the VFS boundary.
use std::fs;
use std::path::PathBuf;

use verter_session::semantic_query::query_key_spec::{render_spec_table, semantic_query_key_specs};

fn main() {
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("semantic_query")
        .join("query_key_spec_table.txt");

    let rendered = render_spec_table(&semantic_query_key_specs());
    fs::write(&artifact, &rendered)
        .unwrap_or_else(|err| panic!("write spec-table artifact `{}`: {err}", artifact.display()));

    println!(
        "wrote SemanticQueryKeySpec table ({} rows) -> {}",
        semantic_query_key_specs().len(),
        artifact.display()
    );
}
