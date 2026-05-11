//! Stage 1 — `FileArtifactStore`: the canonical per-file artifact cache.
//!
//! `FileArtifactStore` is the **authoritative** post-parse cache. It replaces
//! the legacy `IndexedReadyDb` type as the single per-file storage layer on
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
//!   at Stage 3).
//! - [`ParsedEdges`] — content-addressed import-edge facts published per
//!   file version (populated at Stage 9 when the workspace-edge map becomes
//!   idempotent on the env-hash quintuple).
//! - [`ModuleAugmentationFact`] — per-file syntactic augmentation fact
//!   (populated at Stage 3; Stage 1 leaves the vector empty).
//!
//! ## Two API surfaces
//!
//! The store exposes both:
//!
//! 1. **Canonical-keyed legacy surface** — matches the retired
//!    `IndexedReadyDb` shape (`get(canonical, hash)`, `insert(canonical,
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
//!   under a canonical (this path is retired at Stage 7).
//! - **`parse_stable_hash`:** alpha-normalised structural hash over the
//!   file's post-shallow-analysis decl skeleton. Invariant under cosmetic
//!   edits.
//! - **`augmentation_index` populated lazily at Stage 6c:** Stage 1 owns
//!   the skeleton + accessor API.
//!
//! See `/type-cache-architecture` skill for the full rule set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use smallvec::SmallVec;
use verter_semantic::analysis::Hash16;

use crate::project_type_store::IndexedReady;

// ── Project identity wrapper ──

/// Thin newtype around the 16-byte `project_identity` value produced by
/// `IdeProjectConfig::project_identity()`.
///
/// Used as a key dimension on [`AugmentationTargetKey`] to keep
/// augmentation entries from one project from poisoning a sibling
/// project under the same syntactic specifier (Codex P0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectIdentity(pub Hash16);

// ── Interned strings ──

/// An interned module-specifier string (e.g. `"vue"`, `"./local"`,
/// `"*.css"`). Stage 1 wraps an `Arc<str>` so the type is movable into
/// data structures without back-references; later stages can swap in a
/// crate-wide interner if profiling shows hot-path duplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedSpecifier(pub Arc<str>);

