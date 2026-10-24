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
//! **Lifetime is counted.** Every live slot carries an owner count: one
//! per counted child edge from a live parent payload plus one per root
//! (a [`super::node_roots::NodeRootSet`] operand, a construction owner's
//! intern count). The last release destroys the slot iteratively — its
//! children lose the edge it held and are destroyed in turn when that was
//! their last owner — so a node lives exactly as long as something that
//! can still read it. The twelve `Global` primitive vocabulary nodes are the
//! only permanent slots: the store's own hold on them, which a host close
//! gives up once its caches are released (`release_vocabulary`) and the next
//! intern re-establishes. Provenance and the per-node sidecars own nothing.
//! There is no canonical sweep: a document close evicts the CACHE owners
//! bound to the closed content (`release.rs`), and a node a caller still
//! holds stays readable until that caller drops it.
//!
//! **A store's teardown releases its graph at once.** While the owning
//! store drops its caches, their root releases are deferred
//! ([`ArenaTeardown`]); once every cache is gone, an arena no one else holds
//! drops whole — its storage charge, every payload and the dedup index go
//! together, with no per-node release — and an arena a reader still holds
//! releases the deferred roots as they would have released, so what that
//! reader can reach, and only that, stays live and charged.
//!
//! The id space is append-only — a `SemanticNodeId` is a raw `u64` index
//! with no generation tag, so an id is NEVER reused: a stale ordinal
//! resolves through [`NodeArena::get`] to one shared `Opaque(Miss)`
//! placeholder (an unresolved value, never an invalid-id fault) and
//! refuses every root acquisition as `StaleSemanticOperand`.
//!
//! **Sidecar cleanup is bounded by what it cleans.** A per-node sidecar
//! (structure bits, the member-ordinal index, origin edges, stable-key
//! classes) marks the slot it describes ([`NodeArenaCore::track_sidecar`]).
//! Destroying a marked slot appends its id to one pending log the store
//! drains into its sidecars ([`NodeArenaCore::drain_destroyed`]); an
//! unmarked slot is destroyed without a log entry, so the pending log never
//! holds more ids than the stale sidecar entries it removes.
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

/// One interned graph payload: the allocation every read of the node shares.
///
/// While its slot is live the payload's block and heap are charged with the
/// arena's storage. When the slot is released while a reader still holds the
/// payload, that charge passes to the payload itself ([`HeldRelease`]): the
/// drop of its last holder gives it back, in the same step that frees the
/// allocation, so a reader that outlives the slot — or the whole store —
/// keeps the payload charged exactly as long as it keeps it alive.
pub struct NodePayload {
    data: SemanticNodeData,
    release: OnceLock<Box<HeldRelease>>,
    /// The construction (by id, zero for the settled storage pin) this
    /// payload's block and heap were charged to when the arena allocated
    /// them: the payload's eventual release pays THAT owner, so an older
    /// allocation freed during a later construction never pays the open
    /// construction's ACTIVE charge.
    charge_owner: std::sync::atomic::AtomicU64,
}

/// What the last holder of a released payload gives back: its block and
/// heap, and its ownership of the shared allocations it reaches.
struct HeldRelease {
    charges: Arc<parking_lot::Mutex<ArenaCharges>>,
    scope: NodeScopeId,
    /// The freed payload's charging owner
    /// ([`NodePayload::charge_owner`]).
    owner: u64,
}

impl Drop for NodePayload {
    fn drop(&mut self) {
        if let Some(held) = self.release.take() {
            held.charges
                .lock()
                .release_payload(&self.data, &held.scope, held.owner);
        }
    }
}

/// Identity of the next construction ownership opened on any arena: unique
/// among every live scope, so a guard closes exactly its own.
fn next_construction_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// One construction a thread opened, stacked with the constructions its
/// thread encloses: the token names the arena — by its charge lock's
/// identity — and the construction, so an allocation on that thread charges
/// the construction ITS OWN producer opened, never whichever construction
/// any thread on the shared arena opened last.
#[derive(Clone, Copy)]
struct ConstructionToken {
    charges: usize,
    id: u64,
}

std::thread_local! {
    /// The constructions open on this thread, innermost last.
    static OPEN_CONSTRUCTIONS: std::cell::RefCell<Vec<ConstructionToken>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The construction the CURRENT thread opened on the arena whose charges
/// are identified by `charges`, innermost first.
fn current_thread_construction(charges: usize) -> Option<u64> {
    OPEN_CONSTRUCTIONS.with(|open| {
        open.borrow()
            .iter()
            .rev()
            .find(|token| token.charges == charges)
            .map(|token| token.id)
    })
}

/// Record `token`'s construction as open on this thread.
fn open_thread_construction(token: ConstructionToken) {
    OPEN_CONSTRUCTIONS.with(|open| open.borrow_mut().push(token));
}

/// Remove this thread's record of the construction `token` names, wherever
/// it sits in the thread's stack.
fn close_thread_construction(token: &ConstructionToken) {
    OPEN_CONSTRUCTIONS.with(|open| {
        open.borrow_mut()
            .retain(|held| held.charges != token.charges || held.id != token.id);
    });
}

/// Construction ownership over the arena's freshly allocated storage: the
/// payload blocks, their heap and the slot storage this arena allocates
/// while the guard is open — on the OPENING THREAD, whose allocations the
/// guard's construction owns — are charged as the opening computation's
/// IN-FLIGHT payload — [`ChargeClass::Active`], charged unconditionally —
/// instead of joining the settled storage pin, so a cold construction owns
/// its fresh graph bytes exactly once before its delivery settles them.
///
/// Dropping the guard settles the construction: its remaining charge hands
/// off ACTIVE→PINNED into the arena's storage pin, in one transition per
/// granule, and the owned payload cells it charged in flight settle with
/// it — so the bytes stay charged throughout, never released, never
/// doubled, however the producer ends (publication, caller handoff,
/// cancellation, failure or panic).
///
/// [`ChargeClass::Active`]: verter_session_query::retention::ChargeClass::Active
pub(crate) struct ConstructionChargeGuard {
    charges: Arc<parking_lot::Mutex<ArenaCharges>>,
    token: ConstructionToken,
}

impl Drop for ConstructionChargeGuard {
    fn drop(&mut self) {
        close_thread_construction(&self.token);
        let mut charges = self.charges.lock();
        let Some(open) = charges.construction.remove(&self.token.id) else {
            return;
        };
        let pin = charges.storage.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                &open.account,
                ARENA_STORAGE_GRANULE,
            )
        });
        open.charge.settle_into(pin);
        for handles in &open.handles {
            handles.hand_off_in_flight();
        }
    }
}

impl NodeArenaCore {
    /// Open a construction ownership over this arena's fresh allocations:
    /// while the returned guard is open, every payload block, payload heap
    /// and slot storage this arena allocates ON THIS THREAD is charged
    /// ACTIVE against `account` as the opening computation's in-flight
    /// payload, and the guard's drop settles the remainder into the arena's
    /// storage pin.
    pub(crate) fn open_construction_charge(
        &self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> ConstructionChargeGuard {
        let charges = self.inner.read().charges.clone();
        let token = ConstructionToken {
            charges: Arc::as_ptr(&charges) as usize,
            id: next_construction_id(),
        };
        charges.lock().construction.insert(
            token.id,
            OpenConstruction {
                account: Arc::clone(account),
                charge: verter_session_query::retention::granular::GranularInFlight::new(
                    ARENA_STORAGE_GRANULE,
                ),
                handles: Vec::new(),
            },
        );
        open_thread_construction(token);
        ConstructionChargeGuard { charges, token }
    }
}

/// A shared read of an interned payload: derefs to the payload, and every
/// clone shares the one allocation and its charge.
#[derive(Clone)]
pub struct NodeData(Arc<NodePayload>);

impl NodeData {
    /// A payload allocation no slot holds yet.
    pub(crate) fn new(data: SemanticNodeData) -> Self {
        Self(Arc::new(NodePayload {
            data,
            release: OnceLock::new(),
            charge_owner: std::sync::atomic::AtomicU64::new(0),
        }))
    }

    /// Whether both reads share one payload allocation.
    #[must_use]
    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }

    fn downgrade(&self) -> Weak<NodePayload> {
        Arc::downgrade(&self.0)
    }
}

impl std::ops::Deref for NodeData {
    type Target = SemanticNodeData;
    fn deref(&self) -> &SemanticNodeData {
        &self.0.data
    }
}

impl AsRef<SemanticNodeData> for NodeData {
    fn as_ref(&self) -> &SemanticNodeData {
        &self.0.data
    }
}

impl PartialEq for NodeData {
    /// Payload equality, as for the `Arc` it wraps.
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || self.0.data == other.0.data
    }
}

