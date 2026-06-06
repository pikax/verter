//! Freshness guard for the generated typeinfo manifest data.
//!
//! Every file under `crates/verter_session/tests/manifest_data/`
//! (`typeinfo_ignored_test_manifest_rows.rs`,
//! `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`) is
//! produced from `scripts/gen-typeinfo-ignore-manifest.py`
//! (`pnpm gen:typeinfo-manifest`) — the SOLE writer of all three files.
//! The authoritative §10.4.1 row→block partition feeds ONLY each
//! `IgnoredTestRow`'s `block_id` (joined with the live `#[ignore]`
//! discovery and the Capability Map). The `AdditionalProofRow` table and
//! the `TYPEINFO_PARITY_BLOCKS` block contracts (each block's
//! required_guards/verification_labels/prereqs/mechanisms) come from the
//! generator's own Python maps, NOT from §10.4.1. Whenever the generator
//! or its inputs change, the committed files must be regenerated and
//! committed in the same change.
//!
//! This guard mirrors the proto-bindings freshness pattern
//! (`crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs`): it
//! invokes the generator in `--check` mode, which regenerates each
//! tracked output in memory and byte-compares against the committed file
//! WITHOUT writing the tree, exiting non-zero (status 6) on any drift and
//! naming the stale file(s). A hand-edit to ANY generated manifest file —
//! or a generator change without regen — makes this test FAIL.
//!
//! The check gracefully skips when `python3` is absent (running `cargo
//! test` on a machine without python), exactly as the proto freshness
//! test skips when `buf` is absent. CI ships python3, so the
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

/// Locate a runnable `python3` interpreter.
///
/// 1. Prefer an explicit `PYTHON3` / `PYTHON` env override (CI hook).
/// 2. Fall back to `python3` / `python` on `PATH`.
/// 3. Return `None` when none resolves — the test then skips gracefully
///    (running on a python-free machine), mirroring how the proto
///    freshness test skips when `buf` is absent.
fn locate_python(workspace_root: &Path) -> Option<PathBuf> {
    for var in ["PYTHON3", "PYTHON"] {
        if let Some(val) = std::env::var_os(var) {
            let candidate = PathBuf::from(&val);
            if candidate.is_file() {
                return Some(candidate);
            }
            // Bare name in the override → resolve via PATH below.
            if let Some(found) = which_on_path(&candidate) {
                return Some(found);
            }
        }
    }
    for name in ["python3", "python"] {
        if let Some(found) = which_on_path(Path::new(name)) {
            return Some(found);
        }
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
fn typeinfo_manifest_files_are_byte_equal_to_regenerated_generator_output() {
    let root = workspace_root();
    let script = root.join("scripts").join("gen-typeinfo-ignore-manifest.py");
    assert!(
        script.is_file(),
        "manifest generator script missing at {}",
        script.display(),
    );

    let Some(python) = locate_python(&root) else {
        // Skip gracefully when python3 isn't installed (e.g. running
        // `cargo test` on a python-free machine), exactly as the proto
        // freshness test skips when `buf` is absent. CI ships python3.
        eprintln!(
            "skipping manifest freshness check: no `python3`/`python` found via \
             $PYTHON3/$PYTHON or on `PATH`. Install python3 (CI ships it) to run \
             `python3 scripts/gen-typeinfo-ignore-manifest.py --check`."
        );
        return;
    };

    let output = Command::new(&python)
        .arg(&script)
        .arg("--check")
        .current_dir(&root)
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "invoke `{} {} --check`: {err}",
                python.display(),
                script.display(),
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the committed typeinfo manifest data is STALE w.r.t. \
         `scripts/gen-typeinfo-ignore-manifest.py`. The generator is the SOLE \
         writer of `crates/verter_session/tests/manifest_data/*.rs`; regenerate \
         with `pnpm gen:typeinfo-manifest` and commit the result.\n\
         generator exit: {status}\n\
         --- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        status = output.status,
    );
}
