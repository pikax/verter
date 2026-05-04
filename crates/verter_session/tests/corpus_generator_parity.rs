//! Parity test — ensures the committed corpus audit tests tree
//! matches what `scripts/gen-corpus-audit-tests.mjs` would produce
//! against the vendored Vue fixture set under
//! `crates/verter_session/tests/component_meta_audit_corpus/fixtures/`.
//!
//! Fails with a readable diff when the committed tree drifts from
//! the generator output. Remediation: rerun
//! `node scripts/gen-corpus-audit-tests.mjs` and commit the result.
//!
//! Hermetic: the generator and fixtures both live inside the
//! checkout, so this test runs without `.integration-tests/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate the workspace root (the ancestor that contains
/// `scripts/gen-corpus-audit-tests.mjs`).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("scripts/gen-corpus-audit-tests.mjs").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate `scripts/gen-corpus-audit-tests.mjs` from `{}`; \
                 is the workspace root reachable from CARGO_MANIFEST_DIR?",
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

    // The vendored fixture set under `fixtures/` is the input the
    // generator reads, not output it produces. Filter it out of the
    // structural-drift check so the parity test stays focused on
    // generator output.
    let is_generator_output = |rel: &str| !rel.starts_with("fixtures/") && rel != "fixtures";
    let gen_names: std::collections::BTreeSet<_> = generated_files
        .iter()
        .map(|(rel, _)| rel.clone())
        .filter(|rel| is_generator_output(rel))
        .collect();
    let com_names: std::collections::BTreeSet<_> = committed_files
        .iter()
        .map(|(rel, _)| rel.clone())
        .filter(|rel| is_generator_output(rel))
        .collect();

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

/// Slug-derivation helper matching the generator's convention in
/// `slugFor()` — converts a Vue filename into the `snake_case`
/// identifier the generator emits.
fn slug_for(vue_rel: &str) -> String {
    let no_ext = vue_rel.trim_end_matches(".vue").replace('\\', "/");
    let with_unders: String = no_ext.replace(['/', '-'], "_");
    // CamelCase → snake_case via insert underscore before an uppercase
    // letter preceded by lower/digit.
    let mut out = String::with_capacity(with_unders.len() + 8);
    let mut prev: Option<char> = None;
    for ch in with_unders.chars() {
        if ch.is_ascii_uppercase() {
            if let Some(p) = prev {
                if p.is_ascii_lowercase() || p.is_ascii_digit() {
                    out.push('_');
                }
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
        prev = Some(ch);
    }
    out
}

fn runtime_components_dir(root: &Path) -> PathBuf {
    root.join("crates/verter_session/tests/component_meta_audit_corpus/fixtures")
}

/// Walk the vendored Vue fixtures and return the set of component
/// slugs the generator MUST emit.
fn expected_slugs(root: &Path) -> BTreeSet<String> {
    let components_dir = runtime_components_dir(root);
    let mut slugs = BTreeSet::new();
    let mut stack = vec![components_dir.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
            let entry = entry.unwrap();
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("vue") {
                let rel = p
                    .strip_prefix(&components_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                slugs.insert(slug_for(&rel));
            }
        }
    }
    slugs
}

/// Asserts the committed corpus tree covers EVERY vendored `.vue`
/// component under
/// `crates/verter_session/tests/component_meta_audit_corpus/fixtures/`
/// — no missing slugs, no extra ones. The generator is the trusted
/// source of slug derivation; this test guards against the generator
/// regressing to a subset pass.
///
/// Discriminating: if a future edit narrows the component sweep
/// (e.g. adds a filter that drops nested-subdir components), the
/// missing slugs surface in the diff below.
#[test]
fn corpus_audit_coverage_covers_every_vendored_component() {
    let root = workspace_root();
    let expected = expected_slugs(&root);
    assert!(
        !expected.is_empty(),
        "discovery produced zero components — is the fixture present?",
    );

    let committed_subdir = root.join("crates/verter_session/tests/component_meta_audit_corpus");
    let committed: BTreeSet<String> = fs::read_dir(&committed_subdir)
        .unwrap_or_else(|e| panic!("read_dir {committed_subdir:?}: {e}"))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let p = entry.path();
            if !p.is_file() {
                return None;
            }
            let name = p.file_name()?.to_string_lossy().into_owned();
            if name == "README.md" || name == "mod.rs" {
                return None;
            }
            let slug = name.strip_suffix(".rs")?.to_string();
            Some(slug)
        })
        .collect();

    let missing: Vec<&String> = expected.difference(&committed).collect();
    let extra: Vec<&String> = committed.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "corpus audit coverage drift. Missing (expected — not committed): {missing:?}. \
         Extra (committed — not expected): {extra:?}. \
         Re-run `node scripts/gen-corpus-audit-tests.mjs` and commit.",
    );
}

