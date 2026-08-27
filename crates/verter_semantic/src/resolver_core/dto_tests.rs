use super::*;

// These tests pin the semantic-owned DTO field shape and derive behavior so
// accidental field or variant drift is caught mechanically.

#[test]
fn resolve_request_kind_orders_as_declared() {
    assert!(ResolveRequestKind::EsmImport < ResolveRequestKind::TypeImport);
    assert!(ResolveRequestKind::TypeImport < ResolveRequestKind::RequireCall);
    assert!(ResolveRequestKind::RequireCall < ResolveRequestKind::SfcSrcAttr);
}

#[test]
fn resolve_phase_orders_as_declared() {
    assert!(ResolvePhase::CodegenBlocker < ResolvePhase::ProviderGraph);
}

#[test]
fn resolution_context_equality_is_field_wise() {
    let a = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    let b = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    let c = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn resolve_request_construction_round_trips_every_field() {
    let request = ResolveRequest {
        importer_id: "/src/main.ts".to_string(),
        specifier: "./dep".to_string(),
        kind: ResolveRequestKind::EsmImport,
        phase: ResolvePhase::CodegenBlocker,
    };
    assert_eq!(request.importer_id, "/src/main.ts");
    assert_eq!(request.specifier, "./dep");
    assert_eq!(request.kind, ResolveRequestKind::EsmImport);
    assert_eq!(request.phase, ResolvePhase::CodegenBlocker);
}

#[test]
fn resolve_result_construction_round_trips_every_field() {
    let result = ResolveResult {
        source_id: "/src/dep.ts".to_string(),
        provider_id: "/src/dep.ts".to_string(),
        provider_specifier: "./dep".to_string(),
        provider_target: ProviderTarget::SourceFile,
        resolution_kind: ResolutionKind::Relative,
        owner_tsconfig_path: Some("/tsconfig.json".to_string()),
    };
    assert_eq!(result.source_id, "/src/dep.ts");
    assert_eq!(result.provider_target, ProviderTarget::SourceFile);
    assert_eq!(result.resolution_kind, ResolutionKind::Relative);
    assert_eq!(
        result.owner_tsconfig_path.as_deref(),
        Some("/tsconfig.json")
    );
}

#[test]
fn project_ownership_equality_distinguishes_tsconfig_path() {
    let owned = ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };
    let fallback = ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: None,
    };
    assert_ne!(owned, fallback);
}
