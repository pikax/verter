//! Node arena — structurally interning, sharded dedup, stable ids.
//!
//! The arena pairs each interned [`SemanticNodeData`] with an **origin-scope
//! sidecar**. Both the node vec and the parallel scope
//! vec live inside one `RwLock<ArenaInner>` so reads (`node_data`,
//! `node_scope`) are concurrent while writes (intern-miss) serialize.
//!
//! **Structural interning.** Two callers that construct the same
//! `SemanticNodeData::Primitive(Number)` in the same scope share one
//! [`SemanticNodeId`] — preventing the semantic graph from growing
//! unbounded under repeated structural construction. Cross-scope
//! same-payload interns stay distinct.
//!
//! **Fingerprint-narrowed dedup index.** Each `(payload, scope)` pair is
//! reduced to a 64-bit [`structural_fingerprint`] once, up front, outside
//! any lock. The per-shard dedup index is keyed by that fingerprint —
//! `u64 -> bucket of candidates` — so intern lookups hash a single `u64`
//! rather than re-walking the whole payload (a `SurfaceView` with every
//! member + span) on the map probe. The fingerprint only NARROWS: each
//! bucket holds the handful of nodes whose fingerprint collided, and a
//! per-bucket content `Eq` over `(payload, scope)` is the identity
//! authority. Two nodes intern to the same id **iff** they are structurally
//! and scope equal (spans, accessibility, and `NodeScopeId` all included),
//! so a fingerprint collision can never alias two distinct nodes — it only
//! lengthens a bucket scan.
//!
//! **Single payload allocation.** On an intern-miss the payload is boxed
//! into one `Arc` that is shared by refcount between the dense arena vec
//! (`ArenaInner::nodes`, indexed by `id.0`) and the dedup bucket. The index
//! holds an `Arc` handle, never a deep clone of the payload — so the graph
//! carries exactly one copy of each node's body.
//!
//! **HashDoS-safe fingerprint.** Node payloads embed workspace-derived
//! names (file paths, type names). A seedless hash would let an attacker
//! craft names that force every node into one fingerprint bucket
//! (`O(n)` per intern). The fingerprint therefore hashes through a
//! process-seeded SipHash ([`RandomState`]), and the bucket map itself
//! uses the std `RandomState` hasher — never `FxHash`. Fingerprints never
//! cross a process / serialization boundary (they key only the in-memory,
//! per-generation dedup index), so a per-process random seed is correct.
//!
//! **Sharded dedup index.** The dedup index lives on
//! `[Mutex<ShardIndex>; NUM_SHARDS]` rather than inside `ArenaInner`.
//! The fingerprint's low bits route to a specific shard; intern-hits
//! (the steady-state hot path) take only that shard's Mutex — so `K`
//! threads interning payloads that route to distinct shards proceed
//! in parallel. Intern-misses acquire the shard Mutex, then briefly
//! acquire `inner.write()` to allocate the next sequential id and
//! push the node. Storage stays global and dense so `id.0 as usize`
//! indexing + `a.0 + 1 == b.0` serial-id invariant are preserved.
//!
//! Dispatch builders query the sidecar via [`super::SemanticGraphStore::node_scope`]
//! to route per-base-scope lookups through the correct
//! [`SessionSolverHost`](crate::resolver_core::solver_host::SessionSolverHost)
//! without threading scope through every call.
//!
//! **Release (document close).** The id space is append-only — a
//! `SemanticNodeId` is a raw `u64` index with no generation tag, and ids are
//! retained outside this arena (shape-cache keys, the member-ordinal sidecar,
//! relation proofs, materialised provenance), so an id is NEVER reused. What
//! a close reclaims is the PAYLOAD: [`NodeArena::release_canonical`]
//! tombstones every node whose origin scope is the closed canonical, plus
//! every node whose payload embeds a released id (children are interned
//! before their parents, so a parent of a released node is dead too — no
//! live node ever embeds a released id, except the sealed
//! `DeferredCallable` carrier whose parts are unreadable here). A
//! tombstoned slot drops its `Arc` payload and its scope, leaves the dedup
//! index, and resolves through [`NodeArena::get`] to one shared
//! `Opaque(Miss)` placeholder so a stale holder degrades to an unresolved
//! value instead of an invalid-id fault.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::{Arc, OnceLock};

use smallvec::SmallVec;

