//! R28 arch-guard: the Phase 1 parse-phase fact emitter MUST NOT
//! call into cross-decl AST traversal.
//!
//! Verify-bullet 14: the emitter walks `IndexedReady.shallow_state`
//! (which the shallow walk has already populated) and consumes raw
//! source ONLY for module-augmentation extraction (a single-decl
//! walk per `declare module … {}` block, NOT a cross-decl
//! traversal — see fact_emission.rs::collect_augmentations).
//!
//! This test verifies the architectural boundary by source-text
//! grepping the production fact-emission module for banned API
//! calls. The architecture-guard pattern follows the
//! existing `architecture_guards.rs` precedent.
//!
//! Architectural rules bound: R28.

use std::fs;
use std::path::PathBuf;

fn fact_emission_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("fact_emission.rs");
    fs::read_to_string(&path).expect("read fact_emission.rs")
}

#[test]
fn fact_emission_does_not_invoke_cross_decl_oxc_walks() {
    let src = fact_emission_source();
    // Banned: any direct OXC parser/walker invocation. Phase 1
    // consumes pre-extracted `ShallowFileState`; it MUST NOT
    // re-parse or re-walk the full source.
    let banned_substrings: &[&str] = &[
        // OXC entry points
        "oxc_parser",
        "oxc_ast::",
        "Parser::new",
        "oxc::parser",
        // Cross-decl walker entry points in the existing parser
        // crate — Phase 1 MUST NOT call these.
        "parse_module(",
        "parse_script(",
        "analyze_external_type_program",
        // Internal session resolver entry points — Phase 1 MUST
        // NOT cross into resolve-domain code.
        "resolver_runtime::",
        "RouteDb::",
        "ImportedRootDb::",
        "ResolvedImportFacts",
    ];
    for banned in banned_substrings {
        assert!(
            !src.contains(banned),
            "Phase 1 fact emitter MUST NOT reference `{banned}` — \
             R28 arch-guard forbids cross-decl AST traversal in \
             parse-phase emission"
        );
    }
}

#[test]
fn fact_emission_only_reads_shallow_state_or_raw_source() {
    // The legal Phase 1 inputs are:
    //   - `IndexedReady.shallow_state` (pre-extracted by the
    //     shallow walk).
    //   - `IndexedReady.raw_source` (for the single-decl
    //     `declare module …` extractor only — covered separately
    //     in `extract_module_augmentations_from_source`).
    //
    // The emitter source MUST reference `shallow_state` and
    // `raw_source` directly, and MUST NOT pull in cross-file
    // resolver state.
    let src = fact_emission_source();
    assert!(
        src.contains("shallow_state"),
        "fact_emission MUST consume IndexedReady.shallow_state"
    );
    assert!(
        src.contains("raw_source"),
        "fact_emission MUST consume IndexedReady.raw_source (for module-augmentation extraction)"
    );
    // Forbidden: cross-file resolver-state pulls.
    let banned_state_refs: &[&str] = &[
        "host.routes",
        "host.imported_root",
        "ResolverContext",
        "type_solver::",
    ];
    for banned in banned_state_refs {
        assert!(
            !src.contains(banned),
            "fact_emission MUST NOT reference `{banned}` — Phase 1 is parse-domain only"
        );
    }
}

#[test]
fn fact_emission_emits_only_parse_domain_fact_keys() {
    // Phase 1 emits parse-domain fact keys only. The emitter
    // source MUST NOT construct resolve-domain or route-surface
    // `FactKey` variants.
    let src = fact_emission_source();
    let banned_fact_keys: &[&str] = &[
        "FactKey::ResolvedImportClause",
        "FactKey::ResolvedReexportBinding",
        "FactKey::EffectiveExportSet",
        "FactKey::ModuleAugmentationIndexShape",
    ];
    for banned in banned_fact_keys {
        assert!(
            !src.contains(banned),
            "Phase 1 fact emitter MUST NOT construct `{banned}` — \
             those are resolve-domain / route-surface variants \
             populated by Stage 6 (R12)"
        );
    }
}
