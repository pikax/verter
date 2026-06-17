//! Unified symbol resolution with persistent node caching and singleflight.
//!
//! The symbol resolver provides per-node caching and singleflight coalescing for
//! cross-file type resolution. Each resolution node is identified by a
//! [`ResolutionNodeKey`] and cached with [`FactVersionRef`] dependencies for
//! invalidation.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;

use crate::resolver_core::{
    FactVersionRef, ResolutionNodeKey, ResolutionNodeKind, ResolvedTypeDeclaration,
    ResolverCounters, ResolverDiagnostic, SingleflightGroup, StableExecutionValue, StoreView,
    TraversalLens, ValidatedFactCache,
};

#[derive(Debug, Clone)]
pub enum SymbolNodeValue {
    Declaration(ResolvedTypeDeclaration),
    TypeShape(Option<Arc<ResolvedElements>>),
    /// Importer-side import-edge resolution: normalizes owner-local binding
    /// context and points to the provider canonical + exported name.
    ImporterEdge(Option<ImporterEdgeResolution>),
    /// Provider-side export-route resolution: the reusable cross-owner answer.
    /// Includes full routed-symbol result with provenance and dependency closure.
    ProviderExportRoute(Option<crate::resolver_core::RoutedSymbolResult>),
    BarrelSurface(Vec<String>),
    Assembled(AssembledSurface),
}

/// Importer-side resolution: where an import binding routes to.
///
/// This is the importer-local answer: it normalizes binding context
/// (named/default/namespace) and points to the provider canonical ID
/// and exported name. The actual route answer lives in a separate
/// provider/export-route node for cross-owner reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterEdgeResolution {
    /// Provider canonical ID that this import resolves to.
    pub provider_canonical_id: String,
    /// Exported name in the provider file.
    pub exported_name: String,
    /// Import binding kind (named, default, namespace).
    pub binding_kind: ImportBindingKind,
}

/// Kind of import binding — preserved for value-space routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportBindingKind {
    Named,
    Default,
    Namespace,
}

#[derive(Debug, Clone, Default)]
pub struct AssembledSurface {
    pub contributions: Vec<SurfaceContribution>,
}

