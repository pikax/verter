//! Host-owned typed DB wrappers for the 10 component-meta caches that
//! were previously authoritative inside `ComponentMetaQueryEngine`.
//!
//! ## Architecture
//!
//! Each cache is a typed `*Db` wrapper that routes its cold build through
//! one of the two cache-runtime node entry points, so the cooperative
//! singleflight protocol (one-winner cold build, cooperative joiner
//! waits, panic safety, post-compute revalidation) stays
//! cache-runtime-internal and no `*Db` names a cooperative primitive
//! directly. The wrappers split into two families by storage shape:
//!
//! - **Single-entry artifact caches** — a `DashMap<Key, Arc<CacheEntry>>`
//!   plus a per-cache `InflightTable<QueryFlightKey<Key>>`. Their
//!   `get_or_compute<F>(key, ctx, compute) -> Option<value>` builds a
//!   [`SingleEntryArtifactNode`] (an [`ArtifactNode`]) over the map /
//!   flight table / live counter and routes through
//!   [`crate::cache_runtime::node::lookup`]. The node's winner-only
//!   `post_publish` / `removal_cleanup` hooks keep the shared
//!   `component_meta_cache_live` counter in step with the live map.
//! - **Reverse-indexed multi-candidate query-identity caches** —
//!   `ImportedRegistryDb`, `MaterializeStructureDb`, `RefCycleResultDb`.
//!   Each wraps a shared
//!   [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore)
//!   plus a per-cache `InflightTable<QueryFlightKey<Key>>`, and its
//!   `get_or_compute_admit<F>(key, ctx, compute)` builds a
//!   [`QueryCandidateNode`] (a [`QueryNode`]) and routes through
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

use crate::cache_runtime::admission::{CacheAdmission, CacheEntry, NonAdmissionReason};
use crate::cache_runtime::node::{lookup, ArtifactNode, ComputeCtx, QueryFlightKey};
use crate::cache_runtime::singleflight::InflightTable;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{FactVersionRef, ResolvedTypeDeclaration, ResolverContext};
use crate::semantic_query::DepSignature;
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
// Each cache's `get_or_compute` builds a `SingleEntryArtifactNode` over
// its own `entries` / `inflight` / `live_counter` plus the per-call
// `compute` closure, then routes through
// [`crate::cache_runtime::node::lookup`]. The stored value is the domain
// value; every validity rail (the fact signature, the self-root
// canonicals, the compute-time generation) lives in
// [`crate::cache_runtime::CacheEntry`].

/// The three-way outcome a single-entry cache's per-call cold-build
/// closure reports.
///
/// It mirrors the runtime's own [`CacheAdmission`] vocabulary, minus the
/// bookkeeping the node owns (the self-root set and the compute-time
/// generation stamp). Modelling REFUSAL as a first-class arm — rather than
/// as a `None` — is what keeps refusal orthogonal to failure: a refused
/// admission still hands the freshly-computed value back to the winner, so
/// no producer has to re-run its resolution to recover the value it just
/// computed.
enum SingleEntryOutcome<V> {
    /// Valid AND cacheable: the value plus the path-precise fact signature
    /// the entry is validated by.
    Cacheable(V, Arc<[FactVersionRef]>),
    /// Valid but NOT cacheable: publish nothing, serve the value to the
    /// winner verbatim. Never a fabricated `Partial` — refusal is
    /// CACHE-ONLY.
    ReturnOnly(V, NonAdmissionReason),
    /// The cold build itself failed to produce a value.
    Failed,
}

/// Per-call [`ArtifactNode`] adapter for the single-entry caches.
///
/// Holds borrows of the owning cache's published map, flight table, and
/// live counter, plus the entry's self-root canonicals and the per-call
/// cold-build closure. The closure returns a [`SingleEntryOutcome`]: the
/// node stamps the compute-time generation and the self-root set onto a
/// `Cacheable` outcome and lowers the other two arms verbatim.
struct SingleEntryArtifactNode<'a, K, V, F>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: FnOnce() -> SingleEntryOutcome<V>,
{
    entries: &'a DashMap<K, Arc<CacheEntry<V>>>,
    inflight: &'a InflightTable<QueryFlightKey<K>>,
    live_counter: &'a AtomicU64,
    /// The canonicals the warm-read validator checks STRICTLY. Owned by
    /// the funnel (it derives them from the key), not by the per-call
    /// closure.
    self_root_canonicals: Arc<[Arc<str>]>,
    /// `FnOnce` carried in a `RefCell<Option<_>>` so the `&self`
    /// `compute` method (the `ArtifactNode` trait takes `&self`) can
    /// `take()` it exactly once on the cold winner's call.
    compute: std::cell::RefCell<Option<F>>,
}

impl<'a, K, V, F> ArtifactNode for SingleEntryArtifactNode<'a, K, V, F>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: FnOnce() -> SingleEntryOutcome<V>,
{
    type Key = K;
    type Value = V;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        self.inflight
    }

    fn compute(&self, _key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        let compute = self
            .compute
            .borrow_mut()
            .take()
            .expect("single-entry compute is taken exactly once by the cold winner");
        match compute() {
            SingleEntryOutcome::Cacheable(value, facts) => CacheAdmission::Cacheable {
                value,
                signature: ReadSetSignature::new(facts),
                self_root_canonicals: Arc::clone(&self.self_root_canonicals),
                validated_at_generation: cx.generation(),
            },
            SingleEntryOutcome::ReturnOnly(value, reason) => {
                CacheAdmission::ReturnOnly { value, reason }
            }
            SingleEntryOutcome::Failed => CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            },
        }
    }

    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        // Generation gate (the project-shape counterpart of the
        // file-content carrier check — a `ProjectGeneration` reset bumps
        // no file content) plus strict self-root fact validation. A
        // passing validation also bubbles the entry's facts into the
        // caller's outer tracer.
        if entry.validated_at_generation == cx.generation()
            && entry
                .signature
                .validate_with_self_roots(cx.resolver, &entry.self_root_canonicals)
        {
            entry.signature.bubble(cx.resolver);
            Some(entry.value.clone())
        } else {
            None
        }
    }

    fn post_publish(&self, _key: &Self::Key, _entry: &Arc<CacheEntry<Self::Value>>) {
        // Winner-only — fires after `entries.insert` AND a successful
        // post-compute revalidation, so the bump is paired with the
        // published map entry and is structurally unreachable on the
        // revalidation-fail path (no leak).
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    fn removal_cleanup(&self, _key: &Self::Key, _entry: &Arc<CacheEntry<Self::Value>>) {
        // Removal-side counterpart of `post_publish` — the substrate
        // fires this on the warm-hit reject path AND the joiner-fork
        // reject path, so the counter tracks live entries, not lifetime
        // inserts.
        self.live_counter.fetch_sub(1, Ordering::Relaxed);
    }
}

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

/// Lower a producer's cold-build result into a [`SingleEntryOutcome`] by
/// consulting the cacheability probe **after** the compute has run.
///
/// **The sampling point is load-bearing.** A verdict read at funnel ENTRY
/// covers only what the producer consumed BEFORE the funnel; the compute runs
/// LATER — inside the cold winner's closure — so a non-cacheable read taken
/// there lands after such a check and would be published as `Cacheable`. The
/// tracer accumulates monotonically, so a verdict read HERE, at the end of the
/// compute, covers everything the compute consumed, provided the scope
/// ENCLOSES the producer — which is exactly what an unforgeable
/// [`CacheabilityProbe`](crate::fact_signature_helpers::CacheabilityProbe)
/// guarantees.
///
/// A non-cacheable verdict routes the value through `ReturnOnly`: the write is
/// refused, the freshly-computed value is still handed back to the winner. The
/// value is never dropped, so no producer re-runs its resolution to recover
/// what it just computed, and the result stays `Complete` — refusal never
/// fabricates a `Partial`.
///
/// `UnresolvedProvenance` is the probe's refusal reason: a value derived from a
/// fenced serve, a broken decl-body lease, an unrootable import route, or an
/// unobservable contributor source env cannot be soundly rooted for warm
/// admission — its fact stamps read the LIVE view while its payload came from a
/// basis the rail cannot re-check. A producer that could not root its own value
/// ([`ComputedEntry::Unrooted`]) reports its own typed reason and is refused
/// regardless of the probe's verdict; either way the value survives.
///
/// # This is not the funnel's only post-compute verdict
///
/// A `Cacheable` outcome still faces the substrate's `revalidate_after_compute`
/// before it publishes. That gate fails when the store view MOVED under the
/// compute (a file it read was edited, or the project generation was reset,
/// between its first read and the publish), and there the funnel returns `None`
/// — the value is NOT handed to the winner, and the producer re-derives against
/// the fresh view.
///
/// The two refusals are opposites and must not be conflated. A cacheability
/// refusal means the value IS a consistent snapshot of the view it ran under
/// and merely cannot be rooted, so keeping it is honest. A revalidation
/// rejection means the value is a consistent snapshot of NO view — its reads
/// straddle the mutation — so serving it would hand the caller a torn result
/// and bubble the superseded facts into the enclosing entry's signature.
/// Discarding it is the completion fence's retry-on-mid-flight-change. Pinned
/// by `declaration_lookup_straddling_compute_is_not_served_to_the_winner`.
fn single_entry_admission<V>(
    probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
    computed: ComputedEntry<V>,
) -> SingleEntryOutcome<V> {
    match computed {
        ComputedEntry::Rooted(value, facts) => {
            if probe.non_cacheable() {
                SingleEntryOutcome::ReturnOnly(value, NonAdmissionReason::UnresolvedProvenance)
            } else {
                SingleEntryOutcome::Cacheable(value, facts)
            }
        }
        ComputedEntry::Unrooted(value, reason) => SingleEntryOutcome::ReturnOnly(value, reason),
        ComputedEntry::Failed => SingleEntryOutcome::Failed,
    }
}

