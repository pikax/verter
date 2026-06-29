use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

pub(crate) mod ambient_resolve;
pub(crate) mod bare_name_resolve;
pub(crate) mod component_meta;
pub mod component_meta_query_engine;
pub mod component_meta_registry;
mod component_meta_request;
mod declaration_metadata;
mod export_graph;
mod external_macro_types;
mod external_type_body;
pub mod external_type_frontier;
mod fallthrough;
pub mod fallthrough_override_key;
mod fallthrough_request;
pub mod fallthrough_resolver;
pub mod hot_prepared;
pub mod prepared_decl;
pub mod resolver_runtime;
pub mod route_demand;
mod runtime_values;
pub mod shallow_file_state;
pub(crate) mod structural_body_memo;
pub mod structural_body_memo_instrumentation;
pub(crate) mod surface_projector;
#[cfg(test)]
mod surface_projector_tests;
pub mod svelte_default_synth;
pub mod symbol_resolver;
pub mod type_expansion;
pub mod type_expansion_host;
pub mod type_expansion_verter;
pub mod vue_default_synth;

pub mod fact_read_set;
pub mod fuses;
pub(crate) mod host_resolver_context;
pub mod imported_root_db;
pub(crate) mod request_store_view;
pub(crate) mod resolver_context;
pub mod route_db;
pub(crate) mod scope_shadowing;
pub(crate) mod session_resolver_context;

pub use fact_read_set::{FactReadSet, FactReadSetCell, FactReadSetFinalise};
// Substrate re-export. Hot-path callers construct the
// request-bound wrapper at entry points; the wiring lands in the
// hot-path conversion commit (C).
#[cfg(any(test, debug_assertions))]
pub(crate) use host_resolver_context::with_bare_host_ctx_for_test;
#[allow(unused_imports)]
pub(crate) use host_resolver_context::HostResolverContext;
#[allow(unused_imports)]
pub(crate) use request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
pub(crate) use resolver_context::{MaterializeScopeObservation, ResolverContext};
pub(crate) use session_resolver_context::SessionResolverContext;

pub use fuses::{FuseBudgets, FuseState, FuseTrip};
pub use imported_root_db::{ImportedRootDb, ImportedRootResult};
pub use route_db::{
    BarrelRouteSurface, BarrelSurfaceKey, EffectiveExportEntry, EffectiveExportSetEntry,
    EffectiveExportSetKey, EffectiveExportSetScope, RouteDb, RouteNameKey, RouteResult,
    ROUTE_DB_RESOLVER_VERSION,
};

pub type ResolverHash16 = verter_semantic::analysis::Hash16;
pub(crate) use component_meta::component_meta_resolved_macros;
pub use component_meta::{
    collect_requested_binding_names, component_meta_type_registry, resolve_component_meta_parts,
    ComponentMetaEvalOutputs, ComponentMetaResolutionPurpose, ComponentMetaResolverHost,
    ResolvedComponentMetaParts, ResolvedImportedMacroSurface, ResolvedJsdocBlock, ResolvedJsdocTag,
    ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};
pub use component_meta_query_engine::ComponentMetaQueryEngine;
// The surface-projection helpers (`projected_surface_from_semantic_node`,
// `surface_view_to_projected_surface`, `projected_surface_to_type_expr`,
// `projected_surface_to_expanded_shape`) are intentionally NOT re-exported from
// `resolver_core`: the raw `SemanticNodeId` / `&SurfaceView` → surface
// projection stays confined to the query-engine subtree (in-subtree callers
// reach them via `super::surface::`; out-of-subtree callers route through the
// engine's sink-local methods `dispatch_projected_surface_to_type_expr` /
// `projected_expanded_shape_from_node` / the routed-surface methods).
pub(crate) use component_meta_query_engine::{
    lower_and_project_to_expanded_node, project_admitted_node_to_expanded_node,
    project_class_a_published, project_class_a_terminal_node, project_expr_surface_expr_node,
    type_expr_contains_semantic_miss, AdmittedRouteProjectionNode,
};
// `type_expr_root_is_unmaterialized_sentinel` survives only as the `#[cfg(test)]`
// parity oracle for the node-domain root-sentinel fact (production reads
// `node_root_is_unmaterialized_sentinel_with_dispatch`); the raised-shape suite
// imports it through this re-export.
#[cfg(test)]
pub(crate) use component_meta_query_engine::type_expr_root_is_unmaterialized_sentinel;
pub use component_meta_request::{run_component_meta_request, ComponentMetaRequestHost};
pub use declaration_metadata::{
    resolve_direct_local_type_declaration, resolve_local_type_declaration,
    resolve_type_declaration, DeclarationMetadataResolver, ResolvedDeclarationKind,
    ResolvedExportTarget, ResolvedLocalTypeSymbolMetadata, ResolvedTypeDeclaration,
};
pub use export_graph::{
    get_export_span_follow_reexports_from_graph, resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort, resolve_named_export_from_graph, ExportGraphResolver,
    ExportSurface, ResolvedGraphExport,
};
pub use external_macro_types::{
    collect_external_macro_types, ExternalMacroTypeCollection, ExternalMacroTypeCollectorHost,
    ExternalMacroTypeDiagnostic,
};
pub use external_type_body::{
    resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
};
pub use external_type_frontier::{
    ExternalTypeFrontier, FrontierHost, PendingExternalSymbol, ResolvedRouteProvenance,
    ResolvedSymbol, ResolvedSymbolStatus, RouteKind,
};
pub use fallthrough::{
    append_component_candidate_branches, append_native_candidate_branch,
    collect_dynamic_root_candidates_from_type, component_import_candidate_for_binding,
    extend_unique_fact_versions, fallthrough_cache_key, known_spread_keys_from_type_expr,
    merge_fallthrough_branches, push_partial_reason, resolve_fallthrough_surface,
    structural_substitute_typeof_refs, DynamicRootCandidate, FallthroughComputeHost,
    FallthroughPropOverride, FallthroughPropOverrideSet, FallthroughResolutionView,
    FallthroughResolverHost, KnownSpreadKeys, ResolvedConsumedBindings, ResolvedFallthroughSurface,
};
pub use fallthrough_override_key::FallthroughOverrideIdentity;
pub use fallthrough_request::{run_fallthrough_request, FallthroughRequestHost};
pub use prepared_decl::{
    build_prepared_type_decl_cache, build_prepared_value_decl_cache,
    prepare_augmentation_type_decl, prepare_exported_type_decl, prepare_exported_value_decl,
    prepare_local_type_decl, prepare_local_value_decl, ImportCanonicalization,
};
pub use route_demand::{
    merge_route_demands, RouteDemand, RouteProvenance, RouteProvenanceKind, RoutedExternalDep,
    RoutedSymbolResult, RoutedSymbolStatus, SymbolSpace,
};
pub use runtime_values::{
    materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver,
};
pub use shallow_file_state::{
    BudgetDomain, BudgetExceededFailure, ClassifiedTypeDeps, ExportTarget, ExternalSymbolRef,
    ImportTarget, LocalClosureResult, LocalClosureStatus, ResolutionBudgets, ResolutionCounters,
    ShallowFileState, ShallowImportResolver, ShallowTypeSymbol, ShallowTypeView,
    ShallowValueSymbol, WildcardReexport,
};
pub use surface_projector::{project_macro_surfaces, ProjectedMacroSurfaces, ResolvedNativeProp};

/// Lane-identity token for singleflight / stability-request
/// deduplication.
///
/// This token is the SOLE identity `run_stable_request` (and the
/// `SingleflightGroup` lanes it drives) coalesce on, and a FOLLOWER
/// receives the LEADER's stable result WITHOUT revalidating it against
/// the follower's own view. The token must therefore be a COMPLETE
/// validity oracle: two requests may coalesce onto one lane ONLY if
/// their views are validation-equivalent.
///
/// `epoch` + `session` alone are NOT complete — a view's EXTERNAL
/// validity can change (env-hash / project-identity / project-generation /
/// overlay) WITHOUT moving the `store_view_epoch`. `validity_fingerprint`
/// closes that hole: the production
/// [`crate::resolver_store::HostStoreView`] folds the EXTERNAL-supersession
/// dimensions of its `StoreViewValidationToken` into it (the SAME oracle
/// the executors' promotion fence `is_stable` compares), so two views that
/// would externally-supersede each other get distinct lane identities and
/// never wrongly coalesce. Test / permissive stubs leave it `0` (their
/// views are validation-trivial).
///
/// The additive `artifact_generation` /
/// `load_generation` are DELIBERATELY EXCLUDED from the fold: a cold
/// compute advances those generations as its OWN work (publishing
/// artifacts, loading dependencies), so two concurrent identical cold
/// requests legitimately observe different additive generations. Folding
/// them would split those identical requests across distinct lanes and
/// spawn multiple cold winners instead of one leader + N-1 dedup-joining
/// followers — the same self-fencing the promotion oracle avoids. A
/// follower on the same external lane IS validation-equivalent: the leader
/// only promotes when the external dimensions are coherent.
///
/// `epoch` and `session` are retained as separate fields because callers
/// read them directly (e.g. the route-surface validator inspects
/// `session` to reject session views; the snapshot identity threads
/// `epoch`). `validity_fingerprint` is additive: it tightens lane
/// identity without changing what those reads observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewCompatToken {
    pub epoch: u64,
    pub session: Option<u64>,
    /// Fold of the EXTERNAL-supersession dimensions of the
    /// `StoreViewValidationToken` the view was built under (epoch,
    /// project-generation, env-hash, project-identity, overlay). `0` for
    /// validation-trivial stub views. Folds every external validity-
    /// affecting dimension that `epoch` alone does not cover — and excludes
    /// the additive artifact / load generations a cold
    /// compute advances as its own work — so the singleflight / stability
    /// coalescing lane is the SAME oracle the promotion fence applies.
    pub validity_fingerprint: u64,
}

pub trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;

    /// Validate a fact reference under this view. Implementers MUST
    /// supply this method; the trait does NOT provide a default
    /// because legacy substrate variants (`FileWholeHash`,
    /// `DerivedFactHash`) need an implementer-specific check.
    /// Per-domain implementers route the per-domain variants here
    /// via the matching `validates_*_domain` methods.
    fn validates(&self, fact: &FactVersionRef) -> bool;

    /// Validate a parse-domain fact reference (R26). Default impl
    /// returns `false`; implementers that emit parse-domain facts
    /// override.
    fn validates_parse_domain(&self, _fact: &ParseFactRef) -> bool {
        false
    }

    /// Validate a resolve-imports-domain fact reference (R26).
    /// Default impl returns `false`; the resolver implementer
    /// overrides.
    fn validates_resolve_imports_domain(&self, _fact: &ResolveImportsFactRef) -> bool {
        false
    }

    /// Validate a route-surface-domain fact reference (R26). Default
    /// impl returns `false`; the `RouteDb` implementer overrides.
    fn validates_route_surface_domain(&self, _fact: &RouteSurfaceFactRef) -> bool {
        false
    }

    /// Validate a **self-root** `FileWholeHash` fact strictly.
    ///
    /// A self-root is the whole-hash fact for a query-identity cache
    /// entry's OWN keyed canonical (as opposed to a cross-file
    /// dependency fact). [`Self::validates`] applies a lazy
    /// "untracked file → optimistically accept" rule to a plain
    /// `FileWholeHash`: a file loaded as a dependency after the view
    /// snapshot has no tracked hash, and forcing every such dependency
    /// through a permissive recheck would be expensive. That
    /// permissiveness is unsafe for a self-root: an untracked self-root
    /// canonical means the cache entry's own file is gone (or its
    /// content is unknown to this view), which must FAIL validation —
    /// otherwise the entry survives a same-canonical content edit.
    ///
    /// This method is the strict counterpart: an untracked or
    /// hash-mismatched self-root canonical returns `false`. The default
    /// impl delegates to [`Self::validates`] so non-production
    /// `StoreView` stubs keep their existing behavior; the production
    /// [`crate::resolver_store::HostStoreView`] overrides it to reject
    /// the untracked case. Callers that hold the explicit self-root
    /// canonical set route through
    /// [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`].
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        self.validates(&FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: *hash,
        })
    }

    /// Whether the view tracks a specific file (has its hash in the snapshot).
    ///
    /// Used by route-derived cache materialization paths to decide whether to
    /// include `DerivedFactHash::ImportRoute` in validation facts. Untracked
    /// dependency files never have `set_import_dependencies` called on them,
    /// so their route facts are safe to omit — eliminating false cache misses.
    fn tracks_file(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Direct read of the view's `DerivedFactHash` snapshot for a
    /// `(canonical, kind)` pair.
    ///
    /// Returns `Some(hash)` when the view's per-domain producer has
    /// snapshotted a derived hash for the pair (e.g.
    /// `HostStoreView::derived_hashes[(canonical, ImportRoute)]`),
    /// `None` otherwise. Used by per-rejection attribution helpers
    /// (e.g. `attribute_prepared_decl_bundle_rejection`) to
    /// distinguish "entry absent" from "entry present, hash differs"
    /// without re-probing the validator with synthetic hashes.
    ///
    /// Default returns `None` so test-only / permissive views inherit
    /// "no derived snapshot" semantics; production `HostStoreView`
    /// overrides to return the actual snapshot value.
    fn derived_hash_for(
        &self,
        _canonical_id: &str,
        _kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        None
    }

    /// Validate every fact in `sig` under this view; return `true` iff
    /// all entries validate. Empty signatures trivially return `true`.
    ///
    /// Default impl calls [`Self::validates`] on each entry.
    /// Implementers that can short-circuit (e.g. generation-monotone
    /// views) may override for performance.
    #[inline]
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        sig.iter().all(|f| self.validates(f))
    }

    /// Promote a lazily-materialised canonical's route facts into the
    /// request-scoped completion overlay.
    ///
    /// Called by the cold prepared-decl-bundle materialiser for
    /// declaration files (`.d.ts` / `.d.mts` / `.d.cts`) whose
    /// `IndexedReady` materialised AFTER the request-entry
    /// [`crate::resolver_store::HostStoreView`] snapshot was built —
    /// entries published after that snapshot are invisible to the
    /// view, so every subsequent warm-validation read of the bundle's
    /// stored derived-fact hashes would route through the base view's
    /// untracked-canonical reject and trigger a fresh cold rebuild.
    /// With promotion the next read sees the canonical as tracked, the
    /// warm validation matches, and the bundle's cold/warm ratio
    /// collapses from O(N) cold rebuilds to the expected 1:N (one cold
    /// + N-1 warm).
    ///
    /// The producer-side caller is responsible for the epoch guard
    /// (skip the call if the host's `current_store_view_epoch` no
    /// longer matches the base view's `mutation_epoch`) — keeping
    /// the trait off the concrete `VerterHost` type to preserve the
    /// resolver-context seal (architecture guard
    /// `no_concrete_verter_host_in_seal_scope`).
    ///
    /// Implementers writing into a per-request overlay must:
    /// - Insert `whole_hash` into the overlay's `whole_hashes` map
    ///   (so `validates_self_root_whole_hash` accepts the bundle's
    ///   `FileWholeHash` self-root).
    /// - Insert `route_hash` into the overlay's `derived_hashes` under
    ///   the `Route` kind when `Some`.
    /// - Insert `import_route_hash` into the overlay's `derived_hashes`
    ///   under the `ImportRoute` kind when `Some` (this is the leak
    ///   the producer captures — the bundle's fact hash MUST match
    ///   what the view's snapshot would carry).
    ///
    /// Default impl is no-op so non-request views (the bare
    /// [`crate::resolver_store::HostStoreView`], test-only
    /// [`PermissiveStoreView`], etc.) inherit "no overlay" semantics
    /// — they have no per-request append-only side maps to mutate.
    fn promote_route_completion(
        &self,
        _canonical: &str,
        _whole_hash: crate::types::Hash16,
        _route_hash: Option<crate::types::Hash16>,
        _import_route_hash: Option<crate::types::Hash16>,
    ) {
    }
}

/// Forward [`StoreView`] through a shared reference, including the unsized
/// `&dyn StoreView` form.
///
/// This lets a generic `view: &V where V: StoreView` validator accept a
/// `ctx.store_view()` borrow (`&dyn StoreView`) directly — e.g. the
/// fallthrough resolver validates per-element / per-child / per-root
/// node-cache entries through `self.ctx.store_view()` so the validation
/// rides the request-bound, currentness-gated `RequestStoreView` rather
/// than a separately-rebuilt raw `HostStoreView`. Every method just
/// re-dispatches to the referent.
impl<T: StoreView + ?Sized> StoreView for &T {
    #[inline]
    fn compat_token(&self) -> StoreViewCompatToken {
        (**self).compat_token()
    }
    #[inline]
    fn validates(&self, fact: &FactVersionRef) -> bool {
        (**self).validates(fact)
    }
    #[inline]
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        (**self).validates_parse_domain(fact)
    }
    #[inline]
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool {
        (**self).validates_resolve_imports_domain(fact)
    }
    #[inline]
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        (**self).validates_route_surface_domain(fact)
    }
    #[inline]
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        (**self).validates_self_root_whole_hash(canonical_id, hash)
    }
    #[inline]
    fn tracks_file(&self, canonical_id: &str) -> bool {
        (**self).tracks_file(canonical_id)
    }
    #[inline]
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        (**self).derived_hash_for(canonical_id, kind)
    }
    #[inline]
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        (**self).validates_fact_signature(sig)
    }
    #[inline]
    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: crate::types::Hash16,
        route_hash: Option<crate::types::Hash16>,
        import_route_hash: Option<crate::types::Hash16>,
    ) {
        (**self).promote_route_completion(canonical, whole_hash, route_hash, import_route_hash)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PermissiveStoreView;

impl StoreView for PermissiveStoreView {
    fn compat_token(&self) -> StoreViewCompatToken {
        // Validation-trivial: `validates` accepts every fact, so any
        // coalescing under this view is safe — `validity_fingerprint`
        // stays `0`.
        StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        }
    }

    fn validates(&self, _fact: &FactVersionRef) -> bool {
        true
    }
}

pub trait ResolverStore {
    type View: StoreView;

    fn snapshot_view(&self) -> Self::View;
}

