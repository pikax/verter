//! Owner import surface cache.
//!
//! [`OwnerImportSurfaceDb`] is the reusable direct-owner-imports artifact:
//! one [`OwnerImportSurface`] per `(owner_canonical, owner_whole_hash)`
//! pair. Given an owner file, it resolves every direct `import` binding to
//! its originating canonical + exported-name once per owner version and
//! serves every consumer — dependency collection, registry hydration, the
//! solver host, fallthrough / meta projection — from the same entry.
//!
//! Today the pipeline asks `resolve_imported_type_root` from each
//! stage independently. wires the stages onto this cache so direct
//! imports resolve exactly once per owner version.
//!
//! ## Contract
//!
//! - Key: `(owner_canonical, owner_whole_hash)`.
//! - Value: `Arc<OwnerImportSurface>` — immutable map from local import
//!   name → resolved root identity.
//! - **No request view in the key.** The cache is project-global.
//! - Stale owner versions are rejected at lookup time; callers materialize
//!   through the shared route layer to repopulate.
//! - Warm hits return the surface + an exact dep-signature fragment the
//!   caller folds into its dependency-fact set for the publish-side
//!   completion-fence revalidation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use verter_semantic::analysis::Hash16;

use crate::semantic_query::{DepSignature, DepVersion};

/// A single resolved direct import from the owner file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOwnerImport {
    /// Canonical file id the import resolves to.
    pub canonical_id: Arc<str>,
    /// Name exported from the resolved canonical (may differ from the local
    /// binding name after `import { X as Y } from '...'`).
    pub exported_name: Arc<str>,
    /// Whole-hash of the resolved canonical at materialization time. Used
    /// by the dep-signature fragment.
    pub target_whole_hash: Option<Hash16>,
}

/// Immutable import surface for one owner/version pair.
#[derive(Debug, Clone)]
pub struct OwnerImportSurface {
    /// Owner canonical id.
    pub owner_canonical: Arc<str>,
    /// Owner whole-hash (content identity).
    pub owner_whole_hash: Hash16,
    /// Local binding name → resolved root identity.
    pub bindings: Arc<FxHashMap<Arc<str>, ResolvedOwnerImport>>,
    /// Path-precise fact-tracer carrier — the primary cache-validity
    /// oracle (R28). Carries the materialisation's
    /// `facts: Arc<[FactVersionRef]>` observed under the active
    /// fact tracer (parse / resolve-imports / route-surface facts on
    /// every transitively read canonical, including the owner's own
    /// `FileWholeHash`). Warm reads validate this carrier against the
    /// live `StoreView` BEFORE bubbling.
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
    /// Project generation this surface was built under, snapshotted by
    /// the producer before its materialisation walk dispatched any
    /// work. The `read_set_signature` carrier validates only
    /// file-content whole-hashes; a `ProjectGeneration` reset (tsconfig /
    /// path-alias / SDK / workspace-folder change) bumps no file
    /// content, so without this stamp a stale-by-project-generation
    /// surface racing a `bump_project_generation_and_evict` cold-publish
    /// window would validate forever on file-content terms. Every
    /// read-side gate ([`OwnerImportSurfaceDb::get_with_view`]) rejects
    /// the surface when `validated_at_generation` differs from the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    pub validated_at_generation: u64,
}

/// Host-owned cache of owner import surfaces. Keyed by owner canonical;
/// the entry carries the owner whole-hash so stale versions are rejected
/// at lookup time.
pub struct OwnerImportSurfaceDb {
    entries: DashMap<Arc<str>, Arc<OwnerImportSurface>>,
    live_counter: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract. Production paths always use
    /// [`crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION`]; test fixtures
    /// may construct a Db with an explicit older version to exercise the
    /// stale-entry eviction invariant.
    schema_version: u32,
}

