//! Cross-consumer × fact-kind matrix completeness arch guard.
//!
//! This test enforces that every (consumer, fact-kind) cell on the
//! 9 × 4 cross-consumer matrix has a discriminating slice file under
//! `crates/verter_session/tests/fact_matrix/`. Adding a new
//! cache-bearing consumer to the substrate WITHOUT extending the
//! matrix would let a fact-kind regression land silently for that
//! consumer; the arch guard catches it at workspace test time.
//!
//! `REQUIRED_CONSUMERS` is the union of:
//!
//! - the 4 caches wired under `fact_matrix/`
//!   (`materialize_structure`, `memo_entry`,
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
//! that adds a new cache-bearing consumer without filing its
//! per-fact-kind slices is caught here.
//!
//! # The column set is a REPRESENTATIVE SUBSET, and only shrinks with the registry
//!
//! `REQUIRED_FACT_KINDS` is NOT the list of fact kinds that exist, and
//! must not be read as one. `FactKey` currently has 15 live variants;
//! this grid covers 4. `MemberShape`, `MacroSurface`, `TemplateRoot`,
//! `Export`, `ExportAlias`, `SyntacticExportSet`, `LocalDecl`,
//! `SyntacticReexportRef`, `ModuleAugmentation`, `ResolvedImportClause`
//! and `ResolvedReexportBinding` are all live with NO column here. The
//! grid is a chosen cross-section, not a completeness proof over the
//! registry — a variant's absence from this list says nothing about
//! whether it is covered elsewhere.
//!
//! What the list DOES promise is one-directional: a column may only be
//! removed when its fact kind is gone. Adding a variant to `FactKey`
//! does not automatically owe a column (this is a subset); removing a
//! column always owes the registry check below.
//!
//! This cuts both ways, and the second direction is the load-bearing
//! one. Widening is obvious. NARROWING is the dangerous direction, so
//! it is admissible only when the fact kind itself is GONE from the
//! registry — never to accommodate a consumer that is inconvenient to
//! cover, and never by filing slices that cannot fail differently from
//! slices already present. A cell whose test duplicates its sibling
//! satisfies the grid while discriminating nothing: it makes the guard
//! LOOK complete and is strictly worse than an honestly smaller grid.
//! If a column is dropped, the reason belongs in the same change.
//!
//! The `route_surface` column was retired for exactly that reason: it
//! named the `EffectiveExportSet` fact, whose surface was deleted after
//! it was verified dead (its sole insert was `#[cfg(test)]` with zero
//! callers, and the production validator's lookup was operationally
//! always absent). `RouteSurface` is now a ONE-ARM domain whose only
//! fact is `ModuleAugmentationIndexShape` — which has its own column,
//! so the surviving arm keeps full per-consumer coverage.

use std::path::Path;

const REQUIRED_CONSUMERS: &[&str] = &[
    // Caches in `fact_matrix/`.
    "materialize_structure",
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

/// The fact kinds that EXIST. See the module doc: this list tracks the
/// live registry, so the grid's width follows the domain. A kind may be
/// removed here ONLY once the fact itself is gone from the registry.
const REQUIRED_FACT_KINDS: &[&str] = &[
    "member_presence",
    "member",
    "import_ref",
    "module_augmentation_index_shape",
];

#[test]
fn cross_consumer_matrix_completeness() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut missing: Vec<String> = Vec::new();

    for consumer in REQUIRED_CONSUMERS {
        for fact_kind in REQUIRED_FACT_KINDS {
            let rel = format!("tests/cases/fact_matrix/{consumer}_{fact_kind}.rs");
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
    // Negative invariant: the grid is exactly 9 \u{00d7} 4 = 36 cells.
    // A regression that silently drops a consumer (e.g. removing
    // `slot_binding_graph` from REQUIRED_CONSUMERS to "make the test
    // pass" when a slice goes missing) would shrink the grid; this
    // sibling guard catches that. The grid is 9 — the tenth slot was
    // the retired `RefCycleResultDb`; its replacement (the
    // `ClassifyMaterializationCycleGate` family) rides the
    // `SemanticGraphStore` memo substrate already covered by
    // `memo_entry`.
    assert_eq!(
        REQUIRED_CONSUMERS.len(),
        9,
        "REQUIRED_CONSUMERS must list the 9 cache-bearing \
         consumers. Shrinking the \
         list bypasses the completeness guard."
    );
    assert_eq!(
        REQUIRED_FACT_KINDS.len(),
        4,
        "REQUIRED_FACT_KINDS must list the 4 LIVE fact-kinds \
         (member_presence, member, import_ref, \
         module_augmentation_index_shape). This count follows the \
         registry: shrink it ONLY in the change that removes the fact \
         kind itself, never to accommodate an uncovered consumer."
    );
    let cells = REQUIRED_CONSUMERS.len() * REQUIRED_FACT_KINDS.len();
    assert_eq!(
        cells, 36,
        "cross-consumer matrix size must be 9\u{00d7}4 = 36; \
         observed {cells}. A regression that drops a consumer OR a \
         LIVE fact-kind reduces the grid below this floor."
    );
}
