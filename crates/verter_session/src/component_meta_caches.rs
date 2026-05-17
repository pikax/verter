//! Host-owned typed DB wrappers for the 10 component-meta caches that
//! were previously authoritative inside `ComponentMetaQueryEngine`.
//!
//! ## Architecture
//!
//! Each cache is a typed `*Db` wrapper around `DashMap<Key, Arc<Entry>>`
//! plus a per-cache `InflightTable<Key>` (admission control isolation per
//! D3.2). The wrappers share the same shape:
//!
//! - `Entry` carries `(value, dep_signature)`.
//! - The cold-compute entry point delegates to a cooperative-admission
//!   primitive for one-winner-cold-build, panic-safety, and
//!   post-compute revalidation. Most wrappers expose
//!   `get_or_compute<F>(key, host, compute) -> Option<value>` over
//!   [`cooperative_get_or_insert`]. Two consume the
//!   [`ComputeAdmission`](crate::cooperative_admission::ComputeAdmission)
//!   API of `cooperative_admit_with_post_publish` so a
//!   valid-but-non-cacheable cold outcome is broadcast to joiners
//!   without admitting the cache: `ImportedRegistryDb` via its
//!   `get_or_compute_admit` method (the producer runs its
//!   fuse-consuming resolution inside the singleflight `compute`
//!   closure), and `MaterializeStructureDb` via the materialiser's
//!   direct use of its `entries()` / `inflight()` accessors.
//! - `validate(&Entry)` and `revalidate_after_compute(&Entry)` consult
//!   [`HostFenceValidator`](crate::host_manage::HostFenceValidator) to
//!   reject entries whose `dep_signature` no longer matches host state.
//! - Per-canonical and project-generation invalidation hooks wired into
//!   [`ProjectTypeStore::evict_canonical`] and
//!   [`ProjectTypeStore::bump_project_generation_and_evict`].
//!
//! ## D3.5 — `Arc<str>` / `Arc<TypeExpr>` keys
//!
//! Keys live in [`crate::resolver_core::cache_keys`] with the wide-string
//! fields migrated to `Arc<str>` per D3.5. Cloning a key is a cheap
//! refcount bump rather than a heap allocation + copy.
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
use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;
use verter_type_expr::TypeExpr;

use crate::cooperative_admission::{cooperative_get_or_insert, InflightTable};
use crate::fact_signature_helpers::{
    bubble_fact_signature, validate_fact_signature_with_self_roots,
};
use crate::instant::Instant;
use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
use crate::resolver_core::cache_keys::{
    PreparedMemberCacheKey, PreparedSurfaceCacheKey, PreparedTargetCacheKey,
    RoutedExprSurfaceCacheKey,
};
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{FactVersionRef, ResolvedTypeDeclaration, ResolverContext};
use crate::semantic_query::{DepSignature, ProjectionMode};

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
}

pub type ImportedRegistryKey = (Arc<str>, Arc<str>);

