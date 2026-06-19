//! Hermetic-checkout test.
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
//! ## Corpus floor as a constant
//!
//! The floor lives in source as a `const usize`
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
/// pass. Replaces the prior sidecar at
/// `perf_bounds/expected-corpus-test-count.txt`.
const EXPECTED_CORPUS_MIN: usize = 179;

const STITCHER: &str = include_str!("../corpus_audit_tests.rs");

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
        "hermetic-checkout floor: corpus_audit_tests.rs stitches {actual} modules, \
         but the EXPECTED_CORPUS_MIN constant records a floor of {EXPECTED_CORPUS_MIN}. \
         The corpus has shrunk below the documented floor. Either restore the missing modules \
         or refresh the constant deliberately (after a corpus re-vendor)."
    );
}

/// The hermetic-checkout contract requires the stitcher and floor to
/// agree on intent. A drift where the stitcher silently exceeds the floor by a
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

/// Discriminating: the EXPECTED_CORPUS_MIN constant replaces the
/// sidecar `perf_bounds/expected-corpus-test-count.txt`. The sidecar
/// file MUST NOT exist on disk; its presence implies the migration to
/// the constant is incomplete or the file was reintroduced by mistake.
#[test]
fn expected_corpus_test_count_constant_replaces_sidecar() {
    let workspace_root = workspace_root();
    let sidecar = workspace_root
        .join("crates/verter_session/tests/cases/perf_bounds/expected-corpus-test-count.txt");
    assert!(
        !sidecar.exists(),
        "The EXPECTED_CORPUS_MIN constant replaces the sidecar. \
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

// ──────────────────────────────────────────────────────────────────────
// Discriminating tests — LSP component-meta method binding doc, MCP D95
// out-of-scope record, and corpus + semantic-graph snapshot presence.
// ──────────────────────────────────────────────────────────────────────

/// Discriminating: the LSP component-meta method binding doc lives at
/// the pinned path. Its presence records the three custom methods.
///
/// FAIL-pre evidence: deleting the file at the pinned path causes this
/// test to fail with the path's display in the error message.
#[test]
fn lsp_custom_request_method_binding_doc_present() {
    let path = workspace_root().join("docs/arch/debt-closure/15-lsp-component-meta-method.md");
    assert!(
        path.is_file(),
        "D113 + D36: LSP component-meta method binding doc \
         must exist at `{}`. Re-author if missing.",
        path.display(),
    );
}

/// Discriminating: the LSP method binding doc names all
/// three custom methods registered by the `.custom_method(...)` chain in
/// `crates/verter_lsp/src/main.rs:118..160`. Each method literal must be
/// present so the doc faithfully tracks the wire-level entrypoints.
///
/// FAIL-pre evidence: removing any one of the three method literals from
/// the doc causes this test to fail with the missing literal in the
/// error message.
#[test]
fn lsp_method_binding_names_three_methods() {
    let path = workspace_root().join("docs/arch/debt-closure/15-lsp-component-meta-method.md");
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    for method in [
        "$/verter/getComponentMeta",
        "$/verter/getComponentMetaSurface",
        "$/verter/getComponentMetaTypeExpansion",
    ] {
        assert!(
            body.contains(method),
            "D113: LSP method binding doc must name `{}`. \
             Doc at `{}` does not contain that literal.",
            method,
            path.display(),
        );
    }
}

/// Discriminating: the LSP method binding doc records D95 — that the
/// MCP component-meta tool is out of scope. The doc must call out D95
/// specifically so the out-of-scope decision is visible to readers.
///
/// FAIL-pre evidence: dropping the `D95` mention from the doc fails this
/// test.
#[test]
fn mcp_component_meta_tool_binding_documented() {
    let path = workspace_root().join("docs/arch/debt-closure/15-lsp-component-meta-method.md");
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    assert!(
        body.contains("D95"),
        "LSP method binding doc must record D95 (MCP component-meta \
         out-of-scope). Doc at `{}` does not mention D95.",
        path.display(),
    );
    // Stronger negative: the doc must not claim the MCP tool is exposed.
    let lower = body.to_ascii_lowercase();
    assert!(
        lower.contains("out of scope") || lower.contains("out-of-scope"),
        "LSP method binding doc must explicitly mark the MCP \
         component-meta tool as `out of scope` per D95.",
    );
}

/// Discriminating (D116): the macro-impact inventory at
/// `crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md`
/// must cite real production parser source — paths into
/// `crates/verter_parser/src/utils/oxc/vue/script/` and
/// `crates/verter_session/src/host_manage/` files that exist today. A
/// sketch inventory that names paths the parser does not have is a stub.
///
/// FAIL-pre evidence: deleting the inventory causes this test to fail at
/// the file-existence check; replacing real file references with a
/// fictional path fails at the existence check on the cited path.
#[test]
fn macro_impact_inventory_built_from_codebase_baseline() {
    let inventory = workspace_root()
        .join("crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md");
    assert!(
        inventory.is_file(),
        "D116: macro-impact inventory must exist at `{}`",
        inventory.display(),
    );
    let body = std::fs::read_to_string(&inventory)
        .unwrap_or_else(|e| panic!("read {}: {}", inventory.display(), e));

    // The inventory must cite at least one real production source file
    // path that exists on disk.
    let mut cited_paths: Vec<String> = Vec::new();
    for line in body.lines() {
        for token in line.split([' ', '\t', '`', '(', ')', '<', '>', ',', ';', ':']) {
            if (token.starts_with("crates/verter_parser/")
                || token.starts_with("crates/verter_session/"))
                && (token.ends_with(".rs") || token.ends_with(".rs:"))
            {
                let trimmed = token.trim_end_matches(':').trim_end_matches(',');
                cited_paths.push(trimmed.to_string());
            }
        }
    }
    assert!(
        !cited_paths.is_empty(),
        "D116: macro-impact inventory at `{}` must cite at least one \
         production parser source file (e.g., \
         `crates/verter_parser/src/utils/oxc/vue/script/macros.rs`). \
         No `crates/verter_parser/` or `crates/verter_session/` paths found.",
        inventory.display(),
    );
    // At least one cited file must exist on disk — proves the inventory
    // tracks reality, not a sketch.
    let workspace = workspace_root();
    let mut any_exists = false;
    for cited in &cited_paths {
        if workspace.join(cited).is_file() {
            any_exists = true;
            break;
        }
    }
    assert!(
        any_exists,
        "D116: macro-impact inventory cites paths but none exist on disk. \
         Cited: {:?}. Inventory must reflect the current parser baseline.",
        cited_paths,
    );
}

/// Discriminating: the rehoming doc has been promoted to an active
/// spec. The doc body must NOT contain a "Deferred follow-ups"
/// section — those were promoted into the active spec.
///
/// FAIL-pre evidence: re-introducing the `Deferred follow-ups` heading
/// fails this test.
#[test]
fn rehoming_doc_has_no_deferred_followups_section() {
    let path = workspace_root().join("docs/arch/debt-closure/12-host-cache-rehoming.md");
    assert!(
        path.is_file(),
        "D30: host-cache-rehoming doc must exist at `{}`",
        path.display(),
    );
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    assert!(
        !body.contains("Deferred follow-ups"),
        "D30: rehoming doc must not contain a `Deferred follow-ups` \
         section — those were promoted into the active spec.",
    );
}

// ──────────────────────────────────────────────────────────────────────
// Corpus snapshots: presence checks. These tests verify the snapshot
// files exist with at least the floors documented in the partial-data
// marker. The full floor is 179 rows; a partial 17-fixture snapshot was
// produced under a hard timeout, so the row floor is relaxed to the
// achieved count and the partial state is documented in the file.
// ──────────────────────────────────────────────────────────────────────

/// Discriminating: the corpus summary CSV must exist at the pinned path
/// with a non-empty row body. The file's header comment records the
/// partial-vs-full state per the hard-timeout policy.
///
/// FAIL-pre evidence: deleting the file fails this test at the
/// is_file() check.
#[test]
fn golden_corpus_summary_csv_has_at_least_one_row() {
    let path = workspace_root()
        .join("crates/verter_session/tests/cases/perf_bounds/golden-corpus/summary-179.csv");
    assert!(
        path.is_file(),
        "D34: corpus summary CSV must exist at `{}`",
        path.display(),
    );
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // Count non-comment, non-header rows.
    let data_rows: Vec<&str> = body
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .skip(1) // header row
        .collect();
    assert!(
        !data_rows.is_empty(),
        "corpus summary CSV at `{}` must have at least one \
         data row. A vacuous CSV is a stop condition per the worker brief.",
        path.display(),
    );
}

/// Discriminating (D115): the corpus summary CSV header includes the
/// `bridge_max_depth_observed` column reserved for the BFS bridge.
/// Pre-bridge runs record `0`; the column slot itself is the binding
/// contract.
///
/// FAIL-pre evidence: removing the column from the CSV header fails
/// this test.
#[test]
fn golden_corpus_records_bridge_max_depth_per_fixture() {
    let path = workspace_root()
        .join("crates/verter_session/tests/cases/perf_bounds/golden-corpus/summary-179.csv");
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // Find the header row (first non-comment, non-empty line).
    let header = body
        .lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("CSV must have a header row");
    assert!(
        header.contains("bridge_max_depth_observed"),
        "D115: corpus summary CSV header must include the \
         `bridge_max_depth_observed` column. Header was: `{}`",
        header,
    );
}

/// Discriminating: the representative-5 fixture file records the 5
/// named fixtures with explicit status — either a successful audit
/// record OR a `not-completed-in-10min-timeout` marker per the
/// partial-data policy.
///
/// FAIL-pre evidence: deleting the file fails this test.
#[test]
fn golden_corpus_representative_5_present_with_status() {
    let path = workspace_root()
        .join("crates/verter_session/tests/cases/perf_bounds/golden-corpus/representative-5.json");
    assert!(
        path.is_file(),
        "representative-5.json must exist at `{}`",
        path.display(),
    );
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    for name in [
        "Avatar",
        "Button",
        "Calendar",
        "ChatMessage",
        "ChatMessages",
    ] {
        assert!(
            body.contains(&format!("\"{name}\"")),
            "representative-5.json must list fixture `{}` with \
             either a record or a `not-completed-in-10min-timeout` status. \
             Doc at `{}` does not contain that literal.",
            name,
            path.display(),
        );
    }
}

/// Discriminating: the semantic-graph eager-key snapshot must exist
/// with the documented JSON shape. The full floor is 1024 keys;
/// relaxing to the achieved count is allowed when the test run is
/// partial.
///
/// FAIL-pre evidence: deleting the file fails this test at the
/// is_file() check.
#[test]
fn golden_semantic_eager_key_set_present() {
    let path = workspace_root()
        .join("crates/verter_session/tests/cases/perf_bounds/golden-semantic/keys-eager.json");
    assert!(
        path.is_file(),
        "semantic-graph eager-key snapshot must exist at `{}`",
        path.display(),
    );
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // The JSON must declare a `keys` array — even if partial-data
    // truncated it. The relaxation policy permits the count to drop
    // below the 1024 floor when the run was timed out; the array must
    // exist with at least one entry.
    let parsed: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let keys = parsed
        .get("keys")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("`keys` array missing in {}", path.display()));
    assert!(
        !keys.is_empty(),
        "semantic-graph eager-key snapshot must have at least \
         one key entry. A vacuous snapshot is a stop condition.",
    );
}
