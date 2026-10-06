use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::scope_shadowing::*;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use verter_session_query::inputs::prepared::TypeParamBinding;

fn make_binding(name: &str, ordinal: u16) -> TypeParamBinding {
    TypeParamBinding {
        name: Arc::from(name),
        ordinal,
    }
}

fn payload_with(names: &[&str], type_bindings: &[&str]) -> DeclarationScopePayload {
    payload_with_imports(names, type_bindings, &[])
}

fn payload_with_imports(
    names: &[&str],
    type_bindings: &[&str],
    import_names: &[&str],
) -> DeclarationScopePayload {
    let (bundle, owner) = bundle_with_imports(names, type_bindings, import_names);
    DeclarationScopePayload::from_bundle(&*bundle, owner)
}

fn bundle_with_imports(
    names: &[&str],
    type_bindings: &[&str],
    import_names: &[&str],
) -> (Arc<PreparedDeclBundle>, verter_type_expr::TopLevelOwnerId) {
    use crate::resolver_core::prepared_decl::{build_prepared_decl_bundle, ImportCanonicalization};
    use verter_session_query::inputs::prepared::ImportBinding;
    let scope_type_names: rustc_hash::FxHashSet<String> =
        names.iter().map(|s| s.to_string()).collect();
    let mut bindings: FxHashMap<String, TypeParamBinding> = FxHashMap::default();
    for (i, name) in type_bindings.iter().enumerate() {
        bindings.insert(name.to_string(), make_binding(name, i as u16));
    }
    let mut import_bindings: FxHashMap<String, ImportBinding> = FxHashMap::default();
    for name in import_names {
        import_bindings.insert(
            name.to_string(),
            ImportBinding {
                canonical_id: "/import-src.ts".to_string(),
                exported_name: (*name).to_string(),
            },
        );
    }
    // The payload is a shared view over a prepared-decl bundle:
    // build a minimal bundle and stamp the fixture's surfaces onto
    // its (pub) scope fields.
    let state = crate::resolver_core::ShallowFileState::service_backed_for_test("");
    let interner =
        Arc::new(crate::identity_interner::IdentityInterner::with_process_local_account());
    let mut bundle = build_prepared_decl_bundle(
        "/shadow-fixture.ts",
        state,
        FxHashMap::default(),
        bindings.clone(),
        ImportCanonicalization::default(),
        &interner,
    );
    let owner = verter_type_expr::TopLevelOwnerId::instance(0);
    let owner_scope = Arc::get_mut(&mut bundle.owner_scopes)
        .expect("fixture owns its scope collection before projection")
        .entry(owner)
        .or_default();
    owner_scope.scope_type_names = scope_type_names;
    owner_scope.import_bindings = import_bindings;
    owner_scope.script_setup_type_bindings = bindings;
    (Arc::new(bundle), owner)
}

#[test]
fn payload_probe_agrees_with_the_folded_shadow_set() {
    let payload = payload_with_imports(&["Local"], &["Binding"], &["Awaited"]);
    let folded = ScopeShadowing::from_scope_payload(Some(&payload));
    for name in ["Local", "Binding", "Awaited", "Partial", ""] {
        assert_eq!(
            ScopeShadowing::scope_payload_shadows_lib(Some(&payload), name),
            folded.is_shadowing_lib(name),
            "{name:?}"
        );
    }
    for source in ["Local", "Binding", "Awaited"] {
        assert!(
            ScopeShadowing::scope_payload_shadows_lib(Some(&payload), source),
            "each of the three sources shadows on its own: {source:?}"
        );
    }
    assert!(!ScopeShadowing::scope_payload_shadows_lib(None, "Awaited"));
}

#[test]
fn empty_shadow_set_does_not_shadow_any_name() {
    let shadow = ScopeShadowing::empty();
    // Discriminating positive: an empty shadow set never
    // suppresses the builtin fast-path.
    assert!(!shadow.is_shadowing_lib("Pick"));
    assert!(!shadow.is_shadowing_lib("Omit"));
    assert!(!shadow.is_shadowing_lib(""));
}

#[test]
fn from_scope_payload_includes_scope_type_names() {
    // Userland `type Pick<T,_K> = T` lands in scope_type_names.
    let payload = payload_with(&["Pick", "Cfg"], &[]);
    let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
    // Discriminating positive: the userland Pick shadows the
    // ambient-lib `Pick`.
    assert!(shadow.is_shadowing_lib("Pick"));
    // Discriminating negative: an unrelated builtin name with no
    // userland counterpart is NOT shadowed.
    assert!(!shadow.is_shadowing_lib("Omit"));
    // Other scope-local types (Cfg) also enter the shadow set so
    // a userland helper named after a builtin is also caught.
    assert!(shadow.is_shadowing_lib("Cfg"));
}