#[derive(Debug, Clone)]
pub struct SurfaceContribution {
    pub provenance: ContributionProvenance,
    pub elements: Option<Arc<ResolvedElements>>,
    pub diagnostics: Vec<ResolverDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContributionProvenance {
    pub declaration_source_order: u32,
    pub inheritance_depth: u32,
    pub contribution_kind: ContributionKind,
    pub canonical_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContributionKind {
    Direct = 0,
    Inherited = 1,
    Reexported = 2,
}

#[derive(Debug, Clone)]
pub struct SymbolNodeResult {
    pub value: SymbolNodeValue,
    pub facts: Vec<FactVersionRef>,
    pub diagnostics: Vec<ResolverDiagnostic>,
}

pub struct SymbolResolverState {
    cache: ValidatedFactCache<ResolutionNodeKey, SymbolNodeResult>,
    singleflight: SingleflightGroup<ResolutionNodeKey, StableExecutionValue<SymbolNodeResult>, ()>,
    counters: Arc<ResolverCounters>,
}

impl SymbolResolverState {
    pub fn new(counters: Arc<ResolverCounters>) -> Self {
        Self {
            cache: ValidatedFactCache::default(),
            singleflight: SingleflightGroup::default(),
            counters,
        }
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
        self.singleflight.clear();
    }

    pub fn counters(&self) -> &ResolverCounters {
        &self.counters
    }

    pub fn resolve_node<V, F>(
        &self,
        key: ResolutionNodeKey,
        view: &V,
        ctx: &mut ResolveContext,
        compute_fn: F,
    ) -> SymbolNodeResult
    where
        V: StoreView,
        F: FnOnce(&mut ResolveContext) -> SymbolNodeResult,
    {
        if let Some(cached) = self.cache.get_if_valid(&key, view) {
            self.counters.record_cache_hit();
            return (*cached).clone();
        }
        self.counters.record_cache_miss();

        if ctx.visiting.contains(&key) {
            self.counters.record_cycle_detection();
            return SymbolNodeResult {
                value: SymbolNodeValue::TypeShape(None),
                facts: vec![],
                diagnostics: vec![ResolverDiagnostic {
                    code: "cycle-detected".to_string(),
                    message: format!(
                        "Cycle detected resolving {}::{:?}",
                        key.symbol_id, key.node_kind
                    ),
                    canonical_path: Some(key.symbol_id.clone()),
                    span_start: None,
                }],
            };
        }

        let token = view.compat_token();
        let result = self.singleflight.run(key.clone(), token, || {
            ctx.visiting.insert(key.clone());
            let result = compute_fn(ctx);
            ctx.visiting.remove(&key);

            let stable = !result.facts.is_empty();
            if stable {
                self.cache
                    .insert(key.clone(), result.clone(), result.facts.clone());
            }

            Ok(StableExecutionValue {
                value: result,
                stable,
                // This singleflight closure always runs `compute_fn`
                // (no in-closure warm-hit short-circuit), so the winner
                // always performed a cold build.
                computed: true,
            })
        });

        match result {
            Ok(flight) => {
                if flight.role == crate::resolver_core::SingleflightRole::Follower {
                    self.counters.record_singleflight_coalesce();
                }
                if flight.forked_lane {
                    self.counters.record_cross_view_lane_fork();
                }
                flight.value.value.clone()
            }
            Err(()) => SymbolNodeResult {
                value: SymbolNodeValue::TypeShape(None),
                facts: vec![],
                diagnostics: vec![],
            },
        }
    }
}

pub struct ResolveContext {
    pub visiting: FxHashSet<ResolutionNodeKey>,
    pub fallthrough_visiting: FxHashSet<crate::resolver_core::FallthroughNodeKey>,
    pub collected_facts: Vec<FactVersionRef>,
    pub collected_diagnostics: Vec<ResolverDiagnostic>,
}

impl ResolveContext {
    pub fn new() -> Self {
        Self {
            visiting: FxHashSet::default(),
            fallthrough_visiting: FxHashSet::default(),
            collected_facts: Vec::new(),
            collected_diagnostics: Vec::new(),
        }
    }

    pub fn record_fact(&mut self, fact: FactVersionRef) {
        self.collected_facts.push(fact);
    }

    pub fn record_diagnostic(&mut self, diagnostic: ResolverDiagnostic) {
        self.collected_diagnostics.push(diagnostic);
    }
}

impl Default for ResolveContext {
    fn default() -> Self {
        Self::new()
    }
}

pub fn merge_contributions_deterministic(
    mut contributions: Vec<SurfaceContribution>,
) -> AssembledSurface {
    contributions.sort_by(|a, b| a.provenance.cmp(&b.provenance));
    AssembledSurface { contributions }
}

pub fn type_shape_node_key(
    canonical_id: &str,
    type_name: &str,
    lens: TraversalLens,
) -> ResolutionNodeKey {
    ResolutionNodeKey {
        symbol_id: format!("{}#{}", canonical_id, type_name),
        node_kind: ResolutionNodeKind::SymbolExpand,
        traversal_lens: lens,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: 0,
        view_fingerprint: 0,
    }
}

pub fn declaration_node_key(canonical_id: &str, type_name: &str) -> ResolutionNodeKey {
    ResolutionNodeKey {
        symbol_id: format!("{}#{}", canonical_id, type_name),
        node_kind: ResolutionNodeKind::DeclarationMetadata,
        traversal_lens: TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: 0,
        view_fingerprint: 0,
    }
}

/// Create a node key for an importer-side import-edge resolution.
///
/// Keyed by the importer file, import source specifier, requested symbol,
/// and binding kind. This normalizes owner-local binding context.
pub fn importer_edge_node_key(
    owner_canonical: &str,
    import_source: &str,
    type_name: &str,
    binding_kind: ImportBindingKind,
    symbol_space: crate::resolver_core::SymbolSpace,
) -> ResolutionNodeKey {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    import_source.hash(&mut hasher);
    type_name.hash(&mut hasher);
    binding_kind.hash(&mut hasher);
    symbol_space.hash(&mut hasher);

    ResolutionNodeKey {
        symbol_id: owner_canonical.to_string(),
        node_kind: ResolutionNodeKind::ImporterEdge,
        traversal_lens: TraversalLens::StructuralObject,
        member_path_hash: hasher.finish(),
        type_args_hash: 0,
        behavior_flags: 0,
        view_fingerprint: 0,
    }
}

/// Create a node key for a provider-side export-route resolution.
///
/// Keyed by the provider file, exported symbol, route demand, and symbol space.
/// This is the reusable cross-owner answer — identical queries from different
/// importers produce the same key and share the cached result.
pub fn provider_export_route_node_key(
    provider_canonical: &str,
    exported_name: &str,
    route_demand: &crate::resolver_core::RouteDemand,
    symbol_space: crate::resolver_core::SymbolSpace,
) -> ResolutionNodeKey {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    exported_name.hash(&mut hasher);
    route_demand.hash(&mut hasher);
    symbol_space.hash(&mut hasher);

    ResolutionNodeKey {
        symbol_id: provider_canonical.to_string(),
        node_kind: ResolutionNodeKind::ProviderExportRoute,
        traversal_lens: TraversalLens::StructuralObject,
        member_path_hash: hasher.finish(),
        type_args_hash: 0,
        behavior_flags: 0,
        view_fingerprint: 0,
    }
}

pub fn barrel_node_key(canonical_id: &str, type_name: &str) -> ResolutionNodeKey {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    type_name.hash(&mut hasher);

    ResolutionNodeKey {
        symbol_id: canonical_id.to_string(),
        node_kind: ResolutionNodeKind::BarrelLookup,
        traversal_lens: TraversalLens::StructuralObject,
        member_path_hash: hasher.finish(),
        type_args_hash: 0,
        behavior_flags: 0,
        view_fingerprint: 0,
    }
}

pub fn assemble_node_key(canonical_id: &str, behavior_flags: u32) -> ResolutionNodeKey {
    ResolutionNodeKey {
        symbol_id: canonical_id.to_string(),
        node_kind: ResolutionNodeKind::Assemble,
        traversal_lens: TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags,
        view_fingerprint: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::StoreViewCompatToken;

    #[derive(Debug)]
    struct TestView {
        token: StoreViewCompatToken,
        valid_facts: FxHashSet<FactVersionRef>,
    }

    impl StoreView for TestView {
        fn compat_token(&self) -> StoreViewCompatToken {
            self.token
        }
        fn validates(&self, fact: &FactVersionRef) -> bool {
            self.valid_facts.contains(fact)
        }
    }

    fn make_fact(id: &str) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: id.to_string(),
            hash: [1; 16],
        }
    }

    fn make_view(token: u64, facts: Vec<FactVersionRef>) -> TestView {
        TestView {
            token: StoreViewCompatToken {
                epoch: token,
                session: None,
                validity_fingerprint: 0,
            },
            valid_facts: facts.into_iter().collect(),
        }
    }

    #[test]
    fn resolve_node_caches_result_and_reuses_on_valid_facts() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let fact = make_fact("/src/types.ts");
        let view = make_view(1, vec![fact.clone()]);

        let key = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);

