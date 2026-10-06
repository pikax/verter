//! Node arena — structurally interning, sharded dedup, stable ids.
//!
//! The arena pairs each interned [`SemanticNodeData`] with an **origin-scope
//! sidecar**. Both live in chunked slot storage inside one
//! `RwLock<ArenaInner>` so reads (`node_data`, `node_scope`) are concurrent
//! while writes (intern-miss) serialize.
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
//! into one `Arc` that is shared by refcount between the arena's slot
//! storage (`ArenaInner`, addressed by `id.0`) and the dedup bucket. The index
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
//! push the node. Ids are handed out sequentially (`a.0 + 1 == b.0`) and
//! address the chunked storage described under **Storage**.
//!
//! **Acyclic by contract.** A payload names children the arena already
//! holds: every child id is below the id the payload is interned at, so the
//! node graph is a DAG and every structural walk over it terminates without
//! cycle detection. A payload naming a child the arena could still allocate
//! (an id at or above the new node's, below [`UNALLOCATABLE_ID_FLOOR`]) is a
//! forward reference, the only way to close a cycle; it interns as the typed
//! `Opaque(ForeignSemanticOperand)` refusal instead, never as its payload.
//! Ids from the floor up are never allocated (binder tokens, sentinels), so
//! they can dangle but never close a cycle.
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
//! live node ever embeds a released id; the sealed `DeferredCallable`
//! carrier's parts are walked through its topology-only visitor). A
//! tombstoned slot drops its `Arc` payload and its scope, leaves the dedup
//! index, and resolves through [`NodeArena::get`] to one shared
//! `Opaque(Miss)` placeholder so a stale holder degrades to an unresolved
//! value instead of an invalid-id fault.
//!
//! **Storage is chunked, so released slots actually disappear.** Slots live
//! in fixed-size chunks of [`CHUNK_LEN`] consecutive ids, keyed by
//! `id >> CHUNK_BITS`. A release drops a slot's payload and scope, and a chunk
//! whose every slot has been released is dropped whole, so the memory the
//! arena holds follows the LIVE node set rather than the number of ids ever
//! handed out: an editing session that closes and reopens documents mints
//! fresh ids forever but keeps a bounded number of chunks. An id whose chunk
//! is gone reads as released, exactly like a tombstoned slot in a live chunk,
//! and an id at or past the next id to hand out was never allocated. A chunk
//! stays while any one of its slots is live (a long-lived node interned
//! alongside churned ones pins its chunk), so the retained storage is bounded
//! by the live set times the chunk size, never by history.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::{Arc, OnceLock};

use smallvec::SmallVec;

use crate::instant::Instant;
use crate::semantic_query::{NodeScopeId, SemanticNodeData, SemanticNodeId};

/// Node ids from here up are never allocated: they name non-arena operands
/// (signature-kernel binder tokens live at bit 63) or absent-node sentinels,
/// so a payload naming one can dangle but never close a cycle.
pub const UNALLOCATABLE_ID_FLOOR: u64 = 1 << 62;

/// Whether `data`, interned at `id`, names a child the arena could allocate
/// at or after `id`: a forward reference, the only way to close a cycle.
fn names_forward_child(data: &SemanticNodeData, id: SemanticNodeId) -> bool {
    let mut forward = false;
    let mut check = |child: SemanticNodeId| {
        forward |= child.0 >= id.0 && child.0 < UNALLOCATABLE_ID_FLOOR;
    };
    data.for_each_retained_child(&mut check);
    forward
}

