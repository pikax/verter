//! Output read and publication capabilities owned by the sole query driver.
//! Durable DBs contain storage; request-local adapters perform validation,
//! tracing, nested work, and cooperative admission in the original order.
use super::raise::MaterializedOutputTypeExpr;
use crate::cache_runtime::node::{lookup, ArtifactNode, ComputeCtx, QueryFlightKey};
use crate::cache_runtime::singleflight::InflightTable;
use crate::cache_runtime::{CacheAdmission, CacheEntry, NonAdmissionReason};
use crate::component_meta_caches::*;
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::resolver_core::fact_validation_port::FactValidation;
use crate::resolver_core::ResolvedTypeDeclaration;
use dashmap::DashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
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
pub(crate) struct MemoRead<'a, D, O = ()> {
    db: &'a D,
    facts: &'a dyn FactValidation,
    observations: O,
}
impl<'a, D> MemoRead<'a, D> {
    pub(super) fn new(db: &'a D, facts: &'a dyn FactValidation) -> Self {
        Self {
            db,
            facts,
            observations: (),
        }
    }
}
/// Request-local output coordination; storage never owns cold callbacks.
pub(crate) struct MemoPublish<'a, D> {
    db: &'a D,
    facts: &'a dyn FactValidation,
}
impl<'a, D> MemoPublish<'a, D> {
    pub(super) fn new(db: &'a D, facts: &'a dyn FactValidation) -> Self {
        Self { db, facts }
    }
    #[cfg(test)]
    pub(crate) fn for_test(db: &'a D, facts: &'a dyn FactValidation) -> Self {
        Self { db, facts }
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
    ctx: &dyn FactValidation,
) -> Option<V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    let entry_arc = entries.get(key).map(|e| e.clone())?;
    if entry_arc.validated_at_generation == ctx.current_project_generation()
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
pub(crate) struct ShapeCacheOwnerScope<'db, 't> {
    db: &'db ShapeCacheDb,
    ctx: &'db dyn FactValidation,
    probe: &'t crate::fact_signature_helpers::CacheabilityProbe<'t>,
}

impl ShapeCacheOwnerScope<'_, '_> {
    #[must_use]
    pub(crate) fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        MemoRead::new(self.db, self.ctx).peek(key)
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ShapeCacheKey,
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        MemoPublish::new(self.db, self.ctx)
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
    pub(crate) fn peek(&self, key: &ImportedRegistryKey) -> Option<ImportedRegistryValue> {
        let storage = self.db.attach_output();
        let ctx = self.facts;

        let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::clone(&key.0)]);
        let generation = ctx.current_project_generation();
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
        let result = single_entry_peek(storage.entries, key, ctx);
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
impl MemoPublish<'_, ImportedRegistryDb> {
    #[cfg(test)]
    pub(crate) fn inject_concurrent_publish_for_test(
        &self,
        key: ImportedRegistryKey,
        entry: Arc<ImportedRegistryEntry>,
    ) {
        self.db.insert_for_test(key, entry);
    }

