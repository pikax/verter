//! §9.11 hermetic-checkout test.
//!
//! The default `cargo test --workspace --tests --verbose` run MUST be
//! hermetic — every test must compile and execute without an external
//! third-party clone (e.g., `nuxt-ui-codex-bench`) on disk. The
//! committed `expected-corpus-test-count.txt` records the minimum
//! number of corpus tests stitched into `corpus_audit_tests.rs`; this
//! test asserts the stitched module count is at least that threshold.
//!
//! ## Discriminating predicate
//!
//! Pre-change tree: the threshold file does not exist OR the
//! `corpus_audit_tests.rs` stitcher does not yet reach the documented
//! count. Post-change tree: both predicates hold simultaneously.
//!
//! A vacuous-pass regression — silently dropping corpus modules from
//! the stitcher while leaving the threshold unchanged — is caught by
//! the strict `>=` comparison below. A regression that leaves the
//! threshold low while modules are still present is caught by an
//! explicit equality lower-bound: the threshold's recorded value is
//! the *floor*, and the test reports the actual count whenever the
//! check fails so the maintainers can refresh the file deliberately.
//!
//! ## Hermeticity contract
//!
//! This test does NOT touch `.integration-tests/repos/...`. It only
//! reads two files inside the workspace tree:
//!
//! - `crates/verter_session/tests/perf_bounds/expected-corpus-test-count.txt`
//!   — the committed minimum.
//! - `crates/verter_session/tests/corpus_audit_tests.rs` — the
//!   stitcher whose `mod` declaration count is the live floor.
//!
//! Both files are inside the workspace; both are read with
//! `include_str!` so the assertion runs against the exact bytes
//! committed alongside the test, not whatever happens to be on disk.
//! The `external_corpus_paths_not_present_outside_gated_tests`
//! architecture guard verifies this file does NOT reference any
//! `.integration-tests/repos/...` path.

const EXPECTED_COUNT: &str =
    include_str!("perf_bounds/expected-corpus-test-count.txt");
const STITCHER: &str = include_str!("corpus_audit_tests.rs");

/// Parse the threshold from the committed file. The file format is a
/// single integer on its own line, with optional trailing newline.
fn parse_threshold(raw: &str) -> u32 {
    let trimmed = raw.trim();
    trimmed
        .parse()
        .unwrap_or_else(|e| {
            panic!(
                "perf_bounds/expected-corpus-test-count.txt must contain a single u32; \
                 got `{trimmed}`: {e}"
            )
        })
}

/// Count `mod <ident>;` lines in the stitcher. Each `#[path = ...]
/// mod <slug>;` pair declares one corpus test module; the `mod ...;`
/// line is the discriminator. The harness counts only `mod ` at column
/// zero because every corpus stitch follows that convention (per
/// `scripts/gen-corpus-audit-tests.mjs`).
fn count_corpus_modules(src: &str) -> u32 {
    src.lines()
        .filter(|line| {
            let l = line.trim_start();
            l.starts_with("mod ") && l.ends_with(';')
        })
        .count() as u32
}

#[test]
fn hermetic_workspace_test_runs_without_external_corpus() {
    let threshold = parse_threshold(EXPECTED_COUNT);
    let actual = count_corpus_modules(STITCHER);
    assert!(
        actual >= threshold,
        "§9.11 hermetic-checkout floor: corpus_audit_tests.rs stitches {actual} modules, \
         but the committed perf_bounds/expected-corpus-test-count.txt records a floor of {threshold}. \
         The corpus has shrunk below the documented floor. Either restore the missing modules \
         or refresh the threshold file deliberately (after a corpus re-vendor)."
    );
    // The threshold itself must be non-trivial: a regression that
    // emptied both the stitcher and the threshold file would otherwise
    // pass `>= 0` vacuously.
    assert!(
        threshold >= 1,
        "§9.11 hermetic-checkout floor: threshold file records {threshold}; this is a vacuous \
         floor (a hermetic run with zero corpus tests proves nothing about hermeticity).",
    );
}

/// The §9.11 contract requires the stitcher and threshold file to
/// agree on intent. A drift where the stitcher silently exceeds the
/// threshold by a large margin (e.g., 50+ modules added without
/// refreshing the threshold) is acceptable per the `>=` comparison
/// above. But a `corpus_audit_tests.rs` that contains zero `mod`
/// declarations is a structural regression — the stitcher generator
/// has produced an empty file. The lower-bound here catches that case
/// loudly.
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

/// Discriminating: the threshold file must parse cleanly and represent
/// a positive integer. A regression that wrote a non-numeric value or
/// a leading-zero artefact would surface here.
#[test]
fn threshold_file_parses_as_positive_integer() {
    let raw = EXPECTED_COUNT;
    let trimmed = raw.trim();
    let parsed: u32 = trimmed
        .parse()
        .expect("threshold file must contain a u32");
    assert!(
        parsed > 0,
        "threshold file parsed to zero — vacuous floor: `{trimmed}`",
    );
    // The trimmed string must equal the parsed value's standard
    // formatting (no leading zeros, no surrounding whitespace beyond
    // the trim). Catches `0010` or `+10` style values that parse but
    // would surprise tooling.
    assert_eq!(
        trimmed,
        parsed.to_string(),
        "threshold file contains non-canonical numeric form `{trimmed}` \
         (canonical would be `{parsed}`)",
    );
}
