use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

pub(crate) mod ambient_resolve;
pub(crate) mod bare_name_resolve;
pub(crate) mod cache_keys;
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
mod fallthrough_request;
pub mod fallthrough_resolver;
pub mod prepared_decl;
pub mod resolver_runtime;
pub mod route_demand;
mod runtime_values;
pub mod shallow_file_state;
pub(crate) mod surface_projector;
#[cfg(test)]
mod surface_projector_tests;
pub mod symbol_resolver;
pub mod type_expansion;
pub mod type_expansion_host;
pub mod type_expansion_verter;
pub mod vue_default_synth;

pub mod checker_text_adapter;

pub mod fact_read_set;
pub mod fuses;
pub mod imported_root_db;
pub(crate) mod resolver_context;
pub mod route_db;
pub(crate) mod scope_shadowing;
pub(crate) mod session_resolver_context;

pub use fact_read_set::{FactReadSet, FactReadSetCell, FactReadSetFinalise};
pub(crate) use resolver_context::{MaterializeScopeObservation, ResolverContext};
pub(crate) use session_resolver_context::SessionResolverContext;

pub use fuses::{FuseBudgets, FuseState, FuseTrip};
pub use imported_root_db::{ImportedRootDb, ImportedRootResult};
pub use route_db::{
    BarrelRouteSurface, EffectiveExportEntry, EffectiveExportSetEntry, EffectiveExportSetKey,
    RouteDb, RouteResult,
};

pub type ResolverHash16 = verter_semantic::analysis::Hash16;
pub use component_meta::{
    collect_requested_binding_names, component_meta_resolved_macros, component_meta_type_registry,
    resolve_component_meta_parts, resolved_elements_to_type_expr_via_type_text,
    ComponentMetaEvalOutputs, ComponentMetaResolutionPurpose, ComponentMetaResolverHost,
    ResolvedComponentMetaParts, ResolvedImportedMacroSurface, ResolvedJsdocBlock, ResolvedJsdocTag,
    ResolvedMacroMeta, ResolvedTypeRegistryMeta,
};
pub use component_meta_query_engine::ComponentMetaQueryEngine;
pub(crate) use component_meta_query_engine::{
    projected_surface_from_semantic_node, projected_surface_to_expanded_shape,
    projected_surface_to_type_expr, type_expr_contains_semantic_miss, type_expr_has_any_object_arm,
    type_expr_is_expanded_surface,
};
pub use component_meta_request::{run_component_meta_request, ComponentMetaRequestHost};
pub use declaration_metadata::{
    resolve_direct_local_type_declaration, resolve_local_type_declaration,
    resolve_type_declaration, DeclarationMetadataResolver, ResolvedDeclarationKind,
    ResolvedExportTarget, ResolvedLocalTypeSymbolMetadata, ResolvedTypeDeclaration,
};
pub use export_graph::{
    get_export_span_follow_reexports_from_graph, resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort, resolve_named_export_from_graph, ExportGraphFileKind,
    ExportGraphResolver, ExportSurface, ResolvedGraphExport,
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
    collect_dynamic_root_candidates_from_type, evaluate_value_expression_via_env_or_dispatch,
    extend_unique_fact_versions, fallthrough_cache_key, hash_prop_type_overrides,
    inject_prop_type_overrides, known_spread_keys_from_type_expr, merge_fallthrough_branches,
    push_partial_reason, resolve_fallthrough_surface, resolve_usage_prop_type,
    structural_substitute_typeof_refs, DynamicRootCandidate, FallthroughComputeHost,
    FallthroughResolutionView, FallthroughResolverHost, KnownSpreadKeys, ResolvedConsumedBindings,
    ResolvedFallthroughSurface,
};
pub use fallthrough_request::{run_fallthrough_request, FallthroughRequestHost};
pub use prepared_decl::{
    build_prepared_type_decl_cache, build_prepared_value_decl_cache, prepare_exported_type_decl,
    prepare_exported_value_decl, prepare_local_type_decl, prepare_local_value_decl,
};
pub use route_demand::{
    merge_route_demands, RouteDemand, RouteProvenance, RouteProvenanceKind, RoutedExternalDep,
    RoutedSymbolResult, RoutedSymbolStatus, SymbolSpace,
};
pub use runtime_values::{
    materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver,
};
pub use shallow_file_state::{
    BudgetDomain, BudgetExceededFailure, ExportTarget, ExternalSymbolRef, ImportTarget,
    LocalClosureResult, LocalClosureStatus, ResolutionBudgets, ResolutionCounters,
    ShallowFileState, ShallowImportResolver, ShallowTypeSymbol, ShallowTypeView,
    ShallowValueSymbol, WildcardReexport,
};
pub use surface_projector::{
    project_macro_surfaces, slot_info_from_type_expr, ProjectedMacroSurfaces, ResolvedNativeProp,
};