use crate::instant::Instant;
use crate::semantic_query::{NodeScopeId, SemanticNodeData, SemanticNodeId};

pub(super) const NUM_SHARDS: usize = 16;
pub(super) const SHARD_MASK: u64 = (NUM_SHARDS as u64) - 1;

/// One entry in a fingerprint bucket: the shared payload `Arc` (the SAME
/// allocation stored in [`ArenaInner::nodes`]), its origin scope, and the
/// interned id. The payload and scope are retained so the bucket scan can
/// content-`Eq` a query against candidates without touching `inner`.
type NodeCandidate = (Arc<SemanticNodeData>, NodeScopeId, SemanticNodeId);

/// Per-shard dedup index. Keyed by the structural fingerprint of
/// `(payload, scope)`; each bucket holds the candidate nodes whose
/// fingerprint routed here. A lookup content-`Eq`s the query
/// `(payload, scope)` against the (usually single) candidate to confirm
/// identity — the fingerprint only narrows.
///
/// The bucket map uses the std [`RandomState`] (SipHash) hasher, **not**
/// `FxHash`: workspace-derived names in the payload feed the fingerprint,
/// so a predictable hash would be a HashDoS vector.
#[derive(Default)]
pub(super) struct ShardIndex {
    index: HashMap<u64, SmallVec<[NodeCandidate; 1]>>,
}

impl ShardIndex {
    /// Return the interned id for the node structurally + scope equal to
    /// `(data, scope)` within this shard, or `None`. The `fingerprint`
    /// already selected the bucket; the per-candidate content `Eq` is the
    /// identity authority that keeps a fingerprint collision from aliasing
    /// two distinct nodes.
    fn lookup(
        &self,
        fingerprint: u64,
        data: &SemanticNodeData,
        scope: &NodeScopeId,
    ) -> Option<SemanticNodeId> {
        let bucket = self.index.get(&fingerprint)?;
        bucket.iter().find_map(|(cand_data, cand_scope, cand_id)| {
            (cand_scope == scope && cand_data.as_ref() == data).then_some(*cand_id)
        })
    }
}

/// Interior state of [`NodeArena`]. Held behind an `RwLock` so reads of
/// `(nodes, scopes)` (non-hot-path) are concurrent while the allocating
/// intern-miss path serializes on the writer.
#[derive(Default)]
pub(super) struct ArenaInner {
    /// Dense payload storage indexed by `id.0`. `None` is a RELEASED slot:
    /// the id was handed out, its payload was dropped by
    /// [`NodeArena::release_canonical`], and the id is never reused.
    nodes: Vec<Option<Arc<SemanticNodeData>>>,
    /// Origin-scope sidecar. Index-aligned with `nodes`.
    /// `Some(scope)` records the scope the node was first interned in
    /// (`Global` for scope-less structural nodes, `File { .. }` for
    /// declaration-origin nodes). `None` for a released slot.
    scopes: Vec<Option<NodeScopeId>>,
    /// Number of `Some` payload slots — `nodes.len()` minus the released
    /// slots. Maintained on every push and release so the live count is
    /// O(1).
    live: usize,
}

/// Process-global seed for structural fingerprints. A single
/// [`RandomState`] captured once per process — SipHash-quality and randomly
/// seeded so workspace-derived names carried in node payloads cannot be
/// used to force fingerprint (and thus bucket) collisions. Fingerprints are
/// purely an in-memory, per-generation interning optimisation and never
/// cross a process / serialization boundary, so a per-process random seed
/// is correct.
fn fingerprint_seed() -> &'static RandomState {
    static SEED: OnceLock<RandomState> = OnceLock::new();
    SEED.get_or_init(RandomState::new)
}

/// Structural fingerprint of `(data, scope)` — the 64-bit narrowing key for
/// the dedup bucket index. Hashes the FULL structural identity (payload
/// incl. spans, plus origin scope) through the node's own [`Hash`] impl, so
/// equal `(data, scope)` pairs always fingerprint equal and land in the
/// same bucket (Hash/Eq consistency is load-bearing for dedup). It only
/// narrows candidates; per-bucket content `Eq` is the identity authority,
/// so a fingerprint collision never aliases two distinct nodes.
///
/// Because it routes through [`fingerprint_seed`], the fingerprint of a
/// given payload is stable within a process run but unpredictable across
/// runs — the HashDoS defense.
fn structural_fingerprint(data: &SemanticNodeData, scope: &NodeScopeId) -> u64 {
    let mut hasher = fingerprint_seed().build_hasher();
    data.hash(&mut hasher);
    scope.hash(&mut hasher);
    hasher.finish()
}