#[test]
fn from_scope_payload_includes_script_setup_type_bindings() {
    // Script-setup generic `<script setup generic="Pick">` lands in
    // scope_type_bindings (NOT scope_type_names) — the gate must
    // catch this independently.
    let payload = payload_with(&[], &["Pick"]);
    let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
    // Discriminating positive: the script-setup generic param
    // named after a builtin shadows it.
    assert!(shadow.is_shadowing_lib("Pick"));
    // Discriminating negative: a different builtin remains
    // unshadowed.
    assert!(!shadow.is_shadowing_lib("Partial"));
}

#[test]
fn from_scope_payload_includes_import_bindings() {
    // An imported name (`import type { Partial } from "./x"`) lands in
    // `import_bindings` (NOT scope_type_names / scope_type_bindings) — the
    // carrier head-resolution path rehydrates an EMPTY `name_resolution`, so
    // the import binding must shadow the builtin THROUGH this set or an
    // imported `Partial` would wrongly resolve to `__builtin__.Partial`.
    let payload = payload_with_imports(&[], &[], &["Partial"]);
    let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
    // Discriminating positive: the imported `Partial` shadows the builtin.
    assert!(
        shadow.is_shadowing_lib("Partial"),
        "an imported name colliding with a builtin must shadow it (the carrier path's \
         empty name_resolution relies on this)"
    );
    // Discriminating negative: a different builtin with no import remains
    // unshadowed (so the fix does not over-shadow).
    assert!(!shadow.is_shadowing_lib("Pick"));
}

/// A shadow set is a view: constructing one takes a reference to the
/// owner scope's prepared bundle and copies none of its names, so a
/// scope with thousands of declared types costs one refcount per
/// construction — the dispatch constructs one per instantiation.
#[test]
fn a_shadow_set_views_its_bundle_without_copying_names() {
    let names: Vec<String> = (0..3000).map(|index| format!("T{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (bundle, owner) = bundle_with_imports(&names, &[], &[]);
    let pure_bundle = bundle.input_record();
    let raw_before = Arc::strong_count(&bundle);
    let before = Arc::strong_count(&pure_bundle);
    let shadows: Vec<ScopeShadowing> = (0..8)
        .map(|_| ScopeShadowing::from_prepared_decl_bundle(&*bundle, owner))
        .collect();
    assert_eq!(
        Arc::strong_count(&pure_bundle),
        before + shadows.len(),
        "each shadow set holds the bundle it reads"
    );
    for shadow in &shadows {
        let payload = shadow
            .payload_for_tests()
            .expect("a bundle-backed shadow set");
        assert!(Arc::ptr_eq(payload.bundle_for_tests(), &pure_bundle));
        assert!(shadow.is_shadowing_lib("T2999"));
    }
    drop(shadows);
    assert_eq!(Arc::strong_count(&pure_bundle), before);
    assert_eq!(
        Arc::strong_count(&bundle),
        raw_before,
        "pure shadow views never retain source workers"
    );
}

#[test]
fn from_scope_payload_none_returns_empty_set() {
    let shadow = ScopeShadowing::from_scope_payload(None);
    // Discriminating: a `None` payload (e.g. global lowering)
    // shadows nothing — the ambient-lib fast-path stays active
    // for ALL names.
    assert!(!shadow.is_shadowing_lib("Pick"));
    assert!(!shadow.is_shadowing_lib("Omit"));
    assert!(!shadow.is_shadowing_lib("Partial"));
}

#[test]
fn shadow_sets_from_payload_and_bundle_observe_same_names() {
    // Single-source-of-truth invariant: the payload path and the bundle
    // path read the same three surfaces of the same owner scope.
    let (bundle, owner) = bundle_with_imports(&["Pick", "Cfg"], &["T"], &["Imported"]);
    let shadow_from_payload = ScopeShadowing::from_scope_payload(Some(
        &DeclarationScopePayload::from_bundle(&*bundle, owner),
    ));
    let shadow_from_bundle = ScopeShadowing::from_prepared_decl_bundle(&*bundle, owner);
    for name in ["Pick", "Cfg", "T", "Imported", "Omit", ""] {
        assert_eq!(
            shadow_from_payload.is_shadowing_lib(name),
            shadow_from_bundle.is_shadowing_lib(name),
            "{name:?}"
        );
    }
    for name in ["Pick", "Cfg", "T", "Imported"] {
        assert!(shadow_from_bundle.is_shadowing_lib(name), "{name:?}");
    }
    // Negative: an unrelated builtin remains unshadowed via
    // BOTH construction paths.
    assert!(!shadow_from_payload.is_shadowing_lib("Omit"));
    assert!(!shadow_from_bundle.is_shadowing_lib("Omit"));
    // Another owner scope of the same bundle declares nothing.
    let other = ScopeShadowing::from_prepared_decl_bundle(
        &*bundle,
        verter_type_expr::TopLevelOwnerId::instance(1),
    );
    assert!(!other.is_shadowing_lib("Pick"));
}
