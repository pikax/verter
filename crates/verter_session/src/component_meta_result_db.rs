//! Final component-meta result cache.
//!
//! [`ComponentMetaResultDb`] is the authoritative final-payload cache for
//! component-meta results. Identical repeated requests on an unchanged
//! owner return from this cache with near-zero resolver work; concurrent
//! cold requests for the same owner/query coalesce onto one build.
//!
//! ## Contract
//!
//! - **Slot key:** [`ComponentMetaResultKey`] =
//!   `(owner_canonical, options_fingerprint)` — content-free, per the
//!   query-identity-cache model. The owner's content version
//!   (`owner_whole_hash`) is NOT part of the slot key; it is carried by
//!   the candidate and validated strictly on read.
//! - **Slot value:** a bounded candidate list. Concurrent overlay
//!   variants of the same owner coexist as candidates in one slot,
//!   capped at [`ComponentMetaResultDb::PER_SLOT_CANDIDATE_CAP`]; a
//!   global insertion-ordered budget
//!   ([`ComponentMetaResultDb::GLOBAL_BUDGET`]) caps the total candidate
//!   count across all slots. Eviction is FIFO (oldest insertion first);
//!   evicting a still-valid candidate only forces a recompute. The
//!   bounded substrate is the routine memory-reclamation path — old
//!   per-version entries do not accumulate unbounded in a long-lived
//!   session.
//! - **Candidate payload:** an immutable `Arc` payload — the native
//!   component-meta result and any strictly projected derivatives — plus
//!   the exact [`crate::fact_signature_helpers::ReadSetSignature`] the
//!   build observed. Lookups revalidate that signature against the live
//!   host.
//! - **`options_fingerprint` is a stable `Hash16`** produced from a
//!   manually-stable serialization of output-affecting fields only —
//!   never request ids, trace flags, or caller metadata.
//! - Cancelled, budget-exceeded, or partial results are **not** promoted
//!   into the cache. They must surface as `QueryError` variants to the
//!   caller.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_semantic::analysis::Hash16;

use crate::bounded_query_retention::BoundedCandidateMap;
use crate::resolver_core::StoreView;
use crate::types::ProjectionMode;

/// Stable fingerprint over output-affecting options. Constructed by the
/// caller from an explicitly versioned serialization; the type alias
/// points at the workspace-wide [`Hash16`] so downstream tooling does not
/// invent a parallel hash.
pub type ComponentMetaOptionsFingerprint = Hash16;

/// Content-free slot key for the final component-meta result.
///
/// The owner's content version is intentionally absent — concurrent
/// content versions of the same owner coexist as candidates inside the
/// slot this key addresses (the documented query-identity-cache model).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaResultKey {
    pub owner_canonical: Arc<str>,
    pub options_fingerprint: ComponentMetaOptionsFingerprint,
}

/// Cache entry — the payload plus the carrier holding both
/// dependency-signature rails.
///
/// The carrier's legacy rail is the whole-hash / project-generation
/// signature validated through
/// [`crate::host_manage::HostFenceValidator`]; the fact rail is the
/// path-precise signature produced by the `with_fact_tracer` scope
/// wrapping the cold compute. Warm-hit reads gate on
/// [`StoreView::validates_fact_signature`] BEFORE falling through to
/// the legacy validator so a fact-version bump on any cross-file dep
/// invalidates the warm hit without consulting the legacy whole-hash
/// oracle.
pub struct ComponentMetaResultEntry<P> {
    pub payload: Arc<P>,
    pub read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
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
            // Rehydrated state was synthesised cold; suppression decisions
            // already applied at publish time. Synthesis diagnostics live
            // on the cached `ComponentMetaAnalysis.macro_expansion_diagnostics`.
            synthesis_diagnostics: Vec::new(),
            synthesis_should_suppress: false,
        })
    }
}

impl<P> Clone for ComponentMetaResultEntry<P> {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload.clone(),
            read_set_signature: self.read_set_signature.clone(),
        }
    }
}