/// Test builds: every node id `data`'s `Debug` form prints is one
/// [`SemanticNodeData::for_each_retained_child`] visits (or an
/// unallocatable one). Read off the printed payload, independently of the
/// walk, so a variant or field the walk forgets fails the first test that
/// interns it, rather than leaving a releasing holder to keep the node it
/// names alive. The visited ids are sorted once and each printed id found
/// by a binary search, so a wide payload (a union of hundreds of members)
/// costs its size, not its size squared.
#[cfg(any(test, feature = "test-support"))]
fn assert_retained_walk_is_complete(data: &SemanticNodeData) {
    const MARK: &str = "SemanticNodeId(";
    let printed = format!("{data:?}");
    if !printed.contains(MARK) {
        return;
    }
    let mut retained: SmallVec<[u64; 16]> = SmallVec::new();
    data.for_each_retained_child(|child| retained.push(child.0));
    retained.sort_unstable();
    let mut rest = printed.as_str();
    while let Some(at) = rest.find(MARK) {
        rest = &rest[at + MARK.len()..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(held) = rest[..digits].parse::<u64>() else {
            continue;
        };
        assert!(
            held >= UNALLOCATABLE_ID_FLOOR || retained.binary_search(&held).is_ok(),
            "a {:?} payload holds node {held}, which for_each_retained_child never visits",
            data.node_tag()
        );
    }
}

pub const NUM_SHARDS: usize = 16;
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

/// Ids per storage chunk (see the module docs, **Storage**). Small enough
/// that one long-lived node pins little: a chunk is ~24 KiB of slot storage.
pub(super) const CHUNK_BITS: u32 = 8;
pub(super) const CHUNK_LEN: usize = 1 << CHUNK_BITS;
const CHUNK_MASK: u64 = (CHUNK_LEN as u64) - 1;

/// Storage for `CHUNK_LEN` consecutive ids. Index-aligned `nodes` / `scopes`:
/// `Some` is a live slot, `None` a released one (or, in the chunk still
/// receiving ids, one not handed out yet — [`ArenaInner::next_id`] tells).
struct Chunk {
    nodes: Box<[Option<Arc<SemanticNodeData>>]>,
    scopes: Box<[Option<NodeScopeId>]>,
    /// Slots of this chunk still holding a payload; the chunk is dropped
    /// when it reaches zero.
    live: usize,
}

impl Chunk {
    fn empty() -> Self {
        Self {
            nodes: (0..CHUNK_LEN).map(|_| None).collect(),
            scopes: (0..CHUNK_LEN).map(|_| None).collect(),
            live: 0,
        }
    }
}

/// What an id addresses.
enum Slot<'a> {
    /// Never handed out.
    Unallocated,
    /// Handed out and released (its chunk may be gone).
    Released,
    Live(&'a Arc<SemanticNodeData>, &'a NodeScopeId),
}

/// Interior state of [`NodeArena`]. Held behind an `RwLock` so reads
/// (non-hot-path) are concurrent while the allocating intern-miss path
/// serializes on the writer.
#[derive(Default)]
pub(super) struct ArenaInner {
    /// Live chunks keyed by `id >> CHUNK_BITS`. A chunk is created by the first
    /// id allocated into it and dropped by the release of its last live slot.
    chunks: rustc_hash::FxHashMap<u64, Chunk>,
    /// The next id to hand out: every id below it was allocated exactly once.
    next_id: u64,
    /// Live slots across every chunk, maintained on every allocation and
    /// release so the live count is O(1).
    live: usize,
}

