//! Host-owned typed DB wrappers for the 10 component-meta caches that
//! were previously authoritative inside `ComponentMetaQueryEngine`.
//!
//! ## Architecture
//!
//! Each typed `*Db` owns storage only. Request-local `MemoRead` and
//! `MemoPublish` capabilities in `project_semantic_dispatch::memo` select its
//! storage attachment and perform validation, tracing and cold-build work
//! through the cache runtime. The cooperative singleflight protocol
//! (one-winner cold build, cooperative joiner waits, panic safety and
//! post-compute revalidation) stays cache-runtime-internal. The storage
//! attachments split into two families:
//!
//! - **Single-entry artifact caches** — a `DashMap<Key, Arc<CacheEntry>>`
//!   plus a per-cache `InflightTable<QueryFlightKey<Key>>`. Their
//!   publication capability builds a request-local artifact node over the
//!   map / flight table / live counter and routes through
//!   [`crate::cache_runtime::node::lookup`]. The node's winner-only
//!   `post_publish` / `removal_cleanup` hooks keep the shared
//!   `component_meta_cache_live` counter in step with the live map.
//! - **Reverse-indexed multi-candidate query-identity cache** —
//!   `ImportedRegistryDb`. It wraps a shared
//!   [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore)
//!   plus a per-cache `InflightTable<QueryFlightKey<Key>>`, and its
//!   publication capability builds a request-local query-candidate node and
//!   routes through
//!   [`crate::cache_runtime::node::query::lookup`]. The producer's cold
//!   `compute` returns a
//!   [`ComputeAdmission`](crate::cache_runtime::singleflight::ComputeAdmission)
//!   so a valid-but-non-cacheable outcome (`ReturnOnly`) returns to the
//!   winning flight alone without admitting a candidate — concurrent
//!   joiners cannot view-validate it and instead fork and cold-recompute
//!   for their own view. Storage (the slot install, the live-counter
//!   net-bump, the reverse-index registration, the retention admission,
//!   and the deferred FIFO eviction) is the store's, driven through the
//!   node's `publish_core` / `evict_deferred` / `publish_fence` /
//!   `lookup_candidate` under the split publish lifecycle.
//!
//! ## Live-counter accounting invariant
//!
//! The shared `component_meta_cache_live` counter must equal the number
//! of entries / candidates actually live across the cache maps and stores
//! on EVERY admission path. The single-entry caches bump it in the node's
//! winner-only `post_publish` (fired exactly once, after the map insert
//! and a successful post-compute revalidation) and decrement it in
//! `removal_cleanup`; the increment is never placed in the `compute`
//! closure, so a revalidation-fail cold build (a project-generation reset
//! landed during the cold window) publishes no entry and leaks no count.
//! The query-identity stores net-bump the counter under the slot guard in
//! `publish_core` and decrement it in every store removal path
//! (per-canonical drain, deferred FIFO victim, project-generation
//! `clear`, schema eviction), so a stale candidate skipped on read is not
//! reaped on the read path and the counter still tracks live candidates.
//! Read-side validation (`validate` / `lookup_candidate` and the
//! post-compute revalidation) rejects an entry / candidate whose
//! `read_set_signature.facts` no longer validate against the live
//! `StoreView`. Per-canonical and project-generation invalidation hooks
//! are wired into [`ProjectTypeStore::evict_canonical`] and
//! [`ProjectTypeStore::bump_project_generation_and_evict`].
//!
//! ## D3.5 — `Arc<str>` / `Arc<TypeExpr>` keys
//!
//! Cache keys use `Arc<str>` for wide-string fields per D3.5. Cloning a
//! key is a cheap refcount bump rather than a heap allocation + copy.
//!
//! ## Engine read-through views
//!
//! `ComponentMetaQueryEngine` keeps a per-request `RefCell<FxHashMap>`
//! mirror of each DB so repeated lookups in one request hit the local
//! mirror first. Per the D3.2 contract, the mirror is **non-authoritative
//! scratch only** — it never inserts entries the host DB doesn't have,
//! never holds an independent dep-signature, and never invalidates
//! independently. The mirror clears on engine drop (per-request scope).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
#[cfg(any(test, feature = "test-support"))]
use verter_type_expr::TypeExpr;

use crate::cache_runtime::admission::{CacheEntry, NonAdmissionReason};
use crate::cache_runtime::node::QueryFlightKey;
use crate::cache_runtime::singleflight::InflightTable;
use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::ResolvedTypeDeclaration;
use verter_session_query::facts::fact_cache::FactVersionRef;
#[cfg(any(test, feature = "test-support"))]
use verter_session_query::facts::fact_cache::ReadSetSignature;
// `ProjectionMode` is referenced only by the mode-only key constructors and
// the schema-probe helpers, all gated `cfg(any(test, feature = "test-support"))`;
// gate the import to match so release does not see it unused.
#[cfg(any(test, feature = "test-support"))]
use crate::semantic_query::ProjectionMode;

// ===========================================================================
// Shared single-entry artifact node
// ===========================================================================
//
// The single-entry caches below (`DeclarationLookupDb`, `ResolvabilityDb`,
// `OwnerCollectionDb`, `ShapeCacheDb`) all
// store one entry per key validated by a path-precise fact signature
// plus a generation gate, and all bump a shared live counter on publish
// / decrement it on removal. They are identical modulo the key type, the
// value type, and the self-root derivation, so they share ONE
// [`ArtifactNode`] implementation rather than repeating the cooperative-
// admission closure plumbing per cache.
//
// The request facade's `MemoPublish` selects the passive cache attachment and
// constructs its cold artifact adapter with the per-call computation. It uses
// the existing [`crate::cache_runtime::node::lookup`] protocol; the durable DB
// retains no compute closure or request authority. The stored value is the domain
// value; every validity rail (the fact signature, the self-root
// canonicals, the compute-time generation) lives in
// [`crate::cache_runtime::CacheEntry`].