impl OwnerImportSurfaceDb {
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    /// Construct with an externally-shared `live_counter` so the
    /// [`ProjectTypeStoreCounters`](crate::project_type_store::ProjectTypeStoreCounters)
    /// snapshot reflects live-entry changes without a second read.
    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::with_counter_and_schema_version(
            live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db,
    /// for cache_invariant_migration fixtures that need to plant stale
    /// entries under a prior cluster version.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_and_schema_version(Arc::new(AtomicU64::new(0)), schema_version)
    }

    fn with_counter_and_schema_version(live_counter: Arc<AtomicU64>, schema_version: u32) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter,
            schema_version,
        }
    }

    /// Look up the owner surface for `owner_canonical` if the cached entry
    /// matches `expected_owner_whole_hash`. Stale entries are rejected at
    /// the key level so no callers observe a mixed-version surface. This
    /// permissive variant does NOT validate `fact_dep_signature` against
    /// any view; production callers prefer [`Self::get_with_view`] which
    /// gates the entry on its observed chain facts (R3/R26/R28).
    ///
    /// Lookups against a Db whose `schema_version` does not match the current
    /// [`crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION`] return `None`
    /// without consulting the entry map. Use [`Self::evict_if_schema_mismatch`]
    /// to drain the storage; it is exposed so test fixtures can verify the
    /// stale-eviction invariant deterministically.
    #[must_use]
    fn lookup_owner_hash_candidate(
        &self,
        owner_canonical: &str,
        expected_owner_whole_hash: Hash16,
    ) -> Option<Arc<OwnerImportSurface>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = match self.entries.get(owner_canonical) {
            Some(entry) if entry.owner_whole_hash == expected_owner_whole_hash => {
                Some(entry.clone())
            }
            _ => None,
        };
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .owner_import
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .owner_import
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        result
    }

    /// Test-support wrapper for cache-state fixtures that intentionally inspect
    /// the owner/hash slot without fact validation. Production callers use
    /// [`Self::get_with_view`].
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub fn get(
        &self,
        owner_canonical: &str,
        expected_owner_whole_hash: Hash16,
    ) -> Option<Arc<OwnerImportSurface>> {
        self.lookup_owner_hash_candidate(owner_canonical, expected_owner_whole_hash)
    }

    /// Look up the owner surface for `owner_canonical` and validate
    /// every fact recorded in its `fact_dep_signature` against the
    /// caller's `StoreView` AND its `validated_at_generation` against
    /// the live project generation. R3/R26/R28: the producer threads
    /// every barrel/reexport chain participant's fact into the
    /// signature so a chain-internal change (barrel retarget, route
    /// surface edit) invalidates the cached surface on read — no
    /// eager `evict_canonical` required. The project-generation gate
    /// is the project-shape counterpart of the carrier check: a
    /// `ProjectGeneration` reset (tsconfig / path-alias / SDK /
    /// workspace-folder change) bumps no file content, so a surface
    /// whose `validated_at_generation` no longer equals the live
    /// generation is stale even though its carrier still validates
    /// — a `bump_project_generation_and_evict` racing a cold publish
    /// can otherwise strand a stale surface.
    #[must_use]
    pub fn get_with_view<V>(
        &self,
        host: &crate::VerterHost,
        owner_canonical: &str,
        expected_owner_whole_hash: Hash16,
        view: &V,
    ) -> Option<Arc<OwnerImportSurface>>
    where
        V: crate::resolver_core::StoreView + ?Sized,
    {
        let candidate =
            self.lookup_owner_hash_candidate(owner_canonical, expected_owner_whole_hash)?;
        if candidate.validated_at_generation
            != host.project_type_store().current_project_generation()
        {
            return None;
        }
        if candidate
            .read_set_signature
            .facts
            .iter()
            .all(|fact| view.validates(fact))
        {
            return Some(candidate);
        }
        None
    }

    /// Owner-controlled warm-or-cold admission. The DB performs the validated
    /// warm lookup, atomically cleans only the stale content version, invokes
    /// the cold closure, and owns the sole production write. A valid but
    /// non-cacheable result is served from `ReturnOnly` without mutation.
    pub(crate) fn get_or_compute<V, F>(
        &self,
        host: &crate::VerterHost,
        owner_canonical: &str,
        owner_whole_hash: Hash16,
        view: &V,
        compute: F,
    ) -> Option<Arc<OwnerImportSurface>>
    where
        V: crate::resolver_core::StoreView + ?Sized,
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            Arc<OwnerImportSurface>,
            Arc<OwnerImportSurface>,
        >,
    {
        if let Some(cached) = self.get_with_view(host, owner_canonical, owner_whole_hash, view) {
            // R28 fact-bubble-up on the WARM path — mirror of
            // `RouteDb::get_or_resolve_route_observing_facts`'s warm-hit
            // branch. Re-observe the surface's recorded chain deps (owner
            // + leaf `FileWholeHash` facts + route-chain facts — exactly
            // the validated `read_set_signature`, never a broader set)
            // into every active tracer on this thread, so an ENCLOSING
            // traced cold compute folding this warm surface roots the
            // same dependency facts a cold build fans out. Without this,
            // an enclosing entry publishes without the chain deps and a
            // later leaf edit / barrel retarget cannot invalidate it (the
            // typeinfo published-Surface stale-warm hole). No-op when no
            // tracer is installed (R24 warm-hit cost discipline).
            crate::fact_signature_helpers::observe_fact_signature(&cached.read_set_signature.facts);
            return Some(cached);
        }
        self.remove_if_owner_hash_matches(owner_canonical, owner_whole_hash);

        let (decision, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
            let decision = compute();
            let surface = match &decision {
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface) => {
                    Some(surface)
                }
                crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value, ..
                } => Some(value),
                crate::cache_runtime::singleflight::ComputeAdmission::Failed => None,
            };
            // The owner re-observes every producer-supplied direct-chain fact
            // before finalisation. The admitted signature is rebuilt solely from
            // this owner-owned tracer; a caller cannot hand a raw signature to
            // the write.
            if let Some(surface) = surface {
                for fact in surface.read_set_signature.facts.iter() {
                    crate::resolver_core::resolver_context::observe_fan_out(fact.clone());
                }
            }
            decision
        });
        host.provenance
            .owner_import_surface_fact_tracer_installs
            .fetch_add(1, Ordering::Relaxed);

        let rebind = |surface: Arc<OwnerImportSurface>,
                      facts: Arc<[crate::resolver_core::FactVersionRef]>| {
            Arc::new(OwnerImportSurface {
                owner_canonical: Arc::clone(&surface.owner_canonical),
                owner_whole_hash: surface.owner_whole_hash,
                bindings: Arc::clone(&surface.bindings),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                validated_at_generation: surface.validated_at_generation,
            })
        };

        match (decision, finalise) {
            (
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface),
                crate::resolver_core::FactReadSetFinalise::Ok(facts),
            ) => {
                let surface = rebind(surface, facts);
                let generation_current = surface.validated_at_generation
                    == host.project_type_store().current_project_generation();
                let identity_current = surface.owner_canonical.as_ref() == owner_canonical
                    && surface.owner_whole_hash == owner_whole_hash;
                // `view` may be a deliberately fixed request snapshot that
                // predates artifacts loaded during this cold walk. Rechecking
                // the newly minted facts against that old snapshot would reject
                // correct cold results. Generation and key identity fence the
                // publish race here; every warm read performs strict fact
                // validation against its own caller view in `get_with_view`.
                if !generation_current || !identity_current {
                    crate::cache_runtime::admission::propagate_non_admission(
                        crate::cache_runtime::NonAdmissionReason::GenerationSuperseded,
                    );
                    return None;
                }
                self.insert_owned(Arc::clone(&surface.owner_canonical), Arc::clone(&surface));
                Some(surface)
            }
            (
                crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly { value, reason },
                crate::resolver_core::FactReadSetFinalise::Ok(facts),
            ) => {
                crate::cache_runtime::admission::propagate_non_admission(reason);
                Some(rebind(value, facts))
            }
            (
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface)
                | crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value: surface,
                    ..
                },
                crate::resolver_core::FactReadSetFinalise::NonCacheable(facts),
            ) => {
                host.provenance
                    .owner_import_surface_fenced_serve_refusals
                    .fetch_add(1, Ordering::Relaxed);
                crate::cache_runtime::admission::propagate_non_admission(
                    crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
                );
                Some(rebind(surface, facts))
            }
            (
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(surface)
                | crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value: surface,
                    ..
                },
                crate::resolver_core::FactReadSetFinalise::Overflow,
            ) => {
                host.provenance
                    .owner_import_surface_overflow_refusals
                    .fetch_add(1, Ordering::Relaxed);
                crate::cache_runtime::admission::propagate_non_admission(
                    crate::cache_runtime::NonAdmissionReason::SignatureOverflow,
                );
                Some(surface)
            }
            (crate::cache_runtime::singleflight::ComputeAdmission::Failed, _) => None,
        }
    }

    /// Insert or replace the surface for `owner_canonical`. A replacement
    /// does not change the live-entry count.
    fn insert_owned(&self, owner_canonical: Arc<str>, surface: Arc<OwnerImportSurface>) {
        let prev = self.entries.insert(owner_canonical, surface);
        if prev.is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Test-support seed that intentionally bypasses real cold admission.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert(&self, owner_canonical: Arc<str>, surface: Arc<OwnerImportSurface>) {
        self.insert_owned(owner_canonical, surface);
    }

    /// Remove the currently stored owner surface only when it is the exact
    /// content version that just failed validated lookup. The predicate and
    /// removal are one DashMap operation, so a concurrent fresh replacement is
    /// never evicted by stale-miss cleanup.
    pub(crate) fn remove_if_owner_hash_matches(
        &self,
        owner_canonical: &str,
        expected_owner_whole_hash: Hash16,
    ) -> bool {
        let removed = self
            .entries
            .remove_if(owner_canonical, |_, surface| {
                surface.owner_whole_hash == expected_owner_whole_hash
            })
            .is_some();
        if removed {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }

    /// Remove the surface outright — used on file-close / hard evict.
    pub(crate) fn remove(&self, owner_canonical: &str) {
        if self.entries.remove(owner_canonical).is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Drop every cached surface. Called on project-generation bumps
    /// (tsconfig / SDK / workspace-folder changes) — owner surfaces
    /// depend on resolved routes, which project-shape changes may
    /// shift, so nothing is safe to keep warm.
    pub fn entries_drain_for_generation_bump(&self) {
        let count = self.entries.len();
        self.entries.clear();
        self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
    }

    /// Number of live owner surfaces. Primarily for debug counters.
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let canonical: Arc<str> = Arc::from(marker);
        let surface = Arc::new(OwnerImportSurface {
            owner_canonical: Arc::clone(&canonical),
            owner_whole_hash: [0u8; 16],
            bindings: Arc::new(FxHashMap::default()),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
            validated_at_generation: 0,
        });
        self.insert(canonical, surface);
    }
}

impl Default for OwnerImportSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::cache_schema::CacheSchemaVersioned for OwnerImportSurfaceDb {
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

impl crate::invalidation_domain::ParticipatesInInvalidation for OwnerImportSurfaceDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.entries_drain_for_generation_bump();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for OwnerImportSurfaceDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.len();
        self.remove(canonical_id);
        let after = self.len();
        before.saturating_sub(after)
    }
}