/// Asserts the generator's output is deterministic: running the
/// generator twice into two separate tempdirs MUST produce
/// byte-identical `corpus_audit_tests.rs` entry files AND
/// byte-identical per-component test files. Pins the cross-platform
/// determinism requirement that the generator sorts input files
/// lexicographically.
///
/// Discriminating: if `readdirSync` starts returning OS-dependent
/// order (the generator sorts after discovery, so this is the guard
/// on that sort), or if a future edit introduces a non-deterministic
/// template (e.g. `Date.now()` in a comment), the second run
/// produces a different tree and this test fails.
#[test]
fn corpus_audit_mod_rs_regenerates_deterministically_across_platforms() {
    let root = workspace_root();
    let generator = root.join("scripts/gen-corpus-audit-tests.mjs");
    assert!(
        generator.exists(),
        "generator script missing at {generator:?}"
    );

    let run = |dir: &Path| {
        let status = Command::new("node")
            .arg(&generator)
            .arg("--dry-run")
            .arg(format!("--output-dir={}", dir.display()))
            .current_dir(&root)
            .status()
            .expect("spawn node");
        assert!(status.success(), "generator exited non-zero: {status:?}");
    };

    let a = tempfile::tempdir().expect("tempdir a");
    let b = tempfile::tempdir().expect("tempdir b");
    run(a.path());
    run(b.path());

    // Compare entry point files byte-for-byte (after LF normalization).
    let entry_a = fs::read_to_string(a.path().join("corpus_audit_tests.rs"))
        .expect("read entry a")
        .replace("\r\n", "\n");
    let entry_b = fs::read_to_string(b.path().join("corpus_audit_tests.rs"))
        .expect("read entry b")
        .replace("\r\n", "\n");
    assert_eq!(
        entry_a, entry_b,
        "corpus_audit_tests.rs differs across two generator runs — generator is non-deterministic",
    );

    // Compare per-component files byte-for-byte.
    let snapshot = |dir: &Path| -> Vec<(String, String)> {
        let subdir = dir.join("component_meta_audit_corpus");
        let mut out = Vec::new();
        for entry in fs::read_dir(&subdir).unwrap_or_else(|e| panic!("{subdir:?}: {e}")) {
            let entry = entry.unwrap();
            let p = entry.path();
            if p.is_file() {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let contents = fs::read_to_string(&p)
                    .unwrap_or_else(|e| panic!("{p:?}: {e}"))
                    .replace("\r\n", "\n");
                out.push((name, contents));
            }
        }
        out.sort_by(|x, y| x.0.cmp(&y.0));
        out
    };
    let files_a = snapshot(a.path());
    let files_b = snapshot(b.path());
    assert_eq!(files_a.len(), files_b.len());
    for ((name_a, contents_a), (name_b, contents_b)) in files_a.iter().zip(files_b.iter()) {
        assert_eq!(name_a, name_b, "file name drift between runs");
        assert_eq!(
            contents_a, contents_b,
            "contents of `{name_a}` differ across two generator runs — generator is non-deterministic",
        );
    }
}