/// Host-owned final result cache. Generic over the payload type so native
/// and compat projections can share the same backing without double-caching
/// semantic meaning.
///
/// Backed by [`BoundedCandidateMap`]: the slot key is the content-free
/// [`ComponentMetaResultKey`], the per-candidate discriminant is the
/// owner whole-hash, and the candidate payload is a
/// [`ComponentMetaResultEntry`]. Per-slot and global caps reclaim old
/// per-version entries write-side so a long-lived session does not grow
/// the cache monotonically with the owner edit count.
pub struct ComponentMetaResultDb<P> {
    inner: BoundedCandidateMap<ComponentMetaResultKey, Hash16, ComponentMetaResultEntry<P>>,
    /// Tracks the substrate's live candidate count. Synced from
    /// [`BoundedCandidateMap::live_count`] after every mutation so the
    /// `ProjectTypeStore` counter snapshot reports true occupancy.
    live_counter: Arc<AtomicU64>,
    /// Counts every eviction / removal — replaces the historical
    /// "stale sweep" counter.
    stale_sweeps: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl<P> ComponentMetaResultDb<P> {
    /// Per-slot candidate cap. One owner + one options fingerprint is one
    /// slot; concurrent content versions of that owner are candidates in
    /// the slot, capped here. A fifth version evicts the oldest. Four
    /// covers the `{current, previous, two concurrent overlay}` working
    /// set (architecture rule R20 multi-candidate model) — the shared
    /// substrate's [`crate::bounded_query_retention::DEFAULT_CANDIDATE_CAP`].
    pub const PER_SLOT_CANDIDATE_CAP: usize = crate::bounded_query_retention::DEFAULT_CANDIDATE_CAP;

    /// Global total-candidate budget across every slot. A long-lived
    /// editor session touching many distinct owners caps here before
    /// FIFO eviction reclaims the oldest candidates. Tuned against the
    /// plan's memory budget order-of-magnitude.
    pub const GLOBAL_BUDGET: usize = 512;

    #[must_use]
    pub fn new() -> Self {
        Self::with_counters(Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counters(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
    ) -> Self {
        Self::with_counters_and_schema_version(
            live_counter,
            stale_sweeps,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, debug_assertions))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counters_and_schema_version(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            schema_version,
        )
    }

    fn with_counters_and_schema_version(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
        schema_version: u32,
    ) -> Self {
        Self {
            inner: BoundedCandidateMap::with_caps(
                Self::PER_SLOT_CANDIDATE_CAP,
                Self::GLOBAL_BUDGET,
            ),
            live_counter,
            stale_sweeps,
            schema_version,
        }
    }

    /// Sync the external live counter to the substrate's authoritative
    /// candidate count. Called after every mutation.
    fn sync_live_counter(&self) {
        self.live_counter
            .store(self.inner.live_count() as u64, Ordering::Relaxed);
    }

    /// Per-slot candidate cap currently configured on the substrate.
    #[must_use]
    pub fn per_slot_candidate_cap(&self) -> usize {
        self.inner.per_slot_cap()
    }

    /// Global total-candidate budget currently configured on the
    /// substrate.
    #[must_use]
    pub fn global_budget(&self) -> usize {
        self.inner.global_cap()
    }

    /// Strict lookup — returns the cached entry for the given owner
    /// content version when a matching candidate is present. The caller
    /// is responsible for revalidating the dep signature before
    /// publishing the result; this split keeps the cache decoupled from
    /// the live host.
    ///
    /// Lookups against a Db whose `schema_version` does not match the
    /// current [`crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION`]
    /// return `None`.
    #[must_use]
    pub fn get(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<ComponentMetaResultEntry<P>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = self
            .inner
            .get_candidate(key, &owner_whole_hash)
            .map(|c| c.value.clone());
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

    /// View-aware lookup. Returns the cached entry only when a candidate
    /// matches the owner content version AND that candidate's
    /// `fact_dep_signature` validates under the supplied [`StoreView`];
    /// otherwise returns `None` (caller falls through to cold recompute).
    ///
    /// Fact-precise validation is the primary cache oracle: a fact
    /// version shift on any transitively observed cross-file dep
    /// invalidates the warm hit without consulting the legacy
    /// `dep_signature` whole-hash validator. The legacy signature
    /// continues to be carried on the candidate; `get_with_view` does NOT
    /// consult it — callers that need the legacy oracle revalidate
    /// separately.
    ///
    /// Increments [`crate::types::MetaProvenance::component_meta_result_cache_hits`]
    /// on a validated warm return and
    /// [`crate::types::MetaProvenance::component_meta_result_cache_misses`]
    /// on every miss path (absent candidate OR fact-validation failure).
    #[must_use]
    pub fn get_with_view<V: StoreView + ?Sized>(
        &self,
        host: &crate::VerterHost,
        view: &V,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<Arc<ComponentMetaResultEntry<P>>> {
        let bump_miss = |host: &crate::VerterHost| {
            host.provenance()
                .component_meta_result_cache_misses
                .fetch_add(1, Ordering::Relaxed);
            // Keep the per-request `cache_layers.component_meta` audit
            // counter in sync with the `.get()` accessor so
            // joiner-accounting assertions continue to attribute a miss
            // to the cold winner.
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.cache_counters
                    .component_meta
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        };
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            bump_miss(host);
            return None;
        }
        // Clone the candidate `Arc` out of the slot before validating —
        // a concurrent eviction cannot invalidate this borrow.
        let candidate = match self.inner.get_candidate(key, &owner_whole_hash) {
            Some(c) => c,
            None => {
                bump_miss(host);
                return None;
            }
        };
        // Fact-precise validation: every entry in the signature must
        // validate under the live view. An empty signature trivially
        // passes (entries published outside an installed tracer scope —
        // typically test fixtures — fall through to the legacy validator
        // on the caller side).
        if !view.validates_fact_signature(&candidate.value.read_set_signature.facts) {
            bump_miss(host);
            return None;
        }
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.cache_counters
                .component_meta
                .hits
                .fetch_add(1, Ordering::Relaxed);
        }
        host.provenance()
            .component_meta_result_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        Some(Arc::new(candidate.value.clone()))
    }

    /// Insert a final result entry for the given owner content version.
    /// Cancelled, budget-exceeded, or partial results must **not** be
    /// passed here — callers are responsible for filtering. The cache
    /// does not inspect the payload.
    ///
    /// The owner whole-hash is the candidate discriminant: re-inserting
    /// the same `(key, owner_whole_hash)` refreshes the candidate in
    /// place; a new owner content version appends a candidate to the
    /// slot, and the bounded substrate evicts the oldest candidate /
    /// global-oldest entry to stay within the per-slot and global caps.
    pub fn insert(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        entry: ComponentMetaResultEntry<P>,
    ) {
        let evicted = self.inner.admit(key, owner_whole_hash, entry);
        if evicted > 0 {
            self.stale_sweeps
                .fetch_add(evicted as u64, Ordering::Relaxed);
        }
        self.sync_live_counter();
    }

    /// Remove the candidate for one owner content version. Returns the
    /// removed entry when present.
    pub fn remove(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<ComponentMetaResultEntry<P>> {
        let candidate = self.inner.get_candidate(key, &owner_whole_hash)?;
        let removed = self.inner.evict_candidate(key, candidate.seq);
        if removed {
            self.stale_sweeps.fetch_add(1, Ordering::Relaxed);
            self.sync_live_counter();
            Some(candidate.value.clone())
        } else {
            None
        }
    }

    /// Drop every cached entry. Called on project-generation bumps
    /// (tsconfig / SDK / workspace-folder changes) — final results
    /// depend on routes and intrinsic resolution, which project-shape
    /// changes may shift.
    pub fn invalidate_all(&self) {
        let removed = self.inner.clear();
        if removed > 0 {
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        self.sync_live_counter();
    }

    /// Invalidate every cached entry whose owner canonical matches
    /// `owner_canonical`, across all owner whole-hashes and options
    /// fingerprints. Called on owner-file content changes. Returns the
    /// number of candidates evicted.
    pub fn invalidate_owner(&self, owner_canonical: &str) -> usize {
        let removed = self
            .inner
            .retain_slots(|key| key.owner_canonical.as_ref() != owner_canonical);
        if removed > 0 {
            self.stale_sweeps
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        self.sync_live_counter();
        removed
    }

    /// Total live candidate count across every slot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.live_count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.live_count() == 0
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant. The caller supplies a payload
    /// constructor so generic-parameter Dbs (`ComponentMetaResultDb<P>`)
    /// can be exercised without binding the helper to a single payload
    /// type.
    #[cfg(any(test, debug_assertions))]
    pub fn insert_synthetic_for_schema_test_with_payload(&self, marker: &str, payload: P) {
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from(marker),
            options_fingerprint: [0u8; 16],
        };
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(payload),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
        };
        self.insert(key, [0u8; 16], entry);
    }
}

impl ComponentMetaResultDb<CachedComponentMetaResult> {
    /// Test-only accessor returning the merged carrier dep_signature
    /// canonicals for an owner. Used by the slot-binding regression
    /// `slot_bindings_dep_signature_merges_carrier_deps` to inspect
    /// the dep-signature carriers stored alongside the cached entry.
    /// Returns an empty vec when the owner has no cached entry.
    ///
    /// Constructs the lookup key with the same options fingerprint
    /// the production `publish_component_meta_cache_entry` writes
    /// (the default `ComponentMetaOptions` fingerprint), so the
    /// lookup matches the published entry. A bare
    /// `ComponentMetaOptionsFingerprint::default()` (= zeros) would
    /// silently miss every published entry, masking real cache-key
    /// drift behind a permanently empty result.
    #[cfg(test)]
    pub fn dep_signature_for_owner_in_test(
        host: &crate::VerterHost,
        owner_canonical: &str,
    ) -> Vec<std::sync::Arc<str>> {
        let store = host.project_type_store();
        let whole_hash = host
            .ensure_indexed_ready(owner_canonical)
            .map(|ir| ir.whole_hash)
            .unwrap_or_default();
        let key = ComponentMetaResultKey {
            owner_canonical: std::sync::Arc::from(owner_canonical),
            options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
                &crate::host_manage::ComponentMetaOptions::default(),
            ),
        };
        let backing = store.component_meta_results();
        match backing.get(&key, whole_hash) {
            Some(entry) => entry
                .read_set_signature
                .legacy
                .iter()
                .map(|(canonical, _)| std::sync::Arc::clone(canonical))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Test-only accessor returning whether the owner has a cached
    /// entry. Used by the slot-binding regression
    /// `slot_bindings_skip_cache_on_budget_exceeded` to assert that
    /// fatal-suppression synthesis runs do not warm the cache.
    ///
    /// Constructs the lookup key with the same options fingerprint
    /// the production `publish_component_meta_cache_entry` writes
    /// (the default `ComponentMetaOptions` fingerprint).
    #[cfg(test)]
    pub fn has_owner_entry_in_test(host: &crate::VerterHost, owner_canonical: &str) -> bool {
        let store = host.project_type_store();
        let whole_hash = host
            .ensure_indexed_ready(owner_canonical)
            .map(|ir| ir.whole_hash)
            .unwrap_or_default();
        let key = ComponentMetaResultKey {
            owner_canonical: std::sync::Arc::from(owner_canonical),
            options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
                &crate::host_manage::ComponentMetaOptions::default(),
            ),
        };
        store
            .component_meta_results()
            .get(&key, whole_hash)
            .is_some()
    }
}

impl<P> Default for ComponentMetaResultDb<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> crate::cache_schema::CacheSchemaVersioned for ComponentMetaResultDb<P> {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        let count = self.inner.clear();
        if count > 0 {
            self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        }
        self.sync_live_counter();
        count
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
        // A content edit on the owner canonical drops every cached
        // result for that owner across all whole-hashes and options.
        self.invalidate_owner(canonical_id)
    }
}

impl<P> std::fmt::Debug for ComponentMetaResultDb<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentMetaResultDb")
            .field("live_candidates", &self.len())
            .field("per_slot_cap", &self.per_slot_candidate_cap())
            .field("global_budget", &self.global_budget())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::DepVersion;

    fn empty_sig() -> crate::fact_signature_helpers::ReadSetSignature {
        crate::fact_signature_helpers::ReadSetSignature::empty()
    }

    #[test]
    fn insert_and_get_roundtrip() {
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct MockPayload(u32);
        let db: ComponentMetaResultDb<MockPayload> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/Accordion.vue"),
            options_fingerprint: [9u8; 16],
        };
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(MockPayload(42)),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(
                Arc::from(Vec::<crate::resolver_core::FactVersionRef>::new()),
                Arc::from(
                    vec![(
                        Arc::<str>::from("/w/Accordion.vue"),
                        DepVersion::WholeHash([1u8; 16]),
                    )]
                    .into_boxed_slice(),
                ),
            ),
        };
        db.insert(key.clone(), [1u8; 16], entry);
        let hit = db.get(&key, [1u8; 16]).unwrap();
        assert_eq!(*hit.payload, MockPayload(42));
        assert_eq!(hit.read_set_signature.legacy.len(), 1);
    }

    #[test]
    fn distinct_options_fingerprints_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k1 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            options_fingerprint: [1u8; 16],
        };
        let k2 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            options_fingerprint: [2u8; 16],
        };
        db.insert(
            k1.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                read_set_signature: empty_sig(),
            },
        );
        assert!(db.get(&k1, [1u8; 16]).is_some());
        assert!(db.get(&k2, [1u8; 16]).is_none());
    }

    /// Distinct owner content versions are distinct candidates inside
    /// one slot — a lookup for one version never returns another's
    /// payload.
    #[test]
    fn distinct_owner_hashes_are_distinct_candidates() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            options_fingerprint: [9u8; 16],
        };
        db.insert(
            key.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                read_set_signature: empty_sig(),
            },
        );
        db.insert(
            key.clone(),
            [2u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(2u32),
                read_set_signature: empty_sig(),
            },
        );
        assert_eq!(*db.get(&key, [1u8; 16]).unwrap().payload, 1);
        assert_eq!(*db.get(&key, [2u8; 16]).unwrap().payload, 2);
        assert!(db.get(&key, [3u8; 16]).is_none());
        // Both versions coexist as candidates in the one slot.
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn remove_clears_entry() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            options_fingerprint: [0u8; 16],
        };
        db.insert(
            key.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(5u32),
                read_set_signature: empty_sig(),
            },
        );
        assert!(db.remove(&key, [1u8; 16]).is_some());
        assert!(db.get(&key, [1u8; 16]).is_none());
    }

    /// The per-slot candidate cap bounds how many content versions of
    /// one owner are retained. DISCRIMINATES: an unbounded slot would
    /// retain every version.
    #[test]
    fn per_slot_cap_bounds_owner_versions() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            options_fingerprint: [0u8; 16],
        };
        // Insert one more version than the per-slot cap.
        let versions = ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP + 3;
        for v in 0..versions {
            let mut hash = [0u8; 16];
            hash[0] = v as u8;
            db.insert(
                key.clone(),
                hash,
                ComponentMetaResultEntry {
                    payload: Arc::new(v as u32),
                    read_set_signature: empty_sig(),
                },
            );
        }
        assert_eq!(
            db.len(),
            ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP,
            "the slot must retain at most PER_SLOT_CANDIDATE_CAP versions",
        );
        // The oldest versions were evicted; the newest survive.
        let mut hash_last = [0u8; 16];
        hash_last[0] = (versions - 1) as u8;
        assert!(
            db.get(&key, hash_last).is_some(),
            "the newest version must still be cached",
        );
        let mut hash_first = [0u8; 16];
        hash_first[0] = 0;
        assert!(
            db.get(&key, hash_first).is_none(),
            "the oldest version must have been evicted by the bounded cap",
        );
    }

    #[test]
    fn cap_constants_match_plan() {
        assert_eq!(ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP, 4);
        assert_eq!(ComponentMetaResultDb::<u32>::GLOBAL_BUDGET, 512);
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        assert_eq!(db.per_slot_candidate_cap(), 4);
        assert_eq!(db.global_budget(), 512);
    }

    /// `invalidate_owner` drops every candidate whose owner canonical
    /// matches, regardless of owner whole-hash / options. Unrelated
    /// owners stay warm.
    #[test]
    fn invalidate_owner_removes_all_keys_for_one_canonical() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let mk_key = |owner: &str| ComponentMetaResultKey {
            owner_canonical: Arc::from(owner),
            options_fingerprint: [0u8; 16],
        };
        let mk_entry = || ComponentMetaResultEntry {
            payload: Arc::new(1u32),
            read_set_signature: empty_sig(),
        };

        // Two versions for /w/a.vue, one for /w/b.vue.
        db.insert(mk_key("/w/a.vue"), [1u8; 16], mk_entry());
        db.insert(mk_key("/w/a.vue"), [2u8; 16], mk_entry());
        db.insert(mk_key("/w/b.vue"), [1u8; 16], mk_entry());

        let removed = db.invalidate_owner("/w/a.vue");
        assert_eq!(removed, 2);
        // /w/b.vue stays.
        assert!(db.get(&mk_key("/w/b.vue"), [1u8; 16]).is_some());
        // /w/a.vue is fully gone.
        assert!(db.get(&mk_key("/w/a.vue"), [1u8; 16]).is_none());
    }
}
