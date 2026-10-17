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
//! **Single payload allocation.** The arena slot owns the payload Arc.
//! Dedup candidates hold Weak handles and verify slot liveness while
//! acquiring a root under the arena lock. The index never keeps a graph
//! node alive after its last owner releases it in the controlled harness.
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
//! **Sharded dedup index.** Fingerprints route to one of
//! `[Mutex<ShardIndex>; NUM_SHARDS]`. A hit holds that shard and the arena
//! write lock while checking the candidate's live slot and acquiring its
//! root. A miss takes locks in the same order to allocate and retain its
//! children. Last release drops the arena lock before retiring weak shard
//! entries, so no inverse lock order is introduced. Ids are sequential and
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
//!
//! **Storage is charged.** The arena's backing is a liveness obligation of
//! the graph — refusing it would refuse the computation — so it is pinned
//! against the store's retention account through
//! [`GranularPin`](verter_session_query::retention::granular::GranularPin)s:
//! every live chunk's slot storage and table entry, every live payload's
//! `Arc` block, and each dedup shard's table and spilled buckets, charged
//! from the moment they are allocated until they are freed.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::{Arc, OnceLock, Weak};

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
/// names alive. Both multisets are sorted once and compared, so a wide
/// payload (a union of hundreds of members) costs its size, not its size
/// squared.
#[cfg(any(test, feature = "test-support"))]
fn assert_retained_walk_is_complete(data: &SemanticNodeData) {
    let mut retained = SmallVec::<[u64; 16]>::new();
    data.for_each_retained_child(|child| retained.push(child.0));
    assert_retained_multiset(data, retained);
}

#[cfg(any(test, feature = "test-support"))]
fn assert_retained_multiset(data: &SemanticNodeData, mut retained: SmallVec<[u64; 16]>) {
    use std::fmt::Write as _;
    let mut printed = PrintedNodeIds::default();
    write!(printed, "{data:?}").expect("a payload's Debug form writes");
    let mut expected = printed.ids;
    // This carrier predates typed node ids and prints its retained child as
    // a bare ordinal. Read the field directly, independently of the visitor.
    if let SemanticNodeData::SyntheticBinding { value_node, .. } = data {
        if *value_node < UNALLOCATABLE_ID_FLOOR {
            expected.push(*value_node);
        }
    }
    retained.retain(|held| *held < UNALLOCATABLE_ID_FLOOR);
    sort_small(&mut retained);
    sort_small(&mut expected);
    assert_eq!(
        retained,
        expected,
        "retained operand multiset for {:?}",
        data.node_tag()
    );
}

/// Sort a node-id multiset in place. Payloads name a handful of children,
/// which a plain insertion sort orders without the slice sort's machinery;
/// a wide one (a union of hundreds of members) takes the slice sort.
#[cfg(any(test, feature = "test-support"))]
fn sort_small(ids: &mut SmallVec<[u64; 16]>) {
    if ids.len() > 16 {
        ids.sort_unstable();
        return;
    }
    let ids = ids.as_mut_slice();
    let mut next = 1;
    while next < ids.len() {
        let held = ids[next];
        let mut at = next;
        while at > 0 && ids[at - 1] > held {
            ids[at] = ids[at - 1];
            at -= 1;
        }
        ids[at] = held;
        next += 1;
    }
}

/// The allocatable node ids a `Debug` rendering prints, read as it is
/// written rather than from a buffered copy. The derived form of
/// [`SemanticNodeId`] writes its name, then `(`, then its digits, each as
/// one write, so the reader matches whole writes and never scans text.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
struct PrintedNodeIds {
    stage: PrintedNodeIdStage,
    ids: SmallVec<[u64; 16]>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum PrintedNodeIdStage {
    #[default]
    Text,
    Name,
    Open,
}

#[cfg(any(test, feature = "test-support"))]
impl std::fmt::Write for PrintedNodeIds {
    fn write_str(&mut self, written: &str) -> std::fmt::Result {
        const NAME: &[u8] = b"SemanticNodeId";
        let bytes = written.as_bytes();
        let stage = self.stage;
        self.stage = PrintedNodeIdStage::Text;
        match stage {
            PrintedNodeIdStage::Name if bytes == b"(" => self.stage = PrintedNodeIdStage::Open,
            PrintedNodeIdStage::Open => {
                if let Ok(held) = written.parse::<u64>() {
                    if held < UNALLOCATABLE_ID_FLOOR {
                        self.ids.push(held);
                    }
                }
            }
            _ if bytes.len() == NAME.len() && bytes == NAME => {
                self.stage = PrintedNodeIdStage::Name;
            }
            _ => {}
        }
        Ok(())
    }
}

pub const NUM_SHARDS: usize = 16;
pub(super) const SHARD_MASK: u64 = (NUM_SHARDS as u64) - 1;

/// One entry in a fingerprint bucket: the shared payload `Arc` (the SAME
/// allocation stored in [`ArenaInner::nodes`]), its origin scope, and the
/// interned id. The payload and scope are retained so the bucket scan can
/// content-`Eq` a query against candidates without touching `inner`.
type NodeCandidate = (Weak<SemanticNodeData>, NodeScopeId, SemanticNodeId);

/// Per-shard dedup index. Keyed by the structural fingerprint of
/// `(payload, scope)`; each bucket holds the candidate nodes whose
/// fingerprint routed here. A lookup content-`Eq`s the query
/// `(payload, scope)` against the (usually single) candidate to confirm
/// identity — the fingerprint only narrows.
///
/// The bucket key is the fingerprint itself, which is already seeded
/// randomly per process ([`fingerprint_seed`]): workspace-derived names in
/// the payload feed it, so it must stay unpredictable (a HashDoS vector
/// otherwise), and keying the map by it keeps that property without hashing
/// it a second time ([`FingerprintKeyHasher`]).
#[derive(Default)]
pub(super) struct ShardIndex {
    index: HashMap<u64, SmallVec<[NodeCandidate; 1]>, FingerprintKeys>,
    /// The shard's table and spilled buckets, pinned while they live.
    storage: Option<verter_session_query::retention::granular::GranularPin>,
    /// The table bytes [`Self::storage`] currently counts.
    table_bytes: usize,
}

/// Granule a shard's index storage is charged in.
const SHARD_STORAGE_GRANULE: usize = 4 * 1024;

/// Granule the slot storage and payloads are charged in.
const ARENA_STORAGE_GRANULE: usize = 64 * 1024;

/// The heap one fingerprint bucket holds once it spills past its one inline
/// candidate.
fn bucket_heap_bytes(bucket: &SmallVec<[NodeCandidate; 1]>) -> usize {
    if bucket.spilled() {
        verter_session_query::retention::allocation::heap_block_bytes(
            bucket.capacity() * std::mem::size_of::<NodeCandidate>(),
        )
    } else {
        0
    }
}

/// The bytes one live chunk costs: its box, its four slot arrays and its
/// entry in the chunk table.
fn chunk_storage_bytes() -> usize {
    use verter_session_query::retention::allocation::{box_bytes, heap_block_bytes};
    box_bytes::<Chunk>()
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<Option<Arc<SemanticNodeData>>>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<Option<NodeScopeId>>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<usize>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<bool>())
        + std::mem::size_of::<Option<Box<Chunk>>>()
}

