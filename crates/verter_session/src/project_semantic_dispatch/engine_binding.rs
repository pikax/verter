//! A one-time attachment consumed by the query facade. This carrier has no
//! resource getters and cannot execute a query or expose the session store.

use std::sync::Arc;
// The overflow counter's only reader is the fact-validation proof surface, so
// the import follows the field's own gate (see `EngineObservers::overflow`).
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::AtomicU64;

use crate::component_meta_caches::{
    DeclarationLookupDb, ImportedRegistryDb, OwnerCollectionDb, ResolvabilityDb, ShapeCacheDb,
};

pub struct EngineBinding {
    pub(super) macro_mirrors: crate::resolver_core::request_inputs::MacroMirrorSelector,
    pub(super) observers: EngineObservers,
    // The two fact-validation stores below belong to the fact-validation proof
    // surface: their only producers (`app_config_no_override_proof_get_or_compute`,
    // `produce_binder_identity_facts`) and their only readers
    // (`project_semantic_dispatch/memo.rs`) are compiled under
    // `test` / `test-support`. They carry the SAME gate, so a shipped build
    // (the wasm32 lane among them) does not hold a per-project store with no
    // reader, and no proof-state handle changes shape between build
    // configurations.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) app_config_proofs: Arc<crate::app_config_proof_db::AppConfigNoOverrideProofDb>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) binder_facts: Arc<crate::binder_identity_facts::BinderIdentityFactsStore>,
    pub(super) graph: Arc<crate::semantic_query_memo::SemanticGraphStore>,
    pub(super) flow_slice: Arc<crate::cache_runtime::flow_slice_node::FlowSliceStores>,
    pub(super) intrinsics: Arc<crate::intrinsic_registry::IntrinsicRegistry>,
    pub(super) imported_registry: Arc<ImportedRegistryDb>,
    pub(super) declarations: Arc<DeclarationLookupDb>,
    pub(super) resolvability: Arc<ResolvabilityDb>,
    pub(super) owner_collections: Arc<OwnerCollectionDb>,
    pub(super) shapes: Arc<ShapeCacheDb>,
    pub(super) component_meta_results: Arc<
        crate::component_meta_result_db::ComponentMetaResultDb<
            crate::component_meta_result_db::CachedComponentMetaResult,
        >,
    >,
    pub(super) vue_surfaces: Arc<
        crate::framework::surface_store::FrameworkSurfaceStore<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    >,
    pub(super) svelte_surfaces: Arc<
        crate::framework::surface_store::FrameworkSurfaceStore<
            crate::typeinfo::framework_surface::SvelteSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    >,
    pub(super) identities: Arc<crate::identity_interner::IdentityInterner>,
    pub(super) mapper_binders: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
}

impl EngineBinding {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        observers: EngineObservers,
        macro_mirrors: crate::resolver_core::request_inputs::MacroMirrorSelector,
        #[cfg(any(test, feature = "test-support"))] app_config_proofs: Arc<
            crate::app_config_proof_db::AppConfigNoOverrideProofDb,
        >,
        #[cfg(any(test, feature = "test-support"))] binder_facts: Arc<
            crate::binder_identity_facts::BinderIdentityFactsStore,
        >,
        graph: Arc<crate::semantic_query_memo::SemanticGraphStore>,
        flow_slice: Arc<crate::cache_runtime::flow_slice_node::FlowSliceStores>,
        intrinsics: Arc<crate::intrinsic_registry::IntrinsicRegistry>,
        imported_registry: Arc<ImportedRegistryDb>,
        declarations: Arc<DeclarationLookupDb>,
        resolvability: Arc<ResolvabilityDb>,
        owner_collections: Arc<OwnerCollectionDb>,
        shapes: Arc<ShapeCacheDb>,
        component_meta_results: Arc<
            crate::component_meta_result_db::ComponentMetaResultDb<
                crate::component_meta_result_db::CachedComponentMetaResult,
            >,
        >,
        vue_surfaces: Arc<
            crate::framework::surface_store::FrameworkSurfaceStore<
                crate::typeinfo::framework_surface::VueSurfaceKey,
                crate::typeinfo::framework_surface::MacroSurfaceDtos,
            >,
        >,
        svelte_surfaces: Arc<
            crate::framework::surface_store::FrameworkSurfaceStore<
                crate::typeinfo::framework_surface::SvelteSurfaceKey,
                crate::typeinfo::framework_surface::MacroSurfaceDtos,
            >,
        >,
        identities: Arc<crate::identity_interner::IdentityInterner>,
        mapper_binders: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
    ) -> Self {
        Self {
            observers,
            macro_mirrors,
            #[cfg(any(test, feature = "test-support"))]
            app_config_proofs,
            #[cfg(any(test, feature = "test-support"))]
            binder_facts,
            graph,
            flow_slice,
            intrinsics,
            imported_registry,
            declarations,
            resolvability,
            owner_collections,
            shapes,
            component_meta_results,
            vue_surfaces,
            svelte_surfaces,
            identities,
            mapper_binders,
        }
    }
}

/// Immutable execution policy selected once at the request boundary.
#[derive(Clone)]
pub struct EnginePolicy {
    pub(crate) depth_budget: usize,
    pub(crate) synthesis_steps: Option<u32>,
    pub(crate) walker_pathological_cap: Option<usize>,
}
impl EnginePolicy {
    pub(crate) fn from_config(config: &crate::HostConfig) -> Self {
        Self {
            depth_budget: config.depth_budget,
            synthesis_steps: config.recursion_budget_overrides.synthesis_steps,
            walker_pathological_cap: config.recursion_budget_overrides.walker_pathological_cap,
        }
    }
}

/// Selected counters and fault witnesses; these own no query or source service.
pub(crate) struct EngineObservers {
    // The overflow counter is read by the unbound-observer fact-tracer
    // basis, whose only consumer is the fact-validation proof surface
    // (`test` / `test-support`), so it carries that same gate.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) overflow: Arc<AtomicU64>,
    pub(super) provenance: Arc<crate::meta_provenance::MetaProvenance>,
    pub(super) relation: Arc<crate::project_semantic_dispatch::relation_knobs::RelationHostKnobs>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) flow:
        Arc<super::flow_return::flow_admission_fault_injection::FlowAdmissionFaultKnobs>,
    #[cfg(test)]
    pub(super) forcing: Arc<crate::host_test_force::TestForceKnobs>,
}
impl EngineObservers {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        #[cfg(any(test, feature = "test-support"))] overflow: Arc<AtomicU64>,
        provenance: Arc<crate::meta_provenance::MetaProvenance>,
        relation: Arc<crate::project_semantic_dispatch::relation_knobs::RelationHostKnobs>,
        #[cfg(any(test, feature = "test-support"))] flow: Arc<
            super::flow_return::flow_admission_fault_injection::FlowAdmissionFaultKnobs,
        >,
        #[cfg(test)] forcing: Arc<crate::host_test_force::TestForceKnobs>,
    ) -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            overflow,
            provenance,
            relation,
            #[cfg(any(test, feature = "test-support"))]
            flow,
            #[cfg(test)]
            forcing,
        }
    }
}
