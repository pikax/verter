//! Output read and publication capabilities owned by the sole query driver.
//! Durable DBs contain storage; request-local adapters perform validation,
//! tracing, nested work, and cooperative admission in the original order.
use super::raise::MaterializedOutputTypeExpr;
use crate::cache_runtime::node::{lookup, ArtifactNode, ComputeCtx, QueryFlightKey};
use crate::cache_runtime::singleflight::InflightTable;
use crate::cache_runtime::{CacheAdmission, CacheEntry, NonAdmissionReason};
use crate::component_meta_caches::*;
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::resolver_core::fact_validation_port::{FactValidation, LiveFactValidation};
use crate::resolver_core::resolver_context::{RequestFlags, RequestSnapshot};
use dashmap::DashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use verter_session_query::declarations::metadata::ResolvedTypeDeclaration;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::fact_cache::ReadSetSignature;
/// Selected output storage; there are no raw-resource accessors.
pub(crate) struct SingleEntryAttachment<'a, K: Eq + std::hash::Hash + Clone, V> {
    entries: &'a DashMap<K, Arc<CacheEntry<V>>>,
    inflight: &'a InflightTable<QueryFlightKey<K>>,
    live_counter: &'a AtomicU64,
    schema_version: u32,
}
impl<'a, K: Eq + std::hash::Hash + Clone, V> SingleEntryAttachment<'a, K, V> {
    pub(crate) fn new(
        entries: &'a DashMap<K, Arc<CacheEntry<V>>>,
        inflight: &'a InflightTable<QueryFlightKey<K>>,
        live_counter: &'a AtomicU64,
        schema_version: u32,
    ) -> Self {
        Self {
            entries,
            inflight,
            live_counter,
            schema_version,
        }
    }
}
pub(crate) struct CandidateAttachment<
    'a,
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
> {
    store: &'a crate::cache_runtime::ReverseIndexedCandidateStore<K, V>,
    inflight: &'a InflightTable<QueryFlightKey<K>>,
}
impl<
        'a,
        K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    > CandidateAttachment<'a, K, V>
{
    pub(crate) fn new(
        store: &'a crate::cache_runtime::ReverseIndexedCandidateStore<K, V>,
        inflight: &'a InflightTable<QueryFlightKey<K>>,
    ) -> Self {
        Self { store, inflight }
    }
}
/// Read-only typed output operations; no source services or mutation escape.
pub struct MemoRead<'a, D> {
    db: &'a D,
    facts: &'a dyn FactValidation,
    /// The request snapshot's flag handles, borrowed once at the request
    /// boundary: the warm-read generation gates below read them as plain
    /// fields, with no per-gate port dispatch.
    flags: &'a RequestFlags,
}
impl<'a, D> MemoRead<'a, D> {
    pub(super) fn new(db: &'a D, facts: &'a dyn FactValidation, flags: &'a RequestFlags) -> Self {
        Self { db, facts, flags }
    }
}
/// Request-local output coordination; storage never owns cold callbacks.
///
/// `W` is the request's concrete workspace clock source: a publish samples
/// the live aggregate basis through it without a dynamic dispatch.
pub struct MemoPublish<'a, D, W> {
    db: &'a D,
    facts: &'a dyn LiveFactValidation<Clocks = W>,
    /// The snapshot the host captured when it admitted this request,
    /// borrowed once at the request boundary so the admission generation
    /// gates and basis samples below read plain fields.
    snapshot: &'a RequestSnapshot<W>,
}
impl<'a, D, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone> MemoPublish<'a, D, W> {
    pub(super) fn new(
        db: &'a D,
        facts: &'a dyn LiveFactValidation<Clocks = W>,
        snapshot: &'a RequestSnapshot<W>,
    ) -> Self {
        Self {
            db,
            facts,
            snapshot,
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(
        db: &'a D,
        facts: &'a dyn LiveFactValidation<Clocks = W>,
        snapshot: &'a RequestSnapshot<W>,
    ) -> Self {
        Self {
            db,
            facts,
            snapshot,
        }
    }
}

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
/// Per-call [`QueryNode`] adapter for the reverse-indexed multi-candidate
/// cache (`ImportedRegistryDb`).
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
    ctx: &'a dyn FactValidation,
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
        crate::project_semantic_dispatch::memo::read_candidate(self.store, key, |candidate| {
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
fn single_entry_admission<V, W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
    probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
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
    ctx: &dyn FactValidation,
    flags: &RequestFlags,
) -> Option<V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    let entry_arc = entries.get(key).map(|e| e.clone())?;
    if entry_arc.validated_at_generation == flags.current_project_generation()
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
/// Sealed capability for one Shape-cache owner scope. Only
/// [`MemoPublish::with_owner_scope`] can construct it, and its tracer borrow
/// cannot escape the callback. All production Shape-cache writes require this
/// capability, so key classification, gates, and the complete cold compute can
/// share one driver-owned scope.
pub struct ShapeCacheOwnerScope<
    'db,
    't,
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
> {
    db: &'db ShapeCacheDb,
    ctx: &'db dyn LiveFactValidation<Clocks = W>,
    snapshot: &'db RequestSnapshot<W>,
    probe: &'t crate::fact_signature_helpers::CacheabilityProbe<'t, W>,
}

impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    ShapeCacheOwnerScope<'_, '_, W>
{
    #[must_use]
    pub fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        MemoRead::new(self.db, self.ctx, self.snapshot.flags()).peek(key)
    }

    pub fn get_or_compute<F>(
        &self,
        key: &ShapeCacheKey,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        MemoPublish::new(self.db, self.ctx, self.snapshot)
            .get_or_compute_in_scope(key, self.ctx, self.probe, compute)
    }

    #[must_use]
    pub fn non_cacheable(&self) -> bool {
        self.probe.non_cacheable()
    }

    pub fn admit_computed(
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
impl MemoRead<'_, ImportedRegistryDb> {
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
    pub fn peek(&self, key: &ImportedRegistryKey) -> Option<ImportedRegistryValue> {
        let storage = self.db.attach_output();
        let ctx = self.facts;

        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let generation = self.flags.current_project_generation();
        crate::project_semantic_dispatch::memo::read_candidate(storage.store, key, |candidate| {
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
}

impl MemoRead<'_, ShapeCacheDb> {
    /// Peek-only lookup: returns the cached value only if its
    /// fact_dep_signature is still valid against `ctx`.
    pub(crate) fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        let storage = self.db.attach_output();
        let ctx = self.facts;

        if storage.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation rejects a same-scope content edit.
        // The entry carries its own self-roots, validated strictly.
        let result = single_entry_peek(storage.entries, key, ctx, self.flags);
        if let Some(rctx) = crate::request_context::current_request_context() {
            // Every ShapeCacheDb subject is a member-shape-route identity
            // now that the TypeExpr-START route also keys its LOWERED
            // settled node (`MemberValueNode`) — so all peeks count into
            // the member-shape layer. The former `materialize_memo`
            // per-request layer stays as a field on the audit
            // `CacheLayerBreakdown` (additive wire rule) and reads zero.
            let counter = &rctx.cache_counters.member_shape_cache;
            if result.is_some() {
                counter.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                counter.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }
}
impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    MemoPublish<'_, ImportedRegistryDb, W>
{
    #[cfg(any(test, feature = "test-support"))]
    pub fn inject_concurrent_publish_for_test(
        &self,
        key: ImportedRegistryKey,
        entry: Arc<ImportedRegistryEntry>,
    ) {
        self.db.insert_for_test(key, entry);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn peek(&self, key: &ImportedRegistryKey) -> Option<ImportedRegistryValue> {
        MemoRead::new(self.db, self.facts, self.snapshot.flags()).peek(key)
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
    pub fn get_or_compute_admit<P, Prepare, F>(
        &self,
        key: &ImportedRegistryKey,
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
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
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
        ctx: &dyn FactValidation,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
        compute: F,
    ) -> Option<ImportedRegistryValue>
    where
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            ImportedRegistryValue,
            ImportedRegistryEntry,
        >,
    {
        let storage = self.db.attach_output();

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
                        signature: verter_session_query::facts::fact_cache::ReadSetSignature::new(
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
            store: storage.store,
            inflight: storage.inflight,
            ctx,
            compute: std::cell::RefCell::new(Some(node_compute)),
            unadmitted: None,
        };
        crate::cache_runtime::query::lookup(&node, key.clone(), ctx, self.snapshot.flags())
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_compute_admit_traced_for_test<F>(
        &self,
        key: &ImportedRegistryKey,
        compute: F,
    ) -> Option<ImportedRegistryValue>
    where
        F: FnOnce() -> crate::cache_runtime::singleflight::ComputeAdmission<
            ImportedRegistryValue,
            ImportedRegistryEntry,
        >,
    {
        self.get_or_compute_admit(key, || (), |()| compute())
    }
}
impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    MemoPublish<'_, DeclarationLookupDb, W>
{
    /// The SOLE admission funnel for `DeclarationLookupDb`.
    ///
    /// The request-local publication driver opens the cacheability scope around the cold closure. The
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
    pub fn get_or_compute<P, Prepare, F>(
        &self,
        key: &DeclarationLookupKey,
        prepare: Prepare,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
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
        ctx: &dyn FactValidation,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        let storage = self.db.attach_output();

        // The entry's keyed canonical is its self-root: the warm-read
        // validator validates the self-root `FileWholeHash` strictly so
        // a same-canonical content edit (or a keyed canonical that
        // became untracked) rejects the entry instead of riding the
        // lazy "untracked → accept" rule.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: storage.entries,
            inflight: storage.inflight,
            live_counter: storage.live_counter,
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
        lookup(&node, key.clone(), ctx, self.snapshot.flags())
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_compute_traced_for_test<F>(
        &self,
        key: &DeclarationLookupKey,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    MemoPublish<'_, ResolvabilityDb, W>
{
    /// The SOLE admission funnel for `ResolvabilityDb`.
    ///
    /// The request-local publication driver opens the cacheability scope around the cold closure. The
    /// verdict is consulted AFTER `compute` returns
    /// ([`single_entry_admission`]), so a non-cacheable read taken INSIDE the
    /// cold build refuses the write; the computed bool is still returned to the
    /// winner through `ReturnOnly`.
    ///
    /// The closure reports a [`ComputedEntry`]: a bool this cache cannot ROOT
    /// (an overflowed signature, an unobservable keyed content version, a
    /// request-partial resolution) still rides `ReturnOnly` back to the winner,
    /// so the caller never re-derives a verdict it already has.
    pub fn get_or_compute<P, Prepare, F>(
        &self,
        key: &ResolvabilityKey,
        prepare: Prepare,
        compute: F,
    ) -> Option<bool>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<bool>,
    {
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
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
        ctx: &dyn FactValidation,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> ComputedEntry<bool>,
    {
        let storage = self.db.attach_output();

        // The keyed source canonical is the entry's self-root — strict
        // warm-read validation rejects a same-canonical edit or an
        // untracked keyed canonical.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: storage.entries,
            inflight: storage.inflight,
            live_counter: storage.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                single_entry_admission(probe, compute())
            })),
        };
        lookup(&node, key.clone(), ctx, self.snapshot.flags())
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_compute_traced_for_test<F>(
        &self,
        key: &ResolvabilityKey,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> ComputedEntry<bool>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    MemoPublish<'_, OwnerCollectionDb, W>
{
    /// The SOLE admission funnel for `OwnerCollectionDb`.
    ///
    /// The request-local publication driver opens the cacheability scope around the cold closure. The
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
    pub fn get_or_compute<P, Prepare, F>(
        &self,
        key: &OwnerCollectionKey,
        prepare: Prepare,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        Prepare: FnOnce() -> P,
        F: FnOnce(P) -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
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
        ctx: &dyn FactValidation,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        F: FnOnce() -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        let storage = self.db.attach_output();

        // The owner canonical is the entry's self-root. The locator is
        // content-free, but the position it addresses is only meaningful
        // against the owner content version the producer observed, so
        // strict self-root validation remains the correctness floor — a
        // content edit to the owner file invalidates the cached locator.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let node = SingleEntryArtifactNode {
            entries: storage.entries,
            inflight: storage.inflight,
            live_counter: storage.live_counter,
            self_root_canonicals: self_roots,
            compute: std::cell::RefCell::new(Some(move || {
                single_entry_admission(probe, compute())
            })),
        };
        lookup(&node, key.clone(), ctx, self.snapshot.flags())
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_compute_traced_for_test<F>(
        &self,
        key: &OwnerCollectionKey,
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        F: FnOnce() -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>
    MemoPublish<'_, ShapeCacheDb, W>
{
    #[cfg(any(test, feature = "test-support"))]
    pub fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        MemoRead::new(self.db, self.facts, self.snapshot.flags()).peek(key)
    }
    /// Open the sole production admission scope for this DB. The sealed
    /// capability passed to `f` is the only route to a Shape-cache write.
    pub(crate) fn with_owner_scope<'db, F, R>(&'db self, f: F) -> R
    where
        F: for<'t> FnOnce(ShapeCacheOwnerScope<'db, 't, W>) -> R,
    {
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx_and_snapshot(
                ctx,
                self.snapshot,
            ),
            |probe| {
                f(ShapeCacheOwnerScope {
                    db: self.db,
                    ctx,
                    snapshot: self.snapshot,
                    probe,
                })
            },
        )
        .0
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
        ctx: &dyn FactValidation,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_, W>,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        let storage = self.db.attach_output();

        // The subject's scope canonical is the entry's self-root —
        // strict warm-read validation rejects a same-scope content edit.
        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(key.scope_canonical())]);
        let node = SingleEntryArtifactNode {
            entries: storage.entries,
            inflight: storage.inflight,
            live_counter: storage.live_counter,
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
        lookup(&node, key.clone(), ctx, self.snapshot.flags())
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_or_compute_traced_for_test<F>(
        &self,
        key: &ShapeCacheKey,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        self.with_owner_scope(|scope| scope.get_or_compute(key, compute))
    }
    /// Test-only sibling of [`Self::get_or_compute_traced_for_test`] for
    /// [`Self::admit_computed`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn admit_computed_traced_for_test(
        &self,
        key: &ShapeCacheKey,
        value: MaterializedOutputTypeExpr,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) -> MaterializedOutputTypeExpr {
        self.with_owner_scope(|scope| scope.admit_computed(key, value, fact_dep_signature))
    }
}
impl<C: crate::resolver_core::ResolverCapabilities> super::ProjectSemanticDispatch<'_, C> {
    pub fn imported_registry_read(&self) -> MemoRead<'_, ImportedRegistryDb> {
        MemoRead::new(
            self.binding.imported_registry.as_ref(),
            self.ctx,
            self.snapshot.flags(),
        )
    }
    pub fn imported_registry_publish(&self) -> MemoPublish<'_, ImportedRegistryDb, C::Clocks> {
        MemoPublish::new(
            self.binding.imported_registry.as_ref(),
            self.ctx,
            self.snapshot,
        )
    }

    pub fn declaration_publish(&self) -> MemoPublish<'_, DeclarationLookupDb, C::Clocks> {
        MemoPublish::new(self.binding.declarations.as_ref(), self.ctx, self.snapshot)
    }

    pub fn resolvability_publish(&self) -> MemoPublish<'_, ResolvabilityDb, C::Clocks> {
        MemoPublish::new(self.binding.resolvability.as_ref(), self.ctx, self.snapshot)
    }

    pub fn owner_collection_publish(&self) -> MemoPublish<'_, OwnerCollectionDb, C::Clocks> {
        MemoPublish::new(
            self.binding.owner_collections.as_ref(),
            self.ctx,
            self.snapshot,
        )
    }
    pub fn with_shape_scope<F, R>(&self, f: F) -> R
    where
        F: for<'t> FnOnce(ShapeCacheOwnerScope<'_, 't, C::Clocks>) -> R,
    {
        MemoPublish::new(self.binding.shapes.as_ref(), self.ctx, self.snapshot).with_owner_scope(f)
    }
}

/// Narrow traced operations a host-owned result store's facade composes.
///
/// Each one runs against this dispatch's own request facts, so a facade that
/// owns its store's policy (key, admission decision, counters) still reads,
/// validates and traces through exactly the request the dispatch serves.
/// None of them exposes the fact service, the binding, or a store.
impl<C: crate::resolver_core::ResolverCapabilities> super::ProjectSemanticDispatch<'_, C> {
    /// The request's live project generation.
    #[must_use]
    pub fn current_project_generation(&self) -> u64 {
        self.snapshot.current_project_generation()
    }

    /// Whether every fact in `facts` still validates under the request's live
    /// view.
    #[must_use]
    pub fn validates_fact_signature(&self, facts: &[FactVersionRef]) -> bool {
        self.ctx.validates_fact_signature(facts)
    }

    /// Run `compute` under a fresh fact tracer seeded from the request's live
    /// aggregate clocks (an unvouched basis), and return its value with the
    /// finalised read set.
    pub fn traced_compute<R>(
        &self,
        compute: impl FnOnce() -> R,
    ) -> (
        R,
        verter_session_query::facts::fact_read_set::FactReadSetFinalise,
    ) {
        let (value, read_set) = crate::resolver_core::resolver_context::with_fact_tracer_cell(
            verter_session_query::facts::fact_cache::AggregateGenerations::from_seed(
                &verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
                &self.snapshot.clocks().live(),
            ),
            |_cell| compute(),
        );
        (value, read_set.finalise())
    }

    /// Run `compute` under a fact tracer that is bound to no request port: the
    /// basis is the engine's own unbound observers (the overflow counter and
    /// the test forcing knobs).
    #[cfg(any(test, feature = "test-support"))]
    pub fn traced_unbound<R>(
        &self,
        compute: impl FnOnce() -> R,
    ) -> (
        R,
        verter_session_query::facts::fact_read_set::FactReadSetFinalise,
    ) {
        crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound_observers(
                &self.binding.observers.overflow,
                &self.binding.observers.forcing,
            ),
            compute,
        )
    }

    /// The host attachment this dispatch's request carries, opaque to the
    /// engine.
    #[must_use]
    pub fn host_attachment(&self) -> &C::HostAttachment {
        self.host_attachment
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> super::ProjectSemanticDispatch<'_, C> {
    pub fn intern_resolved_identity(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> verter_session_query::type_solver::host::ResolvedRootIdentity {
        let pool = &self.binding.identities;
        verter_session_query::type_solver::host::ResolvedRootIdentity::new_in_owner(
            pool.intern(canonical),
            owner,
            pool.intern(name),
        )
    }
}

/// Validate an ordered candidate snapshot after all storage guards have dropped.
pub fn read_candidate<K, V, F>(
    store: &crate::cache_runtime::ReverseIndexedCandidateStore<K, V>,
    key: &K,
    mut accept: F,
) -> Option<V>
where
    K: Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: FnMut(
        &crate::cache_runtime::Candidate<crate::cache_runtime::FactCandidateDiscriminant, V>,
    ) -> Option<V>,
{
    let snapshot = store.candidate_snapshot(key)?;
    for candidate in &snapshot {
        if let Some(value) = accept(candidate) {
            return Some(value);
        }
    }
    None
}

/// Release selected shape storage after the lifecycle root releases graph nodes.
pub fn release_shape_canonical(
    db: &ShapeCacheDb,
    canonical_id: &str,
    graph: &crate::semantic_query_memo::SemanticGraphStore,
) -> usize {
    release_shape_entries(db, canonical_id, &|node| graph.node_is_live(node))
}
fn release_shape_entries(
    db: &ShapeCacheDb,
    canonical_id: &str,
    node_is_live: &dyn Fn(crate::semantic_query::SemanticNodeId) -> bool,
) -> usize {
    let storage = db.attach_output();
    let keys: Vec<ShapeCacheKey> = storage
        .entries
        .iter()
        .filter_map(|entry| {
            let key = entry.key();
            let rooted_here = key.scope_canonical().as_ref() == canonical_id;
            let subject_released = match key.member_value_subject_node() {
                Some(node) => !node_is_live(node),
                None => false,
            };
            let depends_here = || {
                entry
                    .value()
                    .signature
                    .canonical_ids()
                    .iter()
                    .any(|canonical| canonical.as_ref() == canonical_id)
            };
            (rooted_here || subject_released || depends_here()).then(|| key.clone())
        })
        .collect();
    let mut removed = 0usize;
    for key in keys {
        if storage.entries.remove(&key).is_some() {
            storage.live_counter.fetch_sub(1, Ordering::Relaxed);
            removed += 1;
        }
    }
    removed
}
