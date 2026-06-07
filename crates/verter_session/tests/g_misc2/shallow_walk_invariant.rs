//! R28 arch-guard: the parse-phase fact emitter MUST NOT call into
//! cross-decl AST traversal AND MUST NOT rescan raw source.
//!
//! The emitter walks `IndexedReady.shallow_state` ONLY (which the
//! shallow walk has already populated). Module-augmentation facts are
//! derived from the typed augmentation inventory
//! (`ShallowFileState.augmentation_scopes` /
//! `augmentation_value_scopes`) — there is NO raw-source byte-scan
//! (Build Philosophy: no stage rescans raw source to rediscover what
//! shallow processing captured).
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
    // Banned: any direct OXC parser/walker invocation. The
    // parse-phase emitter consumes pre-extracted `ShallowFileState`;
    // it MUST NOT re-parse or re-walk the full source.
    let banned_substrings: &[&str] = &[
        // OXC entry points
        "oxc_parser",
        "oxc_ast::",
        "Parser::new",
        "oxc::parser",
        // Cross-decl walker entry points in the existing parser
        // crate — the parse-phase emitter MUST NOT call these.
        "parse_module(",
        "parse_script(",
        "analyze_external_type_program",
        // Internal session resolver entry points — the parse-phase
        // emitter MUST NOT cross into resolve-domain code.
        "resolver_runtime::",
        "RouteDb::",
        "ImportedRootDb::",
        "ResolvedImportFacts",
    ];
    for banned in banned_substrings {
        assert!(
            !src.contains(banned),
            "parse-phase fact emitter MUST NOT reference `{banned}` — \
             R28 arch-guard forbids cross-decl AST traversal in \
             parse-phase emission"
        );
    }
}

#[test]
fn fact_emission_reads_only_shallow_state_never_raw_source() {
    // The SOLE legal parse-phase input is `IndexedReady.shallow_state`
    // (pre-extracted by the shallow walk). Module-augmentation facts
    // are derived from the typed augmentation inventory on the shallow
    // state — there is NO raw-source byte-scan.
    //
    // The emitter source MUST reference `shallow_state`, MUST NOT
    // reference `raw_source` (the retired byte-scanner's input), and
    // MUST NOT pull in cross-file resolver state.
    let src = fact_emission_source();
    assert!(
        src.contains("shallow_state"),
        "fact_emission MUST consume IndexedReady.shallow_state"
    );
    assert!(
        !src.contains("raw_source"),
        "fact_emission MUST NOT rescan IndexedReady.raw_source — augmentation facts are derived \
         from the typed augmentation inventory (no second source of truth, Build Philosophy)"
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
            "fact_emission MUST NOT reference `{banned}` — the parse-phase emitter is parse-domain only"
        );
    }
}

#[test]
fn fact_emission_emits_only_parse_domain_fact_keys() {
    // The parse-phase emitter emits parse-domain fact keys only. The
    // emitter source MUST NOT construct resolve-domain or route-surface
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
            "parse-phase fact emitter MUST NOT construct `{banned}` — \
             those are resolve-domain / route-surface variants \
             populated in the resolve domain (R12)"
        );
    }
}
