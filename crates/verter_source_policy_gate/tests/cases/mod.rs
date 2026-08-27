//! Private sorted manifest of the source-tree guards owned outside
//! `verter_session::tests::cases`. Each entry
//! derives its verdict purely from reading the workspace tree — see the
//! crate-level docs in `Cargo.toml` and `tests/main.rs`.
//!
//! Guards that scan `verter_session` anchor `crate_root()` explicitly at
//! `workspace_root().join("crates/verter_session")`; this crate's own
//! `CARGO_MANIFEST_DIR` is not the subject of those checks. The other guards
//! (`tracked_paths_are_portable`,
//! `tracked_paths_no_machine_roots`, `scanners_replacement`,
//! `framework_known_bug_manifest`) already computed the workspace root
//! generically (git-rooted or two parents up from `CARGO_MANIFEST_DIR`).

mod framework_known_bug_manifest;
mod handle_capable_consumer_guards;
mod output_projector_residual_guards;
mod residual_type_expr_body_reader_inventory;
mod scanners_replacement;
mod tracked_paths_are_portable;
mod tracked_paths_no_machine_roots;
mod whole_env_consumer_graph_native_inventory;
