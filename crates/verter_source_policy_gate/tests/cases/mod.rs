//! Private sorted manifest of the pure source-tree scanners relocated out
//! of `verter_session::tests::cases` (gate-performance step 2). Each entry
//! derives its verdict purely from reading the workspace tree — see the
//! crate-level docs in `Cargo.toml` and `tests/main.rs`.
//!
//! Relocation note: each moved file's `crate_root()` (or equivalent) used
//! to resolve to `CARGO_MANIFEST_DIR`, which was `crates/verter_session`
//! when these lived there. Four of the eight scan `verter_session`'s own
//! `src/` tree, so their `crate_root()` is re-anchored here to
//! `workspace_root().join("crates/verter_session")` instead of this
//! crate's own manifest dir — see the comment at each file's `crate_root()`
//! definition. The other four (`tracked_paths_are_portable`,
//! `tracked_paths_no_machine_roots`, `scanners_replacement`,
//! `framework_known_bug_manifest`) already computed the workspace root
//! generically (git-rooted or two-parents-up from `CARGO_MANIFEST_DIR`,
//! which lands in the same place from this crate's directory) and needed
//! no logic change, only a path-string fix where one hardcoded its own
//! former location.

mod framework_known_bug_manifest;
mod handle_capable_consumer_guards;
mod output_projector_residual_guards;
mod residual_type_expr_body_reader_inventory;
mod scanners_replacement;
mod tracked_paths_are_portable;
mod tracked_paths_no_machine_roots;
mod whole_env_consumer_graph_native_inventory;
