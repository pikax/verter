//! Freshness guard for the generated typeinfo manifest data.
//!
//! Every generated manifest artifact (`typeinfo_ignored_test_manifest_rows.rs`,
//! `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`,
//! `typeinfo_guard_registry.rs`, `typeinfo_guard_registry_lib.rs` under
//! `crates/verter_session/tests/cases/manifest_data/`, plus
//! `docs/arch/typeinfo-row-registry-counts.md`) is produced from
//! `scripts/gen-typeinfo-ignore-manifest.mjs`
//! (`pnpm gen:typeinfo-manifest`) — the SOLE writer of all of them.
//! The authoritative append-only row registry at
//! `scripts/manifests/typeinfo-row-block-partition.json` feeds each
//! `IgnoredTestRow`'s `block_id` and `status` (joined with the live
//! `#[ignore]` discovery and the Capability Map); block landing statuses,
//! amendments, and landing transactions come from
//! `scripts/manifests/typeinfo-programme-reconciliation.json`. The
//! `AdditionalProofRow` table, the `TYPEINFO_PARITY_BLOCKS` block
//! contracts (each block's
//! required_guards/verification_labels/prereqs/mechanisms), and the
//! `GuardId` registry come from the generator's own maps, NOT from the row
//! registry. Whenever the generator or its inputs change, the committed
//! files must be regenerated and committed in the same change.
//!
//! This guard mirrors the proto-bindings freshness pattern
//! (`crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs`): it
//! invokes the generator in `--check` mode, which regenerates each
//! tracked output in memory and byte-compares against the committed file
//! WITHOUT writing the tree, exiting non-zero (status 6) on any drift and
//! naming the stale file(s). A hand-edit to ANY generated manifest file —
//! or a generator change without regen — makes this test FAIL.
//!
//! The check gracefully skips when `node` is absent (running `cargo
//! test` on a machine without node), exactly as the proto freshness
//! test skips when `buf` is absent. CI ships node, so the
//! discrimination holds in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

/// Locate a runnable `node` interpreter.
///
/// 1. Prefer an explicit `NODE` env override (CI hook).
/// 2. Fall back to `node` on `PATH`.
/// 3. Return `None` when none resolves — the test then skips gracefully
///    (running on a node-free machine), mirroring how the proto
///    freshness test skips when `buf` is absent.
fn locate_node(workspace_root: &Path) -> Option<PathBuf> {
    if let Some(val) = std::env::var_os("NODE") {
        let candidate = PathBuf::from(&val);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Bare name in the override → resolve via PATH below.
        if let Some(found) = which_on_path(&candidate) {
            return Some(found);
        }
    }
    if let Some(found) = which_on_path(Path::new("node")) {
        return Some(found);
    }
    let _ = workspace_root;
    None
}

/// Minimal `which`: returns the first existing entry for `name` on `PATH`
/// (honouring Windows executable extensions). When `name` is already an
/// absolute existing file it is returned as-is.
fn which_on_path(name: &Path) -> Option<PathBuf> {
    if name.is_absolute() && name.is_file() {
        return Some(name.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let base = dir.join(name);
        if base.is_file() {
            return Some(base);
        }
        if cfg!(windows) {
            for ext in ["exe", "bat", "cmd"] {
                let candidate = base.with_extension(ext);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Byte-equality freshness discriminator: run the manifest generator in
/// `--check` mode (regenerate-in-memory + byte-compare, no tree write) and
/// assert it reports NO drift across EVERY tracked output file
/// (`typeinfo_ignored_test_manifest_rows.rs`,
/// `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`). Any
/// divergence — a hand-edit to a generated file, or a generator change
/// committed without regenerating — surfaces as a non-zero exit (status 6)
/// and the generator's named-file diff is echoed in the panic message.
#[test]
pub(crate) fn typeinfo_manifest_files_are_byte_equal_to_regenerated_generator_output() {
    let root = workspace_root();
    let script = root
        .join("scripts")
        .join("gen-typeinfo-ignore-manifest.mjs");
    assert!(
        script.is_file(),
        "manifest generator script missing at {}",
        script.display(),
    );

    let Some(node) = locate_node(&root) else {
        // Skip gracefully when node isn't installed (e.g. running
        // `cargo test` on a node-free machine), exactly as the proto
        // freshness test skips when `buf` is absent. CI ships node.
        eprintln!(
            "skipping manifest freshness check: no `node` found via \
             $NODE or on `PATH`. Install node (CI ships it) to run \
             `node scripts/gen-typeinfo-ignore-manifest.mjs --check`."
        );
        return;
    };

    let output = Command::new(&node)
        .arg(&script)
        .arg("--check")
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "invoke `{} {} --check`: {err}",
                node.display(),
                script.display(),
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the committed typeinfo manifest data is STALE w.r.t. \
         `scripts/gen-typeinfo-ignore-manifest.mjs`. The generator is the SOLE \
         writer of `crates/verter_session/tests/cases/manifest_data/*.rs`; regenerate \
         with `pnpm gen:typeinfo-manifest` and commit the result.\n\
         generator exit: {status}\n\
         --- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        status = output.status,
    );
}