pub trait ResolverRuntime {
    fn store_view_token(&self) -> StoreViewCompatToken;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestTraceId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolveRequestTarget {
    Symbol(ResolutionNodeKey),
    Fallthrough(FallthroughNodeKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveRequest {
    pub trace_id: RequestTraceId,
    pub target: ResolveRequestTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivedFactKind {
    /// Provider-owned export route surface hash.
    Route,
    /// Importer-owned effective import-target surface hash.
    ImportRoute,
    DirectSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FactVersionRef {
    FileWholeHash {
        canonical_id: String,
        hash: ResolverHash16,
    },
    DerivedFactHash {
        canonical_id: String,
        kind: DerivedFactKind,
        hash: ResolverHash16,
    },
    // ── R12 per-domain variants ──
    //
    // Each variant carries the fact's domain-scoped reference so
    // [`StoreView::validates`] can route via
    // [`crate::resolver_core::FactDomainTag::from_fact_version_ref`]
    // to the matching per-domain validator. The dispatch table is
    // bounded by `FactDomain` (3 variants), not by `FactKey` — adding
    // a new `FactKey` extends a per-domain `*FactRef` enum but does
    // NOT widen the trait (R26).
    /// Parse-domain fact reference: per-file `FactKey` + observed
    /// hash + lane. Validates against `FileFacts.registry`.
    Parse(ParseFactRef),
    /// Resolve-imports-domain fact reference: per-file
    /// `ResolvedImportFacts` entry. The resolver populates the
    /// underlying store; the variant defines the dispatch surface.
    ResolveImports(ResolveImportsFactRef),
    /// Route-surface-domain fact reference: `RouteDb`-owned
    /// effective-export-set / augmentation-index fingerprint. The
    /// `RouteDb` producer populates the underlying store.
    RouteSurface(RouteSurfaceFactRef),
    /// Project-generation reference: the observed monotonic project
    /// generation a cached value depended on. Validates iff the
    /// host's current project generation equals `generation`.
    ///
    /// Unlike the file-scoped variants above this carries no
    /// canonical id — it roots a value against the project-wide
    /// resolver/config/lib generation rather than any single file's
    /// content. The generation advances on `tsconfig`, path-alias,
    /// SDK, workspace-folder, and project-graph changes (never on a
    /// pure file-content edit).
    ProjectGeneration { generation: u64 },
}

impl FactVersionRef {
    /// The canonical file id this fact references, when the variant is
    /// file-scoped. Used by callers that need to scope a fact set by
    /// owning file (e.g. excluding the owner's own facts when fanning
    /// a curated dependency set into the fact tracer).
    ///
    /// Returns `None` for [`FactVersionRef::ProjectGeneration`], which
    /// is not file-scoped — it roots a value against the project-wide
    /// generation rather than a single canonical's content. A
    /// project-generation fact is therefore never equal to any
    /// excluded owner canonical, so owner-scoped fan-out filters keep
    /// it.
    #[inline]
    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        match self {
            FactVersionRef::FileWholeHash { canonical_id, .. }
            | FactVersionRef::DerivedFactHash { canonical_id, .. } => Some(canonical_id.as_str()),
            FactVersionRef::Parse(p) => Some(p.canonical_id.as_str()),
            FactVersionRef::ResolveImports(r) => Some(r.canonical_id.as_str()),
            FactVersionRef::RouteSurface(r) => Some(r.canonical_id.as_str()),
            FactVersionRef::ProjectGeneration { .. } => None,
        }
    }
}

/// Parse-domain fact reference. Lane is recorded explicitly so
/// validators know whether to check `semantic_hash` (cosmetic edits
/// invariant) or `display_hash` (cosmetic-sensitive). See R13 lane
/// model.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseFactRef {
    pub canonical_id: String,
    pub key: verter_semantic::facts::FactKey,
    pub lane: verter_semantic::facts::FactLane,
    pub expected_hash: ResolverHash16,
}

/// Resolve-imports-domain fact reference. The resolver producer
/// populates the matching store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolveImportsFactRef {
    pub canonical_id: String,
    pub key: verter_semantic::facts::FactKey,
    pub lane: verter_semantic::facts::FactLane,
    pub expected_hash: ResolverHash16,
}

/// Route-surface-domain fact reference. The `RouteDb` producer
/// populates the matching store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteSurfaceFactRef {
    pub canonical_id: String,
    pub key: verter_semantic::facts::FactKey,
    pub lane: verter_semantic::facts::FactLane,
    pub expected_hash: ResolverHash16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraversalLens {
    StructuralObject,
    KeySpace,
    CallableParams,
    CallableReturn,
    ValueTypeOf,
    MemberProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolutionNodeKind {
    /// Importer-side import-edge node: keyed by owner + import source +
    /// requested symbol + binding context.
    ImporterEdge,
    /// Provider-side export-route node: keyed by provider canonical +
    /// requested symbol + route demand + symbol space. Reusable across importers.
    ProviderExportRoute,
    BarrelLookup,
    DeclarationMetadata,
    SymbolExpand,
    MemberProjection,
    KeySpace,
    MappedExpand,
    TypeOfValue,
    Assemble,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolutionNodeKey {
    pub symbol_id: String,
    pub node_kind: ResolutionNodeKind,
    pub traversal_lens: TraversalLens,
    pub member_path_hash: u64,
    pub type_args_hash: u64,
    pub behavior_flags: u32,
    /// Session-view fingerprint (`0` for the overlay-free base host
    /// view, non-zero for session-bearing query paths). Two concurrent
    /// sessions with different overlays admit distinct singleflight
    /// slots under this discriminator (R20 multi-candidate isolation
    /// for the resolved-meta cache).
    pub view_fingerprint: u64,
}

/// Typed fallthrough-node cache key. Each variant is one node kind, so the
/// kind-discriminating fields are not field-overloaded: the override-bearing
/// variants carry a typed [`FallthroughOverrideIdentity`] (not a lossy `u64`),
/// and the intrinsic-surface variant carries its own
/// `(project_anchor, cache_generation, tag)` axes instead of overloading the
/// override field to also smuggle the intrinsic cache generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FallthroughNodeKey {
    ComponentRootFollow {
        canonical: String,
        overrides: FallthroughOverrideIdentity,
        generic_root_propagation: bool,
    },
    BranchUnionMerge {
        canonical: String,
        overrides: FallthroughOverrideIdentity,
        generic_root_propagation: bool,
    },
    ChildComponentSurfaceFollow {
        canonical: String,
        overrides: FallthroughOverrideIdentity,
    },
    ConsumedBindingEvaluation {
        canonical: String,
        branch_key: String,
        overrides: FallthroughOverrideIdentity,
    },
    IntrinsicSurfaceLoad {
        project_anchor: String,
        cache_generation: u64,
        tag: String,
    },
}

impl FallthroughNodeKey {
    /// The owning canonical / project-anchor id this key is rooted in.
    #[must_use]
    pub fn canonical(&self) -> &str {
        match self {
            Self::ComponentRootFollow { canonical, .. }
            | Self::BranchUnionMerge { canonical, .. }
            | Self::ChildComponentSurfaceFollow { canonical, .. }
            | Self::ConsumedBindingEvaluation { canonical, .. } => canonical,
            Self::IntrinsicSurfaceLoad { project_anchor, .. } => project_anchor,
        }
    }

    /// `false` for an override-bearing key whose override identity could not be
    /// projected to an exact canonical key
    /// ([`FallthroughOverrideIdentity::Uncacheable`]). Such a key must NOT be
    /// stored, looked up, or used as a singleflight lane identity — a unit
    /// `Uncacheable` value would alias two genuinely-different override sets.
    /// No-override keys and the intrinsic-surface key are always cacheable.
    #[must_use]
    pub fn is_cacheable(&self) -> bool {
        match self {
            Self::ComponentRootFollow { overrides, .. }
            | Self::BranchUnionMerge { overrides, .. }
            | Self::ChildComponentSurfaceFollow { overrides, .. }
            | Self::ConsumedBindingEvaluation { overrides, .. } => {
                !matches!(overrides, FallthroughOverrideIdentity::Uncacheable)
            }
            Self::IntrinsicSurfaceLoad { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverDiagnostic {
    pub code: String,
    pub message: String,
    pub canonical_path: Option<String>,
    pub span_start: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct StableExecutionValue<V> {
    pub value: V,
    pub stable: bool,
    /// `true` when the singleflight winner produced this value by
    /// running `executor.compute()` (a genuine cold build); `false`
    /// when the winner's flight closure instead served a warm hit from
    /// the executor's own cache (`try_get_cached` succeeded inside the
    /// flight). A winner that served a cache hit performed NO cold work,
    /// so [`run_stable_request`] reports [`RequestSource::Cache`] for it
    /// rather than [`RequestSource::Flight`] — which keeps a late caller
    /// that wins a fresh flight lane (because the prior burst already
    /// completed and reaped its lane) but immediately reads the warm
    /// result attributed as a cache hit, not as a second cold winner.
    pub computed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSource {
    Cache,
    Flight {
        role: SingleflightRole,
        forked_lane: bool,
    },
    Fallback,
}

#[derive(Debug, Clone)]
pub struct RequestRunResult<V> {
    pub value: V,
    pub source: RequestSource,
    pub attempts: usize,
}

pub trait StableRequestExecutor<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    type View: StoreView;
    type Error: Clone;

    fn cache_key(&self) -> K;
    fn snapshot_view(&mut self) -> Self::View;
    /// Whether the view the most recent [`Self::snapshot_view`] returned
    /// was proven current by the underlying store-view manager.
    ///
    /// [`run_stable_request`] consults this to gate THREE decisions, not
    /// one. A non-current snapshot (the manager handed back a known-stale
    /// `StoreViewRead::ReturnOnly` view under sustained churn) must:
    ///
    /// 1. **never serve a warm cache hit** — validating a cache entry's
    ///    fact signature against a stale view false-positives a superseded
    ///    result;
    /// 2. **never join the result-sharing singleflight** — the lane key's
    ///    `compat_token` excludes the additive generations, so a
    ///    non-current snapshot's token can still equal a lane an earlier
    ///    current request retained a stable result on; joining it would
    ///    Follower-return that pre-mutation result without running
    ///    `compute` or `is_stable`;
    /// 3. **never warm the shared cache** — its `is_stable` fence is false,
    ///    so its own cold result is returned-only.
    ///
    /// So on a non-current snapshot the driver bypasses the lane entirely
    /// and runs an isolated cold `compute` whose `is_stable` fence gates
    /// promotion.
    ///
    /// REQUIRED (no default): `true` is a soundness claim that opens all
    /// three gates above — a defaulted `true` would let an executor whose
    /// snapshots CAN be non-current, but which forgets the override,
    /// silently launder stale snapshots as proven-current. Executors
    /// without a churn-prone manager (test stubs) state `true` explicitly.
    fn snapshot_view_is_current(&self) -> bool;
    /// Whether [`Self::snapshot_view`] returns an IMMUTABLE snapshot — the
    /// same view, currentness, and supersession fingerprint on every
    /// attempt, with no re-read of live host state.
    ///
    /// This is `true` only for a caller-pinned FIXED snapshot (e.g. the
    /// component-meta batch's `BatchFixedView`). For such a snapshot a
    /// result that is not [`Self::is_stable`] on the FIRST attempt can NEVER
    /// become stable on a later one: the snapshot, its captured fingerprint,
    /// and its captured currentness are frozen, so neither the
    /// non-current-capture gate nor the captured-vs-live fingerprint gate can
    /// flip across attempts. [`run_stable_request`] therefore returns the
    /// first fenced (return-only) result immediately for an immutable
    /// snapshot instead of recomputing it `max_attempts + 1` times — the
    /// retries cannot converge and only waste cold compute (and the
    /// request's projection budget). A CURRENT + fingerprint-MATCHING fixed
    /// snapshot is stable on the first attempt and promotes exactly as
    /// before; the short-circuit fires ONLY on an unstable immutable result.
    ///
    /// The default returns `false`: a per-attempt snapshot (the `None`
    /// fixed-view path, and every test stub) re-reads live state on each
    /// attempt, so a later attempt may legitimately obtain a freshly-coherent
    /// snapshot and promote (the churn-then-settle case) — those executors
    /// MUST keep retrying.
    fn snapshot_is_immutable(&self) -> bool {
        false
    }
    fn try_get_cached(&mut self, view: &Self::View) -> Option<V>;
    fn compute(&mut self, view: &Self::View) -> Result<V, Self::Error>;
    fn is_stable(&mut self, view: &Self::View) -> bool;
    fn store_stable(&mut self, value: &V);

    fn max_attempts(&self) -> usize {
        3
    }
}

pub fn run_stable_request<K, V, X>(
    singleflight: &SingleflightGroup<K, StableExecutionValue<V>, X::Error>,
    executor: &mut X,
) -> Result<RequestRunResult<V>, X::Error>
where
    K: Clone + Eq + Hash,
    V: Clone,
    X: StableRequestExecutor<K, V>,
{
    let cache_key = executor.cache_key();
    let max_attempts = executor.max_attempts();

    for attempt in 0..max_attempts {
        let store_view = executor.snapshot_view();
        // Currentness of THIS attempt's snapshot. A non-current snapshot
        // (the manager handed back a known-stale `ReturnOnly` view under
        // sustained churn) must NOT serve a warm cache hit: validating a
        // cache entry's fact signature against a stale view false-positives
        // a superseded result. When false, every warm `try_get_cached`
        // peek on this attempt is suppressed and the request falls to the
        // cold flight, whose `is_stable` fence gates promotion.
        let snapshot_is_current = executor.snapshot_view_is_current();

        // A non-current snapshot must not only skip the warm probe — it
        // must also stay OUT of the result-sharing singleflight. The lane
        // key is `(cache_key, compat_token)`, and `compat_token`
        // deliberately excludes the additive artifact / load
        // generations (so two identical cold computes coalesce). A snapshot
        // can therefore be non-current — the manager could not prove this
        // attempt's view coherent — while its `compat_token` still equals a
        // lane an earlier CURRENT request retained a stable `Done` on. If
        // this non-current attempt pinned and `run`-joined that lane it
        // would Follower-return the earlier flight's value WITHOUT running
        // `compute` or the `is_stable` fence — a post-mutation request
        // receiving a pre-mutation result.
        //
        // So a non-current attempt runs its OWN cold compute OFF the shared
        // lane, under its own `is_stable` promotion fence. It never joins,
        // is never joined, and only a `stable` result warms the shared
        // cache (`store_stable`) — a non-current/incoherent snapshot's
        // `is_stable` is false, so its result is returned-only. Crucially
        // this mirrors the on-lane unstable-result path's retry semantics:
        // an UNSTABLE off-lane result does NOT terminate the request — the
        // bounded outer loop continues to the next attempt, which may
        // snapshot a freshly-coherent view and promote (the churn-then-
        // settle case). Only a STABLE off-lane result returns immediately.
        // The loop is bounded by `max_attempts`; on exhaustion the post-
        // loop fallback returns a return-only result.
        if !snapshot_is_current {
            let value = executor.compute(&store_view)?;
            if executor.is_stable(&store_view) {
                executor.store_stable(&value);
                return Ok(RequestRunResult {
                    value,
                    source: RequestSource::Fallback,
                    attempts: attempt + 1,
                });
            }
            // Unstable off-lane result. For a per-attempt snapshot, retry on
            // the next attempt (bounded) — a later attempt may snapshot a
            // freshly-coherent view and promote (churn-then-settle). For an
            // IMMUTABLE snapshot (a caller-pinned fixed view) the next attempt
            // re-presents the SAME non-current capture, so it can never
            // converge to stable — retrying only re-runs the cold compute and
            // burns projection budget. Return the first fenced (return-only)
            // result now.
            if executor.snapshot_is_immutable() {
                return Ok(RequestRunResult {
                    value,
                    source: RequestSource::Fallback,
                    attempts: attempt + 1,
                });
            }
            continue;
        }

        // Pin the singleflight lane for THIS attempt's whole lifetime,
        // BEFORE the pre-flight cache peek and the inner `run` claim, on
        // the SAME lane those steps run on — the actual snapshotted view's
        // `compat_token()`. Without this pin, a concurrent burst can tear
        // down the leader's lane in the gap between a straggler's
        // "cache-miss" decision (the peek below) and its `singleflight.run`
        // claim, so the straggler finds a vacant lane and spawns a second
        // cold leader. The participation pin keeps the lane alive across the
        // whole burst — every concurrent caller pins before it peeks — so the
        // leader's published `Done` rendezvous is still joinable when a
        // straggler finally reaches `run`, and it Follower-joins instead of
        // recomputing. The token folds into the lane key, so the pin only
        // coalesces callers that share the same store-view identity (R20).
        //
        // The pin is taken from the per-attempt `store_view.compat_token()`
        // rather than a separately-derived token, so the PINNED lane is
        // exactly the lane the inner `run` claims for BOTH base
        // (`session: None`) and session (`session: Some(id)`) hosts. This
        // closes two ways the pin lane could drift from the run lane: a
        // session host whose view carries `session: Some(id)` while a
        // cheaper token source reports `session: None`, and a mid-request
        // store-view epoch bump. If the epoch (or session) changes across
        // attempts, the next attempt snapshots a fresh view and re-pins the
        // new lane; the prior attempt's pin releases its now-unrelated lane
        // on drop (a leader landing on the new lane is correct — the
        // pre-bump result is no longer interchangeable).
        let _participation = singleflight.participate(cache_key.clone(), store_view.compat_token());

        // The current path is the ONLY path that reaches here — a
        // non-current snapshot returned above without pinning or joining a
        // lane, so the participate pin and the `run_retaining` join below
        // coalesce ONLY proven-current attempts on the same lane.
        if let Some(cached) = executor.try_get_cached(&store_view) {
            return Ok(RequestRunResult {
                value: cached,
                source: RequestSource::Cache,
                attempts: attempt + 1,
            });
        }

        let flight = singleflight.run_retaining(
            cache_key.clone(),
            store_view.compat_token(),
            || {
                // The leader is proven-current (the non-current branch
                // returned before pinning), so the warm peek here is safe:
                // a hit it retains as a `stable` rendezvous for followers
                // was validated against a current view.
                if let Some(cached) = executor.try_get_cached(&store_view) {
                    return Ok(StableExecutionValue {
                        value: cached,
                        stable: true,
                        computed: false,
                    });
                }

                let value = executor.compute(&store_view)?;
                let stable = executor.is_stable(&store_view);
                if stable {
                    executor.store_stable(&value);
                }

                Ok(StableExecutionValue {
                    value,
                    stable,
                    computed: true,
                })
            },
            // Retain ONLY stable results as a joinable rendezvous. An
            // unstable result (the snapshot view moved mid-compute) must
            // NOT be retained, or the stability-retry loop below — and any
            // concurrent sibling — would join the torn result instead of
            // recomputing against fresh state.
            |sev| sev.stable,
        )?;

        if flight.value.stable {
            // A flight LEADER whose closure served a warm hit (did NOT
            // run `compute`) performed no cold work — attribute it as a
            // cache hit, not a cold flight winner. This is the
            // post-burst straggler case: under load, a caller can claim
            // a fresh flight lane after the prior burst's leader already
            // completed and reaped its lane, then immediately read the
            // warm result the prior leader published. Reporting `Cache`
            // (instead of `Flight { Leader }`) lets the joiner-accounting
            // layer classify it as a joiner rather than a spurious second
            // cold winner. Followers always carry the leader's role.
            let source = match flight.role {
                SingleflightRole::Leader if !flight.value.computed => RequestSource::Cache,
                role => RequestSource::Flight {
                    role,
                    forked_lane: flight.forked_lane,
                },
            };
            return Ok(RequestRunResult {
                value: flight.value.value.clone(),
                source,
                attempts: attempt + 1,
            });
        }

        // The on-lane flight produced an UNSTABLE result (e.g. a fixed view
        // whose captured fingerprint no longer matches the live one). For a
        // per-attempt snapshot the loop continues — a later attempt may
        // snapshot fresh coherent state and promote. For an IMMUTABLE snapshot
        // the next attempt re-presents the SAME stale capture, so the fence
        // can never flip to stable; return the first fenced (return-only)
        // result now instead of recomputing it every remaining attempt plus
        // the fallback.
        if executor.snapshot_is_immutable() {
            return Ok(RequestRunResult {
                value: flight.value.value.clone(),
                source: RequestSource::Fallback,
                attempts: attempt + 1,
            });
        }
    }

    let store_view = executor.snapshot_view();
    Ok(RequestRunResult {
        value: executor.compute(&store_view)?,
        source: RequestSource::Fallback,
        attempts: max_attempts + 1,
    })
}

/// R20 multi-candidate substrate.
///
/// One outer `DashMap` shard holds the cache entries, keyed by `K`.
/// Each entry's `candidates` field is an `ArcSwap` over a `SmallVec`
/// of `Arc<Candidate<V>>`. Concurrent generations of the "same" key
/// (e.g., two file-content versions of the same definition) coexist
/// as distinct candidates, validated independently against the
/// caller's `StoreView`.
///
/// **Read path** (`&self`): shard-read on the `DashMap` →
/// `ArcSwap.load()` → iterate candidates → first validating candidate
/// is the hit. **Zero atomic writes on hit**.
///
/// **Write path** (`&self`): shard-write on the `DashMap` →
/// `ArcSwap.rcu(|old| clone, FIFO-evict if cap reached, push new)`.
///
/// **Signature size cap**: a candidate whose `fact_dep_signature`
/// exceeds [`FACT_SIGNATURE_CAP`] entries is admitted as
/// `NonCacheable` — the candidate does NOT enter the cache, and
/// the `FactSignatureOverflow` audit event fires. Callers fall back
/// to cold recompute; correctness is preserved.
#[derive(Debug)]
pub struct ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    entries: DashMap<K, Arc<CacheEntry<V>>>,
    /// Instrumentation counter: increments every time a candidate's
    /// `fact_dep_signature` is rejected for exceeding
    /// [`FACT_SIGNATURE_CAP`]. Read in tests via
    /// [`ValidatedFactCache::signature_overflow_count`].
    signature_overflow: AtomicU64,
    /// R20 instrumentation counter: increments on each admission
    /// refused by the fact-completeness guard. Read via
    /// [`ValidatedFactCache::admission_refused_count`].
    admission_refused: AtomicU64,
    /// Instrumentation counter: increments on every `ArcSwap::store`
    /// call in the cache substrate. Hot-path reads must never
    /// advance this counter.
    arcswap_stores: AtomicU64,
    /// R24 instrumentation counter: total `get_if_valid` calls
    /// attempted against this cache. Bumped once per read attempt.
    /// Hot-path-safe: a single `fetch_add` per call.
    validations_attempted: AtomicU64,
    /// R24 instrumentation counter: number of `get_if_valid` calls
    /// that found a validating candidate. Hot-path-safe.
    warm_hits: AtomicU64,
    /// R24 instrumentation counter: number of `get_if_valid` calls
    /// where an entry existed but no candidate validated under the
    /// active view. Hot-path-safe.
    stale_misses: AtomicU64,
    /// R24 instrumentation counter: number of archive-style fallback
    /// checks consulted during validation. Zero in steady state on
    /// the post-archive cache substrate — recorded explicitly so
    /// substrate paths that retain a sidecar archive layer can be
    /// detected. Hot-path-safe.
    archive_checks: AtomicU64,
}

impl<K, V> Default for ValidatedFactCache<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            entries: DashMap::new(),
            signature_overflow: AtomicU64::new(0),
            admission_refused: AtomicU64::new(0),
            arcswap_stores: AtomicU64::new(0),
            validations_attempted: AtomicU64::new(0),
            warm_hits: AtomicU64::new(0),
            stale_misses: AtomicU64::new(0),
            archive_checks: AtomicU64::new(0),
        }
    }
}

/// R20 multi-candidate slot. Wraps an `ArcSwap` over a `SmallVec`
/// of candidates so the read path can return without taking any
/// per-slot lock; the write path is an `ArcSwap::rcu` that races
/// against concurrent writers via copy-on-write.
#[derive(Debug)]
pub struct CacheEntry<V> {
    candidates: arc_swap::ArcSwap<smallvec::SmallVec<[Arc<Candidate<V>>; CANDIDATE_CAP]>>,
}

impl<V> Default for CacheEntry<V> {
    fn default() -> Self {
        Self {
            candidates: arc_swap::ArcSwap::from_pointee(smallvec::SmallVec::new()),
        }
    }
}

/// R20 single candidate inside a `CacheEntry`. Multiple candidates
/// coexist when concurrent generations of the same cache key are
/// admitted (e.g., two file-content versions of the same definition,
/// two overlay sessions reaching the same definition with different
/// dep signatures, etc.).
///
/// Substrate spec:
/// - `signature_fingerprint`: short structural digest of the
///   `fact_dep_signature`, used for quick discriminator comparison.
/// - `value`: the actual cached value.
/// - `fact_dep_signature`: the ordered list of `FactVersionRef`
///   facts the candidate observed. Validation iterates this list
///   and short-circuits on the first miss.
#[derive(Debug)]
pub struct Candidate<V> {
    pub signature_fingerprint: [u8; 16],
    pub value: Arc<V>,
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

/// Per-slot candidate cap. The 5th admission triggers FIFO eviction
/// of the oldest candidate.
pub const CANDIDATE_CAP: usize = 4;

/// Per-candidate `fact_dep_signature` size cap. Larger signatures
/// are admitted as `NonCacheable` (the candidate is dropped and the
/// `FactSignatureOverflow` audit event fires). Callers fall back to
/// cold recompute; correctness is preserved.
pub const FACT_SIGNATURE_CAP: usize = 1024;

fn compute_signature_fingerprint(facts: &[FactVersionRef]) -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    // Two FxHasher passes seeded with distinct salts to produce 16
    // bytes of fingerprint without pulling a heavier hash crate.
    let salt_lo = rustc_hash::FxBuildHasher;
    let salt_hi = rustc_hash::FxBuildHasher;
    let mut h_lo = salt_lo.build_hasher();
    let mut h_hi = salt_hi.build_hasher();
    // Distinct constant seeds so the two hashers do not collapse.
    h_lo.write_u64(0xA5A5_A5A5_5A5A_5A5A);
    h_hi.write_u64(0x9E37_79B9_7F4A_7C15);
    for f in facts {
        match f {
            FactVersionRef::FileWholeHash { canonical_id, hash } => {
                h_lo.write(canonical_id.as_bytes());
                h_lo.write(hash);
                h_hi.write(canonical_id.as_bytes());
                h_hi.write(hash);
            }
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => {
                h_lo.write(canonical_id.as_bytes());
                h_lo.write_u8(match kind {
                    DerivedFactKind::Route => 1,
                    DerivedFactKind::ImportRoute => 2,
                    DerivedFactKind::DirectSource => 3,
                });
                h_lo.write(hash);
                h_hi.write(canonical_id.as_bytes());
                h_hi.write_u8(match kind {
                    DerivedFactKind::Route => 1,
                    DerivedFactKind::ImportRoute => 2,
                    DerivedFactKind::DirectSource => 3,
                });
                h_hi.write(hash);
            }
            FactVersionRef::Parse(_)
            | FactVersionRef::ResolveImports(_)
            | FactVersionRef::RouteSurface(_)
            | FactVersionRef::ProjectGeneration { .. } => {
                // Per-domain refs and the project-generation ref
                // serialise to their Debug form; the fingerprint is
                // approximate but stable.
                let s = format!("{f:?}");
                h_lo.write(s.as_bytes());
                h_hi.write(s.as_bytes());
            }
        }
    }
    let lo = h_lo.finish();
    let hi = h_hi.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

impl<K, V> ValidatedFactCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn get_if_valid<TView>(&self, key: &K, view: &TView) -> Option<Arc<V>>
    where
        TView: StoreView + ?Sized,
    {
        // R24 counter: increments once per read attempt. Hot-path
        // single atomic. Producers fold the four `*_count()` reads
        // into a `FactValidationSummary` event at request close-out.
        self.validations_attempted
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let entry = self.entries.get(key)?;
        let candidates = entry.candidates.load();
        for candidate in candidates.iter() {
            let ok = candidate
                .fact_dep_signature
                .iter()
                .all(|fact| view.validates(fact));
            if ok {
                self.warm_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Some(candidate.value.clone());
            }
        }
        // Entry existed but no candidate validated under the active
        // view — stale miss. The hot path's three-counter bump is
        // still cheaper than a single `Arc` clone.
        self.stale_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        None
    }

    /// Like [`Self::get_if_valid`] but validates any `FileWholeHash`
    /// fact whose canonical appears in `self_root_canonicals`
    /// **strictly** via [`StoreView::validates_self_root_whole_hash`].
    ///
    /// `get_if_valid` routes every `FileWholeHash` through the lazy
    /// [`StoreView::validates`], whose untracked-file arm optimistically
    /// accepts — correct for a cross-file *dependency* fact loaded after
    /// the view snapshot, but WRONG for a *self-root*: an untracked
    /// self-root canonical means the cache entry's own keyed file is
    /// gone (deleted), and the lazy arm would serve the stale entry. A
    /// canonical-keyed cache (e.g. the `prepared_decl_bundles` stable
    /// cache, keyed by the bundle's defining canonical) passes that
    /// keyed canonical here so a deleted keyed file rejects the entry.
    /// Every non-self-root fact keeps the lazy cross-file permissiveness.
    pub fn get_if_valid_self_rooted<TView>(
        &self,
        key: &K,
        view: &TView,
        self_root_canonicals: &[&str],
    ) -> Option<Arc<V>>
    where
        TView: StoreView + ?Sized,
    {
        self.validations_attempted
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let entry = self.entries.get(key)?;
        let candidates = entry.candidates.load();
        for candidate in candidates.iter() {
            let ok = candidate.fact_dep_signature.iter().all(|fact| match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash }
                    if self_root_canonicals.contains(&canonical_id.as_str()) =>
                {
                    view.validates_self_root_whole_hash(canonical_id, hash)
                }
                other => view.validates(other),
            });
            if ok {
                self.warm_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Some(candidate.value.clone());
            }
        }
        self.stale_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        None
    }

    /// Attributed sibling of [`Self::get_if_valid_self_rooted`].
    ///
    /// On a warm hit returns `Ok(Arc<V>)` (counters bumped identically).
    /// On a rejection returns `Err((rejected_fact, candidate_count))`
    /// where `rejected_fact` is a clone of the FIRST fact in the
    /// MOST-RECENT candidate (the back of the multi-candidate vec — the
    /// last admitted) that failed `view.validates*`, and
    /// `candidate_count` is the number of candidates considered.
    /// Returns `Err((None, 0))` when no entry exists in the cache at
    /// all (the `DashMap` shard has no key).
    ///
    /// Consumers feed `rejected_fact` to a domain-specific attribution
    /// helper (e.g. `prepared_decl_bundle_with_store_view`'s
    /// `attribute_prepared_decl_bundle_rejection`) so the matching
    /// per-rejection audit counter fires. Counters on this method
    /// mirror [`Self::get_if_valid_self_rooted`] exactly — the
    /// attribution caller adds NO extra counter beyond the per-cause
    /// `AuditEvent`.
    ///
    /// `FactVersionRef` is large by design (carries owned canonical
    /// strings + per-domain payloads); the attribution caller only
    /// inspects the discriminant + canonical, never stores the Err.
    /// Boxing would add a heap alloc per warm-read MISS — the wrong
    /// tradeoff for a hot-path helper, so the clippy lint is allowed.
    #[allow(clippy::result_large_err)]
    pub fn get_if_valid_self_rooted_attributed<TView>(
        &self,
        key: &K,
        view: &TView,
        self_root_canonicals: &[&str],
    ) -> Result<Arc<V>, (Option<FactVersionRef>, usize)>
    where
        TView: StoreView + ?Sized,
    {
        self.validations_attempted
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let entry = match self.entries.get(key) {
            Some(e) => e,
            None => return Err((None, 0)),
        };
        let candidates = entry.candidates.load();
        let candidate_count = candidates.len();
        // Mirror `get_if_valid_self_rooted`: iterate candidates in
        // admission order, return the first validating one.
        let mut last_rejected_fact: Option<FactVersionRef> = None;
        for candidate in candidates.iter() {
            let mut candidate_ok = true;
            for fact in candidate.fact_dep_signature.iter() {
                let fact_ok = match fact {
                    FactVersionRef::FileWholeHash { canonical_id, hash }
                        if self_root_canonicals.contains(&canonical_id.as_str()) =>
                    {
                        view.validates_self_root_whole_hash(canonical_id, hash)
                    }
                    other => view.validates(other),
                };
                if !fact_ok {
                    candidate_ok = false;
                    last_rejected_fact = Some(fact.clone());
                    break;
                }
            }
            if candidate_ok {
                self.warm_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Ok(candidate.value.clone());
            }
        }
        self.stale_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err((last_rejected_fact, candidate_count))
    }

    /// Like [`Self::get_if_valid`] but also returns the validating
    /// candidate's `fact_dep_signature`. Used by producers that must
    /// thread the recorded facts into a downstream cache entry
    /// (e.g. `OwnerImportSurfaceDb`) so dependent caches observe
    /// every chain participant — not only the final value.
    pub fn get_if_valid_with_facts<TView>(
        &self,
        key: &K,
        view: &TView,
    ) -> Option<(Arc<V>, Arc<[FactVersionRef]>)>
    where
        TView: StoreView + ?Sized,
    {
        self.validations_attempted
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let entry = self.entries.get(key)?;
        let candidates = entry.candidates.load();
        for candidate in candidates.iter() {
            let ok = candidate
                .fact_dep_signature
                .iter()
                .all(|fact| view.validates(fact));
            if ok {
                self.warm_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Some((
                    candidate.value.clone(),
                    Arc::clone(&candidate.fact_dep_signature),
                ));
            }
        }
        self.stale_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        None
    }

    pub fn insert(&self, key: K, value: V, facts: Vec<FactVersionRef>) {
        self.insert_arc(key, Arc::new(value), facts);
    }

    pub fn insert_arc(&self, key: K, value: Arc<V>, facts: Vec<FactVersionRef>) {
        // Loose admission. The fact-completeness empty-signature
        // guard is opt-in via `insert_arc_with_kind`; stable-miss
        // producers (e.g. `route_db`, `imported_root_db`) admit
        // through this path.
        self.insert_arc_inner(key, value, facts, None);
    }

    /// Admit with the fact-completeness guard ENABLED. R20 strict
    /// contract: empty signature → refuse + `FactSignatureAdmissionRefused`;
    /// over-cap → refuse + `FactSignatureOverflow`. `cache_kind`
    /// is the `'static str` discriminator on the refusal event.
    pub fn insert_arc_with_kind(
        &self,
        key: K,
        value: Arc<V>,
        facts: Vec<FactVersionRef>,
        cache_kind: &'static str,
    ) {
        self.insert_arc_inner(key, value, facts, Some(cache_kind));
    }

    fn insert_arc_inner(
        &self,
        key: K,
        value: Arc<V>,
        facts: Vec<FactVersionRef>,
        strict_cache_kind: Option<&'static str>,
    ) {
        // R20 signature-size bound. Reject candidates whose fact
        // signature exceeds FACT_SIGNATURE_CAP.
        if facts.len() > FACT_SIGNATURE_CAP {
            self.signature_overflow
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Best-effort typed-event emission. Failures (e.g., no
            // observer / accumulator installed on the current
            // thread) are silent — the counter is the authoritative
            // signal.
            crate::host_manage::push_structured_event(
                crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow {
                    candidate_size: facts.len() as u32,
                    cap: FACT_SIGNATURE_CAP as u32,
                },
            );
            return;
        }
        // R20 fact-completeness guard. Strict callers
        // (`insert_arc_with_kind`) refuse empty signatures so
        // producers must observe at least one fact before admit.
        if let Some(cache_kind) = strict_cache_kind {
            if facts.is_empty() {
                self.admission_refused
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::host_manage::push_structured_event(
                    crate::component_meta_audit::StructuredAuditEvent::FactSignatureAdmissionRefused {
                        cache_kind: Arc::from(cache_kind),
                        reason: verter_audit::AdmissionRefusalReason::EmptySignature,
                    },
                );
                return;
            }
        }
        let fact_arc: Arc<[FactVersionRef]> = Arc::from(facts.into_boxed_slice());
        let fingerprint = compute_signature_fingerprint(&fact_arc);
        let candidate = Arc::new(Candidate {
            signature_fingerprint: fingerprint,
            value,
            fact_dep_signature: fact_arc,
        });

        // Insert-or-update via DashMap. `entry().or_insert_with` is
        // not used because we need to retain the existing `Arc<CacheEntry>`
        // identity for `ArcSwap::rcu` to race-close correctly.
        let entry = self
            .entries
            .entry(key)
            .or_insert_with(|| Arc::new(CacheEntry::default()));
        let candidates_slot = &entry.candidates;
        candidates_slot.rcu(|old| {
            self.arcswap_stores
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut new: smallvec::SmallVec<[Arc<Candidate<V>>; CANDIDATE_CAP]> =
                smallvec::SmallVec::with_capacity(old.len() + 1);
            new.extend(old.iter().cloned());
            // FIFO eviction: drop the oldest entry to keep length at
            // CANDIDATE_CAP after the push.
            if new.len() >= CANDIDATE_CAP {
                let drop_count = new.len() - CANDIDATE_CAP + 1;
                new.drain(..drop_count);
            }
            new.push(Arc::clone(&candidate));
            new
        });
    }

    pub fn values(&self) -> Vec<Arc<V>> {
        let mut out = Vec::new();
        for entry in self.entries.iter() {
            let candidates = entry.value().candidates.load();
            for c in candidates.iter() {
                out.push(c.value.clone());
            }
        }
        out
    }

    /// Test-only: return the admitted `fact_dep_signature` of every
    /// candidate stored under `key`, regardless of validation. Used by
    /// the owner-Route fact-strip regression test to assert what the
    /// publish boundary actually admitted into the cache (a warm-read
    /// helper would hide a candidate that fails validation under the
    /// live view, but the strip contract is about the ADMITTED set).
    #[cfg(test)]
    pub fn candidate_signatures_for_key(&self, key: &K) -> Vec<Arc<[FactVersionRef]>> {
        match self.entries.get(key) {
            Some(entry) => entry
                .candidates
                .load()
                .iter()
                .map(|c| Arc::clone(&c.fact_dep_signature))
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn remove(&self, key: &K) {
        self.entries.remove(key);
    }

    /// Hard-remove: same as `remove` under the post-archive cache
    /// model. Retained for source compatibility; callers may migrate
    /// to `remove` directly.
    pub fn hard_remove(&self, key: &K) {
        self.entries.remove(key);
    }

    /// With the archive map retired, `invalidate` removes the entry
    /// outright. Concurrent generations of the same key are
    /// distinguished by per-candidate fact
    /// validation instead of an archive sidecar; superseded
    /// candidates age out via FIFO under [`CANDIDATE_CAP`].
    pub fn invalidate(&self, key: &K) {
        self.entries.remove(key);
    }

    /// Remove all entries whose key satisfies the predicate.
    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.entries.retain(|k, _| predicate(k));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn snapshot_all(&self) -> Vec<(K, Arc<V>)> {
        let mut out = Vec::new();
        for entry in self.entries.iter() {
            let candidates = entry.value().candidates.load();
            if let Some(c) = candidates.last() {
                out.push((entry.key().clone(), c.value.clone()));
            }
        }
        out
    }

    /// View-free permissive read: returns the last-admitted
    /// candidate's value for `key`, ignoring `fact_dep_signature`
    /// validation. Used by per-domain `StoreView` validators that
    /// need to consult the substrate without re-entering the view's
    /// `validates` dispatch (which would recurse).
    ///
    /// Returns `None` when the slot has no entries.
    #[must_use]
    pub fn lookup_any_candidate(&self, key: &K) -> Option<Arc<V>> {
        let entry = self.entries.get(key)?;
        let candidates = entry.candidates.load();
        candidates.last().map(|c| c.value.clone())
    }

    /// R20 instrumentation: number of times an over-cap
    /// `fact_dep_signature` was rejected.
    pub fn signature_overflow_count(&self) -> u64 {
        self.signature_overflow
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R20 instrumentation: number of admissions refused by the
    /// fact-completeness guard. Pre-canary asserts this stays 0
    /// over steady-state load.
    pub fn admission_refused_count(&self) -> u64 {
        self.admission_refused
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R20 instrumentation: number of `ArcSwap::store` (rcu)
    /// calls observed by this substrate. Hot-path reads must
    /// never advance this counter.
    pub fn arcswap_store_count(&self) -> u64 {
        self.arcswap_stores
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R24 instrumentation: total `get_if_valid` calls attempted.
    pub fn validations_attempted_count(&self) -> u64 {
        self.validations_attempted
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R24 instrumentation: warm hits.
    pub fn warm_hit_count(&self) -> u64 {
        self.warm_hits.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R24 instrumentation: stale misses (entry existed but no
    /// candidate validated under the active view).
    pub fn stale_miss_count(&self) -> u64 {
        self.stale_misses.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// R24 instrumentation: archive-style fallback checks. Always
    /// 0 on the post-archive cache substrate.
    pub fn archive_check_count(&self) -> u64 {
        self.archive_checks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Drain the four R24 validation counters and emit a typed
    /// [`StructuredAuditEvent::FactValidationSummary`] event
    /// attributing the captured totals to `request_id` /
    /// `cache_kind`. Best-effort emission — silent no-op when no
    /// observer accumulator is installed.
    ///
    /// Drains the counters atomically (via `swap`) so a follow-up
    /// pass starts at zero. Callers that want a non-destructive
    /// read can use the four `*_count` accessors directly.
    pub fn emit_validation_summary_for_request(&self, request_id: u64, cache_kind: &'static str) {
        let validations_attempted =
            self.validations_attempted
                .swap(0, std::sync::atomic::Ordering::Relaxed) as u32;
        let warm_hits = self.warm_hits.swap(0, std::sync::atomic::Ordering::Relaxed) as u32;
        let stale_misses = self
            .stale_misses
            .swap(0, std::sync::atomic::Ordering::Relaxed) as u32;
        let archive_checks = self
            .archive_checks
            .swap(0, std::sync::atomic::Ordering::Relaxed) as u32;
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FactValidationSummary {
                request_id,
                cache_kind: Arc::from(cache_kind),
                validations_attempted,
                warm_hits,
                stale_misses,
                archive_checks,
            },
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleflightRole {
    Leader,
    Follower,
}

#[derive(Debug, Clone)]
pub struct SingleflightRunResult<V> {
    pub value: Arc<V>,
    pub role: SingleflightRole,
    pub forked_lane: bool,
}

/// **Test-only.** `Debug`-able wrapper around an optional non-retained seam
/// hook (a `Box<dyn Fn>` is not `Debug`, so it cannot live directly in a
/// `#[derive(Debug)]` struct). Exists only under `cfg(test)`.
#[cfg(test)]
#[derive(Default)]
struct SeamHookSlot(Mutex<Option<Box<dyn Fn() + Send + Sync>>>);

#[cfg(test)]
impl std::fmt::Debug for SeamHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let installed = self.0.lock().is_some();
        f.debug_struct("SeamHookSlot")
            .field("installed", &installed)
            .finish()
    }
}

/// The singleflight lane map plus the per-key live-token index, both
/// guarded by ONE mutex so they can never diverge.
///
/// `live_tokens` is the fork-telemetry source: a multiset of the tokens
/// whose lane for a key is LIVE (`Pending` / `Running`). It is
/// maintained at the lane transitions (insert, terminal publish, abort,
/// reap) so the claim-time "is another token's lane in flight for this
/// key?" check is an O(1) index read instead of an O(#lanes) scan that
/// acquired every lane's inner mutex under the map lock. Lanes that
/// reach a terminal NON-LIVE state are unindexed in the same critical
/// section that publishes the terminal: a retained `Done` at its
/// publish, a non-retained `Done` and an `Aborted` lane at their
/// publish+remove, and a never-claimed `Pending` lane at its reap.
/// `FlightState::live_indexed` makes the unindex exactly-once.
#[derive(Debug)]
struct FlightTable<K, V, E>
where
    K: Eq + Hash,
{
    #[allow(clippy::type_complexity)]
    lanes: FxHashMap<(K, StoreViewCompatToken), Arc<FlightState<V, E>>>,
    live_tokens: FxHashMap<K, smallvec::SmallVec<[StoreViewCompatToken; 2]>>,
}

impl<K, V, E> Default for FlightTable<K, V, E>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            lanes: FxHashMap::default(),
            live_tokens: FxHashMap::default(),
        }
    }
}

impl<K, V, E> FlightTable<K, V, E>
where
    K: Eq + Hash + Clone,
{
    /// Index a freshly-inserted LIVE (`Pending` / `Running`) lane.
    /// Caller holds the table mutex.
    fn mark_live(&mut self, key: &K, token: StoreViewCompatToken, state: &FlightState<V, E>) {
        state
            .live_indexed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.live_tokens.entry(key.clone()).or_default().push(token);
    }

    /// Unindex a lane that is leaving the LIVE set (terminal publish,
    /// abort, or reap of a never-claimed `Pending` lane). Exactly-once
    /// per lane via the `live_indexed` flag, so a path that both
    /// publishes and later reaps the same lane removes one token
    /// occurrence, not two. Caller holds the table mutex.
    fn clear_live(&mut self, key: &K, token: StoreViewCompatToken, state: &FlightState<V, E>) {
        if !state
            .live_indexed
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        if let Some(tokens) = self.live_tokens.get_mut(key) {
            if let Some(pos) = tokens.iter().position(|t| *t == token) {
                tokens.swap_remove(pos);
            }
            if tokens.is_empty() {
                self.live_tokens.remove(key);
            }
        }
    }

    /// Fork telemetry: is another token's lane LIVE for this key?
    fn has_other_live_token(&self, key: &K, token: StoreViewCompatToken) -> bool {
        self.live_tokens
            .get(key)
            .is_some_and(|tokens| tokens.iter().any(|t| *t != token))
    }
}

#[derive(Debug)]
pub struct SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    flights: Mutex<FlightTable<K, V, E>>,
    /// **Test-only.** A rendezvous hook fired on the LEADER thread inside the
    /// NON-RETAINED (`keep == false`) terminal, strictly between the `Done`
    /// publish and the lane removal, WHILE the `flights` lock is held. Tests
    /// install it to prove the publish+remove window is one continuously-held
    /// critical section (P1b). It carries zero footprint in production
    /// builds (the field exists only under `cfg(test)`).
    #[cfg(test)]
    non_retained_seam_hook: SeamHookSlot,
}

#[derive(Debug)]
struct FlightState<V, E> {
    inner: Mutex<FlightInner<V, E>>,
    ready: Condvar,
    /// Count of threads currently pinning this flight lane — the
    /// per-request participation guards held across a whole
    /// [`run_stable_request`] plus the transient self-pin every
    /// [`SingleflightGroup::run`] caller takes. Mutated ONLY under the
    /// group's `flights` map lock (incremented when a pin is acquired,
    /// decremented + reaped when it is released), so every increment is
    /// serialized against the decrement-and-reap.
    ///
    /// The leader keeps its published `Done` result in the map as a
    /// joinable rendezvous and the slot is reaped only when the LAST
    /// pin is released (count reaches zero). This is what makes a
    /// concurrent burst collapse onto ONE cold leader deterministically:
    /// every caller pins the lane via [`SingleflightGroup::participate`]
    /// BEFORE it even peeks its cache, so the lane stays alive for the
    /// whole burst — a straggler that decided "miss" but has not yet
    /// reached the `run` claim still observes the leader's retained
    /// `Done` (because its own participation pin, and every sibling's,
    /// keeps the slot from being reaped) and joins as a `Follower`
    /// instead of spawning a second cold `Leader`.
    ///
    /// Because the slot is dropped once the burst fully drains, the
    /// flight is a per-burst dedup rendezvous, NOT a result cache: a
    /// later independent call re-enters the cold path (preserving the
    /// "non-cacheable / empty-fact results are not persisted" contract
    /// the validated caches own).
    pins: std::sync::atomic::AtomicUsize,
    /// Whether this lane currently holds an occurrence in the group's
    /// per-key [`FlightTable::live_tokens`] fork-telemetry index. Set
    /// at lane insert, cleared exactly once when the lane leaves the
    /// LIVE (`Pending` / `Running`) set. Mutated ONLY under the
    /// `flights` table lock (`Relaxed` suffices — the lock orders it).
    live_indexed: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone)]
enum FlightInner<V, E> {
    /// Lane pinned by a participation guard but no [`SingleflightGroup::run`]
    /// caller has claimed leadership yet. The first `run` caller
    /// transitions this to `Running` and becomes the leader.
    Pending,
    Running {
        owner: std::thread::ThreadId,
    },
    Done(Result<Arc<V>, E>),
    /// The leader's `compute`/`retain` PANICKED before it could publish a
    /// terminal. Set by the leader's panic-abort guard (`LeaderAbortGuard`)
    /// while it unwinds, so waiters parked on `ready` do not block forever
    /// behind a thread that will never publish. A waiter that wakes to
    /// `Aborted` releases its pin and RE-ELECTS against a fresh lane (the
    /// aborted lane is removed under the `flights` lock as part of the
    /// abort, so the re-election creates a new flight). A flight never
    /// LEAVES `Aborted` — the variant exists only to wake and redirect
    /// waiters; the lane carrying it is already detached from the map.
    Aborted,
}

/// RAII pin that keeps a flight lane alive for the duration of a
/// participating request. Acquired by [`SingleflightGroup::participate`]
/// at the very start of [`run_stable_request`] — BEFORE the pre-flight
/// cache peek — so the lane a leader publishes into is not reaped out
/// from under a concurrent straggler that is still between its own
/// cache-miss decision and its `run` claim. Dropping the guard releases
/// the pin and reaps the lane when it was the last one.
pub struct FlightParticipation<'a, K, V, E>
where
    K: Eq + Hash + Clone,
{
    group: &'a SingleflightGroup<K, V, E>,
    lane_key: (K, StoreViewCompatToken),
    state: Arc<FlightState<V, E>>,
}

impl<'a, K, V, E> Drop for FlightParticipation<'a, K, V, E>
where
    K: Eq + Hash + Clone,
{
    fn drop(&mut self) {
        self.group.unpin(&self.lane_key, &self.state);
    }
}

/// Panic-safety guard around the leader's `compute` + `retain` window in
/// [`SingleflightGroup::run_retaining`]. While `armed`, an unwind through
/// this guard's `Drop` (a panic from `compute` or from the `retain`
/// predicate) ABORTS the leader's lane so no waiter is left blocked behind a
/// thread that will never publish a terminal:
///
/// 1. transition the lane's `inner` to [`FlightInner::Aborted`] and
///    `notify_all`, so every parked waiter wakes and RE-ELECTS;
/// 2. release the leader's self-pin (`fetch_sub` under the `flights` lock,
///    preserving the `FlightState::pins` "mutated ONLY under the `flights`
///    map lock" invariant);
/// 3. remove the lane from the map (`ptr_eq`-guarded so a fresh leader's
///    re-inserted slot for the same key is not evicted).
///
/// All three run in ONE critical section holding the `flights` lock across
/// them (lock order `flights` → `inner`), exactly as the non-retained
/// terminal does — so the aborted lane is never observable to a NEW
/// claimant. The panic then continues to unwind past `Drop`.
///
/// On the SUCCESS path the leader sets `armed = false` the instant
/// `compute` + `retain` have returned (BEFORE publishing the real terminal),
/// so the guard's `Drop` is a no-op and the normal terminal logic owns the
/// lane transition. The guard never publishes a result — it only redirects
/// waiters and tears the lane down on panic.
struct LeaderAbortGuard<'a, K, V, E>
where
    K: Eq + Hash + Clone,
{
    group: &'a SingleflightGroup<K, V, E>,
    lane_key: &'a (K, StoreViewCompatToken),
    state: &'a Arc<FlightState<V, E>>,
    armed: bool,
}

impl<'a, K, V, E> Drop for LeaderAbortGuard<'a, K, V, E>
where
    K: Eq + Hash + Clone,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // UNWINDING: the leader's `compute`/`retain` panicked before it
        // published a terminal. Abort the lane atomically under the
        // `flights` lock so waiters re-elect instead of blocking forever.
        let mut flights = self.group.flights.lock();
        {
            let mut inner = self.state.inner.lock();
            *inner = FlightInner::Aborted;
            self.state.ready.notify_all();
        }
        self.state
            .pins
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        flights.clear_live(&self.lane_key.0, self.lane_key.1, self.state);
        if flights
            .lanes
            .get(self.lane_key)
            .is_some_and(|existing| Arc::ptr_eq(existing, self.state))
        {
            flights.lanes.remove(self.lane_key);
        }
    }
}

impl<K, V, E> Default for SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            flights: Mutex::new(FlightTable::default()),
            #[cfg(test)]
            non_retained_seam_hook: SeamHookSlot::default(),
        }
    }
}

impl<K, V, E> SingleflightGroup<K, V, E>
where
    K: Eq + Hash + Clone,
{
    /// Pin a flight lane for the lifetime of the returned guard so the
    /// lane a leader publishes into is not reaped while this caller is
    /// still between its own cache-miss decision and its [`Self::run`]
    /// claim. Acquired at the very start of [`run_stable_request`],
    /// BEFORE the pre-flight cache peek. Creates the lane in a `Pending`
    /// state if it does not yet exist (the first `run` caller claims
    /// leadership of it); otherwise it pins whatever flight is already in
    /// progress or retained.
    pub fn participate(
        &self,
        key: K,
        token: StoreViewCompatToken,
    ) -> FlightParticipation<'_, K, V, E> {
        let lane_key = (key, token);
        let state = {
            let mut flights = self.flights.lock();
            if let Some(existing) = flights.lanes.get(&lane_key).cloned() {
                existing
                    .pins
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                existing
            } else {
                let state = Arc::new(FlightState {
                    inner: Mutex::new(FlightInner::Pending),
                    ready: Condvar::new(),
                    pins: std::sync::atomic::AtomicUsize::new(1),
                    live_indexed: std::sync::atomic::AtomicBool::new(false),
                });
                flights.lanes.insert(lane_key.clone(), state.clone());
                flights.mark_live(&lane_key.0, lane_key.1, &state);
                state
            }
        };
        FlightParticipation {
            group: self,
            lane_key,
            state,
        }
    }

    /// Release one pin on a flight lane and reap the slot when the last
    /// pin is released.
    ///
    /// The decrement and the conditional removal run together under the
    /// `flights` map lock so they are serialized against every pin-side
    /// increment ([`Self::run`]'s self-pin and [`Self::participate`]):
    /// the slot can only be reaped at the instant `pins` reaches zero,
    /// which is also the instant no thread can be mid-claim (a claimer
    /// increments under the same lock before it ever reads the slot). The
    /// removal is `ptr_eq`-guarded so a fresh leader that already
    /// re-inserted a new slot for the same `lane_key` (after this one
    /// drained) is not evicted.
    fn unpin(&self, lane_key: &(K, StoreViewCompatToken), state: &Arc<FlightState<V, E>>) {
        let mut flights = self.flights.lock();
        let remaining = state
            .pins
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
            - 1;
        if remaining == 0 {
            if let Some(existing) = flights.lanes.get(lane_key) {
                if Arc::ptr_eq(existing, state) {
                    flights.lanes.remove(lane_key);
                    // A never-claimed `Pending` lane reaped here is
                    // still live-indexed (no terminal ever published);
                    // unindex it. A retained-`Done` lane was already
                    // unindexed at its publish (exactly-once flag).
                    flights.clear_live(&lane_key.0, lane_key.1, state);
                }
            }
        }
    }
}

impl<K, V, E> SingleflightGroup<K, V, E>
where
    K: Eq + Hash + Clone,
    E: Clone,
{
    pub fn run<F>(
        &self,
        key: K,
        token: StoreViewCompatToken,
        compute: F,
    ) -> Result<SingleflightRunResult<V>, E>
    where
        F: FnOnce() -> Result<V, E>,
    {
        // Default: a successful result is retained as a joinable
        // rendezvous for the burst. Errors are never retained (see
        // `run_retaining`).
        self.run_retaining(key, token, compute, |_| true)
    }

    /// Like [`Self::run`] but the caller decides — via `retain`,
    /// evaluated on the leader's freshly-computed value — whether the
    /// result is retained as a joinable `Done` rendezvous for late burst
    /// members, or discarded immediately so every subsequent claim
    /// (including the same request's own stability retry) recomputes.
    ///
    /// [`run_stable_request`] passes `retain = |v| v.stable`: a STABLE
    /// result is retained for the burst (a concurrent straggler joins it
    /// instead of cold-recomputing); an UNSTABLE result (the snapshot
    /// view moved mid-compute) is NOT retained, so the retry loop — and
    /// any sibling — recomputes against fresh state rather than joining a
    /// torn result. Errors are never retained.
    pub fn run_retaining<F, R>(
        &self,
        key: K,
        token: StoreViewCompatToken,
        compute: F,
        retain: R,
    ) -> Result<SingleflightRunResult<V>, E>
    where
        F: FnOnce() -> Result<V, E>,
        R: FnOnce(&V) -> bool,
    {
        let lane_key = (key.clone(), token);
        let current_thread = std::thread::current().id();

        // `compute` / `retain` are `FnOnce`; a re-electing waiter (one woken
        // to a leader's `Aborted` lane) may itself become the fresh leader
        // and run them. They are consumed AT MOST once across re-election
        // iterations (every path that consumes them then returns), so the
        // `Option::take` is the single-shot carrier the borrow checker
        // needs across the loop.
        let mut compute = Some(compute);
        let mut retain = Some(retain);

        'reelect: loop {
            // Take a self-pin and either claim leadership or take a
            // reference to an existing flight, ALL under the map lock.
            // `pins` is incremented here for the self-pin; `participate`
            // pins are added and released under the same lock, so every
            // increment is serialized against the decrement-and-reap
            // (`unpin`). The lane is never reaped while any pin is held — so
            // a caller arriving in the leader's post-compute gap (but pinned
            // via `participate`) observes the retained `Done` slot rather
            // than a vacant lane.
            //
            // `leader` is `true` when this caller is responsible for running
            // `compute`: either it created the slot, or it found a `Pending`
            // slot a participation pin had created and claimed leadership of
            // it.
            let (state, leader, forked_lane) = {
                let mut flights = self.flights.lock();
                // Fork telemetry: O(1) read of the per-key live-token
                // index (maintained at lane transitions) — true when
                // another token's lane is in flight for this key.
                let forked_lane = flights.has_other_live_token(&key, token);
                if let Some(existing) = flights.lanes.get(&lane_key).cloned() {
                    existing
                        .pins
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Claim a Pending lane (created by a participation pin)
                    // by transitioning it to Running under this thread.
                    let mut leader = false;
                    {
                        let mut inner = existing.inner.lock();
                        if matches!(&*inner, FlightInner::Pending) {
                            *inner = FlightInner::Running {
                                owner: current_thread,
                            };
                            leader = true;
                        }
                    }
                    (existing, leader, forked_lane)
                } else {
                    let state = Arc::new(FlightState {
                        inner: Mutex::new(FlightInner::Running {
                            owner: current_thread,
                        }),
                        ready: Condvar::new(),
                        pins: std::sync::atomic::AtomicUsize::new(1),
                        live_indexed: std::sync::atomic::AtomicBool::new(false),
                    });
                    flights.lanes.insert(lane_key.clone(), state.clone());
                    flights.mark_live(&lane_key.0, lane_key.1, &state);
                    (state, true, forked_lane)
                }
            };

            if leader {
                // Run `compute` + `retain` under a panic-abort guard. If
                // either panics, the guard's `Drop` aborts the lane (sets
                // `Aborted`, notifies waiters, releases this self-pin,
                // removes the lane) so no waiter blocks forever behind a
                // leader that never publishes — then the panic resumes. On
                // the success path the guard is disarmed BEFORE the real
                // terminal is published, so it is a no-op and the normal
                // logic below owns the lane transition.
                let mut abort_guard = LeaderAbortGuard {
                    group: self,
                    lane_key: &lane_key,
                    state: &state,
                    armed: true,
                };
                let result =
                    (compute.take().expect("compute consumed at most once"))().map(Arc::new);
                // Decide retention from the freshly-computed value. A
                // retained (stable) result stays in the map as a joinable
                // rendezvous; a non-retained result (an error, or a value
                // the caller declines to retain such as an unstable one)
                // must NOT be joinable by a NEW claimant — only by waiters
                // already committed to THIS flight, who treat it exactly as
                // a fresh leader would (an unstable result drives them to
                // recompute; an error propagates).
                let keep = matches!(&result, Ok(value)
                    if (retain.take().expect("retain consumed at most once"))(value));
                // Past the last point that can panic — disarm so the guard
                // does not also tear down the lane we are about to publish
                // into.
                abort_guard.armed = false;
                drop(abort_guard);

                if keep {
                    // Publish the stable result, then release this self-pin
                    // via `unpin`. The result stays in the map as a joinable
                    // rendezvous; the slot is reclaimed once the last pin is
                    // released, so a burst member still mid-claim joins it
                    // instead of finding a vacant lane. New claimants joining
                    // a STABLE `Done` is correct and is the whole point.
                    // The publish runs under the `flights` lock (order
                    // `flights` → `inner`, as everywhere) so the lane
                    // leaves the live-token fork-telemetry index in the
                    // same critical section that makes it `Done`.
                    {
                        let mut flights = self.flights.lock();
                        {
                            let mut inner = state.inner.lock();
                            *inner = FlightInner::Done(result.clone());
                            state.ready.notify_all();
                        }
                        flights.clear_live(&lane_key.0, lane_key.1, &state);
                    }
                    self.unpin(&lane_key, &state);
                } else {
                    // Non-retained terminal: publish the `Done` to existing
                    // waiters AND remove the lane in ONE critical section
                    // that holds the `flights` lock across both, so a NEW
                    // claimant (which must take this same `flights` lock
                    // before it can observe the lane's state) can NEVER see
                    // the non-retained `Done` — it finds the lane already
                    // gone and re-elects a fresh leader. Without removing
                    // under the `flights` lock, a claimant could enter
                    // between publish and removal, read `Done(unstable_or_err)`,
                    // and return it as a Follower — joining a result that
                    // must never be retained (an error would wrongly
                    // propagate; an unstable result would be observed before
                    // the recompute). Waiters ALREADY parked on `state.ready`
                    // hold their own `Arc` and are woken by `notify_all`;
                    // they re-check under `inner` after this critical section
                    // and act on the `Done` exactly as a fresh leader's
                    // outcome dictates.
                    //
                    // Lock order is preserved (`flights` then `inner`). The
                    // self-pin `fetch_sub` runs under the `flights` lock too,
                    // so the `FlightState::pins` "mutated ONLY under the
                    // `flights` map lock" invariant holds literally. The
                    // removal is `ptr_eq`-guarded so a fresh leader that
                    // already re-inserted a new slot for the same `lane_key`
                    // is not evicted; any still-outstanding participation pin
                    // on this (now-orphaned) `state` becomes a no-op on its
                    // own `unpin` via the same guard.
                    let mut flights = self.flights.lock();
                    {
                        let mut inner = state.inner.lock();
                        *inner = FlightInner::Done(result.clone());
                        state.ready.notify_all();
                    }
                    state
                        .pins
                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    flights.clear_live(&lane_key.0, lane_key.1, &state);
                    // Test-only rendezvous: fire the seam hook AFTER the
                    // publish and BEFORE the removal, while `flights` is still
                    // held — the window a test probes to prove publish+remove
                    // is one atomic critical section (the `flights` lock is
                    // observably held here). Zero footprint outside `cfg(test)`.
                    #[cfg(test)]
                    if let Some(hook) = self.non_retained_seam_hook.0.lock().as_ref() {
                        hook();
                    }
                    if flights
                        .lanes
                        .get(&lane_key)
                        .is_some_and(|existing| Arc::ptr_eq(existing, &state))
                    {
                        flights.lanes.remove(&lane_key);
                    }
                }
                return result.map(|value| SingleflightRunResult {
                    value,
                    role: SingleflightRole::Leader,
                    forked_lane,
                });
            }

            let mut inner = state.inner.lock();
            loop {
                match &*inner {
                    FlightInner::Running { owner } if *owner == current_thread => {
                        // Same-thread re-entry on a flight this thread itself
                        // leads (recursion sentinel): compute inline as a
                        // nested leader. Release the self-pin taken above.
                        drop(inner);
                        self.unpin(&lane_key, &state);
                        return (compute.take().expect("compute consumed at most once"))().map(
                            |value| SingleflightRunResult {
                                value: Arc::new(value),
                                role: SingleflightRole::Leader,
                                forked_lane,
                            },
                        );
                    }
                    // A `Pending` lane is claimed by the first `run` caller
                    // in the block above; any thread that reaches the loop in
                    // `Pending` state is racing a sibling that is about to
                    // claim leadership — wait for the transition.
                    FlightInner::Pending => state.ready.wait(&mut inner),
                    FlightInner::Running { .. } => state.ready.wait(&mut inner),
                    FlightInner::Done(result) => {
                        let result = result.clone();
                        drop(inner);
                        self.unpin(&lane_key, &state);
                        return result.map(|value| SingleflightRunResult {
                            value,
                            role: SingleflightRole::Follower,
                            forked_lane,
                        });
                    }
                    FlightInner::Aborted => {
                        // The leader this waiter joined PANICKED and aborted
                        // the lane. Release this waiter's self-pin and
                        // re-elect against a fresh lane: the aborted lane was
                        // removed from the map as part of the abort, so the
                        // next claim creates a new flight (one re-electing
                        // waiter becomes the fresh leader and runs its own
                        // `compute`; the rest join it). This is what keeps a
                        // leader-panic from wedging every waiter forever.
                        drop(inner);
                        self.unpin(&lane_key, &state);
                        continue 'reelect;
                    }
                }
            }
        }
    }

    pub fn clear(&self) {
        let mut flights = self.flights.lock();
        flights.lanes.clear();
        flights.live_tokens.clear();
    }

    /// **Test-only.** Strong-reference count of the in-flight
    /// [`FlightState`] `Arc` for `(key, token)`, or `0` if no flight is
    /// currently registered for that lane.
    ///
    /// While only the leader is mid-`compute`, a leader using BARE
    /// [`Self::run`] / [`Self::run_retaining`] (no participation pin) has
    /// the leader-only baseline of 2: the leader's local `state` binding
    /// plus the `flights` map entry. A caller that also holds a
    /// [`Self::participate`] guard on the same lane contributes ONE
    /// additional strong ref, so a parked leader reached through
    /// [`run_stable_request`] — where every caller pins via `participate`
    /// at the top, BEFORE its pre-flight cache peek — has a baseline of 3
    /// (local `state` + `flights` map entry + `participate` guard clone).
    /// Each additional `participate`+`run_retaining` follower then adds up
    /// to two: first its `participate` guard clone, then (once past its
    /// peek and committed to the condvar wait as a Follower) its
    /// `run_retaining`/join clone. Polling this is a deterministic
    /// alternative to a wall-clock `sleep` for observing follower
    /// admission onto the singleflight — it does not race the follower
    /// under parallel load.
    ///
    /// Exposed under `cfg(any(test, debug_assertions))` so integration
    /// tests in `tests/` (which compile without `cfg(test)`) can reach it
    /// transitively through the per-DB test-only accessors.
    #[cfg(any(test, debug_assertions))]
    pub fn test_flight_strong_count(&self, key: &K, token: StoreViewCompatToken) -> usize {
        let lane_key = (key.clone(), token);
        self.flights
            .lock()
            .lanes
            .get(&lane_key)
            .map(Arc::strong_count)
            .unwrap_or(0)
    }

    /// **Test-only.** Install the non-retained seam hook (see
    /// [`SingleflightGroup::non_retained_seam_hook`]). The hook fires on the
    /// leader thread inside the `keep == false` terminal, strictly between
    /// the `Done` publish and the lane removal, while the `flights` lock is
    /// held — the deterministic rendezvous a test uses to prove the
    /// publish+remove window is one continuously-held critical section.
    #[cfg(test)]
    pub fn set_non_retained_seam_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        *self.non_retained_seam_hook.0.lock() = Some(hook);
    }
}