impl Eq for NodeData {}

impl std::fmt::Debug for NodeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.data.fmt(f)
    }
}

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
    // The walk and the printed form usually visit the operands in the same
    // order, and equal sequences are equal multisets: only a differing
    // order is sorted before the multiset comparison.
    if retained == expected {
        return;
    }
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
type NodeCandidate = (Weak<NodePayload>, NodeScopeId, SemanticNodeId);

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
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<Option<NodeData>>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<Option<NodeScopeId>>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<usize>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<bool>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<std::sync::atomic::AtomicBool>())
        + heap_block_bytes(CHUNK_LEN * std::mem::size_of::<u64>())
        + std::mem::size_of::<Option<Box<Chunk>>>()
}

/// The bytes one live payload's `Arc` block costs. What the payload owns
/// beyond its inline record — its member arrays and text, often shared
/// between payloads (`PayloadArray::share`) — is charged through the
/// arena's shared-heap table ([`ArenaInner::charge_payload_heap`]).
fn payload_block_bytes() -> usize {
    verter_session_query::retention::allocation::arc_block_bytes(std::mem::size_of::<NodePayload>())
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
                && cand_data.upgrade().is_some_and(|value| value.data == *data))
            .then_some(*cand_id)
        })
    }
}

/// Destroyed ids a per-node sidecar still describes, and the charge for
/// the log's backing. Only marked slots are logged, so the log never holds
/// more ids than the stale sidecar entries draining it removes.
#[derive(Default)]
pub(super) struct DestroyedLog {
    ids: Vec<SemanticNodeId>,
    storage: Option<verter_session_query::retention::granular::GranularPin>,
    /// The backing bytes [`Self::storage`] currently counts.
    charged: usize,
}

impl DestroyedLog {
    fn extend(
        &mut self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
        ids: &[SemanticNodeId],
    ) {
        self.ids.extend_from_slice(ids);
        self.settle(account);
    }

    /// Hand the log and its charge to cleanup. The batch owns its backing
    /// until cleanup drops it; taking it never leaves an uncharged buffer.
    fn drain(&mut self) -> Self {
        std::mem::take(self)
    }

    fn settle(&mut self, account: &Arc<verter_session_query::retention::SemanticRetentionAccount>) {
        let bytes = verter_session_query::retention::allocation::heap_block_bytes(
            self.ids.capacity() * std::mem::size_of::<SemanticNodeId>(),
        );
        let storage = self.storage.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                account,
                SHARD_STORAGE_GRANULE,
            )
        });
        if bytes > self.charged {
            storage.add(bytes - self.charged);
        } else {
            storage.sub(self.charged - bytes);
        }
        self.charged = bytes;
    }
}

impl std::ops::Deref for DestroyedLog {
    type Target = [SemanticNodeId];

    fn deref(&self) -> &Self::Target {
        &self.ids
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
    nodes: Box<[Option<NodeData>]>,
    scopes: Box<[Option<NodeScopeId>]>,
    owners: Box<[usize]>,
    permanent: Box<[bool]>,
    /// Slots a per-node sidecar describes: destroying one logs its id for
    /// sidecar cleanup. Atomic so a sidecar writer marks a slot under the
    /// arena's read lock.
    tracked: Box<[std::sync::atomic::AtomicBool]>,
    /// Each live slot's dedup fingerprint, recorded when it was interned:
    /// a release finds the slot's bucket without hashing its payload again.
    fingerprints: Box<[u64]>,
    /// The construction (zero for the settled storage pin) this chunk's
    /// storage was charged to when it was created: the chunk's drop pays
    /// that owner.
    charge_owner: u64,
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
            tracked: (0..CHUNK_LEN)
                .map(|_| std::sync::atomic::AtomicBool::new(false))
                .collect(),
            fingerprints: vec![0; CHUNK_LEN].into_boxed_slice(),
            charge_owner: 0,
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
    Live(&'a NodeData, &'a NodeScopeId),
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
    /// A boxed slice rather than a `Vec`: a read indexes it in place, with
    /// no slice conversion, which a debug build would otherwise call out
    /// for on every node read. `window[window_len..]` is empty headroom:
    /// the slice grows geometrically when a chunk lands past its end, so
    /// adding a chunk costs amortised constant time, and shrinks only once
    /// the headroom exceeds the entries in use several times over.
    window: Box<[Option<Box<Chunk>>]>,
    /// Entries of `window` in use: every live window chunk sits below it.
    window_len: usize,
    /// Live chunks in `window`.
    window_live: usize,
}

impl ChunkTable {
    // Every node read resolves its chunk here, so the lookup is written
    // with integer operators and a direct index — no adaptor or conversion
    // a debug build would call out for.
    #[inline(always)]
    fn get(&self, key: &u64) -> Option<&Chunk> {
        let key = *key;
        if key < self.base {
            return self.older.get(&key).map(Box::as_ref);
        }
        let offset = key - self.base;
        if offset >= self.window.len() as u64 {
            return None;
        }
        match &self.window[offset as usize] {
            Some(chunk) => Some(chunk),
            None => None,
        }
    }

    #[inline(always)]
    fn get_mut(&mut self, key: &u64) -> Option<&mut Chunk> {
        let key = *key;
        if key < self.base {
            return self.older.get_mut(&key).map(Box::as_mut);
        }
        let offset = key - self.base;
        if offset >= self.window.len() as u64 {
            return None;
        }
        match &mut self.window[offset as usize] {
            Some(chunk) => Some(chunk),
            None => None,
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
            let capacity = (index + 1)
                .max(self.window.len() * 2)
                .max(CHUNK_WINDOW_SLACK);
            self.resize_window(capacity);
        }
        self.window_len = self.window_len.max(index + 1);
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
    /// entry is dropped, a live chunk moves to `older`. One in-place
    /// rotation per compaction, and a compaction runs only once the empty
    /// entries exceed the live ones, so its cost is amortised over the
    /// releases that made the window sparse.
    fn compact_window(&mut self) {
        if self.window_len <= 2 * self.window_live + CHUNK_WINDOW_SLACK {
            return;
        }
        let len = self.window_len;
        let mut leading = 0;
        let mut live_left = self.window_live;
        while len - leading > 2 * live_left + CHUNK_WINDOW_SLACK / 2 {
            if let Some(chunk) = self.window[leading].take() {
                self.older.insert(self.base + leading as u64, chunk);
                live_left -= 1;
            }
            leading += 1;
        }
        // The leading entries are all empty now: rotating them to the end
        // turns them into headroom.
        self.window[..len].rotate_left(leading);
        self.window_len = len - leading;
        self.base += leading as u64;
        self.window_live = live_left;
        let wanted = 2 * self.window_len.max(CHUNK_WINDOW_SLACK);
        if self.window.len() > 2 * wanted {
            self.resize_window(wanted);
        }
    }

    /// Reallocate the window to `capacity` entries, keeping the ones in use.
    fn resize_window(&mut self, capacity: usize) {
        let mut resized = std::mem::take(&mut self.window).into_vec();
        resized.resize_with(capacity, || None);
        self.window = resized.into_boxed_slice();
    }

    /// Live chunk numbers in ascending order.
    fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        let base = self.base;
        self.older.keys().copied().chain(
            self.window[..self.window_len]
                .iter()
                .enumerate()
                .filter(|(_, chunk)| chunk.is_some())
                .map(move |(index, _)| base + index as u64),
        )
    }

    fn values(&self) -> impl Iterator<Item = &Chunk> {
        self.older.values().map(Box::as_ref).chain(
            self.window[..self.window_len]
                .iter()
                .filter_map(Option::as_deref),
        )
    }

    fn len(&self) -> usize {
        self.older.len() + self.window_live
    }

    /// Entries the table holds, live, empty or headroom: what it costs to
    /// keep.
    #[cfg(test)]
    fn held_entries(&self) -> usize {
        self.older.len() + self.window.len()
    }

