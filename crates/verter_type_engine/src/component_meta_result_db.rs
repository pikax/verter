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
//!   `(owner_canonical, options_fingerprint, project_identity,
//!   parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash)` —
//!   content-free with the full R21 split env axes, per the
//!   query-identity-cache model. The owner's content version
//!   (`owner_whole_hash`) is NOT part of the slot key; it is the
//!   candidate discriminant, carried by the candidate and validated
//!   strictly on read.
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
//!   the exact [`verter_session_query::facts::fact_cache::ReadSetSignature`] the
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

use verter_session_query::analysis::types::Hash16;

use crate::bounded_query_retention::BoundedCandidateMap;

/// Stable fingerprint over output-affecting options. Constructed by the
/// caller from an explicitly versioned serialization; the type alias
/// points at the workspace-wide [`Hash16`] so downstream tooling does not
/// invent a parallel hash.
pub type ComponentMetaOptionsFingerprint = Hash16;

/// Content-free slot key for the final component-meta result.
///
/// "Content-free" per R6: the owner's content version
/// (`owner_whole_hash`) is intentionally absent — concurrent content
/// versions of the same owner coexist as candidates inside the slot this
/// key addresses (the documented query-identity-cache model), discriminated
/// value-side by the candidate's owner whole-hash.
///
/// The key DOES carry the split env axes (R21): `project_identity` plus
/// the four `*_env_hash` dimensions. The final component-meta payload
/// depends on parse/resolve/type/lib env and the owning project, so
/// concurrent env or project variants of the same owner must occupy
/// DISTINCT slots — without these axes a lookup under project/env A could
/// alias onto a slot computed under project/env B. The axes are
/// view-independent (they key on the owning project, not file content),
/// so a single canonical builder
/// (the session key builder) yields identical
/// axes at both the lookup and publish sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaResultKey {
    pub owner_canonical: Arc<str>,
    pub options_fingerprint: ComponentMetaOptionsFingerprint,
    /// Owning-project identity (R21). Prevents cross-project aliasing of
    /// otherwise identical `(owner, options)` slots.
    pub project_identity: verter_session_query::resolution::ProjectIdentity,
    /// Split env axes (R21). The component-meta result depends on all
    /// four — parse (SFC/compiler flags), resolve (module resolution),
    /// type (semantic options), and lib (intrinsic corpus).
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}

/// Cache entry — the payload plus the carrier holding the
/// path-precise fact signature.
///
/// `read_set_signature.facts` is the path-precise signature produced
/// by the engine-owned raw fact-tracer scope wrapping cold compute — the
/// primary cache-validity rail. Warm-hit reads gate on
/// the request-bound `FactValidation::validates_fact_signature`: a fact-version bump on any
/// cross-file dep invalidates the warm hit.
///
/// `validated_at_generation` is the project-generation snapshot the
/// producer captured before its cold compute dispatched any work. The
/// `read_set_signature` carrier validates only file-content
/// whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias /
/// SDK / workspace-folder change) bumps no file content, so without
/// this stamp a stale-by-project-generation entry that raced a
/// `bump_project_generation_and_evict` cold-publish window would
/// validate forever on file-content terms.
/// The engine read operation rejects entries whose generation or fact signature
/// no longer validates through the request's `FactValidation` port. The
/// passive test-only [`ComponentMetaResultDb::get`] checks only the candidate
/// version; production reads use `ProjectSemanticDispatch::read_component_meta_result`.
pub struct ComponentMetaResultEntry<P> {
    pub payload: Arc<P>,
    pub read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature,
    pub validated_at_generation: u64,
}

/// Per-record byte estimates for the component-meta footprint.
///
/// Each constant is the approximate resident cost of one record of that
/// family — the struct plus the short owned strings and typed IR it
/// keeps alive. These are ACCOUNTING estimates: they decide whether the
/// process may RETAIN an entry, never whether a retained entry is valid.
/// An imprecise constant therefore costs hit rate, never correctness.
pub mod footprint {
    /// A surface record with a resolved type descriptor: a prop, event,
    /// slot, model, exposed member, or accepted-surface entry.
    pub const SURFACE_RECORD_BYTES: usize = 512;
    /// A lighter structural record: an import, binding, template ref,
    /// component usage, API call, or style block.
    pub const STRUCTURAL_RECORD_BYTES: usize = 192;
    /// A resolved type-registry analysis, which carries an expanded
    /// member list and is the heaviest per-record family.
    pub const TYPE_RECORD_BYTES: usize = 1024;
    /// One observed dependency fact on the entry's validity rail.
    pub const FACT_BYTES: usize = 64;
}

