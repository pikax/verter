//! Optional drift detector: compares the vendored Vue corpus under
//! `crates/verter_session/tests/component_meta_audit_corpus/fixtures/`
//! against the `nuxt-ui-codex-bench` clone provisioned alongside the
//! repository at `.integration-tests/repos/nuxt-ui-codex-bench/`.
//!
//! Gated behind the `external-corpus` Cargo feature — the default
//! `cargo test --workspace --tests` run MUST stay hermetic. Run with:
//!
//! ```bash
//! cargo test -p verter_session --tests --features external-corpus
//! ```
//!
//! The test surfaces fixture drift so the maintainers can refresh the
//! vendored set deliberately. It is NOT a correctness gate; it does
//! NOT run by default, and CI does not enable the `external-corpus`
//! feature.

#![cfg(feature = "external-corpus")]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join(".integration-tests/repos/nuxt-ui-codex-bench").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "external-corpus test could not locate `.integration-tests/repos/nuxt-ui-codex-bench` \
                 from `{}`; clone the corpus alongside this checkout or unset `--features external-corpus`",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

fn collect_relative_vue_paths(root: &std::path::Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(e) => panic!("read_dir {dir:?}: {e}"),
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("vue") {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel);
            }
        }
    }
    out
}

#[test]
fn vendored_fixture_set_matches_external_corpus_clone() {
    let root = workspace_root();
    let vendored_dir =
        root.join("crates/verter_session/tests/component_meta_audit_corpus/fixtures");
    let upstream_dir =
        root.join(".integration-tests/repos/nuxt-ui-codex-bench/src/runtime/components");

    let vendored = collect_relative_vue_paths(&vendored_dir);
    let upstream = collect_relative_vue_paths(&upstream_dir);

    let missing_in_vendor: Vec<&String> = upstream.difference(&vendored).collect();
    let extra_in_vendor: Vec<&String> = vendored.difference(&upstream).collect();

    assert!(
        missing_in_vendor.is_empty() && extra_in_vendor.is_empty(),
        "vendored corpus has drifted from `nuxt-ui-codex-bench`. \
         Missing in vendor: {missing_in_vendor:?}. Extra in vendor (not upstream): {extra_in_vendor:?}. \
         Refresh by re-vendoring from `.integration-tests/repos/nuxt-ui-codex-bench/src/runtime/components/`."
    );
}
