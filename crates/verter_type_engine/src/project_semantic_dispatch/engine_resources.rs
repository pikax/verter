//! Engine resource construction: the engine's own shared stores plus the one
//! output authority minted for them.
//!
//! [`EngineStores::create`] is the ONLY place an [`OutputAuthority`] comes
//! into existence. It builds a fresh set of engine stores and, in the same
//! step, the authority bound to that set's semantic graph. It accepts no store
//! handle, so recovering a live engine's graph or stores and passing them back
//! cannot remint authority for that engine: every call constructs a NEW engine
//! whose authority binds only to its own graph.
//!
//! Query access and output authority are separated at this construction. The
//! stores become the query resources a request binding clones; the authority
//! is handed to the host's composition code, which stores it privately and
//! lends it only to its terminal output sinks. Neither the binding, the
//! dispatch, nor the request ports can produce or recover it.
//!
//! The same construction is the only mint of the engine's
//! [`SurfaceClaimAuthority`], the capability code outside the engine needs to
//! state a surface-resolution claim. The host holds it privately beside the
//! output authority and lends it to its surface-publication adapters.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};

use super::output_materialization::{MaterializedOutputTypeExpr, OutputTypeExpr};
use super::ProjectSemanticDispatch;
use crate::component_meta_caches::{
    DeclarationLookupDb, ImportedRegistryDb, OwnerCollectionDb, ResolvabilityDb, ShapeCacheDb,
};
use crate::semantic_query::surface_resolution::SurfaceClaimAuthority;
use crate::semantic_query::{ProjectionReductionContext, SemanticNodeId};
use crate::semantic_query_memo::{MemoFamilyBound, SemanticGraphStore};

/// One engine's shared stores, freshly constructed together.
pub struct EngineStores {
    pub graph: Arc<SemanticGraphStore>,
    /// The construction lane of [`Self::graph`]; minted only here.
    pub construction: crate::semantic_query_memo::node_roots::NodeConstructionGrant,
    pub flow_slice: Arc<crate::cache_runtime::flow_slice_node::FlowSliceStores>,
    pub intrinsics: Arc<crate::intrinsic_registry::IntrinsicRegistry>,
    pub imported_registry: Arc<ImportedRegistryDb>,
    pub declarations: Arc<DeclarationLookupDb>,
    pub resolvability: Arc<ResolvabilityDb>,
    pub owner_collections: Arc<OwnerCollectionDb>,
    pub shapes: Arc<ShapeCacheDb>,
    pub identities: Arc<crate::identity_interner::IdentityInterner>,
    pub mapper_binders: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
}

impl EngineStores {
    /// Construct one engine's stores, the output authority bound to them, and
    /// the engine's surface-claim authority.
    ///
    /// `store_account` is the retention account every retaining store charges;
    /// `cache_live` is the live-entry counter the single-entry memo stores
    /// share; `provenance` instruments the semantic graph when present;
    /// `task_registry` is the cycle authority the graph's producers register
    /// with, minted by the caller's composition root.
    #[must_use]
    pub fn create(
        provenance: Option<Arc<crate::engine_provenance::EngineProvenance>>,
        store_account: verter_session_query::retention::StoreAccount,
        cache_live: &Arc<AtomicU64>,
        task_registry: verter_execution::tasks::TaskRegistry,
    ) -> (Self, OutputAuthority, SurfaceClaimAuthority) {
        Self::create_with_memo_family_bound(
            provenance,
            store_account,
            cache_live,
            task_registry,
            MemoFamilyBound::Production,
            Default::default(),
        )
    }