/// Whether a payload binds `canonical_id`'s CONTENT identity regardless of
/// the scope it was interned under: a `DeclRef` / `InstantiationRef` /
/// `TypeParam` whose `DeclIdentity` (canonical + whole hash) names it, a
/// `DeclPlaceholder` refusal for one of its declarations, a `typeof` root
/// or nominal identity in it, a bare reference captured in its scope, a
/// surface whose members / index signatures were DECLARED in it, a
/// signature occurring in it, a sealed callable served from it, or a
/// synthetic binding rooted in it.
///
/// A consumer re-lowering an import from the closed document interns
/// such a node under the CONSUMER's scope, with the closed content's whole
/// hash inside the identity; it embeds no node id, so neither the scope
/// root rule nor the child cascade reaches it, yet it can never be reached
/// again (the reload mints a new hash → a new identity → a new node). One
/// such node per closed content version is exactly the per-cycle growth
/// a close-heavy session shows. The match carries no wildcard: a new
/// variant must be dispositioned here.
fn payload_binds_canonical(data: &SemanticNodeData, canonical_id: &str) -> bool {
    use crate::semantic_query::{ObjectConstructionEffect, QueryError, SignatureReturnCarrier};
    use verter_type_expr::facts::FunctionReturnSource;

    let names = |canonical: &Arc<str>| canonical.as_ref() == canonical_id;
    let declared_in = |origin: &Option<Arc<str>>| origin.as_deref() == Some(canonical_id);
    match data {
        SemanticNodeData::DeclRef { identity } => names(&identity.canonical_id),
        SemanticNodeData::InstantiationRef { base, .. } => names(&base.canonical_id),
        SemanticNodeData::TypeParam { decl, .. } => names(&decl.canonical_id),
        SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
            canonical_id: refused,
            ..
        }) => names(refused),
        SemanticNodeData::Opaque(_) => false,
        SemanticNodeData::Object(view) => {
            view.positive_members()
                .iter()
                .any(|member| declared_in(&member.declaration_origin))
                || view
                    .index_signatures
                    .iter()
                    .any(|signature| declared_in(&signature.declaration_origin))
        }
        SemanticNodeData::ObjectSpreadProgram(program) => {
            program.effects.iter().any(|effect| match effect {
                ObjectConstructionEffect::DirectProperty(effect) => {
                    declared_in(&effect.declaration_origin)
                }
                ObjectConstructionEffect::DirectMethod(effect) => {
                    declared_in(&effect.declaration_origin)
                }
                ObjectConstructionEffect::DirectGet(effect)
                | ObjectConstructionEffect::DirectSet(effect) => {
                    declared_in(&effect.declaration_origin)
                }
                ObjectConstructionEffect::DirectIndex(effect) => {
                    declared_in(&effect.declaration_origin)
                }
                ObjectConstructionEffect::DirectCall(_)
                | ObjectConstructionEffect::DirectConstruct(_)
                | ObjectConstructionEffect::Spread(_) => false,
            })
        }
        SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_) => {
            data.typeof_head()
                .is_some_and(|(root, _)| names(&root.scope.canonical_id))
                || data
                    .typeof_nominal_identity()
                    .is_some_and(|identity| names(&identity.canonical_id))
        }
        SemanticNodeData::BareRef(_) => data.bare_ref_head().is_some_and(|(_, scope)| {
            scope
                .canonical_file()
                .is_some_and(|captured| names(&captured))
        }),
        SemanticNodeData::Signature {
            occurrence,
            return_carrier,
            ..
        } => {
            occurrence
                .as_ref()
                .is_some_and(|occurrence| names(&occurrence.function.anchor.canonical_id))
                || match return_carrier {
                    SignatureReturnCarrier::Declared(_) => false,
                    SignatureReturnCarrier::Function(source) => match source {
                        FunctionReturnSource::Declared(locator) => {
                            names(&locator.slot().anchor.canonical_id)
                        }
                        FunctionReturnSource::Flow(identity) => {
                            names(&identity.anchor.canonical_id)
                        }
                        FunctionReturnSource::Absent => false,
                    },
                }
        }
        SemanticNodeData::DeferredCallable(callable) => names(callable.declaring_canonical()),
        SemanticNodeData::SyntheticBinding { id, .. } => names(&id.scope_canonical_id),
        // A module specifier is not a canonical; an infer binder's identity
        // is private and scope-bound (its scope root rule applies); the
        // rest hold no declaration identity at all.
        SemanticNodeData::ImportType(_)
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::InferRef { .. }
        | SemanticNodeData::Alias(_)
        | SemanticNodeData::Union(_)
        | SemanticNodeData::Intersection(_)
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Array { .. }
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::MergedDecl { .. }
        | SemanticNodeData::RawFallback { .. }
        | SemanticNodeData::IntrinsicApplication { .. } => false,
    }
}