/// The bytes one live payload's `Arc` block costs. What the payload owns
/// beyond its inline record — its member arrays and text, often shared
/// between payloads (`PayloadArray::share`) — is charged through the
/// arena's shared-heap table ([`ArenaInner::charge_payload_heap`]).
fn payload_block_bytes() -> usize {
    verter_session_query::retention::allocation::arc_block_bytes(std::mem::size_of::<
        SemanticNodeData,
    >())
}

/// Builds [`FingerprintKeyHasher`]s for the dedup index.
#[derive(Default, Clone, Copy)]
pub(super) struct FingerprintKeys;

impl BuildHasher for FingerprintKeys {
    type Hasher = FingerprintKeyHasher;

    fn build_hasher(&self) -> FingerprintKeyHasher {
        FingerprintKeyHasher(0)
    }
}

/// Places a fingerprint in the dedup index without rehashing it. Every
/// fingerprint of one shard shares its low routing bits
/// ([`SHARD_MASK`]), so the high half is folded into the low one: the
/// table's position bits and its tag bits both stay seeded-random.
pub(super) struct FingerprintKeyHasher(u64);

impl Hasher for FingerprintKeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(byte);
        }
    }

    fn write_u64(&mut self, fingerprint: u64) {
        self.0 = fingerprint ^ (fingerprint >> 32);
    }
}