    /// [`Self::create`] with the semantic graph's family memo bounded by
    /// `memo_family_bound` and its node slots in `ownership`'s reclamation
    /// regime at construction.
    #[must_use]
    pub fn create_with_memo_family_bound(
        provenance: Option<Arc<crate::engine_provenance::EngineProvenance>>,
        store_account: verter_session_query::retention::StoreAccount,
        cache_live: &Arc<AtomicU64>,
        task_registry: verter_execution::tasks::TaskRegistry,
        memo_family_bound: MemoFamilyBound,
        ownership: crate::semantic_query_memo::GraphOwnership,
    ) -> (Self, OutputAuthority, SurfaceClaimAuthority) {
        let retention_account = Arc::clone(store_account.get());
        let graph = Arc::new(
            match provenance {
                Some(prov) => SemanticGraphStore::with_provenance_and_ownership(
                    prov,
                    store_account,
                    ownership,
                ),
                None => SemanticGraphStore::with_account_and_ownership(store_account, ownership),
            }
            .with_task_registry(task_registry)
            .with_memo_family_bound(memo_family_bound),
        );
        let authority = OutputAuthority {
            engine: EngineIdentity(Arc::downgrade(&graph)),
        };
        let stores = Self {
            construction: crate::semantic_query_memo::node_roots::NodeConstructionGrant::mint(
                &graph,
            ),
            graph,
            flow_slice: Arc::new(crate::cache_runtime::flow_slice_node::FlowSliceStores::new()),
            intrinsics: Arc::new(crate::intrinsic_registry::IntrinsicRegistry::with_defaults()),
            imported_registry: Arc::new(ImportedRegistryDb::with_counter(Arc::clone(cache_live))),
            declarations: Arc::new(DeclarationLookupDb::with_counter(Arc::clone(cache_live))),
            resolvability: Arc::new(ResolvabilityDb::with_counter(Arc::clone(cache_live))),
            owner_collections: Arc::new(OwnerCollectionDb::with_counter(Arc::clone(cache_live))),
            shapes: Arc::new(ShapeCacheDb::with_counter(Arc::clone(cache_live))),
            identities: Arc::new(crate::identity_interner::IdentityInterner::new(
                retention_account,
            )),
            mapper_binders: Arc::new(crate::mapper_binder_registry::MapperBinderRegistry::new()),
        };
        (stores, authority, SurfaceClaimAuthority::mint())
    }
}

/// The engine-owned authority to turn a graph node back into a published
/// [`verter_type_expr::TypeExpr`].
///
/// Non-forgeable: its field is private to this module, it has no public
/// constructor, and it is neither `Clone`, `Copy` nor `Default`. The only mint
/// is [`EngineStores::create`]. It is bound to the semantic graph of the
/// engine it was minted with; materializing through a dispatch over any other
/// engine panics (a composition defect, never a silent cross-engine raise).
///
/// Every sealed output carrier unwraps only with a borrowed authority, so the
/// reverse boundary stays closed to every holder of query access. Each carrier
/// is stamped with the identity of the engine that produced it, and unwrapping
/// with the authority of any other engine panics: minting a fresh engine never
/// yields output power over a live one.
pub struct OutputAuthority {
    engine: EngineIdentity,
}

/// The private identity of one engine: a non-owning handle on its semantic
/// graph, compared by allocation. Sealed carriers carry it so their unwrap can
/// be checked against the authority; holding one grants nothing.
#[derive(Clone)]
pub(crate) struct EngineIdentity(Weak<SemanticGraphStore>);

impl EngineIdentity {
    pub(super) fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> crate::semantic_query_memo::node_roots::IdentityNodeRoots {
        let Some(graph) = self.0.upgrade() else {
            // Unbound fixtures have only their explicit construction scope.
            return crate::semantic_query_memo::node_roots::IdentityNodeRoots::scoped(ids);
        };
        crate::semantic_query_memo::node_roots::IdentityNodeRoots(
            graph
                .root_existing_nodes(
                    ids.into_iter()
                        .filter(|node| node.0 < crate::semantic_query_memo::UNALLOCATABLE_ID_FLOOR)
                        .collect(),
                )
                .expect("output holder operands belong to its issuing engine"),
        )
    }

    /// The identity of the engine `dispatch` reads.
    pub(super) fn of<C: crate::resolver_core::ResolverCapabilities>(
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Self {
        Self(Arc::downgrade(&dispatch.binding.graph))
    }

    /// An identity no engine has: a carrier stamped with it unwraps under no
    /// authority. Used only by test fixtures that assemble carriers without an
    /// engine.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn unbound() -> Self {
        Self(Weak::new())
    }
}

impl OutputAuthority {
    pub(crate) fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> crate::semantic_query_memo::node_roots::IdentityNodeRoots {
        self.engine.root_nodes(ids)
    }