/// Warm-read peek shared by the single-entry caches that expose a
/// `peek()` method: validate the entry's signature strictly against its
/// own self-roots plus the generation gate, bubbling on a hit.
fn single_entry_peek<K, V>(
    entries: &DashMap<K, Arc<CacheEntry<V>>>,
    key: &K,
    ctx: &dyn ResolverContext,
) -> Option<V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    let entry_arc = entries.get(key).map(|e| e.clone())?;
    if entry_arc.validated_at_generation == ctx.project_type_store().current_project_generation()
        && entry_arc
            .signature
            .validate_with_self_roots(ctx, &entry_arc.self_root_canonicals)
    {
        entry_arc.signature.bubble(ctx);
        Some(entry_arc.value.clone())
    } else {
        None
    }
}

/// Per-call [`QueryNode`] adapter for the reverse-indexed multi-candidate
/// caches (`ImportedRegistryDb`, `MaterializeStructureDb`,
/// `RefCycleResultDb`).
///
/// Mirrors [`SingleEntryArtifactNode`] for the query-identity family:
/// holds a borrow of the owning cache's
/// [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore)
/// plus the per-call cold-build closure, and routes the cold build through
/// [`crate::cache_runtime::node::query::lookup`] — so the cooperative
/// primitive stays cache-runtime-internal and no consumer names it
/// directly. The closure returns a node-level
/// [`CacheAdmission`](crate::cache_runtime::admission::CacheAdmission) the
/// producer already built (the producer keeps ownership of its
/// `install_fact_tracer` / fact-merge logic). `publish_core` /
/// `evict_deferred` / `publish_fence` / `lookup_candidate` delegate to the
/// store, so the split publish lifecycle (counter + reverse index + budget
/// admission under the slot guard, then deferred FIFO eviction under the
/// fence) is the store's.
struct QueryCandidateNode<'a, K, V, F>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: FnOnce() -> CacheAdmission<V>,
{
    store: &'a crate::cache_runtime::ReverseIndexedCandidateStore<K, V>,
    inflight: &'a InflightTable<QueryFlightKey<K>>,
    ctx: &'a dyn ResolverContext,
    /// `FnOnce` carried in a `RefCell<Option<_>>` so the `&self` `compute`
    /// method can `take()` it exactly once on the cold winner's call.
    compute: std::cell::RefCell<Option<F>>,
    /// Winner-side lowering for an admission-REFUSED computed value (see
    /// [`crate::cache_runtime::QueryNode::lower_unadmitted`]). `Some`
    /// opts the cache in: the winner returns the COMPUTED value lowered
    /// to its non-cacheable form instead of `None`. `None` keeps the
    /// substrate's failure semantics for this cache.
    unadmitted: Option<fn(&V) -> V>,
}

