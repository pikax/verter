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

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
    Arc::new(verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default())
}

fn build_with_import(
    local: &str,
    specifier: &str,
    imported: &str,
    resolved_canonical: &str,
) -> Arc<IndexedReady> {
    let mut import_targets = FxHashMap::default();
    import_targets.insert(
        local.to_string(),
        ImportTarget {
            source_specifier: specifier.to_string(),
            imported_name: imported.to_string(),
            canonical_id: resolved_canonical.to_string(),
        },
    );
    let mut import_locals = FxHashSet::default();
    import_locals.insert(local.to_string());
    let shallow = ShallowFileState {
        whole_hash: [0u8; 16],
        exports: FxHashMap::default(),
        wildcard_reexports: Vec::new(),
        symbols: FxHashMap::default(),
        value_symbols: FxHashMap::default(),
        import_locals,
        import_targets,
        augmentation_scopes: Default::default(),
        augmentation_value_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        cached_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

#[test]
fn import_ref_invariant_under_resolution_change() {
    // R12: parse-domain `ImportRef` records ONLY the syntactic
    // `(specifier, binding, space)`. Two `IndexedReady`s with
    // IDENTICAL import_targets but DIFFERENT resolved canonicals
    // (as would happen under a `paths` config edit that aliases
    // the specifier elsewhere) MUST produce identical
    // semantic_hashes for the parse-domain `ImportRef` fact.
    let pre_resolve_paths = build_with_import("Theme", "./theme", "Theme", "/old/theme.ts");
    let post_resolve_paths = build_with_import("Theme", "./theme", "Theme", "/new/theme.ts");

    let emission_pre = emit_parse_facts(&pre_resolve_paths);
    let emission_post = emit_parse_facts(&post_resolve_paths);

    let key = FactKey::ImportRef {
        specifier: InternedSpecifier::from("./theme"),
        binding: InternedName::from("Theme"),
        space: SymbolSpace::Type,
    };
    let pre_fact = emission_pre
        .facts
        .lookup(&key)
        .expect("ImportRef emitted pre");
    let post_fact = emission_post
        .facts
        .lookup(&key)
        .expect("ImportRef emitted post");
    assert_eq!(
        pre_fact.semantic_hash, post_fact.semantic_hash,
        "R12: parse-domain ImportRef MUST be invariant under resolution change"
    );
}

#[test]
fn import_ref_changes_when_specifier_text_changes() {
    // Discrimination: a real specifier-text edit DOES change the
    // parse-domain fact (would invalidate the consumer's resolved
    // canonical via the ResolvedImportClause fact).
    let with_theme = build_with_import("Theme", "./theme", "Theme", "/theme.ts");
    let with_styles = build_with_import("Theme", "./styles", "Theme", "/theme.ts");

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
    let with_theme = build_with_import("Theme", "./theme", "Theme", "/theme.ts");
    let with_alias = build_with_import("ThemeAlias", "./theme", "Theme", "/theme.ts");
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
