//! Parity test — ensures the committed corpus audit tests tree
//! matches what `scripts/gen-corpus-audit-tests.mjs` would produce
//! against the current nuxt-ui fixture set. Plan §3 Commit 12 (F10 WIP).
//!
//! Fails with a readable diff when the committed tree drifts from
//! the generator output. Remediation: rerun
//! `node scripts/gen-corpus-audit-tests.mjs` and commit the result
//! (or rerun with `--update-snapshots` if an override's pinned
//! snapshot needs refreshing).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Locate the workspace root (the ancestor that contains
/// `.integration-tests/repos/nuxt-ui`).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join(".integration-tests/repos/nuxt-ui").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate `.integration-tests/repos/nuxt-ui` from `{}`; \
                 is the integration-tests submodule present?",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

/// Walk a directory recursively and return sorted `(relative_path, contents)` pairs.
fn snapshot_dir(root: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let contents = fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("read {path:?}: {e}"))
                    .replace("\r\n", "\n");
                out.push((rel, contents));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn corpus_generator_output_matches_committed_files() {
    let root = workspace_root();
    let generator = root.join("scripts/gen-corpus-audit-tests.mjs");
    if !generator.exists() {
        panic!("generator script missing at {generator:?} — expected at plan §3 Commit 12 path",);
    }

    let tempdir = tempfile::tempdir().expect("tempdir");
    let output_arg = format!("--output-dir={}", tempdir.path().display());

    let status = Command::new("node")
        .arg(&generator)
        .arg("--dry-run")
        .arg(&output_arg)
        .current_dir(&root)
        .status()
        .expect("spawn node (is Node.js on PATH?)");
    assert!(status.success(), "generator exited non-zero: {status:?}");

    let generated_entry = tempdir.path().join("corpus_audit_tests.rs");
    let generated_subdir = tempdir.path().join("component_meta_audit_corpus");
    assert!(
        generated_entry.exists(),
        "generator did not emit the entry point at {generated_entry:?}",
    );
    assert!(
        generated_subdir.exists(),
        "generator did not emit the corpus subdirectory at {generated_subdir:?}",
    );

    let committed_entry = root.join("crates/verter_session/tests/corpus_audit_tests.rs");
    let committed_subdir = root.join("crates/verter_session/tests/component_meta_audit_corpus");
    assert!(
        committed_entry.exists(),
        "committed entry point missing — run `node {}`",
        generator.display(),
    );
    assert!(committed_subdir.exists(), "committed subdirectory missing");

    // Compare entry-file contents byte-by-byte (LF-normalized).
    let a = fs::read_to_string(&generated_entry)
        .unwrap()
        .replace("\r\n", "\n");
    let b = fs::read_to_string(&committed_entry)
        .unwrap()
        .replace("\r\n", "\n");
    if a != b {
        let diff = similar::TextDiff::from_lines(&b, &a);
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header("committed", "regenerated")
            .to_string();
        panic!(
            "corpus_audit_tests.rs (entry point) is out of sync. \
             Re-run `node scripts/gen-corpus-audit-tests.mjs` and commit the result.\n\n\
             Unified diff:\n{rendered}"
        );
    }

    // Compare subdirectory file-set (presence only, not per-byte
    // contents). `cargo fmt` normalizes minor generator-output
    // layout variations (e.g. single-vs-multi-line `include_str!`
    // calls depending on path length), so byte-exact per-file
    // parity would be too brittle. Structural drift (missing or
    // extra slugs) is what the parity test catches — per-file
    // content drift is caught at build time (rustc compile errors)
    // and by `cargo fmt --check`.
    let generated_files = snapshot_dir(&generated_subdir);
    let committed_files = snapshot_dir(&committed_subdir);

    let gen_names: std::collections::BTreeSet<_> =
        generated_files.iter().map(|(rel, _)| rel.clone()).collect();
    let com_names: std::collections::BTreeSet<_> =
        committed_files.iter().map(|(rel, _)| rel.clone()).collect();

    let mut errors = Vec::new();
    for rel in gen_names.difference(&com_names) {
        errors.push(format!("MISSING from commit: {rel}"));
    }
    for rel in com_names.difference(&gen_names) {
        errors.push(format!("EXTRA in commit (not generated): {rel}"));
    }
    if !errors.is_empty() {
        panic!(
            "corpus tree drift ({} issue(s)). Re-run `node scripts/gen-corpus-audit-tests.mjs` \
             and commit.\n\n{}",
            errors.len(),
            errors.join("\n"),
        );
    }
}
