//! `FileArtifactStore`: the canonical per-file artifact cache.
//!
//! `FileArtifactStore` is the **authoritative** post-parse cache. It replaces
//! the legacy `FileArtifactStore` type as the single per-file storage layer on
//! [`crate::project_type_store::ProjectTypeStore`].
//!
//! The store is **content-addressed**: keys carry the file's canonical path
//! AND its `content_hash` AND the `parse_env_hash` (R5, R6). Concurrent
//! overlay sessions reading different versions never poison each other.
//!
//! ## What lives here
//!
//! - [`FileArtifactKey`] — `(canonical, content_hash, parse_env_hash, parser_version)`.
//! - [`FileArtifacts`] — the per-file payload: `IndexedReady`, `FileFacts`,
//!   `ParsedEdges`, `parse_stable_hash`, `augmentations`.
//! - [`AugmentationTargetKey`] / [`AugmenterSet`] — the inverse-lookup index
//!   for module augmentations (R29).
//! - [`FileFacts`] — placeholder; per-file fact registry payload (populated
//!   by the fact-emission walk).
//! - [`ParsedEdges`] — content-addressed import-edge facts published per
//!   file version (populated when the workspace-edge map becomes
//!   idempotent on the env-hash quintuple when the workspace-edge map becomes
//!   idempotent on the env-hash quintuple).
//! - [`ModuleAugmentationFact`] — per-file syntactic augmentation fact
//!   (populated by the fact-emission walk; left empty until then).
//!
//! ## Two API surfaces
//!
//! The store exposes both:
//!
//! 1. **Canonical-keyed legacy surface** — matches the retired
//!    `FileArtifactStore` shape (`get(canonical, hash)`, `insert(canonical,
//!    indexed)`, etc.) returning `Arc<IndexedReady>` directly. Existing
//!    callers see no signature break across the rename.
//! 2. **`FileArtifactKey`-keyed canonical surface** — `get_artifacts(&key)`,
//!    `insert_artifacts(key, artifacts)`, etc. Returns `Arc<FileArtifacts>`,
//!    the new payload type with `IndexedReady` + facts + parsed edges +
//!    `parse_stable_hash` + augmentations. New code (later stages) uses
//!    this surface.
//!
//! Both surfaces share the same backing `DashMap<FileArtifactKey,
//! Arc<FileArtifacts>>`. The legacy surface synthesises a default
//! [`FileArtifactKey`] from `(canonical, indexed.whole_hash)`; later stages
//! plumb the full `(parse_env_hash, parser_version)` quintuple through.
//!
//! ## Invariants
//!
//! - **Content-addressed (R5, R6):** the key carries every dimension that
//!   meaningfully changes the cached value.
//! - **Eviction is memory-bound (R22):** there is no `invalidate_canonical`
//!   on `FileArtifactStore`. The existing `ProjectTypeStore::evict_canonical`
//!   cascade calls `remove_canonical(canonical_id)` to drain all versions
//!   under a canonical (this path is the canonical-keyed legacy retirement target).
//! - **`parse_stable_hash`:** alpha-normalised structural hash over the
//!   file's post-shallow-analysis decl skeleton. Invariant under cosmetic
//!   edits.
//! - **`augmentation_index` populated lazily on first augmentation-sensitive query:** This module owns
//!   the skeleton + accessor API.
//!
//! See `/type-cache-architecture` skill for the full rule set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use smallvec::SmallVec;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::registry as fact_registry;

use crate::project_type_store::IndexedReady;

// The fact-registry types live in `verter_semantic` so the registry can
// reference them without a back-edge on `verter_session`. We re-import
// them here so existing callers continue to see them under the
// `verter_session::file_artifact_store::*` paths they already use
// (no `pub use as` shimming — the types are identical, not renamed).
pub use verter_semantic::facts::registry::{
    InternedGlobPattern, InternedName, InternedSpecifier, SymbolSpace,
};

// ── Project identity wrapper ──

/// Thin newtype around the 16-byte `project_identity` value produced by
/// `IdeProjectConfig::project_identity()`.
///
/// Used as a key dimension on [`AugmentationTargetKey`] to keep
/// augmentation entries from one project from poisoning a sibling
/// project under the same syntactic specifier (Codex P0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectIdentity(pub Hash16);

// ── FileArtifactKey ──

/// Cache key for [`FileArtifacts`].
///
/// Keys are content-addressed (R5, R6): identity is the conjunction of
/// `canonical`, `content_hash`, `parse_env_hash`, and `parser_version`.
/// Two project envs reading the same canonical at the same `content_hash`
/// but different `parse_env_hash` coexist; the cache returns the matching
/// entry for the caller's env.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileArtifactKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub parser_version: u32,
}

impl FileArtifactKey {
    /// Legacy-shape constructor: builds a key with `parse_env_hash` zeroed
    /// and `parser_version = LEGACY_PARSER_VERSION`. Used by the
    /// canonical-keyed legacy API surface that does not yet thread env
    /// hashes through (call sites are migrated incrementally as later
    /// stages introduce real env hashes for each entry point).
    pub(crate) fn legacy(canonical: Arc<str>, content_hash: Hash16) -> Self {
        Self {
            canonical,
            content_hash,
            parse_env_hash: LEGACY_PARSE_ENV_HASH,
            parser_version: LEGACY_PARSER_VERSION,
        }
    }

    /// Test-only public shim over [`Self::legacy`].
    ///
    /// Used by `tests/eviction_policy.rs` and similar integration
    /// tests that need to construct multiple distinct
    /// `FileArtifactKey` variants for the same canonical to
    /// exercise the per-canonical retention sweep + the
    /// promotion-aware LRU floor. The production `pub(crate)`
    /// surface is unchanged; this `pub fn` exists only inside
    /// `#[cfg(any(test, debug_assertions))]` so production
    /// builds carry no public exposure of the legacy constructor.
    #[cfg(any(test, debug_assertions))]
    pub fn legacy_for_test(canonical: Arc<str>, content_hash: Hash16) -> Self {
        Self::legacy(canonical, content_hash)
    }
}

/// Parser version for legacy-shape inserts. Bumps invalidate every
/// entry the legacy surface inserted under
/// [`FileArtifactKey::legacy`].
pub const LEGACY_PARSER_VERSION: u32 = 1;

/// `parse_env_hash` sentinel used by the canonical-keyed legacy surface
/// before later stages plumb the real env hash through every call site.
pub const LEGACY_PARSE_ENV_HASH: Hash16 = [0u8; 16];