/// Build a fresh `OwnerImportSurface` from the owner's resolved-import
/// iterator. Callers provide the owner identity, content hash, the
/// pre-resolved `(local_name, canonical_id, exported_name,
/// target_whole_hash)` tuples, the full chain of route-facts observed
/// during resolution (one entry per intermediate barrel / reexport
/// hop), and the project-generation snapshot the producer captured
/// before its materialisation walk dispatched any work. R3/R26/R28 Gap
/// 1: `chain_facts` MUST include every barrel participant's
/// `DerivedFactHash::Route` so a barrel retarget that leaves the final
/// target unchanged still invalidates the cached surface on read.
/// `validated_at_generation` is the project-shape counterpart of the
/// carrier and is checked on every warm read.
pub fn build_owner_import_surface<I>(
    owner_canonical: Arc<str>,
    owner_whole_hash: Hash16,
    resolved_imports: I,
    chain_facts: Vec<crate::resolver_core::FactVersionRef>,
    validated_at_generation: u64,
) -> Arc<OwnerImportSurface>
where
    I: IntoIterator<Item = (Arc<str>, Arc<str>, Arc<str>, Option<Hash16>)>,
{
    let mut bindings: FxHashMap<Arc<str>, ResolvedOwnerImport> = FxHashMap::default();
    let mut sig_entries: Vec<(Arc<str>, DepVersion)> = Vec::new();
    sig_entries.push((
        owner_canonical.clone(),
        DepVersion::WholeHash(owner_whole_hash),
    ));

    for (local_name, canonical_id, exported_name, target_whole_hash) in resolved_imports {
        let binding = ResolvedOwnerImport {
            canonical_id: canonical_id.clone(),
            exported_name,
            target_whole_hash,
        };
        if let Some(hash) = target_whole_hash {
            sig_entries.push((canonical_id.clone(), DepVersion::WholeHash(hash)));
        }
        bindings.insert(local_name, binding);
    }

    // Stable order so the fact signature is deterministic for
    // downstream validation.
    sig_entries.sort_by(|a, b| a.0.cmp(&b.0));
    sig_entries.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let owner_target_fence: DepSignature = Arc::from(sig_entries.into_boxed_slice());

    // Path-precise fact signature: start from the owner + final-target
    // whole-hash facts (`fact_signature_from_fence` lowers the
    // `owner_target_fence` whole-hash entries into `FileWholeHash`
    // facts) and append every chain fact observed by the producer.
    // De-duplicate so the same `FactVersionRef` does not appear twice
    // when fast-path + route-walk both emit it.
    //
    // `owner_target_fence` is built entirely from `DepVersion::WholeHash`
    // entries (the loop above pushes only `WholeHash`, and the owner
    // seed is a `WholeHash`), so `fact_signature_from_fence` — which
    // refuses ONLY on a `RouteGeneration` entry — always yields `Some`.
    // `expect` (not `unwrap_or_default`) enforces that invariant: a
    // `None` here would mean a `RouteGeneration` entry slipped into the
    // fence, and silently substituting an empty signature would publish
    // an unrooted `OwnerImportSurface` cache entry that warm validation
    // could never invalidate.
    let base_facts =
        crate::component_meta_materialize::fact_signature_from_fence(owner_target_fence.as_ref())
            .expect(
                "OwnerImportSurface owner_target_fence is built exclusively from \
                 DepVersion::WholeHash entries, so fact_signature_from_fence — which \
                 refuses only on RouteGeneration — must yield Some",
            );
    let mut combined: Vec<crate::resolver_core::FactVersionRef> =
        base_facts.iter().cloned().collect();
    let mut seen: rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef> =
        combined.iter().cloned().collect();
    for fact in chain_facts {
        if seen.insert(fact.clone()) {
            combined.push(fact);
        }
    }
    let fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]> = Arc::from(combined);

    Arc::new(OwnerImportSurface {
        owner_canonical,
        owner_whole_hash,
        bindings: Arc::new(bindings),
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
            fact_dep_signature,
        ),
        validated_at_generation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_owned_compute_mints_signature_and_refuses_transitive_hazards() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let db = OwnerImportSurfaceDb::new();
        let view = crate::resolver_core::PermissiveStoreView;
        let owner_hash = [7u8; 16];
        let transitive_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/w/dependency.ts".to_string(),
            hash: [8u8; 16],
        };

        let admitted = db
            .get_or_compute(&host, "/w/owner.ts", owner_hash, &view, || {
                crate::resolver_core::resolver_context::observe_fan_out(transitive_fact.clone());
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    build_owner_import_surface(
                        Arc::from("/w/owner.ts"),
                        owner_hash,
                        Vec::new(),
                        Vec::new(),
                        0,
                    ),
                )
            })
            .expect("owner-controlled cold compute must serve its value");
        assert!(
            admitted.read_set_signature.facts.contains(&transitive_fact),
            "the admitted signature must come from the DB-owned tracer"
        );
        assert!(
            db.get("/w/owner.ts", owner_hash).is_some(),
            "a clean traced computation must admit"
        );

        let refused_hash = [9u8; 16];
        let refused = db
            .get_or_compute(&host, "/w/refused.ts", refused_hash, &view, || {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
                );
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    build_owner_import_surface(
                        Arc::from("/w/refused.ts"),
                        refused_hash,
                        Vec::new(),
                        Vec::new(),
                        0,
                    ),
                )
            })
            .expect("non-admission must still serve the cold caller");
        assert_eq!(refused.owner_whole_hash, refused_hash);
        assert!(
            db.get("/w/refused.ts", refused_hash).is_none(),
            "a transitive hazard must never reach owner-import storage"
        );
    }

    /// R28 fact-bubble-up on the WARM path — the owner-import mirror of
    /// `RouteDb::get_or_resolve_route_observing_facts`'s warm-hit branch.
    ///
    /// A warm `get_or_compute` hit must RE-OBSERVE the surface's recorded
    /// chain deps (owner + leaf `FileWholeHash` facts + route-chain facts)
    /// into every active tracer on the current thread, so an ENCLOSING
    /// traced cold compute that folds the warm surface roots the same
    /// dependency facts a cold build would have fanned out. Without the
    /// re-observation the enclosing entry publishes WITHOUT the chain deps
    /// and a later leaf edit / barrel retarget cannot invalidate it —
    /// the typeinfo published-Surface stale-warm hole.
    ///
    /// Discriminating: against a validate-only `get_with_view` warm path
    /// this test FAILS (the outer tracer finalises without the leaf fact);
    /// with the warm-hit `observe_fact_signature` bubble it PASSES.
    #[test]
    fn warm_hit_reobserves_chain_facts_into_active_tracer() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let db = OwnerImportSurfaceDb::new();
        let view = crate::resolver_core::PermissiveStoreView;
        let owner_hash = [7u8; 16];
        let generation = host.project_type_store().current_project_generation();
        let leaf_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/w/leaf.ts".to_string(),
            hash: [9u8; 16],
        };

        // Cold-admit a surface whose signature carries the leaf chain fact.
        let admitted = db
            .get_or_compute(&host, "/w/owner.ts", owner_hash, &view, || {
                crate::resolver_core::resolver_context::observe_fan_out(leaf_fact.clone());
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    build_owner_import_surface(
                        Arc::from("/w/owner.ts"),
                        owner_hash,
                        Vec::new(),
                        Vec::new(),
                        generation,
                    ),
                )
            })
            .expect("cold admit must serve");
        assert!(
            admitted.read_set_signature.facts.contains(&leaf_fact),
            "precondition: the admitted surface signature must carry the leaf chain fact"
        );

        // Warm hit under an OUTER tracer: the surface's chain facts must
        // fan into it. The compute closure must NOT run (warm hit).
        let ((), finalise) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
            let warm = db.get_or_compute(
                &host,
                "/w/owner.ts",
                owner_hash,
                &view,
                || -> crate::cache_runtime::singleflight::ComputeAdmission<
                    Arc<OwnerImportSurface>,
                    Arc<OwnerImportSurface>,
                > { panic!("second call must warm-hit, not recompute") },
            );
            assert!(warm.is_some(), "second call must serve the warm surface");
        });
        let crate::resolver_core::FactReadSetFinalise::Ok(outer_facts) = finalise else {
            panic!("outer tracer must finalise Ok, got a non-cacheable/overflow finalise");
        };
        assert!(
            outer_facts.contains(&leaf_fact),
            "warm OwnerImportSurfaceDb hit must re-observe the surface's chain deps into \
             the caller's active tracer (mirror RouteDb::get_or_resolve_route_observing_facts); \
             outer tracer finalised {outer_facts:?}"
        );
        // Precision guard: the warm bubble observes EXACTLY the surface's
        // recorded signature — the owner self-root plus the leaf chain
        // fact — never a broader set (over-invalidation).
        assert_eq!(
            outer_facts.len(),
            admitted.read_set_signature.facts.len(),
            "warm bubble must fan exactly the surface's recorded facts, not a broader set"
        );
    }

    fn mk_surface(hash: Hash16) -> Arc<OwnerImportSurface> {
        build_owner_import_surface(
            Arc::from("/w/owner.ts"),
            hash,
            vec![
                (
                    Arc::from("Foo"),
                    Arc::from("/w/foo.ts"),
                    Arc::from("Foo"),
                    Some([1u8; 16]),
                ),
                (
                    Arc::from("Bar"),
                    Arc::from("/w/bar.ts"),
                    Arc::from("default"),
                    None,
                ),
            ],
            Vec::new(),
            0,
        )
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let db = OwnerImportSurfaceDb::new();
        let surface = mk_surface([7u8; 16]);
        db.insert(Arc::from("/w/owner.ts"), surface.clone());
        let hit = db.get("/w/owner.ts", [7u8; 16]).unwrap();
        assert_eq!(hit.owner_whole_hash, [7u8; 16]);
        assert_eq!(hit.bindings.len(), 2);
        assert_eq!(
            hit.bindings
                .get(&Arc::<str>::from("Foo"))
                .unwrap()
                .exported_name
                .as_ref(),
            "Foo"
        );
    }

    #[test]
    fn stale_owner_hash_misses() {
        let db = OwnerImportSurfaceDb::new();
        db.insert(Arc::from("/w/owner.ts"), mk_surface([7u8; 16]));
        assert!(db.get("/w/owner.ts", [8u8; 16]).is_none());
    }

    #[test]
    fn fact_signature_includes_owner_and_known_targets() {
        use crate::resolver_core::FactVersionRef;
        let surface = mk_surface([7u8; 16]);
        // Owner + Foo (known hash) become `FileWholeHash` facts; Bar has
        // no hash and is not included.
        let owner_target_facts: Vec<&FactVersionRef> = surface
            .read_set_signature
            .facts
            .iter()
            .filter(|f| matches!(f, FactVersionRef::FileWholeHash { .. }))
            .collect();
        assert_eq!(owner_target_facts.len(), 2);
        assert!(owner_target_facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == "/w/owner.ts" && *hash == [7u8; 16]
        )));
        assert!(owner_target_facts.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == "/w/foo.ts" && *hash == [1u8; 16]
        )));
    }

    #[test]
    fn empty_imports_produces_owner_only_signature() {
        use crate::resolver_core::FactVersionRef;
        let surface =
            build_owner_import_surface(Arc::from("/w/o.ts"), [1u8; 16], vec![], Vec::new(), 0);
        assert_eq!(surface.bindings.len(), 0);
        let owner_target_facts: Vec<&FactVersionRef> = surface
            .read_set_signature
            .facts
            .iter()
            .filter(|f| matches!(f, FactVersionRef::FileWholeHash { .. }))
            .collect();
        assert_eq!(owner_target_facts.len(), 1);
        assert!(matches!(
            owner_target_facts[0],
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == "/w/o.ts"
        ));
    }
}
