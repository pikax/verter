//! Project-global host-owned cache root
//!
//! One [`ProjectTypeStore`] per loaded workspace / project-root set. It is
//! the authoritative cache graph that replaces request-view-scoped memoization
//! for component-meta and the shared type resolver.
//!
//! ## Scope of this module
//!
//! - [`ArtifactRequirements`] — the bitflag-driven artifact DAG boundary
//! - [`IndexedReady`] — the canonical post-parse artifact
//! - [`AnalysisReady`] — analysis augmentation keyed by
//!   [`verter_semantic::analysis::AnalysisScope`]
//! - [`CanonicalArtifactKey`] / [`AnalysisArtifactKey`] — content-rooted
//!   cache keys that collapse concurrent cold requests for the same file
//!   version onto one build
//! - [`ProjectTypeStore`] — the top-level cache container owned by
//!   [`crate::VerterHost`]
//!
//! The actual migration of existing call sites onto these types lives in the
//! subsequent phases of the project-global overhaul — this module introduces
//! the foundation without rewiring the hot path yet.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use verter_semantic::analysis::{AnalysisScope, Hash16};

use crate::component_meta_caches::{
    DeclarationLookupDb, ImportedRegistryDb, OwnerCollectionDb, ResolvabilityDb, ShapeCacheDb,
};
use crate::component_meta_result_db::ComponentMetaResultDb;
use crate::file_artifact_store::FileArtifactStore;
use crate::intrinsic_registry::IntrinsicRegistry;
use crate::owner_import_surface::OwnerImportSurfaceDb;
use crate::resolver_core::imported_root_db::ImportedRootDb;
use crate::resolver_core::route_db::RouteDb;
use crate::semantic_query::DepVersion;
// `HostResolvedNamedTypeKey` lives in `semantic_query` alongside the
// `ResolvedNamedType` query variant; the shared semantic graph owns both
// the identity mapping and the stored payloads.
pub use crate::semantic_query::HostResolvedNamedTypeKey;
use crate::semantic_query_memo::SemanticGraphStore;

// ──────────────────────────────────────────────────────────────────────────
// ArtifactRequirements — readiness DAG boundary
// ──────────────────────────────────────────────────────────────────────────

bitflags::bitflags! {
    /// Artifact readiness requested by a caller.
    ///
    /// The runtime receives a flag set. Feature or caller presets may map to
    /// these flags at the API boundary, but the core runtime only sees the
    /// flag set plus any required [`AnalysisScope`].
    ///
    /// - [`Self::INDEXED`] — canonical imports / exports / shallow symbol
    ///   inventory (the [`IndexedReady`] payload).
    /// - [`Self::ANALYSIS`] — analysis augmentation ([`AnalysisReady`]).
    ///
    /// `SourceReady` (raw source + parse) is the planner's prerequisite and
    /// is never requested directly through this flag set.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ArtifactRequirements: u32 {
        const INDEXED  = 1 << 0;
        const ANALYSIS = 1 << 1;
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Artifact cache keys
// ──────────────────────────────────────────────────────────────────────────

/// Cache identity for canonical-file artifacts. Two callers for the same
/// `(canonical_id, whole_hash)` pair converge on one materialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalArtifactKey {
    pub canonical_id: Arc<str>,
    pub whole_hash: Hash16,
}

/// Cache identity for analysis artifacts. Includes the [`AnalysisScope`]
/// requested by the caller so a broader cached scope can satisfy a narrower
/// later request via bitflag containment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnalysisArtifactKey {
    pub canonical_id: Arc<str>,
    pub whole_hash: Hash16,
    pub scope: AnalysisScope,
}

// ──────────────────────────────────────────────────────────────────────────
// IndexedReady — canonical post-parse lowered artifact
// ──────────────────────────────────────────────────────────────────────────

/// The authoritative per-file lowered shallow artifact for cross-file type
/// resolution and component-meta.
///
/// `IndexedReady` is what a canonical file lowers into after the scheduler
/// parse. It owns the shallow symbol inventory, canonical imports and
/// exports, the raw and eval source snapshots, the script analysis snapshot,
/// and anything else later semantic passes need so they do not rescan the
/// raw file.
///
/// The OXC parse arena is transient — `IndexedReady` stores only owned
/// `Send + Sync` data so long-lived host-owned caches do not carry borrowed
/// AST pointers.
///
/// `IndexedReady` is the single canonical post-parse artifact: every
/// consumer reads it from [`FileArtifactStore`] through
/// [`ProjectTypeStore::indexed`], and the `ensure_indexed_ready_serve`
/// materialise closure is its single producer.
#[derive(Debug, Clone)]
pub struct IndexedReady {
    pub whole_hash: Hash16,
    /// Canonical imports / exports + shallow symbol inventory.
    pub shallow_state: Arc<crate::resolver_core::shallow_file_state::ShallowFileState>,
    /// Resolved import-edge table for this file.
    pub import_routes: Arc<FxHashMap<String, crate::types::DependencyResolution>>,
    /// Optional hash-summary of the import-route table. Used by fact-based
    /// cache validation so route-surface changes invalidate only the files
    /// whose imports actually shifted.
    pub import_route_hash: Option<Hash16>,
    /// Optional hash-summary of the file's route surface (the
    /// declaration-side data `hash_route_surface` digests). Symmetric
    /// to [`import_route_hash`]; populated when
    /// [`ShallowFileState::has_resolvable_surface`] returns `true`.
    /// Used by `current_derived_fact_hash` to
    /// answer cached-route fact queries without rehashing per call.
    /// Invalidation lifecycle == `IndexedReady`'s content-hash
    /// lifecycle: when the canonical's whole_hash changes, a fresh
    /// `IndexedReady` is built and `route_hash` is recomputed.
    pub route_hash: Option<Hash16>,
    /// Workspace `content_generation` captured at edge-canonicalization
    /// time — the generation at which this artifact's cross-file edge
    /// `canonical_id`s (wildcard reexports, named reexports, and plain
    /// import targets, baked into `shallow_state` and `import_routes`)
    /// were resolved. Those edges depend on the dependency file set, NOT
    /// this file's own content, so a content-pinned `IndexedReady` whose
    /// owner content is unchanged can still hold stale edges after a
    /// dependency appears or retargets (e.g. a `.js` edge whose `.d.ts`
    /// companion later appears). Route-surface consumers validate it
    /// through the shared edge-currency oracle
    /// (`route_surface_is_edge_current`): a cross-file-edge-bearing
    /// surface is edge-current only while
    /// `edge_generation == ws().content_generation()`.
    /// A VALUE field (read-side validation) — never a cache key (R6).
    pub edge_generation: u64,
    /// [`ProjectTypeStore::current_project_generation`] captured when this
    /// artifact's route surface was built. Route-resolution mutations
    /// (`configure_projects` / `set_exact_resolutions` /
    /// `configure_resolver`) bump `project_generation` WITHOUT bumping
    /// `content_generation`, so a content-current artifact whose
    /// cross-file edges were resolved under the old project graph is
    /// route-stale: the read gate (`indexed_surface_is_current`) demands
    /// a current stamp for any surface with cross-file edges and routes a
    /// stale one through the edge-refresh materialise (the
    /// content-addressed payload is reused; only the route surface
    /// rebuilds). A VALUE field — never a cache key (R6).
    pub project_generation: u64,
    /// The owner's live `parse_env_hash` (the R21 parse dimension)
    /// captured at materialise time — the parse environment this
    /// artifact's `framework_parse` / `shallow_state` / `decl_bodies` were
    /// built under. Today the base parse env derives from constant
    /// workspace parser flags, but the dimension is load-bearing: the
    /// reuse gates demand equality with the owner's LIVE parse env, so
    /// the day per-project parser flags diverge, a moved parse env
    /// routes the artifact through the FULL re-materialise (re-parse)
    /// instead of the parse-reusing edge refresh or the
    /// route-insensitive no-edge reuse. A VALUE field — never a cache
    /// key (R6); the artifact stays stored under the canonical-keyed
    /// legacy `FileArtifactKey`.
    pub parse_env_hash: Hash16,
    /// Raw file source as-read. Shared immutable handle across consumers.
    pub raw_source: Arc<str>,
    /// Script source used as the body of the eval environment. For a `.vue`
    /// SFC this is **position-preserving**: the same length as `raw_source`
    /// with each `<script>` block's content copied to its raw SFC byte range
    /// and every non-script byte whitespace-blanked (original CR/LF preserved),
    /// so every OXC-produced span is SFC-absolute by construction. For a
    /// non-SFC file this equals the raw source verbatim.
    pub eval_source: Arc<str>,
    /// Framework-neutral parse artifact when the canonical file is a
    /// framework carrier. Plain scripts carry `None`.
    pub framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
    /// Script-level analysis snapshot (imports/exports/macros/bindings/etc.).
    /// Always present after materialization.
    pub script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
    /// Cached per-export signatures used by smart dependent invalidation.
    pub export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
    /// File-level analysis snapshot consumed by component-meta / linter
    /// pipelines.
    pub snapshot: Arc<crate::types::FileAnalysisSnapshot>,
    /// Cached HEADER-ONLY external-type analysis used by the shared type
    /// resolver (import/export/reexport tables + symbol name inventory;
    /// no dependency names, no raw surfaces — body-derived data lives on
    /// the shallow state's lazy declaration-body memo).
    pub external_type_analysis:
        Arc<verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource>,
    /// Mirror of `script_analysis.flags & DECLARES_INTERFACE_APP_CONFIG`
    /// projected onto `IndexedReady` so the
    /// `AppConfigNoOverrideProofDb` production producer can short-circuit
    /// the proof for files that demonstrably cannot contribute an
    /// `interface AppConfig` override without re-walking the analysis
    /// snapshot. Mirrored at materialization time from
    /// [`verter_semantic::analysis::AnalysisFlags::DECLARES_INTERFACE_APP_CONFIG`].
    pub declares_interface_app_config: bool,
}

impl IndexedReady {
    /// Whether this artifact's surface carries any cross-file edges —
    /// resolved import routes, import targets, plain/wildcard reexports.
    /// A surface WITHOUT cross-file edges is insensitive to
    /// route-resolution mutations: nothing on it can retarget, so neither
    /// the `project_generation` stamp nor the `edge_generation` stamp
    /// gates its reuse.
    ///
    /// THE complete edge authority: composes the shallow-inventory
    /// component (`has_shallow_cross_file_edges`) with the
    /// `import_routes` table, whose entries (the external `src=` class,
    /// caller-pushed route snapshots) bake dependency-set-derived targets
    /// the shallow inventory never sees. Every edge-currency consumer
    /// (`route_surface_is_edge_current`, `indexed_surface_is_current`,
    /// `base_snapshot_equivalent`'s stamp gates) consults THIS predicate —
    /// never the component alone.
    #[must_use]
    pub fn has_cross_file_edges(&self) -> bool {
        !self.import_routes.is_empty() || self.shallow_state.has_shallow_cross_file_edges()
    }

