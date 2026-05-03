//! §9.11 hermetic-checkout test.
//!
//! The default `cargo test --workspace --tests --verbose` run MUST be
//! hermetic — every test must compile and execute without an external
//! third-party clone (e.g., a vendored corpus repo) on disk. The
//! `EXPECTED_CORPUS_MIN` constant below records the minimum number of
//! corpus tests stitched into `corpus_audit_tests.rs`; this test
//! asserts the stitched module count is at least that floor.
//!
//! ## Discriminating predicate
//!
//! Pre-change tree: the `corpus_audit_tests.rs` stitcher does not yet
//! reach the documented count. Post-change tree: it does.
//!
//! A vacuous-pass regression — silently dropping corpus modules from
//! the stitcher — is caught by the strict `>=` comparison below. The
//! constant's recorded value is the *floor*; the test reports the
//! actual count whenever the check fails so maintainers can refresh
//! the constant deliberately.
//!
//! ## D34 — corpus floor as a constant
//!
//! Per migration plan D34, the floor lives in source as a `const usize`
//! rather than a sidecar text file. This makes the floor visible in
//! every `cargo test` change-set (no separate file refresh required)
//! and eliminates the parse step. Refreshing the floor is a one-line
//! source edit + commit.
//!
//! ## Hermeticity contract
//!
//! This test does NOT touch the external integration-tests
//! repos clone. It only reads `corpus_audit_tests.rs` (via
//! `include_str!`) so the assertion runs against the exact bytes
//! committed alongside the test, not whatever happens to be on disk.
//! The `external_corpus_paths_not_present_outside_gated_tests`
//! architecture guard scans this file and rejects any forbidden
//! external corpus path literal.

/// Minimum number of stitched corpus tests for hermetic-checkout
/// pass. Per migration plan D34 — replaces the prior sidecar at
/// `perf_bounds/expected-corpus-test-count.txt`.
const EXPECTED_CORPUS_MIN: usize = 179;

const STITCHER: &str = include_str!("corpus_audit_tests.rs");

/// Count `mod <ident>;` lines in the stitcher. Each `#[path = ...]
/// mod <slug>;` pair declares one corpus test module; the `mod ...;`
/// line is the discriminator. The harness counts only `mod ` at column
/// zero because every corpus stitch follows that convention (per
/// `scripts/gen-corpus-audit-tests.mjs`).
fn count_corpus_modules(src: &str) -> usize {
    src.lines()
        .filter(|line| {
            let l = line.trim_start();
            l.starts_with("mod ") && l.ends_with(';')
        })
        .count()
}

// Compile-time check: the constant must be non-trivial. A regression
// that emptied both the stitcher and the constant would otherwise pass
// `>= 0` vacuously.
const _: () = assert!(
    EXPECTED_CORPUS_MIN >= 1,
    "EXPECTED_CORPUS_MIN must be >= 1 — a hermetic run with zero \
     corpus tests proves nothing about hermeticity.",
);

#[test]
fn hermetic_workspace_test_runs_without_external_corpus() {
    let actual = count_corpus_modules(STITCHER);
    assert!(
        actual >= EXPECTED_CORPUS_MIN,
        "§9.11 hermetic-checkout floor: corpus_audit_tests.rs stitches {actual} modules, \
         but the EXPECTED_CORPUS_MIN constant records a floor of {EXPECTED_CORPUS_MIN}. \
         The corpus has shrunk below the documented floor. Either restore the missing modules \
         or refresh the constant deliberately (after a corpus re-vendor)."
    );
}

/// The §9.11 contract requires the stitcher and floor to agree on
/// intent. A drift where the stitcher silently exceeds the floor by a
/// large margin (e.g., 50+ modules added without refreshing the
/// constant) is acceptable per the `>=` comparison above. But a
/// `corpus_audit_tests.rs` that contains zero `mod` declarations is a
/// structural regression — the stitcher generator has produced an
/// empty file. The lower-bound here catches that case loudly.
#[test]
fn corpus_stitcher_is_non_empty() {
    let actual = count_corpus_modules(STITCHER);
    assert!(
        actual >= 1,
        "corpus_audit_tests.rs has zero `mod` declarations — the corpus stitcher is empty. \
         Run `node scripts/gen-corpus-audit-tests.mjs` to regenerate from \
         `tests/component_meta_audit_corpus/`."
    );
}

/// Discriminating (D34): the EXPECTED_CORPUS_MIN constant replaces the
/// sidecar `perf_bounds/expected-corpus-test-count.txt`. The sidecar
/// file MUST NOT exist on disk; its presence implies the migration to
/// the constant is incomplete or the file was reintroduced by mistake.
#[test]
fn expected_corpus_test_count_constant_replaces_sidecar() {
    let workspace_root = workspace_root();
    let sidecar = workspace_root
        .join("crates/verter_session/tests/perf_bounds/expected-corpus-test-count.txt");
    assert!(
        !sidecar.exists(),
        "Migration plan D34: the EXPECTED_CORPUS_MIN constant replaces the sidecar. \
         The file `{}` must not exist on disk. Delete it and bump the constant in \
         hermetic_checkout.rs deliberately when refreshing the floor.",
        sidecar.display(),
    );
}

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is `crates/verter_session/`; ascend two levels
    // to reach the workspace root.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("workspace root reachable from CARGO_MANIFEST_DIR")
}
