//! Project-global host-owned cache root (Phase 1)
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

use crate::component_meta_result_db::ComponentMetaResultDb;
use crate::intrinsic_registry::IntrinsicRegistry;
use crate::owner_import_surface::OwnerImportSurfaceDb;
use crate::resolver_core::imported_root_db::ImportedRootDb;
use crate::resolver_core::route_db::RouteDb;
use crate::semantic_query::DepVersion;
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
/// exports, and anything else later semantic passes need so they do not
/// rescan the raw file.
///
/// The OXC parse arena is transient — `IndexedReady` stores only owned
/// `Send + Sync` data so long-lived host-owned caches do not carry borrowed
/// AST pointers.
///
/// In Phase 1 this type wraps the pre-existing
/// [`ShallowFileState`](crate::resolver_core::shallow_file_state::ShallowFileState)
/// and the canonical import-route table. Later phases expand its schema to
/// cover the full `/type-resolution` skill contract — prepared declarations,
/// value-symbol annotations, SFC block anchors, etc. — rather than piping
/// each new field through an ad hoc side channel.
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

/// Host-owned cache of canonical [`IndexedReady`] artifacts. Keyed by
/// canonical file id; the entry carries the whole-hash so stale keys can be
/// rejected without rerunning a live lookup.
pub struct IndexedReadyDb {
    entries: DashMap<Arc<str>, Arc<IndexedReady>>,
    /// Live entry counter — bumped on insert of a new canonical key,
    /// decremented on remove. Replacement (insert with existing key) does
    /// not change the count.
    live_counter: Arc<AtomicU64>,
    /// Stale-sweep counter — bumped when [`Self::remove`] evicts an
    /// existing entry or a replacement supersedes a prior whole-hash.
    stale_sweeps: Arc<AtomicU64>,
}

impl IndexedReadyDb {
    pub fn new() -> Self {
        Self::with_counters(Default::default(), Default::default())
    }

