//! A `&VerterHost`-shaped external type does not satisfy
//! `ResolverObservation`'s bound.
//!
//! `verter_semantic` cannot even name `VerterHost` (it lives in
//! `verter_session`, which depends on `verter_semantic`, never the
//! reverse) — this fixture stands a locally-defined host-shaped handle in
//! for it and proves the trait's seal (private to `verter_semantic`)
//! rejects an external implementor regardless of what it holds. If this
//! ever compiles, the trait stopped being sealed against outside crates,
//! which is the layer-safe ownership guarantee.

use verter_semantic::resolver_core::{
    AttemptOutcome, AugmentationTargetKey, CanonicalId, EnvHashes, FlowFunctionObservationKey,
    LoweredTypeDecl, LoweredValueDecl, ModuleAugmentationIndexObservation,
    ResolutionPackageManifest, StoreViewProjectIdentity,
};
use verter_semantic::resolver_core::ResolverObservation;

/// Stands in for a host/scheduler-backed handle an outside crate might try
/// to launder through the observation interface.
struct FakeHostHandle {
    #[allow(dead_code)]
    scheduler_marker: (),
}

// Trait methods are stubbed out (never called — this fixture never runs,
// only compiles) so the ONLY compile error is the seal violation below,
// not an unrelated "missing trait items" error that would obscure it.
impl ResolverObservation for FakeHostHandle {
    fn env_hashes(&self, _canonical: Option<&str>) -> AttemptOutcome<EnvHashes> {
        unimplemented!()
    }

    fn project_identity(
        &self,
        _canonical: Option<&str>,
    ) -> AttemptOutcome<StoreViewProjectIdentity> {
        unimplemented!()
    }

    fn whole_hash(&self, _canonical: &str) -> AttemptOutcome<Option<[u8; 16]>> {
        unimplemented!()
    }

    fn workspace_is_package_backed(&self, _canonical: &str) -> AttemptOutcome<bool> {
        unimplemented!()
    }

    fn lookup_ambient_symbol(
        &self,
        _consumer_project: verter_semantic::resolver_core::ProjectStableKey,
        _symbol: &str,
    ) -> AttemptOutcome<Option<verter_semantic::resolver_core::AmbientSymbolHit>> {
        unimplemented!()
    }

    fn project_generation(&self) -> AttemptOutcome<u64> {
        unimplemented!()
    }

    fn type_decl(
        &self,
        _canonical: &str,
        _owner: verter_type_expr::TopLevelOwnerId,
        _name: &str,
    ) -> AttemptOutcome<Option<std::sync::Arc<LoweredTypeDecl>>> {
        unimplemented!()
    }

    fn value_decl(
        &self,
        _canonical: &str,
        _owner: verter_type_expr::TopLevelOwnerId,
        _name: &str,
    ) -> AttemptOutcome<Option<std::sync::Arc<LoweredValueDecl>>> {
        unimplemented!()
    }

    fn module_augmentation_index(
        &self,
        _target: &AugmentationTargetKey,
    ) -> AttemptOutcome<ModuleAugmentationIndexObservation> {
        unimplemented!()
    }

    fn function_body_skeleton(
        &self,
        _key: &FlowFunctionObservationKey,
    ) -> AttemptOutcome<Option<std::sync::Arc<verter_semantic::analysis::flow::FunctionBodySkeleton>>>
    {
        unimplemented!()
    }

    fn path_probe(
        &self,
        _path: &str,
    ) -> AttemptOutcome<verter_semantic::resolver_core::PathProbe> {
        unimplemented!()
    }

    fn real_path(&self, _path: &str) -> AttemptOutcome<Option<CanonicalId>> {
        unimplemented!()
    }

    fn package_manifest(
        &self,
        _directory: &str,
    ) -> AttemptOutcome<Option<std::sync::Arc<ResolutionPackageManifest>>> {
        unimplemented!()
    }
}

fn main() {}
