//! Final component-meta result cache.
//!
//! [`ComponentMetaResultDb`] is the authoritative final-payload cache for
//! component-meta results. Identical repeated requests on an unchanged
//! owner return from this cache with near-zero resolver work; concurrent
//! cold requests for the same owner/query coalesce onto one build.
//!
//! ## Contract
//!
//! - Key: [`ComponentMetaResultKey`] =
//!   `(owner_canonical, owner_whole_hash, options_fingerprint)`.
//! - Value: immutable `Arc` payloads — the native component-meta result
//!   and any strictly projected derivatives.
//! - **Transitive dependency validation on lookup**: every warm entry
//!   stores the exact [`DepSignature`] it observed at build time; lookups
//!   revalidate that signature against the live host.
//! - **`options_fingerprint` is a stable `Hash16`** produced from a
//!   manually-stable serialization of output-affecting fields only —
//!   never request ids, trace flags, or caller metadata.
//! - Cancelled, budget-exceeded, or partial results are **not** promoted
//!   into the cache. They must surface as `QueryError` variants to the
//!   caller.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::Hash16;

use crate::semantic_query::DepSignature;
use crate::types::ProjectionMode;

/// Stable fingerprint over output-affecting options. Constructed by the
/// caller from an explicitly versioned serialization; the type alias
/// points at the workspace-wide [`Hash16`] so downstream tooling does not
/// invent a parallel hash.
pub type ComponentMetaOptionsFingerprint = Hash16;

/// Cache key for the final component-meta result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaResultKey {
    pub owner_canonical: Arc<str>,
    pub owner_whole_hash: Hash16,
    pub options_fingerprint: ComponentMetaOptionsFingerprint,
}

/// Cache entry — the payload plus the exact dep signature observed during
/// the build.
pub struct ComponentMetaResultEntry<P> {
    pub payload: Arc<P>,
    pub dep_signature: DepSignature,
}

/// Sanitized snapshot of a
/// [`crate::meta_resolve::ResolvedComponentMetaState`] suitable for
/// cross-request reuse. Excludes per-request fields (`request_id`,
/// `compute_audit`) and the [`FileAnalysisSnapshot`] (reloaded from
/// `ProjectTypeStore::indexed()` at rehydrate time).
///
/// Field-by-field partition (per D4.1):
///
/// - **EXCLUDED — per-request, never cached:**
///   - `request_id: u64` (allocated per request).
///   - `compute_audit: Option<...>` (request-specific timings/counters).
///
/// - **EXCLUDED — snapshot-derived, reloaded from host:**
///   - `snapshot: FileAnalysisSnapshot` (reload via
///     `ProjectTypeStore::indexed().get(canonical, whole_hash)`).
///
/// - **INCLUDED — content-addressed via `dep_signature`:**
///   - `mode`, `whole_hash`.
///   - `resolved_macros`, `resolved_type_registry`,
///     `resolved_type_registry_meta`.
///   - `evaluated_types`.
///   - `fact_versions`.
///   - `surface_identities` (audit sidecar; cache, do not rehydrate as
///     None).
///   - `origin_graph` (audit sidecar; cache).
#[derive(Debug, Clone)]
pub struct ResolutionTemplate {
    pub mode: ProjectionMode,
    pub whole_hash: Hash16,
    pub resolved_macros: Vec<crate::meta_resolve::ResolvedMacroMeta>,
    pub resolved_type_registry:
        Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    pub resolved_type_registry_meta: Vec<crate::meta_resolve::ResolvedTypeRegistryMeta>,
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
}

/// Cached component-meta payload AND its sanitized
/// resolution sidecar. The DB generic migrates from
/// `ComponentMetaResultDb<ComponentMetaAnalysis>` to
/// `ComponentMetaResultDb<CachedComponentMetaResult>` so warm-cache
/// hits on the audit-enabled path
/// (`VerterHost::get_component_meta_with_resolution`) can rehydrate
/// both halves without rerunning the cold resolver.
#[derive(Debug, Clone)]
pub struct CachedComponentMetaResult {
    pub analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    pub resolution_template: ResolutionTemplate,
    /// Owner canonical id used to reload `snapshot` via
    /// [`ProjectTypeStore::indexed()`] on rehydrate.
    pub canonical_id: Arc<str>,
    /// Owner whole-hash this template was produced against.
    pub whole_hash: Hash16,
}

impl ResolutionTemplate {
    /// Build a template by sanitizing a freshly-resolved
    /// [`crate::meta_resolve::ResolvedComponentMetaState`]. Strips
    /// `request_id`, `snapshot`, and `compute_audit`; keeps the
    /// content-addressed sidecars.
    #[must_use]
    pub fn from_resolved_state(resolved: &crate::meta_resolve::ResolvedComponentMetaState) -> Self {
        Self {
            mode: resolved.mode,
            whole_hash: resolved.whole_hash,
            resolved_macros: resolved.resolved_macros.clone(),
            resolved_type_registry: resolved.resolved_type_registry.clone(),
            resolved_type_registry_meta: resolved.resolved_type_registry_meta.clone(),
            evaluated_types: resolved.evaluated_types.clone(),
            fact_versions: resolved.fact_versions.clone(),
            surface_identities: resolved.surface_identities.clone(),
            origin_graph: resolved.origin_graph.clone(),
        }
    }