impl ShardIndex {
    /// Settle the shard's charge after its table or a bucket changed:
    /// `bucket_delta` is the change in spilled-bucket heap.
    fn settle(
        &mut self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
        bucket_delta: isize,
    ) {
        let table = self.index.capacity()
            * (std::mem::size_of::<(u64, SmallVec<[NodeCandidate; 1]>)>() + 1);
        let storage = self.storage.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                account,
                SHARD_STORAGE_GRANULE,
            )
        });
        if table > self.table_bytes {
            storage.add(table - self.table_bytes);
        } else {
            storage.sub(self.table_bytes - table);
        }
        self.table_bytes = table;
        match bucket_delta {
            delta if delta > 0 => storage.add(delta.unsigned_abs()),
            delta => storage.sub(delta.unsigned_abs()),
        }
    }

    /// Remove `id` from the bucket `fingerprint` names, dropping the bucket
    /// once empty, and settle the shard's charge.
    fn remove_candidate(
        &mut self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
        fingerprint: u64,
        id: SemanticNodeId,
    ) {
        let Some(bucket) = self.index.get_mut(&fingerprint) else {
            return;
        };
        let before = bucket_heap_bytes(bucket);
        bucket.retain(|(_, _, candidate)| *candidate != id);
        let after = if bucket.is_empty() {
            self.index.remove(&fingerprint);
            0
        } else {
            bucket_heap_bytes(bucket)
        };
        self.settle(account, after as isize - before as isize);
    }

    /// Record a freshly interned candidate and settle the shard's charge.
    fn insert_candidate(
        &mut self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
        fingerprint: u64,
        candidate: NodeCandidate,
    ) {
        let bucket = self.index.entry(fingerprint).or_default();
        let before = bucket_heap_bytes(bucket);
        bucket.push(candidate);
        let after = bucket_heap_bytes(bucket);
        self.settle(account, after as isize - before as isize);
    }

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
        mut is_live: impl FnMut(SemanticNodeId) -> bool,
    ) -> Option<SemanticNodeId> {
        let bucket = self.index.get(&fingerprint)?;
        bucket.iter().find_map(|(cand_data, cand_scope, cand_id)| {
            (cand_scope == scope
                && is_live(*cand_id)
                && cand_data
                    .upgrade()
                    .is_some_and(|value| value.as_ref() == data))
            .then_some(*cand_id)
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
    owners: Box<[usize]>,
    permanent: Box<[bool]>,
    /// Slots of this chunk still holding a payload; the chunk is dropped
    /// when it reaches zero.
    live: usize,
}

impl Chunk {
    fn empty() -> Self {
        Self {
            nodes: (0..CHUNK_LEN).map(|_| None).collect(),
            scopes: (0..CHUNK_LEN).map(|_| None).collect(),
            owners: vec![0; CHUNK_LEN].into_boxed_slice(),
            permanent: vec![false; CHUNK_LEN].into_boxed_slice(),
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

/// Empty window entries tolerated beyond the window's live chunks before
/// the window is compacted.
const CHUNK_WINDOW_SLACK: usize = 16;

/// Live chunks addressed by chunk number (`id >> CHUNK_BITS`).
///
/// Ids are monotonic, so recent chunks sit in a dense window addressed by
/// `chunk - base`: a lookup there is one bounds check, not a hash probe.
/// The window stays dense: once its empty entries outnumber its live chunks
/// (plus [`CHUNK_WINDOW_SLACK`]), the leading entries leave it, empty ones
/// dropped and live ones moved to `older`, an ordered map of the chunks
/// numbered below the window. The table's own storage therefore follows the
/// live chunk count, never the number of chunks ever allocated.
#[derive(Default)]
struct ChunkTable {
    /// Live chunks numbered below `base`.
    older: std::collections::BTreeMap<u64, Box<Chunk>>,
    /// The chunk number of `window[0]`.
    base: u64,
    window: Vec<Option<Box<Chunk>>>,
    /// Live chunks in `window`.
    window_live: usize,
}

impl ChunkTable {
    // Every node read resolves its chunk here, so the lookup is written as
    // one indexed match rather than a chain of option adaptors.
    #[inline]
    fn get(&self, key: &u64) -> Option<&Chunk> {
        let Some(offset) = key.checked_sub(self.base) else {
            return self.older.get(key).map(Box::as_ref);
        };
        match self.window.get(usize::try_from(offset).ok()?) {
            Some(Some(chunk)) => Some(chunk),
            _ => None,
        }
    }

    #[inline]
    fn get_mut(&mut self, key: &u64) -> Option<&mut Chunk> {
        let Some(offset) = key.checked_sub(self.base) else {
            return self.older.get_mut(key).map(Box::as_mut);
        };
        match self.window.get_mut(usize::try_from(offset).ok()?) {
            Some(Some(chunk)) => Some(chunk),
            _ => None,
        }
    }

    fn get_or_insert_empty(&mut self, key: u64) -> &mut Chunk {
        let Some(offset) = key.checked_sub(self.base) else {
            return self
                .older
                .entry(key)
                .or_insert_with(|| Box::new(Chunk::empty()));
        };
        let index = usize::try_from(offset).expect("chunk number fits the address space");
        if self.window.len() <= index {
            self.window.resize_with(index + 1, || None);
        }
        let slot = &mut self.window[index];
        if slot.is_none() {
            self.window_live += 1;
        }
        slot.get_or_insert_with(|| Box::new(Chunk::empty()))
    }

    fn remove(&mut self, key: &u64) {
        let Some(offset) = key.checked_sub(self.base) else {
            self.older.remove(key);
            return;
        };
        let Some(slot) = usize::try_from(offset)
            .ok()
            .and_then(|index| self.window.get_mut(index))
        else {
            return;
        };
        if slot.take().is_some() {
            self.window_live -= 1;
            self.compact_window();
        }
    }

    /// Move the window's leading entries out while it is sparse: an empty
    /// entry is dropped, a live chunk moves to `older`. One drain per
    /// compaction, and a compaction runs only once the empty entries exceed
    /// the live ones, so its cost is amortised over the releases that made
    /// the window sparse.
    fn compact_window(&mut self) {
        if self.window.len() <= 2 * self.window_live + CHUNK_WINDOW_SLACK {
            return;
        }
        let len = self.window.len();
        let mut leading = 0;
        let mut live_left = self.window_live;
        while len - leading > 2 * live_left + CHUNK_WINDOW_SLACK / 2 {
            if let Some(chunk) = self.window[leading].take() {
                self.older.insert(self.base + leading as u64, chunk);
                live_left -= 1;
            }
            leading += 1;
        }
        self.window.drain(..leading);
        self.base += leading as u64;
        self.window_live = live_left;
    }

    /// Live chunk numbers in ascending order.
    fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        let base = self.base;
        self.older.keys().copied().chain(
            self.window
                .iter()
                .enumerate()
                .filter(|(_, chunk)| chunk.is_some())
                .map(move |(index, _)| base + index as u64),
        )
    }

    fn values(&self) -> impl Iterator<Item = &Chunk> {
        self.older
            .values()
            .map(Box::as_ref)
            .chain(self.window.iter().filter_map(Option::as_deref))
    }

    fn len(&self) -> usize {
        self.older.len() + self.window_live
    }

    /// Entries the table holds, live or empty: what it costs to keep.
    #[cfg(test)]
    fn held_entries(&self) -> usize {
        self.older.len() + self.window.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Interior state of [`NodeArena`]. Held behind an `RwLock` so reads
/// (non-hot-path) are concurrent while the allocating intern-miss path
/// serializes on the writer.
#[derive(Default)]
pub(super) struct ArenaInner {
    /// Live chunks keyed by `id >> CHUNK_BITS`. A chunk is created by the first
    /// id allocated into it and dropped by the release of its last live slot.
    chunks: ChunkTable,
    /// The next id to hand out: every id below it was allocated exactly once.
    next_id: u64,
    /// Live slots across every chunk, maintained on every allocation and
    /// release so the live count is O(1).
    live: usize,
    /// The live chunks' slot storage, the live payloads' blocks and the heap
    /// they reach, pinned while they live.
    storage: Option<verter_session_query::retention::granular::GranularPin>,
    /// Owners of every shared allocation a live payload or scope reaches,
    /// keyed by address: a payload, or an enclosing shared allocation. An
    /// allocation is charged when it gains its first owner and released
    /// with its last, so one shared between many payloads
    /// (`PayloadArray::share`, an interned name) is charged once.
    shared_heap: rustc_hash::FxHashMap<usize, usize>,
    /// Payloads of released slots a reader still holds: their blocks and
    /// heap stay charged with graph storage, and their shared allocations
    /// keep their owner, until the arena next finds the reader gone.
    held: Vec<(Arc<SemanticNodeData>, NodeScopeId)>,
}

/// Bytes one bucket of the arena's shared-heap table holds: its key, its
/// owner count and its control byte.
const SHARED_HEAP_BUCKET_BYTES: usize = std::mem::size_of::<(usize, usize)>() + 1;

/// Charges the heap a payload gains as it becomes live, counting each shared
/// allocation once arena-wide.
struct ChargeHeap<'a> {
    shared: &'a mut rustc_hash::FxHashMap<usize, usize>,
    bytes: usize,
    /// The owned allocations the payload reaches: charged by their own
    /// cells, which the payload's handles keep alive.
    handles: verter_session_query::retention::allocation::AliasSet,
}

impl crate::semantic_query::node_payload_footprint::PayloadHeapVisitor for ChargeHeap<'_> {
    fn owned(&mut self, bytes: usize) {
        self.bytes += bytes;
    }

    fn shared(&mut self, key: usize, bytes: usize) -> bool {
        let owners = self.shared.entry(key).or_insert(0);
        *owners += 1;
        let first = *owners == 1;
        if first {
            self.bytes += bytes;
        }
        first
    }

    fn handle(
        &mut self,
        offer: &mut dyn FnMut(&mut verter_session_query::retention::allocation::AliasSet) -> usize,
    ) {
        let counted = offer(&mut self.handles);
        verter_debug_assert::verter_debug_assert_eq!(counted, 0, "an owned handle is collected");
    }
}

/// Releases the heap a payload held as its slot is freed: a shared
/// allocation is released with its last owner.
struct ReleaseHeap<'a> {
    shared: &'a mut rustc_hash::FxHashMap<usize, usize>,
    bytes: usize,
}

impl crate::semantic_query::node_payload_footprint::PayloadHeapVisitor for ReleaseHeap<'_> {
    fn owned(&mut self, bytes: usize) {
        self.bytes += bytes;
    }

    fn shared(&mut self, key: usize, bytes: usize) -> bool {
        let owners = self
            .shared
            .get_mut(&key)
            .expect("a live payload's shared allocation has an owner record");
        *owners -= 1;
        let last = *owners == 0;
        if last {
            self.shared.remove(&key);
            self.bytes += bytes;
        }
        last
    }

    /// An owned allocation releases its own charge with its last handle.
    fn handle(
        &mut self,
        _offer: &mut dyn FnMut(&mut verter_session_query::retention::allocation::AliasSet) -> usize,
    ) {
    }
}

impl ArenaInner {
    /// The shared-heap table's bucket bytes.
    fn shared_heap_table_bytes(&self) -> usize {
        self.shared_heap.capacity() * SHARED_HEAP_BUCKET_BYTES
    }

    /// The heap `payload` and `scope` reach that is not charged yet, now
    /// owned by one more live slot, plus the table growth recording it.
    fn charge_payload_heap(
        &mut self,
        payload: &SemanticNodeData,
        scope: &NodeScopeId,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> usize {
        use crate::semantic_query::node_payload_footprint::PayloadHeap as _;
        let table_before = self.shared_heap_table_bytes();
        let mut visitor = ChargeHeap {
            shared: &mut self.shared_heap,
            bytes: 0,
            handles: verter_session_query::retention::allocation::AliasSet::new(),
        };
        payload.visit_payload_heap(&mut visitor);
        scope.visit_payload_heap(&mut visitor);
        let bytes = visitor.bytes;
        // The payload's owned allocations are charged by their own cells —
        // pinned, like the graph storage that keeps them — and shared with
        // every other holder of the same allocation.
        visitor
            .handles
            .take_handles()
            .pin(verter_session_query::retention::owned::ChargeCtx::new(
                account,
            ));
        bytes + (self.shared_heap_table_bytes() - table_before)
    }

    /// The heap a freed slot's `payload` and `scope` held that no other
    /// live slot reaches, and the table backing given back with it.
    fn release_payload_heap(&mut self, payload: &SemanticNodeData, scope: &NodeScopeId) -> usize {
        use crate::semantic_query::node_payload_footprint::PayloadHeap as _;
        let table_before = self.shared_heap_table_bytes();
        let mut visitor = ReleaseHeap {
            shared: &mut self.shared_heap,
            bytes: 0,
        };
        payload.visit_payload_heap(&mut visitor);
        scope.visit_payload_heap(&mut visitor);
        let bytes = visitor.bytes;
        let len = self.shared_heap.len();
        if len == 0 {
            self.shared_heap = rustc_hash::FxHashMap::default();
        } else if len * 4 <= self.shared_heap.capacity() {
            self.shared_heap.shrink_to(len * 2);
        }
        bytes + (table_before - self.shared_heap_table_bytes())
    }

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

    /// Hand out the next id for `(payload, scope)`, charging its storage
    /// to `account`.
    fn allocate(
        &mut self,
        payload: Arc<SemanticNodeData>,
        scope: NodeScopeId,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> SemanticNodeId {
        self.release_unheld();
        let id = SemanticNodeId(self.next_id);
        self.next_id += 1;
        let storage = self.storage.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                account,
                ARENA_STORAGE_GRANULE,
            )
        });
        if self.chunks.get(&(id.0 >> CHUNK_BITS)).is_none() {
            storage.add(chunk_storage_bytes());
        }
        storage.add(payload_block_bytes());
        let heap = self.charge_payload_heap(&payload, &scope, account);
        self.storage
            .as_mut()
            .expect("the storage pin was created above")
            .add(heap);
        let chunk = self.chunks.get_or_insert_empty(id.0 >> CHUNK_BITS);
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
        let Some(payload) = chunk.nodes[index].take() else {
            return false;
        };
        let scope = chunk.scopes[index]
            .take()
            .expect("a live slot has its scope");
        chunk.owners[index] = 0;
        chunk.permanent[index] = false;
        chunk.live -= 1;
        self.live -= 1;
        let mut freed = if chunk.live == 0 {
            self.chunks.remove(&key);
            chunk_storage_bytes()
        } else {
            0
        };
        if Arc::strong_count(&payload) == 1 {
            freed += payload_block_bytes() + self.release_payload_heap(&payload, &scope);
        } else {
            // A reader still holds the payload: it stays allocated, so it
            // stays charged until the reader lets go.
            self.held.push((payload, scope));
        }
        if let Some(storage) = self.storage.as_mut() {
            storage.sub(freed);
        }
        self.release_unheld();
        true
    }

    /// Release every held payload its readers have let go of. Under the
    /// writer lock an unreferenced payload cannot gain a reader: the arena
    /// holds its last handle.
    fn release_unheld(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let mut freed = 0;
        let mut held = std::mem::take(&mut self.held);
        held.retain(|(payload, scope)| {
            if Arc::strong_count(payload) > 1 {
                return true;
            }
            freed += payload_block_bytes() + self.release_payload_heap(payload, scope);
            false
        });
        self.held = held;
        if let Some(storage) = self.storage.as_mut() {
            storage.sub(freed);
        }
    }

    /// Every live `(id, payload, scope)` in ascending id order.
    fn live_slots(&self) -> impl Iterator<Item = (u64, &Arc<SemanticNodeData>, &NodeScopeId)> {
        let keys: Vec<u64> = self.chunks.keys().collect();
        keys.into_iter().flat_map(move |key| {
            let chunk = self.chunks.get(&key).expect("listed chunk is live");
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

/// Structural fingerprint of `(data, scope)` — the 64-bit narrowing key for
/// the dedup bucket index. Hashes the FULL structural identity (payload
/// incl. spans, plus origin scope) through the node's own [`Hash`] impl, so
/// equal `(data, scope)` pairs always fingerprint equal and land in the
/// same bucket (Hash/Eq consistency is load-bearing for dedup). It only
/// narrows candidates; per-bucket content `Eq` is the identity authority,
/// so a fingerprint collision never aliases two distinct nodes.
///
/// It hashes with SipHash-1-3 under a per-process random key
/// ([`super::fingerprint_hasher`]), so the fingerprint of a given payload
/// is stable within a process run but unpredictable across runs: a
/// workspace-derived name carried in a payload cannot be chosen to force
/// fingerprint (and thus bucket) collisions — the HashDoS defense.
/// Fingerprints never cross a process or serialization boundary. The scope
/// enters as its own keyed digest ([`scope_digest`]), a function of the
/// scope's value alone, so equal pairs still fingerprint equal.
fn structural_fingerprint(data: &SemanticNodeData, scope: &NodeScopeId) -> u64 {
    let mut hasher = super::fingerprint_hasher::FingerprintHasher::new();
    data.hash(&mut hasher);
    hasher.write_u64(scope_digest(scope));
    hasher.finish()
}

/// The keyed fingerprint hash of a scope.
fn keyed_scope_hash(scope: &NodeScopeId) -> u64 {
    let mut hasher = super::fingerprint_hasher::FingerprintHasher::new();
    scope.hash(&mut hasher);
    hasher.finish()
}

/// The seeded hash of `scope`. Nodes are interned in runs under one
/// declaration scope, so the thread keeps the digest of the last file scope
/// it hashed, holding a weak view of that scope's canonical id so the
/// allocation that recognises it stays its own without keeping its text or
/// its charge past its owners: an equal scope behind another allocation is
/// hashed again, to the same digest.
fn scope_digest(scope: &NodeScopeId) -> u64 {
    type LastScope = (
        verter_session_query::retention::owned::WeakOwnedStr,
        verter_type_expr::TopLevelOwnerId,
        crate::semantic_query::HashValue,
        Option<u32>,
        u64,
    );
    std::thread_local! {
        static LAST_FILE_SCOPE: std::cell::RefCell<Option<LastScope>> =
            const { std::cell::RefCell::new(None) };
    }
    let NodeScopeId::File {
        canonical_id,
        owner,
        whole_hash,
        local_scope,
    } = scope
    else {
        return keyed_scope_hash(scope);
    };
    LAST_FILE_SCOPE.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((held, o, h, l, digest)) = last.as_ref() {
            if held.is(canonical_id) && o == owner && h == whole_hash && l == local_scope {
                return *digest;
            }
        }
        let digest = keyed_scope_hash(scope);
        *last = Some((
            verter_type_expr::OwnedStr::downgrade(canonical_id),
            *owner,
            *whole_hash,
            *local_scope,
            digest,
        ));
        digest
    })
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

    let names = |canonical: &str| canonical == canonical_id;
    let declared_in =
        |origin: &Option<verter_type_expr::OwnedStr>| origin.as_deref() == Some(canonical_id);
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
                    SignatureReturnCarrier::DeferredFlow(source)
                    | SignatureReturnCarrier::EvaluatedFlow(source) => {
                        names(&source.identity().anchor.canonical_id)
                    }
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
    pub(super) core: Arc<NodeArenaCore>,
    pub(super) provenance: Option<Arc<crate::engine_provenance::EngineProvenance>>,
}

pub(crate) struct NodeArenaCore {
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
    #[cfg(any(test, feature = "test-support"))]
    counted: bool,
    #[cfg(test)]
    race_hook: parking_lot::Mutex<Option<ArenaRaceHook>>,
    /// The ONE payload every released slot resolves to through [`Self::get`]
    /// — an `Opaque(Miss)` node ("this value's resolution answered
    /// nothing"), never in the dedup index. Mirrors the
    /// `SemanticGraphRead::node_data` fabrication for an unknown id, so a
    /// consumer still holding a released id reads an unresolved value.
    released_placeholder: Arc<SemanticNodeData>,
    /// The account the arena's storage is pinned against: the owning
    /// store's, bound before the first allocation, or the process-local one.
    retention: OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArenaRaceEvent {
    BeforeDedupLookup,
    BeforeInnerWrite,
    BeforeDedupRetain,
    BeforeRootRelease { blocked: bool },
    AfterRootRelease,
    BeforeShardStorageSnapshot,
}

#[cfg(test)]
type ArenaRaceHook = Arc<dyn Fn(ArenaRaceEvent) + Send + Sync>;

impl Default for NodeArena {
    fn default() -> Self {
        Self {
            core: Arc::new(NodeArenaCore {
                inner: parking_lot::RwLock::new(ArenaInner::default()),
                shards: std::array::from_fn(|_| parking_lot::Mutex::new(ShardIndex::default())),
                #[cfg(any(test, feature = "test-support"))]
                counted: false,
                #[cfg(test)]
                race_hook: Default::default(),
                released_placeholder: Arc::new(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                )),
                retention: OnceLock::new(),
            }),
            provenance: None,
        }
    }
}

impl std::ops::Deref for NodeArena {
    type Target = NodeArenaCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl NodeArenaCore {
    /// Charge the arena's storage to `account` — the owning store's. Bound
    /// once, before the first allocation; a later binding is ignored, so a
    /// charge is never moved between accounts.
    pub(super) fn bind_retention_account(
        &self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) {
        let _ = self.retention.set(Arc::clone(account));
    }

    /// The account the arena's storage is charged to.
    fn retention_account(&self) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.retention
            .get_or_init(verter_session_query::retention::SemanticRetentionAccount::process_local)
    }

    /// Bytes the arena's storage — its live chunks and payload blocks and
    /// its dedup shards — is charged.
    ///
    /// Interning takes a shard and then `inner`, so the slot figure is read
    /// in its own statement: its `inner` guard is gone before any shard is
    /// locked, and a snapshot never holds `inner` while waiting on a shard.
    pub(super) fn storage_charge_bytes(&self) -> usize {
        let slots = self
            .inner
            .read()
            .storage
            .as_ref()
            .map_or(0, |storage| storage.charged());
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::BeforeShardStorageSnapshot);
        slots
            + self
                .shards
                .iter()
                .map(|shard| {
                    shard
                        .lock()
                        .storage
                        .as_ref()
                        .map_or(0, |storage| storage.charged())
                })
                .sum::<usize>()
    }

    /// Bytes the slot storage and payload blocks alone are charged.
    #[cfg(test)]
    pub(super) fn slot_storage_charge_bytes(&self) -> usize {
        self.inner
            .read()
            .storage
            .as_ref()
            .map_or(0, |storage| storage.charged())
    }

    #[cfg(test)]
    fn race_event(&self, event: ArenaRaceEvent) {
        let hook = self.race_hook.lock().clone();
        if let Some(hook) = hook {
            hook(event);
        }
    }
    pub(super) fn ownership_counts(&self) -> super::observability::NodeOwnershipCounts {
        let inner = self.inner.read();
        let mut counts = super::observability::NodeOwnershipCounts::default();
        let mut owners = 0;
        for chunk in inner.chunks.values() {
            for (index, data) in chunk.nodes.iter().enumerate() {
                let Some(data) = data else {
                    continue;
                };
                counts.live_nodes += 1;
                owners += chunk.owners[index];
                counts.permanent_nodes += usize::from(chunk.permanent[index]);
                data.for_each_retained_child(|child| {
                    if child.0 < UNALLOCATABLE_ID_FLOOR
                        && matches!(inner.slot(child), Slot::Live(..))
                    {
                        counts.internal_edges += 1;
                    }
                });
            }
        }
        counts.roots = owners
            .checked_sub(counts.internal_edges)
            .expect("every live edge is counted");
        counts
    }
    pub(crate) fn is_live(&self, id: SemanticNodeId) -> bool {
        matches!(self.inner.read().slot(id), Slot::Live(..))
    }
    /// The payload of a live slot only; a released slot has none.
    pub(super) fn live_payload(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        match self.inner.read().slot(id) {
            Slot::Live(payload, _) => Some(Arc::clone(payload)),
            _ => None,
        }
    }
    pub(crate) fn counted(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.counted
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            false
        }
    }

    pub(super) fn retain(
        &self,
        ids: &[SemanticNodeId],
    ) -> Result<(), crate::semantic_query::QueryError> {
        let mut inner = self.inner.write();
        for id in ids {
            match inner.slot(*id) {
                Slot::Live(..) => {}
                Slot::Released => {
                    return Err(crate::semantic_query::QueryError::StaleSemanticOperand)
                }
                Slot::Unallocated => {
                    return Err(crate::semantic_query::QueryError::ForeignSemanticOperand)
                }
            }
        }
        for id in ids {
            inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap().owners
                [(id.0 & CHUNK_MASK) as usize] += 1;
        }
        Ok(())
    }

    /// Keep only the live ids of `ids` and retain each of them, under one
    /// hold of the arena: a tombstone or an unallocated ordinal owns no
    /// storage, so it is dropped rather than refused.
    pub(super) fn retain_live(&self, ids: &mut super::node_roots::RootIds) {
        let mut inner = self.inner.write();
        ids.retain(|id| matches!(inner.slot(*id), Slot::Live(..)));
        for id in ids.iter() {
            inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap().owners
                [(id.0 & CHUNK_MASK) as usize] += 1;
        }
    }

    pub(super) fn release_roots(&self, ids: &[SemanticNodeId]) {
        if !self.counted() {
            // Without counted release a node outlives its last owner, so a
            // release only returns each id's count.
            #[cfg(test)]
            self.race_event(ArenaRaceEvent::BeforeRootRelease {
                blocked: self.inner.try_write().is_none(),
            });
            let mut inner = self.inner.write();
            for id in ids {
                let Some(chunk) = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)) else {
                    continue;
                };
                let index = (id.0 & CHUNK_MASK) as usize;
                if chunk.nodes[index].is_none() {
                    continue;
                }
                assert!(chunk.owners[index] > 0, "live root released twice");
                chunk.owners[index] -= 1;
            }
            drop(inner);
            #[cfg(test)]
            self.race_event(ArenaRaceEvent::AfterRootRelease);
            return;
        }
        let mut pending = SmallVec::<[SemanticNodeId; 16]>::from_slice(ids);
        let mut retired = SmallVec::<[(SemanticNodeId, u64); 4]>::new();
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::BeforeRootRelease {
            blocked: self.inner.try_write().is_none(),
        });
        let mut inner = self.inner.write();
        while let Some(id) = pending.pop() {
            let Some(chunk) = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)) else {
                continue;
            };
            let index = (id.0 & CHUNK_MASK) as usize;
            if chunk.nodes[index].is_none() {
                continue;
            }
            assert!(chunk.owners[index] > 0, "live root released twice");
            chunk.owners[index] -= 1;
            if chunk.owners[index] != 0 || chunk.permanent[index] || !self.counted() {
                continue;
            }
            let data = chunk.nodes[index].as_ref().unwrap();
            data.for_each_retained_child(|child| {
                if child.0 < UNALLOCATABLE_ID_FLOOR {
                    pending.push(child);
                }
            });
            let fingerprint = structural_fingerprint(data, chunk.scopes[index].as_ref().unwrap());
            inner.release(id);
            retired.push((id, fingerprint));
        }
        drop(inner);
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::AfterRootRelease);
        // Intern always confirms slot liveness under `inner` after a weak
        // upgrade. Removing the index after the tombstone cannot resurrect it.
        for (id, fingerprint) in retired {
            self.shards[(fingerprint & SHARD_MASK) as usize]
                .lock()
                .remove_candidate(self.retention_account(), fingerprint, id);
        }
    }
}