    /// Test-only constructor for fact-emission-style fixtures: a minimal
    /// artifact carrying a REAL shallow inventory + sources, with every
    /// other field defaulted.
    ///
    /// Gated `#[cfg(any(test, debug_assertions))]` (the crate's
    /// cross-crate test-constructor convention): integration tests in
    /// `tests/` compile the lib without `cfg(test)` but with debug
    /// assertions, while release production builds compile this
    /// invariant-bypassing construction path out entirely.
    #[cfg(any(test, debug_assertions))]
    pub fn new_for_test_with_state(
        whole_hash: Hash16,
        shallow_state: Arc<crate::resolver_core::shallow_file_state::ShallowFileState>,
        raw_source: Arc<str>,
        eval_source: Arc<str>,
        external_type_analysis: Arc<
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource,
        >,
    ) -> Self {
        Self {
            whole_hash,
            shallow_state,
            import_routes: Arc::new(FxHashMap::default()),
            import_route_hash: None,
            route_hash: None,
            edge_generation: 0,
            project_generation: 0,
            parse_env_hash: [0u8; 16],
            raw_source,
            eval_source,
            framework_parse: None,
            script_analysis: None,
            export_signatures: None,
            snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
            external_type_analysis,
            declares_interface_app_config: false,
        }
    }

    /// Test-only constructor producing a minimal `IndexedReady` with
    /// stub fields. Consumers of this helper only inspect
    /// `whole_hash`, so everything else is empty. Used by the
    /// `no_legacy_trace_surface` integration test to drive
    /// `FileArtifactStore::insert` through the event-emitting path.
    /// Same `#[cfg(any(test, debug_assertions))]` gate as
    /// [`Self::new_for_test_with_state`] — an invariant-bypassing
    /// construction path never ships in release production builds.
    #[cfg(any(test, debug_assertions))]
    pub fn new_for_test(whole_hash: Hash16) -> Self {
        use rustc_hash::FxHashMap;
        let analysis = Arc::new(
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(
            ),
        );
        let shallow = crate::resolver_core::shallow_file_state::ShallowFileState::from_analysis(
            whole_hash,
            Arc::clone(&analysis),
            None,
        );
        Self {
            whole_hash,
            shallow_state: Arc::new(shallow),
            import_routes: Arc::new(FxHashMap::default()),
            import_route_hash: None,
            route_hash: None,
            edge_generation: 0,
            project_generation: 0,
            parse_env_hash: [0u8; 16],
            raw_source: Arc::from(""),
            eval_source: Arc::from(""),
            framework_parse: None,
            script_analysis: None,
            export_signatures: None,
            snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
            external_type_analysis: analysis,
            declares_interface_app_config: false,
        }
    }
}

/// Outcome of one `ensure_indexed_ready_serve` singleflight flight: the built
/// (or reused) artifact PLUS its publication validity.
///
/// `published == true` means the artifact is (or already was) the
/// store-published current surface — safe for any singleflight follower
/// to adopt. `published == false` means the flight's pre-publish fence
/// tripped (a workspace / route mutation landed mid-flight): the result
/// is ReturnOnly — valid ONLY for the request that ran the flight (its
/// request pre-dates the mutation). A follower whose claim may post-date
/// the mutation must NOT adopt it as current; it re-runs against fresh
/// state. The singleflight `retain` predicate keys off this flag so a
/// fenced result is never retained as a joinable rendezvous for late
/// claimants.
#[derive(Debug, Clone)]
pub struct IndexedFlightOutcome {
    pub(crate) indexed: Arc<IndexedReady>,
    pub(crate) published: bool,
}

// ──────────────────────────────────────────────────────────────────────────
// AnalysisReady — scope-parameterised semantic analysis augmentation
// ──────────────────────────────────────────────────────────────────────────

/// Host-owned analysis artifact built on top of [`IndexedReady`].
///
/// Keyed by `(canonical_id, whole_hash, scope)`. A broader cached entry can
/// satisfy a narrower later request when
/// `cached_scope.contains(requested_scope)` — satisfaction is bitflag-based,
/// not enum-ordinal-based.
#[derive(Debug, Clone)]
pub struct AnalysisReady {
    pub whole_hash: Hash16,
    pub scope: AnalysisScope,
    /// Script analysis snapshot (existing shape — migration work in a later
    /// phase replaces this with a reader over `IndexedReady`-owned lowered
    /// block facts).
    pub script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
    /// Per-export signatures for smart invalidation, when the scope requested
    /// [`AnalysisScope::EXPORT_SIGNATURES`].
    pub export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
    /// File-level analysis snapshot used by the existing component-meta and
    /// linter pipelines.
    pub snapshot: Arc<crate::types::FileAnalysisSnapshot>,
}

// ──────────────────────────────────────────────────────────────────────────
// Concrete backing caches
// ──────────────────────────────────────────────────────────────────────────

/// Host-owned cache of per-file [`AnalysisReady`] artifacts.
pub struct AnalysisReadyDb {
    entries: DashMap<AnalysisArtifactKey, Arc<AnalysisReady>>,
    live_counter: Arc<AtomicU64>,
    stale_sweeps: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl AnalysisReadyDb {
    pub fn new() -> Self {
        Self::with_counters(Default::default(), Default::default())
    }

    pub(crate) fn with_counters(live: Arc<AtomicU64>, stale: Arc<AtomicU64>) -> Self {
        Self::with_counters_and_schema_version(
            live,
            stale,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, debug_assertions))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counters_and_schema_version(
            Default::default(),
            Default::default(),
            schema_version,
        )
    }

    fn with_counters_and_schema_version(
        live: Arc<AtomicU64>,
        stale: Arc<AtomicU64>,
        schema_version: u32,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter: live,
            stale_sweeps: stale,
            schema_version,
        }
    }

    /// Strict lookup by full key.
    #[must_use]
    pub fn get(&self, key: &AnalysisArtifactKey) -> Option<Arc<AnalysisReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = self.entries.get(key).map(|v| v.clone());
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .analysis
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .analysis
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Satisfaction lookup — returns any cached entry whose
    /// `(canonical_id, whole_hash)` matches and whose cached scope contains
    /// the requested scope. This is the bitflag-based containment rule
    /// called out in the project-global overhaul plan.
    #[must_use]
    pub fn find_satisfying(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        requested_scope: AnalysisScope,
    ) -> Option<Arc<AnalysisReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = {
            let mut found: Option<Arc<AnalysisReady>> = None;
            for entry in self.entries.iter() {
                let key = entry.key();
                if key.canonical_id.as_ref() == canonical_id
                    && key.whole_hash == whole_hash
                    && key.scope.contains(requested_scope)
                {
                    found = Some(entry.value().clone());
                    break;
                }
            }
            found
        };
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .analysis
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .analysis
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    pub fn insert(&self, key: AnalysisArtifactKey, analysis: Arc<AnalysisReady>) {
        let prev = self.entries.insert(key, analysis);
        if prev.is_some() {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Invalidate every cached entry for `canonical_id`, regardless of
    /// `(whole_hash, scope)`. Called on file-content changes so a new
    /// whole-hash does not keep stale analysis alive. Returns the number
    /// of entries evicted.
    pub fn invalidate_canonical(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        self.entries.retain(|key, _| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                false
            } else {
                true
            }
        });
        if removed > 0 {
            self.live_counter
                .fetch_sub(removed as u64, Ordering::Relaxed);
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        removed
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let key = AnalysisArtifactKey {
            canonical_id: Arc::from(marker),
            whole_hash: [0u8; 16],
            scope: AnalysisScope::empty(),
        };
        let value = Arc::new(AnalysisReady {
            whole_hash: [0u8; 16],
            scope: AnalysisScope::empty(),
            script_analysis: None,
            export_signatures: None,
            snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
        });
        self.insert(key, value);
    }
}

impl Default for AnalysisReadyDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for AnalysisReadyDb {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        let count = self.entries.len();
        self.entries.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        }
        count
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for AnalysisReadyDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent]
    }
    fn invalidate(&self, _domain: crate::invalidation_domain::InvalidationDomain) {
        // AnalysisReady survives project-generation bumps (the
        // (canonical, whole_hash, scope) identity is sufficient);
        // per-canonical eviction is the only invalidation mode.
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for AnalysisReadyDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        self.invalidate_canonical(canonical_id)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Typed-DB shapes (D17 + D18 + D44 + D46 + D48 + D65)
//
// `DashMap`-backed DB wrappers keyed by canonical id. `CompileCacheDb`'s
// per-canonical state is split into `ProfileState` / `DerivedRawState` /
// `DependencyState` (D48).
//
// `TypeResolutionContextDb` consumes the post-lowering owned artifact
// `crate::owned_artifacts::OwnedTypeResolutionContext`. The OXC parser
// arena is dropped at the lowering boundary so the DB can sit on
// `Send + Sync` host caches. There is no separate eval-env DB: the
// per-file `EvalEnv` is not a stored field but the lazy `whole_env()`
// demand product owned by `IndexedReady`'s `DeclBodyMemo` (a shallow
// declaration index plus body locators), materialised on first
// semantic demand.
// ──────────────────────────────────────────────────────────────────────────

/// Cache identity for typed-DB entries that key by canonical-id +
/// content-version. The owned-artifact typed DBs share this shape so
/// writes validate uniformly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnedArtifactKey {
    pub canonical_id: Arc<str>,
    pub whole_hash: Hash16,
}

impl OwnedArtifactKey {
    #[must_use]
    pub fn new(canonical_id: impl Into<Arc<str>>, whole_hash: Hash16) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            whole_hash,
        }
    }
}

/// Host-owned cache for [`crate::owned_artifacts::OwnedTypeResolutionContext`].
/// Currently populated only by tests; no production lowering path
/// writes owned contexts here yet (the borrowed
/// `ParsedTypeResolutionContext` is built fresh per call on the
/// query-time element-resolver path, tracked-debt on the single-engine
/// shrinking ledger).
///
/// **Invariant**: `Send + Sync + 'static` (per axiom A1 — host-owned
/// caches only). The owned-artifact payload itself is `Send + Sync +
/// 'static` so the cache can sit on a `DashMap` without thread-local
/// workarounds.
#[derive(Debug, Default)]
pub struct TypeResolutionContextDb {
    entries: DashMap<OwnedArtifactKey, Arc<crate::owned_artifacts::OwnedTypeResolutionContext>>,
}

impl TypeResolutionContextDb {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up an owned context by canonical-id + content-version.
    /// Returns `None` when no entry is present.
    #[must_use]
    pub fn get(
        &self,
        key: &OwnedArtifactKey,
    ) -> Option<Arc<crate::owned_artifacts::OwnedTypeResolutionContext>> {
        self.entries.get(key).map(|r| Arc::clone(r.value()))
    }

    /// Insert an entry. The `Arc` payload may be shared — the DB itself
    /// is the owner of that shared handle; callers should not mutate
    /// the inner context after insertion.
    pub fn insert(
        &self,
        key: OwnedArtifactKey,
        value: Arc<crate::owned_artifacts::OwnedTypeResolutionContext>,
    ) {
        self.entries.insert(key, value);
    }