    /// Rehydrate the template into a per-request
    /// [`crate::meta_resolve::ResolvedComponentMetaState`]:
    ///
    /// - **`snapshot`** reloaded from `host.project_type_store().indexed()`
    ///   at `(canonical_id, whole_hash)`. Returns `None` on a bounded
    ///   eviction race (snapshot evicted between the dep_signature
    ///   validation and the reload); callers fall through to the cold
    ///   resolver.
    /// - **`request_id`** is the caller-allocated fresh id.
    /// - **`compute_audit`** stays `None` on warm-cache hits — the
    ///   audit-record consumer observes `from_cache = true` and
    ///   `total_ms = 0` instead.
    /// - All other fields are restored from the cached template.
    pub fn rehydrate(
        &self,
        host: &crate::VerterHost,
        canonical_id: &str,
        whole_hash: Hash16,
        request_id: u64,
    ) -> Option<crate::meta_resolve::ResolvedComponentMetaState> {
        let indexed = host
            .project_type_store()
            .indexed()
            .get(canonical_id, whole_hash)?;
        let snapshot = (*indexed.snapshot).clone();
        Some(crate::meta_resolve::ResolvedComponentMetaState {
            snapshot,
            mode: self.mode,
            whole_hash: self.whole_hash,
            resolved_macros: self.resolved_macros.clone(),
            resolved_type_registry: self.resolved_type_registry.clone(),
            resolved_type_registry_meta: self.resolved_type_registry_meta.clone(),
            evaluated_types: self.evaluated_types.clone(),
            fact_versions: self.fact_versions.clone(),
            compute_audit: None,
            surface_identities: self.surface_identities.clone(),
            origin_graph: self.origin_graph.clone(),
            request_id,
        })
    }
}

impl<P> Clone for ComponentMetaResultEntry<P> {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload.clone(),
            dep_signature: self.dep_signature.clone(),
        }
    }
}

/// Host-owned final result cache. Generic over the payload type so native
/// and compat projections can share the same backing without double-caching
/// semantic meaning.
pub struct ComponentMetaResultDb<P> {
    entries: DashMap<ComponentMetaResultKey, ComponentMetaResultEntry<P>>,
    /// Soft size cap — when exceeded, bounded cleanup slices run on the
    /// top-level query exit path. Cleanup in is
    /// `stale-first, LRU next`; precise policy lives with the dispatcher.
    capacity: usize,
    live_counter: Arc<AtomicU64>,
    stale_sweeps: Arc<AtomicU64>,
}

impl<P> ComponentMetaResultDb<P> {
    /// Default capacity — editor sessions with many owner/options
    /// combinations cap here before triggering bounded stale cleanup. Tuned
    /// later against live profiling; 512 matches the order-of-magnitude
    /// called out in the plan's memory budget.
    pub const DEFAULT_CAPACITY: usize = 512;

    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_counters(
            capacity,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
    }