/// Deterministic shard routing for a `(data, scope)` pair — the low bits of
/// its structural fingerprint. Test-only: production interning computes the
/// fingerprint once in [`NodeArena::intern_with_fingerprint`] and derives
/// the shard from it directly, never re-walking the payload here.
#[cfg(test)]
pub(super) fn shard_index_for(data: &SemanticNodeData, scope: &NodeScopeId) -> usize {
    (structural_fingerprint(data, scope) & SHARD_MASK) as usize
}

pub(super) struct NodeArena {
    /// Global dense storage for node data + sidecar. `RwLock` so readers
    /// (`get`, `scope`) are concurrent and writers (intern-miss) briefly
    /// serialize to push a fresh slot.
    inner: parking_lot::RwLock<ArenaInner>,
    /// Sharded dedup indexes. Each shard owns the fingerprint-range whose
    /// low bits land on it.
    shards: [parking_lot::Mutex<ShardIndex>; NUM_SHARDS],
    /// Optional contention instrumentation. When present,
    /// `push_impl` records per-call counters and `inner.write()`
    /// wait time so downstream passes have evidence-grade contention
    /// data. `None` for test-default arenas constructed via
    /// `Default::default()`.
    pub(super) provenance: Option<Arc<crate::types::MetaProvenance>>,
    /// The ONE payload every released slot resolves to through [`Self::get`]
    /// — an `Opaque(Miss)` node ("this value's resolution answered
    /// nothing"), never in the dedup index. Mirrors the
    /// `SemanticGraphRead::node_data` fabrication for an unknown id, so a
    /// consumer still holding a released id reads an unresolved value.
    released_placeholder: Arc<SemanticNodeData>,
}

impl Default for NodeArena {
    fn default() -> Self {
        Self {
            inner: parking_lot::RwLock::new(ArenaInner::default()),
            shards: std::array::from_fn(|_| parking_lot::Mutex::new(ShardIndex::default())),
            provenance: None,
            released_placeholder: Arc::new(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::Miss,
            )),
        }
    }
}

