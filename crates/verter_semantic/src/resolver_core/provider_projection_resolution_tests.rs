use super::{
    build_project_resolve_result, build_resolve_result, project_exact_result,
    provider_id_for_source, provider_ide_id_for_source, source_id_from_provider_id,
};

fn configured_project(root: &str, tsconfig: &str) -> crate::resolver_core::IdeProjectConfig {
    crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    )
}

fn esm_request(importer_id: &str, specifier: &str) -> crate::resolver_core::ResolveRequest {
    crate::resolver_core::ResolveRequest {
        importer_id: importer_id.to_string(),
        specifier: specifier.to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
    }
}

// ── provider_id_for_source / provider_ide_id_for_source ──

#[test]
fn provider_id_for_source_leaves_a_non_carrier_path_unchanged() {
    assert_eq!(
        provider_id_for_source("/proj/src/main.ts"),
        Some("/proj/src/main.ts".to_string())
    );
}

#[test]
fn provider_id_for_source_appends_the_carrier_api_suffix() {
    assert_eq!(
        provider_id_for_source("/proj/src/Comp.vue"),
        Some("/proj/src/Comp.vue.verter.ts".to_string())
    );
}

#[test]
fn provider_ide_id_for_source_appends_tsx_or_jsx_for_a_carrier() {
    assert_eq!(
        provider_ide_id_for_source("/proj/src/Comp.vue", false),
        Some("/proj/src/Comp.vue.tsx".to_string())
    );
    assert_eq!(
        provider_ide_id_for_source("/proj/src/Comp.vue", true),
        Some("/proj/src/Comp.vue.jsx".to_string())
    );
}

#[test]
fn provider_ide_id_for_source_is_none_for_a_non_carrier() {
    assert_eq!(provider_ide_id_for_source("/proj/src/main.ts", false), None);
}

// ── source_id_from_provider_id ──

#[test]
fn source_id_from_provider_id_strips_the_ide_tsx_suffix() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    assert_eq!(
        source_id_from_provider_id(&projects, "/proj/src/Comp.vue.tsx"),
        Some("/proj/src/Comp.vue".to_string())
    );
}

#[test]
fn source_id_from_provider_id_strips_the_api_verter_ts_suffix() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    assert_eq!(
        source_id_from_provider_id(&projects, "/proj/src/Comp.vue.verter.ts"),
        Some("/proj/src/Comp.vue".to_string())
    );
}

#[test]
fn source_id_from_provider_id_passes_through_an_owned_non_carrier_path() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    assert_eq!(
        source_id_from_provider_id(&projects, "/proj/src/main.ts"),
        Some("/proj/src/main.ts".to_string())
    );
}

#[test]
fn source_id_from_provider_id_misses_when_unowned() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    assert_eq!(
        source_id_from_provider_id(&projects, "/elsewhere/main.ts"),
        None
    );
}

// ── build_resolve_result ──

#[test]
fn build_resolve_result_for_an_owned_non_carrier_target() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    let request = esm_request("/proj/src/main.ts", "./util");

    let result = build_resolve_result(
        &projects,
        &request,
        "/proj/src/util.ts".to_string(),
        crate::resolver_core::ResolutionKind::Relative,
    );

    assert_eq!(result.source_id, "/proj/src/util.ts");
    assert_eq!(result.provider_id, "/proj/src/util.ts");
    assert_eq!(result.provider_specifier, "./util");
    assert_eq!(
        result.provider_target,
        crate::resolver_core::ProviderTarget::ShadowSourceFile
    );
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/proj/tsconfig.json")
    );
}

#[test]
fn build_resolve_result_for_an_owned_carrier_target_uses_a_relative_api_specifier() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    let request = esm_request("/proj/src/Parent.vue", "./Comp.vue");

    let result = build_resolve_result(
        &projects,
        &request,
        "/proj/src/Comp.vue".to_string(),
        crate::resolver_core::ResolutionKind::Relative,
    );

    assert_eq!(result.provider_id, "/proj/src/Comp.vue.verter.ts");
    assert_eq!(
        result.provider_target,
        crate::resolver_core::ProviderTarget::CarrierPublicApi
    );
    // Both importer and target are carriers in the same directory — the
    // relative API specifier stays a same-directory "./" reference.
    assert_eq!(result.provider_specifier, "./Comp.vue.verter.ts");
}

#[test]
fn build_resolve_result_for_an_unowned_target() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    let request = esm_request("/proj/src/main.ts", "missing");

    let result = build_resolve_result(
        &projects,
        &request,
        "/elsewhere/thing.ts".to_string(),
        crate::resolver_core::ResolutionKind::NodeModules,
    );

    assert_eq!(result.provider_id, "/elsewhere/thing.ts");
    assert_eq!(
        result.provider_target,
        crate::resolver_core::ProviderTarget::SourceFile
    );
    assert_eq!(result.provider_specifier, "missing");
    assert!(result.owner_tsconfig_path.is_none());
}

// ── build_project_resolve_result ──

#[test]
fn build_project_resolve_result_keeps_the_literal_specifier_for_a_non_carrier_target() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];

    let result = build_project_resolve_result(
        &projects,
        "@/util",
        "/proj/src/util.ts".to_string(),
        crate::resolver_core::ResolutionKind::TsConfigPath,
    );

    assert_eq!(result.provider_id, "/proj/src/util.ts");
    assert_eq!(
        result.provider_target,
        crate::resolver_core::ProviderTarget::ShadowSourceFile
    );
    assert_eq!(result.provider_specifier, "@/util");
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/proj/tsconfig.json")
    );
}

#[test]
fn build_project_resolve_result_keeps_the_literal_specifier_even_for_a_carrier_target() {
    // Discriminates from build_resolve_result: this function never
    // computes a relative_specifier — provider_specifier is always the
    // literal `specifier` argument, even when the target is a carrier.
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];

    let result = build_project_resolve_result(
        &projects,
        "@/Comp.vue",
        "/proj/src/Comp.vue".to_string(),
        crate::resolver_core::ResolutionKind::WorkspaceAlias,
    );

    assert_eq!(result.provider_id, "/proj/src/Comp.vue.verter.ts");
    assert_eq!(
        result.provider_target,
        crate::resolver_core::ProviderTarget::CarrierPublicApi
    );
    assert_eq!(result.provider_specifier, "@/Comp.vue");
}

// ── project_exact_result ──

#[test]
fn project_exact_result_tags_bundler_and_reprojects_the_provider_graph() {
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json")];
    let ctx = crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
    };

    let result = project_exact_result(
        &projects,
        "/proj/src/main.ts",
        "whatever",
        "/proj/src/exact.ts".to_string(),
        ctx,
    );

    assert_eq!(result.source_id, "/proj/src/exact.ts");
    assert_eq!(
        result.resolution_kind,
        crate::resolver_core::ResolutionKind::Bundler
    );
    assert_eq!(result.provider_id, "/proj/src/exact.ts");
    assert_eq!(result.provider_specifier, "whatever");
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/proj/tsconfig.json")
    );
}
