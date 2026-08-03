//! R12 binding: parse-domain facts carry ZERO resolved-canonical
//! data; resolve-domain facts carry resolutions.
//!
//! Verify-bullet bindings:
//!
//! - Bullet 9 — Parse-domain `ImportRef` semantic_hash invariant
//!   under `paths` config edits. The parse-domain fact carries
//!   only `(specifier, binding, space)`; a tsconfig `paths` change
//!   that affects resolution does NOT shift this fact.
//!
//! Architectural rules bound: R12, R16.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::facts::{FactKey, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::{InternedName, InternedSpecifier};
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::{ImportTarget, ShallowFileState};

fn empty_routes() -> Arc<verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory> {
    Arc::new(verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory::default())
}

fn build_with_import(local: &str, specifier: &str, imported: &str) -> Arc<IndexedReady> {
    build_with_imports(&[(local, specifier, imported)])
}

fn build_with_imports(imports: &[(&str, &str, &str)]) -> Arc<IndexedReady> {
    let mut import_targets = FxHashMap::default();
    let mut import_locals = FxHashSet::default();
    for &(local, specifier, imported) in imports {
        import_targets.insert(
            local.to_string(),
            ImportTarget {
                source_specifier: specifier.to_string(),
                imported_name: imported.to_string(),
                is_namespace: false,
            },
        );
        import_locals.insert(local.to_string());
    }
    let shallow = ShallowFileState::routing_tables_only_for_test(
        [0u8; 16],
        FxHashMap::default(),
        Vec::new(),
        import_locals,
        import_targets,
        empty_routes(),
    );
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        Arc::new(shallow),
        Arc::from(""),
        Arc::from(""),
    ))
}

#[test]
fn import_ref_semantic_hash_depends_only_on_specifier_binding_space() {
    // R12: the parse-domain `ImportRef` records ONLY the syntactic
    // `(specifier, binding, space)`.
    //
    // That a RESOLVED canonical cannot leak into it is now structural —
    // `ImportTarget` has no resolved-canonical field at all, and shallow
    // construction performs no resolution — so this case pins the other
    // half: nothing ELSE about the artifact leaks in either. Two
    // artifacts sharing one import but differing in their surrounding
    // import surface must emit a bit-identical `ImportRef` fact for the
    // shared import. An emitter that folded the owner's whole surface
    // (or its resolved dependency set, were one ever reintroduced) into
    // the hash would fail here.
    let alone = build_with_imports(&[("Theme", "./theme", "Theme")]);
    let alongside =
        build_with_imports(&[("Theme", "./theme", "Theme"), ("Other", "./other", "Other")]);

    let emission_alone = emit_parse_facts(&alone);
    let emission_alongside = emit_parse_facts(&alongside);

    let key = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./theme"),
        binding: InternedName::from("Theme"),
        space: SymbolSpace::Type,
    };
    let alone_fact = emission_alone
        .facts
        .lookup(&key)
        .expect("ImportRef emitted for the lone import");
    let alongside_fact = emission_alongside
        .facts
        .lookup(&key)
        .expect("ImportRef emitted for the shared import");
    assert_eq!(
        alone_fact.semantic_hash, alongside_fact.semantic_hash,
        "R12: parse-domain ImportRef carries ONLY (specifier, binding, space)"
    );
    // Discrimination: the second artifact really did differ.
    assert!(
        emission_alongside
            .facts
            .lookup(&FactKey::ImportRef {
                specifier: InternedSpecifier::from("./other"),
                binding: InternedName::from("Other"),
                space: SymbolSpace::Type,
            })
            .is_some(),
        "the second artifact must genuinely carry the extra import"
    );
    assert!(
        emission_alone
            .facts
            .lookup(&FactKey::ImportRef {
                specifier: InternedSpecifier::from("./other"),
                binding: InternedName::from("Other"),
                space: SymbolSpace::Type,
            })
            .is_none(),
        "the first artifact must genuinely NOT carry it"
    );
}

#[test]
fn import_ref_changes_when_specifier_text_changes() {
    // Discrimination: a real specifier-text edit DOES change the
    // parse-domain fact (would invalidate the consumer's resolved
    // canonical via the ResolvedImportClause fact).
    let with_theme = build_with_import("Theme", "./theme", "Theme");
    let with_styles = build_with_import("Theme", "./styles", "Theme");

    let e1 = emit_parse_facts(&with_theme);
    let e2 = emit_parse_facts(&with_styles);

    let key_theme = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./theme"),
        binding: InternedName::from("Theme"),
        space: SymbolSpace::Type,
    };
    let key_styles = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./styles"),
        binding: InternedName::from("Theme"),
        space: SymbolSpace::Type,
    };
    assert!(
        e1.facts.lookup(&key_theme).is_some(),
        "v1 emits ImportRef('./theme')"
    );
    assert!(
        e1.facts.lookup(&key_styles).is_none(),
        "v1 has NO ImportRef('./styles')"
    );
    assert!(
        e2.facts.lookup(&key_styles).is_some(),
        "v2 emits ImportRef('./styles')"
    );
    assert!(
        e2.facts.lookup(&key_theme).is_none(),
        "v2 has NO ImportRef('./theme')"
    );
}

#[test]
fn import_ref_changes_when_binding_text_changes() {
    let with_theme = build_with_import("Theme", "./theme", "Theme");
    let with_alias = build_with_import("ThemeAlias", "./theme", "Theme");
    let e1 = emit_parse_facts(&with_theme);
    let e2 = emit_parse_facts(&with_alias);
    let key_old = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./theme"),
        binding: InternedName::from("Theme"),
        space: SymbolSpace::Type,
    };
    let key_new = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./theme"),
        binding: InternedName::from("ThemeAlias"),
        space: SymbolSpace::Type,
    };
    assert!(e1.facts.lookup(&key_old).is_some());
    assert!(e1.facts.lookup(&key_new).is_none());
    assert!(e2.facts.lookup(&key_old).is_none());
    assert!(e2.facts.lookup(&key_new).is_some());
}