    /// Remove all entries — invoked by the project-generation
    /// invalidation cascade.
    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Profile-domain DB for the per-canonical compile cache (D48).
///
/// Stores [`crate::types::ProfileState`] keyed by canonical id. Holds
/// per-profile compile outputs (`compile_slots`, `content_overrides`,
/// `style_overrides`, `latest_diagnostics`, `diagnostics_generation`).
/// Profile-flag changes invalidate this entry; source-content changes
/// preserve it; dep-closure changes preserve it. The unified
/// `bump_project_generation_and_evict` cascade clears all three domain
/// DBs together.
///
/// Accessors return references to the underlying map so call sites use
/// the `entry().or_default()` / `get(canonical)` / `get_mut(canonical)` /
/// `iter()` shapes typical of `DashMap`.
#[derive(Debug, Default)]
pub struct CompileCacheDb {
    /// Backing map: `DashMap<String, ProfileState>`. Public accessors
    /// expose this as `entries()`.
    entries: DashMap<String, crate::types::ProfileState>,
}

impl CompileCacheDb {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a reference to the inner storage. Call sites use
    /// `entry(...) / get(...) / get_mut(...) / iter()` directly on the
    /// returned `DashMap`.
    #[must_use]
    pub(crate) fn entries(&self) -> &DashMap<String, crate::types::ProfileState> {
        &self.entries
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Source-content-domain DB for the per-canonical compile cache (D48).
///
/// Stores [`crate::types::DerivedRawState`] keyed by canonical id. Holds
/// source-content-derived caches (`cached_tsc_extract`, `cached_resolved_meta`,
/// `cached_meta_payload`, `raw_template_analysis`, `cached_fallthrough`,
/// `import_routes`, `evicted`, `evicted_whole_hash`). Source-content changes
/// invalidate this entry; profile-flag changes preserve it; dep-closure
/// changes preserve it. The unified `bump_project_generation_and_evict`
/// cascade clears all three domain DBs together.
///
/// `DerivedRawState::import_routes` is a sub-mirror of
/// [`IndexedReady`]`.import_routes`: same content, different invalidation
/// trigger from the IndexedReady source — see the per-type docstring on
/// [`crate::types::DerivedRawState`] for the sub-mirror lifecycle rationale.
#[derive(Debug, Default)]
pub struct DerivedRawCacheDb {
    /// Backing map: `DashMap<String, DerivedRawState>`. Public accessors
    /// expose this as `entries()`.
    entries: DashMap<String, crate::types::DerivedRawState>,
}

impl DerivedRawCacheDb {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn entries(&self) -> &DashMap<String, crate::types::DerivedRawState> {
        &self.entries
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Dependency-closure-domain DB for the per-canonical compile cache (D48).
///
/// Stores [`crate::types::DependencyState`] keyed by canonical id. Holds
/// resolution metadata (`dependencies`, `resolved_type_hashes`, `aliases`,
/// `generation`). Dep-closure changes invalidate this entry;
/// source-content changes invalidate it (because dep-closure is recomputed
/// on parse); profile-flag changes preserve it. The AUTHORITY-RESET
/// `bump_project_generation_and_evict` cascade (`set_workspace` /
/// `close` only) clears all three domain DBs together; route-resolution
/// generation bumps preserve them.
#[derive(Debug, Default)]
pub struct DependencyCacheDb {
    /// Backing map: `DashMap<String, DependencyState>`. Public accessors
    /// expose this as `entries()`.
    entries: DashMap<String, crate::types::DependencyState>,
}

impl DependencyCacheDb {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn entries(&self) -> &DashMap<String, crate::types::DependencyState> {
        &self.entries
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Host-owned typed cache for resolved-type entries — the shared
/// external-type cache backing
/// `lookup_resolved_external_type_cache_with_view` /
/// `store_resolved_external_type_cache_with_view` in
/// `host_resolve::external_type_resolution`. The bounded
/// clear-all-at-`RESOLVED_TYPE_CACHE_CAP` policy
/// (`crate::types::RESOLVED_TYPE_CACHE_CAP`) lives INSIDE the DB so
/// the policy travels with the storage; the DB hosts the inner mutex.
#[derive(Debug, Default)]
pub struct ResolvedTypeCacheDb {
    /// The shared external-type cache map. Held behind a `Mutex`
    /// because the bounded clear-all-at-cap policy needs an atomic
    /// `len() >= cap → clear() → insert(...)` envelope that
    /// `parking_lot::Mutex::lock()` provides.
    entries: parking_lot::Mutex<
        FxHashMap<crate::types::ResolvedTypeCacheKey, crate::types::ResolvedTypeCacheEntry>,
    >,
}

impl ResolvedTypeCacheDb {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up an entry by key. Caller acquires nothing; the DB
    /// internally locks for the read.
    #[must_use]
    pub(crate) fn lookup(
        &self,
        key: &crate::types::ResolvedTypeCacheKey,
    ) -> Option<crate::types::ResolvedTypeCacheEntry> {
        self.entries.lock().get(key).cloned()
    }

    /// Insert an entry. Honours the bounded clear-all-at-cap policy
    /// (D16 — `crate::types::RESOLVED_TYPE_CACHE_CAP`): when the cache
    /// reaches `RESOLVED_TYPE_CACHE_CAP` entries, the entire map
    /// clears before the new entry is inserted. NOT LRU.
    pub(crate) fn insert(
        &self,
        key: crate::types::ResolvedTypeCacheKey,
        entry: crate::types::ResolvedTypeCacheEntry,
    ) {
        let mut guard = self.entries.lock();
        if guard.len() >= crate::types::RESOLVED_TYPE_CACHE_CAP {
            guard.clear();
        }
        guard.insert(key, entry);
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
    }

    /// Drain every entry whose `dep_canonical_id` matches
    /// `canonical_id`. Per the rehoming-doc §3.3 contract: a per-canonical
    /// content edit invalidates the resolved-type entries that
    /// depended on the same canonical so the next resolution pass
    /// recomputes against fresh source.
    pub(crate) fn evict_canonical(&self, canonical_id: &str) {
        self.entries
            .lock()
            .retain(|key, _| key.dep_canonical_id != canonical_id);
    }

    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }
}

// Compile-time `Send + Sync + 'static` guards for the typed DBs. A
// regression that introduces a borrowed lifetime field would fail to
// compile here.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<TypeResolutionContextDb>();
    assert_send_sync_static::<CompileCacheDb>();
    assert_send_sync_static::<ResolvedTypeCacheDb>();
};

// ──────────────────────────────────────────────────────────────────────
// Typed-DB invalidation impls
//
// Every DB-typed field on `ProjectTypeStore` MUST implement
// `ParticipatesInInvalidation` AND `InvalidationByCanonical` so the
// cascade in `invalidate_canonical_across_all_dbs` can dispatch to it
// monomorphically.
// ──────────────────────────────────────────────────────────────────────

impl crate::invalidation_domain::ParticipatesInInvalidation for TypeResolutionContextDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.clear(),
            FileContent => {
                // Per-canonical drain through InvalidationByCanonical.
            }
            ResolverState | TypeGraph | ComponentMeta | AppConfigInterfaceMerge => {
                // Not declared; ignore.
            }
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for TypeResolutionContextDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        // Linear scan — O(N) is acceptable here: only tests populate
        // this DB today, so entry counts stay tiny.
        self.entries.retain(|key, _| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                false
            } else {
                true
            }
        });
        removed
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for CompileCacheDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.clear(),
            FileContent => {}
            ResolverState | TypeGraph | ComponentMeta | AppConfigInterfaceMerge => {}
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for CompileCacheDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // Per-canonical eviction mirrors the off-store
        // `compile_cache.remove(canonical)` path that the rehoming
        // subsumed.
        if self.entries.remove(canonical_id).is_some() {
            1
        } else {
            0
        }
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for DerivedRawCacheDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.clear(),
            FileContent => {}
            ResolverState | TypeGraph | ComponentMeta | AppConfigInterfaceMerge => {}
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for DerivedRawCacheDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        if self.entries.remove(canonical_id).is_some() {
            1
        } else {
            0
        }
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for DependencyCacheDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.clear(),
            FileContent => {}
            ResolverState | TypeGraph | ComponentMeta | AppConfigInterfaceMerge => {}
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for DependencyCacheDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        if self.entries.remove(canonical_id).is_some() {
            1
        } else {
            0
        }
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for ResolvedTypeCacheDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, TypeGraph, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.clear(),
            FileContent | TypeGraph => {}
            ResolverState | ComponentMeta | AppConfigInterfaceMerge => {}
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for ResolvedTypeCacheDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // Linear scan over the mutex-protected map. The bounded
        // clear-all-at-cap policy keeps the map small; per-canonical
        // invalidation walks the map once and drops the matching
        // dep_canonical entries.
        let mut guard = self.entries.lock();
        let mut removed = 0usize;
        guard.retain(|key, _| {
            if key.dep_canonical_id == canonical_id {
                removed += 1;
                false
            } else {
                true
            }
        });
        removed
    }
}

/// Per-layer debug counters / cache stats for observability.
///
/// The plan requires explicit counters for live entries, stale entries,
/// sweeps, evictions, and in-flight waiters so memory and coherence behavior
/// is measurable in tests and benchmarks. Each counter is `Arc<AtomicU64>`
/// so the owning DB can hold a handle and update it in-place; the
/// [`ProjectTypeStoreCounters`] struct stays as a single observation
/// surface for tests and telemetry.
#[derive(Debug, Default, Clone)]
pub struct ProjectTypeStoreCounters {
    pub indexed_live: Arc<AtomicU64>,
    pub indexed_stale_sweeps: Arc<AtomicU64>,
    pub analysis_live: Arc<AtomicU64>,
    pub analysis_stale_sweeps: Arc<AtomicU64>,
    pub owner_import_live: Arc<AtomicU64>,
    pub component_meta_live: Arc<AtomicU64>,
    pub component_meta_stale_sweeps: Arc<AtomicU64>,
    pub inflight_waiters: Arc<AtomicU64>,
    /// Live entry count summed across all 10 component-meta engine cache
    /// DBs (Step 3 closure). Each typed DB shares this counter so the
    /// snapshot reflects total host-owned cache occupancy without
    /// per-DB plumbing in the snapshot surface.
    pub component_meta_cache_live: Arc<AtomicU64>,
}

impl ProjectTypeStoreCounters {
    /// Snapshot numeric counters for test assertions and telemetry. Uses
    /// `Relaxed` ordering because these are diagnostic counters, not
    /// synchronization primitives.
    #[must_use]
    pub fn snapshot(&self) -> ProjectTypeStoreCounterSnapshot {
        ProjectTypeStoreCounterSnapshot {
            indexed_live: self.indexed_live.load(Ordering::Relaxed),
            indexed_stale_sweeps: self.indexed_stale_sweeps.load(Ordering::Relaxed),
            analysis_live: self.analysis_live.load(Ordering::Relaxed),
            analysis_stale_sweeps: self.analysis_stale_sweeps.load(Ordering::Relaxed),
            owner_import_live: self.owner_import_live.load(Ordering::Relaxed),
            component_meta_live: self.component_meta_live.load(Ordering::Relaxed),
            component_meta_stale_sweeps: self.component_meta_stale_sweeps.load(Ordering::Relaxed),
            inflight_waiters: self.inflight_waiters.load(Ordering::Relaxed),
            component_meta_cache_live: self.component_meta_cache_live.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of [`ProjectTypeStoreCounters`] for test assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectTypeStoreCounterSnapshot {
    pub indexed_live: u64,
    pub indexed_stale_sweeps: u64,
    pub analysis_live: u64,
    pub analysis_stale_sweeps: u64,
    pub owner_import_live: u64,
    pub component_meta_live: u64,
    pub component_meta_stale_sweeps: u64,
    pub inflight_waiters: u64,
    pub component_meta_cache_live: u64,
}

/// One per [`crate::VerterHost`] / loaded project. Owns the project-global
/// artifact graph and a monotonically increasing project generation counter
/// used for config / workspace-shape invalidation.
///
/// `tsconfig`, path-alias, active-TS-SDK, workspace-folder, package
/// export-target, and explicit project-graph changes must atomically bump
/// this generation before new queries are admitted. File-content edits do
/// **not** bump the project generation — they bump the per-canonical
/// `whole_hash` and invalidate keys by identity.
pub struct ProjectTypeStore {
    project_generation: AtomicU64,
    /// Canonical [`IndexedReady`] cache.
    indexed: FileArtifactStore,
    /// Analysis augmentation cache keyed by `AnalysisScope`.
    analysis: AnalysisReadyDb,
    /// Rehomed routing-surface cache. `RouteDb` survives as the shared
    /// route/barrel authority under project-global validation semantics.
    routes: Arc<RouteDb>,
    /// Temporary transitive-discovery helper. In this collapses to
    /// transitive-only use; by it folds into the shared route /
    /// semantic-query layer.
    imported_roots: Arc<ImportedRootDb>,
    /// Host-owned semantic-query memo table + node arena. Shared across
    /// every consumer that resolves a `SemanticQueryKey` through the
    /// shared query API.
    semantic_graph: Arc<SemanticGraphStore>,
    /// Owner direct-import surface cache. Direct owner imports
    /// resolve exactly once per owner version and every downstream stage
    /// reads the same entry.
    owner_import_surfaces: OwnerImportSurfaceDb,
    /// Final component-meta result cache. Keyed by
    /// `(owner_canonical, owner_whole_hash, options_fingerprint)`.
    /// Payload is [`crate::component_meta_result_db::CachedComponentMetaResult`]
    /// — the native `ComponentMetaAnalysis` plus the sanitized
    /// resolution sidecar template. `get_component_meta` consults
    /// the cache with completion-fence dep-signature validation
    /// before falling back to the cold resolver path; the same cache
    /// short-circuits `get_component_meta_with_resolution` so
    /// audit-mode warm replays return in near-zero time.
    component_meta_results:
        ComponentMetaResultDb<crate::component_meta_result_db::CachedComponentMetaResult>,
    /// TypeScript `intrinsic` registry. Maps resolved
    /// declaration names that have `= intrinsic` bodies to their
    /// implementation arms. Userland aliases like `Pick` / `Omit` never
    /// reach this registry — it is consulted only after the normal
    /// declaration path resolves to `= intrinsic`.
    intrinsic_registry: IntrinsicRegistry,
    // 10 host-owned typed DB wrappers for the component-meta engine's
    // previously engine-local caches. Each DB consumes the
    // [`crate::cache_runtime::singleflight::cooperative_get_or_insert`]
    // primitive (admission-control, panic safety, post-compute
    // revalidation). The engine keeps a per-request
    // `RefCell<FxHashMap>` mirror as non-authoritative scratch.
    imported_registry_db: ImportedRegistryDb,
    declaration_lookup_db: DeclarationLookupDb,
    resolvability_db: ResolvabilityDb,
    owner_collection_db: OwnerCollectionDb,
    /// Universal shape cache. Replaces the previously-split
    /// `MaterializeMemoDb` (TypeExpr-keyed) and `MemberShapeCacheDb`
    /// (SemanticNode-keyed) with a single store keyed on
    /// `ShapeCacheKey { subject, demand }`. The `ShapeSubject` enum
    /// discriminates TypeExpr-start callers (parser-produced
    /// annotations) from SemanticNode-start callers (settled
    /// `SurfaceMember.value`); the `ShapeDemand` carries a path
    /// segments slice + terminal mode + key filter + surface kind so
    /// per-hop path-precise demands narrow naturally.
    /// See [`crate::component_meta_caches::ShapeCacheDb`].
    shape_cache_db: ShapeCacheDb,
    /// Cache for the structural
    /// materialiser. Sole authoritative host-owned materialiser cache.
    /// The canonical removed-symbol list lives in
    /// `tests/no_legacy_walker.rs::RETIRED_SYMBOLS`.
    materialize_structure_db: crate::component_meta_caches::MaterializeStructureDb,
    /// C — host-owned cache for
    /// `meta_resolve::ref_root_reaches_transitive_cycle_node`. BFS
    /// results stored as `(DeclIdentity → bool)` with reverse-index
    /// invalidation matching `MaterializeStructureDb`.
    ref_cycle_db: crate::component_meta_caches::RefCycleResultDb,
    /// Issue #6 — host-owned proof cache for the ComponentConfig
    /// theme variant fast path. Keyed by
    /// `(app_config_decl_canonical_id, component_key_literal)`. An
    /// entry asserts "no `ui[component_key_literal]` override exists
    /// for this `AppConfig`" and is populated by the slow path's
    /// canonical materialization as a side effect (deferred until
    /// `IndexedReady::declares_interface_app_config` lands). The fast
    /// path consults this DB before projecting the prepared theme
    /// value directly. See
    /// [`crate::app_config_proof_db::AppConfigNoOverrideProofDb`].
    app_config_no_override_proof: crate::app_config_proof_db::AppConfigNoOverrideProofDb,
    /// Host-owned typed DB for [`crate::owned_artifacts::OwnedTypeResolutionContext`].
    /// Currently populated only by tests; no production lowering path
    /// writes owned contexts here yet.
    type_resolution_context_db: TypeResolutionContextDb,
    /// Profile-domain DB for the per-canonical compile cache (D48). Holds
    /// [`crate::types::ProfileState`] entries; the §3.4.2 invalidation
    /// matrix governs eviction triggers.
    compile_cache_db: CompileCacheDb,
    /// Content-addressed compile-output cache for
    /// [`crate::types::CompileCacheMode::Content`] requests. Keyed by the
    /// full env-dimension tuple + content hash; one immutable entry per
    /// key, no fact-validation rail (cross-file edits invalidate through
    /// the env-hash dimensions). The fact-validated `Session` mode uses
    /// the per-profile [`compile_cache_db`](Self::compile_cache_db)
    /// instead, so the two cache families are disjoint by construction.
    compile_output_pure_content: crate::cache_runtime::CompileOutputNodePureContent,
    /// Source-content-domain DB for the per-canonical compile cache (D48).
    /// Holds [`crate::types::DerivedRawState`] entries (sub-mirror of
    /// `IndexedReady.import_routes` plus source-derived analyses); the
    /// §3.4.2 invalidation matrix governs eviction triggers.
    derived_raw_cache_db: DerivedRawCacheDb,
    /// Dependency-closure-domain DB for the per-canonical compile cache
    /// (D48). Holds [`crate::types::DependencyState`] entries (deps,
    /// resolved-type hashes, aliases, generation); the §3.4.2 invalidation
    /// matrix governs eviction triggers.
    dependency_cache_db: DependencyCacheDb,
    /// Host-owned typed cache for resolved-type entries — the shared
    /// external-type cache consumed by
    /// `host_resolve::external_type_resolution`; the bounded
    /// clear-all-at-cap policy lives inside the DB.
    resolved_type_cache_db: ResolvedTypeCacheDb,
    /// Host-owned handle for `verter_semantic::db::SemanticDb`.
    /// **Different crate, different artifact.** This is NOT a typed-DB
    /// wrapper around the project-global graph; it is the
    /// orthogonal query-memo DB serving the semantic surfaces /
    /// bindings / reactivity provenance layer. The handle lives on
    /// `ProjectTypeStore` so the unified `bump_project_generation_and_evict`
    /// cascade can reset it alongside the typed DBs.
    semantic_db: parking_lot::Mutex<verter_semantic::db::SemanticDb>,
    /// Resolve-domain authoritative cache for resolved import / re-export
    /// bindings + per-specifier resolutions. Keyed
    /// `(canonical, content_hash, parse_env_hash, resolve_env_hash,
    /// resolver_version)`; intentionally excludes `lib_env_hash` (R21
    /// scoping rule — base import-target resolution does not depend on
    /// TS lib data). See
    /// [`crate::resolved_import_facts::ResolvedImportFactsDb`].
    resolved_import_facts: Arc<crate::resolved_import_facts::ResolvedImportFactsDb>,
    /// R28 — lazy `Member.semantic_hash` store.
    /// Keyed on `parse_stable_hash` so cosmetic edits preserve the entry.
    /// See [`crate::member_semantic_fact_store::MemberSemanticFactStore`].
    member_semantic_facts: crate::member_semantic_fact_store::MemberSemanticFactStore,
    /// R28 — lazy `Member.display_hash` store.
    /// Keyed on `content_hash` so cosmetic edits recompute display facts only.
    /// See [`crate::member_display_fact_store::MemberDisplayFactStore`].
    member_display_facts: crate::member_display_fact_store::MemberDisplayFactStore,
    /// Host-owned mapped-binder ordinal registry. The
    /// registry hands out STABLE `param_index` ordinals for each
    /// `(canonical, display_name, fingerprint)` triple so two
    /// lowerings of the SAME source mapper produce the SAME
    /// `TypeParam` SemanticNodeId — and therefore the SAME
    /// `MapperKey` cache key for
    /// `SemanticQueryKey::MappedType`. Replaces the legacy
    /// per-dispatcher counter that destabilised mapper identity
    /// across dispatcher instances. See
    /// [`crate::mapper_binder_registry`].
    mapper_binder_registry: Arc<crate::mapper_binder_registry::MapperBinderRegistry>,
    /// Debug / diagnostic counters.
    pub counters: ProjectTypeStoreCounters,
}

impl std::fmt::Debug for ProjectTypeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectTypeStore")
            .field(
                "project_generation",
                &self.project_generation.load(Ordering::Relaxed),
            )
            .field("indexed_entries", &self.indexed.len())
            .field("analysis_entries", &self.analysis.len())
            .field("counters", &self.counters.snapshot())
            .finish_non_exhaustive()
    }
}

impl ProjectTypeStore {
    #[must_use]
    pub fn new() -> Self {
        Self::build(None)
    }

