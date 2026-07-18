#![cfg(feature = "external-corpus")]
//! External-corpus twins of hermetic filesystem tests, exercised against
//! real third-party repository checkouts beside this repo.
//!
//! Excluded from the default canonical run (Testing-Hermeticity); enable
//! explicitly with `--features external-corpus`. When enabled, a missing
//! checkout FAILS the test — no silent vacuous pass.

use super::tests::assert_monorepo_package_paths_resolve;

/// External-corpus twin of `monorepo_package_tsconfig_paths_resolve_at_types`
/// against a real OSS monorepo checkout beside the repo (vize's `scalar`
/// git fixture; override the vize root with `VIZE_ROOT`).
#[test]
fn monorepo_package_tsconfig_paths_resolve_at_types_external_corpus() {
    let vize_root = std::env::var("VIZE_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // crates/verter_workspace → repo root → sibling vize checkout
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("../vize")
        });
    let scalar = vize_root.join("tests/_fixtures/_git/scalar");
    assert!(
        scalar.is_dir(),
        "external-corpus run requires the scalar checkout at {scalar:?} (set VIZE_ROOT); \
         refusing to pass vacuously"
    );
    assert_monorepo_package_paths_resolve(
        &scalar,
        "packages/icons/src/components/ScalarIconPersonSimple.vue",
        "packages/icons/src/types.ts",
        "packages/code-highlight",
    );
}