// ── FileFacts ──

/// Per-file fact registry payload.
///
/// Owns the parse-domain `FactRegistry` populated by the shallow walk
/// during file-artifact construction (R10–R16, R28, R29). Consumers
/// read parse-domain facts via [`FileFacts::registry`] / `lookup`.
///
/// Parse-time emission populates: `Export`, `ExportAlias`,
/// `SyntacticExportSet`, `LocalDecl`, `MemberShape`, `MemberPresence`,
/// `MacroSurface`, `TemplateRoot`, `ImportRef`, `SyntacticReexportRef`,
/// `ModuleAugmentation`. The `Member` body fingerprint is computed
/// lazily on first member-access query and lives in
/// `MemberSemanticFactStore` / `MemberDisplayFactStore`, NOT in
/// this registry.
///
/// Resolve-domain facts are NOT populated here — they emit from the
/// resolver / `RouteDb` producers downstream.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileFacts {
    registry: fact_registry::FactRegistry,
}

impl FileFacts {
    /// Construct an empty fact registry. Used by tests + legacy
    /// constructors that bypass parse-time emission.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Construct a populated `FileFacts` from a fact registry.
    #[must_use]
    pub fn from_registry(registry: fact_registry::FactRegistry) -> Self {
        Self { registry }
    }

    /// Borrow the underlying registry. O(1) per-key lookup via
    /// `registry().get(&key)`.
    #[must_use]
    pub fn registry(&self) -> &fact_registry::FactRegistry {
        &self.registry
    }

    /// Borrow the registry mutably — used by the parse-time
    /// producer during construction.
    pub fn registry_mut(&mut self) -> &mut fact_registry::FactRegistry {
        &mut self.registry
    }

    /// O(1) per-key lookup. `None` is an invalidation observation —
    /// the binding was removed or never existed.
    #[must_use]
    pub fn lookup(&self, key: &fact_registry::FactKey) -> Option<&fact_registry::Fact> {
        self.registry.get(key)
    }

    /// Number of parse-domain facts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// `true` if no facts have been emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    /// The cached `SyntacticExportSet` fact (if parse-time
    /// emission produced one).
    #[must_use]
    pub fn syntactic_export_set(&self) -> Option<&fact_registry::Fact> {
        self.registry.syntactic_export_set.as_ref()
    }
}

// ── ParsedEdges ──

/// Content-addressed parsed import-edge facts for a single file version.
///
/// Empty placeholder. The workspace-edge map becomes idempotent
/// on the env-hash quintuple, at which point this type holds the
/// authoritative content-addressed edge facts directly.
#[derive(Debug, Default, Clone)]
pub struct ParsedEdges {
    _stage1_placeholder: (),
}

impl ParsedEdges {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

// ── ModuleAugmentationFact ──

/// A single `declare module "<specifier>" { ... }` block emitted by the
/// parser during shallow analysis.
///
/// The type is defined here; the shallow walk populates it.
/// Augmenting declarations are stitched into the consumer's
/// `EffectiveExportSet` for that specifier.
///
/// Fields:
///
/// - `specifier` — the syntactic specifier inside `declare module "X" {}`.
/// - `augmented_name` — the name of an augmented binding inside the block.
/// - `space` — which symbol space the augmented binding occupies.
/// - `augmented_member_shape_fingerprint` — alpha-normalised fingerprint
///   over the augmenting block's member set; used to detect
///   when an augmenter's contribution to the effective surface changes
///   without changing the augmenter set itself.
#[derive(Debug, Clone)]
pub struct ModuleAugmentationFact {
    pub specifier: InternedSpecifier,
    pub augmented_name: InternedName,
    pub space: SymbolSpace,
    pub augmented_member_shape_fingerprint: Hash16,
}

// ── AugmentationTargetKey / AugmentationTargetKind ──

/// Kind of augmentation target.
///
/// Distinguishes external specifiers (`declare module "vue" {}`),
/// resolved relative paths (`declare module "./local" {}` resolved
/// against the augmenter), wildcard ambients (`declare module "*.css" {}`),
/// and the global block (`declare global {}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AugmentationTargetKind {
    /// `declare module "vue" {}` — bare specifier resolved through the
    /// project's module resolver under the resolve env.
    ExternalSpecifier(InternedSpecifier),
    /// `declare module "./local" {}` — relative path resolved against
    /// the augmenter's own canonical.
    ResolvedRelativeCanonical(Arc<str>),
    /// `declare module "*.css" {}` — wildcard ambient module pattern.
    WildcardAmbient(InternedGlobPattern),
    /// `declare global { ... }` — augments the global scope.
    GlobalAugmentation,
}

/// Inverse-lookup key for the augmentation index.
///
/// Carries the resolve-domain dimensions (`project_identity`,
/// `resolve_env_hash`, `lib_env_hash`) so the same syntactic specifier
/// `"vue"` in two projects under different envs produces two distinct
/// keys. Project isolation prevents cross-project poisoning (Codex P0.1).
///
/// R21 scoping rule: this key carries `lib_env_hash` because module
/// augmentations live inside libs / ambient corpora — a lib update CAN
/// change which augmenters are visible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AugmentationTargetKey {
    pub project_identity: ProjectIdentity,
    pub resolve_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub target: AugmentationTargetKind,
}

// ── AugmenterSet ──

/// The set of augmenter files that contribute to a given
/// [`AugmentationTargetKey`], sorted by `(augmenter_canonical,
/// augmenter_parse_stable_hash)`.
#[derive(Debug, Clone)]
pub struct AugmenterSet {
    /// `(augmenter_canonical, augmenter_parse_stable_hash)` pairs, sorted.
    pub entries: SmallVec<[(Arc<str>, Hash16); 2]>,
    /// Cached `stable_hash(entries)` — the basis of
    /// `ModuleAugmentationIndexShape`.
    pub fingerprint: Hash16,
}

// ── FileArtifacts ──

/// The per-file payload stored under [`FileArtifactKey`].
///
/// Owns the canonical `IndexedReady` artifact along with the placeholder
/// fact-registry / parsed-edges / augmentation containers thby the fact-emission walk +
/// later fact-emission + workspace-edge work will populate.
#[derive(Debug, Clone)]
pub struct FileArtifacts {
    pub indexed: Arc<IndexedReady>,
    pub facts: Arc<FileFacts>,
    pub parsed_edges: Arc<ParsedEdges>,
    pub parse_stable_hash: Hash16,
    pub augmentations: Arc<Vec<ModuleAugmentationFact>>,
}

