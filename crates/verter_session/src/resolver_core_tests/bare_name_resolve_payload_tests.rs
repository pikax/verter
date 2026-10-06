use crate::resolver_core::bare_name_resolve::*;
use crate::resolver_core::prepared_decl::build_prepared_decl_bundle;
use crate::resolver_core::prepared_decl::ImportCanonicalization;
use crate::resolver_core::ShallowFileState;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use verter_session_query::inputs::prepared::TypeParamBinding;

/// `DeclarationScopePayload` is a VIEW over the prepared-decl
/// bundle: construction shares the bundle's maps through the
/// bundle `Arc` (one refcount bump), never deep-copies them, and
/// script-setup generic params stay in `scope_type_bindings`
/// (the in-scope disjunction checks bindings + names, so the raw
/// `scope_type_names` set no longer materializes the union).
#[test]
fn payload_shares_bundle_maps_instead_of_copying() {
    let source = r#"
export interface Props { label: string }
export const defaults = { label: 'ok' }
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let interner =
        Arc::new(crate::identity_interner::IdentityInterner::with_process_local_account());
    let mut script_setup: FxHashMap<String, TypeParamBinding> = FxHashMap::default();
    script_setup.insert(
        "T".to_string(),
        TypeParamBinding {
            name: Arc::from("T"),
            ordinal: 0,
        },
    );
    let bundle = Arc::new(build_prepared_decl_bundle(
        "/src/Comp.vue.ts",
        Arc::clone(&state),
        FxHashMap::default(),
        script_setup,
        ImportCanonicalization::default(),
        &interner,
    ));

    let module_owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let instance_owner = verter_type_expr::TopLevelOwnerId::instance(0);
    let module_scope = bundle
        .owner_scope(module_owner)
        .expect("ordinary module scope should be prepared");
    let instance_scope = bundle
        .owner_scope(instance_owner)
        .expect("script-setup instance scope should be prepared");
    let module_payload = DeclarationScopePayload::from_bundle(&bundle, module_owner);
    let instance_payload = DeclarationScopePayload::from_bundle(&bundle, instance_owner);

    assert!(
        std::ptr::eq(
            module_payload.scope_type_names(),
            &module_scope.scope_type_names
        ),
        "scope_type_names must be the bundle's own set, not a copy"
    );
    assert!(
        std::ptr::eq(
            module_payload.scope_value_names(),
            &module_scope.scope_value_names
        ),
        "scope_value_names must be the bundle's own set, not a copy"
    );
    assert!(
        std::ptr::eq(
            instance_payload.scope_type_bindings(),
            &instance_scope.script_setup_type_bindings
        ),
        "scope_type_bindings must be the bundle's own map, not a copy"
    );
    assert!(
        std::ptr::eq(
            module_payload.import_bindings(),
            &module_scope.import_bindings
        ),
        "import_bindings must be the bundle's own map, not a copy"
    );

    // Union-removal contract: the script-setup param is visible
    // through the bindings map, NOT through the raw name set.
    assert!(instance_payload.scope_type_bindings().contains_key("T"));
    assert!(!module_payload.scope_type_names().contains("T"));
    assert!(module_payload.scope_type_names().contains("Props"));
    assert!(module_payload.scope_value_names().contains("defaults"));
    assert!(!module_payload.scope_type_bindings().contains_key("T"));
}