/// Pins the current incidental-field set so snapshot stability is
/// not quietly broken. The set is enumerated via the
/// [`IncidentalFields`] trait implementation on
/// [`RequestFootprintAudit`]; this test reads the slice
/// directly from the trait method AND verifies every listed field
/// is actually cleared on the masked output.
///
/// Discriminating along three axes:
///
/// 1. Rename/delete `mask_incidental_spans` → the `contains` check
///    on the helper name fails.
/// 2. Add a field to the trait's `incidental_fields()` slice
///    without extending the match arm in `mask_incidental` → the
///    helper panics at runtime on any footprint with that field
///    populated, surfacing immediately.
/// 3. Change the masker so a listed field is NOT cleared → the
///    behaviour loop at the bottom of this test fails naming the
///    specific field.
#[test]
fn commit_7_snapshots_stable_against_current_incidental_event_names_list() {
    use std::sync::Arc;
    use verter_session::component_meta_audit::{
        IncidentalFields, RequestFootprintAudit, VfsLayer, VfsReadRecord,
    };

    let root = workspace_root();
    // The masking affordance lives on the substrate side now; pin
    // its presence in `verter_audit::footprint`.
    let audit_mod_path = root.join("crates/verter_audit/src/footprint.rs");

    let audit_src =
        fs::read_to_string(&audit_mod_path).unwrap_or_else(|e| panic!("read audit footprint: {e}"));

    assert!(
        audit_src.contains("pub fn mask_incidental_spans"),
        "`pub fn mask_incidental_spans` is missing from \
         `crates/verter_audit/src/footprint.rs` — pinned snapshots lose their \
         stability guarantee. The masking affordance must survive (or this \
         test must be updated in lock-step).",
    );

    // Pin the current trait-declared set. Adding a new field to
    // the mask requires updating BOTH this expected list AND the
    // match statement inside `IncidentalFields::mask_incidental`.
    let incidental_fields = <RequestFootprintAudit as IncidentalFields>::incidental_fields();
    let expected_incidental: &[&str] = &["vfs_reads"];
    assert_eq!(
        incidental_fields, expected_incidental,
        "RequestFootprintAudit::incidental_fields() drifted — if this was \
         intentional, update the expected list here AND regenerate the pinned \
         corpus_representatives snapshots.",
    );

    // Behaviour check: for every field in the trait's declared
    // set, populate it on a fresh footprint and confirm the masked
    // output clears it. This discriminates against a stealth
    // regression where the match arm returns unchanged.
    for &field in incidental_fields {
        let mut fp = RequestFootprintAudit::default();
        match field {
            "vfs_reads" => fp.vfs_reads.push(VfsReadRecord {
                canonical_id: Arc::from("/x.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }),
            unknown => panic!(
                "commit_7_snapshots_stable: incidental_fields() contains `{unknown}` but \
                 this test has no population branch for it — extend the match arm in lock-step",
            ),
        }
        let masked = fp.mask_incidental_spans();
        match field {
            "vfs_reads" => assert!(
                masked.vfs_reads.is_empty(),
                "mask_incidental_spans failed to clear `vfs_reads` — field is listed in \
                 incidental_fields() but survived the mask",
            ),
            _ => unreachable!(),
        }
    }

    // Additionally pin that the F6 authored fixtures are still
    // committed — they're the load-bearing snapshots the
    // incidental-span masker was designed to support.
    let authored_fixtures_dir =
        root.join("crates/verter_session/tests/component_meta_audit/corpus_representatives");
    assert!(
        authored_fixtures_dir.exists(),
        "authored corpus_representatives fixture dir missing at {authored_fixtures_dir:?} — \
         Commit 7 landing should have created it",
    );
    let count = fs::read_dir(&authored_fixtures_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "rs")
        })
        .count();
    assert!(
        count >= 6,
        "expected at least 6 authored corpus_representatives fixtures (Commit 7), found {count}",
    );
}
