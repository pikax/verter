use super::*;
use crate::resolver_core::dto::{ResolvePhase, ResolveRequestKind};

fn ctx() -> ResolutionContext {
    ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    }
}

fn result_fixture() -> ResolveResult {
    ResolveResult {
        source_id: "/src/dep.ts".to_string(),
        provider_id: "/src/dep.ts".to_string(),
        provider_specifier: "./dep".to_string(),
        provider_target: ProviderTarget::SourceFile,
        resolution_kind: ResolutionKind::Relative,
        owner_tsconfig_path: None,
    }
}

// ── NormalizedSpecifier ──

#[test]
fn normalized_specifier_trims_trailing_slash_for_relative() {
    let s = NormalizedSpecifier::new("./dep/");
    assert_eq!(s, NormalizedSpecifier::new("./dep"));
}

#[test]
fn normalized_specifier_keeps_trailing_slash_for_bare_specifier() {
    // Only relative specifiers get the trailing-slash trim.
    let s = NormalizedSpecifier::new("vue/");
    assert_eq!(s, NormalizedSpecifier("vue/".to_string()));
}

#[test]
fn normalized_specifier_rewrites_backslashes_for_bare_specifier() {
    let s = NormalizedSpecifier::new("pkg\\sub");
    assert_eq!(s, NormalizedSpecifier("pkg/sub".to_string()));
}

// ── ResolveContextId ──

#[test]
fn unowned_is_a_fixed_constant() {
    assert_eq!(ResolveContextId::unowned(), ResolveContextId::unowned());
}

#[test]
fn with_provider_projection_copies_target_project_identity() {
    let base = ResolveContextId::from_hashes([1; 16], [2; 16]);
    let target = ResolveContextId::from_hashes([9; 16], [8; 16]);
    let projected = base.with_provider_projection(&target);
    let (project, _resolver_policy, provider_policy) = projected.identity_parts();
    // project_identity is untouched by the projection...
    assert_eq!(project, [1; 16]);
    // ...but provider_policy_identity takes on the target's project identity.
    assert_eq!(provider_policy, [9; 16]);
}

#[test]
fn with_external_provider_projection_is_deterministic_and_input_sensitive() {
    let base = ResolveContextId::from_hashes([1; 16], [2; 16]);
    let a = base
        .clone()
        .with_external_provider_projection(&result_fixture());
    let b = base.with_external_provider_projection(&result_fixture());
    assert_eq!(a, b);

    let mut other = result_fixture();
    other.source_id = "/src/other.ts".to_string();
    let c =
        ResolveContextId::from_hashes([1; 16], [2; 16]).with_external_provider_projection(&other);
    assert_ne!(a, c);
}

// ── ResolutionQueryKey ──

#[test]
fn importer_and_explicit_produce_distinct_entries() {
    let population = ResolutionPopulation::Base;
    let importer = ResolutionQueryKey::importer(
        "/src/main.ts",
        "./dep",
        ctx(),
        ResolveContextId::unowned(),
        population,
    );
    let explicit = ResolutionQueryKey::explicit(
        ProjectIdentity([7; 16]),
        "./dep",
        ctx(),
        ResolveContextId::unowned(),
        population,
    );
    assert_ne!(importer, explicit);
}

// ── ResolutionFactVersion ──

#[test]
#[should_panic(expected = "resolution fact versions must be non-zero")]
fn fresh_rejects_zero() {
    let _ = ResolutionFactVersion::fresh(0);
}

#[test]
fn initial_is_distinct_from_any_fresh_version() {
    assert_ne!(
        ResolutionFactVersion::INITIAL,
        ResolutionFactVersion::fresh(1)
    );
}

// ── ResolutionFactKey ──

#[test]
fn decision_and_owner_resolution_set_are_derived_nodes() {
    let population = ResolutionPopulation::Base;
    let query = ResolutionQueryKey::importer(
        "/src/main.ts",
        "./dep",
        ctx(),
        ResolveContextId::unowned(),
        population,
    );
    let decision = ResolutionFactKey::decision(query);
    assert!(decision.is_derived_node());

    let owner_set = ResolutionFactKey::owner_resolution_set(
        CanonicalResolutionId::new("/src/main.ts"),
        population,
    );
    assert!(owner_set.is_derived_node());

    let path_probe = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/src/main.ts"),
        population,
    };
    assert!(!path_probe.is_derived_node());
}

#[test]
fn in_population_rewrites_every_variant() {
    let base = ResolutionPopulation::Base;
    let key = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/x.ts"),
        population: base,
    };
    let session = ResolutionPopulation::Session(
        crate::resolver_core::resolution_world_identity::SessionFingerprint::from_raw(1),
    );
    let rewritten = key.in_population(session);
    assert_eq!(rewritten.population(), session);
}

#[test]
fn reobservable_path_canonical_id_is_none_for_derived_and_table_lookup_keys() {
    let population = ResolutionPopulation::Base;
    let context_selection = ResolutionFactKey::context_importer("/src/main.ts", population);
    assert_eq!(context_selection.reobservable_path_canonical_id(), None);

    let path_probe = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/src/main.ts"),
        population,
    };
    assert_eq!(
        path_probe.reobservable_path_canonical_id(),
        Some("/src/main.ts")
    );
}

#[test]
fn canonical_id_is_none_for_explicit_project_entries() {
    let population = ResolutionPopulation::Base;
    let key = ResolutionFactKey::context_explicit(ProjectIdentity([3; 16]), population);
    assert_eq!(key.canonical_id(), None);
}

// ── ResolutionFactRef ──

#[test]
fn is_owner_resolution_set_and_is_decision_are_mutually_exclusive_oracles() {
    let population = ResolutionPopulation::Base;
    let owner_ref = ResolutionFactRef::new(
        ResolutionFactKey::owner_resolution_set(CanonicalResolutionId::new("/x.ts"), population),
        ResolutionFactVersion::fresh(1),
    );
    assert!(owner_ref.is_owner_resolution_set());
    assert!(!owner_ref.is_decision());

    let query = ResolutionQueryKey::importer(
        "/src/main.ts",
        "./dep",
        ctx(),
        ResolveContextId::unowned(),
        population,
    );
    let decision_ref = ResolutionFactRef::new(
        ResolutionFactKey::decision(query),
        ResolutionFactVersion::fresh(1),
    );
    assert!(decision_ref.is_decision());
    assert!(!decision_ref.is_owner_resolution_set());
}