/// Lane-identity token for singleflight deduplication.
///
/// Widened in Path C C14 to include session identity so that two sessions
/// with different overlays but the same epoch never coalesce into the same
/// singleflight lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewCompatToken {
    pub epoch: u64,
    pub session: Option<u64>,
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
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PermissiveStoreView;

impl StoreView for PermissiveStoreView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FallthroughNodeKind {
    ComponentRootFollow,
    IntrinsicSurfaceLoad,
    ChildComponentSurfaceFollow,
    ConsumedBindingEvaluation,
    BranchUnionMerge,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughNodeKey {
    pub canonical_component_id: String,
    pub node_kind: FallthroughNodeKind,
    pub override_fingerprint: u64,
    pub behavior_flags: u32,
    pub branch_selector: Option<String>,
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
        if let Some(cached) = executor.try_get_cached(&store_view) {
            return Ok(RequestRunResult {
                value: cached,
                source: RequestSource::Cache,
                attempts: attempt + 1,
            });
        }

        let flight = singleflight.run(cache_key.clone(), store_view.compat_token(), || {
            if let Some(cached) = executor.try_get_cached(&store_view) {
                return Ok(StableExecutionValue {
                    value: cached,
                    stable: true,
                });
            }

            let value = executor.compute(&store_view)?;
            let stable = executor.is_stable(&store_view);
            if stable {
                executor.store_stable(&value);
            }

            Ok(StableExecutionValue { value, stable })
        })?;

        if flight.value.stable {
            return Ok(RequestRunResult {
                value: flight.value.value.clone(),
                source: RequestSource::Flight {
                    role: flight.role,
                    forked_lane: flight.forked_lane,
                },
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
        TView: StoreView,
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
        TView: StoreView,
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
        TView: StoreView,
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

#[derive(Debug)]
pub struct SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    #[allow(clippy::type_complexity)]
    flights: Mutex<FxHashMap<(K, StoreViewCompatToken), Arc<FlightState<V, E>>>>,
}

#[derive(Debug)]
struct FlightState<V, E> {
    inner: Mutex<FlightInner<V, E>>,
    ready: Condvar,
}

#[derive(Debug, Clone)]
enum FlightInner<V, E> {
    Running { owner: std::thread::ThreadId },
    Done(Result<Arc<V>, E>),
}

impl<K, V, E> Default for SingleflightGroup<K, V, E>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self {
            flights: Mutex::new(FxHashMap::default()),
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
        let lane_key = (key.clone(), token);
        let current_thread = std::thread::current().id();

        let (state, leader, forked_lane) = {
            let mut flights = self.flights.lock();
            let forked_lane = flights.keys().any(|(existing_key, existing_token)| {
                existing_key == &key && *existing_token != token
            });
            if let Some(existing) = flights.get(&lane_key).cloned() {
                (existing, false, forked_lane)
            } else {
                let state = Arc::new(FlightState {
                    inner: Mutex::new(FlightInner::Running {
                        owner: current_thread,
                    }),
                    ready: Condvar::new(),
                });
                flights.insert(lane_key.clone(), state.clone());
                (state, true, forked_lane)
            }
        };

        if leader {
            let result = compute().map(Arc::new);
            {
                let mut inner = state.inner.lock();
                *inner = FlightInner::Done(result.clone());
                state.ready.notify_all();
            }
            self.flights.lock().remove(&lane_key);
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
                    drop(inner);
                    return compute().map(|value| SingleflightRunResult {
                        value: Arc::new(value),
                        role: SingleflightRole::Leader,
                        forked_lane,
                    });
                }
                FlightInner::Running { .. } => state.ready.wait(&mut inner),
                FlightInner::Done(result) => {
                    return result.clone().map(|value| SingleflightRunResult {
                        value,
                        role: SingleflightRole::Follower,
                        forked_lane,
                    });
                }
            }
        }
    }

    pub fn clear(&self) {
        self.flights.lock().clear();
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
            }
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
            },
            valid_facts: FxHashSet::default(),
        };

        assert!(cache.get_if_valid(&"node".to_string(), &view).is_none());
    }

    #[test]
    fn compat_token_is_exact_snapshot_epoch_in_v1() {
        let first = StoreViewCompatToken {
            epoch: 10,
            session: None,
        };
        let second = StoreViewCompatToken {
            epoch: 10,
            session: None,
        };
        let third = StoreViewCompatToken {
            epoch: 11,
            session: None,
        };

        assert_eq!(first, second);
        assert_ne!(first, third);
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

    #[test]
    fn singleflight_forks_incompatible_tokens() {
        let group = Arc::new(SingleflightGroup::<String, usize, &'static str>::default());
        let start = Arc::new(Barrier::new(3));
        let computes = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = [
            StoreViewCompatToken {
                epoch: 1,
                session: None,
            },
            StoreViewCompatToken {
                epoch: 2,
                session: None,
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
