//! Private sorted manifest of the consolidated integration-test entries.
//! One `mod <entry>;` per former top-level `tests/<entry>.rs` target. Each
//! entry is its own module so per-entry helpers stay in disjoint scopes —
//! do NOT centralise shared helpers here, and keep this list sorted. The
//! `fixtures/` data directory beside this manifest is reached by absolute
//! `CARGO_MANIFEST_DIR`-relative paths from the entries, not declared here.

mod diagnostics;