// ---------------------------------------------------------------------------
// Observability counters
// ---------------------------------------------------------------------------

/// Atomic counters for resolver observability.
///
/// Thread-safe via `AtomicU64`. The resolver increments these during resolution;
/// consumers read snapshots via `snapshot()` for diagnostics, benchmarks, and tests.
#[derive(Debug, Default)]
pub struct ResolverCounters {
    /// Number of times a cached node result was reused (fact-validated hit).
    pub node_cache_hits: std::sync::atomic::AtomicU64,
    /// Number of times a node had to be recomputed (cache miss or stale).
    pub node_cache_misses: std::sync::atomic::AtomicU64,
    /// Number of times singleflight coalesced a follower onto an in-flight leader.
    pub singleflight_coalesces: std::sync::atomic::AtomicU64,
    /// Number of cycle detections during resolution.
    pub cycle_detections: std::sync::atomic::AtomicU64,
    /// Number of times incompatible StoreViews forked separate singleflight lanes.
    pub cross_view_lane_forks: std::sync::atomic::AtomicU64,
    /// Number of route/barrel fact reuses (cached route entries validated and reused).
    pub route_fact_reuses: std::sync::atomic::AtomicU64,
}

/// A non-atomic snapshot of `ResolverCounters` for reading/comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResolverCountersSnapshot {
    pub node_cache_hits: u64,
    pub node_cache_misses: u64,
    pub singleflight_coalesces: u64,
    pub cycle_detections: u64,
    pub cross_view_lane_forks: u64,
    pub route_fact_reuses: u64,
}