    /// Entries the table addresses, live or empty, headroom excluded.
    #[cfg(test)]
    fn entries_in_use(&self) -> usize {
        self.older.len() + self.window_len
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
    /// The arena's charges. A leaf lock: taken under `inner`, or alone by
    /// the last holder of a released payload, never the other way round.
    charges: Arc<parking_lot::Mutex<ArenaCharges>>,
}

/// The charges of the arena's storage: the live chunks' slot storage, the
/// blocks of the payloads it keeps or a reader still holds, and the heap
/// they reach.
#[derive(Default)]
struct ArenaCharges {
    storage: Option<verter_session_query::retention::granular::GranularPin>,
    /// Owners of every shared allocation a charged payload or scope
    /// reaches, keyed by address: a payload, or an enclosing shared
    /// allocation. An allocation is charged when it gains its first owner
    /// and released with its last, so one shared between many payloads
    /// (`PayloadArray::share`) is charged once — by the owner that first
    /// charged it, which its last owner's release pays back.
    shared_heap: rustc_hash::FxHashMap<usize, SharedHeapCharge>,
    /// Every construction open on this arena, keyed by id: an allocation
    /// charges the construction the ALLOCATING THREAD's producer opened
    /// ([`current_thread_construction`]) — ACTIVE, unconditionally — by
    /// key, never by scanning, because a deep recursion keeps one
    /// construction open per level and the hot allocation path must not
    /// pay the depth. A release pays the owner recorded at charge time, so
    /// concurrent producers on the shared arena own their fresh storage
    /// separately and a freed allocation never pays another owner's
    /// charge. A construction's settlement hands its remainder to the
    /// storage pin (see [`ConstructionChargeGuard`]).
    construction: rustc_hash::FxHashMap<u64, OpenConstruction>,
}

/// One shared allocation in the arena's heap ledger: how many charged
/// payloads reach it, and the construction (zero for the settled storage
/// pin) that first charged it — the owner its last owner's release pays.
struct SharedHeapCharge {
    owners: usize,
    charged_to: u64,
}

/// One open construction ownership over this arena's freshly allocated
/// storage, keyed by its id in [`ArenaCharges::construction`].
struct OpenConstruction {
    account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    charge: verter_session_query::retention::granular::GranularInFlight,
    /// The owned payload cells this construction charged in flight —
    /// ACTIVE, by their own cells — settled with the construction at its
    /// guard's drop. Appended per fresh allocation without dedup: a cell
    /// charges once and the settle hands a still-ACTIVE charge off
    /// idempotently, so a repeated cell costs one no-op entry, never the
    /// quadratic scan a growing dedup would take.
    handles: Vec<verter_session_query::retention::owned::OwnedHandles>,
}

/// Bytes one bucket of the arena's shared-heap table holds: its key, its
/// owner record and its control byte.
const SHARED_HEAP_BUCKET_BYTES: usize = std::mem::size_of::<(usize, SharedHeapCharge)>() + 1;

/// Charges the heap a payload gains as it becomes live, counting each shared
/// allocation once arena-wide.
struct ChargeHeap<'a> {
    shared: &'a mut rustc_hash::FxHashMap<usize, SharedHeapCharge>,
    bytes: usize,
    /// The owner every allocation this measurement first-charges belongs
    /// to: the allocating thread's construction, zero for the settled
    /// storage pin.
    owner: u64,
    /// The owned allocations the payload reaches: charged by their own
    /// cells, which the payload's handles keep alive.
    handles: verter_session_query::retention::allocation::AliasSet,
}

impl crate::semantic_query::node_payload_footprint::PayloadHeapVisitor for ChargeHeap<'_> {
    fn owned(&mut self, bytes: usize) {
        self.bytes += bytes;
    }

    fn shared(&mut self, key: usize, bytes: usize) -> bool {
        let charge = self.shared.entry(key).or_insert_with(|| SharedHeapCharge {
            owners: 0,
            charged_to: self.owner,
        });
        charge.owners += 1;
        let first = charge.owners == 1;
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
/// allocation is released with its last owner, paying the owner that first
/// charged it.
struct ReleaseHeap<'a> {
    shared: &'a mut rustc_hash::FxHashMap<usize, SharedHeapCharge>,
    /// Bytes of allocations only this payload owned: paid by the payload's
    /// own charging owner.
    owned: usize,
    /// Bytes of shared allocations losing their last owner, each with the
    /// owner that first charged it.
    freed: Vec<(usize, u64)>,
}

impl crate::semantic_query::node_payload_footprint::PayloadHeapVisitor for ReleaseHeap<'_> {
    fn owned(&mut self, bytes: usize) {
        self.owned += bytes;
    }