impl<'a, K, V, F> crate::cache_runtime::QueryNode for QueryCandidateNode<'a, K, V, F>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: FnOnce() -> CacheAdmission<V>,
{
    type Key = K;
    type Discriminant = crate::cache_runtime::FactCandidateDiscriminant;
    type Value = V;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        self.inflight
    }

    fn lookup_candidate(
        &self,
        key: &Self::Key,
        cx: &crate::cache_runtime::ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        let generation = cx.generation();
        self.store.lookup(key, |candidate| {
            // Validate against the CANDIDATE's OWN strict self-root set
            // (the producer stamped it at admission) plus the
            // project-generation gate. A stale candidate is skipped (the
            // store keeps it for other views).
            if candidate.validated_at_generation == generation
                && candidate
                    .signature
                    .validate_with_self_roots(self.ctx, &candidate.self_root_canonicals)
            {
                candidate.signature.bubble(self.ctx);
                Some(candidate.value.clone())
            } else {
                None
            }
        })
    }

    fn compute(
        &self,
        _key: &Self::Key,
        _cx: &mut crate::cache_runtime::ComputeCtx<'_>,
    ) -> CacheAdmission<Self::Value> {
        let compute = self
            .compute
            .borrow_mut()
            .take()
            .expect("query-candidate compute is taken exactly once by the cold winner");
        compute()
    }

    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        // The discriminant's generation is the candidate's OWN stamped
        // generation (the producer's in-compute snapshot threaded straight
        // from the `Cacheable` arm), so a same-view re-publish replaces in
        // place rather than coexisting as a duplicate. Reading
        // `cx.generation()` here instead would use the runtime's
        // lookup-entry snapshot and skew under a mid-compute generation
        // bump.
        crate::cache_runtime::FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }

    fn publish_fence(&self) -> Option<&parking_lot::RwLock<()>> {
        // The budgeted stores expose a retention gate; the unbudgeted
        // imported-registry store also exposes one (a no-op fence with no
        // deferred victims, but it keeps the lifecycle uniform).
        Some(self.store.retention_gate())
    }

    fn publish_core(
        &self,
        key: Self::Key,
        candidate: crate::cache_runtime::Candidate<Self::Discriminant, Self::Value>,
    ) -> crate::cache_runtime::PublishCoreOutcome<Self::Key> {
        self.store.publish_core(key, candidate)
    }

    fn evict_deferred(&self, victims: crate::cache_runtime::DeferredVictims<Self::Key>) {
        self.store.evict_deferred(victims);
    }

    fn lower_unadmitted(&self, value: &Self::Value) -> Option<Self::Value> {
        self.unadmitted.map(|lower| lower(value))
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
    /// forever. Every read-side gate ([`ImportedRegistryDb::peek`], the
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

    /// Peek-only lookup: returns the first cached candidate whose
    /// `read_set_signature` is still valid against `ctx`.
    ///
    /// This is the warm-hit half of [`Self::get_or_compute_admit`]
    /// exposed for the producer's compute-once shape: the producer peeks
    /// here first, and on a miss computes the imported-registry value
    /// **once** (the wildcard-route fuse is a side-effecting budget — it
    /// must be consumed at most once per request) before using
    /// `get_or_compute_admit` purely as a signature-building write-through.
    /// The keyed canonical is the candidate's self-root, validated
    /// strictly — a same-canonical content edit, or a keyed canonical
    /// untracked by the live store view, rejects the candidate, exactly
    /// matching the `get_or_compute_admit` warm-hit `lookup` arm.
    pub(crate) fn peek(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
    ) -> Option<ImportedRegistryValue> {
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let generation = ctx.project_type_store().current_project_generation();
        self.store.lookup(key, |candidate| {
            // The carrier validates only file-content whole-hashes; a
            // `ProjectGeneration` reset bumps no file content, so the
            // generation gate is the project-shape counterpart of the
            // carrier check. A stale-by-project-generation candidate is
            // rejected even though its signature still validates.
            if candidate.validated_at_generation == generation
                && candidate
                    .signature
                    .validate_with_self_roots(ctx, &self_roots)
            {
                candidate.signature.bubble(ctx);
                Some(candidate.value.clone())
            } else {
                None
            }
        })
    }

    /// Cooperative-admission cold compute over the imported-registry
    /// cache, routed through the query-identity split-publish lifecycle
    /// adapter over the shared
    /// [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore).
    ///
    /// The producer's `compute` closure runs the expensive,
    /// fuse-consuming `resolve_imported_registry_symbol_with_budget`
    /// resolution INSIDE the per-flight-lane singleflight slot: when
    /// several requests miss the same key concurrently under one view,
    /// exactly ONE winner runs `compute` and every joiner re-reads the
    /// store via the warm-hit lookup. Running the resolution here — rather
    /// than before the admission call — is what makes the wildcard-route
    /// fuse a one-winner cost instead of an N-waiter cost.
    ///
    /// `compute` returns a [`ComputeAdmission`](crate::cache_runtime::singleflight::ComputeAdmission)
    /// over an [`ImportedRegistryEntry`]:
    ///
    /// - `Cacheable(entry)` — the provenance-pure fact signature built;
    ///   the candidate is admitted into the store (counter bump +
    ///   reverse-index registration under the slot guard), joiners re-read
    ///   the published candidate.
    /// - `ReturnOnly { value, reason }` — the resolution produced a valid
    ///   value but shared-cache admission is refused (the signature builder
    ///   could not build, or the test refusal hook fired). Nothing is admitted,
    ///   joiners fork and recompute, and the next cold miss recomputes.
    ///   The reason is preserved for refusal telemetry. The resolution is
    ///   NOT re-run and the fuse is NOT consumed twice.
    /// - `Failed` — the resolution itself failed; joiners surface `None`
    ///   and the next caller retries.
    ///
    /// A `Cacheable` admission can still be REJECTED after the fact by the
    /// substrate's `revalidate_after_compute` — the store view moved under the
    /// compute. That rejection returns `None` (the straddling value is
    /// deliberately discarded, never served) and the producer re-derives against
    /// the fresh view; it is the one path on which the winner resolves twice.
    /// See [`single_entry_admission`] for why the two refusals differ.
    ///
    /// The store carries NO retention budget, so the publish lifecycle has
    /// no deferred budget victims and no publish fence — the per-slot
    /// candidate cap (handled inside `publish_core` under the slot guard)
    /// plus the per-canonical reverse-index drain are the reclamation
    /// paths. The split lifecycle still closes the
    /// install-before-registration race: `publish_core` installs the
    /// candidate, bumps the counter, and registers the reverse index
    /// together under the slot guard, so no concurrent remover can observe
    /// a candidate before its counter / index registration exists.
    pub(crate) fn get_or_compute_admit<P, Prepare, F>(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
        prepare: Prepare,
        compute: F,
    ) -> Option<ImportedRegistryValue>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(
            P,
        ) -> crate::cache_runtime::singleflight::ComputeAdmission<
            ImportedRegistryValue,
            ImportedRegistryEntry,
        >,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |probe| {
                let prepared = prepare();
                self.get_or_compute_admit_in_scope(key, ctx, probe, || compute(prepared))
            },
        )
        .0
    }

    fn get_or_compute_admit_in_scope<F>(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        compute: F,
    ) -> Option<ImportedRegistryValue>
    where
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            ImportedRegistryValue,
            ImportedRegistryEntry,
        >,
    {
        // The keyed canonical is the candidate's self-root — validated
        // strictly on warm read (same-canonical edit / untracked keyed
        // canonical → miss).
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        // Unpack the producer's domain `ComputeAdmission<V, Entry>` into a
        // node-level `CacheAdmission<V>` — the cold-build closure the
        // `QueryCandidateNode` adapter runs. The producer keeps its
        // fuse-consuming resolution; this only re-shapes the carrier and
        // stamps the keyed canonical's self-root set.
        let node_compute = move || -> CacheAdmission<ImportedRegistryValue> {
            match compute() {
                // POST-compute cacheability gate. `compute()` runs the whole
                // route walk HERE, inside the flight, so this — not the funnel
                // entry — is the only sampling point that covers it. The
                // resolved value still reaches the winner through `ReturnOnly`;
                // the candidate is not admitted. See `single_entry_admission`
                // for the full rationale.
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(entry)
                    if probe.non_cacheable() =>
                {
                    CacheAdmission::ReturnOnly {
                        value: entry.value,
                        reason: NonAdmissionReason::UnresolvedProvenance,
                    }
                }
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(entry) => {
                    CacheAdmission::Cacheable {
                        value: entry.value,
                        signature: crate::fact_signature_helpers::ReadSetSignature::new(
                            Arc::clone(&entry.fact_dep_signature),
                        ),
                        self_root_canonicals: self_roots,
                        validated_at_generation: entry.validated_at_generation,
                    }
                }
                crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                    value,
                    reason,
                } => CacheAdmission::ReturnOnly { value, reason },
                crate::cache_runtime::singleflight::ComputeAdmission::Failed => {
                    CacheAdmission::Failed {
                        reason: NonAdmissionReason::ComputeFailed,
                    }
                }
            }
        };
        let node = QueryCandidateNode {
            store: &self.store,
            inflight: &self.inflight,
            ctx,
            compute: std::cell::RefCell::new(Some(node_compute)),
            unadmitted: None,
        };
        crate::cache_runtime::query::lookup(&node, key.clone(), ctx)
    }

    /// Test-only: drive [`Self::get_or_compute`] the way a production producer
    /// does — inside a REAL cacheability tracer scope opened around the whole
    /// compute.
    ///
    /// A [`crate::fact_signature_helpers::CacheabilityProbe`] cannot be forged;
    /// `with_cacheability_scope` is its only constructor. So this helper is not an
    /// escape hatch around the admission contract — it IS the contract, spelled for
    /// a test that has no surrounding producer. A test whose compute consumes a
    /// non-cacheable read is refused admission here exactly as production is.
    #[cfg(test)]
    pub(crate) fn get_or_compute_admit_traced_for_test<F>(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<ImportedRegistryValue>
    where
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            ImportedRegistryValue,
            ImportedRegistryEntry,
        >,
    {
        self.get_or_compute_admit(key, ctx, || (), |()| compute())
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

    /// Test-only strong-count probe of the in-flight slot currently
    /// registered for `key`, regardless of which store-view compat token
    /// keys the flight lane. Drives the deterministic singleflight
    /// rendezvous discriminator: a worker is a confirmed cooperative
    /// joiner once it has cloned its own `Arc` to the winner's
    /// in-flight slot, observable as a step up in this count.
    ///
    /// The local cache substrate keys the inflight table by
    /// [`QueryFlightKey<ImportedRegistryKey>`] (bare key + compat
    /// token), but the test only knows the bare key — and every
    /// contending worker in the rendezvous runs under the SAME store
    /// view, so exactly one flight lane is registered. This accessor
    /// scans the inflight table for the slot whose inner key matches
    /// `key` and returns the `Arc`'s strong count; on no registered
    /// slot, returns `None`. Reading through the table's lock does not
    /// itself bump the count.
    #[cfg(test)]
    pub(crate) fn slot_strong_count_for_test(&self, key: &ImportedRegistryKey) -> Option<usize> {
        self.inflight.slot_strong_count_by_inner_key(key)
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
        let signature = crate::fact_signature_helpers::ReadSetSignature::new(Arc::clone(
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

    /// The SOLE admission funnel for `DeclarationLookupDb`.
    ///
    /// The DB opens the cacheability scope itself around the cold closure. The
    /// verdict is consulted AFTER `compute` returns
    /// ([`single_entry_admission`]), so a non-cacheable read taken INSIDE the
    /// cold build refuses the write; the computed declaration is still returned
    /// to the winner through `ReturnOnly`.
    ///
    /// The closure reports a [`ComputedEntry`], not an `Option`: a declaration
    /// this cache cannot ROOT is still a declaration the caller asked for. It
    /// rides `ReturnOnly` back to the winner, so a CACHEABILITY refusal never
    /// costs the second resolution a `None` would have forced.
    ///
    /// `None` therefore means one of exactly two things: the cold build produced
    /// nothing ([`ComputedEntry::Failed`]), or the substrate's post-compute
    /// revalidation rejected the entry because the store view moved under the
    /// compute — a straddling value that must be discarded and re-derived, never
    /// served. See [`single_entry_admission`].
    pub(crate) fn get_or_compute<P, Prepare, F>(
        &self,
        key: &DeclarationLookupKey,
        ctx: &dyn ResolverContext,
        prepare: Prepare,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |probe| {
                let prepared = prepare();
                self.get_or_compute_in_scope(key, ctx, probe, || compute(prepared))
            },
        )
        .0
    }

    fn get_or_compute_in_scope<F>(
        &self,
        key: &DeclarationLookupKey,
        ctx: &dyn ResolverContext,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        // The entry's keyed canonical is its self-root: the warm-read
        // validator validates the self-root `FileWholeHash` strictly so
        // a same-canonical content edit (or a keyed canonical that
        // became untracked) rejects the entry instead of riding the
        // lazy "untracked → accept" rule.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: &self.entries,
            inflight: &self.inflight,
            live_counter: &self.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                let computed = match compute() {
                    ComputedEntry::Rooted(value, facts) => {
                        ComputedEntry::Rooted(Arc::new(value), facts)
                    }
                    ComputedEntry::Unrooted(value, reason) => {
                        ComputedEntry::Unrooted(Arc::new(value), reason)
                    }
                    ComputedEntry::Failed => ComputedEntry::Failed,
                };
                single_entry_admission(probe, computed)
            })),
        };
        lookup(&node, key.clone(), ctx)
    }

    /// Test-only: drive [`Self::get_or_compute`] the way a production producer
    /// does — inside a REAL cacheability tracer scope opened around the whole
    /// compute.
    ///
    /// A [`crate::fact_signature_helpers::CacheabilityProbe`] cannot be forged;
    /// `with_cacheability_scope` is its only constructor. So this helper is not an
    /// escape hatch around the admission contract — it IS the contract, spelled for
    /// a test that has no surrounding producer. A test whose compute consumes a
    /// non-cacheable read is refused admission here exactly as production is.
    #[cfg(test)]
    pub(crate) fn get_or_compute_traced_for_test<F>(
        &self,
        key: &DeclarationLookupKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        self.get_or_compute(key, ctx, || (), |()| compute())
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

    /// The SOLE admission funnel for `ResolvabilityDb`.
    ///
    /// The DB opens the cacheability scope itself around the cold closure. The
    /// verdict is consulted AFTER `compute` returns
    /// ([`single_entry_admission`]), so a non-cacheable read taken INSIDE the
    /// cold build refuses the write; the computed bool is still returned to the
    /// winner through `ReturnOnly`.
    ///
    /// The closure reports a [`ComputedEntry`]: a bool this cache cannot ROOT
    /// (an overflowed signature, an unobservable keyed content version, a
    /// request-partial resolution) still rides `ReturnOnly` back to the winner,
    /// so the caller never re-derives a verdict it already has.
    pub(crate) fn get_or_compute<P, Prepare, F>(
        &self,
        key: &ResolvabilityKey,
        ctx: &dyn ResolverContext,
        prepare: Prepare,
        compute: F,
    ) -> Option<bool>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<bool>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |probe| {
                let prepared = prepare();
                self.get_or_compute_in_scope(key, ctx, probe, || compute(prepared))
            },
        )
        .0
    }

    fn get_or_compute_in_scope<F>(
        &self,
        key: &ResolvabilityKey,
        ctx: &dyn ResolverContext,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> ComputedEntry<bool>,
    {
        // The keyed source canonical is the entry's self-root — strict
        // warm-read validation rejects a same-canonical edit or an
        // untracked keyed canonical.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: &self.entries,
            inflight: &self.inflight,
            live_counter: &self.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                single_entry_admission(probe, compute())
            })),
        };
        lookup(&node, key.clone(), ctx)
    }

    /// Test-only: drive [`Self::get_or_compute`] the way a production producer
    /// does — inside a REAL cacheability tracer scope opened around the whole
    /// compute.
    ///
    /// A [`crate::fact_signature_helpers::CacheabilityProbe`] cannot be forged;
    /// `with_cacheability_scope` is its only constructor. So this helper is not an
    /// escape hatch around the admission contract — it IS the contract, spelled for
    /// a test that has no surrounding producer. A test whose compute consumes a
    /// non-cacheable read is refused admission here exactly as production is.
    #[cfg(test)]
    pub(crate) fn get_or_compute_traced_for_test<F>(
        &self,
        key: &ResolvabilityKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> ComputedEntry<bool>,
    {
        self.get_or_compute(key, ctx, || (), |()| compute())
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

    /// The SOLE admission funnel for `OwnerCollectionDb`.
    ///
    /// The DB opens the cacheability scope itself around the cold closure. The
    /// verdict is consulted AFTER `compute` returns
    /// ([`single_entry_admission`]); the computed locator is still returned to
    /// the winner through `ReturnOnly`.
    ///
    /// The reason this funnel needs the probe is CONTENT-NEUTRAL and worth
    /// stating plainly: the value is built from a prepared-decl read, and a
    /// BROKEN DECL-BODY LEASE (`LeaseMiss`) makes that read yield a degraded
    /// `None` WITHOUT superseding the artifact or moving the owner's content
    /// hash. The entry would therefore root on the LIVE hash and validate on
    /// every warm read forever — permanently shadowing a recoverable
    /// declaration as a proven absence. `PreparedDeclBundle::get` leaves its
    /// write-once slot VACANT on a lease miss for exactly this reason; this
    /// funnel must not undo that by publishing the `None` one level up.
    ///
    /// The closure reports a [`ComputedEntry`], whose `Failed` arm means "no
    /// prepared-decl observation at all" — distinct from `Unrooted`, which is a
    /// locator the cache may serve but must not publish.
    pub(crate) fn get_or_compute<P, Prepare, F>(
        &self,
        key: &OwnerCollectionKey,
        ctx: &dyn ResolverContext,
        prepare: Prepare,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |probe| {
                let prepared = prepare();
                self.get_or_compute_in_scope(key, ctx, probe, || compute(prepared))
            },
        )
        .0
    }

    fn get_or_compute_in_scope<F>(
        &self,
        key: &OwnerCollectionKey,
        ctx: &dyn ResolverContext,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        F: FnOnce() -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        // The owner canonical is the entry's self-root. The locator is
        // content-free, but the position it addresses is only meaningful
        // against the owner content version the producer observed, so
        // strict self-root validation remains the correctness floor — a
        // content edit to the owner file invalidates the cached locator.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: &self.entries,
            inflight: &self.inflight,
            live_counter: &self.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                single_entry_admission(probe, compute())
            })),
        };
        lookup(&node, key.clone(), ctx)
    }

    /// Test-only: drive [`Self::get_or_compute`] the way a production producer
    /// does — inside a REAL cacheability tracer scope opened around the whole
    /// compute.
    ///
    /// A [`crate::fact_signature_helpers::CacheabilityProbe`] cannot be forged;
    /// `with_cacheability_scope` is its only constructor. So this helper is not an
    /// escape hatch around the admission contract — it IS the contract, spelled for
    /// a test that has no surrounding producer. A test whose compute consumes a
    /// non-cacheable read is refused admission here exactly as production is.
    #[cfg(test)]
    pub(crate) fn get_or_compute_traced_for_test<F>(
        &self,
        key: &OwnerCollectionKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        F: FnOnce() -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        self.get_or_compute(key, ctx, || (), |()| compute())
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

/// Sealed capability for one Shape-cache owner scope. Only
/// [`ShapeCacheDb::with_owner_scope`] can construct it, and its tracer borrow
/// cannot escape the callback. All production Shape-cache writes require this
/// capability, so key classification, gates, and the complete cold compute can
/// share one DB-owned scope.
pub(crate) struct ShapeCacheOwnerScope<'db, 't> {
    db: &'db ShapeCacheDb,
    ctx: &'db dyn ResolverContext,
    probe: &'t crate::fact_signature_helpers::CacheabilityProbe<'t>,
}

impl ShapeCacheOwnerScope<'_, '_> {
    #[must_use]
    pub(crate) fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        self.db.peek(key, self.ctx)
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ShapeCacheKey,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        self.db
            .get_or_compute_in_scope(key, self.ctx, self.probe, compute)
    }

    #[must_use]
    pub(crate) fn non_cacheable(&self) -> bool {
        self.probe.non_cacheable()
    }

    pub(crate) fn admit_computed(
        &self,
        key: &ShapeCacheKey,
        value: MaterializedOutputTypeExpr,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) -> MaterializedOutputTypeExpr {
        let value_for_closure = value.clone();
        self.get_or_compute(key, move || Some((value_for_closure, fact_dep_signature)))
            .unwrap_or(value)
    }
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

    /// Open the sole production admission scope for this DB. The sealed
    /// capability passed to `f` is the only route to a Shape-cache write.
    pub(crate) fn with_owner_scope<'db, F, R>(&'db self, ctx: &'db dyn ResolverContext, f: F) -> R
    where
        F: for<'t> FnOnce(ShapeCacheOwnerScope<'db, 't>) -> R,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |probe| {
                f(ShapeCacheOwnerScope {
                    db: self,
                    ctx,
                    probe,
                })
            },
        )
        .0
    }

    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<MaterializedOutputTypeExpr> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation rejects a same-scope content edit.
        // The entry carries its own self-roots, validated strictly.
        let result = single_entry_peek(&self.entries, key, ctx);
        if let Some(rctx) = crate::request_context::current_request_context() {
            // Every ShapeCacheDb subject is a member-shape-route identity
            // now that the TypeExpr-START route also keys its LOWERED
            // settled node (`MemberValueNode`) — so all peeks count into
            // the member-shape layer. The former `materialize_memo`
            // per-request layer stays as a field on the audit
            // `CacheLayerBreakdown` (additive wire rule) and reads zero.
            let counter = match &key.subject {
                ShapeSubject::MemberValueNode { .. } | ShapeSubject::SyntheticBinding { .. } => {
                    &rctx.cache_counters.member_shape_cache
                }
            };
            if result.is_some() {
                counter.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                counter.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// The SOLE admission funnel for every `ShapeCacheDb` subject —
    /// [`Self::admit_computed`] delegates here, so this is the one place a
    /// shape can enter the shared cache.
    ///
    /// `probe` is the [`crate::fact_signature_helpers::CacheabilityProbe`] of
    /// the cacheability tracer scope enclosing the producer's compute. It is
    /// REQUIRED, not optional: the token can be minted only by
    /// `fact_signature_helpers::with_cacheability_scope`, so a producer that
    /// runs its compute with NO tracer cannot reach this function at all, and a
    /// producer that HAS a scope cannot forget to consult its verdict — the
    /// funnel consults it. That closes both shapes of the laundering class by
    /// construction (an untraced producer, and a traced producer that drops the
    /// verdict on the floor).
    ///
    /// A non-cacheable verdict (a fenced serve / lease miss / unrootable route /
    /// unobservable source env consumed anywhere in the compute, or a
    /// fact-signature overflow) publishes NOTHING: the value is returned to the
    /// winner through `ReturnOnly`. Refusal is CACHE-ONLY — the value stays
    /// `Complete`, never a fabricated `Partial`.
    ///
    /// The verdict is sampled AFTER `compute()` returns, inside the cold
    /// winner's closure. That is the load-bearing sampling point: `compute()`
    /// runs INSIDE the flight, so a check at funnel entry alone would sit
    /// BEFORE every read the compute performs, and a fenced serve consumed
    /// there would sail into a `Cacheable` admission. See
    /// [`single_entry_admission`].
    fn get_or_compute_in_scope<F>(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation rejects a same-scope content edit.
        let self_roots: Arc<[Arc<str>]> =
            Arc::from(vec![Arc::clone(key.subject.scope_canonical())]);
        let node = SingleEntryArtifactNode {
            entries: &self.entries,
            inflight: &self.inflight,
            live_counter: &self.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                // Central partial gate, folded through the SAME `ReturnOnly`
                // arm as the cacheability refusal. The gate is PURE over the
                // value's OWN `result_is_partial` — a computed shape that is
                // itself a GENUINE partial must NOT be admitted (a warm replay
                // would serve the partial as a complete shape). It does NOT
                // OR-in any request-global partial sticky. Both refusals keep
                // the value: `ReturnOnly` hands it back to the winner, so
                // neither needs a side-channel cell to survive the flight.
                match single_entry_admission(probe, compute().into()) {
                    SingleEntryOutcome::Cacheable(value, _facts)
                        if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                            value.result_is_partial(),
                        ) =>
                    {
                        SingleEntryOutcome::ReturnOnly(value, NonAdmissionReason::PartialResult)
                    }
                    outcome => outcome,
                }
            })),
        };
        lookup(&node, key.clone(), ctx)
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
    ///
    /// Central partial gate: this delegates to [`Self::get_or_compute`],
    /// whose `refuse_result_cache_admission_if_partial` gate is PURE over
    /// the value's OWN `result_is_partial` and refuses to admit a GENUINE
    /// partial. The gate does NOT OR-in any request-global partial sticky.
    /// On refusal the value is returned verbatim and `peek` continues to
    /// miss.
    /// Test-only: drive [`Self::get_or_compute`] the way a production producer
    /// does — inside a REAL cacheability tracer scope opened around the whole
    /// compute.
    ///
    /// A [`crate::fact_signature_helpers::CacheabilityProbe`] cannot be forged;
    /// `with_cacheability_scope` is its only constructor. So this helper is not an
    /// escape hatch around the admission contract — it is the contract, spelled for
    /// a test that has no surrounding producer. A test whose compute consumes a
    /// fenced serve is refused admission here exactly as production is.
    #[cfg(test)]
    pub(crate) fn get_or_compute_traced_for_test<F>(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        self.with_owner_scope(ctx, |scope| scope.get_or_compute(key, compute))
    }

    /// Test-only sibling of [`Self::get_or_compute_traced_for_test`] for
    /// [`Self::admit_computed`].
    #[cfg(test)]
    pub(crate) fn admit_computed_traced_for_test(
        &self,
        key: &ShapeCacheKey,
        ctx: &dyn ResolverContext,
        value: MaterializedOutputTypeExpr,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) -> MaterializedOutputTypeExpr {
        self.with_owner_scope(ctx, |scope| {
            scope.admit_computed(key, value, fact_dep_signature)
        })
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
                TypeExpr::Unknown { raw: String::new() },
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

use crate::component_meta_materialize::{MaterializationCacheKey, MaterializeOutcome};

/// Entry stored in `MaterializeStructureDb`. Carries the
/// cacheable `MaterializeOutcome` (`Value` or `Miss` only — `Recursive`
/// and `Tainted` are non-cacheable per-call sentinels), the
/// observed-root carrier, and the explicit self-root canonical set.
///
/// The carrier holds the path-precise fact signature observed during
/// the cold build (the materialiser's traced read set) — the sole
/// cache-validity rail. `self_root_canonicals` lists the canonicals
/// whose `FileWholeHash` fact the warm-read validator must check
/// **strictly** — the materialise SUBJECT's declaration-origin file:
/// the EXTRACTED ROUTE ROOT for a route-shaped subject (`Pick`/`Omit`/
/// IndexedAccess), the `base` node's origin for a non-route subject.
/// The consumer materialise scope is NEVER a self-root: a value's
/// identity does not depend on which consumer reached it (R7 cross-owner
/// reuse). A non-route `Global`-origin base (or a route-shaped subject
/// whose extracted root has no authoritative content hash) yields an
/// empty `self_root_canonicals`. A content edit to a self-root
/// canonical, or a self-root canonical the live store view no longer
/// tracks, rejects the entry through
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
    /// read and post-compute revalidation: the materialise SUBJECT's
    /// declaration-origin file — the extracted route root for a
    /// route-shaped subject, the `base` node's origin for a non-route
    /// subject (empty for a non-route `Global`-origin base, or a
    /// route-shaped subject whose extracted root has no authoritative
    /// content hash). The consumer materialise scope is NEVER a
    /// self-root (R7 cross-owner reuse). An untracked or hash-mismatched
    /// self-root rejects the entry.
    pub self_root_canonicals: Arc<[Arc<str>]>,
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

/// Final-result cache for the structural materialiser.
///
/// Routes through the shared
/// [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore):
/// the per-canonical reverse index enables O(K) invalidation cleanup, and
/// the embedded FIFO retention budget is the routine memory-reclamation
/// path. The cache key is the content-free
/// [`MaterializationCacheKey`](crate::component_meta_materialize::MaterializationCacheKey)
/// (canonical-subject slot + projection axes + resolve_env_hash — NO
/// content/version hash), so concurrent content versions of one subject
/// co-locate as candidates in ONE slot (R20), each rooted value-side by
/// its `ReadSetSignature.facts` + `self_root_canonicals` +
/// `validated_at_generation`; the budget FIFO-evicts the oldest past
/// [`Self::MAX_ENTRIES`] so a long-lived session does not accumulate
/// stale per-version candidates unbounded. Concurrent base/overlay
/// variants of one content-free key coexist as candidates (R20).
pub struct MaterializeStructureDb {
    /// The shared reverse-indexed multi-candidate store (budgeted form).
    /// Owns the slots, the per-canonical reverse index, the FIFO retention
    /// budget, the shared live counter, and the retention gate (the
    /// publish fence).
    store: crate::cache_runtime::ReverseIndexedCandidateStore<
        MaterializationCacheKey,
        crate::semantic_query::CacheRead<MaterializeOutcome>,
    >,
    /// Per-cache flight table keyed by the flight identity (cache key +
    /// store-view compat token) so two overlays on one key do not coalesce.
    inflight: InflightTable<QueryFlightKey<MaterializationCacheKey>>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
}

impl MaterializeStructureDb {
    /// Total candidate-count cap. A long-lived editor session that
    /// re-materialises many owner versions caps here; the oldest
    /// candidates are FIFO-evicted via the store's deferred-eviction path.
    pub const MAX_ENTRIES: usize = 2048;

    /// Construct a fresh cache.
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::with_counter_schema_and_budget(
            live_counter,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
            Self::MAX_ENTRIES,
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counter_schema_and_budget(
            Arc::new(AtomicU64::new(0)),
            schema_version,
            Self::MAX_ENTRIES,
        )
    }

    fn with_counter_schema_and_budget(
        live_counter: Arc<AtomicU64>,
        schema_version: u32,
        budget_cap: usize,
    ) -> Self {
        Self {
            store: crate::cache_runtime::ReverseIndexedCandidateStore::with_counter_and_budget(
                live_counter,
                budget_cap,
            ),
            inflight: InflightTable::new(),
            schema_version,
        }
    }

    /// Configured total candidate-count cap.
    #[must_use]
    pub fn retention_cap(&self) -> usize {
        self.store.retention_cap()
    }

    /// Read-only strict-validation peek — never mutates the store.
    ///
    /// A candidate is returned only when its carrier validates strictly
    /// against the live store view AND its `validated_at_generation` still
    /// equals the live project generation. A stale candidate is SKIPPED,
    /// not reaped — the store keeps it for other views, and routine
    /// reclamation is the FIFO retention budget + the per-canonical drain.
    /// The shared `live_counter` therefore tracks live candidates without
    /// any read-path decrement.
    pub(crate) fn peek(
        &self,
        key: &MaterializationCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<MaterializeOutcome>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // Warm-hit read-side validation through the store. The candidate's
        // self-root canonicals (ONLY the materialise SUBJECT's
        // declaration-origin file — the `base` node's origin for a
        // non-route subject, or the EXTRACTED ROOT's declaration file for
        // a route-shaped subject; NEVER the consumer materialise scope, R7
        // cross-owner reuse) validate **strictly**; every other fact keeps
        // the lazy cross-file permissiveness. The generation gate is the
        // project-shape counterpart of the carrier check (a
        // `ProjectGeneration` reset bumps no file content). A stale
        // candidate never bubbles. The store skips a stale candidate on
        // read (the slot keeps it for other views); routine reclamation is
        // the FIFO budget + per-canonical drain.
        let generation = ctx.project_type_store().current_project_generation();
        let result = self.store.lookup(key, |candidate| {
            if candidate.validated_at_generation == generation
                && candidate
                    .signature
                    .validate_with_self_roots(ctx, &candidate.self_root_canonicals)
            {
                candidate.signature.bubble(ctx);
                crate::host_manage::record_materialize_structure_cache_hit();
                Some(candidate.value.clone())
            } else {
                None
            }
        });
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

    /// Cooperative-admission cold compute over the materialise-structure
    /// cache, routed through the query-identity split-publish lifecycle
    /// adapter over the shared store. The producer supplies only the cold
    /// materialisation closure. The DB invokes it through
    /// [`crate::component_meta_materialize::trace_materialize_compute`], so
    /// the owner controls the completeness scope, fact tracer, finalisation,
    /// and typed admission decision before the storage write.
    ///
    /// The store is budgeted, so the publish lifecycle threads the store's
    /// `retention_gate` as the publish fence and the FIFO retention
    /// victims through `evict_deferred`: the fence read guard spans
    /// revalidation → publish_core (counter bump + reverse-index
    /// registration + retention-admission record under the slot guard) →
    /// evict_deferred (the guard-free FIFO victim eviction), so a
    /// project-generation `clear` cannot interleave and the re-entrant
    /// eviction cannot self-deadlock.
    pub(crate) fn get_or_compute_admit<F>(
        &self,
        key: &MaterializationCacheKey,
        ctx: &dyn ResolverContext,
        compute: F,
    ) -> Option<crate::semantic_query::CacheRead<MaterializeOutcome>>
    where
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            crate::semantic_query::CacheRead<MaterializeOutcome>,
            MaterializeStructureEntry,
        >,
    {
        // Unpack the producer's domain `ComputeAdmission<V, Entry>` into a
        // node-level `CacheAdmission<V>`. `outcome -> value.value`,
        // `dispatch_dep_signature -> value.dep_signature`,
        // `read_set_signature -> signature`.
        let node_compute =
            move || -> CacheAdmission<crate::semantic_query::CacheRead<MaterializeOutcome>> {
                match crate::component_meta_materialize::trace_materialize_compute(ctx, compute) {
                    crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(entry) => {
                        // Defensive completeness invariant: only
                        // complete/cacheable entries lower into
                        // `MaterializeStructureDb`. A genuine partial is
                        // converted to `ReturnOnly` by
                        // `finish_materialize_admission` + the fact-tracer
                        // wrapper arms — both keyed on the PER-COLD-COMPUTE
                        // completeness scope (`current_cold_compute_completeness`)
                        // and run WHILE that scope is live (inside `compute`),
                        // BEFORE this lowering runs. So a `Cacheable` arrival
                        // here means THIS compute's scope was complete by
                        // construction. The invariant is NOT keyed on the
                        // request-global suppress sticky: a sibling consumer's
                        // request-scoped partial must NOT block this
                        // consumer's value-complete entry, and the scope has
                        // already dropped by the time `compute()` returns —
                        // its in-scope gates are the authority, not a
                        // post-scope re-read.
                        CacheAdmission::Cacheable {
                            value: crate::semantic_query::CacheRead {
                                value: entry.outcome,
                                dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                                walker_diagnostics: Arc::from([]),
                                cache_suppress: false,
                                result_is_partial: false,
                            },
                            signature: entry.read_set_signature,
                            self_root_canonicals: entry.self_root_canonicals,
                            validated_at_generation: entry.validated_at_generation,
                        }
                    }
                    crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
                        value,
                        reason,
                    } => CacheAdmission::ReturnOnly { value, reason },
                    crate::cache_runtime::singleflight::ComputeAdmission::Failed => {
                        CacheAdmission::Failed {
                            reason: NonAdmissionReason::ComputeFailed,
                        }
                    }
                }
            };
        let node = QueryCandidateNode {
            store: &self.store,
            inflight: &self.inflight,
            ctx,
            compute: std::cell::RefCell::new(Some(node_compute)),
            // Admission refusal returns the COMPUTED outcome, lowered to
            // its non-cacheable form: `cache_suppress = true` taints the
            // enclosing build's memo admission; `result_is_partial` is
            // untouched (a refused COMPLETE compute is benign
            // non-cacheability, never a partial). Discarding the value
            // would force the materialiser fallback to fabricate a
            // shallower substitute, making the published surface a
            // function of admission timing / parse order.
            unadmitted: Some(|read| {
                let mut read = read.clone();
                read.cache_suppress = true;
                read
            }),
        };
        crate::cache_runtime::query::lookup(&node, key.clone(), ctx)
    }

    /// Drop every candidate whose carrier references `canonical_id`,
    /// draining via the store's per-canonical reverse index in O(K).
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        self.store.invalidate_canonical(canonical_id);
    }

    /// Drop every cache candidate. Used on project-generation bumps.
    /// Takes the store's `retention_gate` write guard across the whole
    /// slot+index+budget+counter clear.
    pub fn invalidate_all(&self) {
        self.store.invalidate_all();
    }

    /// Number of warm candidates.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.store.live_count()
    }

    /// Number of distinct cache candidates currently materialised.
    ///
    /// **R7 cross-owner reuse contract.** N consumer scopes that reach
    /// the same content-free `MaterializationCacheKey` subject (the
    /// canonical-subject slot + projection path/policy/mode axes — NO
    /// consumer-scope dimension at all) collapse to ONE slot. Used by
    /// `tests/cases/g_misc0/cross_owner_materialise_reuse_production.rs` to verify the
    /// production-flow contract: driving
    /// `materialize_component_meta_structure` from N owners with a shared
    /// inner type produces one candidate for that slot under one view.
    ///
    /// Synonym for [`live_count`](Self::live_count).
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.store.live_count()
    }

    /// Test-only synthetic-candidate inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test(&self, marker: &str) {
        use crate::component_meta_materialize::MaterializationScope;
        use crate::semantic_query::{ResolvedDeclSlotIdentity, SemanticNodeId};
        // Synthetic content-free canonical-subject key: `marker` seeds the
        // slot's defining canonical so distinct markers produce distinct
        // slots (the schema test needs distinguishable synthetic entries).
        // Built via the canonical env-bearing `type_slot` constructor with
        // explicit synthetic-zero env axes — this fixture has no live host
        // to source env from, and the schema-eviction invariant under test
        // is env-independent. (The zero-env `type_slot_unscoped` convenience
        // constructor is test-fixture-only and rejected in this
        // debug-visible function by `no_production_caller_of_zero_env_slot_constructors`.)
        let key = MaterializationCacheKey {
            decl: ResolvedDeclSlotIdentity::type_slot(
                Arc::from(marker),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("__schema_test__"),
                0,
                [0u8; 16],
                [0u8; 16],
            ),
            projection_path: crate::resolver_core::RouteDemand::Whole,
            scope_axis: MaterializationScope::TopLevel,
            projection_mode: ProjectionMode::Shallow,
            normalized_type_args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
        };
        let value = crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Miss(SemanticNodeId(0)),
            dep_signature: Arc::from(Vec::new()),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
            result_is_partial: false,
        };
        self.store.insert_for_test(
            key,
            value,
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new()),
            0,
        );
    }

    /// Test-only: admit a candidate at a specific generation directly into
    /// the store, bypassing the cooperative flight. Drives the
    /// generation-gate `peek` discriminator.
    #[cfg(test)]
    pub(crate) fn insert_for_test(
        &self,
        key: MaterializationCacheKey,
        outcome: MaterializeOutcome,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
    ) {
        let value = crate::semantic_query::CacheRead {
            value: outcome,
            dep_signature: Arc::from(Vec::new()),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
            result_is_partial: false,
        };
        self.store.insert_for_test(
            key,
            value,
            read_set_signature,
            self_root_canonicals,
            validated_at_generation,
        );
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
        self.store.evict_if_schema_mismatch()
    }
}