impl NodeArena {
    /// Intern `data` with the `Global` scope tag. Helper intermediates and
    /// purely structural nodes use this path — most existing interning
    /// sites fall into this bucket.
    pub(super) fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.push_impl(data, NodeScopeId::Global)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Called by
    /// builders that know the declaration origin — `build_resolve_decl`,
    /// `build_typeof`, `build_instantiate`, etc.
    pub(super) fn push_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.push_impl(data, scope)
    }

    fn push_impl(&self, mut data: SemanticNodeData, scope: NodeScopeId) -> SemanticNodeId {
        // TS literal-type identity boundary: the checker interns numeric
        // literal types by SameValueZero, so `-0` IS the literal type `0`
        // (verified against the pinned checker: `0 & -0` is `0`, `0 | -0`
        // is one arm, `-0` is assignable to `0`). Payload equality below is
        // bit identity, which diverges from that exactly at the signed
        // zero — normalize it here, the single intern choke point, so the
        // two spellings share one node and no downstream comparison,
        // dedup, or disjointness proof can tell them apart. NaN payloads
        // pass through untouched (bit identity already groups them).
        if let SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(n)) = &mut data
        {
            if *n == 0.0 && n.is_sign_negative() {
                *n = 0.0;
            }
        }
        // Fingerprint once, up front, outside every lock. Both shard
        // routing and the bucket key derive from this single hash of the
        // payload; the map probe then only hashes a `u64`.
        let fingerprint = structural_fingerprint(&data, &scope);
        self.intern_with_fingerprint(data, scope, fingerprint)
    }

    /// Core intern path. `fingerprint` narrows to a shard + bucket; the
    /// per-bucket content `Eq` over `(data, scope)` is the identity
    /// authority. Split out from [`push_impl`] so tests can force a
    /// fingerprint — driving two distinct payloads into one bucket — and
    /// assert the content-`Eq` split (collision safety).
    fn intern_with_fingerprint(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        fingerprint: u64,
    ) -> SemanticNodeId {
        // Capture the variant bucket before moving `data` so the
        // contention instrumentation can bucket per-variant pushes.
        let discriminant = data.node_tag().bucket_index();
        let shard_idx = (fingerprint & SHARD_MASK) as usize;

        // Sharded dedup hot path. The fingerprint routes to its shard and
        // the bucket scan checks for an existing id; the miss path acquires
        // `inner.write()` briefly to push the new slot.
        let (id, is_miss, write_wait_ns) = {
            let timing_on = verter_scheduler::request_context::current_timing_enabled();
            // Fast path: shard-hit. Shard Mutex is short-lived; parallel
            // across shards.
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let shard = self.shards[shard_idx].lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
            if let Some(existing) = shard.lookup(fingerprint, &data, &scope) {
                (existing, false, 0u64)
            } else {
                drop(shard);
                // Miss: re-acquire the shard (to serialize concurrent
                // misses for the same key on this shard) and then
                // briefly acquire inner.write() to allocate.
                let lock_start = if timing_on {
                    Some(Instant::now())
                } else {
                    None
                };
                let mut shard = self.shards[shard_idx].lock();
                let lock_wait = lock_start
                    .map(|t| t.elapsed())
                    .unwrap_or(std::time::Duration::ZERO);
                crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
                if let Some(existing) = shard.lookup(fingerprint, &data, &scope) {
                    // Another thread beat us to it.
                    (existing, false, 0u64)
                } else {
                    let write_start = Instant::now();
                    let mut inner = self.inner.write();
                    let wait = write_start.elapsed().as_nanos() as u64;
                    let id = SemanticNodeId(inner.nodes.len() as u64);
                    // ONE payload allocation, shared by refcount between the
                    // dense arena storage and the dedup bucket — the payload
                    // is never deep-cloned into the index.
                    let payload = Arc::new(data);
                    inner.nodes.push(Some(Arc::clone(&payload)));
                    inner.scopes.push(Some(scope.clone()));
                    inner.live += 1;
                    drop(inner);
                    shard
                        .index
                        .entry(fingerprint)
                        .or_default()
                        .push((payload, scope, id));
                    (id, true, wait)
                }
            }
        };

        if let Some(prov) = self.provenance.as_ref() {
            use std::sync::atomic::Ordering::Relaxed;
            prov.node_arena_pushes.fetch_add(1, Relaxed);
            if is_miss {
                prov.node_arena_intern_miss.fetch_add(1, Relaxed);
            }
            prov.node_arena_inner_write_wait_ns
                .fetch_add(write_wait_ns, Relaxed);
            // In range by construction: the array is sized to the tag bound.
            prov.node_arena_pushes_per_discriminant[discriminant].fetch_add(1, Relaxed);
        }

        id
    }

    /// Read the payload for `id`. `None` for an id this arena never handed
    /// out; the shared `Opaque(Miss)` placeholder for a RELEASED id (see
    /// [`Self::release_canonical`]); the interned payload otherwise.
    pub(super) fn get(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        let inner = self.inner.read();
        match inner.nodes.get(id.0 as usize) {
            Some(Some(payload)) => Some(Arc::clone(payload)),
            Some(None) => Some(Arc::clone(&self.released_placeholder)),
            None => None,
        }
    }

    /// Whether `id` names a slot that still holds its interned payload —
    /// `false` for an id never handed out and for a released slot.
    pub(super) fn is_live(&self, id: SemanticNodeId) -> bool {
        let inner = self.inner.read();
        matches!(inner.nodes.get(id.0 as usize), Some(Some(_)))
    }

    /// Return the recorded origin scope for `id` — `None` for invalid
    /// ids and released slots, `Some(scope)` for everything else.
    pub(super) fn scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        let inner = self.inner.read();
        inner.scopes.get(id.0 as usize).cloned().flatten()
    }

    /// Number of id slots ever allocated — the append-only id space,
    /// INCLUDING released slots. Equals [`Self::live_len`] until the first
    /// release.
    pub(super) fn len(&self) -> usize {
        self.inner.read().nodes.len()
    }

    /// Number of slots that still hold a payload — the retained node set.
    pub(super) fn live_len(&self) -> usize {
        self.inner.read().live
    }

    /// Release every node the closed `canonical_id` retained: the nodes
    /// whose origin scope is `File { canonical_id, .. }`, the sealed
    /// `DeferredCallable` carriers whose served position is declared in
    /// it, and — transitively — every node whose payload embeds a released
    /// id (a parent of a dead node can never be reached again: its dedup
    /// key names an id that is never re-minted, and the memo entries that
    /// held it are drained by the caller). Global-scope nodes are released
    /// ONLY through that cascade; a scope-less node that embeds no released
    /// id (a primitive, a shared literal) stays.
    ///
    /// Each released slot drops its payload `Arc` and scope, leaves the
    /// dedup index (so a re-intern of the same `(payload, scope)` mints a
    /// fresh id), and keeps its id — see the module docs for why ids are
    /// never reused. Returns the released ids in ascending order.
    ///
    /// **Lock order.** The dead set is computed under `inner.read()`, the
    /// dedup entries are dropped shard by shard with NO `inner` lock held
    /// (the intern-miss path holds a shard mutex and THEN takes
    /// `inner.write()`, so this method never nests the two the other way),
    /// and only then are the payloads dropped under `inner.write()`. That
    /// order also means no dedup hit can ever hand out an id whose payload
    /// is already gone.
    ///
    /// **Cost.** One pass over the live slots (plus a confirming pass) when
    /// the canonical owns at least one node; an early return otherwise. A
    /// close is a user-driven, rare event, so the O(nodes) scan is paid
    /// there rather than as a per-intern reverse index.
    pub(super) fn release_canonical(&self, canonical_id: &str) -> Vec<SemanticNodeId> {
        let mut released: Vec<(SemanticNodeId, u64)> = Vec::new();
        {
            let inner = self.inner.read();
            let slot_count = inner.nodes.len();
            let mut dead: Vec<bool> = vec![false; slot_count];
            let is_root = |index: usize| -> bool {
                match (&inner.nodes[index], &inner.scopes[index]) {
                    (None, _) => false,
                    (
                        Some(_),
                        Some(NodeScopeId::File {
                            canonical_id: c, ..
                        }),
                    ) if c.as_ref() == canonical_id => true,
                    (Some(payload), _) => payload_binds_canonical(payload, canonical_id),
                }
            };
            let mut any_root = false;
            for (index, is_dead) in dead.iter_mut().enumerate().take(slot_count) {
                if is_root(index) {
                    *is_dead = true;
                    any_root = true;
                }
            }
            if !any_root {
                return Vec::new();
            }
            // Cascade to every live node embedding a dead id. Children are
            // interned before their parents, so one ascending pass settles
            // the common case; the loop re-runs until a pass changes
            // nothing, which also covers any parent interned out of order.
            loop {
                let mut changed = false;
                for index in 0..slot_count {
                    if dead[index] {
                        continue;
                    }
                    let Some(payload) = inner.nodes[index].as_ref() else {
                        continue;
                    };
                    let mut embeds_dead = false;
                    let _walk = payload.for_each_child(|child| {
                        if let Some(true) = dead.get(child.0 as usize) {
                            embeds_dead = true;
                        }
                    });
                    if embeds_dead {
                        dead[index] = true;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            for (index, is_dead) in dead.iter().enumerate() {
                if !*is_dead {
                    continue;
                }
                let (Some(payload), Some(scope)) = (&inner.nodes[index], &inner.scopes[index])
                else {
                    continue;
                };
                released.push((
                    SemanticNodeId(index as u64),
                    structural_fingerprint(payload, scope),
                ));
            }
        }
        if released.is_empty() {
            return Vec::new();
        }
        // Drop the dedup entries FIRST, shard by shard, so no intern can
        // hit a released node once its payload is gone.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let mut per_shard: Vec<Vec<(u64, SemanticNodeId)>> = vec![Vec::new(); NUM_SHARDS];
        for (id, fingerprint) in &released {
            per_shard[(fingerprint & SHARD_MASK) as usize].push((*fingerprint, *id));
        }
        for (shard_index, victims) in per_shard.into_iter().enumerate() {
            if victims.is_empty() {
                continue;
            }
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let mut shard = self.shards[shard_index].lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
            for (fingerprint, id) in victims {
                let Some(bucket) = shard.index.get_mut(&fingerprint) else {
                    continue;
                };
                bucket.retain(|(_, _, cand_id)| *cand_id != id);
                if bucket.is_empty() {
                    shard.index.remove(&fingerprint);
                }
            }
        }
        // Now drop the payloads. A slot already released by a concurrent
        // call is skipped so `live` stays exact.
        let mut inner = self.inner.write();
        let mut ids: Vec<SemanticNodeId> = Vec::with_capacity(released.len());
        for (id, _) in released {
            let index = id.0 as usize;
            if inner.nodes[index].take().is_some() {
                inner.scopes[index] = None;
                inner.live -= 1;
                ids.push(id);
            }
        }
        ids
    }

    /// Drop shard-dedup entries for the given canonical id.
    /// Invariant: invalidation does NOT drop `NodeScopeId::Global`
    /// — only `File { canonical_id: c, .. }` matches. Entries keyed at
    /// any other `File` canonical also survive.
    ///
    /// **Architectural property: the underlying arena Vec is
    /// append-only.** Existing `SemanticNodeId`s remain valid and
    /// resolve to the same payload via `get`/`scope`; this method
    /// affects only the dedup-shard's view of "next intern of this
    /// `(payload, scope)` pair returns the existing id". After
    /// invalidation, a re-intern of the same `(payload, File{c})`
    /// pair allocates a fresh node slot (and thus a fresh id),
    /// guaranteeing freshness against the changed canonical's content
    /// generation. The arena's dense node / scope storage is never
    /// shrunk: `SemanticNodeId` is a raw `u64` index with no generation
    /// tag, so reclaiming the id space would require a generational
    /// `SemanticNodeId` redesign.
    ///
    /// Touches every shard mutex once. Each shard's retain walk is
    /// O(shard size). When `node_arena_lock_acquisitions` is wired
    /// into the audit context, each shard lock acquisition is recorded.
    pub(super) fn invalidate_for_canonical(&self, canonical_id: &str) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        for shard in self.shards.iter() {
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let mut shard = shard.lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
            shard.index.retain(|_fingerprint, bucket| {
                bucket.retain(|(_, scope, _)| match scope {
                    // Invariant: Global scope is never dropped on invalidation.
                    NodeScopeId::Global => true,
                    NodeScopeId::File {
                        canonical_id: c, ..
                    } => c.as_ref() != canonical_id,
                });
                // Drop now-empty buckets so the fingerprint index stays dense.
                !bucket.is_empty()
            });
        }
    }

    /// Test-only: assert the dedup bucket holding `id` shares the SAME
    /// `Arc` allocation as the dense arena vec — i.e. the payload was
    /// interned once and shared by refcount, never deep-cloned into the
    /// index. Returns `false` if `id` has no dense slot or no bucket entry.
    #[cfg(test)]
    pub(super) fn debug_bucket_shares_arena_arc(&self, id: SemanticNodeId) -> bool {
        let arena_arc = match self.inner.read().nodes.get(id.0 as usize) {
            Some(Some(arc)) => Arc::clone(arc),
            _ => return false,
        };
        for shard in self.shards.iter() {
            let shard = shard.lock();
            for bucket in shard.index.values() {
                for (cand_arc, _scope, cand_id) in bucket.iter() {
                    if *cand_id == id {
                        return Arc::ptr_eq(cand_arc, &arena_arc);
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod arena_intern_tests {
    use super::*;
    use crate::semantic_query::PrimitiveKind;

    fn file_scope(canonical: &str) -> NodeScopeId {
        NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [7u8; 16],
            local_scope: None,
        }
    }

    /// Hash/`Eq` consistency under the seeded fingerprint: two `Eq`
    /// payloads MUST fingerprint identically, else they would land in
    /// different buckets and dedup would silently break. Discriminating
    /// against a fingerprint inconsistent with the node's `Eq`.
    #[test]
    fn equal_nodes_share_fingerprint() {
        let a = SemanticNodeData::Primitive(PrimitiveKind::String);
        let b = SemanticNodeData::Primitive(PrimitiveKind::String);
        assert_eq!(a, b);
        assert_eq!(
            structural_fingerprint(&a, &NodeScopeId::Global),
            structural_fingerprint(&b, &NodeScopeId::Global),
            "structurally-equal nodes must fingerprint equal (Hash/Eq consistency)",
        );
    }

    /// Collision-bucket authority. Two structurally DISTINCT payloads,
    /// forced into ONE fingerprint bucket via the intern seam, must NOT
    /// alias: the fingerprint only narrows, the per-bucket content `Eq`
    /// decides identity. Discriminating against dropping the content-`Eq`
    /// (returning the first candidate) from the collision path.
    #[test]
    fn collision_bucket_disambiguates_distinct_payloads() {
        let arena = NodeArena::default();
        let a = SemanticNodeData::Primitive(PrimitiveKind::String);
        let b = SemanticNodeData::Primitive(PrimitiveKind::Number);
        let forced_fp = 0x00C0_FFEE_u64;

        let id_a = arena.intern_with_fingerprint(a.clone(), NodeScopeId::Global, forced_fp);
        let id_b = arena.intern_with_fingerprint(b.clone(), NodeScopeId::Global, forced_fp);
        assert_ne!(
            id_a, id_b,
            "distinct payloads sharing one fingerprint bucket must get distinct ids (no aliasing)",
        );

        // Re-intern of each SAME payload into the SAME (collided) bucket
        // still dedups to its own id.
        let id_a2 = arena.intern_with_fingerprint(a, NodeScopeId::Global, forced_fp);
        let id_b2 = arena.intern_with_fingerprint(b, NodeScopeId::Global, forced_fp);
        assert_eq!(id_a, id_a2, "same payload in a collided bucket must dedup");
        assert_eq!(id_b, id_b2, "same payload in a collided bucket must dedup");

        // Each id resolves to its OWN payload (not the bucket-neighbour's).
        assert!(matches!(
            *arena.get(id_a).unwrap(),
            SemanticNodeData::Primitive(PrimitiveKind::String)
        ));
        assert!(matches!(
            *arena.get(id_b).unwrap(),
            SemanticNodeData::Primitive(PrimitiveKind::Number)
        ));
    }

    /// Scope is part of identity even inside a collided bucket. The SAME
    /// payload at DIFFERENT scopes, forced into one bucket, must not alias.
    /// Discriminating against dropping the scope compare from the bucket
    /// content-`Eq`.
    #[test]
    fn collision_bucket_distinguishes_by_scope() {
        let arena = NodeArena::default();
        let payload = SemanticNodeData::Primitive(PrimitiveKind::Boolean);
        let forced_fp = 0x0000_ABCD_u64;

        let id_global =
            arena.intern_with_fingerprint(payload.clone(), NodeScopeId::Global, forced_fp);
        let id_file = arena.intern_with_fingerprint(payload, file_scope("/w/a.ts"), forced_fp);
        assert_ne!(
            id_global, id_file,
            "same payload at different scopes in one bucket must get distinct ids (scope is identity)",
        );
    }

    /// The dedup index shares the arena's payload `Arc` rather than a deep
    /// clone — the graph-RSS win. Discriminating against re-introducing an
    /// `Arc::new(data.clone())` into the bucket.
    #[test]
    fn payload_stored_once_shared_arc() {
        let arena = NodeArena::default();
        let id = arena.push(SemanticNodeData::Primitive(PrimitiveKind::String));
        assert!(
            arena.debug_bucket_shares_arena_arc(id),
            "dedup index must share the arena's payload Arc, not a deep clone",
        );
    }

    /// Append-only id stability: interning is dense + sequential, and a
    /// re-intern of the same `(payload, scope)` returns the existing id
    /// (no renumbering, no duplicate allocation).
    #[test]
    fn ids_are_dense_and_stable() {
        let arena = NodeArena::default();
        let a = arena.push(SemanticNodeData::Primitive(PrimitiveKind::String));
        let b = arena.push(SemanticNodeData::Primitive(PrimitiveKind::Number));
        assert_eq!(a.0 + 1, b.0, "ids allocate densely and sequentially");
        let a_again = arena.push(SemanticNodeData::Primitive(PrimitiveKind::String));
        assert_eq!(a, a_again, "re-intern returns the existing id");
        assert_eq!(arena.len(), 2, "dedup does not allocate a new slot");
    }
}