    /// Construct a store wired to the host's
    /// [`MetaProvenance`](crate::types::MetaProvenance) so the embedded
    /// [`SemanticGraphStore`] reports its contention instrumentation
    /// counters through the shared provenance surface. Test-only
    /// `ProjectTypeStore::new()` callers stay uninstrumented
    /// (semantic-graph stats remain visible through their own
    /// `stats_snapshot` surface).
    #[must_use]
    pub fn with_provenance(provenance: Arc<crate::types::MetaProvenance>) -> Self {
        Self::build(Some(provenance))
    }

    fn build(provenance: Option<Arc<crate::types::MetaProvenance>>) -> Self {
        let counters = ProjectTypeStoreCounters::default();
        // Each backing DB holds the same `Arc<AtomicU64>` counters as
        // `counters` so the `snapshot()` method sees in-place updates.
        let indexed = FileArtifactStore::with_counters(
            Arc::clone(&counters.indexed_live),
            Arc::clone(&counters.indexed_stale_sweeps),
        );
        let analysis = AnalysisReadyDb::with_counters(
            Arc::clone(&counters.analysis_live),
            Arc::clone(&counters.analysis_stale_sweeps),
        );
        let owner_import_surfaces =
            OwnerImportSurfaceDb::with_counter(Arc::clone(&counters.owner_import_live));
        let component_meta_results = ComponentMetaResultDb::with_counters(
            Arc::clone(&counters.component_meta_live),
            Arc::clone(&counters.component_meta_stale_sweeps),
        );
        let semantic_graph = match provenance {
            Some(prov) => Arc::new(SemanticGraphStore::with_provenance(prov)),
            None => Arc::new(SemanticGraphStore::new()),
        };
        let imported_registry_db =
            ImportedRegistryDb::with_counter(Arc::clone(&counters.component_meta_cache_live));
        let declaration_lookup_db =
            DeclarationLookupDb::with_counter(Arc::clone(&counters.component_meta_cache_live));
        let resolvability_db =
            ResolvabilityDb::with_counter(Arc::clone(&counters.component_meta_cache_live));
        let owner_collection_db =
            OwnerCollectionDb::with_counter(Arc::clone(&counters.component_meta_cache_live));
        let shape_cache_db =
            ShapeCacheDb::with_counter(Arc::clone(&counters.component_meta_cache_live));
        let materialize_structure_db =
            crate::component_meta_caches::MaterializeStructureDb::with_counter(Arc::clone(
                &counters.component_meta_cache_live,
            ));
        let ref_cycle_db = crate::component_meta_caches::RefCycleResultDb::with_counter(
            Arc::clone(&counters.component_meta_cache_live),
        );
        let app_config_no_override_proof =
            crate::app_config_proof_db::AppConfigNoOverrideProofDb::with_counter(Arc::clone(
                &counters.component_meta_cache_live,
            ));
        Self {
            project_generation: AtomicU64::new(0),
            indexed,
            analysis,
            routes: Arc::new(RouteDb::new()),
            imported_roots: Arc::new(ImportedRootDb::new()),
            semantic_graph,
            owner_import_surfaces,
            component_meta_results,
            intrinsic_registry: IntrinsicRegistry::with_defaults(),
            imported_registry_db,
            declaration_lookup_db,
            resolvability_db,
            owner_collection_db,
            shape_cache_db,
            materialize_structure_db,
            ref_cycle_db,
            app_config_no_override_proof,
            type_resolution_context_db: TypeResolutionContextDb::new(),
            compile_cache_db: CompileCacheDb::new(),
            compile_output_pure_content: crate::cache_runtime::CompileOutputNodePureContent::new(),
            derived_raw_cache_db: DerivedRawCacheDb::new(),
            dependency_cache_db: DependencyCacheDb::new(),
            resolved_type_cache_db: ResolvedTypeCacheDb::new(),
            semantic_db: parking_lot::Mutex::new(verter_semantic::db::SemanticDb::new()),
            resolved_import_facts: Arc::new(
                crate::resolved_import_facts::ResolvedImportFactsDb::new(),
            ),
            member_semantic_facts: crate::member_semantic_fact_store::MemberSemanticFactStore::new(
            ),
            member_display_facts: crate::member_display_fact_store::MemberDisplayFactStore::new(),
            mapper_binder_registry: Arc::new(
                crate::mapper_binder_registry::MapperBinderRegistry::new(),
            ),
            counters,
        }
    }