// ===========================================================================
// C — RefCycleResultDb
// ===========================================================================

use crate::cache_runtime::singleflight::ComputeAdmission;
use crate::semantic_query::{DeclIdentity, ResolvedDeclSlotIdentity};

/// Substrate version for the transitive-cycle BFS result. A bump
/// invalidates every [`RefCycleResultKey`] slot by changing the key (the
/// cached value is still validated by its self-root fact signature; the
/// version is the coarse "the BFS algorithm changed shape" rail).
pub const REF_CYCLE_RESULT_VERSION: u32 = 1;

/// Content-free query-identity key for a transitive-cycle BFS result (R5
/// query-identity family, R6 content-free, R21 split-env).
///
/// The BFS roots at one declaration; its identity is the env-bearing,
/// content-free [`ResolvedDeclSlotIdentity`] slot (which carries
/// `type_env_hash` / `lib_env_hash` / `project_identity` as decl-site
/// identity), plus the extra `resolve_env_hash` the BFS's
/// `Skeleton`-mode instantiation depends on, plus the substrate
/// `version`. The versioned `DeclIdentity` (which embeds `whole_hash`) is
/// intentionally NOT the key — concurrent content versions of the same
/// root co-locate as candidates inside this one slot (R20
/// multi-candidate), each rooted value-side by its
/// `ReadSetSignature.facts` + `self_root_canonicals` and validated
/// strictly on every read. So a content edit to the root or any visited
/// file rejects the stale candidate WITHOUT a key change (R6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefCycleResultKey {
    /// Env-bearing, content-free root declaration slot.
    pub root: ResolvedDeclSlotIdentity,
    /// The `resolve_env_hash` the BFS instantiation depends on (R21 — not
    /// carried by the slot).
    pub resolve_env_hash: crate::semantic_query::HashValue,
    /// Substrate version ([`REF_CYCLE_RESULT_VERSION`]).
    pub version: u32,
}

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

