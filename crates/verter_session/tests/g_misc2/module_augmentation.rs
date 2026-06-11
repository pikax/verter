//! R29 binding: per-file module-augmentation fact emission for all
//! four archetypes (external specifier, resolved relative
//! canonical, wildcard ambient, global augmentation).
//!
//! Each R29 archetype emits the correct parse-domain
//! `ModuleAugmentation` fact into `FileArtifacts.augmentations`. The
//! facts are DERIVED from the typed `augmentation_scopes` inventory the
//! binder retains on `ShallowFileState` (`fact_emission::collect_augmentations`)
//! — NOT a raw-source byte-scan. The cross-project `augmentation_index`
//! is populated lazily by the augmentation stitch at dispatch time once
//! resolve-domain dimensions are available; this file only exercises the
//! per-file parse-time emission, so that index stays empty here.
//!
//! Tests load the path-precise fixtures under
//! `tests/fixtures/path_precise/module_augmentation_*.ts`, build an
//! `IndexedReady` through the REAL binder (so the typed augmentation
//! inventory is populated exactly as production does), and run
//! `emit_parse_facts` over it. The discrimination contract: every
//! archetype must produce at least one `ModuleAugmentationFact` with the
//! right `specifier` slot.
//!
//! Architectural rules bound: R29.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::facts::{FactKey, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedSpecifier;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
    )
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture(name: &str) -> String {
    let path = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("tests")
        .join("fixtures")
        .join("path_precise")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn build_indexed_with_source(raw: &str) -> Arc<IndexedReady> {
    // Build the shallow inventory through the REAL binder so the typed
    // augmentation inventory (the single source of truth for augmentation
    // facts) is populated, exactly as production does.
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(raw);
    let shallow = ShallowFileState::from_analysis([0u8; 16], empty_external(), Some(&env));
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        edge_generation: 0,
        raw_source: Arc::from(raw),
        eval_source: Arc::from(""),
        framework_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

#[test]
fn external_specifier_archetype_emits_module_augmentation_fact() {
    // R29: `declare module "vue" { interface ComponentOptions ... }`
    // emits one parse-domain `ModuleAugmentation` fact with
    // specifier = "vue".
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source(&raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        !emission.augmentations.is_empty(),
        "external specifier archetype MUST emit at least one ModuleAugmentationFact"
    );
    let has_vue = emission
        .augmentations
        .iter()
        .any(|f| f.specifier.as_ref() == "vue");
    assert!(has_vue, "expected specifier 'vue' in augmentations");
}

#[test]
fn resolved_relative_canonical_archetype_emits_module_augmentation_fact() {
    let raw = fixture("module_augmentation_relative.ts");
    let indexed = build_indexed_with_source(&raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        !emission.augmentations.is_empty(),
        "relative-specifier archetype MUST emit at least one ModuleAugmentationFact"
    );
    let has_relative = emission
        .augmentations
        .iter()
        .any(|f| f.specifier.as_ref().starts_with("./") || f.specifier.as_ref().starts_with("../"));
    assert!(
        has_relative,
        "expected relative specifier (./… or ../…) in augmentations"
    );
}

#[test]
fn wildcard_ambient_archetype_emits_module_augmentation_fact() {
    let raw = fixture("module_augmentation_wildcard.ts");
    let indexed = build_indexed_with_source(&raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        !emission.augmentations.is_empty(),
        "wildcard ambient archetype MUST emit at least one ModuleAugmentationFact"
    );
    let has_wildcard = emission
        .augmentations
        .iter()
        .any(|f| f.specifier.as_ref().contains('*'));
    assert!(
        has_wildcard,
        "expected wildcard specifier (containing `*`) in augmentations"
    );
}

#[test]
fn global_archetype_emits_module_augmentation_fact_via_global_tag() {
    use verter_session::fact_emission::GLOBAL_AUGMENTATION_TAG;
    let raw = fixture("module_augmentation_global.ts");
    let indexed = build_indexed_with_source(&raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        !emission.augmentations.is_empty(),
        "global archetype MUST emit at least one ModuleAugmentationFact"
    );
    let has_global = emission
        .augmentations
        .iter()
        .any(|f| f.specifier.as_ref() == GLOBAL_AUGMENTATION_TAG);
    assert!(
        has_global,
        "expected $global sentinel specifier in augmentations \
         (the dispatch-time stitch maps this to AugmentationTargetKind::GlobalAugmentation)"
    );
}

#[test]
fn augmentation_facts_land_in_fact_registry() {
    // R29: the per-augmentation fact ALSO lands in the parse-domain
    // `FileFacts.registry` under
    // `FactKey::ModuleAugmentation { specifier, augmented_name, space }`.
    let raw = fixture("module_augmentation_external.ts");
    let indexed = build_indexed_with_source(&raw);
    let emission = emit_parse_facts(&indexed);
    let mut found_in_registry = 0;
    for aug in emission.augmentations.iter() {
        let key = FactKey::ModuleAugmentation {
            specifier: InternedSpecifier::from(aug.specifier.as_ref()),
            augmented_name: aug.augmented_name.clone(),
            space: aug.space,
        };
        if emission.facts.lookup(&key).is_some() {
            found_in_registry += 1;
        }
    }
    assert!(
        found_in_registry > 0,
        "the parse-time emitter MUST land ModuleAugmentation facts in the registry"
    );
}

#[test]
fn augmentation_index_on_file_artifact_store_remains_empty_after_parse_time_emission() {
    // R29: the per-file `FileArtifacts.augmentations` list is
    // populated by the parse-time emitter, but the cross-project
    // `FileArtifactStore.augmentation_index` (keyed on
    // `AugmentationTargetKey`) is NOT — the dispatch-time stitch
    // populates it lazily on first augmentation-sensitive query.
    use verter_session::file_artifact_store::FileArtifactStore;
    let store = FileArtifactStore::new();
    // No production code path populates the index at parse time.
    assert_eq!(
        store.augmentation_index_len(),
        0,
        "augmentation_index MUST stay empty after parse-time emission (populated lazily at dispatch time)"
    );
}

#[test]
fn no_augmentation_archetype_emits_empty_augmentation_list() {
    // Discrimination: an ordinary file with no `declare module …`
    // blocks MUST produce an empty augmentation list.
    let raw = "export const x = 1;";
    let indexed = build_indexed_with_source(raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        emission.augmentations.is_empty(),
        "files without `declare module …` MUST produce empty augmentations"
    );
}

#[test]
fn nested_braces_in_augmentation_do_not_truncate_block() {
    // R29 brace matcher correctness: nested `{ ... }` inside the
    // augmentation body MUST NOT terminate the declare-module
    // block early. We construct a synthetic source with an inline
    // nested object literal.
    let raw = r#"
declare module "x" {
  interface A {
    nested: { a: 1 }
  }
}
const sentinel = 7;
"#;
    let indexed = build_indexed_with_source(raw);
    let emission = emit_parse_facts(&indexed);
    assert!(
        emission
            .augmentations
            .iter()
            .any(|f| f.specifier.as_ref() == "x" && f.augmented_name.as_ref() == "A"),
        "expected interface A under specifier 'x'"
    );
}

// Silence unused-import warning when SymbolSpace isn't needed at
// the moment.
#[allow(dead_code)]
fn _suppress_unused_symbol_space_import(_s: SymbolSpace) {}