    /// Current monotonic project generation. Owned by the host / workspace
    /// layer — queries read it but never mutate it.
    pub fn project_generation(&self) -> u64 {
        self.project_generation.load(Ordering::Acquire)
    }

    /// Alias — equivalent to [`Self::project_generation`] with
    /// a clearer name for the route-only shallow materialiser's tier-3
    /// staleness gate (sub-). The materialiser captures
    /// this value before reading + parsing, then re-checks it inside the
    /// pre-publish fence to detect mid-flight `bump_project_generation`
    /// mutations (`configure_projects`, `set_exact_resolutions`,
    /// `configure_resolver`).
    #[must_use]
    pub fn current_project_generation(&self) -> u64 {
        self.project_generation()
    }

    /// Bump the project generation. Invoked exclusively by the host /
    /// workspace layer on `tsconfig`, path-alias, active-TS-SDK,
    /// workspace-folder, package export-target, and explicit project-graph
    /// changes — never on file-content edits.
    pub fn bump_project_generation(&self) -> u64 {
        self.project_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn indexed(&self) -> &FileArtifactStore {
        &self.indexed
    }

    /// Canonical accessor for the post-parse file artifact cache.
    ///
    /// Returns the same `FileArtifactStore` that the legacy [`Self::indexed`]
    /// accessor returns; new code should prefer this name. The two are
    /// kept in step until [`Self::indexed`] is retired.
    pub fn file_artifacts(&self) -> &FileArtifactStore {
        &self.indexed
    }

    pub fn analysis(&self) -> &AnalysisReadyDb {
        &self.analysis
    }

    pub fn routes(&self) -> &Arc<RouteDb> {
        &self.routes
    }

    pub fn imported_roots(&self) -> &Arc<ImportedRootDb> {
        &self.imported_roots
    }

    /// Return a cloned `Arc<RouteDb>` handle
    /// that callers can store and use as a stable, shared reference.
    /// Identical authority to `routes()` but returns a clonable owned
    /// `Arc` so the [`UnifiedResolverRuntime`](crate::resolver_core::resolver_runtime::UnifiedResolverRuntime)
    /// can hold a handle pinned to the same project-store-owned instance.
    /// Successive calls always return Arcs that
    /// [`Arc::ptr_eq`](std::sync::Arc::ptr_eq) the inner instance.
    #[must_use]
    pub fn routes_handle(&self) -> Arc<RouteDb> {
        Arc::clone(&self.routes)
    }

    /// Return a cloned `Arc<ImportedRootDb>` handle. See
    /// [`Self::routes_handle`] for the full rationale.
    #[must_use]
    pub fn imported_roots_handle(&self) -> Arc<ImportedRootDb> {
        Arc::clone(&self.imported_roots)
    }

    // ──────────────────────────────────────────────────────────────────
    // Typed-DB accessors
    // ──────────────────────────────────────────────────────────────────

    /// Typed DB for [`crate::owned_artifacts::OwnedTypeResolutionContext`].
    /// Currently populated only by tests; no production lowering path
    /// writes owned contexts here yet.
    pub fn type_resolution_context_cache(&self) -> &TypeResolutionContextDb {
        &self.type_resolution_context_db
    }

    /// Profile-domain DB for the per-canonical compile cache (D48).
    /// Stores [`crate::types::ProfileState`] entries; profile-flag
    /// changes invalidate, source-content changes preserve.
    pub fn compile_cache(&self) -> &CompileCacheDb {
        &self.compile_cache_db
    }

    /// Content-addressed compile-output cache node for
    /// [`crate::types::CompileCacheMode::Content`] requests.
    pub(crate) fn compile_output_pure_content(
        &self,
    ) -> &crate::cache_runtime::CompileOutputNodePureContent {
        &self.compile_output_pure_content
    }

    /// Source-content-domain DB for the per-canonical compile cache
    /// (D48). Stores [`crate::types::DerivedRawState`] entries;
    /// source-content changes invalidate, profile-flag changes preserve.
    pub fn derived_raw_cache(&self) -> &DerivedRawCacheDb {
        &self.derived_raw_cache_db
    }

    /// Dependency-closure-domain DB for the per-canonical compile cache
    /// (D48). Stores [`crate::types::DependencyState`] entries;
    /// dep-closure / source-content changes invalidate, profile-flag
    /// changes preserve.
    pub fn dependency_cache(&self) -> &DependencyCacheDb {
        &self.dependency_cache_db
    }

    /// Typed DB for resolved-type entries — the shared external-type
    /// cache consumed by `host_resolve::external_type_resolution`.
    pub fn resolved_type_cache(&self) -> &ResolvedTypeCacheDb {
        &self.resolved_type_cache_db
    }

    /// Handle to the host-owned
    /// [`verter_semantic::db::SemanticDb`] (different crate, different
    /// artifact type than [`Self::semantic_graph`]). Returns the
    /// `MutexGuard` so every call site locks through this one
    /// accessor.
    pub fn semantic_db(&self) -> parking_lot::MutexGuard<'_, verter_semantic::db::SemanticDb> {
        self.semantic_db.lock()
    }

    /// Host-owned semantic-query memo table. Shared across every consumer
    /// that dispatches through the semantic-query API.
    pub fn semantic_graph(&self) -> &Arc<SemanticGraphStore> {
        &self.semantic_graph
    }

    /// Owner direct-import surface cache. Direct owner imports
    /// resolve exactly once per owner version through this cache.
    pub fn owner_import_surfaces(&self) -> &OwnerImportSurfaceDb {
        &self.owner_import_surfaces
    }

    /// Final component-meta result cache.
    pub fn component_meta_results(
        &self,
    ) -> &ComponentMetaResultDb<crate::component_meta_result_db::CachedComponentMetaResult> {
        &self.component_meta_results
    }

    /// TypeScript `intrinsic` registry. Read-only from the
    /// resolver hot path; the host may re-register entries at boot when
    /// the active TS SDK is swapped.
    pub fn intrinsic_registry(&self) -> &IntrinsicRegistry {
        &self.intrinsic_registry
    }

    // ----- Step 3 closure: 10 typed DB accessors -----

    pub fn imported_registry_db(&self) -> &ImportedRegistryDb {
        &self.imported_registry_db
    }

    pub fn declaration_db(&self) -> &DeclarationLookupDb {
        &self.declaration_lookup_db
    }

    pub fn resolvable_db(&self) -> &ResolvabilityDb {
        &self.resolvability_db
    }

    pub fn owner_collection_db(&self) -> &OwnerCollectionDb {
        &self.owner_collection_db
    }

    /// Universal shape cache. Replaces
    /// `MaterializeMemoDb` + `MemberShapeCacheDb`. The `ShapeSubject`
    /// enum on the key discriminates TypeExpr-start vs
    /// SemanticNode-start callers.
    /// See [`crate::component_meta_caches::ShapeCacheDb`].
    pub fn shape_cache_db(&self) -> &ShapeCacheDb {
        &self.shape_cache_db
    }

    /// For the structural-materialiser
    /// final-result cache. Sole authoritative materialiser cache; the
    /// canonical removed-symbol list lives in
    /// `tests/no_legacy_walker.rs::RETIRED_SYMBOLS`.
    pub fn materialize_structure_db(
        &self,
    ) -> &crate::component_meta_caches::MaterializeStructureDb {
        &self.materialize_structure_db
    }

    /// C — accessor for the host-owned
    /// transitive-cycle BFS cache consulted by
    /// `meta_resolve::ref_root_reaches_transitive_cycle_node`.
    pub fn ref_cycle_db(&self) -> &crate::component_meta_caches::RefCycleResultDb {
        &self.ref_cycle_db
    }

    /// Resolve-domain authoritative store for resolved import /
    /// re-export bindings + per-specifier resolutions. Keyed by
    /// `(canonical, content_hash, parse_env_hash, resolve_env_hash,
    /// resolver_version)` — see
    /// [`crate::resolved_import_facts::ResolvedImportFactsKey`].
    /// `lib_env_hash` is intentionally absent (R21 scoping rule).
    pub fn resolved_import_facts(&self) -> &crate::resolved_import_facts::ResolvedImportFactsDb {
        &self.resolved_import_facts
    }

    /// Cloned `Arc` handle for the resolved-import facts cache.
    /// Mirrors [`Self::routes_handle`] — returns a clonable shared
    /// reference so call sites that capture a long-lived snapshot
    /// (e.g. `HostStoreView::build`) can hold an owned handle pinned
    /// to the same store-owned instance. Successive calls return
    /// `Arc`s that [`Arc::ptr_eq`] the inner instance.
    #[must_use]
    pub fn resolved_import_facts_handle(
        &self,
    ) -> &Arc<crate::resolved_import_facts::ResolvedImportFactsDb> {
        &self.resolved_import_facts
    }

    /// R28 — lazy `Member.semantic_hash` store keyed on
    /// `parse_stable_hash` so cosmetic edits preserve the entry. See
    /// [`crate::member_semantic_fact_store::MemberSemanticFactStore`]
    /// for the producer contract.
    pub fn member_semantic_fact_store(
        &self,
    ) -> &crate::member_semantic_fact_store::MemberSemanticFactStore {
        &self.member_semantic_facts
    }

    /// R28 — lazy `Member.display_hash` store keyed on `content_hash`
    /// so cosmetic edits recompute display facts only. See
    /// [`crate::member_display_fact_store::MemberDisplayFactStore`]
    /// for the producer contract.
    pub fn member_display_fact_store(
        &self,
    ) -> &crate::member_display_fact_store::MemberDisplayFactStore {
        &self.member_display_facts
    }