/// Host-owned cache for transitive cycle BFS results.
///
/// Mirrors [`MaterializeStructureDb`]: a shared budgeted
/// [`ReverseIndexedCandidateStore`](crate::cache_runtime::ReverseIndexedCandidateStore)
/// keyed by the content-free [`RefCycleResultKey`] slot, whose
/// per-canonical reverse index drains under `invalidate_for_canonical`
/// (the canonicals come from each candidate's value-side self-roots, not
/// the key), whose `FactCandidateDiscriminant`
/// (generation + facts) selects which candidate a re-publish replaces,
/// and whose shared `live_counter` (an `Arc<AtomicU64>` across all
/// sibling DBs) uses the saturating-subtract pattern on `invalidate_all`
/// to preserve other DBs' contributions.
///
/// Cooperative-admission integration: `get_or_compute_admit` builds a
/// [`QueryCandidateNode`] over the store and routes the cold-path BFS
/// through [`crate::cache_runtime::node::query::lookup`] and the
/// split publish lifecycle. The BFS `compute` closure runs synchronously
/// on the caller's thread (see `cache_runtime/singleflight.rs`
/// synchronous-compute contract), so borrow-capture of
/// `&dyn ResolverContext` is safe — no thread-hop occurs. An overflowed /
/// unrootable signature returns the computed bool through
/// [`ComputeAdmission::ReturnOnly`] without admitting a candidate; a
/// `ReturnOnly` value is winner-only (it carries no view-validatable
/// entry), so the winner alone receives it without re-running its BFS
/// uncached, while a joiner that coalesced onto the `ReturnOnly`
/// winner forks and cold-recomputes its own BFS against its own view.
pub struct RefCycleResultDb {
    /// The shared reverse-indexed multi-candidate store (budgeted form).
    /// The content-free [`RefCycleResultKey`] slot keys it: concurrent
    /// content versions of the same root co-locate as candidates inside
    /// one slot (each rooted value-side by its self-root fact signature),
    /// the FIFO retention budget is the routine reclamation path, and the
    /// per-canonical reverse index (derived from each candidate's
    /// value-side canonicals, NOT the key) drains on per-canonical
    /// invalidation. Concurrent base/overlay variants coexist as
    /// candidates (R20).
    store: crate::cache_runtime::ReverseIndexedCandidateStore<
        RefCycleResultKey,
        crate::semantic_query::CacheRead<bool>,
    >,
    /// Per-cache flight table keyed by the flight identity (cache key +
    /// store-view compat token) so two overlays on one key do not coalesce.
    inflight: InflightTable<QueryFlightKey<RefCycleResultKey>>,
}

