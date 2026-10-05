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

pub struct EngineBinding<M> {
    pub(super) macro_mirrors: M,
    pub(super) observers: EngineObservers,
    pub(super) graph: Arc<crate::semantic_query_memo::SemanticGraphStore>,
    pub(super) flow_slice: Arc<crate::cache_runtime::flow_slice_node::FlowSliceStores>,
    pub(super) intrinsics: Arc<crate::intrinsic_registry::IntrinsicRegistry>,
    pub(super) imported_registry: Arc<ImportedRegistryDb>,
    pub(super) declarations: Arc<DeclarationLookupDb>,
    pub(super) resolvability: Arc<ResolvabilityDb>,
    pub(super) owner_collections: Arc<OwnerCollectionDb>,
    pub(super) shapes: Arc<ShapeCacheDb>,
    pub(super) identities: Arc<crate::identity_interner::IdentityInterner>,
    pub(super) mapper_binders: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
}

impl<M> EngineBinding<M> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        observers: EngineObservers,
        macro_mirrors: M,
        graph: Arc<crate::semantic_query_memo::SemanticGraphStore>,
        flow_slice: Arc<crate::cache_runtime::flow_slice_node::FlowSliceStores>,
        intrinsics: Arc<crate::intrinsic_registry::IntrinsicRegistry>,
        imported_registry: Arc<ImportedRegistryDb>,
        declarations: Arc<DeclarationLookupDb>,
        resolvability: Arc<ResolvabilityDb>,
        owner_collections: Arc<OwnerCollectionDb>,
        shapes: Arc<ShapeCacheDb>,
        identities: Arc<crate::identity_interner::IdentityInterner>,
        mapper_binders: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
    ) -> Self {
        Self {
            observers,
            macro_mirrors,
            graph,
            flow_slice,
            intrinsics,
            imported_registry,
            declarations,
            resolvability,
            owner_collections,
            shapes,
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
    /// Select the engine's execution policy from already-resolved values. The
    /// host translates its own configuration into these at the request
    /// boundary; the engine never reads the host configuration.
    pub(crate) fn new(
        depth_budget: usize,
        synthesis_steps: Option<u32>,
        walker_pathological_cap: Option<usize>,
    ) -> Self {
        Self {
            depth_budget,
            synthesis_steps,
            walker_pathological_cap,
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
    #[cfg(any(test, feature = "test-support"))]
    pub(super) forcing: Arc<crate::engine_test_knobs::TestKnobs>,
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
        #[cfg(any(test, feature = "test-support"))] forcing: Arc<
            crate::engine_test_knobs::TestKnobs,
        >,
    ) -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            overflow,
            provenance,
            relation,
            #[cfg(any(test, feature = "test-support"))]
            flow,
            #[cfg(any(test, feature = "test-support"))]
            forcing,
        }
    }
}