impl ArenaInner {
    fn slot(&self, id: SemanticNodeId) -> Slot<'_> {
        if id.0 >= self.next_id {
            return Slot::Unallocated;
        }
        let Some(chunk) = self.chunks.get(&(id.0 >> CHUNK_BITS)) else {
            return Slot::Released;
        };
        let index = (id.0 & CHUNK_MASK) as usize;
        match (&chunk.nodes[index], &chunk.scopes[index]) {
            (Some(payload), Some(scope)) => Slot::Live(payload, scope),
            _ => Slot::Released,
        }
    }

    /// Hand out the next id for `(payload, scope)`.
    fn allocate(&mut self, payload: Arc<SemanticNodeData>, scope: NodeScopeId) -> SemanticNodeId {
        let id = SemanticNodeId(self.next_id);
        self.next_id += 1;
        let chunk = self
            .chunks
            .entry(id.0 >> CHUNK_BITS)
            .or_insert_with(Chunk::empty);
        let index = (id.0 & CHUNK_MASK) as usize;
        chunk.nodes[index] = Some(payload);
        chunk.scopes[index] = Some(scope);
        chunk.live += 1;
        self.live += 1;
        id
    }

    /// Release `id`'s slot; `true` when it held a payload. Drops the chunk
    /// when that was its last live slot.
    fn release(&mut self, id: SemanticNodeId) -> bool {
        let key = id.0 >> CHUNK_BITS;
        let Some(chunk) = self.chunks.get_mut(&key) else {
            return false;
        };
        let index = (id.0 & CHUNK_MASK) as usize;
        if chunk.nodes[index].take().is_none() {
            return false;
        }
        chunk.scopes[index] = None;
        chunk.live -= 1;
        self.live -= 1;
        if chunk.live == 0 {
            self.chunks.remove(&key);
        }
        true
    }

    /// Every live `(id, payload, scope)` in ascending id order.
    fn live_slots(&self) -> impl Iterator<Item = (u64, &Arc<SemanticNodeData>, &NodeScopeId)> {
        let mut keys: Vec<u64> = self.chunks.keys().copied().collect();
        keys.sort_unstable();
        keys.into_iter().flat_map(move |key| {
            let chunk = &self.chunks[&key];
            chunk
                .nodes
                .iter()
                .zip(chunk.scopes.iter())
                .enumerate()
                .filter_map(move |(index, (node, scope))| match (node, scope) {
                    (Some(payload), Some(scope)) => {
                        Some(((key << CHUNK_BITS) | index as u64, payload, scope))
                    }
                    _ => None,
                })
        })
    }
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
        // A class expression's instance is the class authored in `canonical_id`;
        // its surface node is reached by the child cascade.
        SemanticNodeData::ClassExpressionInstance { identity, .. } => names(&identity.canonical_id),
        // An enum member's literal type is the enum declared in `canonical_id`;
        // the value it stands for (`base`) is reached by the child cascade.
        SemanticNodeData::EnumLiteral(literal) => names(&literal.enum_decl.canonical_id),
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
#[cfg(any(test, feature = "test-support"))]
pub fn shard_index_for(data: &SemanticNodeData, scope: &NodeScopeId) -> usize {
    (structural_fingerprint(data, scope) & SHARD_MASK) as usize
}

pub(crate) struct NodeArena {
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
    pub(super) provenance: Option<Arc<crate::engine_provenance::EngineProvenance>>,
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
            let timing_on = verter_execution::request_context::current_timing_enabled();
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
            crate::request_observers::record_node_arena_lock_acquisition(lock_wait);
            if let Some(existing) = shard.lookup(fingerprint, &data, &scope) {
                (existing, false, 0u64)
            } else {
                drop(shard);
                #[cfg(any(test, feature = "test-support"))]
                assert_retained_walk_is_complete(&data);
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
                crate::request_observers::record_node_arena_lock_acquisition(lock_wait);
                if let Some(existing) = shard.lookup(fingerprint, &data, &scope) {
                    // Another thread beat us to it.
                    (existing, false, 0u64)
                } else {
                    let write_start = Instant::now();
                    let mut inner = self.inner.write();
                    let wait = write_start.elapsed().as_nanos() as u64;
                    // The id this payload would take. Ids are monotonic and
                    // never reused (storage is chunked, not a vector), so the
                    // next id to hand out is the allocation point the
                    // acyclicity rule reads; a released child sits below it
                    // and is no forward reference.
                    let id = SemanticNodeId(inner.next_id);
                    if names_forward_child(&data, id) {
                        drop(inner);
                        drop(shard);
                        return self.push_impl(
                            SemanticNodeData::Opaque(
                                crate::semantic_query::QueryError::ForeignSemanticOperand,
                            ),
                            scope,
                        );
                    }
                    // ONE payload allocation, shared by refcount between the
                    // arena's slot storage and the dedup bucket — the payload
                    // is never deep-cloned into the index.
                    let payload = Arc::new(data);
                    let id = inner.allocate(Arc::clone(&payload), scope.clone());
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
        match inner.slot(id) {
            Slot::Live(payload, _) => Some(Arc::clone(payload)),
            Slot::Released => Some(Arc::clone(&self.released_placeholder)),
            Slot::Unallocated => None,
        }
    }

    /// Whether `id` names a slot that still holds its interned payload —
    /// `false` for an id never handed out and for a released slot.
    pub(super) fn is_live(&self, id: SemanticNodeId) -> bool {
        let inner = self.inner.read();
        matches!(inner.slot(id), Slot::Live(..))
    }

    /// Return the recorded origin scope for `id` — `None` for invalid
    /// ids and released slots, `Some(scope)` for everything else.
    pub(super) fn scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        let inner = self.inner.read();
        match inner.slot(id) {
            Slot::Live(_, scope) => Some(scope.clone()),
            _ => None,
        }
    }