    pub(crate) fn with_counters(
        capacity: usize,
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            capacity,
            live_counter,
            stale_sweeps,
        }
    }

    /// Cache size target. Exceeding it triggers bounded stale cleanup.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Strict lookup — returns the cached entry when present. The caller
    /// is responsible for revalidating the dep signature before publishing
    /// the result; this split keeps the cache decoupled from the live host.
    #[must_use]
    pub fn get(&self, key: &ComponentMetaResultKey) -> Option<ComponentMetaResultEntry<P>> {
        let result = self.entries.get(key).map(|v| v.clone());
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .component_meta
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .component_meta
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Insert a final result entry. Cancelled, budget-exceeded, or partial
    /// results must **not** be passed here — callers are responsible for
    /// filtering. The cache does not inspect the payload.
    pub fn insert(&self, key: ComponentMetaResultKey, entry: ComponentMetaResultEntry<P>) {
        let prev = self.entries.insert(key, entry);
        if prev.is_some() {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        } else {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove a key outright.
    pub fn remove(&self, key: &ComponentMetaResultKey) -> Option<ComponentMetaResultEntry<P>> {
        let removed = self.entries.remove(key).map(|(_, v)| v);
        if removed.is_some() {
            self.live_counter.fetch_sub(1, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
        }
        removed
    }

    /// Drop every cached entry. Called on project-generation bumps
    /// (tsconfig / SDK / workspace-folder changes) — final results
    /// depend on routes and intrinsic resolution, which project-shape
    /// changes may shift.
    pub fn invalidate_all(&self) {
        let count = self.entries.len();
        self.entries.clear();
        if count > 0 {
            self.live_counter.fetch_sub(count as u64, Ordering::Relaxed);
            self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        }
    }

    /// Invalidate every cached entry whose owner canonical matches
    /// `owner_canonical`, across all owner whole-hashes, query kinds, and
    /// options fingerprints. Called on owner-file content changes. Returns
    /// the number of entries evicted.
    pub fn invalidate_owner(&self, owner_canonical: &str) -> usize {
        let mut removed = 0usize;
        self.entries.retain(|key, _| {
            if key.owner_canonical.as_ref() == owner_canonical {
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

    /// Iterate through keys present in the cache. Primarily used by the
    /// bounded cleanup helper below.
    #[must_use]
    pub fn keys(&self) -> Vec<ComponentMetaResultKey> {
        self.entries
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl<P> Default for ComponentMetaResultDb<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> crate::invalidation_domain::ParticipatesInInvalidation for ComponentMetaResultDb<P>
where
    P: Send + Sync,
{
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

impl<P> crate::invalidation_domain::InvalidationByCanonical for ComponentMetaResultDb<P>
where
    P: Send + Sync,
{
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // The result key is (owner_canonical, owner_whole_hash, ...);
        // a content edit on the owner canonical drops every cached
        // result for that owner across all whole-hashes.
        self.invalidate_owner(canonical_id)
    }
}

impl<P> std::fmt::Debug for ComponentMetaResultDb<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentMetaResultDb")
            .field("entries", &self.len())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::DepVersion;

    #[test]
    fn insert_and_get_roundtrip() {
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct MockPayload(u32);
        let db: ComponentMetaResultDb<MockPayload> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/Accordion.vue"),
            owner_whole_hash: [1u8; 16],
            options_fingerprint: [9u8; 16],
        };
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(MockPayload(42)),
            dep_signature: Arc::from(
                vec![(
                    Arc::<str>::from("/w/Accordion.vue"),
                    DepVersion::WholeHash([1u8; 16]),
                )]
                .into_boxed_slice(),
            ),
        };
        db.insert(key.clone(), entry);
        let hit = db.get(&key).unwrap();
        assert_eq!(*hit.payload, MockPayload(42));
        assert_eq!(hit.dep_signature.len(), 1);
    }

    #[test]
    fn distinct_options_fingerprints_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k1 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            options_fingerprint: [1u8; 16],
        };
        let k2 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            options_fingerprint: [2u8; 16],
        };
        db.insert(
            k1.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.get(&k1).is_some());
        assert!(db.get(&k2).is_none());
    }

    #[test]
    fn distinct_owner_hashes_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k_v1 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            options_fingerprint: [9u8; 16],
        };
        let k_v2 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [2u8; 16],
            options_fingerprint: [9u8; 16],
        };
        db.insert(
            k_v1.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.get(&k_v1).is_some());
        assert!(db.get(&k_v2).is_none());
    }

    #[test]
    fn remove_clears_entry() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            options_fingerprint: [0u8; 16],
        };
        db.insert(
            key.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(5u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.remove(&key).is_some());
        assert!(db.get(&key).is_none());
    }

    #[test]
    fn capacity_default_matches_plan() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        assert_eq!(
            db.capacity(),
            ComponentMetaResultDb::<u32>::DEFAULT_CAPACITY
        );
        assert_eq!(ComponentMetaResultDb::<u32>::DEFAULT_CAPACITY, 512);
    }

    /// `invalidate_owner` drops every entry whose owner canonical matches,
    /// regardless of owner whole-hash / options. Unrelated owners stay warm.
    #[test]
    fn invalidate_owner_removes_all_keys_for_one_canonical() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let mk_key = |owner: &str, hash: [u8; 16]| ComponentMetaResultKey {
            owner_canonical: Arc::from(owner),
            owner_whole_hash: hash,
            options_fingerprint: [0u8; 16],
        };
        let mk_entry = || ComponentMetaResultEntry {
            payload: Arc::new(1u32),
            dep_signature: Arc::from(Vec::new().into_boxed_slice()),
        };

        // Two entries for /w/a.vue (different hashes), one for /w/b.vue.
        db.insert(mk_key("/w/a.vue", [1u8; 16]), mk_entry());
        db.insert(mk_key("/w/a.vue", [2u8; 16]), mk_entry());
        db.insert(mk_key("/w/b.vue", [1u8; 16]), mk_entry());

        let removed = db.invalidate_owner("/w/a.vue");
        assert_eq!(removed, 2);
        // /w/b.vue stays.
        assert!(db.get(&mk_key("/w/b.vue", [1u8; 16])).is_some());
        // /w/a.vue is fully gone.
        assert!(db.get(&mk_key("/w/a.vue", [1u8; 16])).is_none());
    }
}