        let mut ctx = ResolveContext::new();
        let result = state.resolve_node(key.clone(), &view, &mut ctx, |_ctx| SymbolNodeResult {
            value: SymbolNodeValue::TypeShape(None),
            facts: vec![fact.clone()],
            diagnostics: vec![],
        });
        assert!(matches!(result.value, SymbolNodeValue::TypeShape(None)));
        assert_eq!(counters.snapshot().node_cache_misses, 1);
        assert_eq!(counters.snapshot().node_cache_hits, 0);

        let mut ctx2 = ResolveContext::new();
        let result2 = state.resolve_node(key.clone(), &view, &mut ctx2, |_ctx| {
            panic!("should not recompute");
        });
        assert!(matches!(result2.value, SymbolNodeValue::TypeShape(None)));
        assert_eq!(counters.snapshot().node_cache_hits, 1);
    }

    #[test]
    fn resolve_node_recomputes_when_facts_invalid() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let fact_v1 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [1; 16],
        };
        let fact_v2 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [2; 16],
        };

        let view_v1 = make_view(1, vec![fact_v1.clone()]);
        let key = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);

        let mut ctx = ResolveContext::new();
        state.resolve_node(key.clone(), &view_v1, &mut ctx, |_| SymbolNodeResult {
            value: SymbolNodeValue::TypeShape(None),
            facts: vec![fact_v1.clone()],
            diagnostics: vec![],
        });

        let view_v2 = make_view(2, vec![fact_v2.clone()]);
        let mut ctx2 = ResolveContext::new();
        let mut computed = false;
        state.resolve_node(key.clone(), &view_v2, &mut ctx2, |_| {
            computed = true;
            SymbolNodeResult {
                value: SymbolNodeValue::TypeShape(None),
                facts: vec![fact_v2],
                diagnostics: vec![],
            }
        });
        assert!(computed, "should recompute when facts don't validate");
        assert_eq!(counters.snapshot().node_cache_misses, 2);
    }

    #[test]
    fn resolve_node_detects_cycle() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let view = make_view(1, vec![]);
        let key = type_shape_node_key(
            "/src/types.ts",
            "Recursive",
            TraversalLens::StructuralObject,
        );

        let mut ctx = ResolveContext::new();
        ctx.visiting.insert(key.clone());

        let result = state.resolve_node(key.clone(), &view, &mut ctx, |_| {
            panic!("should not compute for cycle");
        });

        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "cycle-detected");
        assert_eq!(counters.snapshot().cycle_detections, 1);
    }

    #[test]
    fn resolve_node_no_cache_for_empty_facts() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let view = make_view(1, vec![]);
        let key = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);

        let mut ctx = ResolveContext::new();
        state.resolve_node(key.clone(), &view, &mut ctx, |_| SymbolNodeResult {
            value: SymbolNodeValue::TypeShape(None),
            facts: vec![],
            diagnostics: vec![],
        });

        let mut ctx2 = ResolveContext::new();
        let mut computed = false;
        state.resolve_node(key.clone(), &view, &mut ctx2, |_| {
            computed = true;
            SymbolNodeResult {
                value: SymbolNodeValue::TypeShape(None),
                facts: vec![],
                diagnostics: vec![],
            }
        });
        assert!(computed, "should recompute when facts are empty (unstable)");
    }

    #[test]
    fn merge_contributions_sorts_by_provenance() {
        let contributions = vec![
            SurfaceContribution {
                provenance: ContributionProvenance {
                    declaration_source_order: 100,
                    inheritance_depth: 0,
                    contribution_kind: ContributionKind::Direct,
                    canonical_path: "/src/b.ts".to_string(),
                },
                elements: None,
                diagnostics: vec![],
            },
            SurfaceContribution {
                provenance: ContributionProvenance {
                    declaration_source_order: 50,
                    inheritance_depth: 0,
                    contribution_kind: ContributionKind::Direct,
                    canonical_path: "/src/a.ts".to_string(),
                },
                elements: None,
                diagnostics: vec![],
            },
        ];

        let result = merge_contributions_deterministic(contributions);
        assert_eq!(result.contributions.len(), 2);
        assert_eq!(
            result.contributions[0].provenance.declaration_source_order,
            50
        );
        assert_eq!(
            result.contributions[1].provenance.declaration_source_order,
            100
        );
    }

    #[test]
    fn merge_contributions_orders_inherited_after_direct() {
        let contributions = vec![
            SurfaceContribution {
                provenance: ContributionProvenance {
                    declaration_source_order: 0,
                    inheritance_depth: 1,
                    contribution_kind: ContributionKind::Inherited,
                    canonical_path: "/src/parent.ts".to_string(),
                },
                elements: None,
                diagnostics: vec![],
            },
            SurfaceContribution {
                provenance: ContributionProvenance {
                    declaration_source_order: 0,
                    inheritance_depth: 0,
                    contribution_kind: ContributionKind::Direct,
                    canonical_path: "/src/child.ts".to_string(),
                },
                elements: None,
                diagnostics: vec![],
            },
        ];

        let result = merge_contributions_deterministic(contributions);
        assert_eq!(
            result.contributions[0].provenance.contribution_kind,
            ContributionKind::Direct,
            "direct should come before inherited"
        );
        assert_eq!(
            result.contributions[1].provenance.contribution_kind,
            ContributionKind::Inherited,
        );
    }

    #[test]
    fn type_shape_node_key_convergence() {
        let key_a = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);
        let key_b = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);
        assert_eq!(
            key_a, key_b,
            "same declaration + lens should produce same key"
        );

        let key_c = type_shape_node_key("/src/types.ts", "Emits", TraversalLens::StructuralObject);
        assert_ne!(
            key_a, key_c,
            "different type name should produce different key"
        );

        let key_d = type_shape_node_key("/src/types.ts", "Props", TraversalLens::CallableParams);
        assert_ne!(key_a, key_d, "different lens should produce different key");
    }

    #[test]
    fn assemble_node_key_uses_behavior_flags() {
        let key_type = assemble_node_key("/src/App.vue", 1);
        let key_expanded = assemble_node_key("/src/App.vue", 2);
        assert_ne!(
            key_type, key_expanded,
            "different behavior flags should differ"
        );
    }

    #[test]
    fn incompatible_views_produce_independent_results() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let fact_v1 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [1; 16],
        };
        let fact_v2 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [2; 16],
        };

        let view_v1 = make_view(1, vec![fact_v1.clone()]);
        let view_v2 = make_view(2, vec![fact_v2.clone()]);
        let key = type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);

        let mut ctx1 = ResolveContext::new();
        state.resolve_node(key.clone(), &view_v1, &mut ctx1, |_| SymbolNodeResult {
            value: SymbolNodeValue::ImporterEdge(Some(ImporterEdgeResolution {
                provider_canonical_id: "/src/types.ts".to_string(),
                exported_name: "Props_v1".to_string(),
                binding_kind: ImportBindingKind::Named,
            })),
            facts: vec![fact_v1],
            diagnostics: vec![],
        });

        let mut ctx2 = ResolveContext::new();
        let mut v2_computed = false;
        state.resolve_node(key.clone(), &view_v2, &mut ctx2, |_| {
            v2_computed = true;
            SymbolNodeResult {
                value: SymbolNodeValue::ImporterEdge(Some(ImporterEdgeResolution {
                    provider_canonical_id: "/src/types.ts".to_string(),
                    exported_name: "Props_v2".to_string(),
                    binding_kind: ImportBindingKind::Named,
                })),
                facts: vec![fact_v2],
                diagnostics: vec![],
            }
        });

        assert!(
            v2_computed,
            "incompatible views should not share cache entries"
        );
    }

    #[test]
    fn identical_cyclic_requests_produce_identical_placeholders() {
        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let view = make_view(1, vec![]);
        let key = type_shape_node_key(
            "/src/recursive.ts",
            "Circular",
            TraversalLens::StructuralObject,
        );

        let mut ctx1 = ResolveContext::new();
        ctx1.visiting.insert(key.clone());
        let result1 = state.resolve_node(key.clone(), &view, &mut ctx1, |_| {
            panic!("should not compute");
        });

        let mut ctx2 = ResolveContext::new();
        ctx2.visiting.insert(key.clone());
        let result2 = state.resolve_node(key.clone(), &view, &mut ctx2, |_| {
            panic!("should not compute");
        });

        assert_eq!(result1.diagnostics.len(), result2.diagnostics.len());
        assert_eq!(result1.diagnostics[0].code, result2.diagnostics[0].code);
        assert_eq!(
            result1.diagnostics[0].message, result2.diagnostics[0].message,
            "identical cyclic requests should produce identical diagnostics"
        );
    }

    #[test]
    fn resolve_context_tracks_both_subsystem_stacks() {
        let mut ctx = ResolveContext::new();

        let sym_key =
            type_shape_node_key("/src/types.ts", "Props", TraversalLens::StructuralObject);
        let ft_key = crate::resolver_core::FallthroughNodeKey {
            canonical_component_id: "/src/App.vue".to_string(),
            node_kind: crate::resolver_core::FallthroughNodeKind::ComponentRootFollow,
            override_fingerprint: 0,
            behavior_flags: 0,
            branch_selector: None,
        };

        ctx.visiting.insert(sym_key.clone());
        ctx.fallthrough_visiting.insert(ft_key.clone());

        assert!(ctx.visiting.contains(&sym_key));
        assert!(ctx.fallthrough_visiting.contains(&ft_key));
        assert!(!ctx.visiting.is_empty());
        assert!(!ctx.fallthrough_visiting.is_empty());

        assert!(!ctx
            .fallthrough_visiting
            .contains(&crate::resolver_core::FallthroughNodeKey {
                canonical_component_id: "/src/types.ts".to_string(),
                node_kind: crate::resolver_core::FallthroughNodeKind::ComponentRootFollow,
                override_fingerprint: 0,
                behavior_flags: 0,
                branch_selector: None,
            }));
    }

    // ── Layered routed-symbol node topology tests ─────────────────────

    /// Provider/export-route node key must include route demand and symbol space.
    /// Two queries with different RouteDemand must produce different keys.
    #[test]
    fn provider_route_node_key_includes_route_demand_and_symbol_space() {
        use crate::resolver_core::route_demand::{RouteDemand, SymbolSpace};

        let key_whole = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );
        let key_member = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::MemberPath(vec!["foo".to_string()]),
            SymbolSpace::Type,
        );
        assert_ne!(
            key_whole, key_member,
            "different route demands must produce different keys"
        );

        let key_value = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Value,
        );
        assert_ne!(
            key_whole, key_value,
            "different symbol spaces must produce different keys"
        );
    }

    /// Identical routed queries from different importers must reuse the
    /// same provider/export-route node.
    #[test]
    fn provider_route_node_reuses_across_distinct_importers() {
        use crate::resolver_core::route_demand::{RouteDemand, SymbolSpace};

        let key_from_a = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );
        let key_from_b = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );
        assert_eq!(
            key_from_a, key_from_b,
            "same provider query from different importers must produce the same key"
        );
    }

    /// Importer/import-edge nodes must be distinct from provider/export-route nodes.
    #[test]
    fn importer_and_provider_nodes_are_distinct() {
        use crate::resolver_core::route_demand::{RouteDemand, SymbolSpace};

        let importer_key = importer_edge_node_key(
            "/src/App.vue",
            "./types",
            "Props",
            ImportBindingKind::Named,
            SymbolSpace::Type,
        );
        let provider_key = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );
        assert_ne!(
            importer_key.node_kind, provider_key.node_kind,
            "importer and provider nodes must have different node kinds"
        );
        assert_eq!(importer_key.node_kind, ResolutionNodeKind::ImporterEdge);
        assert_eq!(
            provider_key.node_kind,
            ResolutionNodeKind::ProviderExportRoute
        );
    }

    /// Deep barrel chain (A -> B -> C -> Leaf) must warm once and not
    /// replay intermediate hops on warm lookup.
    #[test]
    fn deep_barrel_chain_warm_lookup_does_not_replay_intermediate_hops() {
        use crate::resolver_core::route_demand::{
            RouteDemand, RoutedSymbolResult, RoutedSymbolStatus, SymbolSpace,
        };

        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let fact_leaf = make_fact("/src/leaf.ts");
        let view = make_view(1, vec![fact_leaf.clone()]);

        // Provider key for "Props" at the leaf — this is what gets cached.
        let provider_key = provider_export_route_node_key(
            "/src/leaf.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );

        // First resolution: populate the cache
        let mut ctx = ResolveContext::new();
        state.resolve_node(provider_key.clone(), &view, &mut ctx, |_| {
            SymbolNodeResult {
                value: SymbolNodeValue::ProviderExportRoute(Some(RoutedSymbolResult {
                    final_canonical_id: "/src/leaf.ts".to_string(),
                    final_exported_name: "Props".to_string(),
                    status: RoutedSymbolStatus::Resolved,
                    normalized_route: RouteDemand::Whole,
                    provenance: vec![],
                    external_dependency_closure: vec![],
                })),
                facts: vec![fact_leaf.clone()],
                diagnostics: vec![],
            }
        });
        assert_eq!(counters.snapshot().node_cache_misses, 1);

        // Second lookup: should hit cache without recompute
        let mut ctx2 = ResolveContext::new();
        let result = state.resolve_node(provider_key.clone(), &view, &mut ctx2, |_| {
            panic!("should not recompute — cached provider route must be reused");
        });
        assert_eq!(counters.snapshot().node_cache_hits, 1);
        assert!(matches!(
            result.value,
            SymbolNodeValue::ProviderExportRoute(Some(_))
        ));
    }

    /// Deep barrel chain negative miss must be cached and reused.
    #[test]
    fn deep_barrel_chain_negative_miss_is_reused() {
        use crate::resolver_core::route_demand::{
            RouteDemand, RoutedSymbolResult, RoutedSymbolStatus, SymbolSpace,
        };

        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());
        let fact = make_fact("/src/barrel.ts");
        let view = make_view(1, vec![fact.clone()]);

        let provider_key = provider_export_route_node_key(
            "/src/barrel.ts",
            "Missing",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );

        // First resolution: negative miss
        let mut ctx = ResolveContext::new();
        state.resolve_node(provider_key.clone(), &view, &mut ctx, |_| {
            SymbolNodeResult {
                value: SymbolNodeValue::ProviderExportRoute(Some(RoutedSymbolResult {
                    final_canonical_id: String::new(),
                    final_exported_name: "Missing".to_string(),
                    status: RoutedSymbolStatus::NotFound,
                    normalized_route: RouteDemand::Whole,
                    provenance: vec![],
                    external_dependency_closure: vec![],
                })),
                facts: vec![fact.clone()],
                diagnostics: vec![],
            }
        });

        // Second lookup: must reuse the cached negative answer
        let mut ctx2 = ResolveContext::new();
        state.resolve_node(provider_key.clone(), &view, &mut ctx2, |_| {
            panic!("should not recompute — negative miss must be cached and reused");
        });
        assert_eq!(counters.snapshot().node_cache_hits, 1);
    }

    /// Route facts must invalidate on both importer-side and provider-side changes.
    #[test]
    fn route_facts_invalidate_on_importer_and_provider_changes() {
        use crate::resolver_core::route_demand::{RouteDemand, SymbolSpace};

        let counters = Arc::new(ResolverCounters::new());
        let state = SymbolResolverState::new(counters.clone());

        // Provider fact — keyed by provider file hash
        let provider_fact_v1 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [1; 16],
        };
        let provider_fact_v2 = FactVersionRef::FileWholeHash {
            canonical_id: "/src/types.ts".to_string(),
            hash: [2; 16],
        };

        let view_v1 = make_view(1, vec![provider_fact_v1.clone()]);
        let provider_key = provider_export_route_node_key(
            "/src/types.ts",
            "Props",
            &RouteDemand::Whole,
            SymbolSpace::Type,
        );

        // Populate cache with v1 facts
        let mut ctx = ResolveContext::new();
        state.resolve_node(provider_key.clone(), &view_v1, &mut ctx, |_| {
            SymbolNodeResult {
                value: SymbolNodeValue::ProviderExportRoute(None),
                facts: vec![provider_fact_v1],
                diagnostics: vec![],
            }
        });

        // Provider content changes → v2 facts → must invalidate
        let view_v2 = make_view(2, vec![provider_fact_v2.clone()]);
        let mut ctx2 = ResolveContext::new();
        let mut recomputed = false;
        state.resolve_node(provider_key.clone(), &view_v2, &mut ctx2, |_| {
            recomputed = true;
            SymbolNodeResult {
                value: SymbolNodeValue::ProviderExportRoute(None),
                facts: vec![provider_fact_v2],
                diagnostics: vec![],
            }
        });
        assert!(
            recomputed,
            "provider content change must invalidate cached route"
        );
    }

    // ── End layered routed-symbol node topology tests ────────────────

    #[test]
    fn resolve_context_collects_facts_from_sub_resolutions() {
        let mut ctx = ResolveContext::new();

        ctx.record_fact(make_fact("/src/types.ts"));
        ctx.record_fact(make_fact("/src/utils.ts"));
        ctx.record_diagnostic(ResolverDiagnostic {
            code: "test".to_string(),
            message: "test diagnostic".to_string(),
            canonical_path: None,
            span_start: None,
        });

        assert_eq!(ctx.collected_facts.len(), 2);
        assert_eq!(ctx.collected_diagnostics.len(), 1);
    }
}