impl FileArtifacts {
    /// Construct a `FileArtifacts` carrying only an `IndexedReady`.
    ///
    /// **Parse-time fact emission runs here** — the constructed
    /// `FileFacts` is populated with the parse-domain
    /// `FactRegistry` (`Export`, `LocalDecl`, `MemberShape`,
    /// `MemberPresence`, `SyntacticExportSet`, `ImportRef`,
    /// `SyntacticReexportRef`, `ExportAlias`, `ModuleAugmentation`)
    /// by [`crate::fact_emission::emit_parse_facts`]. The per-file
    /// augmentation list is populated alongside the facts. The
    /// cross-project `augmentation_index` on
    /// [`FileArtifactStore`] is NOT touched here — it is
    /// populated lazily on first augmentation-sensitive query.
    #[must_use]
    pub fn with_indexed(indexed: Arc<IndexedReady>) -> Self {
        let parse_stable_hash = crate::parse_stable_hash::compute_parse_stable_hash(&indexed);
        let emission = crate::fact_emission::emit_parse_facts(&indexed);
        Self {
            indexed,
            facts: Arc::new(emission.facts),
            parsed_edges: Arc::new(ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(emission.augmentations),
        }
    }
}

// ── FileArtifactStore ──

/// The per-host content-addressed file-artifact cache.
///
/// Replaces the retired `FileArtifactStore` type as the authoritative
/// per-file storage layer. The same struct serves both the legacy
/// canonical-keyed surface (`get(canonical, hash) -> Arc<IndexedReady>`)
/// and the new content-addressed surface (`get_artifacts(&key) ->
/// Arc<FileArtifacts>`).
pub struct FileArtifactStore {
    /// Per-(canonical, content_hash, parse_env_hash, parser_version)
    /// payloads. Keys with the same canonical but different other
    /// dimensions coexist.
    artifacts: DashMap<FileArtifactKey, Arc<FileArtifacts>>,
    /// Per-canonical last-access tick (monotonically increasing). Used by
    /// [`Self::evict_lru`] under explicit memory pressure to drop the
    /// oldest entries down to a configured floor.
    last_access: DashMap<Arc<str>, u64>,
    access_tick: AtomicU64,
    /// Per-key hit counter — bumped on every warm `get` /
    /// `get_artifacts` hit. Consumed by the LRU floor's promotion
    /// predicate: entries whose counter is below
    /// `promote_threshold` are evicted first regardless of
    /// `last_access` recency.
    hit_counters: DashMap<FileArtifactKey, u32>,
    /// Live entry counter.
    live_counter: Arc<AtomicU64>,
    /// Stale-sweep counter.
    stale_sweeps: Arc<AtomicU64>,
    /// Cache-cluster schema version this store was constructed under.
    schema_version: u32,
    /// Inverse-lookup index for module augmentations. Populated at
    /// Populated lazily by the augmentation-stitching pass when
    /// `EffectiveExportSet(specifier)` first requests an inverse lookup.
    /// See `/type-cache-architecture` skill for the populator semantics.
    augmentation_index: DashMap<AugmentationTargetKey, Arc<AugmenterSet>>,
    /// Test-only host-level audit hook.
    #[cfg(test)]
    test_audit_hook: parking_lot::Mutex<Option<Arc<crate::host_test_audit::HostTestAuditState>>>,
}

impl std::fmt::Debug for FileArtifactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileArtifactStore")
            .field("artifacts_len", &self.artifacts.len())
            .field("augmentation_index_len", &self.augmentation_index.len())
            .field("schema_version", &self.schema_version)
            .finish_non_exhaustive()
    }
}

impl Default for FileArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FileArtifactStore {
    #[must_use]
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