    /// Assert that `dispatch` reads the engine this authority was minted for.
    /// One pointer comparison; no allocation.
    #[inline]
    fn bind<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) {
        assert!(
            std::ptr::eq(self.engine.0.as_ptr(), Arc::as_ptr(&dispatch.binding.graph)),
            "output authority used with a dispatch over a different engine"
        );
    }

    /// Assert that a sealed carrier stamped with `carrier` was produced by the
    /// engine this authority was minted for. Checked on EVERY unwrap; one
    /// pointer comparison.
    #[inline]
    pub(super) fn verify_carrier(&self, carrier: &EngineIdentity) {
        assert!(
            Weak::ptr_eq(&self.engine.0, &carrier.0),
            "output authority used to unwrap a carrier from a different engine"
        );
    }

    /// The identity a carrier this authority seals is stamped with.
    pub(super) fn engine(&self) -> &EngineIdentity {
        &self.engine
    }

    /// Plain SHELL raise (no operator reduction): materialize `node` into a
    /// sealed [`OutputTypeExpr`]. `None` is the miss signal (the node — or a
    /// node required while raising it — is unavailable from the live graph
    /// store); a `None` result is noted as an `OutputMaterializationLoss`
    /// NON-CACHEABLE read BEFORE it is returned (a torn read is never
    /// warm-admitted as a complete raise, and never faults the enclosing
    /// compute's completeness).
    pub fn materialize_output_type_expr<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
        node: SemanticNodeId,
    ) -> Option<OutputTypeExpr> {
        self.bind(dispatch);
        let raised = dispatch.output_shell_raise_sealed(node);
        if raised.is_none() {
            // The note is UNCONDITIONAL — it also fires for a genuinely-absent
            // id (a real absence, not degradation), costing warm hits on that
            // class (fail-closed direction chosen deliberately).
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::OutputMaterializationLoss,
            );
        }
        raised
    }

    /// REDUCE-then-raise: apply the supplied projection reduction context,
    /// then materialize the reduced node into a sealed
    /// [`MaterializedOutputTypeExpr`] (the producing reduced `node_id`, the
    /// sealed `type_expr` payload, the accumulated `dep_signature`, and the
    /// `result_is_partial` flag).
    pub fn materialize_reduced_output_type_expr<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> MaterializedOutputTypeExpr {
        self.bind(dispatch);
        dispatch.raise_and_reduce_with_context(node, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_semantic_dispatch::output_materialization::wrap_output_type_expr;
    use crate::semantic_query::{
        LiteralValue, ProjectionMode, QueryResult, SemanticNodeData, SemanticQueryValue,
        SurfaceMember,
    };
    use crate::semantic_query_memo::ownership_harness::CountedOwnershipHarness;

    #[test]
    fn counted_shape_holders_acquire_after_nested_scope_returns() {
        for drop_output_first in [false, true] {
            let graph = Arc::new(CountedOwnershipHarness::new().into_graph());
            let authority = OutputAuthority {
                engine: EngineIdentity(Arc::downgrade(&graph)),
            };
            let (node, delivery) = {
                let _scope = graph.enter_node_root_scope();
                let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "retained member".into(),
                )));
                let delivery = graph
                    .lease_delivery(&QueryResult::Value(SemanticQueryValue::TypeNode(node)), &[])
                    .unwrap();
                (node, delivery)
            };
            assert_eq!(
                crate::semantic_query_memo::node_roots::NodeRootScope::current_arena_identity(),
                None
            );
            let output = MaterializedOutputTypeExpr::from_parts(
                Some(node),
                wrap_output_type_expr(
                    &authority,
                    verter_type_expr::TypeExpr::string_literal("retained member"),
                ),
                Arc::from([]),
                false,
            );
            let member = SurfaceMember {
                key: verter_type_expr::AuthoredPropertyKey::string("member"),
                value: node,
                optional: false,
                readonly: false,
                method_kind: None,
                has_implementation_body: false,
                visibility: verter_type_expr::MemberVisibility::Public,
                spans: Default::default(),
                declaration_origin: None,
                declared_in_macro_type_arg: Default::default(),
                merge_role: Default::default(),
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            };
            let key = crate::component_meta_caches::ShapeCacheKey::surface_member_value_whole_with_context(
                &authority,
                Arc::from("/member.vue"),
                &member,
                ProjectionReductionContext::published(ProjectionMode::Shallow),
            );
            drop(delivery);
            assert!(
                graph.node_is_live(node),
                "holders assembled outside the scope own the returned node"
            );
            assert_eq!(graph.node_ownership_counts().roots, 2);
            if drop_output_first {
                drop(output);
                assert!(
                    graph.node_is_live(node),
                    "the shape key owns its subject independently"
                );
                drop(key);
            } else {
                drop(key);
                assert!(
                    graph.node_is_live(node),
                    "the output owns its metadata node independently"
                );
                drop(output);
            }
            assert_eq!(graph.node_ownership_counts().live_nodes, 0);
        }
    }
}