/// What a producer's cold build actually produced, BEFORE the cacheability
/// verdict is applied.
///
/// The three arms separate the two things a `None` used to conflate:
///
/// - `Rooted` — a value AND a fact signature that can root it. Admissible if
///   the probe agrees.
/// - `Unrooted` — a value, but nothing to root it with (the signature
///   overflowed, the keyed content version could not be observed, the value is
///   a genuine partial). The value is CORRECT and belongs to the caller; only
///   the WRITE is refused.
/// - `Failed` — no value at all.
///
/// Collapsing `Unrooted` into `Failed` is what produced the discard-and-
/// re-resolve shape: the funnel returned `None`, the producer could not tell
/// "refused" from "failed", and re-ran the resolution it had just completed —
/// paying a second compute with no guarantee the second run reproduces the
/// first.
pub(crate) enum ComputedEntry<V> {
    /// Value plus the path-precise fact signature that roots it.
    Rooted(V, Arc<[FactVersionRef]>),
    /// Value produced, but no signature can root it. Serve it; publish nothing.
    Unrooted(V, NonAdmissionReason),
    /// The cold build produced no value.
    Failed,
}

impl<V> From<Option<(V, Arc<[FactVersionRef]>)>> for ComputedEntry<V> {
    /// Adapt the funnels whose producers hold their own copy of the computed
    /// value (the shape caches: the value is captured before the closure and
    /// returned verbatim on refusal, so a `None` costs no re-compute).
    fn from(computed: Option<(V, Arc<[FactVersionRef]>)>) -> Self {
        match computed {
            Some((value, facts)) => ComputedEntry::Rooted(value, facts),
            None => ComputedEntry::Failed,
        }
    }
}

// ===========================================================================
// 1. ImportedRegistryDb — `(canonical, name) → Option<ResolvedImportedRegistrySymbol>`
// ===========================================================================

#[derive(Clone)]
pub struct ImportedRegistryEntry {
    pub value: Option<Arc<ResolvedImportedRegistrySymbol>>,
    /// R3/R26/R28 fact-precise dependency signature recorded during the
    /// cold-compute pass that produced this entry. Validated on every
    /// warm-hit read — and on post-compute revalidation — against the
    /// producer's current fact registry via
    /// [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`].
    /// The entry's keyed canonical(s) are passed as the self-root set,
    /// so the leading self-root `FileWholeHash` is validated strictly:
    /// a same-canonical content edit, or a keyed canonical untracked by
    /// the live store view, rejects the entry.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
    /// Project generation this entry was computed under, snapshotted by
    /// the producer before the cold compute dispatched any work. The
    /// `fact_dep_signature` carrier validates only file-content
    /// whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias /
    /// SDK / workspace-folder change) bumps no file content, so without
    /// this field a stale-by-project-generation entry would validate
    /// forever. Every request-driver read-side gate (the imported-registry read, the
    /// cooperative `validate` closure, the cooperative
    /// `revalidate_after_compute` closure) rejects the entry when
    /// `validated_at_generation` differs from the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    pub validated_at_generation: u64,
}

pub type ImportedRegistryKey = (Arc<str>, verter_type_expr::TopLevelOwnerId, Arc<str>);

/// The value the imported-registry store holds per candidate.
pub type ImportedRegistryValue = Option<Arc<ResolvedImportedRegistrySymbol>>;

