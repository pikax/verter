//! Cross-consumer × fact-kind matrix completeness arch guard.
//!
//! This test enforces that every (consumer, fact-kind) cell on the
//! 10 × 5 cross-consumer matrix has a discriminating slice file under
//! `crates/verter_session/tests/fact_matrix/`. Adding a new
//! cache-bearing consumer to the substrate WITHOUT extending the
//! matrix would let a fact-kind regression land silently for that
//! consumer; the arch guard catches it at workspace test time.
//!
//! `REQUIRED_CONSUMERS` is the union of:
//!
//! - the 5 caches wired under `fact_matrix/`
//!   (`materialize_structure`, `ref_cycle`, `memo_entry`,
//!   `app_config_proof`, `owner_import_surface`); and
//! - the 5 caches wired in the top-level
//!   slices (`compile_tier`, `component_meta`, `fallthrough`,
//!   `route_surface`, `slot_binding_graph`).
//!
//! Each slice MUST live at
//! `crates/verter_session/tests/fact_matrix/{consumer}_{fact_kind}.rs`
//! so the arch guard finds it by deterministic path. The top-level
//! matrix tests (e.g.
//! `component_meta_result_matrix_member.rs`) remain in place as
//! substrate-correctness anchors; the `fact_matrix/`-rooted
//! slices give the completeness arch guard one filename per cell.
//!
//! Discrimination: each REQUIRED_CONSUMERS entry whose slice file
//! does NOT exist on disk causes this test to FAIL. A regression
//! that adds a new cache-bearing consumer without filing its 5
//! per-fact-kind slices is caught here.

use std::path::Path;

const REQUIRED_CONSUMERS: &[&str] = &[
    // Caches in `fact_matrix/`.
    "materialize_structure",
    "ref_cycle",
    "memo_entry",
    "app_config_proof",
    "owner_import_surface",
    // Caches filed under `fact_matrix/` from the top-level slices.
    "compile_tier",
    "component_meta",
    "fallthrough",
    "route_surface",
    "slot_binding_graph",
];

const REQUIRED_FACT_KINDS: &[&str] = &[
    "member_presence",
    "member",
    "import_ref",
    "route_surface",
    "module_augmentation_index_shape",
];

#[test]
fn cross_consumer_matrix_completeness() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut missing: Vec<String> = Vec::new();

    for consumer in REQUIRED_CONSUMERS {
        for fact_kind in REQUIRED_FACT_KINDS {
            let rel = format!("tests/fact_matrix/{consumer}_{fact_kind}.rs");
            let abs = manifest_dir.join(&rel);
            if !abs.exists() {
                missing.push(format!(
                    "crates/verter_session/{rel} (consumer={consumer}, \
                     fact_kind={fact_kind})"
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "cross-consumer matrix completeness arch guard:\n\
         {} matrix slice(s) missing:\n  {}\n\n\
         Every (consumer, fact_kind) cell on the {}\u{00d7}{} \
         cross-consumer matrix MUST have a discriminating slice at \
         `crates/verter_session/tests/fact_matrix/<consumer>_<fact_kind>.rs`. \
         Add the missing file with a representative fixture or a \
         documented degenerate-cell reduction. NO empty bodies, NO \
         `assert!(true)`.",
        missing.len(),
        missing.join("\n  "),
        REQUIRED_CONSUMERS.len(),
        REQUIRED_FACT_KINDS.len(),
    );
}

#[test]
fn cross_consumer_matrix_grid_size_matches_expected() {
    // Negative invariant: the grid is exactly 10 \u{00d7} 5 = 50 cells.
    // A regression that silently drops a consumer (e.g. removing
    // `slot_binding_graph` from REQUIRED_CONSUMERS to "make the test
    // pass" when a slice goes missing) would shrink the grid; this
    // sibling guard catches that.
    assert_eq!(
        REQUIRED_CONSUMERS.len(),
        10,
        "REQUIRED_CONSUMERS must list the 10 cache-bearing \
         consumers. Shrinking the \
         list bypasses the completeness guard."
    );
    assert_eq!(
        REQUIRED_FACT_KINDS.len(),
        5,
        "REQUIRED_FACT_KINDS must list the 5 fact-kinds \
         (member_presence, member, import_ref, route_surface, \
         module_augmentation_index_shape)."
    );
    let cells = REQUIRED_CONSUMERS.len() * REQUIRED_FACT_KINDS.len();
    assert_eq!(
        cells, 50,
        "cross-consumer matrix size must be 10\u{00d7}5 = 50; \
         observed {cells}. A regression that drops a consumer OR a \
         fact-kind reduces the grid size below this floor."
    );
}