impl RefCycleResultDb {
    /// Total candidate-count cap. A long-lived session that re-runs the
    /// transitive-cycle BFS for many owner versions caps here; the oldest
    /// candidates are FIFO-evicted via the store's deferred-eviction path.
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
            store: crate::cache_runtime::ReverseIndexedCandidateStore::with_counter_and_budget(
                live_counter,
                budget_cap,
            ),
            inflight: InflightTable::new(),
        }
    }

    /// Configured total candidate-count cap.
    #[must_use]
    pub fn retention_cap(&self) -> usize {
        self.store.retention_cap()
    }

    /// Read-only test accessor for the shared `live_counter`. Used by the
    /// invalidation tests to verify that `invalidate_for_canonical` and
    /// `invalidate_all` correctly decrement the counter without
    /// corrupting sibling DBs' contributions to the shared sum.
    #[cfg(test)]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.store.live_counter_for_test()
    }

    /// Cooperative-admission cold BFS over the ref-cycle cache, routed
    /// through the query-identity split-publish lifecycle adapter over the
    /// shared store. The producer supplies only the BFS closure. The DB invokes
    /// it through `trace_ref_cycle_compute`, so the owner controls tracer
    /// installation, evidence finalisation, and typed admission before the
    /// storage write.
    ///
    /// The store is budgeted, so the publish lifecycle threads the store's
    /// `retention_gate` as the publish fence and the FIFO retention
    /// victims through `evict_deferred`. The fence read guard spans
    /// revalidation → publish_core → evict_deferred, so a
    /// project-generation `clear` cannot interleave and the re-entrant
    /// eviction cannot self-deadlock. A `ReturnOnly` outcome (overflow /
    /// unrootable / `RouteGeneration`) returns the computed bool to the
    /// winner without admitting; joiners fork and recompute.
    /// Build the content-free [`RefCycleResultKey`] for a BFS root
    /// `DeclIdentity` via the shared, U2-derived slot builders
    /// (`type_slot_for` + `resolve_env_hash_for` — the SAME builders the
    /// reducer caches use, sourcing env from the live host). The lookup
    /// and the publish both route through this one builder, so they key on
    /// the identical slot; the `DeclIdentity`'s `whole_hash` is dropped
    /// (content rooting lives on the value).
    fn key_for(id: &DeclIdentity, ctx: &dyn ResolverContext) -> RefCycleResultKey {
        let dispatch = ctx.dispatch();
        RefCycleResultKey {
            root: dispatch.type_slot_for(
                Arc::clone(&id.canonical_id),
                id.owner,
                Arc::clone(&id.decl_name),
            ),
            resolve_env_hash: dispatch.resolve_env_hash_for(&id.canonical_id),
            version: REF_CYCLE_RESULT_VERSION,
        }
    }

    pub(crate) fn get_or_compute_admit<C>(
        &self,
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
        // Unpack the producer's domain `ComputeAdmission<V, Entry>` into a
        // node-level `CacheAdmission<V>`. `result -> value.value`,
        // `dispatch_dep_signature -> value.dep_signature`,
        // `read_set_signature -> signature`.
        let node_compute = move || -> CacheAdmission<crate::semantic_query::CacheRead<bool>> {
            match trace_ref_cycle_compute(ctx, compute_bfs) {
                ComputeAdmission::Cacheable(entry) => CacheAdmission::Cacheable {
                    value: crate::semantic_query::CacheRead {
                        value: entry.result,
                        dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                        walker_diagnostics: Arc::from([]),
                        cache_suppress: false,
                        result_is_partial: false,
                    },
                    signature: entry.read_set_signature,
                    self_root_canonicals: entry.self_root_canonicals,
                    validated_at_generation: entry.validated_at_generation,
                },
                ComputeAdmission::ReturnOnly { value, reason } => {
                    CacheAdmission::ReturnOnly { value, reason }
                }
                ComputeAdmission::Failed => CacheAdmission::Failed {
                    reason: NonAdmissionReason::ComputeFailed,
                },
            }
        };
        let node = QueryCandidateNode {
            store: &self.store,
            inflight: &self.inflight,
            ctx,
            compute: std::cell::RefCell::new(Some(node_compute)),
            unadmitted: None,
        };
        crate::cache_runtime::query::lookup(&node, Self::key_for(id, ctx), ctx)
    }

    /// Strict-validation peek.
    ///
    /// Every read validates the candidate's carrier against the live
    /// store view BEFORE returning — there is no carrier-bypassing fast
    /// return. The candidate's `self_root_canonicals` (the BFS root file
    /// plus every visited declaration's file) validate **strictly**: a
    /// same-canonical content edit, or a self-root canonical the live
    /// store view no longer tracks, rejects the candidate; every other
    /// fact keeps the lazy cross-file permissiveness. The candidate's
    /// `validated_at_generation` must additionally still equal the live
    /// project generation. A stale candidate never returns and never
    /// bubbles (the store keeps it for other views; routine reclamation is
    /// the FIFO budget + per-canonical drain).
    pub(crate) fn peek(
        &self,
        id: &DeclIdentity,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<bool>> {
        let generation = ctx.project_type_store().current_project_generation();
        let key = Self::key_for(id, ctx);
        let result = self.store.lookup(&key, |candidate| {
            if candidate.validated_at_generation == generation
                && candidate
                    .signature
                    .validate_with_self_roots(ctx, &candidate.self_root_canonicals)
            {
                candidate.signature.bubble(ctx);
                Some(candidate.value.clone())
            } else {
                None
            }
        });
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

    /// Drop every candidate whose carrier references `canonical_id`,
    /// draining via the store's per-canonical reverse index in O(K).
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        self.store.invalidate_canonical(canonical_id);
    }

    /// Drop every cache candidate. Used on project-generation bumps.
    /// Takes the store's `retention_gate` write guard across the whole
    /// slot+index+budget+counter clear.
    pub fn invalidate_all(&self) {
        self.store.invalidate_all();
    }

    /// Number of warm candidates.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.store.live_count()
    }

    /// Test-only: admit a candidate at a specific generation directly into
    /// the store, bypassing the cooperative flight. Drives the
    /// generation-gate `peek` discriminator.
    #[cfg(test)]
    pub(crate) fn insert_for_test(
        &self,
        id: &DeclIdentity,
        ctx: &dyn ResolverContext,
        result: bool,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
    ) {
        let value = crate::semantic_query::CacheRead {
            value: result,
            dep_signature: Arc::from(Vec::new()),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
            result_is_partial: false,
        };
        // Plant under the SAME content-free key `peek` builds for `id`, so
        // the planted candidate is addressable by a subsequent peek.
        self.store.insert_for_test(
            Self::key_for(id, ctx),
            value,
            read_set_signature,
            self_root_canonicals,
            validated_at_generation,
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
/// Delegates to [`RefCycleResultDb::peek`], a strict-validation read:
/// every hit validates the entry's carrier against the live store view
/// before returning — there is no generation-equal fast return that
/// bypasses validation. Returns `Some(read)` only when a cached entry
/// exists AND its strict self-root validation passes; returns `None` on
/// a true cache miss or a stale entry (a stale candidate is skipped on
/// read — it stays resident for other views — and the caller falls
/// through to BFS compute; reclamation is the FIFO budget / per-canonical
/// drain / schema eviction / generation clear).
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
        self.store.invalidate_canonical(canonical_id)
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
/// thread (per singleflight's synchronous-compute contract),
/// so capturing `&dyn ResolverContext` and `&DeclIdentity` directly is safe.
///
/// On cooperative-admission success: bumps `live_counter`, registers
/// the reverse-index, and returns `Some(CacheRead)`. The winner's cold
/// BFS runs once; an overflowed / unrootable signature returns the
/// computed bool through [`ComputeAdmission::ReturnOnly`] without
/// admitting the entry and **without the winner re-running its BFS
/// uncached** (a joiner coalesced onto a `ReturnOnly` winner forks and
/// cold-recomputes against its own view — singleflight's winner-only
/// `ReturnOnly` contract). `None` is returned
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
    db.get_or_compute_admit(id, ctx, compute_bfs)
}

/// Run the complete ref-cycle cold computation inside the DB owner's fact
/// tracer and lower the finalised evidence to a typed admission outcome.
fn trace_ref_cycle_compute<C>(
    ctx: &dyn ResolverContext,
    compute_bfs: C,
) -> ComputeAdmission<crate::semantic_query::CacheRead<bool>, RefCycleEntry>
where
    C: FnOnce(
        &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
        &mut Vec<(Arc<str>, crate::types::Hash16)>,
    ) -> bool,
{
    // Wrap the BFS cold-compute with `install_fact_tracer`. On `Ok`,
    // merge the traced observation set on top of the visited-identity
    // self-roots. On `Overflow`, return the computed bool via
    // `ReturnOnly` (winner-only; the winner does not re-run its BFS
    // uncached, and joiners fork per the singleflight contract). The
    // Db drives the
    // query-identity split-publish lifecycle over the shared store
    // (warm-hit lookup, revalidation, publish-core under the slot guard,
    // guard-free deferred FIFO eviction, publish fence).
    let host = ctx.host_for_fact_tracer_install();
    let provenance = Arc::clone(&host.provenance);
    // Snapshot the project generation BEFORE the BFS dispatches
    // any work. A `ProjectGeneration` reset that lands during
    // the cold BFS window bumps this; the post-compute
    // revalidation (run under the `publish_fence` read guard)
    // then rejects the entry, and a stale entry can neither
    // survive a reset nor publish into a freshly-cleared cache.
    let validated_at_generation = ctx.project_type_store().current_project_generation();
    let mut compute_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut observed_self_roots: Vec<(Arc<str>, crate::types::Hash16)> = Vec::new();
    let (result, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
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
    let return_only_value = |dep_signature: DepSignature| crate::semantic_query::CacheRead {
        value: result,
        dep_signature,
        walker_diagnostics: Arc::from([]),
        cache_suppress: false,
        result_is_partial: false,
    };
    // ReturnOnly never publishes — fenced-serve arm. A BFS whose
    // traced scope consumed a FENCED (ReturnOnly) IndexedReady
    // serve derived its reachability answer from a
    // served-without-publication artifact while its fact carrier
    // validates against the live view. The computed bool is still
    // returned via `ReturnOnly`, carrying the BFS fence.
    if matches!(
        &finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    ) {
        provenance
            .ref_cycle_overflow_refusals
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return ComputeAdmission::ReturnOnly {
            value: return_only_value(Arc::clone(&dispatch_dep_signature)),
            reason: crate::cache_runtime::NonAdmissionReason::GenerationSuperseded,
        };
    }
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
        return ComputeAdmission::ReturnOnly {
            value: return_only_value(Arc::clone(&dispatch_dep_signature)),
            reason: crate::cache_runtime::NonAdmissionReason::RouteGenerationDependency,
        };
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
        return ComputeAdmission::ReturnOnly {
            value: return_only_value(Arc::clone(&dispatch_dep_signature)),
            reason: crate::cache_runtime::NonAdmissionReason::ForcedTestRefusal,
        };
    }
    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(traced) => {
            // Build the observed-root carrier: visited-identity
            // self-roots prepended, traced facts merged on top.
            match ref_cycle_read_set(&observed_self_roots, &traced) {
                Some((facts, self_root_canonicals)) => ComputeAdmission::Cacheable(RefCycleEntry {
                    result,
                    read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                    dispatch_dep_signature,
                    self_root_canonicals,
                    validated_at_generation,
                }),
                None => {
                    // A torn observation among the visited
                    // self-roots — the value is valid but the
                    // signature cannot be built strictly. The
                    // computed bool is returned via `ReturnOnly`,
                    // carrying the BFS fence.
                    provenance
                        .ref_cycle_overflow_refusals
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    ComputeAdmission::ReturnOnly {
                        value: return_only_value(dispatch_dep_signature),
                        reason: crate::cache_runtime::NonAdmissionReason::SelfRootConflict,
                    }
                }
            }
        }
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_) => {
            unreachable!("non-cacheable finalise returned above before cache admission")
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            provenance
                .ref_cycle_overflow_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Tracer overflowed — return the computed bool via
            // `ReturnOnly`, carrying the BFS fence; no second
            // uncached BFS.
            ComputeAdmission::ReturnOnly {
                value: return_only_value(dispatch_dep_signature),
                reason: crate::cache_runtime::NonAdmissionReason::SignatureOverflow,
            }
        }
    }
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
/// inside the seal contract (`no_concrete_verter_host_in_seal_scope`
/// arch guard). Integration tests reach this via the
/// crate-public wrapper
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
        // The "no override" determination is a structural query
        // into the interface members. For the producer's
        // substrate-correctness contract, the
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
    // ReturnOnly never publishes — fenced-serve arm: a proof derived
    // from a served-without-publication artifact must not seal a
    // shared no-override entry whose facts validate against the live
    // view. Decline to publish; the consumer takes the slow path.
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
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_) => {
            host.provenance
                .app_config_proof_overflow_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            host.provenance
                .app_config_proof_overflow_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        }
    }
}