impl<P> verter_session_query::retention::RetainedFootprint for ComponentMetaResultEntry<P>
where
    P: verter_session_query::retention::RetainedFootprint,
{
    fn retained_footprint_bytes(&self) -> usize {
        self.payload.retained_footprint_bytes()
            + self.read_set_signature.facts.len() * footprint::FACT_BYTES
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
    }
}

impl<P> Clone for ComponentMetaResultEntry<P> {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload.clone(),
            read_set_signature: self.read_set_signature.clone(),
            validated_at_generation: self.validated_at_generation,
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
    /// Tracks the substrate's live candidate count for the
    /// `ProjectTypeStore` counter snapshot. Maintained by net-delta
    /// accounting ([`Self::apply_live_delta`]): every mutation applies
    /// the exact `added - removed` delta via atomic `fetch_add` /
    /// `fetch_sub`, so concurrent mutations compose without an absolute
    /// snapshot clobbering a newer count.
    live_counter: Arc<AtomicU64>,
    /// Counts every eviction / removal — replaces the historical
    /// "stale sweep" counter.
    #[cfg(any(test, feature = "semantic-observe"))]
    stale_sweeps: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
    /// The aggregate retained-byte account this cache admits against.
    ///
    /// There is no account-less result cache: the field carries a
    /// [`StoreAccount`](verter_session_query::retention::StoreAccount),
    /// whose `Default` is the ONE process-local account. A Db built
    /// outside a host type store therefore
    /// admits against the same ceiling rather than retaining entries that
    /// consume no aggregate headroom.
    retention_account: verter_session_query::retention::StoreAccount,
}

impl<P> ComponentMetaResultDb<P> {
    pub(crate) fn is_current_schema(&self) -> bool {
        self.schema_version == crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION
    }
    pub(crate) fn candidate(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<
        Arc<
            crate::bounded_query_retention::RetentionCandidate<Hash16, ComponentMetaResultEntry<P>>,
        >,
    > {
        self.inner.get_candidate(key, &owner_whole_hash)
    }

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

    pub fn with_counters(live_counter: Arc<AtomicU64>, stale_sweeps: Arc<AtomicU64>) -> Self {
        Self::with_counters_and_schema_version(
            live_counter,
            stale_sweeps,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
            verter_session_query::retention::StoreAccount::default(),
        )
    }

    /// The production constructor: counters plus the project's aggregate
    /// retention account, so every admitted entry's bytes are charged
    /// against the one process-local ceiling.
    pub fn with_counters_and_account(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
        retention_account: verter_session_query::retention::StoreAccount,
    ) -> Self {
        Self::with_counters_and_schema_version(
            live_counter,
            stale_sweeps,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
            retention_account,
        )
    }

    /// The aggregate retained-byte account this cache admits against.
    /// Always present; see the field docs.
    #[must_use]
    pub(crate) fn retention_account(
        &self,
    ) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.retention_account.get()
    }