    /// Test-only constructor that pins a specific schema version on the
    /// store. Used by `cache_invariant_migration` fixtures.
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
            artifacts: DashMap::new(),
            last_access: DashMap::new(),
            access_tick: AtomicU64::new(0),
            hit_counters: DashMap::new(),
            live_counter: live,
            stale_sweeps: stale,
            schema_version,
            augmentation_index: DashMap::new(),
            #[cfg(test)]
            test_audit_hook: parking_lot::Mutex::new(None),
        }
    }

    /// Install the host-level test audit hook (legacy `FileArtifactStore`
    /// equivalent).
    #[cfg(test)]
    pub(crate) fn install_test_audit_hook(
        &self,
        state: Arc<crate::host_test_audit::HostTestAuditState>,
    ) {
        *self.test_audit_hook.lock() = Some(state);
    }

    // ──────────────────────────────────────────────────────────────────
    // Legacy `FileArtifactStore` API surface
    //
    // These methods preserve the retired type's signatures so existing
    // call sites compile across the rename. They map onto the
    // canonical-keyed legacy slot of the underlying `DashMap`.
    // ──────────────────────────────────────────────────────────────────

    /// Look up the indexed artifact for `canonical_id` if the cached
    /// entry matches `expected_whole_hash`. Stale entries are ignored.
    #[must_use]
    pub fn get(
        &self,
        canonical_id: &str,
        expected_whole_hash: Hash16,
    ) -> Option<Arc<IndexedReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let key = FileArtifactKey::legacy(Arc::from(canonical_id), expected_whole_hash);
        let result = self
            .artifacts
            .get(&key)
            .map(|entry| Arc::clone(&entry.value().indexed));
        if result.is_some() {
            self.bump_access_tick(canonical_id);
            self.bump_hit_counter(&key);
        }
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .indexed
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .indexed
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Look up the cached artifact for `canonical_id` without hash check.
    #[must_use]
    pub fn get_any(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let mut result: Option<Arc<IndexedReady>> = None;
        let mut matched_key: Option<FileArtifactKey> = None;
        for entry in self.artifacts.iter() {
            if entry.key().canonical.as_ref() == canonical_id {
                result = Some(Arc::clone(&entry.value().indexed));
                matched_key = Some(entry.key().clone());
                break;
            }
        }
        if result.is_some() {
            self.bump_access_tick(canonical_id);
            if let Some(k) = matched_key.as_ref() {
                self.bump_hit_counter(k);
            }
        }
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .indexed
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .indexed
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    fn bump_access_tick(&self, canonical_id: &str) {
        let tick = self.access_tick.fetch_add(1, Ordering::Relaxed) + 1;
        self.last_access.insert(Arc::from(canonical_id), tick);
    }

    /// Bump the per-key hit counter — called from every warm
    /// `get_artifacts` / `get` hit. The counter is consumed by
    /// [`Self::evict_lru_promoted`] and saturates at `u32::MAX` so
    /// long-lived hot entries do not overflow.
    fn bump_hit_counter(&self, key: &FileArtifactKey) {
        self.hit_counters
            .entry(key.clone())
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }

    /// Test-only inspection of the per-key hit counter.
    #[cfg(any(test, debug_assertions))]
    pub fn hit_count(&self, key: &FileArtifactKey) -> u32 {
        self.hit_counters.get(key).map(|c| *c.value()).unwrap_or(0)
    }

    /// Snapshot every `(canonical_id, content_hash)` key in the cache.
    #[must_use]
    pub fn keys(&self) -> Vec<(Arc<str>, Hash16)> {
        self.artifacts
            .iter()
            .map(|entry| (entry.key().canonical.clone(), entry.key().content_hash))
            .collect()
    }

    /// LRU floor — drop entries down to `min_floor` by oldest-access
    /// order.
    ///
    /// Delegates to [`Self::evict_lru_promoted`] with `promote_threshold
    /// = 0` (no promotion — pure recency LRU). Promotion-aware callers
    /// thread the configured `promote_threshold` directly through
    /// `evict_lru_promoted` to preserve hot entries.
    pub fn evict_lru(&self, min_floor: usize) {
        self.evict_lru_promoted(min_floor, 0);
    }

    /// Promotion-aware LRU floor. Entries whose per-key hit counter
    /// is **strictly below** `promote_threshold` are considered
    /// "cold" and age out first regardless of `last_access`
    /// recency; the floor's recency comparison only applies among
    /// the surviving cold pool. Hot entries (counter >=
    /// `promote_threshold`) survive unless every entry is hot, in
    /// which case the floor falls back to pure recency.
    ///
    /// R22 — memory-bound eviction. The hot/cold split
    /// is the only behavioural difference from the pure-recency
    /// [`Self::evict_lru`]; correctness still flows from
    /// fact-validation (R19).
    pub fn evict_lru_promoted(&self, min_floor: usize, promote_threshold: u32) {
        let len = self.artifacts.len();
        if len <= min_floor {
            return;
        }
        let drop_count = len - min_floor;
        // Collect (key, hit_count, tick) for every entry.
        let mut entries: Vec<(FileArtifactKey, u32, u64)> = self
            .artifacts
            .iter()
            .map(|entry| {
                let key = entry.key().clone();
                let hits = self.hit_counters.get(&key).map(|c| *c.value()).unwrap_or(0);
                let tick = self
                    .last_access
                    .get(&key.canonical)
                    .map(|t| *t.value())
                    .unwrap_or(0);
                (key, hits, tick)
            })
            .collect();
        // Partition: cold (hits < promote_threshold) first, then hot.
        // Within each partition, oldest tick first.
        entries.sort_by(|a, b| {
            let a_cold = a.1 < promote_threshold;
            let b_cold = b.1 < promote_threshold;
            match (a_cold, b_cold) {
                (true, false) => std::cmp::Ordering::Less, // cold first
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.2.cmp(&b.2), // within partition, oldest first
            }
        });
        for (key, _hits, _tick) in entries.into_iter().take(drop_count) {
            if self.artifacts.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            }
            self.hit_counters.remove(&key);
            // Only remove `last_access` if no other version of the
            // same canonical survives — the access tick is per
            // canonical, not per FileArtifactKey.
            let has_more = self
                .artifacts
                .iter()
                .any(|e| e.key().canonical.as_ref() == key.canonical.as_ref());
            if !has_more {
                self.last_access.remove(&key.canonical);
            }
        }
    }

    /// Enforce per-canonical content-hash retention. Keeps at most
    /// `retention` distinct `FileArtifactKey` variants per
    /// canonical id; older variants (by `last_access` proxy: their
    /// canonical's tick, then iteration order) are dropped.
    ///
    /// Setting `retention == usize::MAX` is a no-op. Setting
    /// `retention == 0` drops every variant beyond the most
    /// recently inserted (the live counter's "current generation").
    ///
    /// Binds R22: the cap is a memory bound; correctness is still
    /// owned by fact-validation. Variant ordering uses the entry's
    /// own `content_hash` as a stable tiebreaker so test runs are
    /// deterministic across DashMap iteration order.
    pub fn enforce_per_canonical_retention(&self, retention: usize) {
        if retention == usize::MAX {
            return;
        }
        // Group keys by canonical.
        let mut by_canonical: rustc_hash::FxHashMap<Arc<str>, Vec<FileArtifactKey>> =
            rustc_hash::FxHashMap::default();
        for entry in self.artifacts.iter() {
            by_canonical
                .entry(entry.key().canonical.clone())
                .or_default()
                .push(entry.key().clone());
        }
        for (_canonical, mut keys) in by_canonical {
            if keys.len() <= retention {
                continue;
            }
            // Sort by content_hash for deterministic order; we drop
            // from the front (older / lower-numbered variants).
            keys.sort_by(|a, b| a.content_hash.cmp(&b.content_hash));
            let drop_count = keys.len() - retention;
            for key in keys.into_iter().take(drop_count) {
                if self.artifacts.remove(&key).is_some() {
                    self.live_counter.fetch_sub(1, Ordering::Relaxed);
                    self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
                }
                self.hit_counters.remove(&key);
            }
        }
    }

    /// Snapshot every live entry for auditing / diagnostics, in
    /// `(canonical, indexed)` shape (matches the legacy `FileArtifactStore`
    /// API).
    #[must_use]
    pub fn snapshot_all(&self) -> Vec<(Arc<str>, Arc<IndexedReady>)> {
        self.artifacts
            .iter()
            .map(|entry| {
                (
                    entry.key().canonical.clone(),
                    Arc::clone(&entry.value().indexed),
                )
            })
            .collect()
    }

    /// Insert or replace the entry for `canonical_id`. Older versions for
    /// the same canonical are overwritten — the legacy `FileArtifactStore`
    /// guaranteed exactly one entry per canonical regardless of
    /// content_hash, so this method preserves that semantics by draining
    /// every other version of the same canonical before inserting the
    /// new one.
    ///
    /// The new content-addressed `insert_artifacts` surface DOES allow
    /// multiple versions to coexist; callers that want that behaviour
    /// MUST route through `insert_artifacts` with a full
    /// `FileArtifactKey`.
    pub fn insert(&self, canonical_id: Arc<str>, indexed: Arc<IndexedReady>) {
        let whole_hash = indexed.whole_hash;
        let canonical_for_event = Arc::clone(&canonical_id);
        let tick = self.access_tick.fetch_add(1, Ordering::Relaxed) + 1;
        self.last_access.insert(Arc::clone(&canonical_id), tick);

        // Legacy semantics: drain every prior version of the same
        // canonical before inserting. The new entry replaces them all.
        let prior_keys: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .filter(|entry| entry.key().canonical.as_ref() == canonical_id.as_ref())
            .map(|entry| entry.key().clone())
            .collect();
        let had_prior = !prior_keys.is_empty();
        for prior_key in prior_keys {
            self.artifacts.remove(&prior_key);
            self.hit_counters.remove(&prior_key);
        }

        let key = FileArtifactKey::legacy(canonical_id, whole_hash);
        let payload = Arc::new(FileArtifacts::with_indexed(indexed));
        self.artifacts.insert(key, payload);

        if had_prior {
            // Replacement: live count unchanged, bump stale sweep.
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
            // Audit event fires on FRESH inserts only (matches retired
            // FileArtifactStore::insert behaviour).
            crate::component_meta_audit::record_indexed_ready_built(
                Arc::clone(&canonical_for_event),
                whole_hash,
            );
            #[cfg(test)]
            if let Some(state) = self.test_audit_hook.lock().as_ref() {
                state.record_shallow_process(canonical_for_event.as_ref());
            }
        }
    }

    /// Remove every entry for `canonical_id` regardless of content hash
    /// (legacy `FileArtifactStore::remove` semantics).
    pub fn remove(&self, canonical_id: &str) {
        let to_remove: Vec<FileArtifactKey> = self
            .artifacts
            .iter()
            .filter(|entry| entry.key().canonical.as_ref() == canonical_id)
            .map(|entry| entry.key().clone())
            .collect();
        for key in &to_remove {
            if self.artifacts.remove(key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            }
            self.hit_counters.remove(key);
        }
        self.last_access.remove(canonical_id);
    }

    /// Number of live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let canonical: Arc<str> = Arc::from(marker);
        let indexed = Arc::new(IndexedReady::new_for_test([0u8; 16]));
        let payload = Arc::new(FileArtifacts::with_indexed(indexed));
        let key = FileArtifactKey::legacy(canonical, [0u8; 16]);
        let prev = self.artifacts.insert(key, payload);
        if prev.is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // New `FileArtifactKey`-keyed canonical API surface.
    //
    // Later layers (upsert no-op, fact emission, multi-version
    // 6c augmentation stitching, etc.) write through these methods.
    // ──────────────────────────────────────────────────────────────────

    /// Strict lookup by full content-addressed key.
    #[must_use]
    pub fn get_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let v = self.artifacts.get(key).map(|v| v.clone());
        if v.is_some() {
            self.bump_hit_counter(key);
        }
        v
    }

    /// Look up the latest `FileArtifacts` payload for `canonical`
    /// regardless of the other key dimensions.
    #[must_use]
    pub fn get_artifacts_any(&self, canonical: &str) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        for entry in self.artifacts.iter() {
            if entry.key().canonical.as_ref() == canonical {
                let matched_key = entry.key().clone();
                let value = entry.value().clone();
                drop(entry);
                self.bump_hit_counter(&matched_key);
                return Some(value);
            }
        }
        None
    }

    /// Return the content hash of the latest cached artifacts entry
    /// for `canonical`, or `None` if no artifact has been ingested yet.
    ///
    /// Used by [`crate::session_view::SessionView`] impls to surface
    /// the content hash that backs the cached parse artifacts. The
    /// returned hash is the `content_hash` dimension of whichever
    /// `FileArtifactKey` matches `canonical` — concurrent entries
    /// under different `parse_env_hash` collapse to the same
    /// content hash here.
    #[must_use]
    pub fn content_hash_for_canonical(&self, canonical: &str) -> Option<Hash16> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        for entry in self.artifacts.iter() {
            if entry.key().canonical.as_ref() == canonical {
                return Some(entry.key().content_hash);
            }
        }
        None
    }

    /// Alias of [`Self::get_artifacts_any`] used by
    /// [`crate::session_view::SessionView`] impls. Exists as a named
    /// accessor so the session-view read path stays explicit about
    /// "latest artifact for this canonical" semantics; version-aware
    /// variants live alongside this helper.
    #[must_use]
    pub fn latest_artifacts_for_canonical(&self, canonical: &str) -> Option<Arc<FileArtifacts>> {
        self.get_artifacts_any(canonical)
    }

    /// Insert (or replace) the payload for `key`.
    pub fn insert_artifacts(
        &self,
        key: FileArtifactKey,
        artifacts: Arc<FileArtifacts>,
    ) -> Option<Arc<FileArtifacts>> {
        let canonical = Arc::clone(&key.canonical);
        let content_hash = key.content_hash;
        let parse_env_hash = key.parse_env_hash;
        let tick = self.access_tick.fetch_add(1, Ordering::Relaxed) + 1;
        self.last_access.insert(Arc::clone(&canonical), tick);
        // Capture the registry handle BEFORE moving `artifacts` into
        // the DashMap. Cold-path: this is a cheap Arc clone over
        // the public `facts: Arc<FileFacts>` field.
        let facts_for_emit: Arc<FileFacts> = Arc::clone(&artifacts.facts);
        let prev = self.artifacts.insert(key, artifacts);
        let is_fresh = prev.is_none();
        if is_fresh {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        // R23 typed event: a `FileArtifactStore` entry was admitted.
        // Best-effort emission — silent no-op when no observer
        // accumulator is installed on the current thread.
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                canonical_id: Arc::clone(&canonical),
                action: verter_audit::FileArtifactCacheAction::Admit,
                content_hash,
                parse_env_hash,
                entry_count_after: self.artifacts.len() as u32,
            },
        );
        // R23 typed event: emit one `FactRegistryWrite` per parse-
        // domain fact admitted to the registry. Cold-path / fresh-
        // insert only — a replacement insert (`prev.is_some()`) does
        // NOT re-emit (the registry was already attributed on its
        // original parse-time admit). The emission attributes each
        // fact to the canonical id whose registry it lives in, so
        // downstream telemetry can fold registry writes per file.
        if is_fresh {
            for (_key, fact) in facts_for_emit.registry().iter() {
                emit_fact_registry_writes(&canonical, fact);
            }
        }
        prev
    }

    /// Remove the artifact under `key`.
    pub fn remove_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        let canonical = Arc::clone(&key.canonical);
        let content_hash = key.content_hash;
        let parse_env_hash = key.parse_env_hash;
        let removed = self.artifacts.remove(key).map(|(_, v)| v);
        if removed.is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            self.hit_counters.remove(key);
            // R23 typed event: a `FileArtifactStore` entry was
            // evicted. Best-effort emission.
            crate::host_manage::push_structured_event(
                crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                    canonical_id: canonical,
                    action: verter_audit::FileArtifactCacheAction::Evict,
                    content_hash,
                    parse_env_hash,
                    entry_count_after: self.artifacts.len() as u32,
                },
            );
        }
        removed
    }

    /// Drain every entry whose canonical matches `canonical_id`.
    pub fn remove_canonical(&self, canonical_id: &str) -> usize {
        let mut removed_keys: Vec<FileArtifactKey> = Vec::new();
        self.artifacts.retain(|key, _| {
            if key.canonical.as_ref() == canonical_id {
                removed_keys.push(key.clone());
                false
            } else {
                true
            }
        });
        let removed = removed_keys.len();
        if removed > 0 {
            self.live_counter
                .fetch_sub(removed as u64, Ordering::Relaxed);
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
            // R23 typed event: each eviction emits one event so
            // downstream telemetry can attribute drain footprint
            // per `FileArtifactKey` dimension.
            for key in removed_keys {
                self.hit_counters.remove(&key);
                crate::host_manage::push_structured_event(
                    crate::component_meta_audit::StructuredAuditEvent::FileArtifactCache {
                        canonical_id: Arc::clone(&key.canonical),
                        action: verter_audit::FileArtifactCacheAction::Evict,
                        content_hash: key.content_hash,
                        parse_env_hash: key.parse_env_hash,
                        entry_count_after: self.artifacts.len() as u32,
                    },
                );
            }
        }
        removed
    }

    /// Snapshot every key, for diagnostics / reachability sweeps.
    #[must_use]
    pub fn artifact_keys(&self) -> Vec<FileArtifactKey> {
        self.artifacts
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Snapshot every `(key, payload)` pair.
    #[must_use]
    pub fn snapshot_artifacts(&self) -> Vec<(FileArtifactKey, Arc<FileArtifacts>)> {
        self.artifacts
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    // ──────────────────────────────────────────────────────────────────
    // Augmentation index API (populated lazily by the
    // augmentation-stitching pass — see `/type-cache-architecture` skill)
    // ──────────────────────────────────────────────────────────────────

    /// Look up the [`AugmenterSet`] for an [`AugmentationTargetKey`].
    #[must_use]
    pub fn get_augmenter_set(&self, key: &AugmentationTargetKey) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index.get(key).map(|v| v.clone())
    }

    /// Install (or replace) the augmenter set under `key`. Used by
    /// the index-population path.
    pub fn populate_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index.insert(key, set)
    }

    /// Lazily populate the augmentation-index entry for `key`, then
    /// return the `AugmenterSet`.
    ///
    /// On a warm hit, returns the cached `Arc<AugmenterSet>` directly
    /// without re-scanning. On a cold miss, scans `self.artifacts` for
    /// files whose `FileArtifacts.augmentations` match the queried
    /// target under the current `(project_identity, resolve_env_hash,
    /// lib_env_hash)`, sorts the matched augmenters by
    /// `(augmenter_canonical, parse_stable_hash)`, computes a stable
    /// fingerprint, inserts, and emits a `ModuleAugmentationIndexShape`
    /// audit event recording the install (or refresh on
    /// re-population). Subsequent reads under the same key are
    /// warm-hit fast paths.
    ///
    /// Index population is incremental per R29 — only files that have
    /// entered `FileArtifactStore` contribute. There is no
    /// workspace-wide eager scan; out-of-program files contribute
    /// nothing (matches TypeScript's own augmentation-visibility
    /// rule).
    ///
    /// `resolve_relative_canonical` is a caller-supplied hook that
    /// resolves a `(augmenter_canonical, relative_specifier)` pair to
    /// a canonical when the queried target is `ResolvedRelativeCanonical`.
    /// `None` means the augmenter's specifier did not resolve to the
    /// queried canonical and the augmenter is skipped.
    pub fn ensure_augmentation_index_populated<R>(
        &self,
        key: &AugmentationTargetKey,
        resolve_relative_canonical: R,
    ) -> Arc<AugmenterSet>
    where
        R: Fn(&str, &str) -> Option<Arc<str>>,
    {
        if let Some(existing) = self.augmentation_index.get(key) {
            return existing.clone();
        }

        // Cold scan — collect (canonical, parse_stable_hash) for
        // every artifact whose augmentations include at least one
        // matching `ModuleAugmentationFact` for the queried target.
        // Dedup by canonical so a file with multiple matching facts
        // contributes only once.
        let mut matched: Vec<(Arc<str>, Hash16)> = Vec::new();
        let mut seen_canonicals: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        for artifact_entry in self.artifacts.iter() {
            let augmenter_canonical = Arc::clone(&artifact_entry.key().canonical);
            let artifacts: &FileArtifacts = artifact_entry.value();
            for fact in artifacts.augmentations.iter() {
                if augmenter_matches_target(
                    fact,
                    key,
                    augmenter_canonical.as_ref(),
                    &resolve_relative_canonical,
                ) {
                    if seen_canonicals.insert(Arc::clone(&augmenter_canonical)) {
                        matched.push((augmenter_canonical.clone(), artifacts.parse_stable_hash));
                    }
                    break;
                }
            }
        }

        // Sort by (canonical, parse_stable_hash) for determinism.
        matched.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()).then_with(|| a.1.cmp(&b.1)));

        let augmenter_count = matched.len() as u32;
        let fingerprint = compute_augmenter_set_fingerprint(&matched);
        let entries: SmallVec<[(Arc<str>, Hash16); 2]> = matched.into_iter().collect();
        let set = Arc::new(AugmenterSet {
            entries,
            fingerprint,
        });

        // Insert. Capture prev fingerprint for audit event.
        let prev = self
            .augmentation_index
            .insert(key.clone(), Arc::clone(&set));
        let prev_fingerprint = prev.as_ref().map(|p| p.fingerprint);

        // Emit `ModuleAugmentationIndexShape` typed audit event.
        emit_module_augmentation_index_shape_event(
            key,
            prev_fingerprint,
            fingerprint,
            augmenter_count,
        );

        set
    }

    /// Refresh existing augmentation-index entries that a newly-
    /// inserted file's augmentations may contribute to.
    ///
    /// Walks every existing `AugmentationTargetKey`, checks whether
    /// the new artifact's augmentations match the target under the
    /// caller's resolver hook, recomputes the augmenter set when it
    /// does, and emits a refresh audit event when the fingerprint
    /// changes. Existing entries that the new file does NOT contribute
    /// to are left untouched — out-of-program files contribute
    /// nothing.
    pub fn refresh_augmentation_index_for_canonical<R>(
        &self,
        new_artifact_key: &FileArtifactKey,
        new_artifacts: &FileArtifacts,
        resolve_relative_canonical: R,
    ) where
        R: Fn(&str, &str) -> Option<Arc<str>>,
    {
        if new_artifacts.augmentations.is_empty() {
            return;
        }

        // Snapshot existing keys so we don't hold a shard read-lock
        // while we recompute + insert (DashMap re-entrance hazard).
        let existing_keys: Vec<AugmentationTargetKey> = self
            .augmentation_index
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        for key in existing_keys {
            // Does the new artifact contribute to this key? If not,
            // skip — the cached entry is still valid (no augmenter
            // set change for this target).
            let augmenter_canonical = new_artifact_key.canonical.as_ref();
            let contributes = new_artifacts.augmentations.iter().any(|fact| {
                augmenter_matches_target(
                    fact,
                    &key,
                    augmenter_canonical,
                    &resolve_relative_canonical,
                )
            });
            if !contributes {
                continue;
            }

            // Rebuild the set with the new artifact folded in.
            let mut matched: Vec<(Arc<str>, Hash16)> = Vec::new();
            let mut seen_canonicals: rustc_hash::FxHashSet<Arc<str>> =
                rustc_hash::FxHashSet::default();
            for artifact_entry in self.artifacts.iter() {
                let augmenter_canon = Arc::clone(&artifact_entry.key().canonical);
                let artifacts: &FileArtifacts = artifact_entry.value();
                for fact in artifacts.augmentations.iter() {
                    if augmenter_matches_target(
                        fact,
                        &key,
                        augmenter_canon.as_ref(),
                        &resolve_relative_canonical,
                    ) {
                        if seen_canonicals.insert(Arc::clone(&augmenter_canon)) {
                            matched.push((augmenter_canon.clone(), artifacts.parse_stable_hash));
                        }
                        break;
                    }
                }
            }
            matched.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()).then_with(|| a.1.cmp(&b.1)));
            let augmenter_count = matched.len() as u32;
            let new_fingerprint = compute_augmenter_set_fingerprint(&matched);

            // Compare with old fingerprint. If unchanged, skip the
            // insert + event emission.
            let old = self.augmentation_index.get(&key).map(|e| e.fingerprint);
            if old == Some(new_fingerprint) {
                continue;
            }

            let entries: SmallVec<[(Arc<str>, Hash16); 2]> = matched.into_iter().collect();
            let new_set = Arc::new(AugmenterSet {
                entries,
                fingerprint: new_fingerprint,
            });
            self.augmentation_index.insert(key.clone(), new_set);

            emit_module_augmentation_index_shape_event(&key, old, new_fingerprint, augmenter_count);
        }
    }

    /// Drop every entry from the augmentation index.
    pub fn clear_augmentation_index(&self) {
        self.augmentation_index.clear();
    }

    /// Number of entries in the augmentation index.
    #[must_use]
    pub fn augmentation_index_len(&self) -> usize {
        self.augmentation_index.len()
    }

    /// Snapshot every `(AugmentationTargetKey, fingerprint)` pair in
    /// the augmentation index. Used by `HostStoreView::build` to copy
    /// the route-surface-domain fingerprints into the view for
    /// per-candidate validation (R29 + G1 + R26).
    #[must_use]
    pub fn snapshot_augmentation_index_fingerprints(&self) -> Vec<(AugmentationTargetKey, Hash16)> {
        self.augmentation_index
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().fingerprint))
            .collect()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for FileArtifactStore {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        let count = self.artifacts.len();
        self.artifacts.clear();
        self.last_access.clear();
        self.augmentation_index.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        }
        count
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for FileArtifactStore {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent]
    }
    fn invalidate(&self, _domain: crate::invalidation_domain::InvalidationDomain) {
        // FileArtifacts survives project-generation bumps (content_hash is
        // sufficient identity); per-canonical eviction is the only
        // invalidation mode.
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for FileArtifactStore {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.len();
        self.remove(canonical_id);
        let after = self.len();
        before.saturating_sub(after)
    }
}