pub struct ImportedRegistryDb {
    /// The shared reverse-indexed multi-candidate store. No retention
    /// budget — the per-slot candidate cap plus the per-canonical
    /// reverse-index drain are the reclamation paths (the keyed canonical
    /// is the entry's self-root, so a content edit invalidates exactly its
    /// own resolved imports).
    store: crate::cache_runtime::ReverseIndexedCandidateStore<
        ImportedRegistryKey,
        ImportedRegistryValue,
    >,
    /// Per-cache flight table keyed by the flight identity (cache key +
    /// store-view compat token) so two overlays on one key do not coalesce.
    inflight: InflightTable<QueryFlightKey<ImportedRegistryKey>>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl ImportedRegistryDb {
    pub(crate) fn attach_output(
        &self,
    ) -> crate::project_semantic_dispatch::memo::CandidateAttachment<
        '_,
        ImportedRegistryKey,
        ImportedRegistryValue,
    > {
        crate::project_semantic_dispatch::memo::CandidateAttachment::new(
            &self.store,
            &self.inflight,
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture<'a, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
        &'a self,
        ctx: &'a dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> crate::project_semantic_dispatch::memo::MemoPublish<'a, Self, W> {
        crate::project_semantic_dispatch::memo::MemoPublish::for_test(self, ctx)
    }

    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::with_counter_and_schema_version(
            live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_and_schema_version(Arc::new(AtomicU64::new(0)), schema_version)
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            store: crate::cache_runtime::ReverseIndexedCandidateStore::with_counter(live_counter),
            inflight: InflightTable::new(),
            schema_version,
        }
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        // Drain via the store's per-canonical reverse index in O(K)
        // (candidates owned by this canonical) instead of O(N) (total
        // candidates). The index is populated on every cold publish, so a
        // content edit on the file invalidates exactly its own resolved
        // imports.
        self.store.invalidate_canonical(canonical_id);
    }

    pub fn invalidate_all(&self) {
        self.store.invalidate_all();
    }

    pub fn live_count(&self) -> usize {
        self.store.live_count()
    }

    /// Test-only: is ANY candidate for `key` currently registered under
    /// its keyed canonical in the store's reverse index? Drives the
    /// reverse-index consistency discriminators — a candidate's
    /// registration must track its slot membership exactly.
    #[cfg(test)]
    pub(crate) fn reverse_index_contains_for_test(&self, key: &ImportedRegistryKey) -> bool {
        self.store
            .reverse_index_contains_key_for_test(key.0.as_ref(), key)
    }

    /// TEST-ONLY: joiner-park witness for the imported-registry inflight
    /// table. Arm before spawning workers.
    #[cfg(test)]
    pub(crate) fn subscribe_joiner_park_for_test(&self) -> std::sync::mpsc::Receiver<()> {
        self.inflight.subscribe_joiner_park()
    }

    /// Test-only direct insertion entry point used by the
    /// invalidation-perf regression test
    /// (`crates/verter_session/tests/cases/g_misc0/invalidation_perf.rs`). Bypasses the
    /// cooperative-admission inflight slot and admits the candidate into
    /// the store (registering its reverse index) identically to the cold
    /// publish path. NOT for use from production code.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_for_test(&self, key: ImportedRegistryKey, entry: Arc<ImportedRegistryEntry>) {
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let signature = verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::clone(
            &entry.fact_dep_signature,
        ));
        self.store.insert_for_test(
            key,
            entry.value.clone(),
            signature,
            self_roots,
            entry.validated_at_generation,
        );
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant. The entry payload is a placeholder;
    /// the fixture only inspects `live_count()` before and after
    /// `evict_if_schema_mismatch()`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let key: ImportedRegistryKey = (
            Arc::from(marker),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("synthetic"),
        );
        let entry = Arc::new(ImportedRegistryEntry {
            value: None,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
            validated_at_generation: 0,
        });
        self.insert_for_test(key, entry);
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for ImportedRegistryDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }

    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        match domain {
            ProjectGeneration => self.invalidate_all(),
            FileContent | ResolverState => {
                // Per-canonical invalidation goes through
                // InvalidationByCanonical. Wholesale invalidation by
                // domain is not used for these domains; they are
                // declared so the cascade can route them to the
                // monomorphic per-canonical path.
            }
            TypeGraph | ComponentMeta | AppConfigInterfaceMerge => {
                // Not declared; ignore.
            }
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for ImportedRegistryDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        self.store.invalidate_canonical(canonical_id)
    }
}

impl Default for ImportedRegistryDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for ImportedRegistryDb {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        self.store.evict_if_schema_mismatch()
    }
}

// ===========================================================================
// 2. DeclarationLookupDb — `(canonical, owner, name) → ResolvedTypeDeclaration`
// ===========================================================================

pub type DeclarationLookupKey = (Arc<str>, verter_type_expr::TopLevelOwnerId, Arc<str>);

pub struct DeclarationLookupDb {
    entries: DashMap<DeclarationLookupKey, Arc<CacheEntry<Arc<ResolvedTypeDeclaration>>>>,
    inflight: InflightTable<QueryFlightKey<DeclarationLookupKey>>,
    live_counter: Arc<AtomicU64>,
}

