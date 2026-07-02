//! Regenerate the two-table typeinfo manifest ledger (§10).
//!
//! Emits three checked-in, generated-not-hand-maintained files:
//!
//! 1. `crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs`
//!    — the 362 `IgnoredTestRow`s, each with the full 13-column schema.
//! 2. `crates/verter_session/tests/manifest_data/typeinfo_additional_proof_rows.rs`
//!    — the CLOSED set of 7 coverage-only `AdditionalProofRow`s.
//! 3. `crates/verter_session/tests/manifest_data/typeinfo_parity_blocks.rs`
//!    — the `TYPEINFO_PARITY_BLOCKS` DAG (every block + prereqs +
//!    dominant mechanism + consumed mechanisms).
//!
//! Each `IgnoredTestRow`'s `block_id` is COMPUTED here from the
//! authoritative §10.4.1 row→block partition in
//! `docs/arch/native-typeinfo-parity.md` joined with the live
//! `#[ignore = "..."]` discovery + the Capability Map — NOT hand-typed
//! 362 times. The `AdditionalProofRow` table (file 2) and the
//! `TYPEINFO_PARITY_BLOCKS` DAG (file 3, with each block's
//! `required_guards`/`verification_labels`/prereqs/mechanisms) are
//! authored in this generator's own static maps (`build_additional_rows`,
//! `emit_block_rows`, `BLOCK_TO_REQUIRED_GUARDS`, `BLOCK_VERIFICATION_LABELS`,
//! the prereq/mechanism maps), NOT derived from §10.4.1. The Rust
//! guard tests only diff/fail; they never write the generated source (repo
//! rule: generators are scripts, not tests).
//!
//! Run after adding / removing / renaming an ignored test, or after the
//! §10.4.1 partition changes:
//!
//! ```text
//! cargo run -p verter_session --bin gen-typeinfo-manifest
//! # or via pnpm:
//! pnpm gen:typeinfo-manifest
//! ```
//!
//! Commit the regenerated rows alongside the source changes that prompted
//! the regeneration.
//!
//! Pass `--check` (or `--verify`) to regenerate in memory and EXIT NON-ZERO
//! (status 6) if any committed file diverges, WITHOUT writing — the drift
//! gate (the Rust guard tests only diff/fail, never write tracked source):
//!
//! ```text
//! cargo run -p verter_session --bin gen-typeinfo-manifest -- --check
//! # or via pnpm:
//! pnpm gen:typeinfo-manifest:check
//! ```

mod args;
mod data;
mod derive;
mod emit;
mod model;
mod partition;
mod run;

use std::process::exit;

fn main() {
    let check_only = args::parse_args();
    exit(run::run(check_only));
}