    /// Host-owned mapped-binder ordinal registry. Hands
    /// out STABLE `param_index` ordinals for each `(canonical,
    /// display_name, fingerprint)` triple so two lowerings of the
    /// SAME source mapper produce the SAME `TypeParam`
    /// SemanticNodeId — and therefore the SAME `MapperKey` cache
    /// key for `SemanticQueryKey::MappedType`. See
    /// [`crate::mapper_binder_registry`].
    pub(crate) fn mapper_binder_registry(
        &self,
    ) -> &Arc<crate::mapper_binder_registry::MapperBinderRegistry> {
        &self.mapper_binder_registry
    }

    /// Issue #6 / accessor for the `AppConfigNoOverrideProof`
    /// cache consulted by the ComponentConfig theme variant fast path.
    /// On miss, the fast path declines and the slow path runs.
    pub fn app_config_no_override_proof_db(
        &self,
    ) -> &crate::app_config_proof_db::AppConfigNoOverrideProofDb {
        &self.app_config_no_override_proof
    }

    /// Build a `(project_generation, whole_hash)` dep-signature pair that
    /// downstream callers fold into their dependency-fact set for the
    /// publish-side completion-fence revalidation.
    #[must_use]
    pub fn dep_version_for(&self, whole_hash: Hash16) -> DepVersion {
        // Callers merge file-version facts as `DepVersion::WholeHash` and add
        // the project-generation fact separately. This helper keeps the
        // returned version self-contained so tests have a stable variant to
        // assert against.
        let _ = self.project_generation();
        DepVersion::WholeHash(whole_hash)
    }

    /// Targeted invalidation on file content / routing change.
    ///
    /// Called from the host's `evict_canonical` flow. Removes or
    /// invalidates every entry in the project-global cache graph that
    /// pertains to `canonical_id`:
    /// - `FileArtifactStore`: removes the entry (lookup would otherwise
    ///   harmlessly miss via whole-hash mismatch, but the memory would
    ///   leak until re-materialization overwrote it).
    /// - `AnalysisReadyDb`: removes every `(hash, scope)` entry for the
    ///   canonical.
    /// - `OwnerImportSurfaceDb`: removes the owner surface.
    /// - `ComponentMetaResultDb`: removes every result keyed on the owner.
    /// - `SemanticGraphStore`: removes every memo entry whose scope
    ///   references the canonical, and every Vue macro resolution entry
    ///   keyed on the canonical.
    pub fn evict_canonical(&self, canonical_id: &str) {
        self.indexed.remove(canonical_id);
        self.analysis.invalidate_canonical(canonical_id);
        self.owner_import_surfaces.remove(canonical_id);
        self.component_meta_results.invalidate_owner(canonical_id);
        self.semantic_graph.invalidate_canonical(canonical_id);
        self.semantic_graph
            .invalidate_resolved_named_types_for_canonical(canonical_id);
        // Step 3 closure: invalidate every host-owned engine cache that
        // keys on `canonical_id` so a content edit on the file invalidates
        // its own resolved declarations / projections / materializations.
        self.imported_registry_db.invalidate_canonical(canonical_id);
        self.declaration_lookup_db
            .invalidate_canonical(canonical_id);
        self.resolvability_db.invalidate_canonical(canonical_id);
        self.owner_collection_db.invalidate_canonical(canonical_id);
        // The universal shape cache joins the per-canonical
        // eviction cascade. Replaces the previously-split
        // `materialize_memo_db` + `member_shape_cache_db`.
        self.shape_cache_db.invalidate_canonical(canonical_id);
        // Reverse-index drain on the
        // structural-materialiser cache (sole materialiser cache).
        self.materialize_structure_db
            .invalidate_for_canonical(canonical_id);
        // R — same per-canonical reverse-index drain
        // for the BFS cycle-result cache.
        self.ref_cycle_db.invalidate_for_canonical(canonical_id);
        // Issue #6 / drop any AppConfigNoOverrideProof entry
        // whose dep_signature references this canonical or whose
        // app_config_decl_canonical_id IS this canonical.
        self.app_config_no_override_proof
            .invalidate_canonical(canonical_id);
        // Unified cascade — F2 (resolved_type_cache) participates per
        // the rehoming-doc §3.3 contract: per-canonical content edits
        // drain entries whose `dep_canonical_id` references the same
        // canonical. Pre-rehoming the off-store cache only had
        // clear-all-at-cap; this method is the per-canonical drain.
        self.resolved_type_cache_db.evict_canonical(canonical_id);
        // Unified cascade — F5 (semantic_db) participates per the
        // rehoming-doc §3.3 contract: a per-canonical content edit
        // invalidates the semantic-fact cache for the same canonical
        // so subsequent semantic queries observe an unavailable cache
        // and recompute against fresh facts.
        self.semantic_db.lock().invalidate(canonical_id);
        // R28 — the two-fact `MemberPresence`/`Member` model splits
        // body fingerprints across semantic vs display lanes. A
        // per-canonical content edit drops both lanes' entries for
        // the canonical so subsequent member-body fingerprints emit
        // from fresh source.
        self.member_semantic_facts
            .invalidate_canonical(canonical_id);
        self.member_display_facts.invalidate_canonical(canonical_id);
        // Drop the per-canonical mapper-binder
        // registry slot. The next lowering of any mapper in this
        // file starts with a fresh `Arc::as_ptr` keyspace so a
        // pointer reuse across the content edit cannot collide
        // with a stale fingerprint. See
        // [`crate::mapper_binder_registry`] for the registry
        // contract.
        self.mapper_binder_registry
            .clear_for_canonical(canonical_id);
        // D48 split: the per-domain compile-cache entries
        // (CompileCacheDb / DerivedRawCacheDb / DependencyCacheDb) are
        // NOT dropped here. The matrix routes the "source content
        // change for owner" trigger through
        // [`Self::evict_for_source_content_change`] (called from the
        // host-level upsert flow before per-domain re-population).
        // `evict_canonical` is the project-type-store-level cascade
        // for the project-global graph (IndexedReady, analysis, owner
        // surfaces, etc.) and stays orthogonal to the per-canonical
        // compile-cache caches.
    }

    /// Per-canonical eviction for a ROUTE-RESOLUTION mutation whose
    /// content identity is unchanged (`set_exact_resolutions`): the full
    /// [`Self::evict_canonical`] cascade EXCEPT the `FileArtifactStore`
    /// removal. The content-addressed `IndexedReady` payload stays
    /// retained so the next read refreshes only its route surface
    /// through the project-stamp gate (edge refresh — no re-read, no
    /// re-parse); every derived/query-identity layer for the canonical
    /// still drains.
    pub fn evict_canonical_for_route_mutation(&self, canonical_id: &str) {
        self.analysis.invalidate_canonical(canonical_id);
        self.owner_import_surfaces.remove(canonical_id);
        self.component_meta_results.invalidate_owner(canonical_id);
        self.semantic_graph.invalidate_canonical(canonical_id);
        self.semantic_graph
            .invalidate_resolved_named_types_for_canonical(canonical_id);
        self.imported_registry_db.invalidate_canonical(canonical_id);
        self.declaration_lookup_db
            .invalidate_canonical(canonical_id);
        self.resolvability_db.invalidate_canonical(canonical_id);
        self.owner_collection_db.invalidate_canonical(canonical_id);
        self.shape_cache_db.invalidate_canonical(canonical_id);
        self.materialize_structure_db
            .invalidate_for_canonical(canonical_id);
        self.ref_cycle_db.invalidate_for_canonical(canonical_id);
        self.app_config_no_override_proof
            .invalidate_canonical(canonical_id);
        self.resolved_type_cache_db.evict_canonical(canonical_id);
        self.semantic_db.lock().invalidate(canonical_id);
        self.member_semantic_facts
            .invalidate_canonical(canonical_id);
        self.member_display_facts.invalidate_canonical(canonical_id);
        self.mapper_binder_registry
            .clear_for_canonical(canonical_id);
    }

    /// Live-content reachability sweep on [`FileArtifactStore`].
    ///
    /// R22 contract: the reverse import graph drives reachability GC +
    /// LSP affected-files reporting + diagnostics, but is **never**
    /// wired to cache invalidation. Any cached `FileArtifacts` entry
    /// whose `(canonical_id, content_hash)` pair is not present in
    /// `live_publish_set` is dropped. The publish set is the union of
    /// every `(canonical_id, content_hash)` reachable from any open
    /// editor / live VFS state — callers compute it from their VFS
    /// snapshot and pass it in.
    ///
    /// Per D40 + D119: when `memory_pressure: true`, an additional
    /// LRU floor sweep runs after reachability and drops entries down
    /// to `min_floor` by oldest-access order. The default
    /// [`crate::types::EvictionPolicyConfig`]
    /// has `memory_pressure_threshold == usize::MAX`, so callers
    /// derived from `HostConfig::eviction_policy` never enter this
    /// branch in default builds.
    ///
    /// The sweep operates on the unified [`FileArtifactStore`], which
    /// holds `IndexedReady`, `FileFacts`, `ParsedEdges`, and
    /// augmentations under one key — hence the broader name reflects
    /// what reachability GC actually drops.
    pub fn evict_unreachable_artifacts(
        &self,
        live_publish_set: &rustc_hash::FxHashSet<(Arc<str>, Hash16)>,
        memory_pressure: bool,
        min_floor: usize,
    ) {
        // The legacy 3-arg API delegates to the policy-aware variant
        // with policy defaults so existing callers retain their
        // behaviour unchanged. Policy-aware callers thread an explicit
        // `EvictionPolicyConfig` through `evict_unreachable_artifacts_with_policy`.
        let policy = crate::types::EvictionPolicyConfig {
            memory_pressure_threshold: usize::MAX,
            min_floor,
            ..crate::types::EvictionPolicyConfig::default()
        };
        self.evict_unreachable_artifacts_with_policy(live_publish_set, memory_pressure, &policy);
    }

    /// Policy-aware reachability + LRU floor + per-canonical
    /// retention sweep. Consumes the full
    /// [`crate::types::EvictionPolicyConfig`] so callers can opt in
    /// to per-canonical retention + promotion-aware LRU eviction.
    ///
    /// Pass `memory_pressure: true` to trigger the LRU floor +
    /// per-canonical retention sweep; with `false` only the
    /// reachability sweep runs (R22 memory-bound default).
    pub fn evict_unreachable_artifacts_with_policy(
        &self,
        live_publish_set: &rustc_hash::FxHashSet<(Arc<str>, Hash16)>,
        memory_pressure: bool,
        policy: &crate::types::EvictionPolicyConfig,
    ) {
        // D33: live-content reachability first. Each full
        // [`FileArtifactKey`] is checked against the live set by its
        // `(canonical, content_hash)` projection; only the unreachable
        // version is dropped (other versions of the same canonical
        // survive — the broader `remove(canonical)` API would drop
        // every version of a canonical even when only one version was
        // unreachable, which the version-specific evict avoids).
        for key in self.indexed.artifact_keys() {
            let projection = (Arc::clone(&key.canonical), key.content_hash);
            if !live_publish_set.contains(&projection) {
                let _ = self.indexed.remove_artifacts(&key);
            }
        }
        // Per-canonical retention runs on every sweep so
        // long-lived sessions don't accumulate unbounded variants.
        // The default retention (3) covers the {current, previous,
        // baseline} window; `usize::MAX` disables the cap.
        self.indexed
            .enforce_per_canonical_retention(policy.per_canonical_content_hash_retention);
        // D40 + D119: LRU floor only under explicit memory pressure.
        // memory_pressure_threshold = usize::MAX by default — never
        // triggered. The capability is preserved for production
        // callers that opt in.
        if memory_pressure {
            self.indexed
                .evict_lru_promoted(policy.min_floor, policy.promote_threshold);
        }
    }
    /// D48 matrix row 1 — Source content change for owner.
    ///
    /// Drops the source-content-domain (`DerivedRawCacheDb`) and
    /// dep-closure-domain (`DependencyCacheDb`) entries for `canonical_id`
    /// while PRESERVING the profile-domain (`CompileCacheDb`) entry.
    /// Called from the host-level upsert flow when source content
    /// changes; the upsert then re-populates the dropped entries with
    /// freshly-computed state.
    ///
    /// This is the per-domain analogue to `evict_canonical`: the
    /// project-global graph evictions go through `evict_canonical`,
    /// the per-canonical compile-cache evictions go through this method.
    pub fn evict_for_source_content_change(&self, canonical_id: &str) {
        self.derived_raw_cache_db.entries().remove(canonical_id);
        self.dependency_cache_db.entries().remove(canonical_id);
    }

