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
//!   [`cooperative_get_or_insert_with_post_publish`]. Two consume the
//!   [`ComputeAdmission`](crate::cooperative_admission::ComputeAdmission)
//!   API of `cooperative_admit_with_post_publish` so a
//!   valid-but-non-cacheable cold outcome is broadcast to joiners
//!   without admitting the cache: `ImportedRegistryDb` via its
//!   `get_or_compute_admit` method (the producer runs its
//!   fuse-consuming resolution inside the singleflight `compute`
//!   closure), and `MaterializeStructureDb` via the materialiser's
//!   direct use of its `entries()` / `inflight()` accessors.
//!
//! ## Live-counter accounting invariant
//!
//! The shared `component_meta_cache_live` counter must equal the number
//! of entries actually live in the cache maps on EVERY admission path.
//! Every wrapper therefore bumps the counter in the substrate's
//! winner-only `post_publish` callback — fired exactly once, after
//! `map.insert` and a successful `revalidate_after_compute` — and
//! decrements it in the `removal_cleanup` callback (and on every direct
//! map removal: per-canonical / project-generation invalidation, budget
//! eviction, schema-mismatch evict — plus, for the two retention-bounded
//! caches whose `peek` reaps stale entries, that stale-`peek` reap; the
//! `cooperative_get_or_insert` engine wrappers below have non-reaping
//! `peek`s). The increment is
//! never placed in the `compute` closure: a cold compute that fails
//! `revalidate_after_compute` (a project-generation reset landed during
//! the cold window) publishes no map entry and the substrate runs no
//! `removal_cleanup`, so a pre-publication bump would leak permanently.
//! `post_publish` is structurally unreachable on the revalidation-fail
//! path, so the bump is correct-by-construction: an entry contributes
//! `+1` exactly while it is live in the map.
//! - `validate(&Entry)` and `revalidate_after_compute(&Entry)` reject
//!   entries whose `read_set_signature.facts` no longer validate
//!   against the live `StoreView`.
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

