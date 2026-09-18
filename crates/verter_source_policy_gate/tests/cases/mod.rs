//! Private sorted manifest of the source-tree guards owned outside
//! `verter_session::tests::cases`. Each remaining consumer is an
//! independent `#[test]` (schema, portability, known-bug bijection) or a
//! compile-time capability witness. Canonical Surface 1 uses Nextest, so
//! those tests still run in separate processes.

mod framework_known_bug_manifest;
mod scanners_replacement;
mod semantic_capability_witnesses;
mod tracked_paths_are_portable;