    /// AUTHORITY-RESET bump: project-generation bump plus a wholesale
    /// wipe of every cache layer whose identity depends on project
    /// configuration — INCLUDING the per-canonical compile / derived /
    /// dependency payloads.
    ///
    /// Reserved for content-authority swaps and full teardowns
    /// (`set_workspace`, `close`): the wide per-canonical clears orphan
    /// retained state against an authority that no longer exists.
    /// Route-resolution mutations (`set_exact_resolutions`,
    /// `configure_projects`, `set_import_dependencies`) MUST NOT call
    /// this — they use the stamp-only `bump_project_generation` (stale
    /// entries miss by validation; route surfaces edge-refresh on
    /// demand) plus owner-scoped route-mirror repair. Wholesale-clearing
    /// `derived_raw_cache_db` for a route mutation flips every
    /// scheduler-tracked canonical's derived state away while its
    /// scheduler source lives on — the exact accreted-patchwork seam the
    /// stamp-only model removes.
    pub fn bump_project_generation_and_evict(&self) -> u64 {
        let generation = self.bump_project_generation();
        // File-content identity stays (IndexedReady / AnalysisReady keyed
        // by whole_hash are still correct for the same file content).
        // What becomes stale is:
        //   - route / barrel facts (config changes may shift resolution)
        //   - owner import surfaces (routes they resolved may shift)
        //   - component-meta results (depend on routes / intrinsic SDK)
        //   - semantic query memo (derived from routes + intrinsics)
        //   - Vue macro resolution artifacts (route-sensitive cross-file
        //     resolution: a tsconfig change can redirect the same name to
        //     a different target file)
        self.owner_import_surfaces
            .entries_drain_for_generation_bump();
        // `invalidate_all` drops every `SemanticNodeId`-keyed structure
        // on the graph store — including the Vue macro resolved-named-type
        // identity map — and aborts every in-flight macro-resolution
        // build. It also advances the resolved-named-type reset epoch via
        // `BudgetedNamedTypeIndex::clear_and_bump_generation`, which bumps
        // the epoch INSIDE the same `retention_gate.write()` critical
        // section that clears the identity map and its budget — bump and
        // clear are one atomic step. A macro-resolution build aborted by
        // this call keeps running until its next abort check, but its
        // `HostNamedTypeCacheAdapter` snapshotted the pre-bump epoch, so
        // any straggler `insert_resolved_named_type` it performs — however
        // long after this bump — is rejected by the epoch fence: that
        // fence re-reads and compares the epoch UNDER the same
        // `retention_gate.read()` guard that performs the map insert, so a
        // straggler is fully ordered against the clear+bump with no
        // window. No post-`invalidate_all` re-sweep is needed: the fence
        // prevents the stale insert from ever landing. The node arena
        // itself is append-only and is not reset.
        let _ = self.semantic_graph.invalidate_all();
        self.component_meta_results.invalidate_all();
        // Step 3 closure: project-shape change invalidates every engine
        // cache (entries depend on the same routes / intrinsics that
        // change at the project-generation boundary).
        self.imported_registry_db.invalidate_all();
        self.declaration_lookup_db.invalidate_all();
        self.resolvability_db.invalidate_all();
        self.owner_collection_db.invalidate_all();
        // The universal shape cache joins the project-generation
        // invalidation cascade. Replaces `materialize_memo_db` +
        // `member_shape_cache_db`.
        self.shape_cache_db.invalidate_all();
        self.materialize_structure_db.invalidate_all();
        // R — project-shape change invalidates the
        // BFS cycle-result cache (entries depend on the same routes /
        // intrinsics that change at the project-generation boundary).
        self.ref_cycle_db.invalidate_all();
        // Issue #6 / project-shape change invalidates every
        // proof entry; the proof's dep signature includes routes and
        // workspace-level interface-merging state.
        self.app_config_no_override_proof.invalidate_all();
        // Rehomed off-store caches join the unified project-generation
        // cascade (host-cache-rehoming.md §3.4). Routes / intrinsics
        // drive each of these caches' freshness, so a tsconfig-style
        // bump invalidates them along with the typed DBs above.
        //
        // D48 invalidation matrix — `bump_project_generation_and_evict`
        // row: ALL THREE per-canonical compile-cache sub-domains drop
        // (ProfileState + DerivedRawState + DependencyState) because a
        // project-shape change can shift profile flags AND content
        // routing AND dep closures simultaneously.
        self.compile_cache_db.clear();
        self.derived_raw_cache_db.clear();
        self.dependency_cache_db.clear();
        self.resolved_type_cache_db.clear();
        *self.semantic_db.lock() = verter_semantic::db::SemanticDb::new();
        generation
    }
}

impl Default for ProjectTypeStore {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// typed cache invalidation domain registration.
//
// `PROJECT_TYPE_STORE_DB_INVENTORY` and `ProjectTypeStore::all_dbs_for_invalidation`
// are the single source of truth for which DBs participate in the
// invalidation cascade. Adding a new DB-typed field on
// `ProjectTypeStore` requires three coordinated edits:
//
//   1. The struct field declaration above.
//   2. The constructor in `Self::build`.
//   3. ONE line in `PROJECT_TYPE_STORE_DB_INVENTORY` AND ONE line in
//      `all_dbs_for_invalidation`'s vector.
//
// The architecture guard
// `every_db_field_in_project_type_store_appears_in_inventory`
// (in `crates/verter_session/tests/architecture_guards.rs`) walks the
// struct via `syn::parse_file` and asserts every DB-typed field name
// appears in the inventory. Adding a field outside the inventory fails
// the guard.
// ──────────────────────────────────────────────────────────────────────────

pub const PROJECT_TYPE_STORE_DB_INVENTORY: &[&str] = &[
    "indexed",
    "analysis",
    "routes",
    "imported_roots",
    "semantic_graph",
    "owner_import_surfaces",
    "component_meta_results",
    "intrinsic_registry",
    "imported_registry_db",
    "declaration_lookup_db",
    "resolvability_db",
    "owner_collection_db",
    "shape_cache_db",
    "materialize_structure_db",
    "ref_cycle_db",
    "app_config_no_override_proof",
    "type_resolution_context_db",
    "compile_cache_db",
    // D48 split: source-content-domain and dep-closure-domain siblings
    // of `compile_cache_db`. Each fans into the unified
    // `bump_project_generation_and_evict` cascade.
    "derived_raw_cache_db",
    "dependency_cache_db",
    "resolved_type_cache_db",
    // The `semantic_db: Mutex<verter_semantic::db::SemanticDb>`
    // handle is intentionally absent from this inventory — it is the
    // *handle* sitting inside `ProjectTypeStore`, not a typed-DB wrapper
    // that implements `ParticipatesInInvalidation`. The unified
    // `bump_project_generation_and_evict` resets it directly via
    // `*self.semantic_db.lock() = SemanticDb::new()` (host-cache-rehoming.md
    // §3.4 F5).
];

impl ProjectTypeStore {
    pub fn all_dbs_for_invalidation(
        &self,
    ) -> Vec<&dyn crate::invalidation_domain::ParticipatesInInvalidation> {
        vec![
            &self.indexed,
            &self.analysis,
            &*self.routes,
            &*self.imported_roots,
            &*self.semantic_graph,
            &self.owner_import_surfaces,
            &self.component_meta_results,
            &self.intrinsic_registry,
            &self.imported_registry_db,
            &self.declaration_lookup_db,
            &self.resolvability_db,
            &self.owner_collection_db,
            &self.shape_cache_db,
            &self.materialize_structure_db,
            &self.ref_cycle_db,
            &self.app_config_no_override_proof,
            &self.type_resolution_context_db,
            &self.compile_cache_db,
            // Source-content-domain and dep-closure-domain siblings of
            // `compile_cache_db`.
            &self.derived_raw_cache_db,
            &self.dependency_cache_db,
            &self.resolved_type_cache_db,
        ]
    }

