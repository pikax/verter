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
//!   caller merges into the active
//!   [`CompletionFence`](crate::completion_fence::CompletionFence).

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
    /// Dep-signature fragment the caller merges into an active
    /// [`CompletionFence`](crate::completion_fence::CompletionFence). The
    /// owner's own whole-hash is always included; transitive target hashes
    /// are appended as they are observed.
    pub dep_signature: DepSignature,
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
    #[cfg(any(test, debug_assertions))]
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
    /// the key level so no callers observe a mixed-version surface.
    ///
    /// Lookups against a Db whose `schema_version` does not match the current
    /// [`crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION`] return `None`
    /// without consulting the entry map. Use [`Self::evict_if_schema_mismatch`]
    /// to drain the storage; it is exposed so test fixtures can verify the
    /// stale-eviction invariant deterministically.
    #[must_use]
    pub fn get(
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

    /// Insert or replace the surface for `owner_canonical`. A replacement
    /// does not change the live-entry count.
    pub fn insert(&self, owner_canonical: Arc<str>, surface: Arc<OwnerImportSurface>) {
        let prev = self.entries.insert(owner_canonical, surface);
        if prev.is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove the surface outright — used on file-close / hard evict.
    pub fn remove(&self, owner_canonical: &str) {
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
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        let canonical: Arc<str> = Arc::from(marker);
        let surface = Arc::new(OwnerImportSurface {
            owner_canonical: Arc::clone(&canonical),
            owner_whole_hash: [0u8; 16],
            bindings: Arc::new(FxHashMap::default()),
            dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
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
/// iterator. Callers provide the owner identity, content hash, and the
/// pre-resolved `(local_name, canonical_id, exported_name,
/// target_whole_hash)` tuples; the helper normalizes them into the
/// host-owned representation.
pub fn build_owner_import_surface<I>(
    owner_canonical: Arc<str>,
    owner_whole_hash: Hash16,
    resolved_imports: I,
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

    // Stable order so the dep signature is deterministic for downstream
    // validation.
    sig_entries.sort_by(|a, b| a.0.cmp(&b.0));
    sig_entries.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    Arc::new(OwnerImportSurface {
        owner_canonical,
        owner_whole_hash,
        bindings: Arc::new(bindings),
        dep_signature: Arc::from(sig_entries.into_boxed_slice()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn dep_signature_includes_owner_and_known_targets() {
        let surface = mk_surface([7u8; 16]);
        // Owner + Foo (known hash); Bar has no hash and is not included.
        let signed: Vec<_> = surface
            .dep_signature
            .iter()
            .map(|(c, v)| (c.as_ref().to_string(), v.clone()))
            .collect();
        assert_eq!(signed.len(), 2);
        assert!(signed
            .iter()
            .any(|(c, v)| c == "/w/owner.ts"
                && matches!(v, DepVersion::WholeHash(h) if *h == [7u8; 16])));
        assert!(signed
            .iter()
            .any(|(c, v)| c == "/w/foo.ts"
                && matches!(v, DepVersion::WholeHash(h) if *h == [1u8; 16])));
    }

    #[test]
    fn empty_imports_produces_owner_only_signature() {
        let surface = build_owner_import_surface(Arc::from("/w/o.ts"), [1u8; 16], vec![]);
        assert_eq!(surface.bindings.len(), 0);
        assert_eq!(surface.dep_signature.len(), 1);
        assert_eq!(surface.dep_signature[0].0.as_ref(), "/w/o.ts");
    }
}