pub struct ImportedRegistryDb {
    entries: DashMap<ImportedRegistryKey, Arc<ImportedRegistryEntry>>,
    inflight: InflightTable<ImportedRegistryKey>,
    live_counter: Arc<AtomicU64>,
    canonical_index: crate::invalidation_domain::CanonicalReverseIndex<ImportedRegistryKey>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl ImportedRegistryDb {
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
    #[cfg(any(test, debug_assertions))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_and_schema_version(Arc::new(AtomicU64::new(0)), schema_version)
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
            canonical_index: crate::invalidation_domain::CanonicalReverseIndex::new(),
            schema_version,
        }
    }

    /// Peek-only lookup: returns the cached value only if its
    /// `fact_dep_signature` is still valid against `ctx`.
    ///
    /// This is the warm-hit half of [`Self::get_or_compute`] exposed
    /// for the producer's compute-once shape: the producer peeks here
    /// first, and on a miss computes the imported-registry value
    /// **once** (the wildcard-route fuse is a side-effecting budget —
    /// it must be consumed at most once per request) before using
    /// `get_or_compute` purely as a signature-building write-through.
    /// The keyed canonical is the entry's self-root, validated
    /// strictly — a same-canonical content edit, or a keyed canonical
    /// untracked by the live store view, rejects the entry, exactly
    /// matching the `get_or_compute` warm-hit `validate` arm.
    pub(crate) fn peek(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<Arc<ResolvedImportedRegistrySymbol>>> {
        let self_roots: [&str; 1] = [key.0.as_ref()];
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if validate_fact_signature_with_self_roots(ctx, &entry_arc.fact_dep_signature, &self_roots)
        {
            bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    /// Cooperative-admission cold compute over the imported-registry
    /// cache, routed through
    /// [`crate::cooperative_admission::cooperative_admit_with_post_publish`]
    /// (the [`ComputeAdmission`](crate::cooperative_admission::ComputeAdmission)
    /// API).
    ///
    /// The producer's `compute` closure runs the expensive,
    /// fuse-consuming `resolve_imported_registry_symbol_with_budget`
    /// resolution INSIDE the per-key `InflightTable` singleflight slot:
    /// when several requests miss the same key concurrently, exactly
    /// ONE winner runs `compute` and every joiner blocks on the slot
    /// condvar and reuses the winner's value. Running the resolution
    /// here — rather than before the admission call — is what makes the
    /// wildcard-route fuse a one-winner cost instead of an N-waiter
    /// cost.
    ///
    /// `compute` returns a [`ComputeAdmission`](crate::cooperative_admission::ComputeAdmission):
    ///
    /// - `Cacheable(entry)` — the provenance-pure fact signature built;
    ///   the entry is admitted, `post_publish` registers the reverse
    ///   index, joiners re-read the published entry.
    /// - `ReturnOnly(value)` — the resolution produced a valid value
    ///   but shared-cache admission is refused (the signature builder
    ///   could not build, or the test refusal hook fired). The cache
    ///   stays empty, joiners receive `value` through the slot's
    ///   return-only channel, and the next cold miss recomputes. The
    ///   resolution is NOT re-run and the fuse is NOT consumed twice.
    /// - `Failed` — the resolution itself failed; joiners surface
    ///   `None` and the next caller retries.
    pub(crate) fn get_or_compute_admit<F>(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Option<Arc<ResolvedImportedRegistrySymbol>>>
    where
        F: FnOnce() -> crate::cooperative_admission::ComputeAdmission<
            Option<Arc<ResolvedImportedRegistrySymbol>>,
            ImportedRegistryEntry,
        >,
    {
        let live_counter = Arc::clone(&self.live_counter);
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        let key_for_post_publish = key.clone();
        let canonical_index = &self.canonical_index;
        let canonical_index_for_removal = &self.canonical_index;
        // The keyed canonical is the entry's self-root — validated
        // strictly on warm read (same-canonical edit / untracked keyed
        // canonical → miss).
        let self_roots: [&str; 1] = [key.0.as_ref()];
        crate::cooperative_admission::cooperative_admit_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &ImportedRegistryEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            compute,
            |entry: &ImportedRegistryEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &ImportedRegistryEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            // Removal-side counterpart of `post_publish`: when the
            // substrate removes an already-published entry (warm-hit
            // reject or joiner-fork reject) the live counter must
            // decrement and the per-canonical reverse-index
            // registration must drop, symmetric with the
            // `post_publish` bump + register. Without this a
            // joiner-fork over-counts the live counter and leaves a
            // dangling reverse-index entry.
            move |removed_key: &ImportedRegistryKey, _entry: &Arc<ImportedRegistryEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
                canonical_index_for_removal.unregister(removed_key.0.as_ref(), removed_key);
            },
            move |_entry_arc: &Arc<ImportedRegistryEntry>, _key: &ImportedRegistryKey| {
                // Fires AFTER entries.insert AND AFTER successful
                // post-compute revalidation — only for the `Cacheable`
                // arm. A `ReturnOnly` outcome is NOT admitted, so the
                // live counter is bumped and the reverse index is
                // registered exactly when the entry actually lands.
                live_counter.fetch_add(1, Ordering::Relaxed);
                let canonical = Arc::clone(&key_for_post_publish.0);
                canonical_index.register(&canonical, key_for_post_publish.clone());
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        // Drain via the per-canonical reverse index in
        // O(K) (entries owned by this canonical) instead of O(N) (total
        // entries). The index is populated on every cooperative
        // post-publish, so a content edit on the file invalidates
        // exactly its own resolved imports.
        let drained = self.canonical_index.drain_for(canonical_id);
        for key in drained {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_index.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    /// Test-only accessor for the per-cache in-flight table. Drives
    /// the deterministic cooperative-joiner rendezvous in
    /// `query_db_self_root_tests.rs` — a follower is a confirmed
    /// joiner once it has cloned its own `Arc` to the winner's
    /// in-flight slot, observable via `InflightTable::slot_strong_count`.
    #[cfg(test)]
    pub(crate) fn inflight_table_for_test(&self) -> &InflightTable<ImportedRegistryKey> {
        &self.inflight
    }

    /// Test-only: is `key` currently registered in the per-canonical
    /// reverse index? Drives the P2 removal-cleanup discriminator —
    /// after a joiner-fork removes the winner's entry, the winner's
    /// key must NOT dangle in the reverse index.
    #[cfg(test)]
    pub(crate) fn reverse_index_contains_for_test(&self, key: &ImportedRegistryKey) -> bool {
        self.canonical_index.contains(key.0.as_ref(), key)
    }

    /// Test-only direct insertion entry point used by the
    /// invalidation-perf regression test
    /// (`crates/verter_session/tests/invalidation_perf.rs`). Bypasses
    /// the cooperative-admission inflight slot and registers the entry
    /// in the per-canonical reverse index identically to the cold
    /// post-publish path. NOT for use from production code.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_for_test(&self, key: ImportedRegistryKey, entry: Arc<ImportedRegistryEntry>) {
        let canonical = Arc::clone(&key.0);
        self.canonical_index.register(&canonical, key.clone());
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant. The entry payload is a placeholder;
    /// the fixture only inspects `live_count()` before and after
    /// `evict_if_schema_mismatch()`.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let key: ImportedRegistryKey = (Arc::from(marker), Arc::from("synthetic"));
        let entry = Arc::new(ImportedRegistryEntry {
            value: None,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
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
        let drained = self.canonical_index.drain_for(canonical_id);
        let mut removed = 0usize;
        for key in drained {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                removed += 1;
            }
        }
        removed
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
        let count = self.entries.len();
        self.entries.clear();
        self.canonical_index.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
        }
        count
    }
}

// ===========================================================================
// 2. DeclarationLookupDb — `(canonical, name) → ResolvedTypeDeclaration`
// ===========================================================================

#[derive(Clone)]
pub struct DeclarationLookupEntry {
    pub value: Arc<ResolvedTypeDeclaration>,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub type DeclarationLookupKey = (Arc<str>, Arc<str>);

pub struct DeclarationLookupDb {
    entries: DashMap<DeclarationLookupKey, Arc<DeclarationLookupEntry>>,
    inflight: InflightTable<DeclarationLookupKey>,
    live_counter: Arc<AtomicU64>,
}

impl DeclarationLookupDb {
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &DeclarationLookupKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> Option<(ResolvedTypeDeclaration, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment:
        // when the substrate removes an already-published entry (warm-hit
        // reject or joiner-fork reject), the live counter must decrement
        // symmetrically so it tracks live entries, not lifetime inserts.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The entry's keyed canonical is its self-root: the warm-read
        // validator validates the self-root `FileWholeHash` strictly so
        // a same-canonical content edit (or a keyed canonical that
        // became untracked) rejects the entry instead of riding the
        // lazy "untracked → accept" rule.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &DeclarationLookupEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    DeclarationLookupEntry {
                        value: Arc::new(value),
                        fact_dep_signature,
                    }
                })
            },
            |entry: &DeclarationLookupEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &DeclarationLookupEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &DeclarationLookupKey, _entry: &Arc<DeclarationLookupEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<DeclarationLookupKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _) = entry.key();
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
// 3. ResolvabilityDb — `(canonical, name) → bool`
// ===========================================================================

#[derive(Clone)]
pub struct ResolvabilityEntry {
    pub value: bool,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub type ResolvabilityKey = (Arc<str>, Arc<str>);

pub struct ResolvabilityDb {
    entries: DashMap<ResolvabilityKey, Arc<ResolvabilityEntry>>,
    inflight: InflightTable<ResolvabilityKey>,
    live_counter: Arc<AtomicU64>,
}

impl ResolvabilityDb {
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ResolvabilityKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> Option<(bool, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries, not lifetime
        // inserts.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed source canonical is the entry's self-root — strict
        // warm-read validation rejects a same-canonical edit or an
        // untracked keyed canonical.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &ResolvabilityEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value)
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    ResolvabilityEntry {
                        value,
                        fact_dep_signature,
                    }
                })
            },
            |entry: &ResolvabilityEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value
            },
            |entry: &ResolvabilityEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &ResolvabilityKey, _entry: &Arc<ResolvabilityEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<ResolvabilityKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _) = entry.key();
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
// 4. OwnerCollectionDb — `Arc<str> (name) → Option<TypeExpr>`
//
// Note: keyed solely by name within an owner scope. Since multiple owners
// may collide on the same name with different TypeExprs, the entry tracks
// the owner_canonical at insertion time and validates per-canonical only.
// ===========================================================================