    /// Monomorphic statically-dispatched
    /// per-canonical eviction cascade across every DB on the store.
    ///
    /// Each call site below invokes the per-DB
    /// [`InvalidationByCanonical::invalidate_canonical_for`] impl
    /// directly (no `dyn` dispatch, no virtual call). The cascade
    /// returns the total number of entries dropped across all DBs.
    ///
    /// Called by the host's per-file-content-change invalidation path
    /// (one call per canonical id whose `whole_hash` shifts). The
    /// per-DB implementations route through their own secondary
    /// indices (see [`crate::invalidation_domain::CanonicalReverseIndex`])
    /// for O(K) drain, where K = entries owned by the canonical.
    ///
    /// `FileArtifactStore` and `ComponentMetaResultDb` are not on the
    /// canonical-by-canonical path: `FileArtifactStore` keys directly on
    /// `Arc<str>` and exposes `remove(canonical_id)`,
    /// `ComponentMetaResultDb` keys on owner-canonical and exposes
    /// `invalidate_owner(owner_canonical)`. Both are invoked here so
    /// the cascade covers the inventory uniformly.
    pub fn invalidate_canonical_across_all_dbs(&self, canonical_id: &str) -> usize {
        use crate::invalidation_domain::InvalidationByCanonical;

        // Statically-dispatched InvalidationByCanonical calls — one
        // per registered DB-typed field on `ProjectTypeStore`. No
        // `dyn` dispatch; the compiler monomorphises each call. The
        // call order matches `PROJECT_TYPE_STORE_DB_INVENTORY` and
        // `all_dbs_for_invalidation()`.
        let mut total: usize = 0;
        total = total.saturating_add(self.indexed.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(self.analysis.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(self.routes.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(self.imported_roots.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(self.semantic_graph.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(
            self.owner_import_surfaces
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.component_meta_results
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.intrinsic_registry
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.imported_registry_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.declaration_lookup_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(self.resolvability_db.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(
            self.owner_collection_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(self.shape_cache_db.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(
            self.materialize_structure_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(self.ref_cycle_db.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(
            self.app_config_no_override_proof
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.type_resolution_context_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(self.compile_cache_db.invalidate_canonical_for(canonical_id));
        total = total.saturating_add(
            self.derived_raw_cache_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.dependency_cache_db
                .invalidate_canonical_for(canonical_id),
        );
        total = total.saturating_add(
            self.resolved_type_cache_db
                .invalidate_canonical_for(canonical_id),
        );
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a minimal `ComponentMetaAnalysis` for tests that only care
    /// about cache-key behaviour. Keeps the test surface decoupled from the
    /// real resolver's analysis output.
    fn empty_component_meta_analysis(
    ) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        use verter_semantic::analysis::component_meta::{
            AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags,
            FallthroughSurface, NoFallthroughReason, RootReachability,
        };
        ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: ComponentMetaFlags::default(),
            root_reachability: RootReachability::NoFallthrough {
                reason: NoFallthroughReason::NoTemplate,
            },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: FallthroughSurface::None {
                reason: NoFallthroughReason::NoTemplate,
            },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: String::new(),
        }
    }

    #[test]
    fn artifact_requirements_bitflags_round_trip() {
        let both = ArtifactRequirements::INDEXED | ArtifactRequirements::ANALYSIS;
        assert!(both.contains(ArtifactRequirements::INDEXED));
        assert!(both.contains(ArtifactRequirements::ANALYSIS));

        let indexed_only = ArtifactRequirements::INDEXED;
        assert!(indexed_only.contains(ArtifactRequirements::INDEXED));
        assert!(!indexed_only.contains(ArtifactRequirements::ANALYSIS));
    }

    #[test]
    fn broader_analysis_scope_satisfies_narrower_request() {
        let db = AnalysisReadyDb::new();
        let snapshot = Arc::new(crate::types::FileAnalysisSnapshot::default());
        let whole_hash = [1u8; 16];

        // Cache a BUILD-scope entry.
        db.insert(
            AnalysisArtifactKey {
                canonical_id: Arc::from("/w/a.ts"),
                whole_hash,
                scope: AnalysisScope::BUILD,
            },
            Arc::new(AnalysisReady {
                whole_hash,
                scope: AnalysisScope::BUILD,
                script_analysis: None,
                export_signatures: None,
                snapshot,
            }),
        );

        // A strict-key lookup for ESSENTIAL must miss even though ESSENTIAL
        // is a narrower scope — it is not the exact cached scope.
        let miss = db.get(&AnalysisArtifactKey {
            canonical_id: Arc::from("/w/a.ts"),
            whole_hash,
            scope: AnalysisScope::ESSENTIAL,
        });
        assert!(miss.is_none());

        // Satisfaction lookup succeeds because BUILD.contains(ESSENTIAL-ish
        // subset) — the key bits requested fit inside BUILD.
        let narrower = AnalysisScope::IMPORTS | AnalysisScope::BINDINGS | AnalysisScope::MACROS;
        let hit = db.find_satisfying("/w/a.ts", whole_hash, narrower);
        assert!(hit.is_some());
    }

    #[test]
    fn stale_whole_hash_invalidates_indexed_read() {
        let db = FileArtifactStore::new();
        let hash_v1 = [1u8; 16];
        let hash_v2 = [2u8; 16];
        let analysis = Arc::new(
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(
            ),
        );
        let shallow = Arc::new(
            crate::resolver_core::shallow_file_state::ShallowFileState::from_analysis(
                hash_v1, analysis, None,
            ),
        );
        db.insert(
            Arc::from("/w/a.ts"),
            Arc::new(IndexedReady {
                whole_hash: hash_v1,
                shallow_state: shallow,
                import_routes: Arc::new(FxHashMap::default()),
                import_route_hash: None,
                route_hash: None,
                edge_generation: 0,
                project_generation: 0,
                parse_env_hash: [0u8; 16],
                raw_source: Arc::from(""),
                eval_source: Arc::from(""),
                framework_parse: None,
                script_analysis: None,
                export_signatures: None,
                snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
                external_type_analysis: Arc::new(
                    verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
                ),
                declares_interface_app_config: false,
            }),
        );
        assert!(db.get("/w/a.ts", hash_v1).is_some());
        // Wrong hash — caller expects a different version → miss.
        assert!(db.get("/w/a.ts", hash_v2).is_none());
    }

    #[test]
    fn project_generation_bumps_monotonically() {
        let store = ProjectTypeStore::new();
        assert_eq!(store.project_generation(), 0);
        let g1 = store.bump_project_generation();
        assert_eq!(g1, 1);
        let g2 = store.bump_project_generation();
        assert_eq!(g2, 2);
        assert_eq!(store.project_generation(), 2);
    }

    /// The shared `Arc<AtomicU64>` counters on [`ProjectTypeStoreCounters`]
    /// reflect in-place updates from the backing DBs. Inserting a new
    /// canonical bumps `indexed_live`; replacing it under a new hash bumps
    /// `indexed_stale_sweeps` without increasing `indexed_live`.
    #[test]
    fn indexed_counters_reflect_insertions_and_replacements() {
        let store = ProjectTypeStore::new();
        let analysis = Arc::new(
            verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(
            ),
        );
        let mk_indexed = |hash: Hash16| {
            Arc::new(IndexedReady {
                whole_hash: hash,
                shallow_state: Arc::new(
                    crate::resolver_core::shallow_file_state::ShallowFileState::from_analysis(
                        hash,
                        Arc::clone(&analysis),
                        None,
                    ),
                ),
                import_routes: Arc::new(FxHashMap::default()),
                import_route_hash: None,
                route_hash: None,
                edge_generation: 0,
                project_generation: 0,
                parse_env_hash: [0u8; 16],
                raw_source: Arc::from(""),
                eval_source: Arc::from(""),
                framework_parse: None,
                script_analysis: None,
                export_signatures: None,
                snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
                external_type_analysis: Arc::clone(&analysis),
                declares_interface_app_config: false,
            })
        };

        let snap0 = store.counters.snapshot();
        assert_eq!(snap0.indexed_live, 0);
        assert_eq!(snap0.indexed_stale_sweeps, 0);

        store
            .indexed()
            .insert(Arc::from("/w/a.ts"), mk_indexed([1u8; 16]));
        let snap1 = store.counters.snapshot();
        assert_eq!(snap1.indexed_live, 1);
        assert_eq!(snap1.indexed_stale_sweeps, 0);

        // Replacement under the same canonical — live stays at 1; stale
        // sweep bumps.
        store
            .indexed()
            .insert(Arc::from("/w/a.ts"), mk_indexed([2u8; 16]));
        let snap2 = store.counters.snapshot();
        assert_eq!(snap2.indexed_live, 1);
        assert_eq!(snap2.indexed_stale_sweeps, 1);

        // Remove brings live to 0 and bumps stale sweep again.
        store.indexed().remove("/w/a.ts");
        let snap3 = store.counters.snapshot();
        assert_eq!(snap3.indexed_live, 0);
        assert_eq!(snap3.indexed_stale_sweeps, 2);
    }

    /// Calling `evict_canonical` on a canonical that has no entries across
    /// any of the owned DBs must be a no-op — no counter underflow, no
    /// panic, no dangling in-flight entries.
    #[test]
    fn evict_canonical_is_a_noop_for_unseen_canonical() {
        let store = ProjectTypeStore::new();
        store.evict_canonical("/w/never-seen.ts");
        let snap = store.counters.snapshot();
        assert_eq!(snap.indexed_live, 0);
        assert_eq!(snap.indexed_stale_sweeps, 0);
        assert_eq!(snap.analysis_live, 0);
        assert_eq!(snap.owner_import_live, 0);
        assert_eq!(snap.component_meta_live, 0);
    }

    /// `bump_project_generation_and_evict` clears every generation-sensitive
    /// cache and bumps the project generation counter. Invoked atomically
    /// on tsconfig / SDK / workspace-folder changes.
    #[test]
    fn bump_project_generation_and_evict_clears_route_and_result_layers() {
        let store = ProjectTypeStore::new();
        let hash = [3u8; 16];

        // Populate owner-import, component-meta, semantic-graph.
        store.owner_import_surfaces().insert(
            Arc::from("/w/o.vue"),
            Arc::new(crate::owner_import_surface::OwnerImportSurface {
                owner_canonical: Arc::from("/w/o.vue"),
                owner_whole_hash: hash,
                bindings: Arc::new(FxHashMap::default()),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
                validated_at_generation: 0,
            }),
        );
        store.component_meta_results().insert(
            crate::component_meta_result_db::ComponentMetaResultKey {
                owner_canonical: Arc::from("/w/o.vue"),
                options_fingerprint: [0u8; 16],
                project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
                parse_env_hash: [0u8; 16],
                resolve_env_hash: [0u8; 16],
                type_env_hash: [0u8; 16],
                lib_env_hash: [0u8; 16],
            },
            hash,
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(crate::component_meta_result_db::CachedComponentMetaResult {
                    analysis: empty_component_meta_analysis(),
                    resolution_template: crate::component_meta_result_db::ResolutionTemplate {
                        mode: crate::types::ProjectionMode::Expanded,
                        whole_hash: hash,
                        resolved_macros: Vec::new(),
                        resolved_type_registry: Vec::new(),
                        resolved_type_registry_meta: Vec::new(),
                        evaluated_types: None,
                        fact_versions: Vec::new(),
                        surface_identities: None,
                        origin_graph: None,
                    },
                    canonical_id: Arc::from("/w/o.vue"),
                    whole_hash: hash,
                }),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
                validated_at_generation: 0,
            },
        );

        assert_eq!(store.owner_import_surfaces().len(), 1);
        assert_eq!(store.component_meta_results().len(), 1);

        let g_before = store.project_generation();
        let g_after = store.bump_project_generation_and_evict();
        assert_eq!(g_after, g_before + 1);

        // Route-sensitive layers cleared; hash-rooted IndexedReady /
        // AnalysisReady survive (they key on whole_hash, not project gen).
        assert_eq!(store.owner_import_surfaces().len(), 0);
        assert_eq!(store.component_meta_results().len(), 0);
    }

    #[test]
    fn analysis_invalidate_canonical_removes_all_scopes_and_updates_counters() {
        let store = ProjectTypeStore::new();
        let snapshot = Arc::new(crate::types::FileAnalysisSnapshot::default());
        let hash = [1u8; 16];

        store.analysis().insert(
            AnalysisArtifactKey {
                canonical_id: Arc::from("/w/a.ts"),
                whole_hash: hash,
                scope: AnalysisScope::BUILD,
            },
            Arc::new(AnalysisReady {
                whole_hash: hash,
                scope: AnalysisScope::BUILD,
                script_analysis: None,
                export_signatures: None,
                snapshot: Arc::clone(&snapshot),
            }),
        );
        store.analysis().insert(
            AnalysisArtifactKey {
                canonical_id: Arc::from("/w/a.ts"),
                whole_hash: hash,
                scope: AnalysisScope::LSP,
            },
            Arc::new(AnalysisReady {
                whole_hash: hash,
                scope: AnalysisScope::LSP,
                script_analysis: None,
                export_signatures: None,
                snapshot: Arc::clone(&snapshot),
            }),
        );
        // Unrelated canonical — stays after invalidation.
        store.analysis().insert(
            AnalysisArtifactKey {
                canonical_id: Arc::from("/w/b.ts"),
                whole_hash: hash,
                scope: AnalysisScope::BUILD,
            },
            Arc::new(AnalysisReady {
                whole_hash: hash,
                scope: AnalysisScope::BUILD,
                script_analysis: None,
                export_signatures: None,
                snapshot: Arc::clone(&snapshot),
            }),
        );
        assert_eq!(store.counters.snapshot().analysis_live, 3);

        let removed = store.analysis().invalidate_canonical("/w/a.ts");
        assert_eq!(removed, 2);
        let snap = store.counters.snapshot();
        assert_eq!(snap.analysis_live, 1);
        assert_eq!(snap.analysis_stale_sweeps, 2);

        // B.ts still resolvable.
        assert!(store
            .analysis()
            .get(&AnalysisArtifactKey {
                canonical_id: Arc::from("/w/b.ts"),
                whole_hash: hash,
                scope: AnalysisScope::BUILD,
            })
            .is_some());
    }
}
