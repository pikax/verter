//! Compiler-visible ownership checks for semantic fact values.
//!
//! Workspace cache authority may re-export these values, but every re-export
//! must be the exact semantic nominal type. `TypeId` equality is insensitive
//! to spelling, formatting, and source location while discriminating a second
//! struct/enum with the same fields.

use std::any::TypeId;

fn assert_same_nominal_type<Semantic: 'static, Workspace: 'static>() {
    assert_eq!(TypeId::of::<Semantic>(), TypeId::of::<Workspace>());
}

#[test]
fn fact_version_value_graph_is_semantic_owned_and_workspace_reexported() {
    use verter_semantic::facts::version as semantic;
    use verter_workspace::fact_cache as workspace;

    assert_same_nominal_type::<semantic::FactHash16, workspace::FactHash16>();
    assert_same_nominal_type::<semantic::CompactionDomain, workspace::CompactionDomain>();
    assert_same_nominal_type::<semantic::AggregateStamp, workspace::AggregateStamp>();
    assert_same_nominal_type::<semantic::RouteSurfaceStamp, workspace::RouteSurfaceStamp>();
    assert_same_nominal_type::<semantic::SemanticImportsStamp, workspace::SemanticImportsStamp>();
    assert_same_nominal_type::<semantic::ResolutionRootsStamp, workspace::ResolutionRootsStamp>();
    assert_same_nominal_type::<
        semantic::SessionOverlayFingerprint,
        workspace::SessionOverlayFingerprint,
    >();
    assert_same_nominal_type::<semantic::OverlayId, workspace::OverlayId>();
    assert_same_nominal_type::<semantic::ViewPopulationParent, workspace::ViewPopulationParent>();
    assert_same_nominal_type::<semantic::CompletionOverlayState, workspace::CompletionOverlayState>(
    );
    assert_same_nominal_type::<semantic::RequestCompletion, workspace::RequestCompletion>();
    assert_same_nominal_type::<semantic::ViewPopulation, workspace::ViewPopulation>();
    assert_same_nominal_type::<semantic::AggregatePopulation, workspace::AggregatePopulation>();
    assert_same_nominal_type::<semantic::DomainGenerationFact, workspace::DomainGenerationFact>();
    assert_same_nominal_type::<semantic::ParseEnvHash, workspace::ParseEnvHash>();
    assert_same_nominal_type::<semantic::DerivedFactKind, workspace::DerivedFactKind>();
    assert_same_nominal_type::<semantic::ParseFactRef, workspace::ParseFactRef>();
    assert_same_nominal_type::<semantic::ResolveImportsFactRef, workspace::ResolveImportsFactRef>();
    assert_same_nominal_type::<semantic::RouteSurfaceFactRef, workspace::RouteSurfaceFactRef>();
    assert_same_nominal_type::<
        semantic::ProgramAnalysisFunctionRef,
        workspace::ProgramAnalysisFunctionRef,
    >();
    assert_same_nominal_type::<semantic::ProgramAnalysisFactRef, workspace::ProgramAnalysisFactRef>(
    );
    assert_same_nominal_type::<semantic::StrictSelfRootWorld, workspace::StrictSelfRootWorld>();
    assert_same_nominal_type::<semantic::FactVersionRef, workspace::FactVersionRef>();
    assert_same_nominal_type::<
        semantic::FactAttribution<'static>,
        workspace::FactAttribution<'static>,
    >();

    // Mutation control: replacing any workspace `pub use` above with a
    // duplicate nominal definition keeps fields/methods compiling but makes
    // that exact TypeId assertion red.
}

#[test]
fn resolution_identity_value_graph_is_semantic_owned_and_workspace_reexported() {
    use verter_semantic::facts::resolution as semantic;
    use verter_workspace::resolution_currency as workspace;

    assert_same_nominal_type::<semantic::CanonicalResolutionId, workspace::CanonicalResolutionId>();
    assert_same_nominal_type::<semantic::NormalizedSpecifier, workspace::NormalizedSpecifier>();
    assert_same_nominal_type::<semantic::RawSpecifier, workspace::RawSpecifier>();
    assert_same_nominal_type::<semantic::ProjectIdentity, workspace::ProjectIdentity>();
    assert_same_nominal_type::<semantic::ResolverPolicyIdentity, workspace::ResolverPolicyIdentity>(
    );
    assert_same_nominal_type::<semantic::ProviderPolicyIdentity, workspace::ProviderPolicyIdentity>(
    );
    assert_same_nominal_type::<semantic::ResolveEnvHash, workspace::ResolveEnvHash>();
    assert_same_nominal_type::<semantic::ResolutionEntry, workspace::ResolutionEntry>();
    assert_same_nominal_type::<semantic::ResolveContextId, workspace::ResolveContextId>();
    assert_same_nominal_type::<semantic::ResolutionQueryKey, workspace::ResolutionQueryKey>();
    assert_same_nominal_type::<semantic::ResolutionFactVersion, workspace::ResolutionFactVersion>();
    assert_same_nominal_type::<semantic::ResolutionFactKey, workspace::ResolutionFactKey>();
    assert_same_nominal_type::<semantic::ResolutionFactRef, workspace::ResolutionFactRef>();

    // Mutation control: a workspace-local shadow graph has distinct nominal
    // identity even when its entire field vocabulary mirrors semantic.
}

#[test]
fn input_resolution_policy_is_the_exact_semantic_owned_nominal_value() {
    assert_same_nominal_type::<
        verter_semantic::resolver_core::InputResolutionBudgets,
        verter_workspace::InputResolutionBudgets,
    >();

    let tightened =
        verter_workspace::InputResolutionBudgets::try_tightened(128, 512, 65_536, 32, 4)
            .expect("the workspace re-export must retain the semantic constructor");
    let _workspace = verter_workspace::MemoryWorkspace::new_with_input_resolution_budgets(
        Default::default(),
        tightened,
    );

    // Mutation control: replace the workspace re-export with a local five-field
    // policy carrier and route either constructor through it. The TypeId
    // equality fails even when the duplicate exposes identical values.
}