impl DeclarationLookupDb {
    pub(crate) fn attach_output(
        &self,
    ) -> crate::project_semantic_dispatch::memo::SingleEntryAttachment<
        '_,
        DeclarationLookupKey,
        Arc<ResolvedTypeDeclaration>,
    > {
        crate::project_semantic_dispatch::memo::SingleEntryAttachment::new(
            &self.entries,
            &self.inflight,
            &self.live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture<'a, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
        &'a self,
        ctx: &'a dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> crate::project_semantic_dispatch::memo::MemoPublish<'a, Self, W> {
        crate::project_semantic_dispatch::memo::MemoPublish::for_test(self, ctx)
    }

    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<DeclarationLookupKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for DeclarationLookupDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 3. ResolvabilityDb — `(canonical, owner, name) → bool`
// ===========================================================================

pub type ResolvabilityKey = (Arc<str>, verter_type_expr::TopLevelOwnerId, Arc<str>);

pub struct ResolvabilityDb {
    entries: DashMap<ResolvabilityKey, Arc<CacheEntry<bool>>>,
    inflight: InflightTable<QueryFlightKey<ResolvabilityKey>>,
    live_counter: Arc<AtomicU64>,
}

impl ResolvabilityDb {
    pub(crate) fn attach_output(
        &self,
    ) -> crate::project_semantic_dispatch::memo::SingleEntryAttachment<'_, ResolvabilityKey, bool>
    {
        crate::project_semantic_dispatch::memo::SingleEntryAttachment::new(
            &self.entries,
            &self.inflight,
            &self.live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture<'a, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
        &'a self,
        ctx: &'a dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> crate::project_semantic_dispatch::memo::MemoPublish<'a, Self, W> {
        crate::project_semantic_dispatch::memo::MemoPublish::for_test(self, ctx)
    }

    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<ResolvabilityKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for ResolvabilityDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 4. OwnerCollectionDb — `(canonical, owner, name) → Option<AuthoredBodyLocator>`
//
// Note: keyed solely by name within an owner scope. Since multiple owners
// may collide on the same name with different collection bodies, the entry
// tracks the owner_canonical at insertion time and validates per-canonical
// only.
//
// The VALUE is the content-free AUTHORED BODY LOCATOR of the owner's
// collection declaration — never a stored body. Consumers lower the
// locator on demand through the ONE shared dispatch
// (`raise_authored_locator_to_hot` / `lower_locator`) and read node-domain
// predicates off the lowered node. The key `(owner, name)` and the owner
// self-root are unchanged from the body-bearing era — a VALUE migration
// only, not a key or validity-oracle change.
// ===========================================================================

pub type OwnerCollectionKey = (Arc<str>, verter_type_expr::TopLevelOwnerId, Arc<str>);

pub struct OwnerCollectionDb {
    entries: DashMap<
        OwnerCollectionKey,
        Arc<CacheEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>>,
    >,
    inflight: InflightTable<QueryFlightKey<OwnerCollectionKey>>,
    live_counter: Arc<AtomicU64>,
}

impl OwnerCollectionDb {
    pub(crate) fn attach_output(
        &self,
    ) -> crate::project_semantic_dispatch::memo::SingleEntryAttachment<
        '_,
        OwnerCollectionKey,
        Option<verter_type_expr::locators::AuthoredBodyLocator>,
    > {
        crate::project_semantic_dispatch::memo::SingleEntryAttachment::new(
            &self.entries,
            &self.inflight,
            &self.live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture<'a, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
        &'a self,
        ctx: &'a dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> crate::project_semantic_dispatch::memo::MemoPublish<'a, Self, W> {
        crate::project_semantic_dispatch::memo::MemoPublish::for_test(self, ctx)
    }

    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<OwnerCollectionKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for OwnerCollectionDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 6. ShapeCacheDb — `ShapeCacheKey → MaterializedOutputTypeExpr`
//
// Universal shape cache. Replaces the previously-split
// `MaterializeMemoDb` (TypeExpr-keyed) and `MemberShapeCacheDb`
// (member-value-node-keyed) by lifting the discriminant into a
// `ShapeSubject` variant. ONE cache, not two. Every shape lookup at
// every projection boundary goes through this cache.
//
// The key shape:
//
//   ShapeCacheKey {
//       subject: ShapeSubject,  // TypeExpr (scope+expr) | MemberValueNode
//                               // (scope + sealed MemberShapeNodeSubject) |
//                               // SyntheticBinding (content-free id)
//       demand: ShapeDemand {
//           path: Arc<[PathSegment]>,
//           terminal_context: ProjectionReductionContext,  // mode + demand
//           key_filter: KeyFilter,
//           surface: PublishedSurfaceKind,
//       },
//   }
//
// The empty-path All-filter Registry-surface variant collapses to the
// previously-unkeyed-path cache identity, so existing callers preserve
// behaviour. Path-precise narrowing emerges when projectors, fallthrough,
// and operator reducers thread narrowed `SurfaceProjection` cursors and
// consult `ShapeCacheDb` per-hop.
//
// Broader-to-narrower satisfaction (superset-cache-hit) is intentionally
// absent. Production callers pass `SurfaceProjection::whole_surface
// (Registry)`, which constructs a `ShapeDemand::whole_subject_with_context`
// key under `Published(mode)`; the cache never sees a narrowed key today,
// so retroactive subset extraction would have no callers and no test
// coverage. Narrowing instead emerges path-precisely as projectors call
// `descend_published_member` and re-consult the cache at each hop.
// ===========================================================================

// Overlay/base isolation for `MemberValueNode`-subject entries does NOT
// rely on `SemanticNodeId` being generation-tagged (the arena is
// append-only across generations and IDs are raw `u64`). Isolation comes
// from three mechanisms working together:
//   1. `observe_materialize_scope` is overlay-aware and pins the overlay
//      `IndexedReady` when an overlay covers the scope.
//   2. The entry's fact signature self-roots on that observation's
//      `whole_hash`. A base-mode peek against an overlay-rooted entry
//      fails `ReadSetSignature::validate_with_self_roots`.
//   3. The `CacheEntry::validated_at_generation` field plus
//      `bump_project_generation_and_evict` detect cross-generation drift
//      on overlay open/close.

/// A module-private zero-sized seal carried by every externally-typed
/// [`ShapeSubject`] variant. Its type is not nameable outside
/// `component_meta_caches`, so no other module (in this crate or any
/// downstream crate) can struct-construct a `ShapeSubject` variant by
/// literal — the ONLY build path is the `ShapeCacheKey::*_whole*`
/// constructors. Pattern-matching with `{ .. }` is unaffected.
///
/// The `MemberValueNode` variant does not need this marker: its
/// `node: MemberShapeNodeSubject` field is itself module-private to
/// construct, which already seals that arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ConstructionSeal;

/// Sealed graph-node identity for the regular member-shape subject: the
/// EXACT settled `SurfaceMember.value` graph node. The inner ordinal is
/// module-private so a raw `SemanticNodeId` cannot spread into the
/// shape-key subject from an arbitrary node. The ONLY sanctioned
/// construction is the member-value path (`from_surface_member`); the
/// `#[cfg(test)]` ctor is for the cache-rail validation test only.
///
/// The newtype derives `Hash`/`Eq` TRANSPARENTLY over the same
/// [`crate::semantic_query::SemanticNodeId`] — so the key's hash bytes are
/// unchanged versus a bare ordinal field. This is a representation/naming
/// seal, not a stale-entry compatibility change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MemberShapeNodeSubject(crate::semantic_query::SemanticNodeId);

impl MemberShapeNodeSubject {
    /// Sanctioned construction: a POLICY-ADMITTED component member's value
    /// graph node. Takes the admitted publication token (not a raw
    /// `&SurfaceMember`) so an arbitrary member cannot be routed through the
    /// sealed shape subject — the only construction path for the member-shape
    /// subject is from a member that passed publication admission.
    fn from_surface_member(
        member: &crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember<
            '_,
        >,
    ) -> Self {
        Self(member.member().value)
    }

    /// Test-only construction from a raw `&SurfaceMember`. The cache-rail
    /// key-identity tests (`query_db_self_root_tests`) build keys directly from
    /// synthetic members to assert the subject collapses siblings sharing
    /// `.value`; they do not resolve a real macro surface. Named `_raw` so it
    /// can never masquerade as the production admitted-member path.
    #[cfg(test)]
    fn from_surface_member_raw(member: &crate::semantic_query::SurfaceMember) -> Self {
        Self(member.value)
    }

    /// Test-only arbitrary-node construction for the cache-rail validation
    /// test (`query_db_self_root_tests`), which needs SOME node-subject key,
    /// not specifically a member value.
    #[cfg(test)]
    fn from_semantic_node_for_test(node: crate::semantic_query::SemanticNodeId) -> Self {
        Self(node)
    }
}

/// Subject of a [`ShapeCacheKey`] — the *what* whose shape is cached.
///
/// `MemberValueNode` covers the per-member route whose start point is the
/// settled `SurfaceMember.value` graph node. It is keyed by the sealed
/// [`MemberShapeNodeSubject`] newtype; a raw `TypeExpr` never enters a cache key.
/// `SyntheticBinding` covers explicit deepening of a synthetic
/// slot-binding carrier, keyed by the content-free
/// [`crate::semantic_query::SyntheticBindingId`]. All subjects share the
/// same cache substrate — they differ only in the identity used to key
/// entries.
///
/// The variant payloads are non-constructible outside this module: the
/// `MemberValueNode` arm via the module-private [`MemberShapeNodeSubject`]
/// newtype's inner field, the `SyntheticBinding` arm via a module-private
/// [`ConstructionSeal`] marker. External code matches on the variants
/// (with `{ .. }`) but builds them ONLY through the `ShapeCacheKey`
/// constructors. This is the structural half of synthetic-carrier confinement.
///
/// `private_interfaces` is allowed deliberately: a module-private
/// [`ConstructionSeal`] field on a `pub` enum is reachable for matching
/// yet its type cannot be named outside this module, so the variant
/// cannot be struct-constructed externally. That "more private than the
/// item" shape IS the sealing idiom, not a leak.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(private_interfaces)]
pub enum ShapeSubject {
    /// Member-value graph-node subject. Sibling members whose
    /// `SurfaceMember.value` is the same settled graph node collapse onto
    /// each other's warm hits. Keyed by the sealed
    /// [`MemberShapeNodeSubject`] newtype (its inner arena ordinal
    /// is module-private), so a raw `SemanticNodeId` cannot spread into
    /// the shape-key subject. This is a generation/store-scoped
    /// graph-instance memo (single-entry, fact-validated,
    /// generation-gated), NOT a durable content-free query-identity key.
    MemberValueNode {
        scope: Arc<str>,
        node: MemberShapeNodeSubject,
    },
    /// Synthetic-binding-keyed subject. The explicit-deepen identity for
    /// a `TypeExpr::SyntheticSlotBinding` carrier: the content-free
    /// [`crate::semantic_query::SyntheticBindingId`]
    /// (`scope_canonical_id, surface_kind, slot_name, binding_name`).
    /// The carrier's `value_node` arena ordinal is value-side provenance
    /// only — it round-trips through `SemanticNodeData::SyntheticBinding`
    /// at the compat boundary and NEVER enters this key. The
    /// module-private `_seal` blocks external struct-literal
    /// construction.
    SyntheticBinding {
        id: crate::semantic_query::SyntheticBindingId,
        _seal: ConstructionSeal,
    },
}

impl ShapeSubject {
    /// Canonical scope id this subject is rooted in. Used for
    /// strict self-root warm-read validation and per-canonical
    /// invalidation.
    pub(crate) fn scope_canonical(&self) -> &Arc<str> {
        match self {
            ShapeSubject::MemberValueNode { scope, .. } => scope,
            ShapeSubject::SyntheticBinding { id, .. } => &id.scope_canonical_id,
        }
    }
}

/// Per-call demand a [`ShapeCacheKey`] addresses — the *how* the shape
/// will be consumed. Distinct demands for the same subject keep
/// disjoint entries (e.g. Shallow vs Expanded over the same node,
/// or `Published(Navigate)` vs `StructuralTransit(Navigate)` over the
/// same graph node).
///
/// The terminal-hop demand carries the FULL
/// [`ProjectionReductionContext`] (mode + demand), not just a bare
/// [`ProjectionMode`]. The demand axis lets a per-prop `Navigate`
/// publication slot key disjointly from a `StructuralTransit(Navigate)`
/// carrier-lower slot — same subject, same mode, but distinct reduction
/// work and distinct results. Without the demand axis a transit lower
/// would poison the publication slot (or vice versa) the first time
/// both routes touched the same subject.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeDemand {
    /// Path segments narrowing the requested shape. Empty path =
    /// whole subject. Non-empty path = path-precise demand: projectors
    /// narrow this at the publication boundary.
    pub(crate) path: Arc<[crate::semantic_query::PathSegment]>,
    /// Terminal-hop projection / reduction context. `(mode, demand)`
    /// keyed disjointly so cache slots split per the disjoint-slot
    /// rule ("ShapeCacheKey must carry the complete demand/context,
    /// not a Navigate boolean").
    pub(crate) terminal_context: crate::semantic_query::ProjectionReductionContext,
    /// Key filter at the terminal hop (Pick/Omit narrowing, etc.).
    pub(crate) key_filter: crate::meta_resolve::projection_demand::KeyFilter,
    /// Which published surface this demand serves. Used by the
    /// registry walker to discriminate caller intent + by the cache
    /// key to keep slot identity disjoint across surfaces.
    pub(crate) surface: crate::meta_resolve::projection_demand::PublishedSurfaceKind,
}

impl ShapeDemand {
    /// Demand encoding "whole subject, no path narrowing,
    /// Internal-surface caller, caller-supplied reduction context".
    /// The single entry point for whole-subject demand construction.
    ///
    /// PRODUCTION callers build the [`ProjectionReductionContext`]
    /// explicitly and use the member-value constructor
    /// [`ShapeCacheKey::surface_member_value_whole_with_context`] (which
    /// takes a `&SurfaceMember`). The mode-only
    /// `member_value_node_whole_for_test` constructor wraps a bare
    /// [`ProjectionMode`] in `ProjectionReductionContext::published(mode)`
    /// for the implicit-Published default and are reserved for TESTS /
    /// schema-probe helpers (`member_value_node_whole_for_test` is
    /// `#[cfg(test)]`-only); no production member-value caller routes
    /// through them.
    pub(crate) fn whole_subject_with_context(
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
            terminal_context,
            key_filter: crate::meta_resolve::projection_demand::KeyFilter::All,
            surface: crate::meta_resolve::projection_demand::PublishedSurfaceKind::Internal {
                caller: "ShapeCacheDb::whole_subject",
            },
        }
    }
}

/// In-memory `ShapeCacheDb` slot key. This is a derived-`Hash`/`Eq`
/// DashMap key ONLY — it is NEVER persisted, stable-hashed, or
/// wire-encoded (the type carries no `Serialize`/`Deserialize` and has
/// no `bincode` / stable-hash / proto encode site anywhere in the tree).
/// That is precisely why renaming a `ShapeSubject` variant — with the
/// variant order preserved and the inner ordinal hidden behind a
/// `#[derive(Hash, Eq)]` transparent newtype — keeps the key's runtime
/// hash/equality identity byte-identical and therefore needs NO
/// `CACHE_CLUSTER_SCHEMA_VERSION` bump (the schema version guards
/// PERSISTED artifact layouts, not in-memory DashMap key identity).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeCacheKey {
    /// Module-private so no other module (in-crate or downstream) can
    /// build a `ShapeCacheKey { subject, demand }` struct-literal that
    /// bypasses the classifying `*_whole*` constructors. Sibling modules
    /// read the rooting canonical through [`Self::scope_canonical`].
    subject: ShapeSubject,
    demand: ShapeDemand,
}

impl ShapeCacheKey {
    /// The sealed member-value node retained by this key, when it has that subject.
    pub(crate) fn member_value_subject_node(
        &self,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        match &self.subject {
            ShapeSubject::MemberValueNode { node, .. } => Some(node.0),
            ShapeSubject::SyntheticBinding { .. } => None,
        }
    }

    /// Canonical scope id this key is rooted in — the public accessor
    /// sibling modules use now that `subject` is module-private. Delegates
    /// to [`ShapeSubject::scope_canonical`].
    pub(crate) fn scope_canonical(&self) -> &Arc<str> {
        self.subject.scope_canonical()
    }

    /// Test-only: build a member-value-subject key from an ARBITRARY node id.
    /// Used by the cache-rail self-root validation test, which needs SOME
    /// node-subject key, not specifically a `SurfaceMember.value`. Named
    /// `_for_test` so it can never masquerade as a production constructor.
    #[cfg(test)]
    pub(crate) fn member_value_node_whole_for_test(
        scope: Arc<str>,
        node: crate::semantic_query::SemanticNodeId,
        mode: ProjectionMode,
    ) -> Self {
        Self {
            subject: ShapeSubject::MemberValueNode {
                scope,
                node: MemberShapeNodeSubject::from_semantic_node_for_test(node),
            },
            demand: ShapeDemand::whole_subject_with_context(
                crate::semantic_query::ProjectionReductionContext::published(mode),
            ),
        }
    }

    /// Construct a member-value-subject whole-subject key under an explicit
    /// [`ProjectionReductionContext`]. The SOLE production construction path
    /// for the member-shape subject — it takes the POLICY-ADMITTED publication
    /// token (not a raw `&SurfaceMember`) and reads the admitted member's
    /// `value`, so an arbitrary `SemanticNodeId` / unadmitted member cannot be
    /// routed through the sealed subject.
    pub(crate) fn surface_member_value_whole_with_context(
        scope: Arc<str>,
        member: &crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember<
            '_,
        >,
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            subject: ShapeSubject::MemberValueNode {
                scope,
                node: MemberShapeNodeSubject::from_surface_member(member),
            },
            demand: ShapeDemand::whole_subject_with_context(terminal_context),
        }
    }

    /// Construct a member-value-subject whole-subject key from a RAW producing
    /// `SemanticNodeId` under an explicit [`ProjectionReductionContext`], gated by
    /// the non-output [`crate::meta_resolve::materialize::RegistryMemberShapeKeyCap`].
    /// Preserves the EXACT `ShapeSubject::MemberValueNode` /
    /// `ShapeDemand::whole_subject_with_context` semantics of
    /// [`Self::surface_member_value_whole_with_context`] — sibling registry members
    /// whose first-pass node is the same settled graph node collapse onto each
    /// other's warm hits — without requiring an `AdmittedPublishedMember` (the
    /// registry stabiliser already holds the producing node directly).
    /// Test-only member-value key construction from a raw `&SurfaceMember`. The
    /// cache-rail key-identity tests build keys directly from synthetic members
    /// to assert the subject collapses siblings sharing `.value`; they do not
    /// resolve a real macro surface (and so cannot mint an admitted token).
    /// Named `_raw` so it can never masquerade as the production
    /// admitted-member path.
    #[cfg(test)]
    pub(crate) fn surface_member_value_whole_with_context_raw(
        scope: Arc<str>,
        member: &crate::semantic_query::SurfaceMember,
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            subject: ShapeSubject::MemberValueNode {
                scope,
                node: MemberShapeNodeSubject::from_surface_member_raw(member),
            },
            demand: ShapeDemand::whole_subject_with_context(terminal_context),
        }
    }

    /// Construct a SyntheticBinding-subject whole-subject key (content-
    /// free [`crate::semantic_query::SyntheticBindingId`] identity).
    /// Terminal context is implicitly `Published(mode)`.
    ///
    /// Test-support convenience for the synthetic explicit-deepen proof. The
    /// only callers are
    /// in-crate `#[cfg(test)]` suites and the test-support
    /// `insert_synthetic_carrier_deep_for_test` / `get_synthetic_carrier_deep_for_test`
    /// proof helpers; production keys via
    /// [`Self::synthetic_binding_whole_with_context`]. The gate is the
    /// production-unreachable `test-support` feature (NOT `debug_assertions`,
    /// which is ON in ordinary debug builds) so this stays coherent with the
    /// carrier `_for_test` accessors the helpers feed.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn synthetic_binding_whole(
        id: crate::semantic_query::SyntheticBindingId,
        mode: ProjectionMode,
    ) -> Self {
        Self::synthetic_binding_whole_with_context(
            id,
            crate::semantic_query::ProjectionReductionContext::published(mode),
        )
    }

    /// Construct a SyntheticBinding-subject whole-subject key under an
    /// explicit [`ProjectionReductionContext`]. The content-free
    /// [`crate::semantic_query::SyntheticBindingId`] is the identity; the
    /// carrier's `value_node` is value-side provenance only and never
    /// enters this key.
    pub(crate) fn synthetic_binding_whole_with_context(
        id: crate::semantic_query::SyntheticBindingId,
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            subject: ShapeSubject::SyntheticBinding {
                id,
                _seal: ConstructionSeal,
            },
            demand: ShapeDemand::whole_subject_with_context(terminal_context),
        }
    }
}

pub struct ShapeCacheDb {
    entries: DashMap<ShapeCacheKey, Arc<CacheEntry<MaterializedOutputTypeExpr>>>,
    inflight: InflightTable<QueryFlightKey<ShapeCacheKey>>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl ShapeCacheDb {
    pub(crate) fn attach_output(
        &self,
    ) -> crate::project_semantic_dispatch::memo::SingleEntryAttachment<
        '_,
        ShapeCacheKey,
        MaterializedOutputTypeExpr,
    > {
        crate::project_semantic_dispatch::memo::SingleEntryAttachment::new(
            &self.entries,
            &self.inflight,
            &self.live_counter,
            self.schema_version,
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture<'a, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
        &'a self,
        ctx: &'a dyn crate::resolver_core::fact_validation_port::LiveFactValidation<Clocks = W>,
    ) -> crate::project_semantic_dispatch::memo::MemoPublish<'a, Self, W> {
        crate::project_semantic_dispatch::memo::MemoPublish::for_test(self, ctx)
    }

    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::with_counter_and_schema_version(
            live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_and_schema_version(Arc::new(AtomicU64::new(0)), schema_version)
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
            schema_version,
        }
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<ShapeCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                if entry.key().subject.scope_canonical().as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant.
    ///
    /// Gated `#[cfg(any(test, feature = "test-support"))]` (NOT
    /// `debug_assertions`): it constructs a `MaterializedOutputTypeExpr` via the
    /// capability-free `from_type_expr_for_test` carrier accessor, which is
    /// itself test-support-gated so it is COMPILE-ABSENT from production debug
    /// builds. The `cache_invariant_migration` integration case reaches this
    /// through the production-unreachable `test-support` feature.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
        // The schema-eviction fixture needs SOME key rooted at the marker
        // scope; the content-free synthetic-binding identity is the one
        // subject constructible without a live store (the member-value
        // subject requires a lowered node).
        let key = ShapeCacheKey::synthetic_binding_whole(
            crate::semantic_query::SyntheticBindingId {
                scope_canonical_id: Arc::from(marker),
                surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
                slot_name: None,
                binding_name: Arc::from("__schema_probe__"),
            },
            ProjectionMode::Shallow,
        );
        let entry = Arc::new(CacheEntry {
            value: MaterializedOutputTypeExpr::from_type_expr_for_test(
                None,
                TypeExpr::Unknown(verter_type_expr::UnknownValue::missing_output()),
                Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
                false,
            ),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(vec![Arc::clone(key.subject.scope_canonical())]),
            validated_at_generation: 0,
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    // -----------------------------------------------------------------
    // Synthetic-carrier explicit-deepen positive-proof helpers
    // -----------------------------------------------------------------
    //
    // These helpers exercise the legitimate cache route for deepening a
    // `TypeExpr::SyntheticSlotBinding(SyntheticCarrierKey)` carrier into
    // its underlying member shape, per the
    // `[[component-meta-shallow-by-default-rule]]` and the
    // `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
    // architecture guard.
    //
    // The contract: the ONLY legitimate way to deepen a carrier is to
    // construct
    //   `ShapeCacheKey::synthetic_binding_whole(
    //        SyntheticBindingId::from_carrier_key(carrier), mode)`
    // and consult `ShapeCacheDb`. The cache identity is the content-free
    // `SyntheticBindingId` (`scope_canonical_id, surface_kind, slot_name,
    // binding_name`); the carrier's `value_node` arena ordinal is
    // value-side provenance only. The ONE production consumer of this
    // route is the terminal-demand raise of the synthetic-binding SOURCE
    // arm (`deepen_synthetic_binding_to_hot` in
    // `project_semantic_dispatch/semantic_source.rs`); every projector,
    // reducer, registry, and graph-builder site still refuses the carrier
    // as a shallow terminal. The positive-proof integration test
    // `tests/cases/g_misc0/synthetic_carrier_explicit_deepen_proof.rs` uses these
    // helpers to prove the content-free cache-key identity is well-defined
    // for every consumer of the route.

    /// Insert a synthetic-carrier-deep entry into the cache under the
    /// content-free synthetic-binding identity. The key is built via
    /// `ShapeCacheKey::synthetic_binding_whole(
    ///     SyntheticBindingId::from_carrier_key(carrier), mode)`. Stored as
    /// a `MaterializedOutputTypeExpr` whose `type_expr` is the requested deep
    /// type so a subsequent peek through the same identity returns the
    /// deep shape, not the carrier. The carrier's `value_node` is
    /// value-side provenance and is NOT part of the cache identity, so the
    /// entry's `node_id` is left `None` (the proof reads only the
    /// `type_expr`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_carrier_deep_for_test(
        &self,
        carrier: &verter_type_expr::SyntheticCarrierKey,
        mode: ProjectionMode,
        deep_type: TypeExpr,
    ) {
        use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
        let key = ShapeCacheKey::synthetic_binding_whole(
            crate::semantic_query::SyntheticBindingId::from_carrier_key(carrier),
            mode,
        );
        let entry = Arc::new(CacheEntry {
            value: MaterializedOutputTypeExpr::from_type_expr_for_test(
                None,
                deep_type,
                Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
                false,
            ),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(vec![Arc::clone(key.subject.scope_canonical())]),
            validated_at_generation: 0,
        });
        // Bump `live_counter` ONLY on a genuine new key. `DashMap::insert`
        // returns `Some(old)` on overwrite, `None` on a fresh insert — an
        // unconditional `fetch_add` over-counts when two same-identity
        // carriers (differing only in `value_node` provenance) collapse
        // onto ONE key, diverging the atomic from `entries.len()`.
        if self.entries.insert(key, entry).is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Peek a synthetic-carrier-deep entry out of the cache through the
    /// content-free synthetic-binding identity. Bypasses the full
    /// `ResolverContext`-gated `peek` so the positive-proof test does not
    /// need to stand up a host. Returns the materialised deep `TypeExpr`
    /// if an entry exists for this carrier's content-free identity, or
    /// `None` otherwise — so two carriers differing only in `value_node`
    /// hit the same entry (the ordinal is provenance, not identity).
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_synthetic_carrier_deep_for_test(
        &self,
        carrier: &verter_type_expr::SyntheticCarrierKey,
        mode: ProjectionMode,
    ) -> Option<TypeExpr> {
        let key = ShapeCacheKey::synthetic_binding_whole(
            crate::semantic_query::SyntheticBindingId::from_carrier_key(carrier),
            mode,
        );
        self.entries
            .get(&key)
            .map(|entry| entry.value.type_expr_for_test().clone())
    }
}

impl Default for ShapeCacheDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for ShapeCacheDb {
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
        }
        count
    }
}

// ===========================================================================
//
// ===========================================================================

// and a uniform `invalidate_canonical_for` body that delegates to
// the existing `invalidate_canonical(...)` linear scan and reports
// the count of entries dropped via the `live_count()` delta. Each
// impl is written explicitly (not generated by a `macro_rules!`) so
// the source-structure architecture guard
// `every_db_field_implements_invalidation_by_canonical` can locate
// the impl block by direct text search.

impl crate::invalidation_domain::ParticipatesInInvalidation for DeclarationLookupDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for DeclarationLookupDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for ResolvabilityDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for ResolvabilityDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for OwnerCollectionDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for OwnerCollectionDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for ShapeCacheDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for ShapeCacheDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// AppConfigNoOverrideProofDb production producer
// ═══════════════════════════════════════════════════════════════════════════
/// Production producer for [`crate::app_config_proof_db::AppConfigNoOverrideProofDb`].
///
/// Given a key `(decl_canonical, component_key_literal)`, returns
/// the cached proof entry if one is valid under the live store
/// view, OR runs a cold compute (wrapped in `install_fact_tracer`)
/// and publishes a fresh proof.
///
/// The cold compute checks the `IndexedReady.declares_interface_app_config`
/// flag for `decl_canonical` and observes its `FileWholeHash` fact
/// through the active tracer. The proof's `fact_dep_signature`
/// therefore captures (a) the decl-canonical's whole-hash so an
/// edit to the file invalidates the proof, and (b) any transitive
/// observations the call-chain made through the resolver substrate.
///
/// `publish()` accepts `Arc<[FactVersionRef]>` directly — the
/// path-precise fact-signature substrate (`HostStoreView::validates`)
/// is the sole cache-validity oracle.
///
/// **Cold-build outcome semantics:**
/// - `Some(entry)` published — proof is valid. The fast-path
///   consumer can rely on the fact-signature for warm-hit revalidation.
/// - On `FactReadSetFinalise::Overflow` — refuse cache admission;
///   the next call cold-recomputes. The provenance counter
///   `app_config_proof_overflow_refusals` advances.
///
/// Resolver-tier producer that takes `&dyn ResolverContext` to stay
/// inside the request-port contract (the six ports in
/// `resolver_core::request_ports`, whose compile-contract fixtures prove a
/// request cannot reach ambient host state). Integration tests reach this
/// via the crate-public wrapper
/// [`crate::for_tests::app_config_no_override_proof_get_or_compute_for_tests`].
///
/// The ComponentConfig theme-variant fast-path resolver (a future
/// re-introduction of the retired rescue cascade) and the
/// app-config no-override deferred-proof test both reach this
/// producer.
///
/// Reached today only through tests and the `for_tests` wrapper (both
/// gated by the explicit `test-support` feature); gated to match so the
/// producer is absent from every ordinary production build.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn app_config_no_override_proof_get_or_compute<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
) -> Option<Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx)
        .app_config_no_override_proof_get_or_compute(key)
}