use crate::cooperative_admission::{cooperative_get_or_insert_with_post_publish, InflightTable};
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
    /// Project generation this entry was computed under, snapshotted by
    /// the producer before the cold compute dispatched any work. The
    /// `fact_dep_signature` carrier validates only file-content
    /// whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias /
    /// SDK / workspace-folder change) bumps no file content, so without
    /// this field a stale-by-project-generation entry would validate
    /// forever. Every read-side gate ([`ImportedRegistryDb::peek`], the
    /// cooperative `validate` closure, the cooperative
    /// `revalidate_after_compute` closure) rejects the entry when
    /// `validated_at_generation` differs from the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    pub validated_at_generation: u64,
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
        // The carrier validates only file-content whole-hashes; a
        // `ProjectGeneration` reset bumps no file content, so the
        // generation gate is the project-shape counterpart of the
        // carrier check. A stale-by-project-generation entry is rejected
        // here even though its `fact_dep_signature` still validates.
        if entry_arc.validated_at_generation
            == ctx.project_type_store().current_project_generation()
            && validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            )
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
                // The carrier validates only file-content whole-hashes;
                // the generation gate is the project-shape counterpart
                // (a `ProjectGeneration` reset bumps no file content).
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
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
                // Post-compute revalidation — generation gate plus the
                // file-content carrier check, mirroring the warm-hit
                // `validate` arm. A project-generation reset that landed
                // during the cold window rejects the entry here.
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            // Removal-side counterpart of `post_publish`: when the
            // substrate removes an already-published entry (warm-hit
            // reject or joiner-fork reject) the live counter must
            // decrement and the per-canonical reverse-index
            // registration must drop, symmetric with the
            // `post_publish` bump + register. Without this a
            // joiner-fork over-counts the live counter and leaves a
            // dangling reverse-index entry.
            //
            // The `unregister` is identity-checked against the removed
            // entry's `EntryIdentity`: the substrate's `map.remove_if`
            // and this cleanup are not atomic, so a cold re-publish
            // could land a FRESH entry under the same key (and
            // `register` it) in between. A key-only unregister would
            // then delete that fresh registration and orphan a live
            // entry from `invalidate_canonical`'s reverse-index drain.
            // The identity carried here names the entry this cleanup
            // actually removed, so a re-published entry's registration
            // is preserved.
            move |removed_key: &ImportedRegistryKey, removed_entry: &Arc<ImportedRegistryEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
                canonical_index_for_removal.unregister(
                    removed_key.0.as_ref(),
                    removed_key,
                    crate::invalidation_domain::EntryIdentity::of(removed_entry),
                );
            },
            move |entry_arc: &Arc<ImportedRegistryEntry>, _key: &ImportedRegistryKey| {
                // Fires AFTER entries.insert AND AFTER successful
                // post-compute revalidation — only for the `Cacheable`
                // arm. A `ReturnOnly` outcome is NOT admitted, so the
                // live counter is bumped and the reverse index is
                // registered exactly when the entry actually lands.
                live_counter.fetch_add(1, Ordering::Relaxed);
                let canonical = Arc::clone(&key_for_post_publish.0);
                canonical_index.register(
                    &canonical,
                    key_for_post_publish.clone(),
                    crate::invalidation_domain::EntryIdentity::of(entry_arc),
                );
            },
            // `ImportedRegistryDb` carries no `GlobalRetentionBudget` —
            // its reverse index is the `CanonicalIndex` and there is no
            // map/budget desync class to fence. No publish fence.
            None,
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
        let identity = crate::invalidation_domain::EntryIdentity::of(&entry);
        self.canonical_index
            .register(&canonical, key.clone(), identity);
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback (after `map.insert` + a
        // successful `revalidate_after_compute`), NOT in `compute` — a
        // pre-publication bump leaks when post-compute revalidation
        // rejects the entry (no map entry, no `removal_cleanup`).
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment:
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
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work. The carrier validates only file-content
        // whole-hashes; a `ProjectGeneration` reset bumps no file
        // content, so the entry carries its compute-time generation
        // explicitly. The read-side gates reject the entry once the live
        // generation moves past this snapshot.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &DeclarationLookupEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| DeclarationLookupEntry {
                    value: Arc::new(value),
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &DeclarationLookupEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &DeclarationLookupEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &DeclarationLookupKey, _entry: &Arc<DeclarationLookupEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry and is
            // structurally unreachable on the revalidation-fail path.
            move |_entry_arc: &Arc<DeclarationLookupEntry>, _key: &DeclarationLookupKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries, not lifetime
        // inserts.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed source canonical is the entry's self-root — strict
        // warm-read validation rejects a same-canonical edit or an
        // untracked keyed canonical.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &ResolvabilityEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value)
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| ResolvabilityEntry {
                    value,
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &ResolvabilityEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value
            },
            |entry: &ResolvabilityEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &ResolvabilityKey, _entry: &Arc<ResolvabilityEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<ResolvabilityEntry>, _key: &ResolvabilityKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The owner canonical is the entry's self-root. This cache is
        // body-bearing (stores a `TypeExpr`), so strict self-root
        // validation is the correctness floor — a content edit to the
        // owner file invalidates the cached collection expression.
        let self_roots: [&str; 1] = [key.0.as_ref()];
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &OwnerCollectionEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| OwnerCollectionEntry {
                    owner_canonical,
                    value: value.map(Arc::new),
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &OwnerCollectionEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &OwnerCollectionEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &OwnerCollectionKey, _entry: &Arc<OwnerCollectionEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<OwnerCollectionEntry>, _key: &OwnerCollectionKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
    /// Canonicals validated **strictly** as self-roots on every warm
    /// read and post-compute revalidation: the active scope, the
    /// original declaring canonical, AND the FINAL routed declaring
    /// canonical (when the requested name re-exports through an
    /// intermediate module to a third file). The cache key only
    /// encodes the active scope + original declaring canonical, so the
    /// entry carries the routed canonical explicitly — a content edit
    /// to the third declaring file rejects the entry.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
    ///
    /// Validation uses the entry's OWN `self_root_canonicals` — the
    /// active scope, the original declaring canonical, AND the final
    /// routed declaring canonical (when the requested name re-exports
    /// through an intermediate module). The cache key only encodes the
    /// first two, so the entry carries the routed canonical explicitly;
    /// validating from `key`-derived self-roots alone would leave a
    /// content edit to the third declaring file undetected.
    pub(crate) fn peek(
        &self,
        key: &PreparedTargetCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<(Arc<str>, Arc<str>)>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        let self_roots: Vec<&str> = entry_arc
            .self_root_canonicals
            .iter()
            .map(Arc::as_ref)
            .collect();
        // The carrier validates only file-content whole-hashes; the
        // generation gate is the project-shape counterpart (a
        // `ProjectGeneration` reset bumps no file content).
        if entry_arc.validated_at_generation
            == ctx.project_type_store().current_project_generation()
            && validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            )
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
        F: FnOnce() -> Option<(
            Option<(Arc<str>, Arc<str>)>,
            Arc<[FactVersionRef]>,
            Arc<[Arc<str>]>,
        )>,
    {
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries. The cooperative
        // substrate runs this on its own warm-hit stale-eviction path,
        // so a lazily-evicted entry decrements `live_counter`
        // symmetrically with the `post_publish` increment.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The entry's `self_root_canonicals` (active scope + original
        // declaring canonical + final routed declaring canonical)
        // validate strictly. Reading them from the entry — not the
        // key — covers the routed third declaring file the key never
        // encodes.
        let validate = |entry: &PreparedTargetEntry| -> Option<Option<(Arc<str>, Arc<str>)>> {
            let self_roots: Vec<&str> =
                entry.self_root_canonicals.iter().map(Arc::as_ref).collect();
            // The carrier validates only file-content whole-hashes; the
            // generation gate is the project-shape counterpart.
            if entry.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && validate_fact_signature_with_self_roots(
                    ctx,
                    &entry.fact_dep_signature,
                    &self_roots,
                )
            {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                Some(entry.value.clone())
            } else {
                None
            }
        };
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            validate,
            move || {
                compute().map(|(value, fact_dep_signature, self_root_canonicals)| {
                    PreparedTargetEntry {
                        value,
                        fact_dep_signature,
                        self_root_canonicals,
                        validated_at_generation: generation_snapshot,
                    }
                })
            },
            |entry: &PreparedTargetEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedTargetEntry| {
                let self_roots: Vec<&str> =
                    entry.self_root_canonicals.iter().map(Arc::as_ref).collect();
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &PreparedTargetCacheKey, _entry: &Arc<PreparedTargetEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<PreparedTargetEntry>, _key: &PreparedTargetCacheKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        // Match an entry when `canonical_id` is the active scope, the
        // original declaring canonical, OR a routed declaring file in
        // the entry's `self_root_canonicals`. The cache key encodes only
        // the first two; a `PreparedTarget` whose requested name
        // re-exports through an intermediate module carries the final
        // routed declaring canonical in `self_root_canonicals` only.
        // Editing that third file must invalidate the entry — scanning
        // the entry's own self-roots makes this invalidation complete
        // (an entry missed here would later be rejected lazily by the
        // generic validator path, which does not decrement this DB's
        // `live_counter`).
        let keys: Vec<PreparedTargetCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                let matches = key.active_scope_canonical_id.as_ref() == canonical_id
                    || key.decl_canonical_id.as_ref() == canonical_id
                    || entry
                        .value()
                        .self_root_canonicals
                        .iter()
                        .any(|c| c.as_ref() == canonical_id);
                if matches {
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
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: 0,
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
// 6. ShapeCacheDb — `ShapeCacheKey → MaterializedTypeExpr`
//
// Universal shape cache. Replaces the previously-split
// `MaterializeMemoDb` (TypeExpr-keyed) and `MemberShapeCacheDb`
// (SemanticNode-keyed) by lifting the discriminant into a
// `ShapeSubject` variant. ONE cache, not two. Every shape lookup at
// every projection boundary goes through this cache.
//
// The key shape:
//
//   ShapeCacheKey {
//       subject: ShapeSubject {
//           TypeExpr { scope, expr } | SemanticNode { scope, node }
//       },
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

#[derive(Clone)]
pub struct ShapeCacheEntry {
    pub value: MaterializedTypeExpr,
    /// R3/R26/R28 fact-precise dependency signature. See
    /// [`ImportedRegistryEntry::fact_dep_signature`] for contract.
    pub fact_dep_signature: Arc<[FactVersionRef]>,
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    ///
    /// Overlay/base isolation for `SemanticNode`-subject entries does
    /// NOT rely on `SemanticNodeId` being generation-tagged (the arena
    /// is append-only across generations and IDs are raw `u64`).
    /// Isolation comes from three mechanisms working together:
    ///   1. `observe_materialize_scope` is overlay-aware and pins the
    ///      overlay `IndexedReady` when an overlay covers the scope.
    ///   2. The fact signature self-roots on that observation's
    ///      `whole_hash`. A base-mode peek against an overlay-rooted
    ///      entry fails `validate_fact_signature_with_self_roots`.
    ///   3. This field plus `bump_project_generation_and_evict`
    ///      detect cross-generation drift on overlay open/close.
    pub validated_at_generation: u64,
}

/// Subject of a [`ShapeCacheKey`] — the *what* whose shape is cached.
///
/// `TypeExpr` covers callers whose start point is a parser-produced
/// `TypeExpr` annotation (the legacy `MaterializeMemoDb` shape).
/// `SemanticNode` covers callers whose start point is a settled
/// `SemanticNodeId` (the legacy `MemberShapeCacheDb` shape). Both
/// subjects share the same cache substrate — they differ only in the
/// identity used to key entries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShapeSubject {
    /// TypeExpr-keyed subject. Sibling members of the same
    /// `Pick<Foo, 'a' | 'b'>` raise hash to distinct entries because
    /// the raised `TypeExpr` is structurally distinct per member —
    /// callers seeking per-member dedup should prefer the
    /// `SemanticNode` subject.
    TypeExpr {
        scope: Arc<str>,
        expr: Arc<TypeExpr>,
    },
    /// SemanticNode-keyed subject. Sibling members whose
    /// `SurfaceMember.value` is the same settled graph node collapse
    /// onto each other's warm hits.
    SemanticNode {
        scope: Arc<str>,
        node: crate::semantic_query::SemanticNodeId,
    },
}

impl ShapeSubject {
    /// Canonical scope id this subject is rooted in. Used for
    /// strict self-root warm-read validation and per-canonical
    /// invalidation.
    pub(crate) fn scope_canonical(&self) -> &Arc<str> {
        match self {
            ShapeSubject::TypeExpr { scope, .. } | ShapeSubject::SemanticNode { scope, .. } => {
                scope
            }
        }
    }
}

/// Per-call demand a [`ShapeCacheKey`] addresses — the *how* the shape
/// will be consumed. Distinct demands for the same subject keep
/// disjoint entries (e.g. Shallow vs Expanded over the same TypeExpr,
/// or `Published(Navigate)` vs `StructuralTransit(Navigate)` over the
/// same expression).
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
    /// keyed disjointly so cache slots split per the codex Q3-V
    /// finding ("ShapeCacheKey must carry the complete demand/context,
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
    /// Mode-only callers route through [`ShapeCacheKey::type_expr_whole`]
    /// / [`ShapeCacheKey::semantic_node_whole`], which wrap the mode in
    /// `ProjectionReductionContext::published(mode)` for the
    /// implicit-Published default. Demand-explicit callers build the
    /// context themselves and use the `_with_context` constructors.
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeCacheKey {
    pub(crate) subject: ShapeSubject,
    pub(crate) demand: ShapeDemand,
}

impl ShapeCacheKey {
    /// Construct a TypeExpr-subject whole-subject key (the default
    /// for callers that have not adopted path-precise demand). The
    /// terminal context is implicitly `Published(mode)` for backwards-
    /// compatible whole-subject lookups.
    pub(crate) fn type_expr_whole(
        scope: Arc<str>,
        expr: Arc<TypeExpr>,
        mode: ProjectionMode,
    ) -> Self {
        Self::type_expr_whole_with_context(
            scope,
            expr,
            crate::semantic_query::ProjectionReductionContext::published(mode),
        )
    }

    /// Construct a TypeExpr-subject whole-subject key under an
    /// explicit [`ProjectionReductionContext`]. The context
    /// discriminator keeps the TypeExpr field materialiser's
    /// per-prop `Published(Navigate)` publication slot disjoint from
    /// a `StructuralTransit(Navigate)` carrier-lower slot — same
    /// subject, distinct cache entries.
    pub(crate) fn type_expr_whole_with_context(
        scope: Arc<str>,
        expr: Arc<TypeExpr>,
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            subject: ShapeSubject::TypeExpr { scope, expr },
            demand: ShapeDemand::whole_subject_with_context(terminal_context),
        }
    }

    /// Construct a key for the legacy `MemberShapeCacheDb` shape
    /// (SemanticNode-subject, whole-subject demand). Terminal
    /// context is implicitly `Published(mode)`.
    pub(crate) fn semantic_node_whole(
        scope: Arc<str>,
        node: crate::semantic_query::SemanticNodeId,
        mode: ProjectionMode,
    ) -> Self {
        Self::semantic_node_whole_with_context(
            scope,
            node,
            crate::semantic_query::ProjectionReductionContext::published(mode),
        )
    }

    /// Construct a SemanticNode-subject whole-subject key under an
    /// explicit [`ProjectionReductionContext`].
    pub(crate) fn semantic_node_whole_with_context(
        scope: Arc<str>,
        node: crate::semantic_query::SemanticNodeId,
        terminal_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Self {
        Self {
            subject: ShapeSubject::SemanticNode { scope, node },
            demand: ShapeDemand::whole_subject_with_context(terminal_context),
        }
    }
}

pub struct ShapeCacheDb {
    entries: DashMap<ShapeCacheKey, Arc<ShapeCacheEntry>>,
    inflight: InflightTable<ShapeCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl ShapeCacheDb {
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
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<MaterializedTypeExpr> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation rejects a same-scope content
        // edit.
        let scope = key.subject.scope_canonical().clone();
        let self_roots: [&str; 1] = [scope.as_ref()];
        let result = (|| -> Option<MaterializedTypeExpr> {
            let entry_arc = self.entries.get(key).map(|e| e.clone())?;
            // The carrier validates only file-content whole-hashes; the
            // generation gate is the project-shape counterpart.
            if entry_arc.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && validate_fact_signature_with_self_roots(
                    ctx,
                    &entry_arc.fact_dep_signature,
                    &self_roots,
                )
            {
                bubble_fact_signature(ctx, &entry_arc.fact_dep_signature);
                Some(entry_arc.value.clone())
            } else {
                None
            }
        })();
        if let Some(rctx) = crate::request_context::current_request_context() {
            let counter = match &key.subject {
                ShapeSubject::TypeExpr { .. } => &rctx.cache_counters.materialize_memo,
                ShapeSubject::SemanticNode { .. } => &rctx.cache_counters.member_shape_cache,
            };
            if result.is_some() {
                counter.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                counter.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<MaterializedTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedTypeExpr, Arc<[FactVersionRef]>)>,
    {
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation.
        let scope = key.subject.scope_canonical().clone();
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        let scope_for_validate = Arc::clone(&scope);
        let scope_for_revalidate = Arc::clone(&scope);
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            move |entry: &ShapeCacheEntry| {
                let self_roots: [&str; 1] = [scope_for_validate.as_ref()];
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| ShapeCacheEntry {
                    value,
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &ShapeCacheEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            move |entry: &ShapeCacheEntry| {
                let self_roots: [&str; 1] = [scope_for_revalidate.as_ref()];
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &ShapeCacheKey, _entry: &Arc<ShapeCacheEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<ShapeCacheEntry>, _key: &ShapeCacheKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
        )
    }

    /// Universal-caching admission helper. Admits an already-computed
    /// `(value, fact_dep_signature)` pair into the cache when the
    /// signature is valid. The single centralised admission point —
    /// `admit_member_shape_if_possible` in the projector pipeline
    /// computes the `fact_dep_signature` upfront and delegates here
    /// rather than duplicating the `get_or_compute` plumbing.
    ///
    /// Returns the admitted value (verbatim if admission was refused
    /// — e.g. when the cache's signature check on the live `StoreView`
    /// rejects the entry — the caller still receives the same value
    /// it computed).
    pub(crate) fn admit_computed(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
        value: MaterializedTypeExpr,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) -> MaterializedTypeExpr {
        let value_for_closure = value.clone();
        let admitted = self.get_or_compute(key, ctx, move || {
            Some((value_for_closure, fact_dep_signature))
        });
        admitted.unwrap_or(value)
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
        let key = ShapeCacheKey::type_expr_whole(
            Arc::from(marker),
            Arc::new(TypeExpr::Unknown { raw: String::new() }),
            ProjectionMode::Shallow,
        );
        let entry = Arc::new(ShapeCacheEntry {
            value: MaterializedTypeExpr {
                node_id: None,
                type_expr: TypeExpr::Unknown { raw: String::new() },
                dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
            },
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
            validated_at_generation: 0,
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    // -----------------------------------------------------------------
    // Synthetic-carrier explicit-deepen positive-proof helpers
    // -----------------------------------------------------------------
    //
    // These helpers exercise the documented legitimate cache route for
    // deepening a `TypeExpr::SyntheticSlotBinding(SyntheticCarrierKey)`
    // carrier into its underlying member shape, per the
    // `[[component-meta-shallow-by-default-rule]]` and the
    // `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
    // architecture guard.
    //
    // The contract: the ONLY legitimate way to deepen a carrier is to
    // construct
    //   `ShapeCacheKey::semantic_node_whole(carrier.scope_canonical_id,
    //                                       SemanticNodeId(carrier.value_node),
    //                                       mode)`
    // and consult `ShapeCacheDb`. Zero production consumers exercise
    // this route today — every projector, reducer, registry, and
    // graph-builder site refuses the carrier as a shallow terminal.
    // The positive-proof integration test
    // `tests/synthetic_carrier_explicit_deepen_proof.rs` uses these
    // helpers to prove the cache-key identity round-trip is
    // well-defined for any future consumer that needs it.

    /// Insert a synthetic-carrier-deep entry into the cache under the
    /// legitimate cache-route identity. The key is built via
    /// `ShapeCacheKey::semantic_node_whole(scope, SemanticNodeId(carrier.value_node), mode)`
    /// — the exact shape the architecture guard mandates. Stored as a
    /// `MaterializedTypeExpr` whose `type_expr` is the requested deep
    /// type so a subsequent peek through the same legitimate route
    /// returns the deep shape, not the carrier.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_carrier_deep_for_test(
        &self,
        carrier: &verter_type_expr::SyntheticCarrierKey,
        mode: ProjectionMode,
        deep_type: TypeExpr,
    ) {
        use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
        let key = ShapeCacheKey::semantic_node_whole(
            carrier.scope_canonical_id.clone(),
            crate::semantic_query::SemanticNodeId(carrier.value_node),
            mode,
        );
        // Reuse the cache-route node identity for provenance — keeps a
        // single `SemanticNodeId(_.value_node)` construction site
        // (already routed through the cache factory above) so the
        // architecture guard's negative-grep scanner sees only the
        // legitimate cache-route shape.
        let node_for_provenance = match &key.subject {
            ShapeSubject::SemanticNode { node, .. } => Some(*node),
            ShapeSubject::TypeExpr { .. } => None,
        };
        let entry = Arc::new(ShapeCacheEntry {
            value: MaterializedTypeExpr {
                node_id: node_for_provenance,
                type_expr: deep_type,
                dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
            },
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
            validated_at_generation: 0,
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Peek a synthetic-carrier-deep entry out of the cache through
    /// the legitimate cache-route identity. Bypasses the full
    /// `ResolverContext`-gated `peek` so the positive-proof test does
    /// not need to stand up a host. Returns the materialised deep
    /// `TypeExpr` if an entry exists for this carrier identity, or
    /// `None` otherwise.
    #[cfg(any(test, debug_assertions))]
    pub fn get_synthetic_carrier_deep_for_test(
        &self,
        carrier: &verter_type_expr::SyntheticCarrierKey,
        mode: ProjectionMode,
    ) -> Option<TypeExpr> {
        let key = ShapeCacheKey::semantic_node_whole(
            carrier.scope_canonical_id.clone(),
            crate::semantic_query::SemanticNodeId(carrier.value_node),
            mode,
        );
        self.entries
            .get(&key)
            .map(|entry| entry.value.type_expr.clone())
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
            // The carrier validates only file-content whole-hashes; the
            // generation gate is the project-shape counterpart.
            if entry_arc.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && validate_fact_signature_with_self_roots(
                    ctx,
                    &entry_arc.fact_dep_signature,
                    &self_roots,
                )
            {
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed canonical is the entry's self-root — strict
        // warm-read validation (body-sensitive surface).
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedSurfaceEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| PreparedSurfaceEntry {
                    value,
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &PreparedSurfaceEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedSurfaceEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &PreparedSurfaceCacheKey, _entry: &Arc<PreparedSurfaceEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<PreparedSurfaceEntry>, _key: &PreparedSurfaceCacheKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
            from_root_body: true,
        };
        let entry = Arc::new(PreparedSurfaceEntry {
            value: PreparedSurfacePayload::Empty,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
            validated_at_generation: 0,
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
            // The carrier validates only file-content whole-hashes; the
            // generation gate is the project-shape counterpart.
            if entry_arc.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && validate_fact_signature_with_self_roots(
                    ctx,
                    &entry_arc.fact_dep_signature,
                    &self_roots,
                )
            {
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed canonical is the entry's self-root — strict
        // warm-read validation.
        let self_roots: [&str; 1] = [key.canonical_id.as_ref()];
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedMemberEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| PreparedMemberEntry {
                    value: value.map(Arc::new),
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &PreparedMemberEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &PreparedMemberEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &PreparedMemberCacheKey, _entry: &Arc<PreparedMemberEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<PreparedMemberEntry>, _key: &PreparedMemberCacheKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
            from_root_body: true,
        };
        let entry = Arc::new(PreparedMemberEntry {
            value: None,
            fact_dep_signature: crate::fact_signature_helpers::empty_fact_signature(),
            validated_at_generation: 0,
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
    /// Project generation this entry was computed under. See
    /// [`ImportedRegistryEntry::validated_at_generation`] for the
    /// project-generation staleness contract.
    pub validated_at_generation: u64,
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
        // The carrier validates only file-content whole-hashes; the
        // generation gate is the project-shape counterpart (a
        // `ProjectGeneration` reset bumps no file content).
        if entry_arc.validated_at_generation
            == ctx.project_type_store().current_project_generation()
            && validate_fact_signature_with_self_roots(
                ctx,
                &entry_arc.fact_dep_signature,
                &self_roots,
            )
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
        // Publish-side counterpart: the live counter is bumped in the
        // winner-only `post_publish` callback, NOT in `compute` — see
        // `DeclarationLookupDb::get_or_compute` for the leak rationale.
        let live_counter_for_publish = Arc::clone(&self.live_counter);
        // Removal-side counterpart of the `post_publish` increment —
        // decrements when the substrate removes an already-published
        // entry so the live counter tracks live entries.
        let live_counter_for_removal = Arc::clone(&self.live_counter);
        // The keyed scope canonical is the entry's self-root — strict
        // warm-read validation.
        let self_roots: [&str; 1] = [key.scope_canonical_id.as_ref()];
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work — see `DeclarationLookupDb::get_or_compute`
        // for the project-generation staleness rationale.
        let generation_snapshot = ctx.project_type_store().current_project_generation();
        cooperative_get_or_insert_with_post_publish(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &RoutedExprSurfaceEntry| {
                if entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
                {
                    bubble_fact_signature(ctx, &entry.fact_dep_signature);
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, fact_dep_signature)| RoutedExprSurfaceEntry {
                    value: Arc::new(value),
                    fact_dep_signature,
                    validated_at_generation: generation_snapshot,
                })
            },
            |entry: &RoutedExprSurfaceEntry| {
                bubble_fact_signature(ctx, &entry.fact_dep_signature);
                entry.value.clone()
            },
            |entry: &RoutedExprSurfaceEntry| {
                entry.validated_at_generation
                    == ctx.project_type_store().current_project_generation()
                    && validate_fact_signature_with_self_roots(
                        ctx,
                        &entry.fact_dep_signature,
                        &self_roots,
                    )
            },
            move |_key: &RoutedExprSurfaceCacheKey, _entry: &Arc<RoutedExprSurfaceEntry>| {
                live_counter_for_removal.fetch_sub(1, Ordering::Relaxed);
            },
            // post_publish — fires once on the cold winner's thread,
            // AFTER `entries.insert` AND a successful
            // `revalidate_after_compute`. The live-counter bump rides
            // here so it is paired with the published map entry.
            move |_entry_arc: &Arc<RoutedExprSurfaceEntry>, _key: &RoutedExprSurfaceCacheKey| {
                live_counter_for_publish.fetch_add(1, Ordering::Relaxed);
            },
            // No retention budget on this cache shape — no publish fence.
            None,
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
            validated_at_generation: 0,
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
/// and `Tainted` are non-cacheable per-call sentinels), the
/// observed-root carrier, and the explicit self-root canonical set.
///
/// The carrier holds the path-precise fact signature observed during
/// the cold build (the materialiser's traced read set) — the sole
/// cache-validity rail. `self_root_canonicals` lists the canonicals
/// whose `FileWholeHash` fact the warm-read validator must check
/// **strictly** — ONLY the `base` node's declaration-origin file. The
/// consumer materialise scope is NOT a self-root: a value's identity
/// does not depend on which consumer reached it (R7 cross-owner
/// reuse). A `Global`-origin base yields an empty
/// `self_root_canonicals`. A same-canonical content edit, or a
/// self-root canonical the live store view no longer tracks, rejects
/// the entry through
/// [`crate::fact_signature_helpers::ReadSetSignature::validate_with_self_roots`].
#[derive(Clone)]
pub struct MaterializeStructureEntry {
    /// The cached outcome. ONLY `Value` or `Miss` may be stored here.
    /// The materialiser's publish path enforces this with
    /// `debug_assert!`.
    pub outcome: MaterializeOutcome,
    /// Carrier holding the path-precise fact signature — the sole
    /// cache-validity rail. Warm reads call
    /// `read_set_signature.validate_with_self_roots(ctx,
    /// &self_root_canonicals)` BEFORE bubbling. `canonical_ids()`
    /// drives the reverse-index registration.
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
    /// The cold build's dispatch-return signature — the materialiser's
    /// traced `local_fence` as a `DepSignature`. NOT a cache-validity
    /// rail (the carrier above is the sole validity oracle); it is the
    /// transitive-dependency payload a warm `peek` returns on
    /// `CacheRead.dep_signature` so the component-meta dispatch
    /// accumulator folds the warm sub-query's deps into the owner's
    /// `fact_versions`. Crate-private: the dispatch-return signature is
    /// an internal sibling rail, not a public cache-carrier field.
    pub(crate) dispatch_dep_signature: DepSignature,
    /// Canonicals validated **strictly** as self-roots on every warm
    /// read and post-compute revalidation: ONLY the `base` node's
    /// declaration-origin file (empty for a `Global`-origin base). The
    /// consumer materialise scope is NOT a self-root (R7 cross-owner
    /// reuse). An untracked or hash-mismatched self-root rejects the
    /// entry.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Retention-ledger admission identity — the unique sequence number
    /// this entry is recorded under in the `GlobalRetentionBudget`
    /// ledger. Allocated once at entry construction; `register_post_publish`
    /// records the entry under it, and every removal path forgets exactly
    /// this ledger record via `forget_seq`. Scoping the ledger removal to
    /// this seq (rather than the cache key) means a concurrently-admitted
    /// fresh entry that republished the same key keeps its own ledger
    /// record — symmetric with the `Arc::ptr_eq` guard on the
    /// reverse-index removal.
    pub admission_seq: u64,
    /// Project generation this entry was computed under, captured by the
    /// cold `compute` closure before it dispatched any work. The carrier
    /// (`read_set_signature`) validates only file-content whole-hashes;
    /// a `ProjectGeneration` reset (tsconfig / path-alias / SDK /
    /// workspace-folder change) bumps no file content, so without this
    /// field a stale-by-project-generation entry would validate forever.
    /// Every read-side gate (`peek`, the cooperative `validate` closure)
    /// AND the cooperative post-compute revalidation reject the entry
    /// when `validated_at_generation` differs from the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    /// The revalidation runs under the `MaterializeStructureDb`'s
    /// `retention_gate` read guard, so it is atomic against the
    /// `invalidate_all` clear+bump — a stale entry can neither survive a
    /// reset nor be published into a freshly-cleared cache.
    pub validated_at_generation: u64,
}

/// Per-canonical reverse index shared by [`MaterializeStructureDb`]
/// and [`RefCycleResultDb`]: maps each canonical id to the set of
/// cache keys whose carrier fact rail references it, paired with the
/// registered `ReadSetSignature.facts` `Arc` (the `Arc::ptr_eq`
/// discriminant for the per-canonical invalidation drain).
type CanonicalToKeysIndex<K> =
    DashMap<Arc<str>, parking_lot::Mutex<rustc_hash::FxHashMap<K, Arc<[FactVersionRef]>>>>;

/// Remove the `Arc::ptr_eq`-matching `key` registration from
/// `canonical`'s `canonical_to_keys` shard, then drop the outer shard
/// when that removal empties its inner map.
///
/// Shared by every `canonical_to_keys` removal path in
/// [`MaterializeStructureDb`] and [`RefCycleResultDb`] — budget
/// eviction, per-canonical-invalidation cross-canonical cleanup, and
/// cooperative-removal `unregister_post_publish`. Leaving an emptied
/// shard resident strands an empty `Mutex<map>` plus the canonical
/// `Arc<str>` for the lifetime of the project generation; under churn
/// across many distinct canonicals the reverse index would then grow
/// unbounded, defeating the bound the `GlobalRetentionBudget` exists to
/// enforce.
///
/// **Concurrency.** The outer `canonical_to_keys` is a `DashMap`. The
/// inner-map removal releases the per-canonical `Mutex` before the
/// outer drop is attempted; the outer drop is a single
/// [`DashMap::remove_if`] whose emptiness predicate runs while the
/// `DashMap` shard write lock is held. `register_post_publish`'s
/// inserter holds that same shard write lock for the whole
/// `entry(canonical).or_insert_with(...)` + inner `insert`, so the two
/// serialise: either the inserter runs first and the predicate observes
/// the inner map non-empty (drop skipped), or `remove_if` runs first,
/// drops the empty outer entry, and the inserter's later
/// `or_insert_with` re-creates a fresh shard cleanly. A registration is
/// never stranded in a just-removed outer entry. Mirrors the
/// `BoundedCandidateMap::remove_candidate_by_seq` slot-detach pattern.
fn prune_canonical_to_keys_registration<K>(
    canonical_to_keys: &CanonicalToKeysIndex<K>,
    canonical: &Arc<str>,
    key: &K,
    expected_facts: &Arc<[FactVersionRef]>,
) where
    K: Eq + std::hash::Hash,
{
    if let Some(shard) = canonical_to_keys.get(canonical) {
        let mut map = shard.lock();
        if let Some(existing_sig) = map.get(key) {
            // `Arc::ptr_eq` guard — drop only OUR registration, never a
            // concurrent fresh winner's that republished the same key.
            if Arc::ptr_eq(existing_sig, expected_facts) {
                map.remove(key);
            }
        }
    }
    // Drop the outer shard iff its inner map is now empty. The predicate
    // holds the `DashMap` shard write lock, so it cannot race a
    // concurrent `register_post_publish` inserter on this shard.
    canonical_to_keys.remove_if(canonical, |_, mutex| mutex.lock().is_empty());
}

/// Final-result cache for the structural
/// materialiser. Reverse-index `canonical_to_keys` enables
/// `Arc::ptr_eq`-based invalidation cleanup; cooperative-admission's
/// `post_publish` callback wires the registration.
///
/// The cache key carries a content-derived `SemanticNodeId`, so each
/// distinct content version of an owner produces a fresh entry. The
/// embedded [`crate::bounded_query_retention::GlobalRetentionBudget`] is
/// the routine memory-reclamation path: the `post_publish` hook records
/// each admission and FIFO-evicts the oldest entries past
/// [`Self::MAX_ENTRIES`], so a long-lived session does not accumulate
/// stale per-version structural materialisations unbounded.
pub struct MaterializeStructureDb {
    entries: DashMap<MaterializeStructureCacheKey, Arc<MaterializeStructureEntry>>,
    inflight: InflightTable<MaterializeStructureCacheKey>,
    /// Per-canonical reverse index — see [`CanonicalToKeysIndex`].
    /// `invalidate_for_canonical` drains this map and uses
    /// `Arc::ptr_eq` to discriminate stale entries from fresh
    /// post-publish writes.
    canonical_to_keys: CanonicalToKeysIndex<MaterializeStructureCacheKey>,
    /// Global insertion-ordered total-size budget. Bounds the total
    /// entry count across all keys — the reverse-index drain reclaims
    /// only on per-canonical invalidation, which an owner-content edit
    /// no longer triggers, so this budget is the routine reclamation.
    retention_budget:
        crate::bounded_query_retention::GlobalRetentionBudget<MaterializeStructureCacheKey>,
    live_counter: Arc<AtomicU64>,
    /// Lifecycle gate keeping `entries`, `canonical_to_keys`,
    /// `retention_budget`, and `live_counter` in one lock domain. Every
    /// mutation that touches the map and the budget — the cooperative
    /// publish (`map.insert` + `post_publish`), the cooperative removal
    /// cleanup, a budget-FIFO eviction, a stale-`peek` removal, and a
    /// per-canonical drain — runs under a shared read guard; the
    /// project-generation `clear` (`invalidate_all` /
    /// `evict_if_schema_mismatch`) takes the exclusive write guard
    /// across the whole map+index+budget+counter clear. A `clear`
    /// therefore cannot interleave its clears with a concurrent
    /// publish, so the budget never strands a live `entries` item with
    /// no admission record. `DashMap` stays for hot-path concurrency;
    /// the gate is a coarse reset fence.
    retention_gate: parking_lot::RwLock<()>,
    /// Test-only injection point inside [`Self::invalidate_all`],
    /// parked between the `entries` clear and the budget clear with the
    /// `retention_gate` write guard still held. A race test arms it
    /// with a barrier and calls `wait()` twice to assert the in-flight
    /// clear engages the gate against a concurrent publish. Per-instance;
    /// absent from release builds.
    #[cfg(any(test, debug_assertions))]
    invalidate_all_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::register_post_publish`],
    /// parked AFTER `retention_budget.record_admission` has returned its
    /// FIFO victims but BEFORE `evict_budget_victim` removes them. A race
    /// test arms it with a barrier and calls `wait()` twice to drive a
    /// concurrent same-key re-publish into that gap, then asserts the
    /// identity-scoped `evict_budget_victim` removes the OLD victim and
    /// leaves the fresh re-publish intact. Per-instance; absent from
    /// release builds.
    #[cfg(any(test, debug_assertions))]
    register_post_publish_pre_evict_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl MaterializeStructureDb {
    /// Total entry-count cap. A long-lived editor session that
    /// re-materialises many owner versions caps here; the oldest
    /// entries are FIFO-evicted on the write-side `post_publish` hook.
    pub const MAX_ENTRIES: usize = 2048;

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

    /// Test-only constructor pinning a small `retention_budget` cap so a
    /// reverse-index test drives FIFO eviction without admitting
    /// [`Self::MAX_ENTRIES`] (2048) entries.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_budget_for_test(budget_cap: usize) -> Self {
        let mut db = Self::with_counter_and_schema_version(
            Arc::new(AtomicU64::new(0)),
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        );
        db.retention_budget =
            crate::bounded_query_retention::GlobalRetentionBudget::new(budget_cap);
        db
    }

    /// Test-only — number of distinct outer shards currently resident
    /// in the `canonical_to_keys` reverse index. A budget eviction that
    /// empties a shard's inner map must drop the outer shard, so this
    /// count tracks the surviving canonicals — not the lifetime
    /// canonical count.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn canonical_to_keys_shard_count_for_test(&self) -> usize {
        self.canonical_to_keys.len()
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            retention_budget: crate::bounded_query_retention::GlobalRetentionBudget::new(
                Self::MAX_ENTRIES,
            ),
            live_counter,
            retention_gate: parking_lot::RwLock::new(()),
            #[cfg(any(test, debug_assertions))]
            invalidate_all_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            register_post_publish_pre_evict_gate: parking_lot::Mutex::new(None),
            schema_version,
        }
    }

    /// Configured total entry-count cap.
    #[must_use]
    pub fn retention_cap(&self) -> usize {
        self.retention_budget.cap()
    }

    /// Test-only — number of admission records currently in the
    /// retention ledger. The budget evicts the oldest once this exceeds
    /// [`Self::MAX_ENTRIES`], so it is the count that bounds the cache.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn retention_tracked_len(&self) -> usize {
        self.retention_budget.tracked_len()
    }

    /// Test-only accessor for the lifecycle `retention_gate`. A race
    /// test parks `invalidate_all` mid-flight (via the `invalidate_all`
    /// injection point) and uses `try_read()` on this gate to assert,
    /// deterministically, that the in-flight clear has engaged the
    /// write guard against concurrent publishes.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_retention_gate(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
    }

    /// Read-only test accessor for the shared
    /// live_counter. Used by the publish-fence / generation-revalidation
    /// race tests to verify that revalidation failures do NOT increment
    /// the counter (entries are removed without inflating the live
    /// count).
    #[cfg(test)]
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
            // Carrier-aware validate-before-bubble. The entry's
            // self-root canonicals (ONLY the `base` node's
            // declaration-origin file — NOT the consumer materialise
            // scope, R7 cross-owner reuse) validate **strictly** — an
            // untracked or hash-mismatched self-root rejects the
            // entry; every other fact keeps the lazy cross-file
            // permissiveness. A stale entry never bubbles into the
            // active outer tracer.
            //
            // The generation gate is the project-shape counterpart of
            // the carrier check: the carrier validates only file-content
            // whole-hashes, but a `ProjectGeneration` reset (tsconfig /
            // path-alias / SDK / workspace-folder change) bumps no file
            // content. An entry whose `validated_at_generation` no longer
            // equals the live project generation is stale even though its
            // carrier still validates — reap it.
            if entry_arc.validated_at_generation
                != ctx.project_type_store().current_project_generation()
                || !entry_arc
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry_arc.self_root_canonicals)
            {
                // Stale-entry reap touches `entries`, the
                // `canonical_to_keys` reverse index, and the retention
                // budget — hold the retention gate (shared read) across
                // the whole removal so it does not desync against a
                // concurrent project-generation `clear`.
                let _retention = self.retention_gate.read();
                let removed = self
                    .entries
                    .remove_if(key, |_, e| Arc::ptr_eq(e, &entry_arc));
                if removed.is_some() {
                    self.live_counter.fetch_sub(1, Ordering::Relaxed);
                    // Route the reap through the SAME cleanup the
                    // cooperative-removal path uses: `unregister_post_publish`
                    // unregisters every `canonical_to_keys` registration
                    // the reaped entry held (one per canonical its carrier
                    // referenced) and prunes the now-empty shards, then
                    // forgets exactly this entry's retention-ledger record
                    // via `forget_seq`. A bare `forget_seq` here would
                    // drop the ledger record but leave dead reverse-index
                    // shards resident for a multi-canonical entry.
                    self.unregister_post_publish(
                        key,
                        &entry_arc.read_set_signature,
                        entry_arc.admission_seq,
                    );
                }
                return None;
            }
            entry_arc.read_set_signature.bubble(ctx);
            crate::host_manage::record_materialize_structure_cache_hit();
            Some(crate::semantic_query::CacheRead {
                value: entry_arc.outcome.clone(),
                dep_signature: Arc::clone(&entry_arc.dispatch_dep_signature),
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

    /// Drop every cache entry whose carrier references `canonical_id`.
    /// Uses the `canonical_to_keys` reverse index to find affected
    /// keys; uses `Arc::ptr_eq` to discriminate "our entry" from
    /// concurrent fresh writes.
    ///
    /// Holds the `retention_gate` read guard across the whole
    /// reverse-index drain + `entries` removal + budget `forget_seq` +
    /// counter decrement, so this per-canonical invalidation does not
    /// desync the map and the budget against a concurrent
    /// project-generation `clear` (which takes the write guard).
    ///
    /// Each removed entry's reverse-index registrations and
    /// retention-ledger record are dropped by [`Self::unregister_post_publish`],
    /// the single removal-side cleanup helper. It iterates the entry's
    /// `read_set_signature.canonical_ids()` — the union of fact-rail
    /// canonicals (from `read_set_signature.facts`) and dispatch-fence
    /// canonicals (from `dispatch_dep_signature`) — the same set
    /// [`Self::register_post_publish`] registered under. The
    /// `canonical_id` shard itself is already drained by the
    /// `canonical_to_keys.remove` above; the helper's
    /// `prune_canonical_to_keys_registration` for that canonical is then
    /// a no-op (the shard is gone). The `live_counter` decrement stays
    /// here — `unregister_post_publish` does not touch the counter — so
    /// each removed entry nets exactly one decrement.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let _retention = self.retention_gate.read();
        let drained: Vec<(MaterializeStructureCacheKey, Arc<[FactVersionRef]>)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.read_set_signature.facts, &registered)
            });
            if let Some((_, removed_entry)) = removed {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Route reverse-index + retention-ledger cleanup through
                // the shared removal helper. It prunes the registration
                // under every canonical the carrier referenced
                // (`canonical_ids()` — legacy ∪ facts) and forgets
                // exactly this entry's ledger record (`forget_seq`),
                // identical to the cooperative-removal and stale-`peek`
                // reap paths — no second canonical-set derivation.
                self.unregister_post_publish(
                    key,
                    &removed_entry.read_set_signature,
                    removed_entry.admission_seq,
                );
            }
        }
    }

    /// Drop every cache entry. Used on project-generation bumps.
    ///
    /// Holds the `retention_gate` WRITE guard across the whole
    /// `entries` + `canonical_to_keys` + `retention_budget` +
    /// `live_counter` clear. A concurrent cooperative publish, removal
    /// cleanup, stale-`peek` reap, or per-canonical drain holds the
    /// gate's read guard, so it blocks until this clear completes — no
    /// publish can land a live `entries` item whose budget admission
    /// this reset then erases.
    ///
    /// Saturating-subtract pattern (NOT `store(0)`) because
    /// `live_counter` is shared via `Arc<AtomicU64>` across every typed DB
    /// in `ProjectTypeStore` (`component_meta_cache_live`). A per-DB
    /// `store(0)` would corrupt other DBs' contributions to the shared
    /// sum. Mirrors the existing `ImportedRegistryDb::invalidate_all`
    /// pattern — subtract only this DB's entry count, capped at the
    /// counter's current value to prevent underflow under
    /// concurrent invalidation.
    pub fn invalidate_all(&self) {
        let _retention = self.retention_gate.write();
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        // Test-only injection point — parked between the `entries`
        // clear and the budget clear with the `retention_gate` write
        // guard still held. A race test arms it to assert a concurrent
        // publish is blocked. `None` (production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.invalidate_all_midpoint_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.retention_budget.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    /// Test-only driver: arm the [`Self::invalidate_all`] injection
    /// point with `barrier`. The next `invalidate_all` on this Db calls
    /// `barrier.wait()` twice between the `entries` clear and the
    /// budget clear (with the `retention_gate` write guard held). The
    /// returned guard disarms the injection point on drop.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_invalidate_all_midpoint_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> MaterializeInvalidateGateGuard<'_> {
        *self.invalidate_all_midpoint_gate.lock() = Some(barrier);
        MaterializeInvalidateGateGuard {
            gate: &self.invalidate_all_midpoint_gate,
        }
    }

    /// Test-only driver: arm the [`Self::register_post_publish`]
    /// pre-eviction injection point with `barrier`. The next
    /// `register_post_publish` on this Db calls `barrier.wait()` twice
    /// AFTER `retention_budget.record_admission` returned its FIFO
    /// victims but BEFORE `evict_budget_victim` removes them. A race
    /// test uses this to interleave a concurrent same-key re-publish and
    /// prove the identity-scoped `evict_budget_victim` removes the OLD
    /// victim, not the fresh re-publish. The returned guard disarms the
    /// injection point on drop.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_register_post_publish_pre_evict_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> MaterializeRegisterPreEvictGateGuard<'_> {
        *self.register_post_publish_pre_evict_gate.lock() = Some(barrier);
        MaterializeRegisterPreEvictGateGuard {
            gate: &self.register_post_publish_pre_evict_gate,
        }
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
            dispatch_dep_signature: Arc::from(Vec::new()),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
            validated_at_generation: 0,
        });
        self.entries.insert(key, entry);
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Internal — register a `(key, fact_rail)` pair under every
    /// canonical the entry's carrier fact rail references. Called from
    /// the materialiser's `post_publish` callback. The reverse index
    /// drains under any canonical a fact names, so every fact-rail dep
    /// (`Parse(...)` / `ResolveImports(...)` / `RouteSurface(...)` /
    /// `FileWholeHash` / `DerivedFactHash`) invalidates the entry when
    /// its canonical changes.
    ///
    /// Also records the admission against the global retention budget
    /// and FIFO-evicts the oldest entries once the total exceeds
    /// [`Self::MAX_ENTRIES`]. Evicting a still-valid entry only causes
    /// a later recompute; it never produces an incorrect result.
    ///
    /// `admission_seq` is the published entry's
    /// [`MaterializeStructureEntry::admission_seq`] — the ledger records
    /// this entry under that exact identity, so a later removal path can
    /// forget precisely this ledger record (`forget_seq`) without
    /// dropping a concurrently-admitted fresh entry that republished the
    /// same key.
    pub(crate) fn register_post_publish(
        &self,
        key: MaterializeStructureCacheKey,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        admission_seq: u64,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let registered_facts = Arc::clone(&read_set_signature.facts);
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
            map.insert(key.clone(), Arc::clone(&registered_facts));
        }
        // Global retention budget: record this admission under the
        // entry's own seq, FIFO-evict the oldest entries past the total
        // cap. Each victim carries its admission `seq`, so
        // `evict_budget_victim` removes precisely that admission's entry
        // even if a concurrent same-key re-publish has overwritten the
        // map slot under `victim_key` with a fresh, distinctly-seq'd
        // entry.
        let victims = self.retention_budget.record_admission(admission_seq, key);
        // Test-only injection point — parked AFTER `record_admission`
        // returned its FIFO victims but BEFORE `evict_budget_victim`
        // removes them. A race test arms it to drive a concurrent
        // same-key re-publish into this gap. `None` (production default)
        // is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.register_post_publish_pre_evict_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        for (victim_seq, victim_key) in victims {
            self.evict_budget_victim(victim_seq, &victim_key);
        }
    }

    /// Remove one entry chosen by the retention budget for FIFO
    /// eviction. Drains the entry's reverse-index registrations under
    /// every canonical its carrier referenced and decrements the live
    /// counter, matching the removal-side cleanup the cooperative
    /// substrate runs for a warm-hit reject.
    ///
    /// **Identity-scoped removal.** The map entry under `victim_key` is
    /// removed ONLY when its stored `admission_seq` still equals
    /// `victim_seq`. A same-key re-publish racing this eviction
    /// overwrites the `victim_key` slot with a fresh entry carrying a
    /// distinct seq; a bare-key removal would evict that fresh entry and
    /// strand its live ledger record (cache grows past the cap). The
    /// `remove_if` predicate runs under the `DashMap` shard write lock,
    /// so the seq check and the removal are atomic against a concurrent
    /// re-publish.
    fn evict_budget_victim(&self, victim_seq: u64, victim_key: &MaterializeStructureCacheKey) {
        let Some((_, entry)) = self
            .entries
            .remove_if(victim_key, |_, e| e.admission_seq == victim_seq)
        else {
            return;
        };
        self.live_counter.fetch_sub(1, Ordering::Relaxed);
        // Drop each reverse-index registration; the helper also drops
        // the outer shard when the removal empties its inner map, so a
        // budget-driven eviction wave does not leave empty shards
        // resident.
        for canonical in entry.read_set_signature.canonical_ids() {
            prune_canonical_to_keys_registration(
                &self.canonical_to_keys,
                &canonical,
                victim_key,
                &entry.read_set_signature.facts,
            );
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

    /// Internal — the lifecycle `retention_gate`, passed as the
    /// `publish_fence` to `cooperative_admit_with_post_publish` so the
    /// substrate holds it across `entries.insert` + `post_publish`.
    /// Keeps the cooperative publish in the same lock domain as
    /// `invalidate_all`'s map+budget clear.
    pub(crate) fn publish_fence(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
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

    /// Complete removal-side cleanup, the counterpart of
    /// [`Self::register_post_publish`]. Two callers reach it: the
    /// cooperative-admission substrate when it removes one
    /// already-published entry (a warm-hit reject or a joiner-fork
    /// reject), and [`Self::peek`]'s stale/generation-mismatch reap. The
    /// entry's reverse-index registration must be dropped under EVERY
    /// canonical it referenced, symmetric with the per-canonical
    /// `register_post_publish` insert. `Arc::ptr_eq` against the entry's
    /// `legacy` rail discriminates "our registration" from a concurrent
    /// fresh winner's.
    ///
    /// `admission_seq` is the removed entry's
    /// [`MaterializeStructureEntry::admission_seq`]. The retention-ledger
    /// removal is scoped to that exact seq via `forget_seq` — a key-only
    /// `forget` would also drop a concurrently-admitted fresh entry's
    /// ledger record (a fresh winner can republish the same key in the
    /// `remove_if` → cleanup window), letting the fresh entry escape the
    /// budget count and the cache grow past `MAX_ENTRIES`. Identity
    /// scoping here mirrors the `Arc::ptr_eq` guard on the reverse-index
    /// removal above.
    pub(crate) fn unregister_post_publish(
        &self,
        key: &MaterializeStructureCacheKey,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        admission_seq: u64,
    ) {
        for canonical in read_set_signature.canonical_ids() {
            prune_canonical_to_keys_registration(
                &self.canonical_to_keys,
                &canonical,
                key,
                &read_set_signature.facts,
            );
        }
        // Keep the retention ledger consistent: drop exactly this
        // entry's ledger record, leaving a fresh re-admission of the
        // same key intact.
        self.retention_budget.forget_seq(admission_seq);
    }
}

impl Default for MaterializeStructureDb {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard returned by
/// [`MaterializeStructureDb::test_arm_invalidate_all_midpoint_gate`].
/// Disarms the per-instance `invalidate_all` injection point on drop.
#[cfg(test)]
#[doc(hidden)]
pub(crate) struct MaterializeInvalidateGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for MaterializeInvalidateGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}

/// RAII guard returned by
/// [`MaterializeStructureDb::test_arm_register_post_publish_pre_evict_gate`].
/// Disarms the per-instance pre-eviction injection point on drop so a
/// later `register_post_publish` does not park on a stale barrier.
#[cfg(test)]
#[doc(hidden)]
pub(crate) struct MaterializeRegisterPreEvictGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for MaterializeRegisterPreEvictGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
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
        // Same map + index + budget + counter clear as `invalidate_all`
        // — take the `retention_gate` write guard across the whole
        // clear so a concurrent publish cannot interleave.
        let _retention = self.retention_gate.write();
        let count = self.entries.len();
        self.entries.clear();
        self.canonical_to_keys.clear();
        self.retention_budget.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
        }
        count
    }
}

// ===========================================================================
// C — RefCycleResultDb
// ===========================================================================

use crate::cooperative_admission::{cooperative_admit_with_post_publish, ComputeAdmission};
use crate::semantic_query::DeclIdentity;

/// C — entry stored in `RefCycleResultDb`. Carries the
/// boolean BFS result, the observed-root carrier, and the explicit
/// self-root canonical set.
///
/// The carrier holds the path-precise fact signature observed during
/// the cold BFS — the sole cache-validity rail. The BFS records one
/// observed self-root per visited declaration identity (the
/// `DeclIdentity`'s embedded `(canonical_id, whole_hash)`); those
/// canonicals are the entry's `self_root_canonicals`. Every `peek`
/// validates them **strictly** via
/// [`crate::fact_signature_helpers::ReadSetSignature::validate_with_self_roots`]
/// BEFORE returning — a content edit to the root file or any visited
/// declaration's file rejects the entry.
#[derive(Clone)]
pub struct RefCycleEntry {
    /// `true` when the BFS root reaches a transitive cycle through a
    /// complex helper surface.
    pub result: bool,
    /// Carrier holding the R28 path-precise fact signature — the sole
    /// cache-validity rail. `read_set_signature.validate_with_self_roots(ctx,
    /// &self_root_canonicals)` is the warm-read gate; `canonical_ids()`
    /// drives reverse-index registration.
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
    /// The cold BFS's dispatch-return signature — the BFS's traced
    /// `local_fence` as a `DepSignature`. NOT a cache-validity rail
    /// (the carrier above is the sole validity oracle); it is the
    /// transitive-dependency payload a warm `peek` returns on
    /// `CacheRead.dep_signature` so the component-meta dispatch
    /// accumulator folds the warm sub-query's deps into the owner's
    /// `fact_versions`. Crate-private: the dispatch-return signature is
    /// an internal sibling rail, not a public cache-carrier field.
    pub(crate) dispatch_dep_signature: DepSignature,
    /// Canonicals validated **strictly** as self-roots on every warm
    /// read: the BFS root file plus every visited declaration's file.
    /// An untracked or hash-mismatched self-root rejects the entry.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Retention-ledger admission identity — the unique sequence number
    /// this entry is recorded under in the `GlobalRetentionBudget`
    /// ledger. Allocated once at entry construction; `register_post_publish`
    /// records the entry under it, and every removal path forgets exactly
    /// this ledger record via `forget_seq`. Scoping the ledger removal to
    /// this seq (rather than the cache key) means a concurrently-admitted
    /// fresh entry that republished the same key keeps its own ledger
    /// record — symmetric with the `Arc::ptr_eq` guard on the
    /// reverse-index removal.
    pub admission_seq: u64,
    /// Project generation this entry was computed under, captured by the
    /// cold BFS `compute` closure before it dispatched any work. The
    /// carrier (`read_set_signature`) validates only file-content
    /// whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias /
    /// SDK / workspace-folder change) bumps no file content, so without
    /// this field a stale-by-project-generation entry would validate
    /// forever. Every read-side gate (`peek`, the cooperative `validate`
    /// closure) AND the cooperative post-compute revalidation reject the
    /// entry when `validated_at_generation` differs from the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    /// The revalidation runs under the `RefCycleResultDb`'s
    /// `retention_gate` read guard, so it is atomic against the
    /// `invalidate_all` clear+bump — a stale entry can neither survive a
    /// reset nor be published into a freshly-cleared cache.
    pub validated_at_generation: u64,
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
/// [`cooperative_admit_with_post_publish`], whose `compute`
/// closure runs synchronously on the caller's thread (see
/// `cooperative_admission.rs` synchronous-compute contract).
/// Borrow-capture of `&dyn ResolverContext` in the BFS compute closure
/// is safe because no thread-hop occurs. An overflowed / unrootable
/// signature returns the computed bool through
/// [`ComputeAdmission::ReturnOnly`] — the value reaches every joiner
/// without admitting the entry, and no second uncached BFS runs.
pub struct RefCycleResultDb {
    entries: DashMap<DeclIdentity, Arc<RefCycleEntry>>,
    inflight: InflightTable<DeclIdentity>,
    /// Per-canonical reverse index — maps each canonical id to the set
    /// of cache keys whose dep_signature references it.
    canonical_to_keys: CanonicalToKeysIndex<DeclIdentity>,
    /// Global insertion-ordered total-size budget. The `DeclIdentity`
    /// key embeds the file whole-hash, so each distinct content version
    /// produces a fresh entry; the reverse-index drain reclaims only on
    /// per-canonical invalidation, which an owner-content edit no longer
    /// triggers. This budget is the routine reclamation path —
    /// `post_publish` records each admission and FIFO-evicts the oldest
    /// entries past [`Self::MAX_ENTRIES`].
    retention_budget: crate::bounded_query_retention::GlobalRetentionBudget<DeclIdentity>,
    live_counter: Arc<AtomicU64>,
    /// Lifecycle gate keeping `entries`, `canonical_to_keys`,
    /// `retention_budget`, and `live_counter` in one lock domain. Every
    /// mutation that touches the map and the budget — the cooperative
    /// publish (`map.insert` + `post_publish`), the cooperative removal
    /// cleanup, a budget-FIFO eviction, a stale-`peek` removal, and a
    /// per-canonical drain — runs under a shared read guard;
    /// `invalidate_all` takes the exclusive write guard across the
    /// whole map+index+budget+counter clear. A `clear` therefore cannot
    /// interleave its clears with a concurrent publish, so the budget
    /// never strands a live `entries` item with no admission record.
    retention_gate: parking_lot::RwLock<()>,
    /// Test-only injection point inside [`Self::invalidate_all`],
    /// parked between the `entries` clear and the budget clear with the
    /// `retention_gate` write guard still held. Per-instance; absent
    /// from release builds.
    #[cfg(any(test, debug_assertions))]
    invalidate_all_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

impl RefCycleResultDb {
    /// Total entry-count cap. A long-lived session that re-runs the
    /// transitive-cycle BFS for many owner versions caps here; the
    /// oldest entries are FIFO-evicted on the write-side `post_publish`
    /// hook.
    pub const MAX_ENTRIES: usize = 2048;

    /// Construct with a fresh, unshared `live_counter`. Tests-only.
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    /// Construct with a shared `live_counter` borrowed from
    /// `ProjectTypeStoreCounters::component_meta_cache_live`.
    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::with_counter_and_budget(live_counter, Self::MAX_ENTRIES)
    }

    fn with_counter_and_budget(live_counter: Arc<AtomicU64>, budget_cap: usize) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            retention_budget: crate::bounded_query_retention::GlobalRetentionBudget::new(
                budget_cap,
            ),
            live_counter,
            retention_gate: parking_lot::RwLock::new(()),
            #[cfg(any(test, debug_assertions))]
            invalidate_all_midpoint_gate: parking_lot::Mutex::new(None),
        }
    }

    /// Test-only constructor pinning a small `retention_budget` cap so a
    /// reverse-index test drives FIFO eviction without admitting
    /// [`Self::MAX_ENTRIES`] (2048) entries.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_budget_for_test(budget_cap: usize) -> Self {
        Self::with_counter_and_budget(Arc::new(AtomicU64::new(0)), budget_cap)
    }

    /// Test-only — number of distinct outer shards currently resident
    /// in the `canonical_to_keys` reverse index. A budget eviction that
    /// empties a shard's inner map must drop the outer shard, so this
    /// count tracks the surviving canonicals — not the lifetime
    /// canonical count.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn canonical_to_keys_shard_count_for_test(&self) -> usize {
        self.canonical_to_keys.len()
    }

    /// Configured total entry-count cap.
    #[must_use]
    pub fn retention_cap(&self) -> usize {
        self.retention_budget.cap()
    }

    /// Test-only accessor for the lifecycle `retention_gate`. A race
    /// test parks `invalidate_all` mid-flight and uses `try_read()` on
    /// this gate to assert the in-flight clear engages the write guard
    /// against concurrent publishes.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_retention_gate(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
    }

    /// Test-only driver: arm the [`Self::invalidate_all`] injection
    /// point with `barrier`. The next `invalidate_all` on this Db calls
    /// `barrier.wait()` twice between the `entries` clear and the
    /// budget clear (with the `retention_gate` write guard held).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_invalidate_all_midpoint_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> RefCycleInvalidateGateGuard<'_> {
        *self.invalidate_all_midpoint_gate.lock() = Some(barrier);
        RefCycleInvalidateGateGuard {
            gate: &self.invalidate_all_midpoint_gate,
        }
    }

    /// Test-only — number of admission records currently in the
    /// retention ledger. The budget evicts the oldest once this exceeds
    /// [`Self::MAX_ENTRIES`], so it is the count that bounds the cache.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn retention_tracked_len(&self) -> usize {
        self.retention_budget.tracked_len()
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

    /// Internal — the lifecycle `retention_gate`, passed as the
    /// `publish_fence` to `cooperative_admit_with_post_publish` so the
    /// substrate holds it across `entries.insert` + `post_publish`,
    /// keeping the cooperative publish in the same lock domain as
    /// `invalidate_all`'s map+budget clear.
    pub(crate) fn publish_fence(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
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
    /// every canonical the entry's carrier fact rail references.
    /// Per-canonical mutex acquisition pattern matches
    /// `MaterializeStructureDb`. Bounded by
    /// `read_set_signature.canonical_ids().len() ≤ ~80` (BFS hop cap
    /// + transitive fact set).
    ///
    /// Also records the admission against the global retention budget
    /// and FIFO-evicts the oldest entries once the total exceeds
    /// [`Self::MAX_ENTRIES`]. Evicting a still-valid entry only causes
    /// a later recompute; it never produces an incorrect result.
    ///
    /// `admission_seq` is the published entry's
    /// [`RefCycleEntry::admission_seq`] — the ledger records this entry
    /// under that exact identity, so a later removal path can forget
    /// precisely this ledger record (`forget_seq`) without dropping a
    /// concurrently-admitted fresh entry that republished the same key.
    pub(crate) fn register_post_publish(
        &self,
        key: DeclIdentity,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        admission_seq: u64,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let registered_facts = Arc::clone(&read_set_signature.facts);
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
            map.insert(key.clone(), Arc::clone(&registered_facts));
        }
        // Global retention budget: record this admission under the
        // entry's own seq, FIFO-evict the oldest entries past the total
        // cap. Each victim carries its admission `seq`, so
        // `evict_budget_victim` removes precisely that admission's entry
        // even if a concurrent same-key re-publish has overwritten the
        // map slot under `victim_key` with a fresh, distinctly-seq'd
        // entry.
        let victims = self.retention_budget.record_admission(admission_seq, key);
        for (victim_seq, victim_key) in victims {
            self.evict_budget_victim(victim_seq, &victim_key);
        }
    }

    /// Remove one entry chosen by the retention budget for FIFO
    /// eviction. Drains the entry's reverse-index registrations under
    /// every canonical its carrier referenced and decrements the live
    /// counter, matching the removal-side cleanup the cooperative
    /// substrate runs for a warm-hit reject.
    ///
    /// **Identity-scoped removal.** The map entry under `victim_key` is
    /// removed ONLY when its stored `admission_seq` still equals
    /// `victim_seq`. A same-key re-publish racing this eviction
    /// overwrites the `victim_key` slot with a fresh entry carrying a
    /// distinct seq; a bare-key removal would evict that fresh entry and
    /// strand its live ledger record (cache grows past the cap). The
    /// `remove_if` predicate runs under the `DashMap` shard write lock,
    /// so the seq check and the removal are atomic against a concurrent
    /// re-publish.
    fn evict_budget_victim(&self, victim_seq: u64, victim_key: &DeclIdentity) {
        let Some((_, entry)) = self
            .entries
            .remove_if(victim_key, |_, e| e.admission_seq == victim_seq)
        else {
            return;
        };
        self.live_counter.fetch_sub(1, Ordering::Relaxed);
        // Drop each reverse-index registration; the helper also drops
        // the outer shard when the removal empties its inner map, so a
        // budget-driven eviction wave does not leave empty shards
        // resident.
        for canonical in entry.read_set_signature.canonical_ids() {
            prune_canonical_to_keys_registration(
                &self.canonical_to_keys,
                &canonical,
                victim_key,
                &entry.read_set_signature.facts,
            );
        }
    }

    /// Complete removal-side cleanup, the counterpart of
    /// [`Self::register_post_publish`]. Two callers reach it: the
    /// cooperative-admission substrate when it removes one
    /// already-published entry (a warm-hit reject or a joiner-fork
    /// reject), and [`Self::peek`]'s stale/generation-mismatch reap. The
    /// entry's reverse-index registration must be dropped under EVERY
    /// canonical it referenced, symmetric with the per-canonical
    /// `register_post_publish` insert. `Arc::ptr_eq` against the entry's
    /// `legacy` rail discriminates "our registration" from a concurrent
    /// fresh winner's so a re-published entry's registration is not
    /// stolen.
    ///
    /// `admission_seq` is the removed entry's
    /// [`RefCycleEntry::admission_seq`]. The retention-ledger removal is
    /// scoped to that exact seq via `forget_seq` — a key-only `forget`
    /// would also drop a concurrently-admitted fresh entry's ledger
    /// record (a fresh winner can republish the same key in the
    /// `remove_if` → cleanup window), letting the fresh entry escape the
    /// budget count and the cache grow past `MAX_ENTRIES`. Identity
    /// scoping here mirrors the `Arc::ptr_eq` guard on the reverse-index
    /// removal above.
    pub(crate) fn unregister_post_publish(
        &self,
        key: &DeclIdentity,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        admission_seq: u64,
    ) {
        for canonical in read_set_signature.canonical_ids() {
            prune_canonical_to_keys_registration(
                &self.canonical_to_keys,
                &canonical,
                key,
                &read_set_signature.facts,
            );
        }
        // Keep the retention ledger consistent: drop exactly this
        // entry's ledger record, leaving a fresh re-admission of the
        // same key intact.
        self.retention_budget.forget_seq(admission_seq);
    }

    /// Strict-validation peek.
    ///
    /// Every read validates the entry's carrier against the live store
    /// view BEFORE returning — there is no carrier-bypassing fast
    /// return. The entry's `self_root_canonicals` (the BFS root file
    /// plus every visited declaration's file) validate **strictly**: a
    /// same-canonical content edit, or a self-root canonical the live
    /// store view no longer tracks, rejects the entry; every other fact
    /// keeps the lazy cross-file permissiveness. The entry's
    /// `validated_at_generation` must additionally still equal the live
    /// project generation — a `ProjectGeneration` reset bumps no file
    /// content, so the carrier alone cannot detect it. A stale entry
    /// never returns and never bubbles; it is removed (with
    /// `live_counter` decrement per R8-5).
    pub(crate) fn peek(
        &self,
        id: &DeclIdentity,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<bool>> {
        let result = (|| -> Option<crate::semantic_query::CacheRead<bool>> {
            let entry_arc = self.entries.get(id).map(|e| Arc::clone(&*e))?;
            // Carrier-aware validate-before-bubble with strict
            // self-root validation, plus the project-generation gate
            // (the carrier validates only file-content whole-hashes; a
            // `ProjectGeneration` reset bumps no file content). A stale
            // entry never returns and never bubbles.
            if entry_arc.validated_at_generation
                != ctx.project_type_store().current_project_generation()
                || !entry_arc
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry_arc.self_root_canonicals)
            {
                // Stale-entry reap touches `entries`, the
                // `canonical_to_keys` reverse index, and the retention
                // budget — hold the retention gate (shared read) across
                // the whole removal so it does not desync against a
                // concurrent project-generation `clear`.
                let _retention = self.retention_gate.read();
                // R8-5 — decrement live_counter on stale removal so
                // the shared counter tracks live entries, not stale ones.
                let removed = self
                    .entries
                    .remove_if(id, |_, e| Arc::ptr_eq(e, &entry_arc));
                if removed.is_some() {
                    self.live_counter.fetch_sub(1, Ordering::Relaxed);
                    // Route the reap through the SAME cleanup the
                    // cooperative-removal path uses: `unregister_post_publish`
                    // unregisters every `canonical_to_keys` registration
                    // the reaped entry held and prunes the now-empty
                    // shards, then forgets exactly this entry's
                    // retention-ledger record via `forget_seq`. A bare
                    // `forget_seq` here would leave dead reverse-index
                    // shards resident for a multi-canonical entry.
                    self.unregister_post_publish(
                        id,
                        &entry_arc.read_set_signature,
                        entry_arc.admission_seq,
                    );
                }
                return None;
            }
            entry_arc.read_set_signature.bubble(ctx);
            Some(crate::semantic_query::CacheRead {
                value: entry_arc.result,
                dep_signature: Arc::clone(&entry_arc.dispatch_dep_signature),
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

    /// Drop every cache entry whose carrier references `canonical_id`.
    /// Uses the `canonical_to_keys` reverse index to find affected
    /// keys; `Arc::ptr_eq` discriminates "our entry" from concurrent
    /// fresh writes.
    ///
    /// Holds the `retention_gate` read guard across the whole
    /// reverse-index drain + `entries` removal + budget `forget_seq` +
    /// counter decrement, so this per-canonical invalidation does not
    /// desync the map and the budget against a concurrent `clear`.
    ///
    /// Each removed entry's reverse-index registrations and
    /// retention-ledger record are dropped by [`Self::unregister_post_publish`],
    /// the single removal-side cleanup helper. It iterates the entry's
    /// `read_set_signature.canonical_ids()` — the exact fact-rail
    /// canonical set [`Self::register_post_publish`] registered under.
    /// The `canonical_id` shard itself is already drained by the
    /// `canonical_to_keys.remove` above; the helper's
    /// `prune_canonical_to_keys_registration` for that canonical is then
    /// a no-op. The `live_counter` decrement stays here —
    /// `unregister_post_publish` does not touch the counter — so each
    /// removed entry nets exactly one decrement.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let _retention = self.retention_gate.read();
        let drained: Vec<(DeclIdentity, Arc<[FactVersionRef]>)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.read_set_signature.facts, &registered)
            });
            if let Some((_, removed_entry)) = removed {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Route reverse-index + retention-ledger cleanup through
                // the shared removal helper. It prunes the registration
                // under every canonical the carrier referenced
                // (`canonical_ids()` — legacy ∪ facts) and forgets
                // exactly this entry's ledger record (`forget_seq`),
                // identical to the cooperative-removal and stale-`peek`
                // reap paths — no second canonical-set derivation.
                self.unregister_post_publish(
                    key,
                    &removed_entry.read_set_signature,
                    removed_entry.admission_seq,
                );
            }
        }
    }

    /// Drop every cache entry. Used on project-generation bumps.
    ///
    /// Holds the `retention_gate` WRITE guard across the whole
    /// `entries` + `canonical_to_keys` + `retention_budget` +
    /// `live_counter` clear, so a concurrent cooperative publish,
    /// removal cleanup, stale-`peek` reap, or per-canonical drain
    /// (each holding the read guard) blocks until this clear completes
    /// — no publish can land a live `entries` item whose budget
    /// admission this reset then erases.
    ///
    /// Saturating-subtract pattern (NOT `store(0)`)
    /// because `live_counter` is shared via `Arc<AtomicU64>` across all
    /// typed DBs in `ProjectTypeStore`. A per-DB `store(0)` would
    /// corrupt sibling DBs' contributions to the shared sum.
    pub fn invalidate_all(&self) {
        let _retention = self.retention_gate.write();
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        // Test-only injection point — parked between the `entries`
        // clear and the budget clear with the `retention_gate` write
        // guard still held. `None` (production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.invalidate_all_midpoint_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.retention_budget.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }
}

/// RAII guard returned by
/// [`RefCycleResultDb::test_arm_invalidate_all_midpoint_gate`]. Disarms
/// the per-instance `invalidate_all` injection point on drop.
#[cfg(test)]
#[doc(hidden)]
pub(crate) struct RefCycleInvalidateGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for RefCycleInvalidateGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
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
/// Delegates to [`RefCycleResultDb::peek`], a strict-validation read:
/// every hit validates the entry's carrier against the live store view
/// before returning — there is no generation-equal fast return that
/// bypasses validation. Returns `Some(read)` only when a cached entry
/// exists AND its strict self-root validation passes; returns `None` on
/// a true cache miss or a stale entry (a stale entry is removed and the
/// caller falls through to BFS compute).
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

#[cfg(test)]
thread_local! {
    /// When set, `ref_cycle_db_get_or_compute`'s `compute` closure
    /// takes a `ComputeAdmission::ReturnOnly` exit AFTER the real BFS
    /// has populated `compute_fence` — deterministically reproducing
    /// the production refusal contract (tracer overflow, an
    /// unrootable / torn self-root, or a `RouteGeneration` fence
    /// dependency) without manufacturing a fake fence. The BFS runs
    /// for real (real files read, real `compute_fence`); only the
    /// admission decision is forced. The `ReturnOnly` `CacheRead` must
    /// still carry the BFS fence so the caller's `local_fence` is
    /// extended exactly as the `None`-arm fallback would extend it.
    static FORCE_REF_CYCLE_RETURN_ONLY: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// RAII guard that forces `ref_cycle_db_get_or_compute`'s `compute`
/// closure down a `ComputeAdmission::ReturnOnly` exit for the current
/// thread until dropped.
#[cfg(test)]
pub(crate) struct ForceRefCycleReturnOnlyGuard;

#[cfg(test)]
impl Drop for ForceRefCycleReturnOnlyGuard {
    fn drop(&mut self) {
        FORCE_REF_CYCLE_RETURN_ONLY.with(|f| f.set(false));
    }
}

/// Force `ref_cycle_db_get_or_compute`'s cold `compute` closure to
/// refuse cache admission and return its computed bool via
/// `ComputeAdmission::ReturnOnly` for the current thread until the
/// returned guard drops. The BFS still runs in full.
#[cfg(test)]
pub(crate) fn force_ref_cycle_return_only_for_tests() -> ForceRefCycleReturnOnlyGuard {
    FORCE_REF_CYCLE_RETURN_ONLY.with(|f| f.set(true));
    ForceRefCycleReturnOnlyGuard
}

/// The cooperative-admission wrapper invoked by
/// `meta_resolve::ref_root_reaches_transitive_cycle_node` on the cold
/// path. The `compute` closure runs synchronously on the caller's
/// thread (per cooperative_admission's synchronous-compute contract),
/// so capturing `&dyn ResolverContext` and `&DeclIdentity` directly is safe.
///
/// On cooperative-admission success: bumps `live_counter`, registers
/// the reverse-index, and returns `Some(CacheRead)`. The cold BFS runs
/// once; an overflowed / unrootable signature returns the computed
/// bool through [`ComputeAdmission::ReturnOnly`] without admitting the
/// entry and **without a second uncached BFS**. `None` is returned
/// only when cooperative admission's post-compute revalidation rejects
/// the freshly-built entry.
///
/// `compute_bfs` receives two out-parameters: the legacy dep-fence
/// (`DepVersion` entries) AND `observed_self_roots` — one
/// `(canonical, observed_whole_hash)` per visited declaration identity
/// (the BFS root plus every visited `DeclIdentity`). Those become the
/// entry's strict self-roots.
pub(crate) fn ref_cycle_db_get_or_compute<C>(
    db: &RefCycleResultDb,
    id: &DeclIdentity,
    ctx: &dyn ResolverContext,
    compute_bfs: C,
) -> Option<crate::semantic_query::CacheRead<bool>>
where
    C: FnOnce(
        &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
        &mut Vec<(Arc<str>, crate::types::Hash16)>,
    ) -> bool,
{
    let key_for_register = id.clone();
    // `&RefCycleResultDb` is `Copy`; a dedicated binding lets the
    // removal-side closure capture the db alongside the `post_publish`
    // closure that captures the original `db`.
    let db_for_removal = db;
    // Wrap the BFS cold-compute with `install_fact_tracer`. On `Ok`,
    // merge the traced observation set on top of the visited-identity
    // self-roots. On `Overflow`, return the computed bool via
    // `ReturnOnly` (no second uncached BFS).
    let host = ctx.host_for_fact_tracer_install();
    let provenance = Arc::clone(&host.provenance);
    cooperative_admit_with_post_publish(
        db.entries(),
        db.inflight(),
        id.clone(),
        // Validate(&Entry) -> Option<V> — strict self-root validation
        // plus the project-generation gate. The carrier validates only
        // file-content whole-hashes; a `ProjectGeneration` reset
        // (tsconfig / SDK / workspace-folder change) bumps no file
        // content, so an entry whose `validated_at_generation` no longer
        // matches the live generation is stale even though its carrier
        // still validates.
        |entry: &RefCycleEntry| {
            if entry.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && entry
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry.self_root_canonicals)
            {
                entry.read_set_signature.bubble(ctx);
                Some(crate::semantic_query::CacheRead {
                    value: entry.result,
                    dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                })
            } else {
                None
            }
        },
        // Compute() -> ComputeAdmission<V, Entry>
        || -> ComputeAdmission<crate::semantic_query::CacheRead<bool>, RefCycleEntry> {
            // Snapshot the project generation BEFORE the BFS dispatches
            // any work. A `ProjectGeneration` reset that lands during
            // the cold BFS window bumps this; the post-compute
            // revalidation (run under the `publish_fence` read guard)
            // then rejects the entry, and a stale entry can neither
            // survive a reset nor publish into a freshly-cleared cache.
            let validated_at_generation = ctx.project_type_store().current_project_generation();
            let mut compute_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
            let mut observed_self_roots: Vec<(Arc<str>, crate::types::Hash16)> = Vec::new();
            let (result, finalise) =
                crate::fact_signature_helpers::install_fact_tracer(host, || {
                    compute_bfs(&mut compute_fence, &mut observed_self_roots)
                });
            provenance
                .ref_cycle_fact_tracer_installs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // The dispatch-return fence is the set of files the BFS
            // actually read. It is built up front — before the
            // `RouteGeneration` refusal scan — so every `ReturnOnly`
            // exit can carry it. A `ReturnOnly` `CacheRead` MUST report
            // this fence: the caller (`ref_root_reaches_transitive_cycle_node`)
            // merges `read.dep_signature` into its own `local_fence`, so
            // a `ReturnOnly` that dropped the fence would let an outer
            // computation be cached without the files the BFS read.
            // Carrying it makes the `ReturnOnly` path observably
            // equivalent to the `None`-arm uncached-fallback (which
            // extends the caller's fence via `local_fence.extend(fence)`)
            // without running a second uncached BFS. It is the entry's
            // dispatch-return signature, NOT a cache-validity rail — the
            // fact carrier built by `ref_cycle_read_set` is the validity
            // oracle.
            let dispatch_dep_signature: DepSignature = Arc::from(compute_fence.into_boxed_slice());
            // `cache_suppress` stays `false`: the caller does not
            // consume it. A `ReturnOnly` outcome is non-shareable across
            // cooperative joiners — the winner alone receives this
            // `CacheRead`, and a joiner that coalesced onto a
            // `ReturnOnly` winner forks and cold-recomputes its own BFS
            // (so it builds its own view-accurate fence). The fence
            // therefore reaches the winner's caller through
            // `dep_signature`, never through `cache_suppress`.
            let return_only_value =
                |dep_signature: DepSignature| crate::semantic_query::CacheRead {
                    value: result,
                    dep_signature,
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                };
            // Refuse shared admission when the BFS fence carries a
            // `RouteGeneration` dependency — route generation has no
            // authoritative validating source, so an entry rooted on
            // it could not detect a content edit to the route-observed
            // file. The computed bool is still returned via `ReturnOnly`,
            // carrying the BFS fence.
            if dispatch_dep_signature
                .iter()
                .any(|(_, v)| matches!(v, crate::semantic_query::DepVersion::RouteGeneration(_)))
            {
                provenance
                    .ref_cycle_overflow_refusals
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return ComputeAdmission::ReturnOnly(return_only_value(Arc::clone(
                    &dispatch_dep_signature,
                )));
            }
            // Test-only injection: deterministically reproduce the
            // production refusal contract (tracer overflow / unrootable
            // self-root / `RouteGeneration` fence dependency) AFTER the
            // real BFS has populated `compute_fence`. The `ReturnOnly`
            // `CacheRead` carries the real BFS fence — exactly as every
            // production refusal site does.
            #[cfg(test)]
            if FORCE_REF_CYCLE_RETURN_ONLY.with(|f| f.get()) {
                provenance
                    .ref_cycle_overflow_refusals
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return ComputeAdmission::ReturnOnly(return_only_value(Arc::clone(
                    &dispatch_dep_signature,
                )));
            }
            match finalise {
                crate::resolver_core::FactReadSetFinalise::Ok(traced) => {
                    // Build the observed-root carrier: visited-identity
                    // self-roots prepended, traced facts merged on top.
                    match ref_cycle_read_set(&observed_self_roots, &traced) {
                        Some((facts, self_root_canonicals)) => {
                            ComputeAdmission::Cacheable(RefCycleEntry {
                                result,
                                read_set_signature:
                                    crate::fact_signature_helpers::ReadSetSignature::new(facts),
                                dispatch_dep_signature,
                                self_root_canonicals,
                                admission_seq: crate::bounded_query_retention::next_retention_seq(),
                                validated_at_generation,
                            })
                        }
                        None => {
                            // A torn observation among the visited
                            // self-roots — the value is valid but the
                            // signature cannot be built strictly. The
                            // computed bool is returned via `ReturnOnly`,
                            // carrying the BFS fence.
                            provenance
                                .ref_cycle_overflow_refusals
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            ComputeAdmission::ReturnOnly(return_only_value(dispatch_dep_signature))
                        }
                    }
                }
                crate::resolver_core::FactReadSetFinalise::Overflow => {
                    provenance
                        .ref_cycle_overflow_refusals
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Tracer overflowed — return the computed bool via
                    // `ReturnOnly`, carrying the BFS fence; no second
                    // uncached BFS.
                    ComputeAdmission::ReturnOnly(return_only_value(dispatch_dep_signature))
                }
            }
        },
        // Project(&Entry) -> V — bubble path-precise observation set
        // so outer cold-computes see the BFS's transitive facts.
        |entry: &RefCycleEntry| {
            entry.read_set_signature.bubble(ctx);
            crate::semantic_query::CacheRead {
                value: entry.result,
                dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            }
        },
        // revalidate_after_compute(&Entry) -> bool — strict self-root
        // validation plus the project-generation gate. Runs under the
        // `publish_fence` read guard (the substrate holds it across
        // revalidate→insert→post_publish), so the generation check and
        // the `entries.insert` are atomic against a concurrent
        // `invalidate_all` clear+bump: a BFS entry computed under a
        // superseded project generation is rejected here rather than
        // published into the freshly-cleared cache.
        |entry: &RefCycleEntry| {
            entry.validated_at_generation == ctx.project_type_store().current_project_generation()
                && entry
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry.self_root_canonicals)
        },
        // removal_cleanup(&K, &Arc<Entry>) — removal-side counterpart
        // of `post_publish`. When the substrate removes an
        // already-published entry (warm-hit reject or joiner-fork
        // reject) the live counter must decrement and the
        // per-canonical reverse-index registration must drop,
        // symmetric with the `post_publish` bump + register.
        move |removed_key: &DeclIdentity, removed_entry: &Arc<RefCycleEntry>| {
            db_for_removal.decrement_live_counter();
            db_for_removal.unregister_post_publish(
                removed_key,
                &removed_entry.read_set_signature,
                removed_entry.admission_seq,
            );
        },
        // post_publish(&Arc<Entry>, &K)
        move |entry_arc: &Arc<RefCycleEntry>, _k: &DeclIdentity| {
            db.bump_live_counter();
            db.register_post_publish(
                key_for_register.clone(),
                &entry_arc.read_set_signature,
                entry_arc.admission_seq,
            );
        },
        // publish_fence — the Db's `retention_gate`. The substrate
        // holds it (shared read) across `entries.insert` + `post_publish`
        // so the map insert and the reverse-index + budget admission are
        // one lock-domain mutation, exclusive against `invalidate_all`'s
        // map+budget clear.
        Some(db.publish_fence()),
    )
}

/// Build the observed-root fact signature + self-root canonical set
/// for a [`RefCycleEntry`] — **provenance-pure**.
///
/// The BFS records one `(canonical, observed_whole_hash)` per visited
/// declaration identity (the root plus every visited `DeclIdentity`,
/// each carrying an embedded observed `whole_hash`). Those are the
/// entry's structural self-roots. The signature leads with one
/// self-root `FileWholeHash` per distinct visited canonical, then
/// merges the `install_fact_tracer` scope's traced observation set ON
/// TOP (the `semantic_graph_read_set_signature` discipline): a traced
/// `FileWholeHash` for a self-root canonical folds onto the observed
/// self-root (it MUST agree — a mismatch is a torn read), every other
/// traced fact is kept verbatim.
///
/// Returns `None` — the caller routes the bool through `ReturnOnly` —
/// when two visited identities name the same canonical with
/// conflicting observed hashes, or a traced `FileWholeHash` disagrees
/// with an observed self-root hash.
fn ref_cycle_read_set(
    observed_self_roots: &[(Arc<str>, crate::types::Hash16)],
    traced_facts: &[crate::resolver_core::FactVersionRef],
) -> Option<crate::fact_signature_helpers::StructuralCarrierReadSet> {
    use crate::resolver_core::FactVersionRef;

    // Collapse the visited self-roots into a per-canonical hash map;
    // a conflicting hash for the same canonical is a torn observation.
    let mut self_root_hashes: rustc_hash::FxHashMap<Arc<str>, crate::types::Hash16> =
        rustc_hash::FxHashMap::default();
    for (canonical, observed_hash) in observed_self_roots {
        match self_root_hashes.get(canonical) {
            Some(existing) if existing != observed_hash => return None,
            _ => {
                self_root_hashes.insert(Arc::clone(canonical), *observed_hash);
            }
        }
    }

    let mut facts: Vec<FactVersionRef> =
        Vec::with_capacity(self_root_hashes.len() + traced_facts.len());
    // Lead with one self-root `FileWholeHash` per observed self-root.
    for (canonical, observed_hash) in &self_root_hashes {
        facts.push(FactVersionRef::FileWholeHash {
            canonical_id: canonical.as_ref().to_string(),
            hash: *observed_hash,
        });
    }
    // Merge the traced fact set on top — a traced `FileWholeHash` for a
    // self-root canonical folds onto the observed self-root.
    for fact in traced_facts {
        if let FactVersionRef::FileWholeHash { canonical_id, hash } = fact {
            if let Some(observed_hash) = self_root_hashes.get(canonical_id.as_str()) {
                if hash != observed_hash {
                    return None;
                }
                continue;
            }
        }
        facts.push(fact.clone());
    }

    let mut self_root_canonicals: Vec<Arc<str>> = self_root_hashes.into_keys().collect();
    self_root_canonicals.sort();
    Some((Arc::from(facts), Arc::from(self_root_canonicals)))
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