impl From<&str> for InternedSpecifier {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

/// An interned symbol name (export name, member name, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedName(pub Arc<str>);

impl From<&str> for InternedName {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

/// An interned wildcard pattern (e.g. `"*.css"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedGlobPattern(pub Arc<str>);

impl From<&str> for InternedGlobPattern {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

// ── Symbol space ──

/// The TypeScript / Verter symbol space a binding occupies. A `class Foo`
/// declaration occupies BOTH `Type` and `Value` and emits two distinct
/// facts (R11) — `BothTypeValue` is forbidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolSpace {
    Type,
    Value,
    Namespace,
}

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
}

/// Parser version for legacy-shape inserts. Bumps invalidate every
/// entry the legacy surface inserted under
/// [`FileArtifactKey::legacy`].
pub const LEGACY_PARSER_VERSION: u32 = 1;

/// `parse_env_hash` sentinel used by the canonical-keyed legacy surface
/// before later stages plumb the real env hash through every call site.
pub const LEGACY_PARSE_ENV_HASH: Hash16 = [0u8; 16];

// ── FileFacts placeholder ──

/// Per-file fact registry payload.
///
/// Stage 1 placeholder: holds an empty registry. Stage 3 populates this
/// with the parse-domain facts (`Export`, `LocalDecl`, `MemberShape`,
/// `MemberPresence`, `SyntacticExportSet`, `MacroSurface`, `TemplateRoot`,
/// `ImportRef`, `SyntacticReexportRef`, `ModuleAugmentation`) emitted
/// during shallow analysis (R10–R16, R28, R29).
#[derive(Debug, Default, Clone)]
pub struct FileFacts {
    // Stage 3 populates a FactKey-indexed registry here.
    _stage1_placeholder: (),
}

impl FileFacts {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

// ── ParsedEdges ──

/// Content-addressed parsed import-edge facts for a single file version.
///
/// Stage 1 placeholder: Stage 9 makes the workspace-edge map idempotent
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
/// Stage 1 defines the type; Stage 3 populates it from the shallow walk.
/// Stage 6c stitches the augmenting declarations into the consumer's
/// `EffectiveExportSet` for that specifier.
///
/// Fields:
///
/// - `specifier` — the syntactic specifier inside `declare module "X" {}`.
/// - `augmented_name` — the name of an augmented binding inside the block.
/// - `space` — which symbol space the augmented binding occupies.
/// - `augmented_member_shape_fingerprint` — alpha-normalised fingerprint
///   over the augmenting block's member set; used by Stage 6c to detect
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
/// fact-registry / parsed-edges / augmentation containers that Stage 3 +
/// Stage 9 will populate.
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
    #[must_use]
    pub fn with_indexed(indexed: Arc<IndexedReady>) -> Self {
        let parse_stable_hash = crate::parse_stable_hash::compute_parse_stable_hash(&indexed);
        Self {
            indexed,
            facts: Arc::new(FileFacts::empty()),
            parsed_edges: Arc::new(ParsedEdges::empty()),
            parse_stable_hash,
            augmentations: Arc::new(Vec::new()),
        }
    }
}

// ── FileArtifactStore ──

/// The per-host content-addressed file-artifact cache.
///
/// Replaces the retired `IndexedReadyDb` type as the authoritative
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
            live_counter: live,
            stale_sweeps: stale,
            schema_version,
            augmentation_index: DashMap::new(),
            #[cfg(test)]
            test_audit_hook: parking_lot::Mutex::new(None),
        }
    }

    /// Install the host-level test audit hook (legacy `IndexedReadyDb`
    /// equivalent).
    #[cfg(test)]
    pub(crate) fn install_test_audit_hook(
        &self,
        state: Arc<crate::host_test_audit::HostTestAuditState>,
    ) {
        *self.test_audit_hook.lock() = Some(state);
    }

    // ──────────────────────────────────────────────────────────────────
    // Legacy `IndexedReadyDb` API surface
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
        for entry in self.artifacts.iter() {
            if entry.key().canonical.as_ref() == canonical_id {
                result = Some(Arc::clone(&entry.value().indexed));
                break;
            }
        }
        if result.is_some() {
            self.bump_access_tick(canonical_id);
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
    pub fn evict_lru(&self, min_floor: usize) {
        let len = self.artifacts.len();
        if len <= min_floor {
            return;
        }
        let drop_count = len - min_floor;
        let mut tick_pairs: Vec<(FileArtifactKey, u64)> = self
            .artifacts
            .iter()
            .map(|entry| {
                let key = entry.key().clone();
                let tick = self
                    .last_access
                    .get(&key.canonical)
                    .map(|t| *t.value())
                    .unwrap_or(0);
                (key, tick)
            })
            .collect();
        tick_pairs.sort_by_key(|(_, tick)| *tick);
        for (key, _) in tick_pairs.into_iter().take(drop_count) {
            if self.artifacts.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            }
            self.last_access.remove(&key.canonical);
        }
    }

    /// Snapshot every live entry for auditing / diagnostics, in
    /// `(canonical, indexed)` shape (matches the legacy `IndexedReadyDb`
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
    /// the same canonical are overwritten — the legacy `IndexedReadyDb`
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
            // IndexedReadyDb::insert behaviour).
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
    /// (legacy `IndexedReadyDb::remove` semantics).
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
    // Later stages (Stage 2 `upsert` no-op, Stage 3 fact emission, Stage
    // 6c augmentation stitching, etc.) write through these methods.
    // ──────────────────────────────────────────────────────────────────

    /// Strict lookup by full content-addressed key.
    #[must_use]
    pub fn get_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        self.artifacts.get(key).map(|v| v.clone())
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
                return Some(entry.value().clone());
            }
        }
        None
    }

    /// Insert (or replace) the payload for `key`.
    pub fn insert_artifacts(
        &self,
        key: FileArtifactKey,
        artifacts: Arc<FileArtifacts>,
    ) -> Option<Arc<FileArtifacts>> {
        let canonical = Arc::clone(&key.canonical);
        let tick = self.access_tick.fetch_add(1, Ordering::Relaxed) + 1;
        self.last_access.insert(Arc::clone(&canonical), tick);
        let prev = self.artifacts.insert(key, artifacts);
        if prev.is_some() {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
        prev
    }

    /// Remove the artifact under `key`.
    pub fn remove_artifacts(&self, key: &FileArtifactKey) -> Option<Arc<FileArtifacts>> {
        let removed = self.artifacts.remove(key).map(|(_, v)| v);
        if removed.is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        removed
    }

    /// Drain every entry whose canonical matches `canonical_id`.
    pub fn remove_canonical(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        self.artifacts.retain(|key, _| {
            if key.canonical.as_ref() == canonical_id {
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
    /// Stage 6c during index population.
    pub fn populate_augmenter_set(
        &self,
        key: AugmentationTargetKey,
        set: Arc<AugmenterSet>,
    ) -> Option<Arc<AugmenterSet>> {
        self.augmentation_index.insert(key, set)
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

#[cfg(test)]
#[path = "file_artifact_store_tests.rs"]
mod file_artifact_store_tests;