    fn shared(&mut self, key: usize, bytes: usize) -> bool {
        let charge = self
            .shared
            .get_mut(&key)
            .expect("a live payload's shared allocation has an owner record");
        charge.owners -= 1;
        let last = charge.owners == 0;
        if last {
            let charged_to = charge.charged_to;
            self.shared.remove(&key);
            self.freed.push((bytes, charged_to));
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

impl ArenaCharges {
    /// The owner this thread's next allocation on the arena identified by
    /// `charges` belongs to: the construction this thread's producer opened
    /// while it is still open, or zero for the settled storage pin.
    fn current_owner(&self, charges: usize) -> u64 {
        current_thread_construction(charges)
            .filter(|id| self.construction.contains_key(id))
            .unwrap_or(0)
    }

    /// Charge `bytes` of freshly allocated storage to `owner` — an open
    /// construction's IN-FLIGHT charge, ACTIVE and unconditional, or the
    /// settled storage pin for zero.
    fn charge(
        &mut self,
        owner: u64,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
        bytes: usize,
    ) {
        if bytes == 0 {
            return;
        }
        if owner != 0 {
            if let Some(construction) = self.construction.get_mut(&owner) {
                construction.charge.add(account, bytes);
                return;
            }
        }
        self.storage
            .get_or_insert_with(|| {
                verter_session_query::retention::granular::GranularPin::new(
                    account,
                    ARENA_STORAGE_GRANULE,
                )
            })
            .add(bytes);
    }

    /// Release `bytes` charged to `owner`: the construction that charged
    /// them while it is still open, else — once its settlement folded its
    /// remainder into the storage pin — the pin. Zero names the pin.
    fn sub(&mut self, owner: u64, bytes: usize) {
        if bytes == 0 {
            return;
        }
        if owner != 0 {
            if let Some(construction) = self.construction.get_mut(&owner) {
                construction.charge.sub(bytes);
                return;
            }
        }
        if let Some(storage) = self.storage.as_mut() {
            storage.sub(bytes);
        }
    }

    fn charged(&self) -> usize {
        self.storage.as_ref().map_or(0, |storage| storage.charged())
    }

    /// Give back a payload no slot keeps any more and no reader holds.
    fn release_payload(&mut self, payload: &SemanticNodeData, scope: &NodeScopeId, owner: u64) {
        self.sub(owner, payload_block_bytes());
        self.release_payload_heap(payload, scope, owner);
    }

    /// The shared-heap table's bucket bytes.
    fn shared_heap_table_bytes(&self) -> usize {
        self.shared_heap.capacity() * SHARED_HEAP_BUCKET_BYTES
    }

    /// The heap `payload` and `scope` reach that is not charged yet, now
    /// owned by one more charged payload, plus the table growth recording
    /// it: charged to `owner`, with the table's own storage — the ledger
    /// itself, outliving any construction — charged to the settled pin.
    /// The payload's owned allocations are charged by their own cells:
    /// IN-FLIGHT under the opening construction — collected for its
    /// settlement — and pinned like the graph storage otherwise, shared
    /// with every other holder of the same allocation.
    fn charge_payload_heap(
        &mut self,
        owner: u64,
        payload: &SemanticNodeData,
        scope: &NodeScopeId,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) {
        use crate::semantic_query::node_payload_footprint::PayloadHeap as _;
        let table_before = self.shared_heap_table_bytes();
        let (bytes, handles) = {
            let mut visitor = ChargeHeap {
                shared: &mut self.shared_heap,
                bytes: 0,
                owner,
                handles: verter_session_query::retention::allocation::AliasSet::new(),
            };
            payload.visit_payload_heap(&mut visitor);
            scope.visit_payload_heap(&mut visitor);
            (visitor.bytes, visitor.handles.take_handles())
        };
        self.charge(owner, account, bytes);
        let growth = self.shared_heap_table_bytes() - table_before;
        self.charge(0, account, growth);
        if owner != 0 {
            if let Some(construction) = self.construction.get_mut(&owner) {
                handles.charge_in_flight(verter_session_query::retention::owned::ChargeCtx::new(
                    &construction.account,
                ));
                construction.handles.push(handles);
            }
        } else {
            handles.pin(verter_session_query::retention::owned::ChargeCtx::new(
                account,
            ));
        }
    }

    /// The heap a freed `payload` and `scope` held that no other charged
    /// payload reaches, paid by the owners that charged it, and the table
    /// backing given back with it.
    fn release_payload_heap(
        &mut self,
        payload: &SemanticNodeData,
        scope: &NodeScopeId,
        owner: u64,
    ) {
        use crate::semantic_query::node_payload_footprint::PayloadHeap as _;
        let table_before = self.shared_heap_table_bytes();
        let (owned, freed) = {
            let mut visitor = ReleaseHeap {
                shared: &mut self.shared_heap,
                owned: 0,
                freed: Vec::new(),
            };
            payload.visit_payload_heap(&mut visitor);
            scope.visit_payload_heap(&mut visitor);
            (visitor.owned, visitor.freed)
        };
        let len = self.shared_heap.len();
        if len == 0 {
            self.shared_heap = rustc_hash::FxHashMap::default();
        } else if len * 4 <= self.shared_heap.capacity() {
            self.shared_heap.shrink_to(len * 2);
        }
        let shrink = table_before - self.shared_heap_table_bytes();
        if shrink > 0 {
            if let Some(storage) = self.storage.as_mut() {
                storage.sub(shrink);
            }
        }
        self.sub(owner, owned);
        for (bytes, charged_to) in freed {
            self.sub(charged_to, bytes);
        }
    }
}

impl Drop for ArenaInner {
    /// A store's teardown keeps charged exactly the payloads a reader still
    /// holds: when one does, every slot is released as a close would release
    /// it, so the rest of the storage goes now and each held payload with its
    /// last holder. Without one, every charge goes with the storage.
    fn drop(&mut self) {
        if !self
            .live_slots()
            .any(|(_, payload, _)| Arc::strong_count(&payload.0) > 1)
        {
            return;
        }
        let ids: Vec<u64> = self.live_slots().map(|(id, _, _)| id).collect();
        for id in ids {
            self.release(SemanticNodeId(id));
        }
    }
}

impl ArenaInner {
    #[inline(always)]
    fn slot(&self, id: SemanticNodeId) -> Slot<'_> {
        if id.0 >= self.next_id {
            return Slot::Unallocated;
        }
        let Some(chunk) = self.chunks.get(&(id.0 >> CHUNK_BITS)) else {
            return Slot::Released;
        };
        let index = (id.0 & CHUNK_MASK) as usize;
        // Nested matches, not a matched pair: a debug build would build the
        // pair on every node read.
        match &chunk.nodes[index] {
            Some(payload) => match &chunk.scopes[index] {
                Some(scope) => Slot::Live(payload, scope),
                None => Slot::Released,
            },
            None => Slot::Released,
        }
    }

    /// Hand out the next id for `(payload, scope)`, charging its storage
    /// to `account` — the block, its heap and any fresh chunk to the
    /// construction this thread's producer opened (ACTIVE), or the settled
    /// storage pin, with the owner recorded on the allocation so its
    /// release pays exactly it.
    fn allocate(
        &mut self,
        payload: NodeData,
        scope: NodeScopeId,
        fingerprint: u64,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> SemanticNodeId {
        let id = SemanticNodeId(self.next_id);
        self.next_id += 1;
        let charges_identity = Arc::as_ptr(&self.charges) as usize;
        let owner = {
            let mut charges = self.charges.lock();
            let owner = charges.current_owner(charges_identity);
            if self.chunks.get(&(id.0 >> CHUNK_BITS)).is_none() {
                charges.charge(owner, account, chunk_storage_bytes());
            }
            charges.charge(owner, account, payload_block_bytes());
            charges.charge_payload_heap(owner, &payload, &scope, account);
            payload
                .0
                .charge_owner
                .store(owner, std::sync::atomic::Ordering::Relaxed);
            owner
        };
        let chunk = self.chunks.get_or_insert_empty(id.0 >> CHUNK_BITS);
        let index = (id.0 & CHUNK_MASK) as usize;
        // A chunk with no live slot is the one this allocation just created:
        // it carries the owner its storage charged.
        if chunk.live == 0 {
            chunk.charge_owner = owner;
        }
        chunk.nodes[index] = Some(payload);
        chunk.scopes[index] = Some(scope);
        chunk.fingerprints[index] = fingerprint;
        chunk.live += 1;
        self.live += 1;
        id
    }

    /// Release `id`'s slot; `Some(tracked)` when it held a payload, with
    /// whether a sidecar describes it. Drops the chunk when that was its
    /// last live slot.
    fn release(&mut self, id: SemanticNodeId) -> Option<bool> {
        let key = id.0 >> CHUNK_BITS;
        let chunk = self.chunks.get_mut(&key)?;
        let index = (id.0 & CHUNK_MASK) as usize;
        let payload = chunk.nodes[index].take()?;
        #[cfg(test)]
        SLOT_RELEASES.with(|releases| releases.set(releases.get() + 1));
        let scope = chunk.scopes[index]
            .take()
            .expect("a live slot has its scope");
        chunk.owners[index] = 0;
        chunk.permanent[index] = false;
        let tracked = chunk.tracked[index].swap(false, std::sync::atomic::Ordering::Relaxed);
        chunk.live -= 1;
        self.live -= 1;
        if chunk.live == 0 {
            let charge_owner = chunk.charge_owner;
            self.chunks.remove(&key);
            self.charges.lock().sub(charge_owner, chunk_storage_bytes());
        }
        let payload_owner = payload
            .0
            .charge_owner
            .load(std::sync::atomic::Ordering::Relaxed);
        match Arc::try_unwrap(payload.0) {
            // The slot held the last handle: the payload goes now.
            Ok(payload) => {
                self.charges
                    .lock()
                    .release_payload(&payload.data, &scope, payload_owner)
            }
            // A reader still holds the payload: it stays allocated, so it
            // stays charged until its last holder lets go.
            Err(payload) => {
                let _ = payload.release.set(Box::new(HeldRelease {
                    charges: Arc::clone(&self.charges),
                    scope,
                    owner: payload_owner,
                }));
            }
        }
        Some(tracked)
    }

    /// Every live `(id, payload, scope)` in ascending id order.
    fn live_slots(&self) -> impl Iterator<Item = (u64, &NodeData, &NodeScopeId)> {
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
/// it hashed, naming that scope's canonical id by its allocation's
/// identity stamp: the stamp is never reused and keeps nothing allocated,
/// so the cache outlives no owner of the text. An equal scope behind
/// another allocation is hashed again, to the same digest.
fn scope_digest(scope: &NodeScopeId) -> u64 {
    type LastScope = (
        std::num::NonZeroU64,
        verter_type_expr::TopLevelOwnerId,
        crate::semantic_query::HashValue,
        Option<crate::semantic_query::NodeLocalScopeId>,
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
    let identity = verter_type_expr::OwnedStr::identity(canonical_id);
    LAST_FILE_SCOPE.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((held, o, h, l, digest)) = last.as_ref() {
            if *held == identity && o == owner && h == whole_hash && l == local_scope {
                return *digest;
            }
        }
        let digest = keyed_scope_hash(scope);
        *last = Some((identity, *owner, *whole_hash, *local_scope, digest));
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
/// rule nor the child walk reaches it, yet no lookup reaches it again (the
/// reload mints a new hash → a new identity → a new node). A close
/// therefore evicts the caches holding such a node, or each closed content
/// version would leave one behind. The match carries no wildcard: a new
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
    /// Destroyed ids a per-node sidecar still describes, waiting for the
    /// owning store to drop their sidecar entries (see the module docs,
    /// **Sidecar cleanup**), with the log's backing charge.
    destroyed: parking_lot::Mutex<DestroyedLog>,
    /// The owning store's sidecars, drained by the release that grows
    /// [`Self::destroyed`] past its threshold.
    destroyed_sink: OnceLock<super::release::SidecarSink>,
    #[cfg(test)]
    race_hook: parking_lot::Mutex<Option<ArenaRaceHook>>,
    /// The ONE payload every released slot resolves to through [`Self::get`]
    /// — an `Opaque(Miss)` node ("this value's resolution answered
    /// nothing"), never in the dedup index. Mirrors the
    /// `SemanticGraphRead::node_data` fabrication for an unknown id, so a
    /// consumer still holding a released id reads an unresolved value.
    released_placeholder: NodeData,
    /// The account the arena's storage is pinned against: the owning
    /// store's, bound before the first allocation, or the process-local one.
    retention: OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>,
    /// Whether the owning store is tearing down: set from the start of its
    /// drop to the end ([`ArenaTeardown`]). The lock-free check of every
    /// root release; [`Self::deferred_releases`] is its authority.
    tearing_down: std::sync::atomic::AtomicBool,
    /// The root releases deferred while the owning store tears down, in
    /// release order; `None` outside a teardown.
    deferred_releases: parking_lot::Mutex<Option<Vec<SemanticNodeId>>>,
}

#[cfg(test)]
thread_local! {
    /// Slots this thread released one by one.
    static SLOT_RELEASES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Slots this thread has released one by one so far.
#[cfg(test)]
pub(super) fn slot_releases_for_tests() -> usize {
    SLOT_RELEASES.with(std::cell::Cell::get)
}

/// The owning store's teardown of its arena: [`Self::begin`] at the start
/// of the store's drop, this guard's own drop after every other field of the
/// store has dropped. Between the two, the store's caches release their
/// roots into a deferred list instead of releasing node by node. At the end,
/// an arena only this guard still holds drops whole; one a reader still
/// holds releases the deferred roots in order, so the graph that reader can
/// reach is exactly the graph a node-by-node teardown leaves.
pub(super) struct ArenaTeardown(Arc<NodeArenaCore>);

impl ArenaTeardown {
    pub(super) fn of(arena: &NodeArena) -> Self {
        Self(Arc::clone(&arena.core))
    }

    /// Start deferring root releases.
    pub(super) fn begin(&self) {
        *self.0.deferred_releases.lock() = Some(Vec::new());
        self.0
            .tearing_down
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Drop for ArenaTeardown {
    fn drop(&mut self) {
        let deferred = {
            let mut deferred = self.0.deferred_releases.lock();
            self.0
                .tearing_down
                .store(false, std::sync::atomic::Ordering::Release);
            deferred.take()
        };
        // A reader that still holds the arena keeps reading it: release
        // what the caches held, as they would have. With no reader, the
        // arena drops with this guard, and everything with it.
        if Arc::strong_count(&self.0) > 1 {
            if let Some(ids) = deferred {
                self.0.release_roots(&ids);
            }
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArenaRaceEvent {
    BeforeDedupLookup,
    BeforeInnerWrite,
    BeforeDedupRetain,
    BeforeRootRelease {
        blocked: bool,
    },
    AfterRootRelease,
    BeforeShardStorageSnapshot,
    /// A destroyed-log drain found a sidecar lock held, while it still
    /// holds the sidecar locks it did acquire.
    SidecarDrainBlocked,
}

#[cfg(test)]
pub(super) type ArenaRaceHook = Arc<dyn Fn(ArenaRaceEvent) + Send + Sync>;

impl Default for NodeArena {
    fn default() -> Self {
        Self {
            core: Arc::new(NodeArenaCore {
                inner: parking_lot::RwLock::new(ArenaInner::default()),
                shards: std::array::from_fn(|_| parking_lot::Mutex::new(ShardIndex::default())),
                destroyed: Default::default(),
                destroyed_sink: OnceLock::new(),
                #[cfg(test)]
                race_hook: Default::default(),
                released_placeholder: NodeData::new(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::Miss,
                )),
                retention: OnceLock::new(),
                tearing_down: std::sync::atomic::AtomicBool::new(false),
                deferred_releases: parking_lot::Mutex::new(None),
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
        let slots = self.inner.read().charges.lock().charged();
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
        self.inner.read().charges.lock().charged()
    }

    #[cfg(test)]
    pub(super) fn set_race_hook(&self, hook: Option<ArenaRaceHook>) {
        *self.race_hook.lock() = hook;
    }

    #[cfg(test)]
    pub(super) fn race_event(&self, event: ArenaRaceEvent) {
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
    /// The payload of a live slot only; a released slot has none.
    pub(super) fn live_payload(&self, id: SemanticNodeId) -> Option<NodeData> {
        match self.inner.read().slot(id) {
            Slot::Live(payload, _) => Some(payload.clone()),
            _ => None,
        }
    }
    /// Mark `id` as described by a per-node sidecar entry the caller has
    /// just written, so its destruction is logged for cleanup. `false` when
    /// the slot is no longer live: the caller removes the entry it wrote,
    /// since no destruction will ever report it. An id from
    /// [`UNALLOCATABLE_ID_FLOOR`] up names no slot and is never destroyed.
    pub(super) fn track_sidecar(&self, id: SemanticNodeId) -> bool {
        if id.0 >= UNALLOCATABLE_ID_FLOOR {
            return true;
        }
        let inner = self.inner.read();
        if !matches!(inner.slot(id), Slot::Live(..)) {
            return false;
        }
        inner
            .chunks
            .get(&(id.0 >> CHUNK_BITS))
            .expect("a live slot has its chunk")
            .tracked[(id.0 & CHUNK_MASK) as usize]
            .store(true, std::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Whether `root` is content of the closed `canonical_id`: a node
    /// interned in its file scope, one binding its content identity
    /// ([`payload_binds_canonical`]), or one reaching such a node through its
    /// retained children. A close evicts the cache owners of exactly these
    /// nodes; nothing here releases a slot. `memo` carries the verdicts of
    /// one close across calls, so each live node is classified once; a
    /// destroyed or never-allocated id is not closed content. The walk is
    /// iterative, so a deep DAG costs heap, not stack.
    pub(super) fn reaches_canonical_content(
        &self,
        canonical_id: &str,
        root: SemanticNodeId,
        memo: &mut rustc_hash::FxHashMap<u64, bool>,
    ) -> bool {
        if root.0 >= UNALLOCATABLE_ID_FLOOR {
            return false;
        }
        if let Some(known) = memo.get(&root.0) {
            return *known;
        }
        let inner = self.inner.read();
        let mut stack = vec![(root, false)];
        while let Some((id, expanded)) = stack.pop() {
            if memo.contains_key(&id.0) {
                continue;
            }
            let Slot::Live(payload, scope) = inner.slot(id) else {
                memo.insert(id.0, false);
                continue;
            };
            if expanded {
                let mut reaches = false;
                payload.for_each_retained_child(|child| {
                    reaches |= memo.get(&child.0).copied().unwrap_or(false);
                });
                memo.insert(id.0, reaches);
                continue;
            }
            let direct = match scope {
                NodeScopeId::File {
                    canonical_id: c, ..
                } if c.as_ref() == canonical_id => true,
                _ => payload_binds_canonical(payload, canonical_id),
            };
            if direct {
                memo.insert(id.0, true);
                continue;
            }
            stack.push((id, true));
            payload.for_each_retained_child(|child| {
                if child.0 < UNALLOCATABLE_ID_FLOOR && !memo.contains_key(&child.0) {
                    stack.push((child, false));
                }
            });
        }
        memo.get(&root.0).copied().unwrap_or(false)
    }

    /// Take the destroyed ids whose sidecar entries the store must drop.
    pub(super) fn drain_destroyed(&self) -> DestroyedLog {
        self.destroyed.lock().drain()
    }

    /// Settle the destroyed-log drain a release left to the holder of a
    /// sidecar lock (see [`super::release::SidecarMutex`]).
    pub(super) fn settle_destroyed_log(&self) {
        if let Some(sink) = self.destroyed_sink.get() {
            sink.settle_owed(self);
        }
    }

    /// Bind the owning store's sidecars; the first store bound keeps them.
    pub(super) fn bind_destroyed_sink(&self, sink: super::release::SidecarSink) {
        let _ = self.destroyed_sink.set(sink);
    }

    /// Destroyed ids waiting for sidecar cleanup.
    pub(super) fn pending_destroyed(&self) -> usize {
        self.destroyed.lock().ids.len()
    }

    /// Give back the table of every dedup shard that indexes no node, and
    /// its charge. Returns the tables released.
    ///
    /// A shard's table keeps the capacity its busiest moment needed after
    /// its candidates are removed, so a released workload would otherwise
    /// leave one charged granule per shard it ever touched. A shard that
    /// still indexes a live node keeps its table: dropping it would free
    /// nothing a reader could not reach again.
    pub(super) fn release_idle_dedup_tables(&self) -> usize {
        let mut released = 0;
        for shard in &self.shards {
            let mut shard = shard.lock();
            if shard.index.is_empty() && shard.index.capacity() > 0 {
                shard.index = HashMap::with_hasher(FingerprintKeys);
                shard.settle(self.retention_account(), 0);
                released += 1;
            }
        }
        released
    }

    pub(super) fn retain(
        &self,
        ids: &[SemanticNodeId],
    ) -> Result<(), crate::semantic_query::QueryError> {
        let mut inner = self.inner.write();
        // Index loops: every rooted delivery retains here, and a debug
        // build runs them without an iterator call per id.
        let mut index = 0;
        while index < ids.len() {
            match inner.slot(ids[index]) {
                Slot::Live(..) => {}
                Slot::Released => {
                    return Err(crate::semantic_query::QueryError::StaleSemanticOperand)
                }
                Slot::Unallocated => {
                    return Err(crate::semantic_query::QueryError::ForeignSemanticOperand)
                }
            }
            index += 1;
        }
        let mut index = 0;
        while index < ids.len() {
            let id = ids[index];
            match inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)) {
                Some(chunk) => chunk.owners[(id.0 & CHUNK_MASK) as usize] += 1,
                None => unreachable!("a live slot's chunk is held"),
            }
            index += 1;
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

    /// Give up the store's hold on its primitive vocabulary: every
    /// permanent slot becomes an ordinary counted node, destroyed now when
    /// nothing else owns it and with its last owner otherwise. A host close
    /// calls this once its caches have released their owners, so the first
    /// workload's vocabulary is not left behind as residue; the next intern
    /// of a vocabulary node makes it permanent again. Returns the slots
    /// unpinned.
    pub(super) fn release_vocabulary(&self) -> usize {
        let mut held = Vec::new();
        {
            let mut inner = self.inner.write();
            let keys: Vec<u64> = inner.chunks.keys().collect();
            for key in keys {
                let chunk = inner.chunks.get_mut(&key).expect("a listed chunk is live");
                for index in 0..CHUNK_LEN {
                    if chunk.permanent[index] {
                        chunk.permanent[index] = false;
                        // Take one count so the release below runs the
                        // ordinary last-owner path, whoever else holds it.
                        chunk.owners[index] += 1;
                        held.push(SemanticNodeId((key << CHUNK_BITS) | index as u64));
                    }
                }
            }
        }
        self.release_roots(&held);
        held.len()
    }

    pub(super) fn release_roots(&self, ids: &[SemanticNodeId]) {
        if self.tearing_down.load(std::sync::atomic::Ordering::Acquire) {
            if let Some(deferred) = self.deferred_releases.lock().as_mut() {
                deferred.extend_from_slice(ids);
                return;
            }
        }
        // Most releases only drop a count; the stack of dying nodes' children
        // and the retired list allocate only once a node reaches zero.
        let mut pending: Vec<SemanticNodeId> = Vec::new();
        let mut retired: Vec<(SemanticNodeId, u64, bool)> = Vec::new();
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::BeforeRootRelease {
            blocked: self.inner.try_write().is_none(),
        });
        let mut inner = self.inner.write();
        // Last id first, each dying node's descendants before the next id:
        // the order of one stack seeded with `ids`.
        for &id in ids.iter().rev() {
            Self::release_root_locked(&mut inner, id, &mut pending, &mut retired);
            while let Some(child) = pending.pop() {
                Self::release_root_locked(&mut inner, child, &mut pending, &mut retired);
            }
        }
        drop(inner);
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::AfterRootRelease);
        if retired.is_empty() {
            return;
        }
        // Intern always confirms slot liveness under `inner` after a weak
        // upgrade. Removing the index after the tombstone cannot resurrect it.
        let mut logged = Vec::new();
        for (id, fingerprint, tracked) in retired {
            self.shards[(fingerprint & SHARD_MASK) as usize]
                .lock()
                .remove_candidate(self.retention_account(), fingerprint, id);
            if tracked {
                logged.push(id);
            }
        }
        if !logged.is_empty() {
            let over_threshold = {
                let mut log = self.destroyed.lock();
                log.extend(self.retention_account(), &logged);
                log.ids.len() > super::release::DESTROYED_DRAIN_THRESHOLD
            };
            if over_threshold {
                if let Some(sink) = self.destroyed_sink.get() {
                    sink.settle(self);
                }
            }
        }
    }

    /// Drop one count of `id` under the held arena lock. A node whose last
    /// count goes, and is not permanent, is released: its allocatable
    /// children go on `pending` and it joins `retired` with its
    /// fingerprint and whether a sidecar tracks it.
    fn release_root_locked(
        inner: &mut ArenaInner,
        id: SemanticNodeId,
        pending: &mut Vec<SemanticNodeId>,
        retired: &mut Vec<(SemanticNodeId, u64, bool)>,
    ) {
        let Some(chunk) = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)) else {
            return;
        };
        let index = (id.0 & CHUNK_MASK) as usize;
        if chunk.nodes[index].is_none() {
            return;
        }
        assert!(chunk.owners[index] > 0, "live root released twice");
        chunk.owners[index] -= 1;
        if chunk.owners[index] != 0 || chunk.permanent[index] {
            return;
        }
        let data = chunk.nodes[index].as_ref().unwrap();
        data.for_each_retained_child(|child| {
            if child.0 < UNALLOCATABLE_ID_FLOOR {
                pending.push(child);
            }
        });
        let fingerprint = chunk.fingerprints[index];
        let tracked = inner
            .release(id)
            .expect("the slot was live under the held lock");
        retired.push((id, fingerprint, tracked));
    }
}

impl NodeArena {
    /// Intern `data` with the `Global` scope tag. Helper intermediates and
    /// purely structural nodes use this path — most existing interning
    /// sites fall into this bucket.
    #[cfg(test)]
    pub(super) fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.push_with_scope::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            data,
            NodeScopeId::Global,
        )
    }

    /// Intern `data` and record `scope` in the origin sidecar. Called by
    /// builders that know the declaration origin — `build_resolve_decl`,
    /// `build_typeof`, `build_instantiate`, etc.
    ///
    /// `M` is the capture execution of the unit interning: only a captured
    /// one times or counts its shard-lock acquisitions.
    pub(super) fn push_with_scope<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        let _ = observation;
        let id = self.push_impl::<M>(observation, data, scope);
        super::node_roots::NodeRootScope::capture_constructed(&self.core, id);
        id
    }

    fn push_impl<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        mut data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        let _ = observation;
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
        self.intern_with_fingerprint::<M>(observation, data, scope, fingerprint)
    }

    /// Core intern path. `fingerprint` narrows to a shard + bucket; the
    /// per-bucket content `Eq` over `(data, scope)` is the identity
    /// authority. Split out from [`push_impl`] so tests can force a
    /// fingerprint — driving two distinct payloads into one bucket — and
    /// assert the content-`Eq` split (collision safety).
    fn intern_with_fingerprint<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        data: SemanticNodeData,
        scope: NodeScopeId,
        fingerprint: u64,
    ) -> SemanticNodeId {
        let _ = observation;
        match self.try_intern_with_fingerprint_in::<M>(
            observation,
            data,
            scope.clone(),
            fingerprint,
        ) {
            Ok(id) => id,
            Err(error) => self.push_impl::<M>(observation, SemanticNodeData::Opaque(error), scope),
        }
    }

    /// Explicit bucket selection for arena collision/release fixtures.
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "Fixture seam for explicit fingerprint configurations"
    )]
    fn try_intern_with_fingerprint(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        fingerprint: u64,
    ) -> Result<SemanticNodeId, crate::semantic_query::QueryError> {
        self.try_intern_with_fingerprint_in::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            data,
            scope,
            fingerprint,
        )
    }

    fn try_intern_with_fingerprint_in<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        data: SemanticNodeData,
        scope: NodeScopeId,
        fingerprint: u64,
    ) -> Result<SemanticNodeId, crate::semantic_query::QueryError> {
        let _ = observation;
        // Capture the variant bucket before moving `data` so the
        // contention instrumentation can bucket per-variant pushes.
        #[cfg(feature = "semantic-observe")]
        let discriminant = if super::records_telemetry::<M>() {
            data.node_tag().bucket_index()
        } else {
            0
        };
        let shard_idx = (fingerprint & SHARD_MASK) as usize;
        // The global primitive vocabulary is permanent while the store
        // holds it; an intern after a close re-establishes the hold
        // (`release_vocabulary`).
        let vocabulary =
            matches!(data, SemanticNodeData::Primitive(_)) && scope == NodeScopeId::Global;

        // Sharded dedup hot path. The fingerprint routes to its shard and
        // the bucket scan checks for an existing id; the miss path acquires
        // `inner.write()` briefly to push the new slot.
        let (id, is_miss) = {
            let timing_on = crate::request_observers::capture_timing_enabled::<M>(observation);
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
            crate::request_observers::record_node_arena_lock_acquisition::<M>(
                observation,
                lock_wait,
            );
            #[cfg(test)]
            self.race_event(ArenaRaceEvent::BeforeDedupLookup);
            let mut inner = self.write_for_intern::<M>();
            let hit = shard
                .lookup(fingerprint, &data, &scope, |id| {
                    matches!(inner.slot(id), Slot::Live(..))
                })
                .inspect(|id| {
                    #[cfg(test)]
                    self.race_event(ArenaRaceEvent::BeforeDedupRetain);
                    let chunk = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap();
                    let index = (id.0 & CHUNK_MASK) as usize;
                    chunk.owners[index] += 1;
                    chunk.permanent[index] |= vocabulary;
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
                crate::request_observers::record_node_arena_lock_acquisition::<M>(
                    observation,
                    lock_wait,
                );
                let mut inner = self.write_for_intern::<M>();
                if let Some(existing) = shard.lookup(fingerprint, &data, &scope, |id| {
                    matches!(inner.slot(id), Slot::Live(..))
                }) {
                    // Another thread beat us to it.
                    let chunk = inner.chunks.get_mut(&(existing.0 >> CHUNK_BITS)).unwrap();
                    let index = (existing.0 & CHUNK_MASK) as usize;
                    chunk.owners[index] += 1;
                    chunk.permanent[index] |= vocabulary;
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
                    let payload = NodeData::new(data);
                    for child in children {
                        inner
                            .chunks
                            .get_mut(&(child.0 >> CHUNK_BITS))
                            .unwrap()
                            .owners[(child.0 & CHUNK_MASK) as usize] += 1;
                    }
                    let id = inner.allocate(
                        payload.clone(),
                        scope.clone(),
                        fingerprint,
                        self.retention_account(),
                    );
                    let chunk = inner.chunks.get_mut(&(id.0 >> CHUNK_BITS)).unwrap();
                    let index = (id.0 & CHUNK_MASK) as usize;
                    chunk.owners[index] = 1;
                    chunk.permanent[index] = vocabulary;
                    drop(inner);
                    shard.insert_candidate(
                        self.retention_account(),
                        fingerprint,
                        (payload.downgrade(), scope, id),
                    );
                    (id, true)
                }
            }
        };

        #[cfg(feature = "semantic-observe")]
        if super::records_telemetry::<M>() {
            if let Some(prov) = self.provenance.as_ref() {
                use std::sync::atomic::Ordering::Relaxed;
                prov.node_arena_pushes.fetch_add(1, Relaxed);
                if is_miss {
                    prov.node_arena_intern_miss.fetch_add(1, Relaxed);
                }
                // In range by construction: the array is sized to the tag bound.
                prov.node_arena_pushes_per_discriminant[discriminant].fetch_add(1, Relaxed);
            }
        }
        #[cfg(not(feature = "semantic-observe"))]
        let _ = is_miss;

        Ok(id)
    }

    /// Record acquisition time on every intern write, including dedup hits
    /// and writes that subsequently refuse an invalid operand. The clock is
    /// read only under `semantic-observe`.
    fn write_for_intern<M: verter_audit::observe::ObserveExec>(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, ArenaInner> {
        let _ = std::marker::PhantomData::<M>;
        #[cfg(test)]
        self.race_event(ArenaRaceEvent::BeforeInnerWrite);
        #[cfg(feature = "semantic-observe")]
        let start = if super::records_telemetry::<M>() {
            self.provenance.as_ref().map(|prov| (prov, Instant::now()))
        } else {
            None
        };
        let inner = self.inner.write();
        #[cfg(feature = "semantic-observe")]
        if let Some((prov, start)) = start {
            prov.node_arena_inner_write_wait_ns.fetch_add(
                start.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        inner
    }

    pub(super) fn try_intern_owned<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        mut data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> Result<super::node_roots::NodeRoot, crate::semantic_query::QueryError> {
        let _ = observation;
        if let SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(n)) = &mut data
        {
            if *n == 0.0 {
                *n = 0.0;
            }
        }
        let fingerprint = structural_fingerprint(&data, &scope);
        let id = self.try_intern_with_fingerprint_in::<M>(observation, data, scope, fingerprint)?;
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
        let id = self.push_impl::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            data,
            scope,
        );
        super::node_roots::NodeRoot::adopt(Arc::clone(&self.core), id)
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
    /// out; the shared `Opaque(Miss)` placeholder for a DESTROYED id; the
    /// interned payload otherwise.
    pub(super) fn get(&self, id: SemanticNodeId) -> Option<NodeData> {
        let inner = self.inner.read();
        match inner.slot(id) {
            Slot::Live(payload, _) => Some(payload.clone()),
            Slot::Released => Some(self.released_placeholder.clone()),
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
    /// canonical's content generation. Payloads are dropped only when their
    /// last owner releases them (see the module docs, **Lifetime**).
    ///
    /// Touches every shard mutex once. Each shard's retain walk is
    /// O(shard size). When `node_arena_lock_acquisitions` is wired
    /// into the audit context, each shard lock acquisition is recorded.
    pub(super) fn invalidate_for_canonical<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        canonical_id: &str,
    ) {
        let _ = observation;
        let timing_on = crate::request_observers::capture_timing_enabled::<M>(observation);
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
            crate::request_observers::record_node_arena_lock_acquisition::<M>(
                observation,
                lock_wait,
            );
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
            Slot::Live(arc, _) => arc.clone(),
            _ => return false,
        };
        for shard in self.shards.iter() {
            let shard = shard.lock();
            for bucket in shard.index.values() {
                for (cand_arc, _scope, cand_id) in bucket.iter() {
                    if *cand_id == id {
                        return cand_arc
                            .upgrade()
                            .is_some_and(|candidate| Arc::ptr_eq(&candidate, &arena_arc.0));
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
    /// monotonic counter, not a slot count: after destroyed nodes dropped
    /// whole chunks the storage is smaller than the ids handed out, a
    /// payload over a node interned before it is still interned as itself,
    /// and one naming an id the arena has not handed out is refused.
    /// Discriminating: read against the storage size, the rule would refuse
    /// every payload over a live node whose id exceeds the slots stored.
    #[test]
    fn the_acyclicity_rule_reads_the_next_id_not_the_storage_size() {
        use crate::semantic_query::{LiteralValue, QueryError};
        let arena = NodeArena::default();
        let churned: Vec<_> = (0..(CHUNK_LEN * 3 + 8))
            .map(|n| {
                arena.intern_owned(
                    SemanticNodeData::Literal(LiteralValue::Number(n as f64)),
                    file_scope("/closed.ts"),
                )
            })
            .collect();
        let kept = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(-1.0)),
            file_scope("/kept.ts"),
        );
        drop(churned);
        assert!(
            (kept.id().0 as usize) >= arena.storage_slots(),
            "fixture: the kept node's id is past the slots still stored ({} vs {})",
            kept.id().0,
            arena.storage_slots()
        );
        let over_kept = arena.intern_owned(
            SemanticNodeData::Array {
                element: kept.id(),
                readonly: false,
            },
            NodeScopeId::Global,
        );
        assert!(
            matches!(
                arena.get(over_kept.id()).as_deref(),
                Some(SemanticNodeData::Array { .. })
            ),
            "a payload over a node interned before it is interned as itself"
        );
        let not_handed_out = SemanticNodeId(arena.len() as u64 + 5);
        let refused = arena.intern_owned(
            SemanticNodeData::Array {
                element: not_handed_out,
                readonly: false,
            },
            NodeScopeId::Global,
        );
        assert!(
            matches!(
                arena.get(refused.id()).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::ForeignSemanticOperand))
            ),
            "a payload naming an id not handed out yet is a forward reference"
        );
    }

    /// Whether `id` is content of the closed `canonical` as a close sees it.
    fn closed(arena: &NodeArena, canonical: &str, id: SemanticNodeId) -> bool {
        arena.reaches_canonical_content(canonical, id, &mut Default::default())
    }

    /// A recursive reference is interned unscoped and carries its type
    /// arguments inside an `Opaque` payload the semantic child walk skips.
    /// A close classifies the reference over a closed document's argument as
    /// closed content, so the caches holding it are evicted with the
    /// document; a reference over another document's argument is not.
    /// Discriminating: without the retained-child arm for the arguments the
    /// reference stays cached, one per content version of the closed
    /// document.
    #[test]
    fn a_close_classifies_the_recursive_references_over_its_nodes() {
        use crate::semantic_query::{LiteralValue, QueryError};
        let arena = NodeArena::default();
        let closed_node = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let recursive_over = |arg: SemanticNodeId| {
            arena.intern_owned(
                SemanticNodeData::Opaque(QueryError::RecursiveRef {
                    name: verter_type_expr::OwnedStr::from("Tree"),
                    args: [arg].into(),
                }),
                NodeScopeId::Global,
            )
        };
        let over_closed = recursive_over(closed_node.id());
        let over_kept = recursive_over(kept.id());
        assert!(closed(&arena, "/closed.ts", closed_node.id()));
        assert!(
            closed(&arena, "/closed.ts", over_closed.id()),
            "the reference over the closed document's node is its content"
        );
        assert!(!closed(&arena, "/closed.ts", kept.id()));
        assert!(
            !closed(&arena, "/closed.ts", over_kept.id()),
            "a reference over another document's node is not"
        );
    }

    /// A generic callable declared in one document and instantiated with a
    /// parameter type from another is interned unscoped as a sealed
    /// `DeferredCallable` whose served position stays in its declaring
    /// document. A close of the document the substituted type belongs to
    /// classifies the callable over it (through a parameter or a binder
    /// bound) and a parent naming the callable as closed content; a
    /// callable over another document's type is not.
    /// Discriminating: `for_each_child` answers `Sealed` for this variant
    /// without visiting its children, and the declaring canonical names
    /// neither closed type, so only the retained-child walk reaches them.
    #[test]
    fn a_close_classifies_the_sealed_callables_over_its_nodes() {
        use crate::semantic_query::{
            DeferredCallable, FunctionParam, LiteralValue, SignatureKind, SignatureNodeOccurrence,
            SignatureReturnCarrier, TypeParamDecl,
        };
        use verter_type_expr::facts::{
            FlowFunctionReturnIdentity, FunctionPartIdentity, FunctionReturnSource,
        };
        use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

        let arena = NodeArena::default();
        let closed_node = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let binder = arena.intern_owned(
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
            arena.intern_owned(
                SemanticNodeData::DeferredCallable(DeferredCallable::from_parts_for_tests(
                    SignatureKind::Call,
                    Arc::from([FunctionParam::synthetic(
                        Some(Arc::from("x")),
                        param,
                        false,
                        false,
                    )]),
                    Arc::from([TypeParamDecl {
                        name: verter_type_expr::OwnedStr::from("T"),
                        param: binder.id(),
                        constraint,
                        default: None,
                        original: None,
                        is_const: false,
                    }]),
                    occurrence.clone(),
                    SignatureReturnCarrier::Function(FunctionReturnSource::Absent),
                )),
                NodeScopeId::Global,
            )
        };
        let over_closed_param = callable(closed_node.id(), None);
        let over_closed_bound = callable(kept.id(), Some(closed_node.id()));
        let over_kept = callable(kept.id(), None);
        let parent = arena.intern_owned(
            SemanticNodeData::Array {
                element: over_closed_param.id(),
                readonly: false,
            },
            NodeScopeId::Global,
        );

        assert!(
            closed(&arena, "/closed.ts", over_closed_param.id()),
            "a callable over the closed parameter type is closed content"
        );
        assert!(
            closed(&arena, "/closed.ts", over_closed_bound.id()),
            "a callable whose binder bound is closed is closed content"
        );
        assert!(
            closed(&arena, "/closed.ts", parent.id()),
            "a parent of a closed callable is closed content"
        );
        assert!(!closed(&arena, "/closed.ts", over_kept.id()));
        assert!(!closed(&arena, "/closed.ts", binder.id()));
    }

    /// Substituting a binder through a deferred conditional appends the
    /// `(param, arg)` pair to its pending frame whether or not a branch
    /// mentions the binder, so the frame can name a closed binder while the
    /// check, extends and branches are all another document's. A close of
    /// the binder's document classifies that conditional as its content;
    /// one whose frame names only other nodes is not.
    /// Discriminating: the semantic child walk visits a pending frame's
    /// arguments but never its parameters.
    #[test]
    fn a_close_classifies_the_conditionals_whose_pending_frame_binds_its_nodes() {
        use crate::semantic_query::{ConditionalPendingSubstitution, LiteralValue};

        let arena = NodeArena::default();
        let closed_node = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let bound = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(3.0)),
            file_scope("/kept.ts"),
        );
        let conditional = |param: SemanticNodeId| {
            arena.intern_owned(
                SemanticNodeData::Conditional {
                    check: kept.id(),
                    extends: kept.id(),
                    true_branch_ref: kept.id(),
                    false_branch_ref: kept.id(),
                    distributive: false,
                    pending: Some(verter_session_query::retention::owned::Owned::new(
                        ConditionalPendingSubstitution::empty().append_both(param, bound.id()),
                    )),
                },
                NodeScopeId::Global,
            )
        };
        let binds_closed = conditional(closed_node.id());
        let binds_kept = conditional(kept.id());

        assert!(
            closed(&arena, "/closed.ts", binds_closed.id()),
            "a conditional whose pending frame binds a closed node is closed content"
        );
        assert!(!closed(&arena, "/closed.ts", binds_kept.id()));
    }

    /// A class expression instance declared in a kept document whose
    /// recorded prototype is a closed document's node is closed content,
    /// and one whose prototype is kept is not. Discriminating: the semantic
    /// child walk treats the prototype as a derived record beside the
    /// instance, not a part of it, so only the retained-child walk reaches
    /// it.
    #[test]
    fn a_close_classifies_the_class_expressions_whose_prototype_is_its_node() {
        use crate::semantic_query::{ClassExpressionIdentity, LiteralValue};

        let arena = NodeArena::default();
        let closed_node = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            file_scope("/closed.ts"),
        );
        let kept = arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::Number(2.0)),
            file_scope("/kept.ts"),
        );
        let instance = |prototype: SemanticNodeId| {
            arena.intern_owned(
                SemanticNodeData::ClassExpressionInstance {
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
                    surface: kept.id(),
                },
                NodeScopeId::Global,
            )
        };
        let over_closed = instance(closed_node.id());
        let over_kept = instance(kept.id());

        assert!(
            closed(&arena, "/closed.ts", over_closed.id()),
            "an instance whose prototype is a closed node is closed content"
        );
        assert!(!closed(&arena, "/closed.ts", over_kept.id()));
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

    /// A released node leaves the dedup bucket it was interned into, even
    /// when that bucket's key is not its payload's structural fingerprint:
    /// the release uses the fingerprint recorded at intern rather than
    /// hashing the payload again, so no stale candidate survives it.
    #[test]
    fn released_node_leaves_the_bucket_it_was_interned_into() {
        let arena = NodeArena::default();
        let payload =
            SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String("forced".into()));
        // The fingerprint hasher is keyed per process, so a fixed forced
        // fingerprint shares the payload's shard in some runs. Flipping the
        // lowest shard bit of the payload's own fingerprint never does.
        let forced_fp = structural_fingerprint(&payload, &NodeScopeId::Global) ^ 1;
        assert_ne!(
            forced_fp & SHARD_MASK,
            structural_fingerprint(&payload, &NodeScopeId::Global) & SHARD_MASK,
            "the forced bucket must sit in another shard than the payload's own"
        );
        let id = arena
            .try_intern_with_fingerprint(payload, NodeScopeId::Global, forced_fp)
            .expect("a fresh literal interns");
        let holds_candidate = |arena: &NodeArena| {
            arena.shards.iter().any(|shard| {
                shard
                    .lock()
                    .index
                    .values()
                    .any(|bucket| bucket.iter().any(|(_, _, candidate)| *candidate == id))
            })
        };
        assert!(holds_candidate(&arena));
        arena.release_roots(&[id]);
        assert!(!arena.is_live(id));
        assert!(
            !holds_candidate(&arena),
            "the released node's candidate must leave its interned bucket"
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

        let id_a = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            a.clone(),
            NodeScopeId::Global,
            forced_fp,
        );
        let id_b = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            b.clone(),
            NodeScopeId::Global,
            forced_fp,
        );
        assert_ne!(
            id_a, id_b,
            "distinct payloads sharing one fingerprint bucket must get distinct ids (no aliasing)",
        );

        // Re-intern of each SAME payload into the SAME (collided) bucket
        // still dedups to its own id.
        let id_a2 = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            a,
            NodeScopeId::Global,
            forced_fp,
        );
        let id_b2 = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            b,
            NodeScopeId::Global,
            forced_fp,
        );
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

        let id_global = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            payload.clone(),
            NodeScopeId::Global,
            forced_fp,
        );
        let id_file = arena.intern_with_fingerprint::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            payload,
            file_scope("/w/a.ts"),
            forced_fp,
        );
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

    /// Destroying every node of a chunk drops the chunk, so the storage the
    /// arena holds follows the live set, not the ids ever handed out. Ids
    /// keep counting up and a destroyed id reads as released either way.
    ///
    /// Discriminating: with dense `Vec` storage, `storage_slots` would grow
    /// with every cycle exactly like `len` does here.
    #[test]
    fn released_chunks_are_dropped_while_ids_keep_counting() {
        let arena = NodeArena::default();
        let pinned = arena.push(SemanticNodeData::Primitive(PrimitiveKind::String));
        let per_cycle = CHUNK_LEN + 3;
        let mut last: Option<SemanticNodeId> = None;
        for cycle in 0..40u64 {
            let canonical = "/w/churn.ts";
            let minted: Vec<_> = (0..per_cycle)
                .map(|n| {
                    arena.intern_owned(
                        SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(
                            (cycle * per_cycle as u64 + n as u64) as f64,
                        )),
                        file_scope(canonical),
                    )
                })
                .collect();
            let first = minted[0].id();
            let newest = minted.last().map(super::super::node_roots::NodeRoot::id);
            assert_eq!(arena.live_len(), 1 + per_cycle);
            drop(minted);
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
                assert!(first.0 > previous.0, "cycle {cycle}: ids keep counting up");
            }
            last = newest;
            assert!(
                NodeData::ptr_eq(&arena.get(first).unwrap(), &arena.released_placeholder),
                "cycle {cycle}: a destroyed id reads as the placeholder even after its chunk is gone"
            );
            assert!(!arena.is_live(first));
            assert!(arena.scope(first).is_none());
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