#[derive(Clone)]
pub struct OwnerCollectionEntry {
    #[allow(dead_code)]
    pub owner_canonical: Arc<str>,
    pub value: Option<Arc<TypeExpr>>,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub type OwnerCollectionKey = (Arc<str>, Arc<str>); // (owner, name)

pub struct OwnerCollectionDb {
    entries: DashMap<OwnerCollectionKey, Arc<OwnerCollectionEntry>>,
    inflight: InflightTable<OwnerCollectionKey>,
    live_counter: Arc<AtomicU64>,
}

impl OwnerCollectionDb {
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &OwnerCollectionKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Option<Arc<TypeExpr>>>
    where
        F: FnOnce() -> Option<(Option<TypeExpr>, Arc<[FactVersionRef]>)>,
    {
        let owner_canonical = key.0.clone();
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The owner canonical is the entry's self-root. This cache is
        // body-bearing (stores a `TypeExpr`), so strict self-root
        // validation is the correctness floor — a content edit to the
        // owner file invalidates the cached collection expression.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &OwnerCollectionEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    OwnerCollectionEntry {
                        owner_canonical,
                        value: value.map(Arc::new),
                        fact_dep_signature,
                    }
                })
            },
            |entry: &OwnerCollectionEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &OwnerCollectionEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &OwnerCollectionKey, _entry: &Arc<OwnerCollectionEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<OwnerCollectionKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (owner, _) = entry.key();
                if owner.as_ref() == canonical_id {
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
// 5. PreparedTargetDb — `PreparedTargetCacheKey → Option<(Arc<str>, Arc<str>)>`
// ===========================================================================

#[derive(Clone)]
pub struct PreparedTargetEntry {
    pub value: Option<(Arc<str>, Arc<str>)>,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub struct PreparedTargetDb {
    entries: DashMap<PreparedTargetCacheKey, Arc<PreparedTargetEntry>>,
    inflight: InflightTable<PreparedTargetCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl PreparedTargetDb {
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
    #[cfg(any(test, debug_assertions))]
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

    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &PreparedTargetCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<(Arc<str>, Arc<str>)>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // Both the active scope and the declaring canonical are
        // self-roots: the resolved target depends on the content of
        // each, so an edit to either rejects the entry.
        let self_roots: [&str; 2] = [
            key.active_scope_canonical_id.as_ref(),
            key.decl_canonical_id.as_ref(),
        ];
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if validate_fact_signature_with_self_roots(ctx, &entry_arc.fact_dep_signature, &self_roots)
        {
            bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedTargetCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Option<(Arc<str>, Arc<str>)>>
    where
        F: FnOnce() -> Option<(Option<(Arc<str>, Arc<str>)>, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // Both keyed canonicals (active scope + declaring file) are
        // self-roots — strict warm-read validation.
        let self_roots: [&str; 2] = [
            key.active_scope_canonical_id.as_ref(),
            key.decl_canonical_id.as_ref(),
        ];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedTargetEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedTargetEntry {
                        value,
                        fact_dep_signature,
                    }
                })
            },
            |entry: &PreparedTargetEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedTargetEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &PreparedTargetCacheKey, _entry: &Arc<PreparedTargetEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedTargetCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.active_scope_canonical_id.as_ref() == canonical_id
                    || key.decl_canonical_id.as_ref() == canonical_id
                {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let key = PreparedTargetCacheKey {
            active_scope_canonical_id: Arc::from(marker),
            decl_canonical_id: Arc::from(marker),
            decl_symbol_name: Arc::from("Synthetic"),
            requested_name: Arc::from("Synthetic"),
        };
        let entry = Arc::new(PreparedTargetEntry {
            value: None,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for PreparedTargetDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for PreparedTargetDb {
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
// 6. MaterializeMemoDb — `(Arc<str>, Arc<TypeExpr>, ProjectionMode) → MaterializedTypeExpr`
// ===========================================================================

#[derive(Clone)]
pub struct MaterializeMemoEntry {
    pub value: MaterializedTypeExpr,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub type MaterializeMemoKey = (Arc<str>, Arc<TypeExpr>, ProjectionMode);

pub struct MaterializeMemoDb {
    entries: DashMap<MaterializeMemoKey, Arc<MaterializeMemoEntry>>,
    inflight: InflightTable<MaterializeMemoKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl MaterializeMemoDb {
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
    #[cfg(any(test, debug_assertions))]
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

    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &MaterializeMemoKey,
        ctx: &dyn ResolverContext,
    ) -> Option<MaterializedTypeExpr> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The keyed scope canonical is the entry's self-root — strict
        // warm-read validation rejects a same-scope content edit.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        let result = (|| -> Option<MaterializedTypeExpr> {
            let entry_arc = self.entries.get(key).map(|e| e.clone())?;
            if validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            ) {
                bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
                Some(entry_arc.value.clone())
            } else {
                None
            }
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .materialize_memo
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .materialize_memo
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &MaterializeMemoKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<MaterializedTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedTypeExpr, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed scope canonical is the entry's self-root — strict
        // warm-read validation.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &MaterializeMemoEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    MaterializeMemoEntry {
                        value,
                        fact_dep_signature,
                    }
                })
            },
            |entry: &MaterializeMemoEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &MaterializeMemoEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &MaterializeMemoKey, _entry: &Arc<MaterializeMemoEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<MaterializeMemoKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (scope, _, _) = entry.key();
                if scope.as_ref() == canonical_id {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
        let key: MaterializeMemoKey = (
            Arc::from(marker),
            Arc::new(TypeExpr::Unknown { raw: String::new() }),
            ProjectionMode::Shallow,
        );
        let entry = Arc::new(MaterializeMemoEntry {
            value: MaterializedTypeExpr {
                node_id: None,
                type_expr: TypeExpr::Unknown { raw: String::new() },
                dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
            },
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for MaterializeMemoDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for MaterializeMemoDb {
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
// 8. PreparedSurfaceDb — `PreparedSurfaceCacheKey → PreparedSurfaceProjection`
//
// PreparedSurfaceProjection is private to the engine; we serialize the
// concrete `Arc<ProjectedSurface>` payload through a public adapter.
// ===========================================================================

/// Public projection payload mirrored from the engine's
/// `PreparedSurfaceProjection`. The engine adapter converts at the
/// edges so consumers do not depend on the engine module's private
/// enum.
#[derive(Debug, Clone)]
pub enum PreparedSurfacePayload {
    Surface(Arc<verter_semantic::analysis::type_solver::query_engine::ProjectedSurface>),
    Empty,
    Unsupported,
}

#[derive(Clone)]
pub struct PreparedSurfaceEntry {
    pub value: PreparedSurfacePayload,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub struct PreparedSurfaceDb {
    entries: DashMap<PreparedSurfaceCacheKey, Arc<PreparedSurfaceEntry>>,
    inflight: InflightTable<PreparedSurfaceCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl PreparedSurfaceDb {
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
    #[cfg(any(test, debug_assertions))]
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

    /// Peek-only lookup: returns the cached payload only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &PreparedSurfaceCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<PreparedSurfacePayload> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The keyed canonical is the entry's self-root. The prepared
        // surface encodes body-sensitive structure, so strict self-root
        // validation is the correctness floor — any content edit to the
        // keyed file rejects the cached projection.
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        let result = (|| -> Option<PreparedSurfacePayload> {
            let entry_arc = self.entries.get(key).map(|e| e.clone())?;
            if validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            ) {
                bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
                Some(entry_arc.value.clone())
            } else {
                None
            }
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .prepared_surface
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .prepared_surface
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedSurfaceCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<PreparedSurfacePayload>
    where
        F: FnOnce() -> Option<(PreparedSurfacePayload, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed canonical is the entry's self-root — strict
        // warm-read validation (body-sensitive surface).
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedSurfaceEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedSurfaceEntry {
                        value,
                        fact_dep_signature,
                    }
                })
            },
            |entry: &PreparedSurfaceEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedSurfaceEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &PreparedSurfaceCacheKey, _entry: &Arc<PreparedSurfaceEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedSurfaceCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.canonical_id.as_ref() == canonical_id {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::resolver_core::cache_keys::PreparedSubstitutionKey;
        let key = PreparedSurfaceCacheKey {
            canonical_id: Arc::from(marker),
            symbol_name: Arc::from("Synthetic"),
            substitutions: PreparedSubstitutionKey::Empty,
        };
        let entry = Arc::new(PreparedSurfaceEntry {
            value: PreparedSurfacePayload::Empty,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for PreparedSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for PreparedSurfaceDb {
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
// 9. PreparedMemberDb — `PreparedMemberCacheKey → Option<ProjectedMember>`
// ===========================================================================

#[derive(Clone)]
pub struct PreparedMemberEntry {
    pub value: Option<Arc<ProjectedMember>>,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub struct PreparedMemberDb {
    entries: DashMap<PreparedMemberCacheKey, Arc<PreparedMemberEntry>>,
    inflight: InflightTable<PreparedMemberCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl PreparedMemberDb {
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
    #[cfg(any(test, debug_assertions))]
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

    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &PreparedMemberCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<Arc<ProjectedMember>>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The keyed canonical is the entry's self-root — strict
        // warm-read validation rejects a same-canonical content edit.
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        let result = (|| -> Option<Option<Arc<ProjectedMember>>> {
            let entry_arc = self.entries.get(key).map(|e| e.clone())?;
            if validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            ) {
                bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
                Some(entry_arc.value.clone())
            } else {
                None
            }
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .prepared_member
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .prepared_member
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedMemberCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Option<Arc<ProjectedMember>>>
    where
        F: FnOnce() -> Option<(Option<ProjectedMember>, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed canonical is the entry's self-root — strict
        // warm-read validation.
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedMemberEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedMemberEntry {
                        value: value.map(Arc::new),
                        fact_dep_signature,
                    }
                })
            },
            |entry: &PreparedMemberEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedMemberEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &PreparedMemberCacheKey, _entry: &Arc<PreparedMemberEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedMemberCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.canonical_id.as_ref() == canonical_id {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::resolver_core::cache_keys::{PreparedMemberCacheKind, PreparedSubstitutionKey};
        let key = PreparedMemberCacheKey {
            canonical_id: Arc::from(marker),
            symbol_name: Arc::from("Synthetic"),
            member_name: Arc::from("synthetic"),
            kind: PreparedMemberCacheKind::Requested,
            substitutions: PreparedSubstitutionKey::Empty,
        };
        let entry = Arc::new(PreparedMemberEntry {
            value: None,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for PreparedMemberDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for PreparedMemberDb {
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
// 10. RoutedExprSurfaceDb — `RoutedExprSurfaceCacheKey → TypeExpr`
// ===========================================================================

#[derive(Clone)]
pub struct RoutedExprSurfaceEntry {
    pub value: Arc<TypeExpr>,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
}

pub struct RoutedExprSurfaceDb {
    entries: DashMap<RoutedExprSurfaceCacheKey, Arc<RoutedExprSurfaceEntry>>,
    inflight: InflightTable<RoutedExprSurfaceCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl RoutedExprSurfaceDb {
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
    #[cfg(any(test, debug_assertions))]
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

    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Arc<TypeExpr>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The keyed scope canonical is the entry's self-root — strict
        // warm-read validation rejects a same-scope content edit.
        let self_roots: [&str; 1] = [key.scope_canonical_id.as_ref()];
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if validate_fact_signature_with_self_roots(ctx, &entry_arc.fact_dep_signature, &self_roots)
        {
            bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Arc<TypeExpr>>
    where
        F: FnOnce() -> Option<(TypeExpr, Arc<[FactVersionRef]>)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `compute`-closure increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed scope canonical is the entry's self-root — strict
        // warm-read validation.
        let self_roots: [&str; 1] = [key.scope_canonical_id.as_ref()];
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &RoutedExprSurfaceEntry| {
                if validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                ) {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    RoutedExprSurfaceEntry {
                        value: Arc::new(value),
                        fact_dep_signature,
                    }
                })
            },
            |entry: &RoutedExprSurfaceEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &RoutedExprSurfaceEntry| {
                validate_fact_signature_with_self_roots(ctx, &entry.fact_dep_signature, &self_roots)
            },
            move |_key: &RoutedExprSurfaceCacheKey, _entry: &Arc<RoutedExprSurfaceEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<RoutedExprSurfaceCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.scope_canonical_id.as_ref() == canonical_id {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::resolver_core::RouteDemand;
        let key = RoutedExprSurfaceCacheKey {
            scope_canonical_id: Arc::from(marker),
            root_symbol: Arc::from("Synthetic"),
            route: RouteDemand::Whole,
        };
        let entry = Arc::new(RoutedExprSurfaceEntry {
            value: Arc::new(TypeExpr::Unknown { raw: String::new() }),
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for RoutedExprSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for RoutedExprSurfaceDb {
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

use crate::component_meta_materialize::{MaterializeOutcome, MaterializeStructureCacheKey};

/// Entry stored in `MaterializeStructureDb`. Carries the
/// cacheable `MaterializeOutcome` (`Value` or `Miss` only — `Recursive`
/// and `Tainted` are non-cacheable per-call sentinels) plus the
/// legacy whole-hash `dep_signature` and the R3/R26/R28 path-precise
/// `fact_dep_signature` observed during the cold build.
///
/// The dual signature reflects the AND-gate transitional model:
/// the legacy DepSignature stays in place for its existing consumer
/// ecosystem (`accumulate_dispatch_dep_signature` bubble-up,
/// `dep_signature_valid_for_host` warm-hit validation, audit-event
/// emission). The fact_dep_signature is the R3/R26/R28 substrate
/// that bubbles through [`crate::fact_signature_helpers::bubble_fact_signature`]
/// so an active outer fact tracer accumulates this inner cache's
/// observation set on every transitive hit.
#[derive(Clone)]
pub struct MaterializeStructureEntry {
    /// The cached outcome. ONLY `Value` or `Miss` may be stored here.
    /// The materialiser's publish path enforces this with
    /// `debug_assert!`.
    pub outcome: MaterializeOutcome,
    /// Carrier holding both the legacy `DepSignature` rail (used by
    /// `peek` and cooperative-admission revalidation) AND the R28
    /// path-precise fact signature (used by warm-hit bubble). Warm
    /// reads call `read_set_signature.validate(ctx)` BEFORE bubbling.
    /// `canonical_ids()` drives the unified reverse-index
    /// registration.
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
}

/// Final-result cache for the structural
/// materialiser. Reverse-index `canonical_to_keys` enables
/// `Arc::ptr_eq`-based invalidation cleanup; cooperative-admission's
/// `post_publish` callback wires the registration.
pub struct MaterializeStructureDb {
    entries: DashMap<MaterializeStructureCacheKey, Arc<MaterializeStructureEntry>>,
    inflight: InflightTable<MaterializeStructureCacheKey>,
    /// Per-canonical reverse index: maps each canonical id to the set
    /// of cache keys whose `dep_signature` references it, paired with
    /// the registered `dep_signature` `Arc`. `invalidate_for_canonical`
    /// drains this map and uses `Arc::ptr_eq` to discriminate stale
    /// entries from fresh post-publish writes.
    canonical_to_keys: DashMap<
        Arc<str>,
        parking_lot::Mutex<rustc_hash::FxHashMap<MaterializeStructureCacheKey, DepSignature>>,
    >,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl MaterializeStructureDb {
    /// Construct a fresh cache.
    #[must_use]
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
    #[cfg(any(test, debug_assertions))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_and_schema_version(Arc::new(AtomicU64::new(0)), schema_version)
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            live_counter,
            schema_version,
        }
    }

    /// Read-only test accessor for the shared
    /// live_counter. Used by `materialize_publish_after_invalidation_*`
    /// tests to verify that revalidation failures do NOT increment the
    /// counter (entries are removed without inflating the live count).
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "B1's validated_at_generation field + the test that consumes this accessor are pending; this accessor is reserved for that wiring"
    )]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.live_counter.load(Ordering::Relaxed)
    }

    /// Read-only peek with proactive stale-entry removal.
    /// When the entry's `dep_signature` is stale, remove it (orphan
    /// reaping) and return `None`.
    ///
    /// R8-5 — successful stale removal must decrement the shared
    /// `live_counter` so it tracks live entries (not lifetime inserts).
    /// Without this, every stale peek inflates the shared counter
    /// permanently.
    pub(crate) fn peek(
        &self,
        key: &MaterializeStructureCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<MaterializeOutcome>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = (|| -> Option<crate::semantic_query::CacheRead<MaterializeOutcome>> {
            let entry_arc = self.entries.get(key).map(|e| e.clone())?;
            // Carrier-aware validate-before-bubble: BOTH the legacy
            // whole-hash rail AND the path-precise R28 fact signature
            // must validate against the live store view. A stale
            // entry never bubbles into the active outer tracer.
            if !entry_arc.read_set_signature.validate(ctx) {
                let removed = self
                    .entries
                    .remove_if(key, |_, e| Arc::ptr_eq(e, &entry_arc));
                if removed.is_some() {
                    self.live_counter.fetch_sub(1, Ordering::Relaxed);
                }
                return None;
            }
            entry_arc.read_set_signature.bubble(ctx);
            crate::host_manage::record_materialize_structure_cache_hit();
            Some(crate::semantic_query::CacheRead {
                value: entry_arc.outcome.clone(),
                dep_signature: Arc::clone(&entry_arc.read_set_signature.legacy),
                walker_diagnostics: std::sync::Arc::from([]),
                cache_suppress: false,
            })
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .materialize_structure
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .materialize_structure
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Drop every cache entry whose `dep_signature` references
    /// `canonical_id`. — uses the `canonical_to_keys`
    /// reverse index to find affected keys; uses `Arc::ptr_eq` to
    /// discriminate "our entry" from concurrent fresh writes.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let drained: Vec<(MaterializeStructureCacheKey, DepSignature)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.read_set_signature.legacy, &registered)
            });
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Cross-canonical cleanup with ptr_eq — drop the
                // matching registration in every other canonical's
                // shard.
                for (other_canonical, _) in registered_sig.iter() {
                    if other_canonical.as_ref() == canonical_id {
                        continue;
                    }
                    if let Some(shard) = self.canonical_to_keys.get(other_canonical) {
                        let mut map = shard.lock();
                        if let Some(existing_sig) = map.get(key) {
                            if Arc::ptr_eq(existing_sig, registered_sig) {
                                map.remove(key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Drop every cache entry. Used on project-generation bumps.
    ///
    /// Plan R8-5 — saturating-subtract pattern (NOT `store(0)`) because
    /// `live_counter` is shared via `Arc<AtomicU64>` across every typed DB
    /// in `ProjectTypeStore` (`component_meta_cache_live`). A per-DB
    /// `store(0)` would corrupt other DBs' contributions to the shared
    /// sum. Mirrors the existing `ImportedRegistryDb::invalidate_all`
    /// pattern — subtract only this DB's entry count, capped at the
    /// counter's current value to prevent underflow under
    /// concurrent invalidation.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    /// Number of warm entries.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of distinct cache slots currently materialised in the
    /// `MaterializeStructureDb`'s entry map.
    ///
    /// **R7 cross-owner reuse contract.** N consumer scopes that
    /// reach the same `(base, scope_axis, mode)` collapse to ONE
    /// entry because the cache key's `Hash`/`PartialEq` impls exclude
    /// `scope_canonical_id`. Used by
    /// `tests/cross_owner_materialise_reuse_production.rs` to verify
    /// the production-flow contract: driving
    /// `materialize_component_meta_structure` from N owners with a
    /// shared inner type produces `entry_count == 1` for that slot.
    ///
    /// Synonym for [`live_count`](Self::live_count) kept as a stable
    /// accessor for the landing-gap audit tests; the two will not
    /// diverge.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::component_meta_materialize::MaterializationScope;
        use crate::semantic_query::SemanticNodeId;
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: Arc::from(marker),
            base: SemanticNodeId(0),
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Shallow,
        };
        let entry = Arc::new(MaterializeStructureEntry {
            outcome: MaterializeOutcome::Miss(SemanticNodeId(0)),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Internal — register a `(key, dep_signature_legacy)` pair under
    /// every canonical referenced by the entry's carrier (union of
    /// legacy + facts canonicals). Called from the materialiser's
    /// `post_publish` callback. The unified reverse index drains
    /// under any canonical the carrier references, so fact-only deps
    /// (`Parse(...)` / `ResolveImports(...)` / `RouteSurface(...)`)
    /// invalidate the entry even when the legacy `DepSignature` does
    /// not name the changed canonical.
    pub(crate) fn register_post_publish(
        &self,
        key: MaterializeStructureCacheKey,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let registered_legacy = Arc::clone(&read_set_signature.legacy);
        for canonical in read_set_signature.canonical_ids() {
            let shard = self
                .canonical_to_keys
                .entry(canonical)
                .or_insert_with(|| parking_lot::Mutex::new(rustc_hash::FxHashMap::default()));
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let mut map = shard.value().lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_family_map_lock_acquisition(lock_wait);
            map.insert(key.clone(), Arc::clone(&registered_legacy));
        }
    }

    /// Internal — get the inflight table. Used by the materialiser
    /// for the cooperative-admission write path.
    pub(crate) fn inflight(&self) -> &InflightTable<MaterializeStructureCacheKey> {
        &self.inflight
    }

    /// Internal — get the entries map. Used by the materialiser for
    /// the cooperative-admission write path.
    pub(crate) fn entries(
        &self,
    ) -> &DashMap<MaterializeStructureCacheKey, Arc<MaterializeStructureEntry>> {
        &self.entries
    }

    /// Internal — bump the live counter. Called from the
    /// materialiser's compute closure on successful publish.
    pub(crate) fn bump_live_counter(&self) {
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Internal — decrement the live counter. The removal-side
    /// counterpart of [`Self::bump_live_counter`], called from the
    /// materialiser's cooperative-admission `removal_cleanup` closure
    /// when the substrate removes an already-published entry so the
    /// shared `component_meta_cache_live` counter tracks live entries,
    /// not lifetime inserts.
    pub(crate) fn decrement_live_counter(&self) {
        self.live_counter.fetch_sub(1, Ordering::Relaxed);
    }

    /// Removal-side counterpart of [`Self::register_post_publish`].
    /// When the cooperative-admission substrate removes one
    /// already-published entry (a warm-hit reject or a joiner-fork
    /// reject) the entry's reverse-index registration must be dropped
    /// under EVERY canonical it referenced, symmetric with the
    /// per-canonical `register_post_publish` insert. `Arc::ptr_eq`
    /// against the entry's `legacy` rail discriminates "our
    /// registration" from a concurrent fresh winner's.
    pub(crate) fn unregister_post_publish(
        &self,
        key: &MaterializeStructureCacheKey,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) {
        for canonical in read_set_signature.canonical_ids() {
            if let Some(shard) = self.canonical_to_keys.get(&canonical) {
                let mut map = shard.lock();
                if let Some(existing_sig) = map.get(key) {
                    if Arc::ptr_eq(existing_sig, &read_set_signature.legacy) {
                        map.remove(key);
                    }
                }
            }
        }
    }
}

impl Default for MaterializeStructureDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for MaterializeStructureDb {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        let count = self.entries.len();
        self.entries.clear();
        self.canonical_to_keys.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
        }
        count
    }
}

// ===========================================================================
// C — RefCycleResultDb
// ===========================================================================

use crate::cooperative_admission::cooperative_get_or_insert_with_post_publish;
use crate::semantic_query::DeclIdentity;

/// C — entry stored in `RefCycleResultDb`. Carries the
/// boolean BFS result, the dep-signature recorded during the cold BFS
/// compute, and a `validated_at_generation` field used by `peek`'s
/// generation-local fast path.
///
/// No `Clone` derive — `AtomicU64` is non-Clone. Entries are wrapped in
/// `Arc<RefCycleEntry>`; cloning the `Arc` cheaply shares the entry
/// across cache hits.
pub struct RefCycleEntry {
    /// `true` when the BFS root reaches a transitive cycle through a
    /// complex helper surface.
    pub result: bool,
    /// Carrier holding both the legacy `DepSignature` (used by
    /// `peek`'s slow path) and the R28 path-precise fact signature
    /// (used by warm-hit bubble). `read_set_signature.validate(ctx)`
    /// is the AND-gate; `canonical_ids()` drives reverse-index
    /// registration.
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
    /// Generation-local validity field. Updated to the
    /// current `workspace().content_generation()` on:
    ///   - cold publish (initial value = current generation);
    ///   - successful slow-path revalidation in `peek`.
    ///
    /// `peek`'s fast path returns immediately when `cached ==
    /// current_gen` without walking the dep_signature. Race contract:
    /// `Relaxed` ordering — under heavy invalidation, a thread may see
    /// a stale cached_gen and re-walk the slow path even when the
    /// entry is still valid. This is correct (re-walking catches
    /// genuine staleness) but may briefly reduce cache effectiveness.
    pub validated_at_generation: AtomicU64,
}

/// R — host-owned cache for transitive
/// cycle BFS results.
///
/// Mirrors [`MaterializeStructureDb`]'s reverse-index pattern:
///   - Entries keyed by `DeclIdentity`.
/// - `canonical_to_keys` reverse-index drains under `invalidate_for_canonical`.
/// - `Arc::ptr_eq` discriminates "our entry" from concurrent fresh writes.
///   - `live_counter` shared via `Arc<AtomicU64>` with all sibling DBs;
///     uses the saturating-subtract pattern on `invalidate_all` to
///     preserve other DBs' contributions.
///
/// Cooperative-admission integration: cold-path BFS runs inside
/// [`cooperative_get_or_insert_with_post_publish`], whose `compute`
/// closure runs synchronously on the caller's thread (see
/// `cooperative_admission.rs:278` synchronous-compute contract).
/// Borrow-capture of `&dyn ResolverContext` in the BFS compute closure
/// is safe because no thread-hop occurs.
pub struct RefCycleResultDb {
    entries: DashMap<DeclIdentity, Arc<RefCycleEntry>>,
    inflight: InflightTable<DeclIdentity>,
    /// Per-canonical reverse index — maps each canonical id to the set
    /// of cache keys whose dep_signature references it.
    canonical_to_keys:
        DashMap<Arc<str>, parking_lot::Mutex<rustc_hash::FxHashMap<DeclIdentity, DepSignature>>>,
    live_counter: Arc<AtomicU64>,
}

impl RefCycleResultDb {
    /// Construct with a fresh, unshared `live_counter`. Tests-only.
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    /// Construct with a shared `live_counter` borrowed from
    /// `ProjectTypeStoreCounters::component_meta_cache_live`.
    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            live_counter,
        }
    }

    /// Internal — get the entries map. Used by the BFS-cache compute
    /// closure for `cooperative_get_or_insert_with_post_publish`.
    pub(crate) fn entries(&self) -> &DashMap<DeclIdentity, Arc<RefCycleEntry>> {
        &self.entries
    }

    /// Internal — get the inflight table. Used by the BFS-cache compute
    /// closure for `cooperative_get_or_insert_with_post_publish`.
    pub(crate) fn inflight(&self) -> &InflightTable<DeclIdentity> {
        &self.inflight
    }

    /// Internal — bump the live counter. Called from the BFS-cache
    /// compute closure's `post_publish` callback on successful publish.
    pub(crate) fn bump_live_counter(&self) {
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Removal-side counterpart of [`Self::bump_live_counter`]. Called
    /// from the cooperative-admission `removal_cleanup` closure when
    /// the substrate removes an already-published entry so the shared
    /// `component_meta_cache_live` counter tracks live entries, not
    /// lifetime inserts.
    pub(crate) fn decrement_live_counter(&self) {
        self.live_counter.fetch_sub(1, Ordering::Relaxed);
    }

    /// Read-only test accessor for the shared
    /// `live_counter`. Used by R's invalidation tests to verify that
    /// `invalidate_for_canonical` and `invalidate_all` correctly
    /// decrement the counter without corrupting sibling DBs'
    /// contributions to the shared sum.
    #[cfg(test)]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.live_counter.load(Ordering::Relaxed)
    }

    /// Register the reverse-index after a successful publish under
    /// every canonical referenced by the entry's carrier (union of
    /// legacy + facts canonicals). Per-canonical mutex acquisition
    /// pattern matches `MaterializeStructureDb`. Bounded by
    /// `read_set_signature.canonical_ids().len() ≤ ~80` (BFS hop cap
    /// + transitive fact set).
    pub(crate) fn register_post_publish(
        &self,
        key: DeclIdentity,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let registered_legacy = Arc::clone(&read_set_signature.legacy);
        for canonical in read_set_signature.canonical_ids() {
            let shard = self
                .canonical_to_keys
                .entry(canonical)
                .or_insert_with(|| parking_lot::Mutex::new(rustc_hash::FxHashMap::default()));
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let mut map = shard.value().lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_family_map_lock_acquisition(lock_wait);
            map.insert(key.clone(), Arc::clone(&registered_legacy));
        }
    }

    /// Removal-side counterpart of [`Self::register_post_publish`].
    /// When the cooperative-admission substrate removes one
    /// already-published entry (a warm-hit reject or a joiner-fork
    /// reject) the entry's reverse-index registration must be dropped
    /// under EVERY canonical it referenced, symmetric with the
    /// per-canonical `register_post_publish` insert. `Arc::ptr_eq`
    /// against the entry's `legacy` rail discriminates "our
    /// registration" from a concurrent fresh winner's so a
    /// re-published entry's registration is not stolen.
    pub(crate) fn unregister_post_publish(
        &self,
        key: &DeclIdentity,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) {
        for canonical in read_set_signature.canonical_ids() {
            if let Some(shard) = self.canonical_to_keys.get(&canonical) {
                let mut map = shard.lock();
                if let Some(existing_sig) = map.get(key) {
                    if Arc::ptr_eq(existing_sig, &read_set_signature.legacy) {
                        map.remove(key);
                    }
                }
            }
        }
    }

    /// Generation-local validity peek.
    ///
    /// Fast path: if the entry's `validated_at_generation` matches the
    /// host's current `content_generation`, return the cached value
    /// without walking the dep_signature.
    ///
    /// Slow path: revalidate against `HostFenceValidator`; on success,
    /// update `validated_at_generation` and return; on failure, remove
    /// the stale entry (with `live_counter` decrement per R8-5) and
    /// return `None`.
    pub(crate) fn peek(
        &self,
        id: &DeclIdentity,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<bool>> {
        let result = (|| -> Option<crate::semantic_query::CacheRead<bool>> {
            let entry_arc = self.entries.get(id).map(|e| Arc::clone(&*e))?;
            let current_gen = ctx.workspace_content_generation();
            let cached_gen = entry_arc.validated_at_generation.load(Ordering::Relaxed);
            if cached_gen == current_gen {
                return Some(crate::semantic_query::CacheRead {
                    value: entry_arc.result,
                    dep_signature: Arc::clone(&entry_arc.read_set_signature.legacy),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                });
            }
            // Carrier-aware validate-before-bubble: BOTH rails must
            // validate against the live store view. A stale entry
            // never returns and never bubbles.
            if !entry_arc.read_set_signature.validate(ctx) {
                // R8-5 — decrement live_counter on stale removal so
                // the shared counter tracks live entries, not stale ones.
                let removed = self
                    .entries
                    .remove_if(id, |_, e| Arc::ptr_eq(e, &entry_arc));
                if removed.is_some() {
                    self.live_counter.fetch_sub(1, Ordering::Relaxed);
                }
                return None;
            }
            entry_arc.read_set_signature.bubble(ctx);
            entry_arc
                .validated_at_generation
                .store(current_gen, Ordering::Relaxed);
            Some(crate::semantic_query::CacheRead {
                value: entry_arc.result,
                dep_signature: Arc::clone(&entry_arc.read_set_signature.legacy),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            })
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .ref_cycle
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .ref_cycle
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Drop every cache entry whose `dep_signature`
    /// references `canonical_id`. Uses the `canonical_to_keys` reverse
    /// index to find affected keys; `Arc::ptr_eq` discriminates "our
    /// entry" from concurrent fresh writes.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let drained: Vec<(DeclIdentity, DepSignature)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.read_set_signature.legacy, &registered)
            });
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Cross-canonical cleanup with ptr_eq — drop the
                // matching registration in every other canonical's
                // shard so subsequent invalidations do not double-free.
                for (other_canonical, _) in registered_sig.iter() {
                    if other_canonical.as_ref() == canonical_id {
                        continue;
                    }
                    if let Some(shard) = self.canonical_to_keys.get(other_canonical) {
                        let mut map = shard.lock();
                        if let Some(existing_sig) = map.get(key) {
                            if Arc::ptr_eq(existing_sig, registered_sig) {
                                map.remove(key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Saturating-subtract pattern (NOT `store(0)`)
    /// because `live_counter` is shared via `Arc<AtomicU64>` across all
    /// typed DBs in `ProjectTypeStore`. A per-DB `store(0)` would
    /// corrupt sibling DBs' contributions to the shared sum.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }
}

impl Default for RefCycleResultDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Public hook to consult the BFS cache from
/// `meta_resolve::ref_root_reaches_transitive_cycle_node`.
///
/// Returns `Some(read)` on a generation-local fast hit OR a
/// successful slow-path revalidation. Returns `None` on a true cache
/// miss or a stale entry (the caller falls through to BFS compute).
pub(crate) fn ref_cycle_db_peek(
    db: &RefCycleResultDb,
    id: &DeclIdentity,
    ctx: &dyn ResolverContext,
) -> Option<crate::semantic_query::CacheRead<bool>> {
    db.peek(id, ctx)
}

// ===========================================================================
// typed invalidation domain wiring for every
// component-meta DB. Each DB declares the
// `InvalidationDomain::FileContent | ResolverState | ProjectGeneration`
// triplet (per-canonical eviction reaches all three when a file edit
// invalidates resolved type declarations / route shape / project
// generation).
//
// `InvalidationByCanonical::invalidate_canonical_for` returns the count
// of entries dropped. Per the §12.A3 architecture, the DBs route
// project-shape (ProjectGeneration) bumps through `invalidate(domain)`
// → `invalidate_all`, and per-canonical edits through
// `invalidate_canonical_for`. The `ImportedRegistryDb` impls are
// already wired above (with a per-canonical reverse index for O(K)
// drains); the impls below preserve existing per-DB linear-scan
// invalidation semantics and feed the same count back to the cascade
// so the unit-test harness can verify cascade coverage.
// ===========================================================================

// The DBs below all share the
// `[FileContent, ResolverState, ProjectGeneration]` domain triplet
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

impl crate::invalidation_domain::ParticipatesInInvalidation for PreparedTargetDb {
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

impl crate::invalidation_domain::InvalidationByCanonical for PreparedTargetDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for MaterializeMemoDb {
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

impl crate::invalidation_domain::InvalidationByCanonical for MaterializeMemoDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for PreparedSurfaceDb {
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

impl crate::invalidation_domain::InvalidationByCanonical for PreparedSurfaceDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for PreparedMemberDb {
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

impl crate::invalidation_domain::InvalidationByCanonical for PreparedMemberDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for RoutedExprSurfaceDb {
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

impl crate::invalidation_domain::InvalidationByCanonical for RoutedExprSurfaceDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

// `MaterializeStructureDb` and `RefCycleResultDb` use
// `invalidate_for_canonical` instead of `invalidate_canonical`. Keep
// the trait wiring uniform.
impl crate::invalidation_domain::ParticipatesInInvalidation for MaterializeStructureDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ComponentMeta, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for MaterializeStructureDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_for_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for RefCycleResultDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, TypeGraph, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for RefCycleResultDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.entries().len();
        self.invalidate_for_canonical(canonical_id);
        let after = self.entries().len();
        before.saturating_sub(after)
    }
}

/// The cooperative-admission wrapper invoked by
/// `meta_resolve::ref_root_reaches_transitive_cycle_node` on the cold
/// path. The `compute` closure runs synchronously on the caller's
/// thread (per cooperative_admission's synchronous-compute contract),
/// so capturing `&dyn ResolverContext` and `&DeclIdentity` directly is safe.
///
/// On cooperative-admission success: bumps `live_counter`, registers
/// the reverse-index, and returns `Some(CacheRead)`. On revalidation
/// failure or compute returning `None`: returns `None` and the caller
/// falls back to an uncached recompute.
pub(crate) fn ref_cycle_db_get_or_compute<C>(
    db: &RefCycleResultDb,
    id: &DeclIdentity,
    ctx: &dyn ResolverContext,
    compute_bfs: C,
) -> Option<crate::semantic_query::CacheRead<bool>>
where
    C: FnOnce(&mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>) -> bool,
{
    let key_for_register = id.clone();
    // `&RefCycleResultDb` is `Copy`; a dedicated binding lets the
    // removal-side closure capture the db alongside the `post_publish`
    // closure that captures the original `db`.
    let db_for_removal = db;
    let current_gen = ctx.workspace_content_generation();
    // Block 1.H: wrap the BFS cold-compute with `install_fact_tracer`.
    // On `Ok`, override the entry's `fact_dep_signature` with the
    // traced observation set. On `Overflow`, refuse cache admission.
    let host = ctx.host_for_fact_tracer_install();
    let provenance = Arc::clone(&host.provenance);
    cooperative_get_or_insert_with_post_publish(
        db.entries(),
        db.inflight(),
        id.clone(),
        // Validate(&Entry) -> Option<V> — carrier-aware AND-gate.
        |entry: &RefCycleEntry| {
            if entry.read_set_signature.validate(ctx) {
                entry.read_set_signature.bubble(ctx);
                Some(crate::semantic_query::CacheRead {
                    value: entry.result,
                    dep_signature: Arc::clone(&entry.read_set_signature.legacy),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                })
            } else {
                None
            }
        },
        // Compute() -> Option<Entry>
        || -> Option<RefCycleEntry> {
            let inner = || -> RefCycleEntry {
                let mut compute_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> =
                    Vec::new();
                let result = compute_bfs(&mut compute_fence);
                let fact_dep_signature =
                    crate::component_meta_materialize::fact_signature_from_fence(&compute_fence);
                let legacy: DepSignature = Arc::from(compute_fence.into_boxed_slice());
                RefCycleEntry {
                    result,
                    read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                        fact_dep_signature,
                        legacy,
                    ),
                    validated_at_generation: AtomicU64::new(current_gen),
                }
            };
            let (entry, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, inner);
            provenance
                .ref_cycle_fact_tracer_installs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match finalise {
                crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                    // Override the carrier's facts rail with the
                    // tracer's authoritative observation set; keep
                    // the legacy whole-hash rail as captured.
                    let mut entry = entry;
                    let legacy = Arc::clone(&entry.read_set_signature.legacy);
                    entry.read_set_signature = crate::fact_signature_helpers::ReadSetSignature::new(
                        fact_dep_signature,
                        legacy,
                    );
                    Some(entry)
                }
                crate::resolver_core::FactReadSetFinalise::Overflow => {
                    provenance
                        .ref_cycle_overflow_refusals
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Refuse cache admission; caller cold-recomputes.
                    None
                }
            }
        },
        // Project(&Entry) -> V — bubble path-precise observation set
        // so outer cold-computes see the BFS's transitive facts.
        |entry: &RefCycleEntry| {
            entry.read_set_signature.bubble(ctx);
            crate::semantic_query::CacheRead {
                value: entry.result,
                dep_signature: Arc::clone(&entry.read_set_signature.legacy),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            }
        },
        // revalidate_after_compute(&Entry) -> bool — carrier AND-gate.
        |entry: &RefCycleEntry| entry.read_set_signature.validate(ctx),
        // removal_cleanup(&K, &Arc<Entry>) — removal-side counterpart
        // of `post_publish`. When the substrate removes an
        // already-published entry (warm-hit reject or joiner-fork
        // reject) the live counter must decrement and the
        // per-canonical reverse-index registration must drop,
        // symmetric with the `post_publish` bump + register.
        move |removed_key: &DeclIdentity, removed_entry: &Arc<RefCycleEntry>| {
            db_for_removal.decrement_live_counter();
            db_for_removal.unregister_post_publish(removed_key, &removed_entry.read_set_signature);
        },
        // post_publish(&Arc<Entry>, &K)
        move |entry_arc: &Arc<RefCycleEntry>, _k: &DeclIdentity| {
            db.bump_live_counter();
            db.register_post_publish(key_for_register.clone(), &entry_arc.read_set_signature);
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// AppConfigNoOverrideProofDb production producer (Block 1.H Track 2.4)
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
/// **Codex Option B contract:** `publish()` accepts
/// `Arc<[FactVersionRef]>` directly; the legacy `DepSignature`
/// derivation has been retired (see `app_config_proof_db.rs`).
///
/// **Cold-build outcome semantics:**
/// - `Some(entry)` published — proof is valid. The fast-path
///   consumer can rely on the fact-signature for warm-hit revalidation.
/// - On `FactReadSetFinalise::Overflow` — refuse cache admission;
///   the next call cold-recomputes. The provenance counter
///   `app_config_proof_overflow_refusals` advances.
///
/// Resolver-tier producer that takes `&dyn ResolverContext` to stay
/// inside the seal contract (`no_concrete_verter_host_in_seal_scope`
/// arch guard). Integration tests reach this via the
/// crate-public wrapper
/// [`crate::for_tests::app_config_no_override_proof_get_or_compute_for_tests`].
///
/// The ComponentConfig theme-variant fast-path resolver (a future
/// re-introduction of the retired rescue cascade) and the
/// Block-1.H Track-2.5 deferred test both reach this producer.
pub(crate) fn app_config_no_override_proof_get_or_compute(
    ctx: &dyn crate::resolver_core::ResolverContext,
    key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
) -> Option<Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
    let host = ctx.host_for_fact_tracer_install();
    let db = host.project_type_store.app_config_no_override_proof_db();
    // Warm-hit peek — validate the cached fact_dep_signature against
    // the live store view. The peek bubbles the signature into any
    // active outer tracer on success.
    if let Some(entry) = db.peek(key, ctx) {
        return Some(entry);
    }

    // Cold compute. The closure observes the decl-canonical's whole
    // hash so an edit invalidates the proof.
    let (decl_canonical, _component_key_literal) = key;
    let decl_canonical_for_compute = Arc::clone(decl_canonical);
    let cold_body = move || -> bool {
        // Look up the IndexedReady for the decl canonical. The
        // tracer fan-out picks up any indirect observations the
        // resolver substrate emits.
        //
        // Content-pinned: the observed `FileWholeHash` fact becomes
        // part of this proof entry's `read_set_signature`. A permissive
        // `get_any` could observe a stale artifact's `whole_hash`,
        // sealing the proof against a content hash that is no longer
        // current. A stale candidate is treated identically to "file
        // removed" — `current_content_pinned_indexed` returns `None`,
        // the sentinel-zero hash is observed, and the validator
        // re-derives the proof on the next read.
        let ir = ctx.indexed_for_current_content(decl_canonical_for_compute.as_ref());
        // Observe the file's whole-hash explicitly. If no IndexedReady
        // is present (file removed), record a sentinel zero hash so
        // the validator picks up the absence on the next read.
        let whole_hash = ir.as_ref().map(|ir| ir.whole_hash).unwrap_or_default();
        ctx.observe(crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: decl_canonical_for_compute.as_ref().to_string(),
            hash: whole_hash,
        });
        // Block 1.H Track 2.4: the "no override" determination is a
        // structural query into the interface members. For the
        // producer's substrate-correctness contract, the
        // `declares_interface_app_config` flag short-circuits the
        // walk: a file without `interface AppConfig` cannot
        // contribute an override.
        //
        // Files that DO declare `interface AppConfig` participate in
        // the proof's fact_dep_signature via the file_whole_hash
        // observation above; any edit to the interface body shifts
        // the whole-hash and invalidates the proof. This is the
        // R3/R26/R28 substrate contract — the producer does NOT
        // need to walk the interface body to decide the proof's
        // validation oracle.
        ir.as_ref()
            .map(|ir| !ir.declares_interface_app_config)
            .unwrap_or(true)
    };
    let (no_override, finalise) =
        crate::fact_signature_helpers::install_fact_tracer(host, cold_body);
    host.provenance
        .app_config_proof_fact_tracer_installs
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
            if !no_override {
                // The file declares `interface AppConfig` — we
                // cannot prove "no override" without walking the
                // member set. Decline to publish; the fast-path
                // consumer must take the slow path.
                return None;
            }
            db.publish(key.clone(), Arc::clone(&fact_dep_signature));
            Some(Arc::new(
                crate::app_config_proof_db::AppConfigNoOverrideProofEntry { fact_dep_signature },
            ))
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            host.provenance
                .app_config_proof_overflow_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        }
    }
}