    /// Number of ids ever handed out — the append-only id space, INCLUDING
    /// released ids. Equals [`Self::live_len`] until the first release. This
    /// is an id count, not storage: see [`Self::storage_slots`].
    pub(super) fn len(&self) -> usize {
        self.inner.read().next_id as usize
    }

    /// Slots the arena physically holds right now: every live chunk's
    /// [`CHUNK_LEN`]. Bounded by the live set (times the chunk size), unlike
    /// [`Self::len`]; the figure the retention snapshot reports as storage.
    pub(super) fn storage_slots(&self) -> usize {
        self.inner.read().chunks.len() * CHUNK_LEN
    }

    /// Number of slots that still hold a payload — the retained node set.
    pub(super) fn live_len(&self) -> usize {
        self.inner.read().live
    }

    /// Release every node the closed `canonical_id` retained: the nodes
    /// whose origin scope is `File { canonical_id, .. }`, the sealed
    /// `DeferredCallable` carriers whose served position is declared in
    /// it, and — transitively — every node whose payload embeds a released
    /// id, sealed and pending parts included
    /// ([`SemanticNodeData::for_each_retained_child`])
    /// (a parent of a dead node can never be reached again: its dedup
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
    pub(super) fn release_canonical(&self, canonical_id: &str, below: u64) -> Vec<SemanticNodeId> {
        let mut released: Vec<(SemanticNodeId, u64)> = Vec::new();
        {
            let inner = self.inner.read();
            let mut dead: rustc_hash::FxHashSet<u64> = rustc_hash::FxHashSet::default();
            // Only nodes interned before the close are the closed content's;
            // a node at or past `below` belongs to what the reload interned.
            for (id, payload, scope) in inner.live_slots() {
                if id >= below {
                    continue;
                }
                let is_root = match scope {
                    NodeScopeId::File {
                        canonical_id: c, ..
                    } if c.as_ref() == canonical_id => true,
                    _ => payload_binds_canonical(payload, canonical_id),
                };
                if is_root {
                    dead.insert(id);
                }
            }
            if dead.is_empty() {
                return Vec::new();
            }
            // Cascade to every live node embedding a dead id. Children are
            // interned before their parents, so one ascending pass settles
            // the common case; the loop re-runs until a pass changes
            // nothing, which also covers any parent interned out of order.
            loop {
                let mut changed = false;
                for (id, payload, _) in inner.live_slots() {
                    if dead.contains(&id) {
                        continue;
                    }
                    let mut embeds_dead = false;
                    // Every id the payload retains, not only its semantic
                    // children: a sealed callable's parts, a recursive
                    // reference's arguments, a class expression's prototype
                    // and a pending conditional's binders are each interned
                    // unscoped, one per distinct payload, so an id of the
                    // closed document would otherwise keep one such node per
                    // content version.
                    payload.for_each_retained_child(|child| {
                        embeds_dead |= dead.contains(&child.0);
                    });
                    if embeds_dead {
                        dead.insert(id);
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            for (id, payload, scope) in inner.live_slots() {
                if dead.contains(&id) {
                    released.push((SemanticNodeId(id), structural_fingerprint(payload, scope)));
                }
            }
        }
        if released.is_empty() {
            return Vec::new();
        }
        // Drop the dedup entries FIRST, shard by shard, so no intern can
        // hit a released node once its payload is gone.
        let timing_on = verter_execution::request_context::current_timing_enabled();
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
            crate::request_observers::record_node_arena_lock_acquisition(lock_wait);
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
        // Now drop the payloads (and any chunk that empties). A slot already
        // released by a concurrent call is skipped so `live` stays exact.
        let mut inner = self.inner.write();
        let mut ids: Vec<SemanticNodeId> = Vec::with_capacity(released.len());
        for (id, _) in released {
            if inner.release(id) {
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
    /// **Architectural property: the id space is append-only.** Existing
    /// `SemanticNodeId`s remain valid and resolve to the same payload via
    /// `get`/`scope`; this method affects only the dedup-shard's view of
    /// "next intern of this `(payload, scope)` pair returns the existing
    /// id". After invalidation, a re-intern of the same `(payload, File{c})`
    /// pair allocates a fresh id, guaranteeing freshness against the changed
    /// canonical's content generation. Payloads are dropped only by
    /// [`Self::release_canonical`], whose chunked storage is what keeps the
    /// arena's memory bounded (see the module docs, **Storage**).
    ///
    /// Touches every shard mutex once. Each shard's retain walk is
    /// O(shard size). When `node_arena_lock_acquisitions` is wired
    /// into the audit context, each shard lock acquisition is recorded.
    pub(super) fn invalidate_for_canonical(&self, canonical_id: &str) {
        let timing_on = verter_execution::request_context::current_timing_enabled();
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
            crate::request_observers::record_node_arena_lock_acquisition(lock_wait);
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
        let arena_arc = match self.inner.read().slot(id) {
            Slot::Live(arc, _) => Arc::clone(arc),
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

    /// The acyclicity rule refuses a payload naming a child at or above the
    /// id the payload would take. With chunked storage that id is the
    /// monotonic counter, not a slot count: after a close dropped whole
    /// chunks the storage is smaller than the ids handed out, a payload over
    /// a node interned before it is still interned as itself, and one naming
    /// an id the arena has not handed out is refused. Discriminating: read
    /// against the storage size, the rule would refuse every payload over a
    /// live node whose id exceeds the slots currently stored.
    #[test]
    fn the_acyclicity_rule_reads_the_next_id_not_the_storage_size() {
        use crate::semantic_query::{LiteralValue, QueryError};
        let arena = NodeArena::default();
        for n in 0..(CHUNK_LEN * 3 + 8) {
            let _ = arena.push_with_scope(
                SemanticNodeData::Literal(LiteralValue::Number(n as f64)),
                file_scope("/closed.ts"),
            );
        }
        let kept = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(-1.0)),
            file_scope("/kept.ts"),
        );
        let _ = arena.release_canonical("/closed.ts", u64::MAX);
        assert!(
            (kept.0 as usize) >= arena.storage_slots(),
            "fixture: the kept node's id is past the slots still stored ({} vs {})",
            kept.0,
            arena.storage_slots()
        );
        let over_kept = arena.push(SemanticNodeData::Array {
            element: kept,
            readonly: false,
        });
        assert!(
            matches!(
                arena.get(over_kept).as_deref(),
                Some(SemanticNodeData::Array { .. })
            ),
            "a payload over a node interned before it is interned as itself"
        );
        let not_handed_out = SemanticNodeId(arena.len() as u64 + 5);
        let refused = arena.push(SemanticNodeData::Array {
            element: not_handed_out,
            readonly: false,
        });
        assert!(
            matches!(
                arena.get(refused).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::ForeignSemanticOperand))
            ),
            "a payload naming an id not handed out yet is a forward reference"
        );
    }

    /// A recursive reference is interned unscoped and carries its type
    /// arguments inside an `Opaque` payload the child walk skips. Closing
    /// the document an argument belongs to releases the reference with it;
    /// a reference over arguments of another document stays.
    /// Discriminating: without the cascade's explicit arm the reference
    /// survives the close, one per content version of the closed document.
    #[test]
    fn a_close_releases_the_recursive_references_over_its_nodes() {
        use crate::semantic_query::{LiteralValue, QueryError};
        let arena = NodeArena::default();
        let closed = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let recursive_over = |arg: SemanticNodeId| {
            arena.push(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                name: Arc::from("Tree"),
                args: Arc::from([arg]),
            }))
        };
        let over_closed = recursive_over(closed);
        let over_kept = recursive_over(kept);
        let released = arena.release_canonical("/closed.ts", u64::MAX);
        assert!(released.contains(&closed));
        assert!(
            released.contains(&over_closed),
            "the reference over the closed document's node goes with it"
        );
        assert!(arena.is_live(kept));
        assert!(
            arena.is_live(over_kept),
            "a reference over a live node stays"
        );
    }

    /// A generic callable declared in one document and instantiated with a
    /// parameter type from another is interned unscoped as a sealed
    /// `DeferredCallable` whose served position stays in its declaring
    /// document. Closing the document the substituted type belongs to
    /// releases the callable over it (through a parameter or through a
    /// binder bound), and a parent naming the callable goes with it; a
    /// callable over a live type stays.
    /// Discriminating: the cascade read only `for_each_child`, which answers
    /// `Sealed` for this variant without visiting its children, and the root
    /// check reads only the declaring canonical, so both callables and the
    /// parent survived the close still naming a released id.
    #[test]
    fn a_close_releases_the_sealed_callables_over_its_nodes() {
        use crate::semantic_query::{
            DeferredCallable, FunctionParam, LiteralValue, SignatureKind, SignatureNodeOccurrence,
            SignatureReturnCarrier, TypeParamDecl,
        };
        use verter_type_expr::facts::{
            FlowFunctionReturnIdentity, FunctionPartIdentity, FunctionReturnSource,
        };
        use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

        let arena = NodeArena::default();
        let closed = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let binder = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(3.0)),
            file_scope("/declaring.ts"),
        );
        let occurrence = SignatureNodeOccurrence {
            function: FlowFunctionReturnIdentity {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from("/declaring.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from("f"),
                    space: LocatorSymbolSpace::Value,
                },
                function_part: FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            },
            signature_ordinal: 0,
        };
        let callable = |param: SemanticNodeId, constraint: Option<SemanticNodeId>| {
            arena.push(SemanticNodeData::DeferredCallable(
                DeferredCallable::from_parts_for_tests(
                    SignatureKind::Call,
                    Arc::from([FunctionParam::synthetic(
                        Some(Arc::from("x")),
                        param,
                        false,
                        false,
                    )]),
                    Arc::from([TypeParamDecl {
                        name: Arc::from("T"),
                        param: binder,
                        constraint,
                        default: None,
                        is_const: false,
                    }]),
                    occurrence.clone(),
                    SignatureReturnCarrier::Function(FunctionReturnSource::Absent),
                ),
            ))
        };
        let over_closed_param = callable(closed, None);
        let over_closed_bound = callable(kept, Some(closed));
        let over_kept = callable(kept, None);
        let parent = arena.push(SemanticNodeData::Array {
            element: over_closed_param,
            readonly: false,
        });

        let released = arena.release_canonical("/closed.ts", u64::MAX);

        assert!(released.contains(&closed));
        assert!(
            released.contains(&over_closed_param),
            "a callable whose parameter type was released goes with it"
        );
        assert!(
            released.contains(&over_closed_bound),
            "a callable whose binder bound was released goes with it"
        );
        assert!(
            released.contains(&parent),
            "a parent of a released callable goes with it"
        );
        assert!(arena.is_live(over_kept), "a callable over live types stays");
        assert!(arena.is_live(binder));
    }