    /// Inspect the assigned retention account in lifetime tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn retention_account_for_tests(
        &self,
    ) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.retention_account()
    }

    /// Test-only constructor binding a specific retention account, so a
    /// pressure fixture can exercise the aggregate refusal path without
    /// perturbing the process-local account every other test shares.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_account_for_test(
        retention_account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self::with_counters_and_account(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            verter_session_query::retention::StoreAccount::new(retention_account),
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counters_and_schema_version(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            schema_version,
            verter_session_query::retention::StoreAccount::default(),
        )
    }

    fn with_counters_and_schema_version(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
        schema_version: u32,
        retention_account: verter_session_query::retention::StoreAccount,
    ) -> Self {
        #[cfg(not(any(test, feature = "semantic-observe")))]
        let _ = stale_sweeps;
        Self {
            inner: BoundedCandidateMap::with_caps(
                Self::PER_SLOT_CANDIDATE_CAP,
                Self::GLOBAL_BUDGET,
            ),
            live_counter,
            #[cfg(any(test, feature = "semantic-observe"))]
            stale_sweeps,
            schema_version,
            retention_account,
        }
    }

    #[inline(always)]
    fn observe_removed(&self, count: usize) {
        #[cfg(any(test, feature = "semantic-observe"))]
        self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        #[cfg(not(any(test, feature = "semantic-observe")))]
        let _ = count;
    }

    /// Apply a net live-count delta to the external counter.
    ///
    /// `added` candidates entered the cache and `removed` candidates left
    /// it as part of one mutation. The counter is updated by atomic
    /// `fetch_add` / `fetch_sub` so concurrent mutations compose exactly
    /// — never by an absolute `store` of a re-derived snapshot, which a
    /// racing mutation could clobber. The subtract is saturated against
    /// the counter's current value because `live_counter` is shared via
    /// `Arc<AtomicU64>` across the `ProjectTypeStore` and an underflow
    /// would corrupt every sibling DB's contribution to the shared sum.
    fn apply_live_delta(&self, added: usize, removed: usize) {
        if added > removed {
            self.live_counter
                .fetch_add((added - removed) as u64, Ordering::Relaxed);
        } else if removed > added {
            let delta = (removed - added) as u64;
            self.live_counter.fetch_sub(
                delta.min(self.live_counter.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }
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
    #[cfg(any(test, feature = "test-support"))]
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
    ///
    /// Returns `false` when the aggregate retention account refused the
    /// entry's bytes: the freshly computed value is COMPLETE and is
    /// returned to its caller, the cache simply does not keep it. No
    /// stale candidate is substituted and no partial is fabricated — the
    /// slot is left exactly as it was.
    pub(crate) fn publish_core(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        entry: ComponentMetaResultEntry<P>,
    ) -> bool
    where
        P: verter_session_query::retention::RetainedFootprint,
    {
        let bytes = {
            use verter_session_query::retention::RetainedFootprint as _;
            entry.retained_footprint_bytes()
        };
        let charge = match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            bytes,
            &[&entry.read_set_signature.facts],
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                #[cfg(feature = "semantic-observe")]
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %key.owner_canonical,
                    refusal = %refusal,
                    "skipping component-meta cache promotion: aggregate retention refusal",
                );
                return false;
            }
        };
        let outcome = self.inner.admit(key, owner_whole_hash, entry, charge);
        if outcome.evicted > 0 {
            self.observe_removed(outcome.evicted);
        }
        // Net-delta the live counter: a fresh admission adds one live
        // candidate, an in-place replace adds none, and any FIFO
        // evictions remove that many. `fetch_add`/`fetch_sub` compose
        // exactly under concurrent admissions.
        self.apply_live_delta(usize::from(outcome.fresh), outcome.evicted);
        true
    }

    /// Test-support seed seam. Production admission must route through
    /// the selected engine `MemoPublish`, which owns tracing, finalization,
    /// and the admission decision before passive storage accepts the record.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        entry: ComponentMetaResultEntry<P>,
    ) where
        P: verter_session_query::retention::RetainedFootprint,
    {
        self.publish_core(key, owner_whole_hash, entry);
    }

    /// Remove the candidate for one owner content version. Returns the
    /// removed entry when present.
    #[cfg(any(test, feature = "test-support"))]
    pub fn remove(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<ComponentMetaResultEntry<P>> {
        let candidate = self.inner.get_candidate(key, &owner_whole_hash)?;
        let removed = self.inner.evict_candidate(key, candidate.seq);
        if removed {
            self.observe_removed(1);
            // One live candidate left the cache — net-subtract one.
            self.apply_live_delta(0, 1);
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
            self.observe_removed(removed);
        }
        // `clear` runs under the substrate's `retention_gate` write
        // guard, so `removed` is the exact live count it dropped —
        // net-subtract it. (Net-delta rather than `store(0)` keeps the
        // accounting uniform with the other mutation sites and robust
        // should the `component_meta_live` counter ever be shared.)
        self.apply_live_delta(0, removed);
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
            self.observe_removed(removed);
        }
        // Net-subtract exactly the candidates this invalidation removed.
        self.apply_live_delta(0, removed);
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
    /// schema-version eviction invariant. The caller supplies the slot
    /// `key` (so the zero-env placeholder lives in the test fixture, not
    /// in production source) and a payload, so generic-parameter Dbs
    /// (`ComponentMetaResultDb<P>`) can be exercised without binding the
    /// helper to a single payload type. The `read_set_signature` carrier
    /// is crate-private, so the entry is assembled here rather than in the
    /// integration-test fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test_with_payload(
        &self,
        key: ComponentMetaResultKey,
        payload: P,
    ) where
        P: verter_session_query::retention::RetainedFootprint,
    {
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(payload),
            read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
            validated_at_generation: 0,
        };
        self.insert(key, [0u8; 16], entry);
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
            self.observe_removed(count);
        }
        // `clear` ran under the write guard — `count` is the exact live
        // count it dropped. Net-subtract it.
        self.apply_live_delta(0, count);
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