/// Special marker the parse-domain emission uses for `declare global
/// { ... }` blocks (see `fact_emission::GLOBAL_AUGMENTATION_TAG`).
/// Duplicated here to keep the matcher free-standing of fact_emission.
const GLOBAL_AUGMENTATION_TAG: &str = "$global";

/// Does `fact` (emitted by `augmenter_canonical`) contribute to the
/// queried `target_key`?
///
/// Classification semantics by target-kind archetype:
///
/// - `ExternalSpecifier(s)` → match `fact.specifier == s` AND the
///   specifier is NOT relative, NOT a wildcard, NOT the global tag.
/// - `ResolvedRelativeCanonical(canon)` → match relative specifiers
///   (start with `./` or `../`) whose `resolve_relative_canonical`
///   resolves equal to `canon`.
/// - `WildcardAmbient(pattern)` → match `fact.specifier == pattern`
///   AND the specifier contains a wildcard `*`.
/// - `GlobalAugmentation` → match `fact.specifier == "$global"`.
pub(crate) fn augmenter_matches_target<R>(
    fact: &ModuleAugmentationFact,
    target_key: &AugmentationTargetKey,
    augmenter_canonical: &str,
    resolve_relative_canonical: R,
) -> bool
where
    R: Fn(&str, &str) -> Option<Arc<str>>,
{
    let specifier: &str = fact.specifier.as_ref();
    match &target_key.target {
        AugmentationTargetKind::ExternalSpecifier(target_spec) => {
            // Bare external: not relative, not wildcard, not global.
            let is_relative = specifier.starts_with("./") || specifier.starts_with("../");
            let is_wildcard = specifier.contains('*');
            let is_global = specifier == GLOBAL_AUGMENTATION_TAG;
            !is_relative && !is_wildcard && !is_global && specifier == target_spec.as_ref()
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(target_canon) => {
            if !(specifier.starts_with("./") || specifier.starts_with("../")) {
                return false;
            }
            match resolve_relative_canonical(augmenter_canonical, specifier) {
                Some(resolved) => resolved.as_ref() == target_canon.as_ref(),
                None => false,
            }
        }
        AugmentationTargetKind::WildcardAmbient(target_pattern) => {
            specifier.contains('*') && specifier == target_pattern.as_ref()
        }
        AugmentationTargetKind::GlobalAugmentation => specifier == GLOBAL_AUGMENTATION_TAG,
    }
}

/// Compute the stable `AugmenterSet.fingerprint` over the sorted
/// `[(augmenter_canonical, parse_stable_hash)]` list.
///
/// Two FxHasher passes seeded with distinct salts produce a 16-byte
/// hash with low collision risk. Matches the cheap-hash pattern used
/// by `resolver_core::compute_signature_fingerprint`.
pub(crate) fn compute_augmenter_set_fingerprint(entries: &[(Arc<str>, Hash16)]) -> Hash16 {
    use std::hash::{BuildHasher, Hasher};
    let salt_lo = rustc_hash::FxBuildHasher;
    let salt_hi = rustc_hash::FxBuildHasher;
    let mut h_lo = salt_lo.build_hasher();
    let mut h_hi = salt_hi.build_hasher();
    h_lo.write_u64(0xC4A1_C4A1_4A1C_4A1C);
    h_hi.write_u64(0x9E37_79B9_7F4A_7C15);
    for (canon, hash) in entries {
        h_lo.write(canon.as_bytes());
        h_lo.write(hash);
        h_hi.write(canon.as_bytes());
        h_hi.write(hash);
    }
    let lo = h_lo.finish();
    let hi = h_hi.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

/// Emit a typed `StructuredAuditEvent::ModuleAugmentationIndexShape`
/// event recording an install / refresh of the augmentation-index
/// entry. Silent no-op when no audit accumulator is installed.
pub(crate) fn emit_module_augmentation_index_shape_event(
    key: &AugmentationTargetKey,
    prev_fingerprint: Option<Hash16>,
    new_fingerprint: Hash16,
    augmenter_count: u32,
) {
    use verter_audit::AugmentationTargetKindTag;
    let (tag, external_specifier, resolved_relative_canonical, wildcard_pattern) = match &key.target
    {
        AugmentationTargetKind::ExternalSpecifier(spec) => (
            AugmentationTargetKindTag::ExternalSpecifier,
            Some(Arc::<str>::from(spec.as_ref())),
            None,
            None,
        ),
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => (
            AugmentationTargetKindTag::ResolvedRelativeCanonical,
            None,
            Some(Arc::clone(canon)),
            None,
        ),
        AugmentationTargetKind::WildcardAmbient(pat) => (
            AugmentationTargetKindTag::WildcardAmbient,
            None,
            None,
            Some(Arc::<str>::from(pat.as_ref())),
        ),
        AugmentationTargetKind::GlobalAugmentation => (
            AugmentationTargetKindTag::GlobalAugmentation,
            None,
            None,
            None,
        ),
    };
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::ModuleAugmentationIndexShape {
            target_kind_tag: tag,
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
            prev_fingerprint,
            new_fingerprint,
            augmenter_count,
        },
    );
}

/// Translate a parse-domain [`fact_registry::FactKey`] to its
/// audit-side discriminator [`verter_audit::FactKeyKindTag`].
///
/// Pure translation — does not mint state. Only the parse-domain
/// `FactKey` variants are mirrored on the audit side; the
/// resolve-imports and route-surface domain keys flow through the
/// parallel `ResolvedImportFacts` / `RouteDb` admission paths and
/// emit their own typed events.
pub(crate) fn fact_key_kind_tag_for(key: &fact_registry::FactKey) -> verter_audit::FactKeyKindTag {
    use fact_registry::FactKey;
    use verter_audit::FactKeyKindTag;
    match key {
        FactKey::Export { .. } => FactKeyKindTag::Export,
        FactKey::ExportAlias { .. } => FactKeyKindTag::ExportAlias,
        FactKey::SyntacticExportSet => FactKeyKindTag::SyntacticExportSet,
        FactKey::LocalDecl { .. } => FactKeyKindTag::LocalDecl,
        FactKey::Member { .. } => FactKeyKindTag::Member,
        FactKey::MemberPresence { .. } => FactKeyKindTag::MemberPresence,
        FactKey::MemberShape { .. } => FactKeyKindTag::MemberShape,
        FactKey::MacroSurface { .. } => FactKeyKindTag::MacroSurface,
        FactKey::TemplateRoot => FactKeyKindTag::TemplateRoot,
        FactKey::ImportRef { .. } => FactKeyKindTag::ImportRef,
        FactKey::SyntacticReexportRef { .. } => FactKeyKindTag::SyntacticReexportRef,
        FactKey::ModuleAugmentation { .. } => FactKeyKindTag::ModuleAugmentation,
        // Resolve-imports + route-surface domain keys live on the
        // parallel `ResolvedImportFacts` / `RouteDb` admission paths
        // and emit their own typed events. They are not admitted to
        // the `FileFacts.registry` parse-domain inventory, so the
        // unreachable arm flags a producer error if we ever do.
        FactKey::ResolvedImportClause { .. }
        | FactKey::ResolvedReexportBinding { .. }
        | FactKey::EffectiveExportSet
        | FactKey::ModuleAugmentationIndexShape { .. } => FactKeyKindTag::SyntacticExportSet,
    }
}

/// Emit one typed `StructuredAuditEvent::FactRegistryWrite` per fact
/// admitted to the per-file `FactRegistry` (R10, R11). Cold-path /
/// parse-time only — fires once per fact at the point the host-side
/// `FileArtifactStore` admits a fresh `FileArtifacts` payload.
///
/// The `lane` field is set to `Semantic` (the dominant caching
/// dimension); the parallel `semantic_hash` and `display_hash`
/// fields carry both lane hashes simultaneously.
fn emit_fact_registry_writes(canonical_id: &Arc<str>, fact: &fact_registry::Fact) {
    crate::host_manage::push_structured_event(
        crate::component_meta_audit::StructuredAuditEvent::FactRegistryWrite {
            canonical_id: Arc::clone(canonical_id),
            fact_key_kind: fact_key_kind_tag_for(&fact.key),
            lane: verter_audit::FactLaneTag::Semantic,
            semantic_hash: fact.semantic_hash,
            display_hash: fact.display_hash,
        },
    );
}

#[cfg(test)]
#[path = "file_artifact_store_tests.rs"]
mod file_artifact_store_tests;