    /// Substituting a binder through a deferred conditional appends the
    /// `(param, arg)` pair to its pending frame whether or not a branch
    /// mentions the binder, so the frame can name a released binder while
    /// the check, extends and branches are all live. Closing the binder's
    /// document releases that conditional; one whose frame names only live
    /// nodes stays.
    /// Discriminating: the child walk visits a pending frame's arguments but
    /// never its parameters, so the conditional survived the close naming a
    /// released binder, one per content version of the closed document.
    #[test]
    fn a_close_releases_the_conditionals_whose_pending_frame_binds_its_nodes() {
        use crate::semantic_query::{ConditionalPendingSubstitution, LiteralValue};

        let arena = NodeArena::default();
        let closed = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let bound = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(3.0)),
            file_scope("/kept.ts"),
        );
        let conditional = |param: SemanticNodeId| {
            arena.push(SemanticNodeData::Conditional {
                check: kept,
                extends: kept,
                true_branch_ref: kept,
                false_branch_ref: kept,
                distributive: false,
                pending: Some(Arc::new(
                    ConditionalPendingSubstitution::empty().append_both(param, bound),
                )),
            })
        };
        let binds_closed = conditional(closed);
        let binds_kept = conditional(kept);

        let released = arena.release_canonical("/closed.ts", u64::MAX);

        assert!(
            released.contains(&binds_closed),
            "a conditional whose pending frame binds a released node goes with it"
        );
        assert!(arena.is_live(binds_kept));
    }

    /// A class expression instance declared in a kept document whose
    /// recorded prototype is a closed document's node is released with it,
    /// and one whose prototype is kept stays. Discriminating: the child walk
    /// treats the prototype as a derived record beside the instance, not a
    /// part of it, so only the retention walk reaches it.
    #[test]
    fn a_close_releases_the_class_expressions_whose_prototype_is_its_node() {
        use crate::semantic_query::{ClassExpressionIdentity, LiteralValue};

        let arena = NodeArena::default();
        let closed = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.push_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let instance = |prototype: SemanticNodeId| {
            arena.push(SemanticNodeData::ClassExpressionInstance {
                identity: Arc::new(ClassExpressionIdentity {
                    canonical_id: Arc::from("/kept.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    offset: 0,
                    name: Arc::from("C"),
                    outer_clauses: Arc::from([]),
                    own_arity: 0,
                    constructor_visibility: None,
                    prototype: Some(prototype),
                    object_literal: false,
                }),
                type_arguments: Arc::from([]),
                surface: kept,
            })
        };
        let over_closed = instance(closed);
        let over_kept = instance(kept);

        let released = arena.release_canonical("/closed.ts", u64::MAX);

        assert!(
            released.contains(&over_closed),
            "an instance whose prototype is a released node goes with it"
        );
        assert!(arena.is_live(over_kept));
    }

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

    /// Releasing every node of a chunk drops the chunk, so the storage the
    /// arena holds follows the live set, not the ids ever handed out. Ids
    /// keep counting up and a released id reads as released either way.
    ///
    /// Discriminating: with the previous dense `Vec` storage, `storage_slots`
    /// grew with every cycle exactly like `len` does here.
    #[test]
    fn released_chunks_are_dropped_while_ids_keep_counting() {
        let arena = NodeArena::default();
        let pinned = arena.push(SemanticNodeData::Primitive(PrimitiveKind::String));
        let per_cycle = CHUNK_LEN + 3;
        let mut last: Option<SemanticNodeId> = None;
        for cycle in 0..40u64 {
            let canonical = "/w/churn.ts";
            let mut minted = Vec::with_capacity(per_cycle);
            for n in 0..per_cycle {
                let node = SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(
                    (cycle * per_cycle as u64 + n as u64) as f64,
                ));
                minted.push(arena.push_with_scope(node, file_scope(canonical)));
            }
            let released = arena.release_canonical(canonical, u64::MAX);
            assert_eq!(
                released.len(),
                per_cycle,
                "cycle {cycle}: every minted node is released"
            );
            assert!(
                arena.storage_slots() <= 2 * CHUNK_LEN,
                "cycle {cycle}: at most the pinned chunk and the chunk still receiving ids stay ({} slots)",
                arena.storage_slots()
            );
            assert_eq!(
                arena.live_len(),
                1,
                "cycle {cycle}: only the pinned node is live"
            );
            assert_eq!(
                arena.len(),
                1 + (cycle as usize + 1) * per_cycle,
                "cycle {cycle}: ids are never reused"
            );
            if let Some(previous) = last {
                assert!(
                    minted[0].0 > previous.0,
                    "cycle {cycle}: ids keep counting up"
                );
            }
            last = minted.last().copied();
            assert!(
                Arc::ptr_eq(&arena.get(minted[0]).unwrap(), &arena.released_placeholder),
                "cycle {cycle}: a released id reads as the placeholder even after its chunk is gone"
            );
            assert!(!arena.is_live(minted[0]));
            assert!(arena.scope(minted[0]).is_none());
        }
        assert!(
            arena.is_live(pinned),
            "the long-lived node pins its chunk and stays live"
        );
        assert!(
            arena.get(SemanticNodeId(u64::MAX)).is_none(),
            "an id never handed out is unknown"
        );
    }
}
