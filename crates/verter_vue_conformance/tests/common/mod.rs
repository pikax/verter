//! Shared helpers for the Vue conformance integration suites.

use std::collections::BTreeSet;
use std::path::PathBuf;

use verter_vue_conformance::compare::{compare_modules, Comparison, ModuleInput};
use verter_vue_conformance::{corpus_file, corpus_root, read_text_normalized};

/// Corpus-relative path of one golden artifact for `case_id`/`backend`.
pub fn golden_path(backend: &str, case_id: &str, extension: &str) -> PathBuf {
    corpus_file(
        &corpus_root(),
        &format!("goldens/3.6.0-rc.1/{backend}/{case_id}.{extension}"),
    )
}

pub fn golden_code(backend: &str, case_id: &str) -> String {
    read_text_normalized(&golden_path(backend, case_id, "js")).expect("read golden js")
}

pub fn golden_map(backend: &str, case_id: &str) -> String {
    read_text_normalized(&golden_path(backend, case_id, "map.json")).expect("read golden map")
}

pub fn case_sfc_source(case_id: &str) -> String {
    read_text_normalized(&corpus_file(
        &corpus_root(),
        &format!("cases/{case_id}.vue"),
    ))
    .expect("read corpus sfc")
}

/// Source-authored identifier provenance: the SFC identifier set (see
/// `verter_vue_conformance::authored_identifiers`).
pub fn authored(case_id: &str) -> BTreeSet<String> {
    verter_vue_conformance::authored_identifiers(&case_sfc_source(case_id))
}

/// Code-only comparison (no maps, no diagnostics) with a generous reason cap.
pub fn compare_code(verter: &str, golden: &str, authored: &BTreeSet<String>) -> Comparison {
    let verter = ModuleInput {
        code: verter.to_string(),
        source_map: None,
        diagnostics: Vec::new(),
    };
    let golden = ModuleInput {
        code: golden.to_string(),
        source_map: None,
        diagnostics: Vec::new(),
    };
    compare_modules(&verter, &golden, authored, 64).expect("comparison must not hard-fail")
}

/// Apply a mutation and PROVE it landed (§1a: a plant that fails to apply
/// reports a pass — the assertion on the transformed text is the control).
pub fn plant(original: &str, anchor: &str, replacement: &str, recipe: &str) -> String {
    let mutated = original.replacen(anchor, replacement, 1);
    assert_ne!(
        mutated, original,
        "mutation recipe `{recipe}` failed to apply: anchor not found: {anchor:?}"
    );
    mutated
}

/// Apply a mutation to ALL occurrences and prove it landed.
pub fn plant_all(original: &str, anchor: &str, replacement: &str, recipe: &str) -> String {
    let count = original.matches(anchor).count();
    assert!(
        count >= 1,
        "mutation recipe `{recipe}` failed to apply: anchor not found: {anchor:?}"
    );
    original.replace(anchor, replacement)
}