impl ResolverCounters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ResolverCountersSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        ResolverCountersSnapshot {
            node_cache_hits: self.node_cache_hits.load(Relaxed),
            node_cache_misses: self.node_cache_misses.load(Relaxed),
            singleflight_coalesces: self.singleflight_coalesces.load(Relaxed),
            cycle_detections: self.cycle_detections.load(Relaxed),
            cross_view_lane_forks: self.cross_view_lane_forks.load(Relaxed),
            route_fact_reuses: self.route_fact_reuses.load(Relaxed),
        }
    }

    pub fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.node_cache_hits.store(0, Relaxed);
        self.node_cache_misses.store(0, Relaxed);
        self.singleflight_coalesces.store(0, Relaxed);
        self.cycle_detections.store(0, Relaxed);
        self.cross_view_lane_forks.store(0, Relaxed);
        self.route_fact_reuses.store(0, Relaxed);
    }

    #[inline]
    pub fn record_cache_hit(&self) {
        self.node_cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cache_miss(&self) {
        self.node_cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_singleflight_coalesce(&self) {
        self.singleflight_coalesces
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cycle_detection(&self) {
        self.cycle_detections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_cross_view_lane_fork(&self) {
        self.cross_view_lane_forks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[inline]
    pub fn record_route_fact_reuse(&self) {
        self.route_fact_reuses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

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

    struct TestRequestExecutor {
        key: String,
        cache: ValidatedFactCache<String, usize>,
        valid_fact: FactVersionRef,
        token: StoreViewCompatToken,
        compute_values: VecDeque<usize>,
        stability: VecDeque<bool>,
        published: Vec<usize>,
        computes: usize,
        max_attempts: usize,
        last_stable: bool,
        /// Currentness this executor reports from
        /// [`StableRequestExecutor::snapshot_view_is_current`]. Defaults
        /// to `true`; the non-current-lane-isolation test sets it `false`
        /// to model the manager handing back a `ReturnOnly` seed.
        current: bool,
    }

    impl TestRequestExecutor {
        fn new(key: &str, token: StoreViewCompatToken, max_attempts: usize) -> Self {
            Self {
                key: key.to_string(),
                cache: ValidatedFactCache::default(),
                valid_fact: FactVersionRef::FileWholeHash {
                    canonical_id: "/src/App.vue".to_string(),
                    hash: [1; 16],
                },
                token,
                compute_values: VecDeque::new(),
                stability: VecDeque::new(),
                published: Vec::new(),
                computes: 0,
                max_attempts,
                last_stable: true,
                current: true,
            }
        }

        /// Mark this executor's snapshot as non-current (the manager could
        /// not prove the view current — a `ReturnOnly` seed).
        fn non_current(mut self) -> Self {
            self.current = false;
            self
        }

        fn view(&self) -> TestView {
            TestView {
                token: self.token,
                valid_facts: [self.valid_fact.clone()].into_iter().collect(),
            }
        }
    }

    impl StableRequestExecutor<String, usize> for TestRequestExecutor {
        type View = TestView;
        type Error = &'static str;

        fn cache_key(&self) -> String {
            self.key.clone()
        }

        fn snapshot_view(&mut self) -> Self::View {
            self.view()
        }

        fn snapshot_view_is_current(&self) -> bool {
            self.current
        }

        fn try_get_cached(&mut self, view: &Self::View) -> Option<usize> {
            self.cache
                .get_if_valid(&self.key, view)
                .map(|cached| *cached)
        }

        fn compute(&mut self, _view: &Self::View) -> Result<usize, Self::Error> {
            self.computes += 1;
            self.last_stable = self.stability.pop_front().unwrap_or(true);
            self.compute_values
                .pop_front()
                .ok_or("missing compute value")
        }

        fn is_stable(&mut self, _view: &Self::View) -> bool {
            self.last_stable
        }

        fn store_stable(&mut self, value: &usize) {
            self.published.push(*value);
            self.cache
                .insert(self.key.clone(), *value, vec![self.valid_fact.clone()]);
        }

        fn max_attempts(&self) -> usize {
            self.max_attempts
        }
    }

    #[test]
    fn validated_cache_reuses_entry_when_all_facts_match() {
        let cache = ValidatedFactCache::<String, usize>::default();
        let fact = FactVersionRef::FileWholeHash {
            canonical_id: "/src/App.vue".to_string(),
            hash: [7; 16],
        };
        cache.insert("node".to_string(), 42, vec![fact.clone()]);

        let view = TestView {
            token: StoreViewCompatToken {
                epoch: 3,
                session: None,
                validity_fingerprint: 0,
            },
            valid_facts: [fact].into_iter().collect(),
        };

        assert_eq!(
            cache.get_if_valid(&"node".to_string(), &view),
            Some(Arc::new(42))
        );
    }

    #[test]
    fn validated_cache_rejects_entry_when_any_fact_mismatches() {
        let cache = ValidatedFactCache::<String, usize>::default();
        cache.insert(
            "node".to_string(),
            42,
            vec![FactVersionRef::FileWholeHash {
                canonical_id: "/src/index.ts".to_string(),
                hash: [99u8; 16],
            }],
        );

        let view = TestView {
            token: StoreViewCompatToken {
                epoch: 4,
                session: None,
                validity_fingerprint: 0,
            },
            valid_facts: FxHashSet::default(),
        };

        assert!(cache.get_if_valid(&"node".to_string(), &view).is_none());
    }

    #[test]
    fn compat_token_identity_includes_validity_fingerprint() {
        // The coalescing-lane token is a COMPLETE oracle: equal iff every
        // dimension matches. Two tokens with the SAME epoch + session but
        // a DIFFERENT `validity_fingerprint` (a validity-affecting change
        // the epoch does not cover — artifact / route / load generation,
        // env, identity, overlay) must NOT compare equal, so they never
        // share a singleflight/stability lane.
        let first = StoreViewCompatToken {
            epoch: 10,
            session: None,
            validity_fingerprint: 0,
        };
        let same = StoreViewCompatToken {
            epoch: 10,
            session: None,
            validity_fingerprint: 0,
        };
        let diff_epoch = StoreViewCompatToken {
            epoch: 11,
            session: None,
            validity_fingerprint: 0,
        };
        let diff_fingerprint = StoreViewCompatToken {
            epoch: 10,
            session: None,
            validity_fingerprint: 0xDEAD_BEEF,
        };

        assert_eq!(first, same);
        assert_ne!(first, diff_epoch);
        assert_ne!(
            first, diff_fingerprint,
            "a token differing only in validity_fingerprint MUST NOT compare equal — \
             else two views with distinct complete tokens would wrongly coalesce"
        );
    }

    #[test]
    fn stable_request_returns_cached_value_before_compute() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
                validity_fingerprint: 0,
            },
            3,
        );
        executor
            .cache
            .insert("node".to_string(), 41, vec![executor.valid_fact.clone()]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 41);
        assert_eq!(result.source, RequestSource::Cache);
        assert_eq!(result.attempts, 1);
        assert_eq!(executor.computes, 0);
        assert!(executor.published.is_empty());
    }

    #[test]
    fn stable_request_retries_until_compute_is_stable() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
                validity_fingerprint: 0,
            },
            3,
        );
        executor.compute_values.extend([11, 12]);
        executor.stability.extend([false, true]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 12);
        assert_eq!(
            result.source,
            RequestSource::Flight {
                role: SingleflightRole::Leader,
                forked_lane: false,
            }
        );
        assert_eq!(result.attempts, 2);
        assert_eq!(executor.computes, 2);
        assert_eq!(executor.published, vec![12]);
        assert_eq!(
            executor
                .cache
                .get_if_valid(&"node".to_string(), &executor.view())
                .map(|cached| *cached),
            Some(12)
        );
    }

    #[test]
    fn stable_request_uses_fallback_after_retries_exhausted() {
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();
        let mut executor = TestRequestExecutor::new(
            "node",
            StoreViewCompatToken {
                epoch: 5,
                session: None,
                validity_fingerprint: 0,
            },
            2,
        );
        executor.compute_values.extend([1, 2, 3]);
        executor.stability.extend([false, false, false]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert_eq!(result.value, 3);
        assert_eq!(result.source, RequestSource::Fallback);
        assert_eq!(result.attempts, 3);
        assert_eq!(executor.computes, 3);
        assert!(executor.published.is_empty());
    }

    /// A non-current snapshot must NEVER receive a retained flight's
    /// result as a follower. The lane key folds `compat_token`, which
    /// excludes the additive generations — so a snapshot the manager could
    /// not prove current (a `ReturnOnly` seed: the FULL validation token
    /// drifted on an additive dimension) can still carry a `compat_token`
    /// equal to a lane an earlier CURRENT request retained a stable result
    /// on. If the non-current request joined that retained lane it would
    /// follower-return the pre-mutation value WITHOUT running `compute` or
    /// the `is_stable` promotion fence.
    ///
    /// DISCRIMINATES: pre-fix, `run_stable_request` pinned and
    /// `run_retaining`-joined the lane regardless of currentness, so the
    /// non-current request returned the retained value (`computes == 0`,
    /// `value == 7`). Post-fix, a non-current snapshot bypasses the shared
    /// lane entirely and runs its OWN cold compute (`computes == 1`,
    /// `value == 99`), and because the snapshot is non-current its
    /// `is_stable` fence is false so nothing is promoted.
    #[test]
    fn non_current_snapshot_does_not_join_retained_flight() {
        let token = StoreViewCompatToken {
            epoch: 7,
            session: None,
            validity_fingerprint: 0,
        };
        let singleflight =
            SingleflightGroup::<String, StableExecutionValue<usize>, &'static str>::default();

        // Plant a RETAINED stable `Done` for ("node", token) carrying the
        // stale value 7, kept joinable by an explicit participation pin
        // (as a concurrent burst member would hold). This is exactly the
        // lane an earlier current request would have retained.
        let _pin = singleflight.participate("node".to_string(), token);
        let leader = singleflight
            .run_retaining(
                "node".to_string(),
                token,
                || {
                    Ok(StableExecutionValue {
                        value: 7usize,
                        stable: true,
                        computed: true,
                    })
                },
                |sev| sev.stable,
            )
            .unwrap();
        assert_eq!(leader.role, SingleflightRole::Leader);
        assert_eq!(leader.value.value, 7);
        assert!(
            leader.value.stable,
            "the planted flight must be retained as stable"
        );

        // A NON-CURRENT request for the SAME key whose view carries the
        // SAME compat_token. Its snapshot is non-current, modelling the
        // production state where the manager could not prove the view
        // coherent — so `is_stable` is false (it must not promote) and the
        // bounded loop falls through to the post-loop fallback. The cache
        // starts empty, so the ONLY way it could ever surface the retained
        // flight's value `7` is by joining that lane — which it must not.
        // Each off-lane attempt computes its own value; `max_attempts = 2`
        // ⇒ 2 unstable in-loop attempts (99, 100) then the post-loop
        // fallback (101).
        let mut executor = TestRequestExecutor::new("node", token, 2).non_current();
        executor.compute_values.extend([99, 100, 101]);
        executor.stability.extend([false, false, false]);

        let result = run_stable_request(&singleflight, &mut executor).unwrap();

        assert!(
            executor.computes >= 1,
            "a non-current request MUST run its OWN cold compute, not join the retained flight \
             (computes = {})",
            executor.computes,
        );
        assert_ne!(
            result.value, 7,
            "JOIN-LAYER LEAK: a post-mutation (non-current) request received the retained \
             flight's pre-mutation result (7) — it must compute its own value off the lane",
        );
        assert!(
            [99, 100, 101].contains(&result.value),
            "a non-current request MUST return one of its OWN computed values, got {}",
            result.value,
        );
        assert_eq!(
            result.source,
            RequestSource::Fallback,
            "a non-current request runs off the shared lane (Fallback), it is not a Flight join",
        );
        assert!(
            executor.published.is_empty(),
            "a non-current (is_stable=false) snapshot's result MUST NOT be promoted",
        );

        // The retained flight is still intact for a genuinely-current
        // joiner — the non-current request neither consumed nor mutated it.
        let mut current_joiner = TestRequestExecutor::new("node", token, 3);
        current_joiner.compute_values.push_back(123);
        current_joiner.stability.push_back(true);
        let joined = run_stable_request(&singleflight, &mut current_joiner).unwrap();
        assert_eq!(
            current_joiner.computes, 0,
            "a current joiner reuses the retained flight"
        );
        assert_eq!(
            joined.value, 7,
            "a current joiner receives the retained value"
        );
    }

    #[test]
    fn singleflight_coalesces_same_key_and_token() {
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let group = Arc::clone(&group);
                let start = Arc::clone(&start);
                let computes = Arc::clone(&computes);
                std::thread::spawn(move || {
                    start.wait();
                    group
                        .run(
                            "node".to_string(),
                            StoreViewCompatToken {
                                epoch: 7,
                                session: None,
                                validity_fingerprint: 0,
                            },
                            || {
                                computes.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(50));
                                Ok(42)
                            },
                        )
                        .unwrap()
                })
            })
            .collect();

        start.wait();
        let mut handles = handles.into_iter();
        let first = handles.next().unwrap().join().unwrap();
        let second = handles.next().unwrap().join().unwrap();

        assert_eq!(computes.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first.value, &second.value));
        assert_ne!(first.role, second.role);
        assert_eq!(
            [first.role, second.role]
                .into_iter()
                .filter(|role| *role == SingleflightRole::Leader)
                .count(),
            1
        );
    }

    /// A caller that arrives AFTER the leader has fully completed —
    /// strictly outside the leader's compute window — still joins the
    /// leader's published result as a `Follower` (NOT a second cold
    /// `Leader`) as long as a participation pin keeps the lane alive.
    ///
    /// This is the deterministic, load-independent characterization of
    /// the cold-concurrent singleflight race that
    /// `cache_layer_cold_concurrent_attribution::sixteen_cold_concurrent_…`
    /// hits non-deterministically under CPU contention. The earlier
    /// `singleflight_coalesces_same_key_and_token` test only proves
    /// coalescing for callers that OVERLAP the leader's compute window.
    /// It does NOT exercise the post-compute gap: a caller whose claim
    /// lands after the leader returned. `run_stable_request` holds a
    /// [`SingleflightGroup::participate`] pin across that whole window
    /// (every concurrent request pins before it peeks), so the lane the
    /// leader published into stays joinable; this test models that pin
    /// explicitly.
    ///
    /// DISCRIMINATES: pre-fix, the leader removed its flight slot the
    /// instant `compute()` returned, so a caller arriving in the
    /// post-compute gap found an empty lane and became a second `Leader`
    /// (`computes == 2`) — even though a sibling request was still in
    /// flight. Post-fix, the participation pin keeps the leader's `Done`
    /// rendezvous alive, so the late caller joins as `Follower`
    /// (`computes == 1`).
    #[test]
    fn singleflight_late_caller_joins_pinned_completed_leader_as_follower() {
        let group = SingleflightGroup::<String, usize, &'static str>::default();
        let token = StoreViewCompatToken {
            epoch: 9,
            session: None,
            validity_fingerprint: 0,
        };
        let computes = AtomicUsize::new(0);

        // A sibling request pins the lane (as `run_stable_request` does
        // before its pre-flight peek) and holds the pin across the whole
        // window below — modelling a concurrent burst member that has not
        // yet reached its own `run` claim.
        let _pin = group.participate("node".to_string(), token);

        // Leader runs to completion synchronously and returns BEFORE the
        // late call is issued — the late call lands strictly in the
        // post-compute gap.
        let leader = group
            .run("node".to_string(), token, || {
                computes.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            })
            .unwrap();
        assert_eq!(leader.role, SingleflightRole::Leader);
        assert_eq!(computes.load(Ordering::SeqCst), 1);

        // Late caller, same key + token, issued only now. Because the
        // sibling pin kept the lane alive, it observes the leader's
        // retained `Done` result and joins as a Follower WITHOUT
        // recomputing.
        let late = group
            .run("node".to_string(), token, || {
                computes.fetch_add(1, Ordering::SeqCst);
                Ok(999)
            })
            .unwrap();

        assert_eq!(
            computes.load(Ordering::SeqCst),
            1,
            "late caller must NOT recompute — it joins the pinned leader's published result",
        );
        assert_eq!(
            late.role,
            SingleflightRole::Follower,
            "a caller arriving after the leader finished, while the lane is pinned, must be a Follower",
        );
        assert_eq!(*late.value, 7, "late caller receives the leader's value");
    }

    /// The retained `Done` rendezvous is a per-burst dedup primitive,
    /// NOT a result cache: once the last pin on the lane is released the
    /// slot is reaped, so a SUBSEQUENT independent request re-enters the
    /// cold path as a fresh `Leader` instead of forever returning the
    /// stale retained result. This preserves the "non-cacheable /
    /// empty-fact results are not persisted" contract the validated
    /// caches own.
    ///
    /// DISCRIMINATES a naive "retain Done forever" fix: that fix would
    /// keep `computes == 1` and return the stale value on the post-drain
    /// call. The correct reap drops the slot once the last pin leaves, so
    /// the later call cold-computes again (`computes == 2`).
    #[test]
    fn singleflight_done_rendezvous_is_reaped_after_last_pin() {
        let group = SingleflightGroup::<String, usize, &'static str>::default();
        let token = StoreViewCompatToken {
            epoch: 3,
            session: None,
            validity_fingerprint: 0,
        };
        let computes = AtomicUsize::new(0);

        // A leader completes while a sibling pin is held, and a late
        // caller joins the retained rendezvous.
        {
            let _pin = group.participate("node".to_string(), token);
            let _leader = group
                .run("node".to_string(), token, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(1)
                })
                .unwrap();
            let joiner = group
                .run("node".to_string(), token, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(2)
                })
                .unwrap();
            assert_eq!(computes.load(Ordering::SeqCst), 1, "joiner joined leader");
            assert_eq!(joiner.role, SingleflightRole::Follower);
            assert_eq!(*joiner.value, 1);
            // `_pin` drops here, releasing the last pin and reaping the
            // lane.
        }

        // A fresh request after the lane drained re-enters the cold path.
        let fresh = group
            .run("node".to_string(), token, || {
                computes.fetch_add(1, Ordering::SeqCst);
                Ok(3)
            })
            .unwrap();
        assert_eq!(
            computes.load(Ordering::SeqCst),
            2,
            "after the rendezvous is reaped, a new request must cold-compute again",
        );
        assert_eq!(fresh.role, SingleflightRole::Leader);
        assert_eq!(*fresh.value, 3);
    }

    /// Shared backing state for a session-scoped component-meta request
    /// modelled through the real [`run_stable_request`] entry point. Every
    /// caller snapshots a view whose `compat_token` carries
    /// `session: Some(id)` — exactly what
    /// `SessionRequestHost::snapshot_store_view` ->
    /// `HostStoreView::from_session_id` produces. The cache + cold-compute
    /// counter are shared across the request so a correct dedup collapses
    /// to ONE cold compute.
    struct SessionBurstState {
        cache: ValidatedFactCache<String, usize>,
        valid_fact: FactVersionRef,
        computes: AtomicUsize,
        /// Released by the test driver to let a leader's `compute` return.
        /// Models a leader that is still mid-flight (holding its run-lane
        /// self-pin) until the test has lined up the post-compute gap.
        leader_gate: Mutex<bool>,
        leader_gate_cv: Condvar,
        /// Signalled by the leader once it has entered `compute` (so the
        /// driver knows a leader exists and which thread it is).
        leader_entered: Mutex<bool>,
        leader_entered_cv: Condvar,
    }

    /// Per-thread executor reading the shared [`SessionBurstState`] through
    /// [`run_stable_request`]. The `session: Some(id)` token is the whole
    /// point: pre-fix, `run_stable_request` pinned the lane from a separate
    /// `lane_token` that hardcoded `session: None`, so the pin lane drifted
    /// off the `session: Some(id)` run lane and the post-compute-gap race
    /// was NOT closed for session callers.
    struct SessionBurstExecutor<'a> {
        shared: &'a SessionBurstState,
        session_id: u64,
        /// When `true`, this caller's `compute` blocks on
        /// `shared.leader_gate` (used to PIN the leader mid-flight while
        /// the driver lines up a straggler). When `false`, `compute`
        /// returns immediately.
        gated_leader: bool,
    }

    impl<'a> StableRequestExecutor<String, usize> for SessionBurstExecutor<'a> {
        type View = TestView;
        type Error = &'static str;

        fn cache_key(&self) -> String {
            "node".to_string()
        }

        fn snapshot_view(&mut self) -> Self::View {
            TestView {
                token: StoreViewCompatToken {
                    epoch: 1,
                    session: Some(self.session_id),
                    validity_fingerprint: 0,
                },
                valid_facts: [self.shared.valid_fact.clone()].into_iter().collect(),
            }
        }

        // No churn-prone manager: every snapshot is current by construction.
        fn snapshot_view_is_current(&self) -> bool {
            true
        }

        fn try_get_cached(&mut self, view: &Self::View) -> Option<usize> {
            self.shared
                .cache
                .get_if_valid(&"node".to_string(), view)
                .map(|cached| *cached)
        }

        fn compute(&mut self, _view: &Self::View) -> Result<usize, Self::Error> {
            self.shared.computes.fetch_add(1, Ordering::SeqCst);
            if self.gated_leader {
                // Announce that a leader has entered compute …
                {
                    let mut entered = self.shared.leader_entered.lock();
                    *entered = true;
                    self.shared.leader_entered_cv.notify_all();
                }
                // … and stay mid-flight (holding the run-lane self-pin)
                // until the driver releases the gate.
                let mut open = self.shared.leader_gate.lock();
                while !*open {
                    self.shared.leader_gate_cv.wait(&mut open);
                }
            }
            Ok(42)
        }

        fn is_stable(&mut self, _view: &Self::View) -> bool {
            true
        }

        fn store_stable(&mut self, value: &usize) {
            self.shared.cache.insert(
                "node".to_string(),
                *value,
                vec![self.shared.valid_fact.clone()],
            );
        }

        fn max_attempts(&self) -> usize {
            3
        }
    }

    /// SESSION-lane cold-concurrent dedup contract, deterministic
    /// post-compute-gap form (P1a).
    ///
    /// Models two concurrent session-scoped component-meta requests on the
    /// SAME `session: Some(id)` lane, BOTH flowing through the real
    /// [`run_stable_request`] entry point:
    ///
    /// 1. A **sibling** request enters `compute` as the leader and parks
    ///    there (holding its run-lane self-pin AND its participation pin).
    /// 2. A **straggler** request is then released. It snapshots its view,
    ///    pins, peeks (miss), and reaches `run_retaining`. Because the
    ///    sibling is still mid-flight, the straggler joins the in-flight
    ///    leader as a Follower rather than spawning a second cold leader.
    ///
    /// The participation pin the straggler itself takes is on the lane its
    /// `run_retaining` claims — and that is the whole fix.
    ///
    /// DISCRIMINATES P1a: pre-fix, `run_stable_request` pinned the lane via
    /// a `lane_token()` override hardcoding `session: None`, while the
    /// inner `run_retaining` claimed `store_view.compat_token()` =
    /// `session: Some(id)`. Both requests therefore pinned the WRONG
    /// (`None`) lane; the `Some(id)` run lane carried only the leader's
    /// transient self-pin, so a straggler arriving in the leader's
    /// post-compute gap found a vacant `Some(id)` lane and spawned a SECOND
    /// cold leader (`computes == 2`). Post-fix, the pin is taken from the
    /// actual `store_view.compat_token()`, so pin lane == run lane ==
    /// `Some(id)`, the straggler joins as a Follower, and the burst
    /// collapses to exactly one cold compute (`computes == 1`).
    #[test]
    fn session_cold_concurrent_requests_collapse_to_single_leader() {
        const SESSION_ID: u64 = 7;

        let singleflight = Arc::new(SingleflightGroup::<
            String,
            StableExecutionValue<usize>,
            &'static str,
        >::default());
        let shared = Arc::new(SessionBurstState {
            cache: ValidatedFactCache::default(),
            valid_fact: FactVersionRef::FileWholeHash {
                canonical_id: "/src/App.vue".to_string(),
                hash: [7; 16],
            },
            computes: AtomicUsize::new(0),
            leader_gate: Mutex::new(false),
            leader_gate_cv: Condvar::new(),
            leader_entered: Mutex::new(false),
            leader_entered_cv: Condvar::new(),
        });

        // Sibling/leader: enters `compute`, parks mid-flight holding its
        // run-lane pin.
        let sibling = {
            let singleflight = Arc::clone(&singleflight);
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let mut executor = SessionBurstExecutor {
                    shared: &shared,
                    session_id: SESSION_ID,
                    gated_leader: true,
                };
                run_stable_request(&singleflight, &mut executor).unwrap()
            })
        };

        // Wait until the leader is provably inside `compute` (so the lane
        // is `Running` and the straggler will not itself become leader by
        // racing the claim).
        {
            let mut entered = shared.leader_entered.lock();
            while !*entered {
                shared.leader_entered_cv.wait(&mut entered);
            }
        }

        // Measure the parked-leader strong-count baseline on the
        // `session: Some(id)` run lane BEFORE the straggler exists, so the
        // straggler-progress gate below is derived from the real baseline
        // rather than a hardcoded literal that can silently drift if the
        // leader's ref bookkeeping changes.
        //
        // While the leader is parked mid-`compute` the lane's `FlightState`
        // `Arc` has exactly THREE strong refs:
        //   1 the leader's `run_retaining` local `state` binding
        // + 1 the `flights` map entry
        // + 1 the leader's own `participate` guard clone (every caller pins
        //     via `participate` at the top of `run_stable_request` BEFORE its
        //     pre-flight peek)
        // = 3 (asserted as the baseline invariant).
        let run_lane = StoreViewCompatToken {
            epoch: 1,
            session: Some(SESSION_ID),
            validity_fingerprint: 0,
        };
        let leader_baseline = singleflight.test_flight_strong_count(&"node".to_string(), run_lane);
        assert_eq!(
            leader_baseline, 3,
            "parked-leader strong-count baseline on the `session: Some(id)` lane must be 3 \
             (leader `run_retaining` local + `flights` map entry + leader `participate` guard); \
             a different value means the leader ref bookkeeping changed and the straggler gate \
             below must be re-derived",
        );

        // Straggler: now reaches `run_stable_request` while the leader is
        // still mid-flight. It must Follower-join the leader's flight.
        let straggler = {
            let singleflight = Arc::clone(&singleflight);
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let mut executor = SessionBurstExecutor {
                    shared: &shared,
                    session_id: SESSION_ID,
                    gated_leader: false,
                };
                run_stable_request(&singleflight, &mut executor).unwrap()
            })
        };

        // Wait until the straggler has provably committed INTO
        // `run_retaining` on the leader's lane — past its pre-flight
        // `try_get_cached` peek — so it joins the in-flight leader as a
        // Follower rather than reading a warm cache hit the leader is about
        // to publish. Poll the run-lane strong count deterministically
        // instead of sleeping a fixed wall-clock duration.
        //
        // The straggler adds its refs in TWO distinct steps, and only the
        // SECOND proves it is past its cache peek:
        //   +1 its `participate` guard clone — taken at the top of
        //      `run_stable_request`, BEFORE the line-619 `try_get_cached`
        //      peek (`leader_baseline + 1`). This alone does NOT prove the
        //      straggler has missed the cache.
        //   +1 its `run_retaining` clone — taken AFTER the peek, as it joins
        //      the Running lane and parks on the condvar as a
        //      Follower-in-waiting (`leader_baseline + 2`).
        // Gating on `leader_baseline + 2` is therefore the off-by-one-free
        // condition: it fires ONLY once the straggler holds BOTH refs, i.e.
        // it has provably passed its peek and committed as a Follower, so the
        // leader's `store_stable` publish below cannot turn the straggler
        // into a pre-flight `Cache` hit. (The previous gate fired at the
        // straggler's `participate` step — `leader_baseline + 1` == 4 — which
        // let the leader publish while the straggler was still BEFORE its
        // peek, producing the intermittent `Cache`-instead-of-`Follower`
        // flake under CPU contention.)
        let mut spins = 0;
        loop {
            let count = singleflight.test_flight_strong_count(&"node".to_string(), run_lane);
            if count >= leader_baseline + 2 {
                break;
            }
            spins += 1;
            assert!(
                spins < 10_000_000,
                "straggler never committed into `run_retaining` on the `session: Some(id)` run \
                 lane (count stuck below leader_baseline + 2). This means the straggler \
                 pinned/joined a DIFFERENT lane than the leader's run lane (P1a: pin lane \
                 drifted off the run lane).",
            );
            std::thread::yield_now();
        }

        // Release the leader; both requests complete.
        {
            let mut open = shared.leader_gate.lock();
            *open = true;
            shared.leader_gate_cv.notify_all();
        }

        let sibling_result = sibling.join().unwrap();
        let straggler_result = straggler.join().unwrap();

        // The decisive assertion: exactly ONE cold compute across both
        // session requests.
        let computes = shared.computes.load(Ordering::SeqCst);
        assert_eq!(
            computes, 1,
            "two cold-concurrent session requests on the same `session: Some(id)` lane must \
             collapse to exactly ONE cold compute (got {computes}). A count of 2 means the \
             straggler spawned a second cold leader because the participate-pin lane drifted off \
             the `session: Some(id)` run lane (P1a regression).",
        );

        // Role attribution: one Leader winner, the straggler a Follower
        // joiner.
        assert_eq!(
            sibling_result.source,
            RequestSource::Flight {
                role: SingleflightRole::Leader,
                forked_lane: false,
            },
            "the sibling that ran `compute` is the cold Leader winner",
        );
        assert_eq!(
            straggler_result.source,
            RequestSource::Flight {
                role: SingleflightRole::Follower,
                forked_lane: false,
            },
            "the straggler must Follower-join the in-flight leader, not lead a second flight",
        );
        assert_eq!(sibling_result.value, 42);
        assert_eq!(straggler_result.value, 42);

        // Non-cache contract: the lane fully drains after both pins
        // release.
        assert_eq!(
            singleflight.test_flight_strong_count(&"node".to_string(), run_lane),
            0,
            "the `session: Some(id)` lane must be reaped after the last pin releases \
             (per-burst rendezvous, not a cache)",
        );
    }

    /// Post-compute-gap dedup for a SESSION lane requires the participation
    /// pin and the `run` claim to share the SAME `session: Some(id)` lane
    /// (P1a, primitive form).
    ///
    /// This is the session-token analogue of
    /// [`singleflight_late_caller_joins_pinned_completed_leader_as_follower`],
    /// and it reproduces the exact composition `run_stable_request`
    /// performs — `participate(view_token)` held across a
    /// `run_retaining(view_token)` claim — for the post-compute gap a
    /// burst opens when a straggler's claim lands AFTER the leader fully
    /// returned.
    ///
    /// DISCRIMINATES P1a directly on the `participate`/`run` lane pairing:
    /// when the pin lane MATCHES the run lane (`session: Some(id)`, the
    /// fix), the leader's published `Done` stays joinable across the gap
    /// and the late session caller is a Follower (`computes == 1`). When
    /// the pin lane is the pre-fix `session: None` while the run lane is
    /// `session: Some(id)`, the run lane carries no surviving pin once the
    /// leader returns, its `Done` is reaped, and the late session caller
    /// finds a vacant lane and cold-computes a SECOND time
    /// (`computes == 2`). The test asserts the matched-lane dedup and, as a
    /// guard, that the mismatched-lane pairing does NOT dedup — pinning the
    /// invariant from both sides.
    #[test]
    fn session_post_compute_gap_dedup_requires_pin_lane_equals_run_lane() {
        const SESSION_ID: u64 = 11;
        let run_lane = StoreViewCompatToken {
            epoch: 4,
            session: Some(SESSION_ID),
            validity_fingerprint: 0,
        };

        // --- Matched lane (the fix): pin lane == run lane == Some(id). ---
        {
            let group = SingleflightGroup::<String, usize, &'static str>::default();
            let computes = AtomicUsize::new(0);

            // A concurrent sibling session request pins the run lane — this
            // is exactly the pin `run_stable_request` holds, on the actual
            // view token, post-fix.
            let _sibling_pin = group.participate("node".to_string(), run_lane);

            // Leader session request runs to completion and returns BEFORE
            // the straggler claims — opening the post-compute gap.
            let leader = group
                .run("node".to_string(), run_lane, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(7)
                })
                .unwrap();
            assert_eq!(leader.role, SingleflightRole::Leader);
            assert_eq!(computes.load(Ordering::SeqCst), 1);

            // Straggler session request claims only now. The sibling pin on
            // the SAME lane kept the leader's `Done` joinable.
            let straggler = group
                .run("node".to_string(), run_lane, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(999)
                })
                .unwrap();
            assert_eq!(
                computes.load(Ordering::SeqCst),
                1,
                "matched-lane session straggler must Follower-join the retained `Done`, not recompute",
            );
            assert_eq!(straggler.role, SingleflightRole::Follower);
            assert_eq!(*straggler.value, 7);
        }

        // --- Mismatched lane (the pre-fix bug): pin on `session: None`,
        // run on `session: Some(id)`. The run lane loses its rendezvous in
        // the gap, so the straggler cold-computes again. This guards the
        // invariant from the other side — proving the matched-lane dedup
        // above genuinely depends on the pin landing on the run lane. ---
        {
            let none_lane = StoreViewCompatToken {
                epoch: 4,
                session: None,
                validity_fingerprint: 0,
            };
            let group = SingleflightGroup::<String, usize, &'static str>::default();
            let computes = AtomicUsize::new(0);

            // Pre-fix pin: on the WRONG (`None`) lane.
            let _sibling_pin = group.participate("node".to_string(), none_lane);

            let leader = group
                .run("node".to_string(), run_lane, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(7)
                })
                .unwrap();
            assert_eq!(leader.role, SingleflightRole::Leader);
            assert_eq!(computes.load(Ordering::SeqCst), 1);

            // The run lane (`Some(id)`) had only the leader's transient
            // self-pin; it was reaped on the leader's `keep` unpin. The
            // straggler finds a vacant run lane and leads a SECOND flight.
            let straggler = group
                .run("node".to_string(), run_lane, || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(999)
                })
                .unwrap();
            assert_eq!(
                computes.load(Ordering::SeqCst),
                2,
                "mismatched-lane session straggler must recompute (the WRONG-lane pin did not keep \
                 the `Some(id)` run-lane rendezvous alive) — this is precisely the P1a harm the \
                 matched-lane case above avoids",
            );
            assert_eq!(straggler.role, SingleflightRole::Leader);
            assert_eq!(*straggler.value, 999);
        }
    }

    #[test]
    fn singleflight_forks_incompatible_tokens() {
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = [
            StoreViewCompatToken {
                epoch: 1,
                session: None,
                validity_fingerprint: 0,
            },
            StoreViewCompatToken {
                epoch: 2,
                session: None,
                validity_fingerprint: 0,
            },
        ]
        .into_iter()
        .map(|token| {
            let group = Arc::clone(&group);
            let start = Arc::clone(&start);
            let computes = Arc::clone(&computes);
            std::thread::spawn(move || {
                start.wait();
                group
                    .run("node".to_string(), token, || {
                        computes.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(token.epoch as usize)
                    })
                    .unwrap()
            })
        })
        .collect();

        start.wait();
        let mut handles = handles.into_iter();
        let first = handles.next().unwrap().join().unwrap();
        let second = handles.next().unwrap().join().unwrap();

        assert_eq!(computes.load(Ordering::SeqCst), 2);
        assert_eq!(first.role, SingleflightRole::Leader);
        assert_eq!(second.role, SingleflightRole::Leader);
        assert!(first.forked_lane || second.forked_lane);
        assert!(!Arc::ptr_eq(&first.value, &second.value));
    }

    /// A NON-RETAINED leader result (`retain` returns false) must not leave
    /// a joinable lane behind: once `run_retaining` returns, the lane is
    /// fully reaped and a subsequent claim cold-recomputes as a fresh
    /// Leader (P1b contract, post-return form).
    ///
    /// This pins the non-cache half of the contract deterministically: a
    /// non-retained `Done` is never a persistent rendezvous, and the
    /// self-pin `fetch_sub` (now under the `flights` lock) leaves the pin
    /// count consistent so the reap fires.
    #[test]
    fn singleflight_non_retained_result_leaves_no_joinable_lane() {
        let group = SingleflightGroup::<String, usize, &'static str>::default();
        let token = StoreViewCompatToken {
            epoch: 2,
            session: None,
            validity_fingerprint: 0,
        };
        let computes = AtomicUsize::new(0);

        // Leader produces a value the caller declines to retain.
        let leader = group
            .run_retaining(
                "node".to_string(),
                token,
                || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(1)
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(leader.role, SingleflightRole::Leader);
        assert_eq!(computes.load(Ordering::SeqCst), 1);

        // The non-retained lane must be gone — no lingering `Done`.
        assert_eq!(
            group.test_flight_strong_count(&"node".to_string(), token),
            0,
            "a non-retained result must not leave a joinable lane behind",
        );

        // A subsequent claim re-enters the cold path as a fresh Leader.
        let next = group
            .run_retaining(
                "node".to_string(),
                token,
                || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(2)
                },
                |_| true,
            )
            .unwrap();
        assert_eq!(
            computes.load(Ordering::SeqCst),
            2,
            "after a non-retained result, the next claim must cold-recompute",
        );
        assert_eq!(next.role, SingleflightRole::Leader);
        assert_eq!(*next.value, 2);
    }

    /// A new claimant must NEVER FIRST-OBSERVE a NON-RETAINED `Done`; it
    /// must re-elect a fresh leader against a clean lane (P1b — the
    /// torn/non-retained contract, deterministic form via a sibling pin).
    ///
    /// Reproduces the post-terminal window deterministically — the
    /// keep==false analogue of
    /// [`singleflight_late_caller_joins_pinned_completed_leader_as_follower`].
    /// A sibling holds a `participate` pin on the lane (modelling a
    /// concurrent burst member), so the leader's `FlightState` `Arc` is
    /// kept alive even after the leader's terminal. A leader then runs
    /// `run_retaining(retain = false)` to completion and returns; a LATE
    /// claimant issues its `run_retaining` only afterwards — strictly in
    /// the post-terminal window.
    ///
    /// The decisive property: the late claimant must re-elect (it is a
    /// fresh `Leader` that RECOMPUTES), NEVER a `Follower` joining the
    /// non-retained `Done`. Because the non-retained terminal removes the
    /// lane under the `flights` lock atomically with the publish, the late
    /// claimant finds NO lane (despite the sibling pin keeping the orphaned
    /// `state` alive) and starts a fresh flight. This pins:
    ///   * the non-retained result is not joinable by a new claimant;
    ///   * the immediate removal happens even while another pin is held
    ///     (non-retained results are dropped now, not at last-pin);
    ///   * the self-pin `fetch_sub` (moved under the `flights` lock) keeps
    ///     the count consistent, so the surviving sibling pin's later
    ///     `unpin` neither underflows nor evicts the fresh lane.
    #[test]
    fn non_retained_result_forces_late_claimant_to_reelect_not_join() {
        let group = SingleflightGroup::<String, usize, &'static str>::default();
        let token = StoreViewCompatToken {
            epoch: 6,
            session: None,
            validity_fingerprint: 0,
        };
        let computes = AtomicUsize::new(0);

        // Sibling participation pin (as a concurrent burst member holds),
        // kept alive across the whole window so the leader's `state` is not
        // dropped merely because its own self-pin went away.
        let sibling_pin = group.participate("node".to_string(), token);

        // Leader produces a NON-RETAINED result and returns.
        let leader = group
            .run_retaining(
                "node".to_string(),
                token,
                || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(1)
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(leader.role, SingleflightRole::Leader);
        assert_eq!(computes.load(Ordering::SeqCst), 1);

        // Even though the sibling pin is still held, the non-retained lane
        // was removed immediately under the `flights` lock — it must NOT be
        // joinable.
        let late = group
            .run_retaining(
                "node".to_string(),
                token,
                || {
                    computes.fetch_add(1, Ordering::SeqCst);
                    Ok(2)
                },
                |_| true,
            )
            .unwrap();
        assert_eq!(
            late.role,
            SingleflightRole::Leader,
            "a late claimant must re-elect a fresh leader, never join a non-retained `Done`",
        );
        assert_eq!(
            computes.load(Ordering::SeqCst),
            2,
            "the late claimant must cold-recompute (the non-retained result is not joinable)",
        );
        assert_eq!(*late.value, 2);

        // Dropping the orphaned sibling pin must be a clean no-op (its
        // `unpin` `fetch_sub` does not underflow, and its `ptr_eq` guard
        // does not evict the fresh `Leader` lane the late claimant retained).
        drop(sibling_pin);
        // The fresh stable lane retained by the late `Leader` survives the
        // orphaned sibling-pin drop and is reaped only by its own pins.
        assert_eq!(
            group.test_flight_strong_count(&"node".to_string(), token),
            0,
            "after the orphaned sibling pin drops and the late leader's own pin released, \
             the lane is fully reaped",
        );
    }

    /// The non-retained terminal publishes the `Done` AND removes the lane
    /// inside ONE continuously-held `flights` critical section (P1b — the
    /// atomic publish+remove window). This is the discriminating proof that
    /// no concurrent claimant can EVER first-observe a non-retained `Done`,
    /// because every claimant's only lane observation is gated behind that
    /// same `flights` lock (the claim block in `run_retaining` and
    /// `participate` both take `self.flights.lock()` before reading any lane
    /// state — there is NO lock-free observation path).
    ///
    /// Why this characterises the fix by construction rather than chasing a
    /// torn join: a NEW claimant cannot witness the intermediate `Done`
    /// without acquiring `flights`. Post-fix the leader holds `flights` from
    /// before the publish through past the removal, so the entire window is
    /// invisible by mutual exclusion. A test that instead tried to drive a
    /// real claimant into the window would have to hold the leader at the
    /// seam until the claimant observed — but the claimant's observation
    /// needs the very lock the leader holds, so it would DEADLOCK post-fix
    /// (and a non-blocking signal-and-proceed variant would make the pre-fix
    /// split a lock-race, i.e. flaky, not reliably-failing). The lock-held
    /// invariant is therefore the only deterministic, both-directions
    /// discriminator.
    ///
    /// Mechanism: a `#[cfg(test)]` seam hook fires on the LEADER thread at
    /// the exact point between the non-retained publish and the lane
    /// removal. The hook hands off to a watcher thread that probes the map
    /// lock with a NON-BLOCKING `try_lock` (so it can never deadlock against
    /// the leader) and records whether the lock was free:
    ///   * post-fix the lock is HELD at the seam → `try_lock` returns `None`
    ///     → `lock_was_free == false` (asserted);
    ///   * pre-fix (publish, drop lock, then remove) the seam sits in the
    ///     lock GAP → `try_lock` returns `Some` → `lock_was_free == true`,
    ///     and the assertion FAILS.
    ///
    /// The hook is reached exactly once (a single non-retained terminal), so
    /// the handshake barriers below are a deterministic rendezvous, not a
    /// timing race. A watchdog bounds the watcher so any regression that
    /// prevented the hook from firing fails loudly instead of hanging.
    ///
    /// FAIL-PRE / PASS-POST was proven by temporarily splitting the critical
    /// section (publish under the lock, drop it, then re-acquire to remove):
    /// the watcher observed a free lock at the seam and this test FAILED;
    /// restoring the single held lock made it PASS.
    #[test]
    fn non_retained_publish_and_remove_share_one_held_flights_lock() {
        use std::sync::mpsc;
        use std::time::Instant;

        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let token = StoreViewCompatToken {
            epoch: 14,
            session: None,
            validity_fingerprint: 0,
        };

        // Rendezvous: the leader's seam hook signals `at_seam`, then blocks on
        // `probe_done` until the watcher has finished its NON-BLOCKING lock
        // probe. Because the probe is `try_lock` (never blocks on the lock the
        // leader holds), the leader's wait on `probe_done` always completes —
        // no deadlock in either arrangement.
        let at_seam = Arc::new(Barrier::new(2));
        let probe_done = Arc::new(Barrier::new(2));
        // `true` iff the watcher could acquire `flights` AT THE SEAM (i.e. the
        // publish and the removal were NOT in one held lock — the pre-fix
        // bug). Post-fix this stays `false`.
        let lock_was_free_at_seam = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Set by the watcher once it has actually run, so the watchdog can
        // tell "hook never fired" from "hook fired, lock was held".
        let watcher_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel::<()>();

        // Watcher: waits for the leader to reach the seam, probes the map lock
        // without blocking, records the result, then releases the leader.
        let watcher = {
            let group = Arc::clone(&group);
            let at_seam = Arc::clone(&at_seam);
            let probe_done = Arc::clone(&probe_done);
            let lock_was_free_at_seam = Arc::clone(&lock_was_free_at_seam);
            let watcher_ran = Arc::clone(&watcher_ran);
            std::thread::spawn(move || {
                // Block until the leader is at the publish/remove seam.
                at_seam.wait();
                // NON-BLOCKING probe: held (None) post-fix, free (Some) in the
                // pre-fix lock gap. `try_lock` never parks, so this cannot
                // deadlock against the leader holding `flights`.
                let probe = group.flights.try_lock();
                lock_was_free_at_seam.store(probe.is_some(), Ordering::SeqCst);
                drop(probe);
                watcher_ran.store(true, Ordering::SeqCst);
                let _ = done_tx.send(());
                // Release the leader to finish its critical section (remove the
                // lane, drop the lock) and return.
                probe_done.wait();
            })
        };

        // Install the seam hook for exactly the one non-retained terminal this
        // leader produces. It runs ON the leader thread WHILE (post-fix) the
        // `flights` lock is held, strictly between the publish and the removal.
        {
            let at_seam = Arc::clone(&at_seam);
            let probe_done = Arc::clone(&probe_done);
            group.set_non_retained_seam_hook(Box::new(move || {
                at_seam.wait();
                probe_done.wait();
            }));
        }

        // Leader produces a NON-RETAINED result (`retain = false`), driving the
        // exact `keep == false` critical section the seam hook is wired into.
        let leader_group = Arc::clone(&group);
        let leader = std::thread::spawn(move || {
            leader_group
                .run_retaining(
                    "node".to_string(),
                    token,
                    || -> Result<usize, &'static str> { Ok(1) },
                    |_| false,
                )
                .unwrap()
        });

        // WATCHDOG: the watcher must report (hook fired) within the timeout. A
        // regression that never reaches the seam fails here instead of hanging
        // the suite.
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| {
                let _ = Instant::now();
                panic!(
                "seam watchdog: the non-retained seam hook never fired — the leader did not reach \
                 the publish/remove window (P1b regression)",
            )
            });

        let leader_result = leader.join().expect("leader thread must not panic");
        watcher.join().expect("watcher thread must not panic");

        // The leader genuinely ran the non-retained terminal as a fresh Leader.
        assert_eq!(leader_result.role, SingleflightRole::Leader);
        assert_eq!(*leader_result.value, 1);

        // The watcher actually executed its probe (guards against a trivially
        // passing test where the hook was skipped).
        assert!(
            watcher_ran.load(Ordering::SeqCst),
            "the seam watcher must have run its lock probe",
        );

        // DECISIVE: at the publish/remove seam the `flights` lock was HELD, so
        // the publish and the removal are one atomic critical section. Pre-fix
        // (publish, drop lock, remove) this is `true` and the test FAILS.
        assert!(
            !lock_was_free_at_seam.load(Ordering::SeqCst),
            "the `flights` lock must be HELD continuously across publish AND removal of a \
             non-retained lane — a free lock at the seam means a claimant could first-observe \
             the non-retained `Done` (P1b atomic-window regression)",
        );

        // After the leader returns, the non-retained lane is fully gone (the
        // removal inside the held critical section took effect).
        assert_eq!(
            group.test_flight_strong_count(&"node".to_string(), token),
            0,
            "the non-retained lane must be removed once the leader returns",
        );
    }

    /// A leader whose `compute` PANICS must not wedge its waiters forever:
    /// they re-elect a fresh leader and COMPLETE (P1c — panic-safety /
    /// deadlock-on-panic).
    ///
    /// Several follower threads commit onto a leader's in-flight lane, then
    /// the leader's `compute` panics. The panic-abort guard transitions the
    /// lane to `Aborted`, notifies the waiters, releases the leader's
    /// self-pin, and removes the lane — all under the `flights` lock — then
    /// resumes the panic (so the leader thread itself `join()`s to an
    /// `Err`). Every waiter wakes to `Aborted`, releases its pin, and
    /// re-elects: exactly one becomes the fresh leader and runs ITS own
    /// `compute` (which succeeds), and the rest join that result. The whole
    /// flight therefore completes with the success value.
    ///
    /// WATCHDOG: the followers' results are collected through a channel with
    /// a bounded `recv_timeout`. If a regression leaves a waiter blocked on
    /// the condvar (the pre-fix behaviour: a panicking leader never
    /// transitions `Running` → terminal, never notifies, never releases its
    /// pin, never removes the entry, so waiters wait FOREVER and future
    /// callers find a permanently `Running` lane), the `recv_timeout` fires
    /// and the test FAILS loudly instead of hanging the suite.
    ///
    /// DISCRIMINATES P1c: against the pre-fix code this test HANGS — the
    /// follower threads never return — and the watchdog converts that hang
    /// into a hard failure. Post-fix every follower returns the re-elected
    /// success value well within the timeout.
    #[test]
    fn leader_compute_panic_lets_waiters_reelect_and_complete() {
        use std::sync::mpsc;
        use std::time::Instant;

        const N_FOLLOWERS: usize = 4;
        const SUCCESS_VALUE: usize = 77;
        // Total strong refs on the lane once all followers are parked as
        // waiters while the leader is mid-compute:
        //   1 leader's `state` binding + 1 map entry
        // + N_FOLLOWERS run-claim clones (each parked follower holds one).
        const FOLLOWERS_COMMITTED_COUNT: usize = 2 + N_FOLLOWERS;

        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let token = StoreViewCompatToken {
            epoch: 8,
            session: None,
            validity_fingerprint: 0,
        };
        let leader_in_compute = Arc::new((Mutex::new(false), Condvar::new()));
        let recompute_count = Arc::new(AtomicUsize::new(0));

        // Leader: enters `compute`, waits until every follower has committed
        // as a waiter on its lane, then PANICS.
        let leader = {
            let group = Arc::clone(&group);
            let leader_in_compute = Arc::clone(&leader_in_compute);
            std::thread::spawn(move || {
                let _ = group.run_retaining(
                    "node".to_string(),
                    token,
                    || -> Result<usize, &'static str> {
                        // Announce that the leader is mid-flight (lane is
                        // `Running`), so followers may commit.
                        {
                            let (lock, cv) = &*leader_in_compute;
                            *lock.lock() = true;
                            cv.notify_all();
                        }
                        // Wait until all followers are parked as waiters on
                        // this lane, so the abort genuinely has to WAKE
                        // committed waiters (the exact deadlock surface).
                        let mut spins = 0;
                        while group.test_flight_strong_count(&"node".to_string(), token)
                            < FOLLOWERS_COMMITTED_COUNT
                        {
                            spins += 1;
                            assert!(
                                spins < 10_000_000,
                                "followers never committed onto the leader's lane",
                            );
                            std::thread::yield_now();
                        }
                        panic!("leader compute boom");
                    },
                    |_| true,
                );
            })
        };

        // Wait until the leader is provably inside `compute`.
        {
            let (lock, cv) = &*leader_in_compute;
            let mut in_compute = lock.lock();
            while !*in_compute {
                cv.wait(&mut in_compute);
            }
        }

        // Followers: commit onto the leader's in-flight lane, then (post
        // panic) re-elect and complete. Each reports its outcome through the
        // watchdog channel.
        let (tx, rx) = mpsc::channel::<Result<usize, &'static str>>();
        let mut follower_handles = Vec::new();
        for _ in 0..N_FOLLOWERS {
            let group = Arc::clone(&group);
            let recompute_count = Arc::clone(&recompute_count);
            let tx = tx.clone();
            follower_handles.push(std::thread::spawn(move || {
                let outcome = group
                    .run_retaining(
                        "node".to_string(),
                        token,
                        || {
                            // Only a re-elected fresh leader runs this; count
                            // the genuine recomputes.
                            recompute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(SUCCESS_VALUE)
                        },
                        |_| true,
                    )
                    .map(|run| *run.value);
                // Best-effort send; the receiver may already have all it
                // needs.
                let _ = tx.send(outcome);
            }));
        }
        drop(tx);

        // WATCHDOG: every follower must report within the timeout. A hang
        // (pre-fix deadlock) trips the timeout and fails the test.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut collected = Vec::new();
        while collected.len() < N_FOLLOWERS {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "panic-safety watchdog: only {}/{} followers completed within the timeout — a \
                 waiter is blocked behind the panicked leader (P1c deadlock-on-panic regression)",
                collected.len(),
                N_FOLLOWERS,
            );
            match rx.recv_timeout(remaining) {
                Ok(outcome) => collected.push(outcome),
                Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                    "panic-safety watchdog: only {}/{} followers completed within the timeout — a \
                     waiter is blocked behind the panicked leader (P1c deadlock-on-panic regression)",
                    collected.len(),
                    N_FOLLOWERS,
                ),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        for handle in follower_handles {
            handle.join().expect("follower thread must not panic");
        }
        // The leader thread itself unwound (its `compute` panicked); joining
        // it yields the panic. Assert it DID panic (the abort path ran).
        assert!(
            leader.join().is_err(),
            "the leader thread's `compute` panic must propagate after the abort guard runs",
        );

        // Every follower completed with the re-elected success value.
        assert_eq!(collected.len(), N_FOLLOWERS);
        for outcome in &collected {
            assert_eq!(
                *outcome,
                Ok(SUCCESS_VALUE),
                "every waiter must re-elect and complete with the fresh leader's success value",
            );
        }
        // After the abort, the re-electing waiters cold-recompute through a
        // fresh leader. At least one genuine recompute must occur (the
        // panicked leader produced no value), and never more than one per
        // re-electing thread. The exact count is best-effort dedup: a fresh
        // leader that finishes before the other re-electors commit onto its
        // new lane leaves them to spawn their own fresh leader — that is
        // correct re-election, not a defect. The decisive panic-safety
        // property is the watchdog above (no waiter hangs) plus every
        // waiter completing with the success value.
        let recomputes = recompute_count.load(Ordering::SeqCst);
        assert!(
            (1..=N_FOLLOWERS).contains(&recomputes),
            "after the leader panic, the re-electing waiters must cold-recompute through a fresh \
             leader (expected 1..={N_FOLLOWERS} recomputes, got {recomputes})",
        );

        // The lane is fully reaped once the burst drains.
        assert_eq!(
            group.test_flight_strong_count(&"node".to_string(), token),
            0,
            "the lane must be fully reaped after the panic-and-re-election burst drains",
        );
    }

    /// A panic in the `retain` PREDICATE (not just `compute`) also aborts
    /// the lane, so a later claimant is not wedged behind a permanently
    /// `Running` lane (P1c — the abort guard spans `compute` AND `retain`).
    ///
    /// The guard is disarmed only AFTER `retain` returns, so a `retain`
    /// panic — which happens after `compute` succeeded but before the
    /// terminal is published — still unwinds through the guard and aborts
    /// the lane. This test runs the panicking leader on its own thread
    /// (joining it surfaces the panic), then asserts the lane is gone and a
    /// fresh claim on the same key/token re-elects and completes.
    ///
    /// DISCRIMINATES P1c (retain arm): pre-fix, a `retain` panic left the
    /// lane `Running` forever; the post-panic claim below would block on the
    /// condvar and the test would hang. Post-fix the lane is aborted and the
    /// fresh claim cold-computes.
    #[test]
    fn leader_retain_panic_aborts_lane_so_later_claim_reelects() {
        use std::time::Instant;

        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let token = StoreViewCompatToken {
            epoch: 12,
            session: None,
            validity_fingerprint: 0,
        };

        // Leader: `compute` succeeds, then the `retain` predicate panics.
        let leader = {
            let group = Arc::clone(&group);
            std::thread::spawn(move || {
                let _ = group.run_retaining(
                    "node".to_string(),
                    token,
                    || -> Result<usize, &'static str> { Ok(1) },
                    |_| panic!("retain predicate boom"),
                );
            })
        };
        assert!(
            leader.join().is_err(),
            "the leader thread's `retain` panic must propagate after the abort guard runs",
        );

        // The aborted lane must be gone (no permanently `Running` slot).
        assert_eq!(
            group.test_flight_strong_count(&"node".to_string(), token),
            0,
            "a `retain` panic must abort + remove the lane, leaving no `Running` slot behind",
        );

        // A fresh claim re-elects and completes — guarded by a watchdog so a
        // regression (permanently `Running` lane) fails rather than hangs.
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        std::thread::spawn(move || {
            let run = group
                .run_retaining(
                    "node".to_string(),
                    token,
                    || -> Result<usize, &'static str> { Ok(55) },
                    |_| true,
                )
                .unwrap();
            let _ = tx.send(*run.value);
        });
        let value = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| {
                let _ = Instant::now();
                panic!(
                    "post-`retain`-panic watchdog: a fresh claim blocked behind a permanently \
                     `Running` lane (P1c retain-arm deadlock regression)",
                )
            });
        assert_eq!(
            value, 55,
            "after a `retain` panic aborts the lane, a fresh claim must cold-compute to completion",
        );
    }

    #[test]
    fn singleflight_forks_same_epoch_distinct_validity_fingerprint() {
        // SOUNDNESS: the coalescing lane is keyed by the
        // COMPLETE compat token. Two requests for the same key whose views
        // share an epoch + session but differ in `validity_fingerprint` (a
        // validity-affecting change the epoch does not cover — e.g. a
        // different `artifact_generation` / `load_generation` / env /
        // identity) MUST fork into separate lanes and each compute its own
        // result. A follower in `run_stable_request` returns the LEADER's
        // result WITHOUT revalidating against its own view, so coalescing
        // these would hand a follower a result computed under a different
        // view. Were `StoreViewCompatToken` to carry no
        // `validity_fingerprint` field, distinct-validity-same-epoch
        // requests would collapse onto ONE lane (computes == 1, shared Arc)
        // — exactly the hole this proves closed.
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = [
            StoreViewCompatToken {
                epoch: 9,
                session: None,
                validity_fingerprint: 0xA1A1_A1A1,
            },
            StoreViewCompatToken {
                epoch: 9,
                session: None,
                validity_fingerprint: 0xB2B2_B2B2,
            },
        ]
        .into_iter()
        .map(|token| {
            let group = Arc::clone(&group);
            let start = Arc::clone(&start);
            let computes = Arc::clone(&computes);
            std::thread::spawn(move || {
                start.wait();
                group
                    .run("node".to_string(), token, || {
                        computes.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(token.validity_fingerprint as usize)
                    })
                    .unwrap()
            })
        })
        .collect();

        start.wait();
        let mut handles = handles.into_iter();
        let first = handles.next().unwrap().join().unwrap();
        let second = handles.next().unwrap().join().unwrap();

        assert_eq!(
            computes.load(Ordering::SeqCst),
            2,
            "two views with the same epoch but distinct validity fingerprints MUST \
             fork into separate lanes and each compute (no follower coalescing)"
        );
        assert_eq!(first.role, SingleflightRole::Leader);
        assert_eq!(second.role, SingleflightRole::Leader);
        assert!(
            first.forked_lane || second.forked_lane,
            "the same-key/distinct-fingerprint lanes must be observed as forked"
        );
        assert!(
            !Arc::ptr_eq(&first.value, &second.value),
            "forked lanes MUST NOT share a result Arc (a follower must never receive \
             a leader's result computed under a different complete token)"
        );
    }

    // -----------------------------------------------------------------------
    // ResolverCounters tests
    // -----------------------------------------------------------------------

    #[test]
    fn resolver_counters_default_is_zero() {
        let counters = ResolverCounters::new();
        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 0);
        assert_eq!(snap.node_cache_misses, 0);
        assert_eq!(snap.singleflight_coalesces, 0);
        assert_eq!(snap.cycle_detections, 0);
        assert_eq!(snap.cross_view_lane_forks, 0);
        assert_eq!(snap.route_fact_reuses, 0);
    }

    #[test]
    fn resolver_counters_increment_and_snapshot() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        counters.record_cache_hit();
        counters.record_cache_miss();
        counters.record_singleflight_coalesce();
        counters.record_cycle_detection();
        counters.record_cross_view_lane_fork();
        counters.record_route_fact_reuse();
        counters.record_route_fact_reuse();
        counters.record_route_fact_reuse();

        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 2);
        assert_eq!(snap.node_cache_misses, 1);
        assert_eq!(snap.singleflight_coalesces, 1);
        assert_eq!(snap.cycle_detections, 1);
        assert_eq!(snap.cross_view_lane_forks, 1);
        assert_eq!(snap.route_fact_reuses, 3);
    }

    #[test]
    fn resolver_counters_reset_clears_all() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        counters.record_cache_miss();
        counters.record_singleflight_coalesce();

        counters.reset();
        let snap = counters.snapshot();
        assert_eq!(snap, ResolverCountersSnapshot::default());
    }

    #[test]
    fn resolver_counters_thread_safe() {
        let counters = Arc::new(ResolverCounters::new());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..3)
            .map(|_| {
                let counters = Arc::clone(&counters);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..100 {
                        counters.record_cache_hit();
                        counters.record_cache_miss();
                    }
                })
            })
            .collect();

        barrier.wait();
        for h in handles {
            h.join().unwrap();
        }

        let snap = counters.snapshot();
        assert_eq!(snap.node_cache_hits, 300);
        assert_eq!(snap.node_cache_misses, 300);
    }

    #[test]
    fn resolver_counters_snapshot_is_not_default_after_recording() {
        let counters = ResolverCounters::new();
        counters.record_cache_hit();
        let snap = counters.snapshot();
        assert_ne!(
            snap,
            ResolverCountersSnapshot::default(),
            "snapshot should differ from default after recording"
        );
    }
}