    #[cfg(test)]
    pub(crate) fn peek(&self, key: &ImportedRegistryKey) -> Option<ImportedRegistryValue> {
        MemoRead::new(self.db, self.facts).peek(key)
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
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
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
impl MemoPublish<'_, DeclarationLookupDb> {
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
    pub(crate) fn get_or_compute<P, Prepare, F>(
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
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
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
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> ComputedEntry<ResolvedTypeDeclaration>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl MemoPublish<'_, ResolvabilityDb> {
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
    pub(crate) fn get_or_compute<P, Prepare, F>(
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
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
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
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> ComputedEntry<bool>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl MemoPublish<'_, OwnerCollectionDb> {
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
    pub(crate) fn get_or_compute<P, Prepare, F>(
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
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
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
        compute: F,
    ) -> Option<Option<verter_type_expr::locators::AuthoredBodyLocator>>
    where
        F: FnOnce() -> ComputedEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>,
    {
        self.get_or_compute(key, || (), |()| compute())
    }
}
impl MemoPublish<'_, ShapeCacheDb> {
    #[cfg(test)]
    pub(crate) fn peek(&self, key: &ShapeCacheKey) -> Option<MaterializedOutputTypeExpr> {
        MemoRead::new(self.db, self.facts).peek(key)
    }
    /// Open the sole production admission scope for this DB. The sealed
    /// capability passed to `f` is the only route to a Shape-cache write.
    pub(crate) fn with_owner_scope<'db, F, R>(&'db self, f: F) -> R
    where
        F: for<'t> FnOnce(ShapeCacheOwnerScope<'db, 't>) -> R,
    {
        let ctx = self.facts;

        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            |probe| {
                f(ShapeCacheOwnerScope {
                    db: self.db,
                    ctx,
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
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
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
        compute: F,
    ) -> Option<MaterializedOutputTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedOutputTypeExpr, Arc<[FactVersionRef]>)>,
    {
        self.with_owner_scope(|scope| scope.get_or_compute(key, compute))
    }
    /// Test-only sibling of [`Self::get_or_compute_traced_for_test`] for
    /// [`Self::admit_computed`].
    #[cfg(test)]
    pub(crate) fn admit_computed_traced_for_test(
        &self,
        key: &ShapeCacheKey,
        value: MaterializedOutputTypeExpr,
        fact_dep_signature: Arc<[FactVersionRef]>,
    ) -> MaterializedOutputTypeExpr {
        self.with_owner_scope(|scope| scope.admit_computed(key, value, fact_dep_signature))
    }
}
impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn imported_registry_read(&self) -> MemoRead<'_, ImportedRegistryDb> {
        MemoRead::new(self.binding.imported_registry.as_ref(), self.ctx)
    }
    pub(crate) fn imported_registry_publish(&self) -> MemoPublish<'_, ImportedRegistryDb> {
        MemoPublish::new(self.binding.imported_registry.as_ref(), self.ctx)
    }

    pub(crate) fn declaration_publish(&self) -> MemoPublish<'_, DeclarationLookupDb> {
        MemoPublish::new(self.binding.declarations.as_ref(), self.ctx)
    }

    pub(crate) fn resolvability_publish(&self) -> MemoPublish<'_, ResolvabilityDb> {
        MemoPublish::new(self.binding.resolvability.as_ref(), self.ctx)
    }

    pub(crate) fn owner_collection_publish(&self) -> MemoPublish<'_, OwnerCollectionDb> {
        MemoPublish::new(self.binding.owner_collections.as_ref(), self.ctx)
    }
    pub(crate) fn with_shape_scope<F, R>(&self, f: F) -> R
    where
        F: for<'t> FnOnce(ShapeCacheOwnerScope<'_, 't>) -> R,
    {
        MemoPublish::new(self.binding.shapes.as_ref(), self.ctx).with_owner_scope(f)
    }
}

impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn intern_resolved_identity(
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

impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn read_vue_surface(
        &self,
        key: &crate::framework::surface_store::FullKey<
            crate::typeinfo::framework_surface::VueSurfaceKey,
        >,
        generation: u64,
    ) -> Option<
        Arc<
            crate::framework::surface_store::StoredSurfaceDto<
                crate::typeinfo::framework_surface::MacroSurfaceDtos,
            >,
        >,
    > {
        read_framework_surface(
            self.binding.vue_surfaces.as_ref(),
            key,
            |facts| self.ctx.validates_fact_signature(facts),
            generation,
        )
    }
    pub(crate) fn publish_vue_surface(
        &self,
        key: crate::framework::surface_store::FullKey<
            crate::typeinfo::framework_surface::VueSurfaceKey,
        >,
        entry: crate::framework::surface_store::StoredSurfaceDto<
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    ) -> Arc<
        crate::framework::surface_store::StoredSurfaceDto<
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    > {
        self.binding.vue_surfaces.insert(key, entry)
    }
    pub(crate) fn read_svelte_surface(
        &self,
        key: &crate::framework::surface_store::FullKey<
            crate::typeinfo::framework_surface::SvelteSurfaceKey,
        >,
        generation: u64,
    ) -> Option<
        Arc<
            crate::framework::surface_store::StoredSurfaceDto<
                crate::typeinfo::framework_surface::MacroSurfaceDtos,
            >,
        >,
    > {
        read_framework_surface(
            self.binding.svelte_surfaces.as_ref(),
            key,
            |facts| self.ctx.validates_fact_signature(facts),
            generation,
        )
    }
    pub(crate) fn publish_svelte_surface(
        &self,
        key: crate::framework::surface_store::FullKey<
            crate::typeinfo::framework_surface::SvelteSurfaceKey,
        >,
        entry: crate::framework::surface_store::StoredSurfaceDto<
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    ) -> Arc<
        crate::framework::surface_store::StoredSurfaceDto<
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >,
    > {
        self.binding.svelte_surfaces.insert(key, entry)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn app_config_no_override_proof_get_or_compute(
        &self,
        key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
    ) -> Option<Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
        let ctx = self.ctx;
        let db = self.binding.app_config_proofs.as_ref();
        // Warm-hit peek — validate the cached fact_dep_signature against
        // the live store view. The peek bubbles the signature into any
        // active outer tracer on success.
        if let Some(entry) = self.app_config_proof_read(key) {
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
            ctx.observe(
                verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                    canonical_id: decl_canonical_for_compute.as_ref().to_string(),
                    hash: whole_hash,
                },
            );
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
        let (no_override, finalise) = crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            cold_body,
        );
        self.binding
            .observers
            .provenance
            .app_config_proof_fact_tracer_installs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // ReturnOnly never publishes — fenced-serve arm: a proof derived
        // from a served-without-publication artifact must not seal a
        // shared no-override entry whose facts validate against the live
        // view. Decline to publish; the consumer takes the slow path.
        match finalise {
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(
                fact_dep_signature,
            ) => {
                if !no_override {
                    // The file declares `interface AppConfig` — we
                    // cannot prove "no override" without walking the
                    // member set. Decline to publish; the fast-path
                    // consumer must take the slow path.
                    return None;
                }
                db.publish(key.clone(), Arc::clone(&fact_dep_signature));
                Some(Arc::new(
                    crate::app_config_proof_db::AppConfigNoOverrideProofEntry {
                        fact_dep_signature,
                    },
                ))
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(_) => {
                self.binding
                    .observers
                    .provenance
                    .app_config_proof_overflow_refusals
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Overflow => {
                self.binding
                    .observers
                    .provenance
                    .app_config_proof_overflow_refusals
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }
            // Refuses like an overflow and is counted as neither: the
            // overflow counter is a SIZE observable and a stability refusal
            // must not inflate it.
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::MutationUnstable => {
                None
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn produce_binder_identity_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<BinderIdentityFactsEntry>> {
        let ctx = self.ctx;
        let serve = ctx.ensure_indexed_ready_serve(canonical)?;
        let indexed = serve.indexed;
        let parse_stable_hash =
            crate::parse_stable_hash::compute_parse_stable_hash_inputs(&indexed);
        let key = BinderIdentityFactsKey {
            canonical: Arc::from(canonical),
            parse_stable_hash,
            parse_env_hash: indexed.parse_env_hash,
        };
        let store = self.binding.binder_facts.as_ref();
        if let Some(entry) = store.get(&key) {
            if entry
                .read_set_signature
                .validate_with_self_roots(ctx, std::slice::from_ref(&key.canonical))
            {
                // A warm hit must BUBBLE the entry's read-set into any
                // active outer tracer: an enclosing traced computation
                // admits its own value with THESE binder facts observed, so
                // it is invalidated when a pinned fact moves (the sibling
                // `AppConfigNoOverrideProofDb::peek` pattern).
                crate::fact_signature_helpers::bubble_fact_signature(
                    ctx,
                    &entry.read_set_signature.facts,
                );
                return Some(entry);
            }
            // Stale under the same key (a recorded fact moved): drop it so
            // the fresh recompute below wins future warm reads, then fall
            // through to the cold recompute, which re-pins the signature.
            store.remove(&key);
        }

        let indexed_for_body = Arc::clone(&indexed);
        let canonical_for_body = key.canonical.clone();
        let cold_body = move || -> (BinderIdentityFacts, bool) {
            let facts = project_binder_identity_facts_inputs(
                canonical_for_body.as_ref(),
                &indexed_for_body.shallow_state,
            );
            // Pin the eager parse-lane facts covering the artifact's
            // inputs against the OBSERVED content version. All are header
            // facts — no body-sensitive (`Export` / `LocalDecl` / `Member`)
            // fact is forced, so production lowers zero declaration bodies.
            let mut all_pinned = true;
            let mut pin = |fact_key: FactKey| {
                match crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                    ctx,
                    canonical_for_body.as_ref(),
                    indexed_for_body.whole_hash,
                    fact_key,
                    FactLane::Semantic,
                ) {
                    Some(fact_ref) => {
                        ctx.observe(
                            verter_session_query::facts::fact_cache::FactVersionRef::Parse(
                                fact_ref,
                            ),
                        );
                    }
                    None => {
                        all_pinned = false;
                    }
                }
            };
            pin(FactKey::SyntacticExportSet);
            // Whole-file scope-inventory set pins: a NEW augmentation target
            // (a first `declare module "m" {…}` in a file that had none) or
            // a new namespace block moves the signature even when the
            // parse-stable skeleton is unchanged (EMPTY blocks included).
            pin(FactKey::AugmentationTargetSet);
            pin(FactKey::NamespaceScopeSet);
            for seed in facts.decl_slots.iter() {
                pin(FactKey::MemberShape {
                    exporter: crate::file_artifact_store::InternedName::from(
                        seed.merged_symbol_name.as_ref(),
                    ),
                    space: fact_space(seed.symbol_space),
                });
            }
            // Order-sensitive contributor-sequence pins for every
            // file-surface slot (an overload-group reorder or a same-file
            // declaration swap moves this fact; a comment BETWEEN
            // declarations does not, so the cosmetic warm rate survives).
            for record in facts.declaration_order.iter() {
                pin(FactKey::DeclContributionOrder {
                    name: crate::file_artifact_store::InternedName::from(
                        record.seed.merged_symbol_name.as_ref(),
                    ),
                    owner: record.seed.owner,
                    space: fact_space(record.seed.symbol_space),
                });
            }
            // Per-record augmentation pins for BOTH `declare module`
            // and `declare global` contributions (global blocks key on the
            // `$global` sentinel specifier, the emission's own encoding).
            for record in facts.augmentation_contributions.iter() {
                let specifier = match &record.scope_kind {
                    AugmentationScopeKind::Global => {
                        crate::fact_emission::GLOBAL_AUGMENTATION_TAG.to_string()
                    }
                    AugmentationScopeKind::Module(specifier) => specifier.clone(),
                };
                pin(FactKey::ModuleAugmentation {
                    specifier: crate::file_artifact_store::InternedSpecifier::from(
                        specifier.as_str(),
                    ),
                    owner: record.owner,
                    augmented_name: crate::file_artifact_store::InternedName::from(
                        record.name.as_ref(),
                    ),
                    space: fact_space(record.symbol_space),
                });
            }
            // The per-target contribution SET + ORDER pins, derived from the
            // shallow walk's BLOCK inventory (every `declare module "X" {…}`
            // / `declare global {…}` block, EMPTY ones included): an empty
            // target pins its bare-target hash, so an empty →
            // first-contribution edit moves a pinned hash even when the
            // target set itself is unchanged. The scope-kind tag keeps
            // `declare global {…}` and `declare module "$global" {…}` in
            // DISTINCT target identities (never string-matched at consumers).
            let header_index = &indexed_for_body.shallow_state.headers;
            let mut augmentation_targets: Vec<(
                verter_session_query::facts::AugmentationScopeKindTag,
                String,
                verter_type_expr::TopLevelOwnerId,
            )> = Vec::new();
            for block in &header_index.augmentation_blocks {
                let (scope_kind_tag, specifier) = match &block.scope {
                    AugmentationScopeKind::Global => (
                        verter_session_query::facts::AugmentationScopeKindTag::Global,
                        crate::fact_emission::GLOBAL_AUGMENTATION_TAG.to_string(),
                    ),
                    AugmentationScopeKind::Module(specifier) => (
                        verter_session_query::facts::AugmentationScopeKindTag::Module,
                        specifier.clone(),
                    ),
                };
                let target = (scope_kind_tag, specifier, block.owner);
                if !augmentation_targets.contains(&target) {
                    augmentation_targets.push(target);
                }
            }
            for (scope_kind_tag, specifier, owner) in augmentation_targets {
                pin(FactKey::AugmentationContributionSet {
                    scope_kind_tag,
                    specifier: crate::file_artifact_store::InternedSpecifier::from(
                        specifier.as_str(),
                    ),
                    owner,
                });
                pin(FactKey::AugmentationContributionOrder {
                    scope_kind_tag,
                    specifier: crate::file_artifact_store::InternedSpecifier::from(
                        specifier.as_str(),
                    ),
                    owner,
                });
            }
            (facts, all_pinned)
        };
        let ((facts, all_pinned), finalise) = crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound_observers(
                &self.binding.observers.overflow,
                #[cfg(test)]
                &self.binding.observers.forcing,
            ),
            cold_body,
        );
        let facts = Arc::new(facts);
        // A fenced serve, an unrecoverable observed-version fact registry,
        // or a non-cacheable / overflowed read set never enters the shared
        // store — the fresh artifact is returned without admission.
        let admissible = serve.store_published && all_pinned;
        match finalise {
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(
                fact_dep_signature,
            ) if admissible => {
                let entry = Arc::new(BinderIdentityFactsEntry {
                    facts,
                    read_set_signature: ReadSetSignature::new(fact_dep_signature),
                });
                store.insert(key, Arc::clone(&entry));
                Some(entry)
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(
                fact_dep_signature,
            ) => Some(Arc::new(BinderIdentityFactsEntry {
                facts,
                read_set_signature: ReadSetSignature::new(fact_dep_signature),
            })),
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(_)
            | verter_session_query::facts::fact_read_set::FactReadSetFinalise::Overflow
            | verter_session_query::facts::fact_read_set::FactReadSetFinalise::MutationUnstable => {
                Some(Arc::new(BinderIdentityFactsEntry {
                    facts,
                    read_set_signature: ReadSetSignature::overflow(),
                }))
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
use crate::binder_identity_facts::{
    project_binder_identity_facts_inputs, BinderIdentityFacts, BinderIdentityFactsEntry,
    BinderIdentityFactsKey,
};
#[cfg(any(test, feature = "test-support"))]
use verter_session_query::{
    declarations::AugmentationScopeKind,
    facts::{FactKey, FactLane},
};
#[cfg(any(test, feature = "test-support"))]
fn fact_space(
    space: crate::semantic_query::SemanticSymbolSpace,
) -> verter_session_query::facts::SymbolSpace {
    match space {
        crate::semantic_query::SemanticSymbolSpace::Type => {
            verter_session_query::facts::SymbolSpace::Type
        }
        crate::semantic_query::SemanticSymbolSpace::Value => {
            verter_session_query::facts::SymbolSpace::Value
        }
        crate::semantic_query::SemanticSymbolSpace::Namespace => {
            verter_session_query::facts::SymbolSpace::Namespace
        }
    }
}
#[cfg(any(test, feature = "test-support"))]
impl super::ProjectSemanticDispatch<'_> {
    fn app_config_proof_read(
        &self,
        key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
    ) -> Option<Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
        let entry = self.binding.app_config_proofs.candidate(key)?;
        if !self.ctx.validates_fact_signature(&entry.fact_dep_signature) {
            return None;
        }
        self.ctx
            .observe_borrowed_signature(&entry.fact_dep_signature);
        Some(entry)
    }
}

/// Read a selected framework surface only after its generation and facts validate.
pub(crate) fn read_framework_surface<K, B, F>(
    store: &crate::framework::surface_store::FrameworkSurfaceStore<K, B>,
    key: &crate::framework::surface_store::FullKey<K>,
    validates: F,
    generation: u64,
) -> Option<Arc<crate::framework::surface_store::StoredSurfaceDto<B>>>
where
    K: Clone + Eq + std::hash::Hash + Send + Sync + 'static,
    B: Send + Sync + 'static,
    F: FnOnce(&[verter_session_query::facts::fact_cache::FactVersionRef]) -> bool,
{
    let candidate = store.candidate(key)?;
    if candidate.validated_at_generation != generation
        || !validates(&candidate.read_set_signature.facts)
    {
        return None;
    }
    store.touch(&key.canonical, key.owner_whole_hash);
    Some(candidate)
}

/// Validate an ordered candidate snapshot after all storage guards have dropped.
pub(crate) fn read_candidate<K, V, F>(
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
pub(crate) fn release_shape_canonical(
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

impl<'a, P: Send + Sync>
    MemoRead<
        'a,
        crate::component_meta_result_db::ComponentMetaResultDb<P>,
        &'a crate::MetaProvenance,
    >
{
    pub(crate) fn for_result(
        db: &'a crate::component_meta_result_db::ComponentMetaResultDb<P>,
        facts: &'a dyn FactValidation,
        observations: &'a crate::MetaProvenance,
    ) -> Self {
        Self {
            db,
            facts,
            observations,
        }
    }
    pub(crate) fn peek(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<Arc<ComponentMetaResultEntry<P>>> {
        let bump_miss = |observations: &crate::MetaProvenance| {
            observations
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
        if !self.db.is_current_schema() {
            bump_miss(self.observations);
            return None;
        }
        // Clone the candidate `Arc` out of the slot before validating —
        // a concurrent eviction cannot invalidate this borrow.
        let candidate = match self.db.candidate(key, owner_whole_hash) {
            Some(c) => c,
            None => {
                bump_miss(self.observations);
                return None;
            }
        };
        // Project-generation gate. The carrier validates only
        // file-content whole-hashes; a `ProjectGeneration` reset bumps
        // no file content, so an entry whose `validated_at_generation`
        // no longer equals the live generation is stale even though its
        // carrier still validates. Reject before the fact rail so the
        // miss is attributed correctly.
        if candidate.value.validated_at_generation != self.facts.current_project_generation() {
            bump_miss(self.observations);
            return None;
        }
        // Fact-precise validation: every entry in the signature must
        // validate under the live view. An empty signature trivially
        // passes (entries published outside an installed tracer scope —
        // typically test fixtures — fall through to the legacy validator
        // on the caller side).
        if !self
            .facts
            .validates_fact_signature(&candidate.value.read_set_signature.facts)
        {
            bump_miss(self.observations);
            return None;
        }
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.cache_counters
                .component_meta
                .hits
                .fetch_add(1, Ordering::Relaxed);
        }
        self.observations
            .component_meta_result_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        Some(Arc::new(candidate.value.clone()))
    }
}

pub(crate) fn record_component_meta_result_miss(observations: &crate::MetaProvenance) {
    observations
        .component_meta_result_cache_misses
        .fetch_add(1, Ordering::Relaxed);
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.cache_counters
            .component_meta
            .misses
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl<P: Send + Sync> MemoPublish<'_, crate::component_meta_result_db::ComponentMetaResultDb<P>> {
    pub(crate) fn compute_and_admit_with_entry<R, Compute, Decide>(
        &self,
        canonical: &str,
        path_label: &str,
        compute: Compute,
        decide: Decide,
    ) -> (R, Option<AdmittedComponentMetaResult<P>>)
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> ComponentMetaPublishDecision<P>,
        P: verter_session_query::retention::RetainedFootprint,
    {
        let (value, read_set) = crate::resolver_core::resolver_context::with_fact_tracer_cell(
            verter_session_query::facts::fact_cache::AggregateGenerations::from_seed(
                &verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
                &self.facts.aggregate_clock_reader().live(),
            ),
            |_cell| compute(),
        );
        let finalise = read_set.finalise();
        let mut admitted = None;
        match finalise {
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(facts) => {
                match decide(&value) {
                    ComponentMetaPublishDecision::Publish {
                        key,
                        owner_whole_hash,
                        payload,
                        validated_at_generation,
                    } => {
                        let admitted_facts =
                            crate::component_meta_result_db::strip_owner_route_fact(
                                &key.owner_canonical,
                                &facts,
                            );
                        let entry = Arc::new(ComponentMetaResultEntry {
                            payload,
                            read_set_signature:
                                verter_session_query::facts::fact_cache::ReadSetSignature::new(
                                    admitted_facts,
                                ),
                            validated_at_generation,
                        });
                        // A retention refusal leaves `admitted` unset: the
                        // caller keeps its complete value, and no evidence
                        // carrier claims an entry the cache never stored.
                        if self.db.publish_core(
                            key.clone(),
                            owner_whole_hash,
                            entry.as_ref().clone(),
                        ) {
                            admitted = Some(AdmittedComponentMetaResult {
                                key,
                                owner_whole_hash,
                                entry,
                            });
                        }
                    }
                    ComponentMetaPublishDecision::ReturnOnly(reason) => {
                        crate::cache_runtime::admission::propagate_non_admission(reason);
                        tracing::debug!(
                            target: "verter::audit::record",
                            file = %canonical,
                            path = %path_label,
                            reason = %reason,
                            "skipping component-meta cache promotion: typed admission refusal",
                        );
                    }
                    ComponentMetaPublishDecision::NoValue => {}
                }
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(_) => {
                let reason = crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance;
                crate::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: cold compute consumed a non-cacheable read",
                );
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Overflow => {
                let reason = crate::cache_runtime::NonAdmissionReason::SignatureOverflow;
                crate::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: fact-signature overflowed cap",
                );
            }
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::MutationUnstable => {
                let reason = crate::cache_runtime::NonAdmissionReason::MutationUnstable;
                crate::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: a compaction domain advanced \
                     mid-compute",
                );
            }
        }
        (value, admitted)
    }
}

impl<P: Send + Sync> MemoPublish<'_, crate::component_meta_result_db::ComponentMetaResultDb<P>> {
    pub(crate) fn compute_and_admit<R, Compute, Decide>(
        &self,
        canonical: &str,
        path_label: &str,
        compute: Compute,
        decide: Decide,
    ) -> R
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> ComponentMetaPublishDecision<P>,
        P: verter_session_query::retention::RetainedFootprint,
    {
        self.compute_and_admit_with_entry(canonical, path_label, compute, decide)
            .0
    }
}

impl super::ProjectSemanticDispatch<'_> {
    pub(crate) fn component_meta_result_read(
        &self,
    ) -> MemoRead<
        '_,
        crate::component_meta_result_db::ComponentMetaResultDb<
            crate::component_meta_result_db::CachedComponentMetaResult,
        >,
        &crate::MetaProvenance,
    > {
        MemoRead::for_result(
            self.binding.component_meta_results.as_ref(),
            self.ctx,
            &self.binding.observers.provenance,
        )
    }
    pub(crate) fn component_meta_result_publish(
        &self,
    ) -> MemoPublish<
        '_,
        crate::component_meta_result_db::ComponentMetaResultDb<
            crate::component_meta_result_db::CachedComponentMetaResult,
        >,
    > {
        MemoPublish::new(self.binding.component_meta_results.as_ref(), self.ctx)
    }
}

use crate::component_meta_result_db::{
    AdmittedComponentMetaResult, ComponentMetaPublishDecision, ComponentMetaResultEntry,
    ComponentMetaResultKey,
};
use verter_session_query::analysis::types::Hash16;