    pub(crate) fn with_counters(live: Arc<AtomicU64>, stale: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter: live,
            stale_sweeps: stale,
        }
    }

    /// Look up the indexed artifact for `canonical_id` if the cached entry
    /// matches `expected_whole_hash`. Stale entries are ignored; callers
    /// materialize through the scheduler and re-populate.
    #[must_use]
    pub fn get(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
    ) -> Option<Arc<IndexedReady>> {
        let entry = self.entries.get(canonical_id)?;
        if entry.whole_hash == expected_whole_hash {
            Some(entry.clone())
        } else {
            None
        }
    }

    /// Insert or replace the entry for `canonical_id`. Older versions for the
    /// same canonical are overwritten — strong-consistency lookup is the
    /// responsibility of the caller via `expected_whole_hash`. A replacement
    /// increments the stale-sweep counter so downstream telemetry can see
    /// how often stale entries are superseded.
    pub fn insert(&self, canonical_id: Arc<str>, indexed: Arc<IndexedReady>) {
        let prev = self.entries.insert(canonical_id, indexed);
        if prev.is_some() {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove an entry outright (e.g. from an explicit file close).
    pub fn remove(&self, canonical_id: &str) {
        if self.entries.remove(canonical_id).is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Number of live entries. Primarily intended for per-layer debug
    /// counters / cache stats.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for IndexedReadyDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Host-owned cache of per-file [`AnalysisReady`] artifacts.
pub struct AnalysisReadyDb {
    entries: DashMap<AnalysisArtifactKey, Arc<AnalysisReady>>,
    live_counter: Arc<AtomicU64>,
    stale_sweeps: Arc<AtomicU64>,
}

impl AnalysisReadyDb {
    pub fn new() -> Self {
        Self::with_counters(Default::default(), Default::default())
    }

    pub(crate) fn with_counters(live: Arc<AtomicU64>, stale: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter: live,
            stale_sweeps: stale,
        }
    }

    /// Strict lookup by full key.
    #[must_use]
    pub fn get(&self, key: &AnalysisArtifactKey) -> Option<Arc<AnalysisReady>> {
        self.entries.get(key).map(|v| v.clone())
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
        for entry in self.entries.iter() {
            let key = entry.key();
            if key.canonical_id.as_ref() == canonical_id
                && key.whole_hash == whole_hash
                && key.scope.contains(requested_scope)
            {
                return Some(entry.value().clone());
            }
        }
        None
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
}

impl Default for AnalysisReadyDb {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// ProjectTypeStore
// ──────────────────────────────────────────────────────────────────────────

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
    indexed: IndexedReadyDb,
    /// Analysis augmentation cache keyed by `AnalysisScope`.
    analysis: AnalysisReadyDb,
    /// Rehomed routing-surface cache. `RouteDb` survives as the shared
    /// route/barrel authority under project-global validation semantics.
    routes: Arc<RouteDb>,
    /// Temporary transitive-discovery helper. In Phase 2 this collapses to
    /// transitive-only use; by Phase 5 it folds into the shared route /
    /// semantic-query layer.
    imported_roots: Arc<ImportedRootDb>,
    /// Host-owned semantic-query memo table + node arena. Shared across
    /// every consumer that resolves a `SemanticQueryKey` through the
    /// shared query API.
    semantic_graph: Arc<SemanticGraphStore>,
    /// Owner direct-import surface cache (Phase 2). Direct owner imports
    /// resolve exactly once per owner version and every downstream stage
    /// reads the same entry.
    owner_import_surfaces: OwnerImportSurfaceDb,
    /// Final component-meta result cache (Phase 3). Keyed by
    /// `(owner_canonical, owner_whole_hash, query_kind, options_fingerprint)`.
    /// The payload type is erased at this layer; concrete callers hold a
    /// typed handle through the `ComponentMetaResultDb<P>` API. In Phase 3
    /// this is wired onto the native component-meta payload shape.
    component_meta_results:
        ComponentMetaResultDb<crate::types::FinalComponentMetaPayloadPlaceholder>,
    /// TypeScript `intrinsic` registry (Phase 2.1). Maps resolved
    /// declaration names that have `= intrinsic` bodies to their
    /// implementation arms. Userland aliases like `Pick` / `Omit` never
    /// reach this registry — it is consulted only after the normal
    /// declaration path resolves to `= intrinsic`.
    intrinsic_registry: IntrinsicRegistry,
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
        let counters = ProjectTypeStoreCounters::default();
        // Each backing DB holds the same `Arc<AtomicU64>` counters as
        // `counters` so the `snapshot()` method sees in-place updates.
        let indexed = IndexedReadyDb::with_counters(
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
            ComponentMetaResultDb::<crate::types::FinalComponentMetaPayloadPlaceholder>::DEFAULT_CAPACITY,
            Arc::clone(&counters.component_meta_live),
            Arc::clone(&counters.component_meta_stale_sweeps),
        );
        Self {
            project_generation: AtomicU64::new(0),
            indexed,
            analysis,
            routes: Arc::new(RouteDb::new()),
            imported_roots: Arc::new(ImportedRootDb::new()),
            semantic_graph: Arc::new(SemanticGraphStore::new()),
            owner_import_surfaces,
            component_meta_results,
            intrinsic_registry: IntrinsicRegistry::with_defaults(),
            counters,
        }
    }

    /// Current monotonic project generation. Owned by the host / workspace
    /// layer — queries read it but never mutate it.
    pub fn project_generation(&self) -> u64 {
        self.project_generation.load(Ordering::Acquire)
    }

    /// Bump the project generation. Invoked exclusively by the host /
    /// workspace layer on `tsconfig`, path-alias, active-TS-SDK,
    /// workspace-folder, package export-target, and explicit project-graph
    /// changes — never on file-content edits.
    pub fn bump_project_generation(&self) -> u64 {
        self.project_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn indexed(&self) -> &IndexedReadyDb {
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

    /// Host-owned semantic-query memo table. Shared across every consumer
    /// that dispatches through the semantic-query API.
    pub fn semantic_graph(&self) -> &Arc<SemanticGraphStore> {
        &self.semantic_graph
    }

    /// Owner direct-import surface cache (Phase 2). Direct owner imports
    /// resolve exactly once per owner version through this cache.
    pub fn owner_import_surfaces(&self) -> &OwnerImportSurfaceDb {
        &self.owner_import_surfaces
    }

    /// Final component-meta result cache (Phase 3).
    pub fn component_meta_results(
        &self,
    ) -> &ComponentMetaResultDb<crate::types::FinalComponentMetaPayloadPlaceholder> {
        &self.component_meta_results
    }

    /// TypeScript `intrinsic` registry (Phase 2.1). Read-only from the
    /// resolver hot path; the host may re-register entries at boot when
    /// the active TS SDK is swapped.
    pub fn intrinsic_registry(&self) -> &IntrinsicRegistry {
        &self.intrinsic_registry
    }

    /// Build a `(project_generation, whole_hash)` dep-signature pair that
    /// downstream callers merge into their active
    /// [`CompletionFence`](crate::completion_fence::CompletionFence).
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
    /// - `IndexedReadyDb`: removes the entry (lookup would otherwise
    ///   harmlessly miss via whole-hash mismatch, but the memory would
    ///   leak until re-materialization overwrote it).
    /// - `AnalysisReadyDb`: removes every `(hash, scope)` entry for the
    ///   canonical.
    /// - `OwnerImportSurfaceDb`: removes the owner surface.
    /// - `ComponentMetaResultDb`: removes every result keyed on the owner.
    /// - `SemanticGraphStore`: removes every memo entry whose scope
    ///   references the canonical.
    pub fn evict_canonical(&self, canonical_id: &str) {
        self.indexed.remove(canonical_id);
        self.analysis.invalidate_canonical(canonical_id);
        self.owner_import_surfaces.remove(canonical_id);
        self.component_meta_results.invalidate_owner(canonical_id);
        self.semantic_graph.invalidate_canonical(canonical_id);
    }

    /// Targeted invalidation of a project-generation bump.
    ///
    /// Called when the host / workspace detects `tsconfig`,
    /// active-TypeScript-SDK, workspace-folder, or other project-shape
    /// changes. Bumps the project generation and wipes every cache layer
    /// whose identity depends on project configuration rather than raw
    /// file text. Per plan § A0, this is invoked atomically before new
    /// queries are admitted.
    pub fn bump_project_generation_and_evict(&self) -> u64 {
        let generation = self.bump_project_generation();
        // File-content identity stays (IndexedReady / AnalysisReady keyed
        // by whole_hash are still correct for the same file content).
        // What becomes stale is:
        //   - route / barrel facts (config changes may shift resolution)
        //   - owner import surfaces (routes they resolved may shift)
        //   - component-meta results (depend on routes / intrinsic SDK)
        //   - semantic query memo (derived from routes + intrinsics)
        self.owner_import_surfaces
            .entries_drain_for_generation_bump();
        let _ = self.semantic_graph.invalidate_all();
        self.component_meta_results.invalidate_all();
        generation
    }
}

impl Default for ProjectTypeStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let db = IndexedReadyDb::new();
        let hash_v1 = [1u8; 16];
        let hash_v2 = [2u8; 16];
        let analysis = Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        );
        let shallow = Arc::new(crate::resolver_core::shallow_file_state::ShallowFileState {
            whole_hash: hash_v1,
            exports: FxHashMap::default(),
            wildcard_reexports: vec![],
            symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            import_locals: rustc_hash::FxHashSet::default(),
            import_targets: FxHashMap::default(),
            analysis,
        });
        db.insert(
            Arc::from("/w/a.ts"),
            Arc::new(IndexedReady {
                whole_hash: hash_v1,
                shallow_state: shallow,
                import_routes: Arc::new(FxHashMap::default()),
                import_route_hash: None,
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
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        );
        let mk_indexed = |hash: Hash16| {
            Arc::new(IndexedReady {
                whole_hash: hash,
                shallow_state: Arc::new(
                    crate::resolver_core::shallow_file_state::ShallowFileState {
                        whole_hash: hash,
                        exports: FxHashMap::default(),
                        wildcard_reexports: vec![],
                        symbols: FxHashMap::default(),
                        value_symbols: FxHashMap::default(),
                        import_locals: rustc_hash::FxHashSet::default(),
                        import_targets: FxHashMap::default(),
                        analysis: Arc::clone(&analysis),
                    },
                ),
                import_routes: Arc::new(FxHashMap::default()),
                import_route_hash: None,
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
    /// cache and bumps the project generation counter. Per plan § A0, this
    /// is invoked atomically on tsconfig / SDK / workspace-folder changes.
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
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            }),
        );
        store.component_meta_results().insert(
            crate::component_meta_result_db::ComponentMetaResultKey {
                owner_canonical: Arc::from("/w/o.vue"),
                owner_whole_hash: hash,
                query_kind: crate::component_meta_result_db::ComponentMetaQueryKind::Native,
                options_fingerprint: [0u8; 16],
            },
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(crate::types::FinalComponentMetaPayloadPlaceholder),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
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

        // b.ts still resolvable.
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