impl NodeArena {
    /// Intern `data` with the `Global` scope tag. Helper intermediates and
    /// purely structural nodes use this path — most existing interning
    /// sites fall into this bucket.
    pub(super) fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.push_with_scope(data, NodeScopeId::Global)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Called by
    /// builders that know the declaration origin — `build_resolve_decl`,
    /// `build_typeof`, `build_instantiate`, etc.
    pub(super) fn push_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        let id = self.push_impl(data, scope);
        super::node_roots::NodeRootScope::capture_constructed(&self.core, id);
        id
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
        match self.try_intern_with_fingerprint(data, scope.clone(), fingerprint) {
            Ok(id) => id,
            Err(error) => self.push_impl(SemanticNodeData::Opaque(error), scope),
        }
    }

    fn try_intern_with_fingerprint(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        fingerprint: u64,
    ) -> Result<SemanticNodeId, crate::semantic_query::QueryError> {
        // Capture the variant bucket before moving `data` so the
        // contention instrumentation can bucket per-variant pushes.
        let discriminant = data.node_tag().bucket_index();
        let shard_idx = (fingerprint & SHARD_MASK) as usize;

        // Sharded dedup hot path. The fingerprint routes to its shard and
        // the bucket scan checks for an existing id; the miss path acquires
        // `inner.write()` briefly to push the new slot.
        let (id, is_miss) = {
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
            #[cfg(test)]
            self.race_event(ArenaRaceEvent::BeforeDedupLookup);
            let mut inner = self.write_for_intern();
            let hit = shard
                .lookup(fingerprint, &data, &scope, |id| {
                    matches!(inner.slot(id), Slot::Live(..))
                })
                .inspect(|id| {
                    #[cfg(test)]
                    self.race_event(ArenaRaceEvent::BeforeDedupRetain);
                    inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap().owners
                        [(id.0 & CHUNK_MASK) as usize] += 1;
                });
            drop(inner);
            if let Some(existing) = hit {
                (existing, false)
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
                let mut inner = self.write_for_intern();
                if let Some(existing) = shard.lookup(fingerprint, &data, &scope, |id| {
                    matches!(inner.slot(id), Slot::Live(..))
                }) {
                    // Another thread beat us to it.
                    inner
                        .chunks
                        .get_mut(&(existing.0 >> CHUNK_BITS))
                        .unwrap()
                        .owners[(existing.0 & CHUNK_MASK) as usize] += 1;
                    (existing, false)
                } else {
                    // The id this payload would take. Ids are monotonic and
                    // never reused (storage is chunked, not a vector), so the
                    // next id to hand out is the allocation point the
                    // acyclicity rule reads; a released child sits below it
                    // and is no forward reference.
                    let id = SemanticNodeId(inner.next_id);
                    let mut children = SmallVec::<[SemanticNodeId; 16]>::new();
                    data.for_each_retained_child(|child| {
                        if child.0 < UNALLOCATABLE_ID_FLOOR {
                            children.push(child);
                        }
                    });
                    if names_forward_child(&data, id) {
                        return Err(crate::semantic_query::QueryError::ForeignSemanticOperand);
                    }
                    if children
                        .iter()
                        .any(|child| !matches!(inner.slot(*child), Slot::Live(..)))
                    {
                        return Err(crate::semantic_query::QueryError::StaleSemanticOperand);
                    }
                    // ONE payload allocation, shared by refcount between the
                    // arena's slot storage and the dedup bucket — the payload
                    // is never deep-cloned into the index.
                    let payload = Arc::new(data);
                    for child in children {
                        inner
                            .chunks
                            .get_mut(&(child.0 >> CHUNK_BITS))
                            .unwrap()
                            .owners[(child.0 & CHUNK_MASK) as usize] += 1;
                    }
                    let id = inner.allocate(
                        Arc::clone(&payload),
                        scope.clone(),
                        self.retention_account(),
                    );
                    let chunk = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap();
                    let index = (id.0 & CHUNK_MASK) as usize;
                    chunk.owners[index] = 1;
                    chunk.permanent[index] = scope == NodeScopeId::Global
                        && matches!(&*payload, SemanticNodeData::Primitive(_));
                    drop(inner);
                    shard.insert_candidate(
                        self.retention_account(),
                        fingerprint,
                        (Arc::downgrade(&payload), scope, id),
                    );
                    (id, true)
                }
            }
        };

        if let Some(prov) = self.provenance.as_ref() {
            use std::sync::atomic::Ordering::Relaxed;
            prov.node_arena_pushes.fetch_add(1, Relaxed);
            if is_miss {
                prov.node_arena_intern_miss.fetch_add(1, Relaxed);
            }
            // In range by construction: the array is sized to the tag bound.
            prov.node_arena_pushes_per_discriminant[discriminant].fetch_add(1, Relaxed);
        }

        Ok(id)
    }

    /// Record acquisition time on every intern write, including dedup hits
    /// and writes that subsequently refuse an invalid operand.
    fn write_for_intern(&self) -> parking_lot::RwLockWriteGuard<'_, ArenaInner> {
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::BeforeInnerWrite);
        let start = self.provenance.as_ref().map(|prov| (prov, Instant::now()));
        let inner = self.inner.write();
        if let Some((prov, start)) = start {
            prov.node_arena_inner_write_wait_ns.fetch_add(
                start.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        inner
    }

    pub(super) fn try_intern_owned(
        &self,
        mut data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> Result<super::node_roots::NodeRoot, crate::semantic_query::QueryError> {
        if let SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(n)) = &mut data
        {
            if *n == 0.0 {
                *n = 0.0;
            }
        }
        let fingerprint = structural_fingerprint(&data, &scope);
        let id = self.try_intern_with_fingerprint(data, scope, fingerprint)?;
        Ok(super::node_roots::NodeRoot::adopt(
            Arc::clone(&self.core),
            id,
        ))
    }

    /// Intern `data` and hand back the count its intern took as an owned
    /// root.
    #[cfg(test)]
    pub(super) fn intern_owned(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> super::node_roots::NodeRoot {
        let id = self.push_impl(data, scope);
        super::node_roots::NodeRoot::adopt(Arc::clone(&self.core), id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn counted_for_tests() -> Self {
        let mut arena = Self::default();
        Arc::get_mut(&mut arena.core).unwrap().counted = true;
        arena
    }

    #[cfg(test)]
    pub(super) fn owner_count(&self, id: SemanticNodeId) -> usize {
        self.inner
            .read()
            .chunks
            .get(&(id.0 >> CHUNK_BITS))
            .map_or(0, |chunk| chunk.owners[(id.0 & CHUNK_MASK) as usize])
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
        assert!(
            !self.counted(),
            "counted destruction cannot coexist with legacy sweeps"
        );
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
                shard.remove_candidate(self.retention_account(), fingerprint, id);
            }
        }
        // Now drop the payloads (and any chunk that empties). A slot already
        // released by a concurrent call is skipped so `live` stays exact.
        let mut inner = self.inner.write();
        let mut ids: Vec<SemanticNodeId> = Vec::with_capacity(released.len());
        for (id, _) in released {
            let mut children = SmallVec::<[SemanticNodeId; 16]>::new();
            if let Slot::Live(data, _) = inner.slot(id) {
                data.for_each_retained_child(|child| children.push(child));
            }
            if inner.release(id) {
                ids.push(id);
                for child in children {
                    if let Some(chunk) = inner.chunks.get_mut(&(child.0 >> CHUNK_BITS)) {
                        let index = (child.0 & CHUNK_MASK) as usize;
                        if chunk.nodes[index].is_some() {
                            assert!(chunk.owners[index] > 0, "retained edge released twice");
                            chunk.owners[index] -= 1;
                        }
                    }
                }
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
            let mut bucket_delta = 0isize;
            shard.index.retain(|_fingerprint, bucket| {
                let before = bucket_heap_bytes(bucket);
                bucket.retain(|(_, scope, _)| match scope {
                    // Invariant: Global scope is never dropped on invalidation.
                    NodeScopeId::Global => true,
                    NodeScopeId::File {
                        canonical_id: c, ..
                    } => c.as_ref() != canonical_id,
                });
                let after = if bucket.is_empty() {
                    0
                } else {
                    bucket_heap_bytes(bucket)
                };
                bucket_delta += after as isize - before as isize;
                // Drop now-empty buckets so the fingerprint index stays dense.
                !bucket.is_empty()
            });
            if shard.storage.is_some() {
                shard.settle(self.retention_account(), bucket_delta);
            }
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
                        return cand_arc
                            .upgrade()
                            .is_some_and(|candidate| Arc::ptr_eq(&candidate, &arena_arc));
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
#[path = "arena_counted_tests.rs"]
mod counted_tests;

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
                name: verter_type_expr::OwnedStr::from("Tree"),
                args: [arg].into(),
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
                    canonical_id: verter_type_expr::OwnedStr::from("/declaring.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: verter_type_expr::OwnedStr::from("f"),
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
                        name: verter_type_expr::OwnedStr::from("T"),
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
                pending: Some(verter_session_query::retention::owned::Owned::new(
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
                    canonical_id: verter_type_expr::OwnedStr::from("/kept.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    offset: 0,
                    name: verter_type_expr::OwnedStr::from("C"),
                    outer_clauses: verter_type_expr::OwnedSlice::from([]),
                    own_arity: 0,
                    constructor_visibility: None,
                    prototype: Some(prototype),
                    object_literal: false,
                }),
                type_arguments: crate::semantic_query::PayloadArray::from([]),
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
            canonical_id: verter_type_expr::OwnedStr::from(canonical),
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
